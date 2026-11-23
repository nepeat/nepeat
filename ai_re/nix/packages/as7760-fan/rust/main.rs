// SPDX-License-Identifier: GPL-2.0-only

//! Edge-Core AS7760 fan CPLD (I2C2 -> PCA9548 0x76 ch0 -> 0x66): six dual-rotor
//! fans and one shared PWM, as hwmon. The Rust port of ../src/as7760_fan.c:
//! same module, driver and hwmon device name ("as7760_fan"), same attributes
//! (pwm1, fan1..12_input/_fault/_label), values, errors and register
//! accesses. Probe only reads; a duty write follows the vendor's 0x33=0,
//! 0x11=duty sequence, verifies it and falls back to full speed if it cannot.
//!
//! Kernel interfaces without a Rust API in 7.3 are wrapped in `hwmon` (hwmon
//! registration) and two raw calls (SMBus byte read/write); the
//! i2c_check_functionality() test is the C sliver in shim.c. Unsafe inventory:
//! ../../kernel-rust-ppc64be/README.md.

use core::ffi::{c_void, CStr};
use kernel::ffi::{c_char, c_int, c_long};
use core::ptr::{self, NonNull};
use kernel::{bindings, device::Core, i2c, new_mutex, of, prelude::*, sync::aref::ARef, sync::Mutex};

mod logic;
use logic::{duty_from_pwm, fault, probe_duty_ok, pwm_from_duty, rpm, tach_reg, CHANNELS, DUTY_MAX};
use logic::{REG_DUTY, REG_PRESENT, REG_PRE_WRITE};
use logic::{EIO as E_IO, EOPNOTSUPP as E_OPNOTSUPP};

kernel::of_device_table! {
    OF_TABLE,
    <As7760Fan as i2c::Driver>::IdInfo,
    [(of::DeviceId::new(c"accton,as7760-fan"), ())]
}

// improper_ctypes: i2c_client holds a zero-sized lockdep_map without
// CONFIG_LOCKDEP; only a pointer to it crosses here.
#[allow(improper_ctypes)]
unsafe extern "C" {
    fn as7760_fan_smbus_byte_data_ok(client: *const bindings::i2c_client) -> bool;
}

/// What the hwmon callbacks reach through drvdata.
#[pin_data]
struct Chip {
    client: ARef<i2c::I2cClient>,
    /// Serialises register sequences, as the C driver's mutex.
    #[pin]
    lock: Mutex<()>,
}

fn smbus_read(client: *const bindings::i2c_client, reg: u8) -> Result<u8, i32> {
    // SAFETY: client points to a live, bound i2c_client (callers hold an ARef
    // or the probe's borrow); the call does its own locking.
    let r = unsafe { bindings::i2c_smbus_read_byte_data(client, reg) };
    if r < 0 { Err(-r) } else { Ok(r as u8) }
}

fn smbus_write(client: *const bindings::i2c_client, reg: u8, val: u8) -> Result<(), i32> {
    // SAFETY: as in smbus_read.
    let r = unsafe { bindings::i2c_smbus_write_byte_data(client, reg, val) };
    if r < 0 { Err(-r) } else { Ok(()) }
}

fn raw_client(c: &i2c::I2cClient) -> *const bindings::i2c_client {
    // I2cClient is #[repr(transparent)] over Opaque<i2c_client>: same address.
    ptr::from_ref(c).cast()
}

impl Chip {
    fn raw(&self) -> *const bindings::i2c_client {
        raw_client(&self.client)
    }

    /// fan_read() of the C driver.
    fn read(&self, ty: u32, attr: u32, channel: c_int) -> Result<i64, i32> {
        let _g = self.lock.lock();
        if ty == bindings::hwmon_sensor_types_hwmon_pwm && attr == bindings::hwmon_pwm_attributes_hwmon_pwm_input {
            return pwm_from_duty(smbus_read(self.raw(), REG_DUTY)?);
        }
        let ch = usize::try_from(channel).ok().filter(|&c| c < CHANNELS);
        match (ty, ch) {
            (bindings::hwmon_sensor_types_hwmon_fan, Some(ch)) => {
                let raw = smbus_read(self.raw(), tach_reg(ch))?;
                if attr == bindings::hwmon_fan_attributes_hwmon_fan_input {
                    Ok(rpm(raw))
                } else if attr == bindings::hwmon_fan_attributes_hwmon_fan_fault {
                    Ok(fault(smbus_read(self.raw(), REG_PRESENT)?, raw, ch))
                } else {
                    Err(E_OPNOTSUPP)
                }
            }
            _ => Err(E_OPNOTSUPP),
        }
    }

    /// fan_write() of the C driver, including its fallback to full speed.
    fn write(&self, ty: u32, attr: u32, channel: c_int, val: c_long) -> Result<(), i32> {
        if ty != bindings::hwmon_sensor_types_hwmon_pwm || attr != bindings::hwmon_pwm_attributes_hwmon_pwm_input || channel != 0 {
            return Err(E_OPNOTSUPP);
        }
        let duty = duty_from_pwm(val as i64)?;
        let _g = self.lock.lock();
        smbus_write(self.raw(), REG_PRE_WRITE, 0)?;
        let mut ret = smbus_write(self.raw(), REG_DUTY, duty);
        if ret.is_ok() {
            ret = match smbus_read(self.raw(), REG_DUTY) {
                Ok(check) if check == duty => Ok(()),
                Ok(_) => Err(E_IO),
                Err(e) => Err(e),
            };
        }
        // A requested duty that cannot be verified: attempt full speed and
        // report the failure. A broken bus cannot provide a cooling guarantee.
        if ret.is_err() && smbus_write(self.raw(), REG_DUTY, DUTY_MAX).is_err() {
            dev_err!(self.client.as_ref(), "fan duty failed and full-speed fallback failed\n");
        }
        ret
    }
}

/// hwmon registration with static chip info. Not a general abstraction: just
/// enough for this driver, with its contract stated.
mod hwmon {
    use super::*;

    /// Statics of C types holding pointers to other statics (never written).
    #[repr(transparent)]
    pub(crate) struct Static<T>(pub(crate) T);
    // SAFETY: only ever read, by C and Rust; every pointer inside points to
    // another such static.
    unsafe impl<T> Sync for Static<T> {}

    /// A registered hwmon device; dropping it unregisters it.
    pub(crate) struct Registration(NonNull<bindings::device>);

    impl Registration {
        /// Registers a hwmon device under `parent` (an i2c_client's device).
        ///
        /// # Safety
        ///
        /// `drvdata` must stay valid, and its pointee must not move, until the
        /// returned value is dropped: the callbacks in `chip` read it from any
        /// thread until hwmon_device_unregister() has returned (which waits for
        /// sysfs callbacks in flight). `parent` must be a live device.
        pub(crate) unsafe fn new(
            parent: *mut bindings::device,
            name: &'static CStr,
            drvdata: *mut c_void,
            chip: &'static Static<bindings::hwmon_chip_info>,
        ) -> Result<Self> {
            // SAFETY: parent is a live device (caller), name is static and
            // NUL-terminated, chip and everything it points to is static.
            let p = unsafe {
                bindings::hwmon_device_register_with_info(parent, name.as_ptr(), drvdata, &chip.0, ptr::null_mut())
            };
            let p = kernel::error::from_err_ptr(p)?;
            Ok(Self(NonNull::new(p).ok_or(EINVAL)?))
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: self.0 came from hwmon_device_register_with_info and is
            // unregistered exactly once, here.
            unsafe { bindings::hwmon_device_unregister(self.0.as_ptr()) };
        }
    }

    // SAFETY: the registration is a handle; unregistering it from another
    // thread is allowed.
    unsafe impl Send for Registration {}
    // SAFETY: &Registration offers no operation.
    unsafe impl Sync for Registration {}
}

static CLABELS: [&CStr; CHANNELS] = [
    c"fan1_front", c"fan1_rear", c"fan2_front", c"fan2_rear", c"fan3_front", c"fan3_rear",
    c"fan4_front", c"fan4_rear", c"fan5_front", c"fan5_rear", c"fan6_front", c"fan6_rear",
];

const FAN_ATTRS: u32 = (1 << bindings::hwmon_fan_attributes_hwmon_fan_input)
    | (1 << bindings::hwmon_fan_attributes_hwmon_fan_fault)
    | (1 << bindings::hwmon_fan_attributes_hwmon_fan_label);
static PWM_CONFIG: [u32; 2] = [1 << bindings::hwmon_pwm_attributes_hwmon_pwm_input, 0];
static FAN_CONFIG: [u32; CHANNELS + 1] = {
    let mut c = [FAN_ATTRS; CHANNELS + 1];
    c[CHANNELS] = 0;
    c
};
static PWM_INFO: hwmon::Static<bindings::hwmon_channel_info> = hwmon::Static(bindings::hwmon_channel_info {
    type_: bindings::hwmon_sensor_types_hwmon_pwm,
    config: &raw const PWM_CONFIG as *const u32,
});
static FAN_INFO: hwmon::Static<bindings::hwmon_channel_info> = hwmon::Static(bindings::hwmon_channel_info {
    type_: bindings::hwmon_sensor_types_hwmon_fan,
    config: &raw const FAN_CONFIG as *const u32,
});
static INFO: hwmon::Static<[*const bindings::hwmon_channel_info; 3]> =
    hwmon::Static([&raw const PWM_INFO.0, &raw const FAN_INFO.0, ptr::null()]);
static OPS: hwmon::Static<bindings::hwmon_ops> = hwmon::Static(bindings::hwmon_ops {
    visible: 0,
    is_visible: Some(cb_is_visible),
    read: Some(cb_read),
    read_string: Some(cb_read_string),
    write: Some(cb_write),
});
static CHIP: hwmon::Static<bindings::hwmon_chip_info> = hwmon::Static(bindings::hwmon_chip_info {
    ops: &raw const OPS.0,
    info: &raw const INFO.0 as *const *const bindings::hwmon_channel_info,
});

/// The Chip behind a hwmon device of this driver.
///
/// # Safety
///
/// `dev` must be a hwmon device registered by `As7760Fan::try_probe`, whose
/// drvdata is the pinned Chip that outlives the registration.
unsafe fn chip<'a>(dev: *mut bindings::device) -> &'a Chip {
    // SAFETY: caller; hwmon sets the hwmon device's drvdata to what we passed.
    unsafe { &*bindings::dev_get_drvdata(dev).cast::<Chip>() }
}

unsafe extern "C" fn cb_is_visible(_d: *const c_void, ty: u32, attr: u32, _ch: c_int) -> bindings::umode_t {
    if ty == bindings::hwmon_sensor_types_hwmon_pwm && attr == bindings::hwmon_pwm_attributes_hwmon_pwm_input {
        0o644
    } else if ty == bindings::hwmon_sensor_types_hwmon_fan {
        0o444
    } else {
        0
    }
}

unsafe extern "C" fn cb_read(dev: *mut bindings::device, ty: u32, attr: u32, ch: c_int, val: *mut c_long) -> c_int {
    // SAFETY: hwmon calls this only for our device (chip()'s contract).
    match unsafe { chip(dev) }.read(ty, attr, ch) {
        Ok(v) => {
            // SAFETY: hwmon passes a valid out pointer.
            unsafe { *val = v as c_long };
            0
        }
        Err(e) => -e,
    }
}

unsafe extern "C" fn cb_read_string(_dev: *mut bindings::device, ty: u32, attr: u32, ch: c_int, s: *mut *const c_char) -> c_int {
    match usize::try_from(ch).ok().filter(|&c| c < CHANNELS) {
        Some(c) if ty == bindings::hwmon_sensor_types_hwmon_fan && attr == bindings::hwmon_fan_attributes_hwmon_fan_label => {
            // SAFETY: hwmon passes a valid out pointer; the label is static.
            unsafe { *s = CLABELS[c].as_ptr().cast() };
            0
        }
        _ => -E_OPNOTSUPP,
    }
}

unsafe extern "C" fn cb_write(dev: *mut bindings::device, ty: u32, attr: u32, ch: c_int, val: c_long) -> c_int {
    // SAFETY: as in cb_read.
    match unsafe { chip(dev) }.write(ty, attr, ch, val) {
        Ok(()) => 0,
        Err(e) => -e,
    }
}

struct As7760Fan {
    // Field order is drop order: unregister the hwmon device before the Chip
    // its callbacks use is freed.
    _hwmon: hwmon::Registration,
    _chip: Pin<KBox<Chip>>,
}

impl As7760Fan {
    fn try_probe(idev: &i2c::I2cClient<Core<'_>>) -> Result<Self> {
        let raw = ptr::from_ref(idev).cast::<bindings::i2c_client>();
        // SAFETY: raw is the bound client probe() was called with.
        if !unsafe { as7760_fan_smbus_byte_data_ok(raw) } {
            return Err(Error::from_errno(-E_OPNOTSUPP));
        }
        let duty = smbus_read(raw, REG_DUTY).map_err(|e| Error::from_errno(-e))?;
        if !probe_duty_ok(duty) {
            return Err(ENODEV);
        }
        let client: ARef<i2c::I2cClient> = idev.into();
        let chip = KBox::pin_init(pin_init!(Chip { client, lock <- new_mutex!(()) }), GFP_KERNEL)?;
        let data = ptr::from_ref::<Chip>(&chip).cast_mut().cast::<c_void>();
        // SAFETY: parent: the client's embedded struct device. data: the
        // pinned Chip, which lives in the returned value and is dropped after
        // the registration (field order above).
        let hwmon = unsafe {
            hwmon::Registration::new(&raw mut (*raw.cast_mut()).dev, c"as7760_fan", data, &CHIP)
        }?;
        Ok(Self { _hwmon: hwmon, _chip: chip })
    }
}

impl i2c::Driver for As7760Fan {
    type IdInfo = ();
    type Data<'bound> = Self;
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        idev: &'bound i2c::I2cClient<Core<'_>>,
        _info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self, Error> + 'bound {
        Self::try_probe(idev)
    }
}

kernel::module_i2c_driver! {
    type: As7760Fan,
    name: "as7760_fan",
    authors: ["ai_re"],
    description: "Edge-Core AS7760 six dual-rotor fans and shared PWM (Rust)",
    license: "GPL",
}

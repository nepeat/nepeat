// SPDX-License-Identifier: GPL-2.0

//! Edgecore DMC-12-48R-AS0: full board reset through the board CPLD on reboot.
//!
//! The Rust twin of ../dmc12_cpld_reset.c (same module name, same `bus`
//! parameter, same sequence and fallbacks; read that file's header for the why).
//! Differences: messages carry the module name prefix (`dmc12_cpld_reset:`), and
//! `bus` is not shown under /sys/module (Rust module parameters have no sysfs
//! file in 7.3).
//!
//! Layout: [`sequence`] is the logic (host-tested), [`reboot`] the notifier
//! registration, and this file the SMBus write and the glue. `unsafe`: reboot.rs
//! has 4 blocks, the C callback, 2 `unsafe impl` and 2 hand-written C
//! declarations (checked by abi_check.c); this file has 1 block. Inventory:
//! ../../kernel-rust/README.md.

use kernel::{
    bindings,
    error::to_result,
    i2c::I2cAdapter,
    prelude::*,
    time::{delay::udelay, Delta},
};

mod reboot;
mod sequence;

module! {
    type: Dmc12CpldReset,
    name: "dmc12_cpld_reset",
    authors: ["ai_re"],
    description: "DMC-12 board reset through the CPLD on reboot",
    license: "GPL",
    params: {
        bus: i32 {
            default: 4,
            description: "Linux I2C bus of the CPLD (DT alias i2c4)",
        },
    },
}

/// SMBus byte writes through an I2C adapter, with no client bound: userspace
/// (fan control, interrupt monitor, metrics) keeps raw i2c-dev access to 0x68.
struct Smbus<'a>(&'a I2cAdapter);

impl sequence::ByteWrite for Smbus<'_> {
    type Error = Error;

    fn write_byte_data(&mut self, addr: u16, reg: u8, val: u8) -> Result {
        let mut data = bindings::i2c_smbus_data { byte: val };
        let adap = core::ptr::from_ref(self.0).cast::<bindings::i2c_adapter>().cast_mut();
        // SAFETY: `I2cAdapter` is a `repr(transparent)` wrapper of a valid
        // `struct i2c_adapter` (its type invariant), kept alive by the borrow;
        // `data` is a live `i2c_smbus_data` whose `byte` member is the one a
        // BYTE_DATA write reads. The caller is in process context.
        to_result(unsafe {
            bindings::i2c_smbus_xfer(
                adap,
                addr,
                0,
                bindings::I2C_SMBUS_WRITE as _,
                reg,
                bindings::I2C_SMBUS_BYTE_DATA as _,
                &mut data,
            )
        })
    }
}

struct ResetOnRestart {
    bus: i32,
}

impl reboot::Handler for ResetOnRestart {
    fn notify(&self, action: reboot::Action) {
        if action != reboot::Action::RESTART {
            return;
        }
        let Ok(adap) = I2cAdapter::get(self.bus) else {
            pr_err!("no i2c-{}, falling back\n", self.bus);
            return;
        };
        pr_emerg!("board reset via CPLD\n");
        if let Err(e) = sequence::board_reset(&mut Smbus(&adap)) {
            pr_err!("CPLD write failed ({}), falling back\n", e.to_errno());
            return;
        }
        // Busy-wait as the C module's mdelay(): nothing else should run now.
        for _ in 0..sequence::RESET_WAIT_MS {
            udelay(Delta::from_millis(1));
        }
        // Returning lets the kernel go on to the sp805 restart handler (SoC-only reset).
        pr_err!("CPLD did not reset the board, falling back\n");
    }
}

#[pin_data]
struct Dmc12CpldReset {
    #[pin]
    notifier: reboot::Registration<ResetOnRestart>,
}

impl kernel::InPlaceModule for Dmc12CpldReset {
    fn init(_module: &'static ThisModule) -> impl PinInit<Self, Error> {
        let handler = ResetOnRestart { bus: module_parameters::bus.value() };
        // Lowest priority, to run after other notifiers: they may still want the hardware.
        try_pin_init!(Self {
            notifier <- reboot::Registration::new(handler, i32::MIN),
        })
    }
}

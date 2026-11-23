//! Front-panel ports: enumeration, enable, link, speed, and the board settings of the MAC side
//! (interface type, autoneg, FEC, inter-frame gap).

use std::ffi::{CStr, c_char, c_int};

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::switch::Switch;

/// The most ports one enumeration returns (the C++ backend's buffer).
pub const MAX_PORTS: usize = 512;
/// The most speeds [`Switch::port_speeds`] returns (the C++ backend's buffer; the glue knows 7).
pub const MAX_SPEEDS: usize = 16;

/// A front-panel port as the SDK configured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// The SDK's logical port number.
    pub port: i32,
    /// The first physical port (the lane number `SONiC` uses for the first lane).
    pub first_lane: u32,
    /// Number of lanes, at least 1.
    pub lanes: u32,
    /// Speed in Mb/s after the bring-up.
    pub speed: u32,
    /// The SDK's name, e.g. `ce0`.
    pub name: String,
}

/// Forward error correction in the switch chip's port (`ob_sdk_port_fec_set`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fec {
    /// Off.
    None,
    /// Reed-Solomon (clause 91 at 100G, clause 108 below).
    Rs,
    /// Fire code (clause 74).
    Fc,
}

/// The interface type names `ob_sdk_port_interface_set` knows (`backend.h`,
/// `port_interface_names`).
pub const INTERFACE_NAMES: [&str; 14] = [
    "caui", "caui4", "caui_c2c", "caui_c2m", "xlaui", "cr4", "sr4", "lr4", "kr4", "sfi", "xfi", "cr", "sr", "kr",
];

/// The bytes of a C `char[16]` up to its NUL. `c_char` is signed on x86 and unsigned on
/// ppc64; one byte has no byte order, so its bits are taken as they are.
fn name_text(name: &[c_char; 16]) -> String {
    let bytes: Vec<u8> = name
        .iter()
        .map(|c| u8::from_ne_bytes(c.to_ne_bytes()))
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn non_negative(value: c_int) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::INTERNAL)
}

/// `OB_E_PARAM`, for a value that does not fit the glue's `int`.
fn param() -> Error {
    Error::from_raw(sys::OB_E_PARAM).unwrap_or(Error::INTERNAL)
}

impl Switch {
    /// The CPU port's logical number (`ob_sdk_cpu_port`).
    pub fn cpu_port(&self) -> i32 {
        // SAFETY: self exists, so the unit is attached and initialised; the function takes
        // the unit number only and reads a constant of the chip.
        unsafe { sys::ob_sdk_cpu_port(self.raw_unit()) }
    }

    /// The front-panel ports in the SDK's order (`ob_sdk_ports`): management ports and the
    /// CPU port are not among them.
    pub fn ports(&self) -> Result<Vec<PortInfo>, Error> {
        let empty = sys::ob_port_info_t {
            port: 0,
            phys: 0,
            lanes: 0,
            speed: 0,
            name: [0; 16],
        };
        let mut raw = vec![empty; MAX_PORTS];
        let max = c_int::try_from(raw.len()).unwrap_or(0);
        // SAFETY: self exists, so ob_sdk_init succeeded on this unit. raw is a live Vec of
        // max initialised ob_port_info_t (the layout sdk-glue-sys asserts against the header);
        // the glue writes at most max elements and does not keep the pointer.
        let rv = unsafe { sys::ob_sdk_ports(self.raw_unit(), raw.as_mut_ptr(), max) };
        let count = usize::try_from(check(rv)?).map_err(|_| Error::INTERNAL)?;
        // More than the buffer holds would be a foreign glue.
        let filled = raw.get(..count).ok_or(Error::INTERNAL)?;
        filled
            .iter()
            .map(|p| {
                Ok(PortInfo {
                    port: p.port,
                    first_lane: non_negative(p.phys)?,
                    lanes: non_negative(p.lanes)?.max(1),
                    speed: non_negative(p.speed)?,
                    name: name_text(&p.name),
                })
            })
            .collect()
    }

    /// Enables or disables a port (`bcm_port_enable_set`).
    pub fn port_enable_set(&mut self, port: i32, enable: bool) -> Result<(), Error> {
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers.
        let rv = unsafe { sys::ob_sdk_port_enable_set(self.raw_unit(), port, c_int::from(enable)) };
        check(rv).map(|_| ())
    }

    /// Whether the port's link is up (`bcm_port_link_status_get`).
    pub fn port_link_get(&self, port: i32) -> Result<bool, Error> {
        let mut up: c_int = 0;
        // SAFETY: self exists (BCM layer up). &raw mut up points to a live local the glue
        // writes one int to and does not keep; the rest are plain integers.
        let rv = unsafe { sys::ob_sdk_port_link_get(self.raw_unit(), port, &raw mut up) };
        check(rv)?;
        Ok(up != 0)
    }

    /// Sets the port's speed in Mb/s.
    pub fn port_speed_set(&mut self, port: i32, mbps: u32) -> Result<(), Error> {
        let mbps = c_int::try_from(mbps).map_err(|_| param())?;
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers.
        let rv = unsafe { sys::ob_sdk_port_speed_set(self.raw_unit(), port, mbps) };
        check(rv).map(|_| ())
    }

    /// The port's speed in Mb/s.
    pub fn port_speed_get(&self, port: i32) -> Result<u32, Error> {
        let mut mbps: c_int = 0;
        // SAFETY: self exists (BCM layer up). &raw mut mbps points to a live local the glue
        // writes one int to and does not keep; the rest are plain integers.
        let rv = unsafe { sys::ob_sdk_port_speed_get(self.raw_unit(), port, &raw mut mbps) };
        check(rv)?;
        non_negative(mbps)
    }

    /// The speeds in Mb/s the port can run at (from its local abilities).
    pub fn port_speeds(&self, port: i32) -> Result<Vec<u32>, Error> {
        let mut speeds = [0u32; MAX_SPEEDS];
        let max = c_int::try_from(speeds.len()).unwrap_or(0);
        // SAFETY: self exists (BCM layer up). speeds is a live local of max u32 (uint32_t);
        // the glue writes at most max elements and does not keep the pointer.
        let rv = unsafe { sys::ob_sdk_port_speeds(self.raw_unit(), port, speeds.as_mut_ptr(), max) };
        let count = usize::try_from(check(rv)?).map_err(|_| Error::INTERNAL)?;
        speeds.get(..count).map(<[u32]>::to_vec).ok_or(Error::INTERNAL)
    }

    /// Switches autonegotiation on or off.
    pub fn port_autoneg_set(&mut self, port: i32, on: bool) -> Result<(), Error> {
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers.
        let rv = unsafe { sys::ob_sdk_port_autoneg_set(self.raw_unit(), port, c_int::from(on)) };
        check(rv).map(|_| ())
    }

    /// Sets the interface type the MAC and its lanes run, by one of [`INTERFACE_NAMES`]
    /// (another name: `OB_E_PARAM` from the glue).
    pub fn port_interface_set(&mut self, port: i32, name: &CStr) -> Result<(), Error> {
        let name: *const c_char = name.as_ptr();
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out.
        // name comes from a &CStr: NUL-terminated, alive across the call; the glue compares
        // it with its table (strcmp) and does not keep it.
        let rv = unsafe { sys::ob_sdk_port_interface_set(self.raw_unit(), port, name) };
        check(rv).map(|_| ())
    }

    /// Sets the port's FEC.
    pub fn port_fec_set(&mut self, port: i32, fec: Fec) -> Result<(), Error> {
        let mode: c_int = match fec {
            Fec::None => 0,
            Fec::Rs => 1,
            Fec::Fc => 2,
        };
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers, mode one of the three values the glue defines.
        let rv = unsafe { sys::ob_sdk_port_fec_set(self.raw_unit(), port, mode) };
        check(rv).map(|_| ())
    }

    /// Sets the average inter-frame gap in bit times at the port's current speed.
    pub fn port_ifg_set(&mut self, port: i32, bits: u32) -> Result<(), Error> {
        let bits = c_int::try_from(bits).map_err(|_| param())?;
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers.
        let rv = unsafe { sys::ob_sdk_port_ifg_set(self.raw_unit(), port, bits) };
        check(rv).map(|_| ())
    }
}

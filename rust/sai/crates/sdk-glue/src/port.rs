//! Front-panel ports: enumeration, enable, link, speed, the board settings of the MAC side
//! (interface type, autoneg, FEC, inter-frame gap), frame size, loopback, PVID, learning,
//! counters.

use std::ffi::{CStr, c_char, c_int};

use sdk_glue_sys as sys;

pub use sys::order::{Fec, Learn, Loopback, PortStat};

use crate::error::{Error, check};
use crate::switch::{Switch, count};

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

/// The interface type names `ob_sdk_port_interface_set` knows (`backend.h`,
/// `port_interface_names`).
pub const INTERFACE_NAMES: [&str; 14] = [
    "caui", "caui4", "caui_c2c", "caui_c2m", "xlaui", "cr4", "sr4", "lr4", "kr4", "sfi", "xfi", "cr", "sr", "kr",
];

/// The bytes of a C `char[16]` up to its NUL. `c_char` is signed on x86 and unsigned on ppc64;
/// one byte has no byte order, so its bits are taken as they are.
pub(crate) fn name_text(name: &[c_char]) -> String {
    let bytes: Vec<u8> = name
        .iter()
        .map(|c| u8::from_ne_bytes(c.to_ne_bytes()))
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A value that does not fit the glue's `int`: `OB_E_PARAM`.
pub(crate) fn c_value(value: u32) -> Result<c_int, Error> {
    c_int::try_from(value).map_err(|_| Error::PARAM)
}

impl Switch {
    /// The CPU port's logical number (`ob_sdk_cpu_port`). `-1` if the glue refuses (it cannot,
    /// while this handle exists: the unit is attached and initialised).
    pub fn cpu_port(&self) -> i32 {
        let port = self.glue().cpu_port(self.raw_unit());
        if port < 0 { -1 } else { port }
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
        let n = usize::try_from(check(self.glue().ports(self.raw_unit(), &mut raw))?).map_err(|_| Error::INTERNAL)?;
        // More than the buffer holds would be a foreign glue.
        let filled = raw.get(..n).ok_or(Error::INTERNAL)?;
        filled
            .iter()
            .map(|p| {
                Ok(PortInfo {
                    port: p.port,
                    first_lane: count(p.phys)?,
                    lanes: count(p.lanes)?.max(1),
                    speed: count(p.speed)?,
                    name: name_text(&p.name),
                })
            })
            .collect()
    }

    /// Enables or disables a port (`bcm_port_enable_set`).
    pub fn port_enable_set(&mut self, port: i32, enable: bool) -> Result<(), Error> {
        check(self.glue().port_enable_set(self.raw_unit(), port, c_int::from(enable))).map(|_| ())
    }

    /// Whether the port's link is up (`bcm_port_link_status_get`).
    pub fn port_link_get(&self, port: i32) -> Result<bool, Error> {
        let mut up: c_int = 0;
        check(self.glue().port_link_get(self.raw_unit(), port, &mut up))?;
        Ok(up != 0)
    }

    /// Sets the port's speed in Mb/s.
    pub fn port_speed_set(&mut self, port: i32, mbps: u32) -> Result<(), Error> {
        let mbps = c_value(mbps)?;
        check(self.glue().port_speed_set(self.raw_unit(), port, mbps)).map(|_| ())
    }

    /// The port's speed in Mb/s.
    pub fn port_speed_get(&self, port: i32) -> Result<u32, Error> {
        let mut mbps: c_int = 0;
        check(self.glue().port_speed_get(self.raw_unit(), port, &mut mbps))?;
        count(mbps)
    }

    /// The speeds in Mb/s the port can run at (from its local abilities).
    pub fn port_speeds(&self, port: i32) -> Result<Vec<u32>, Error> {
        let mut speeds = [0u32; MAX_SPEEDS];
        let n = usize::try_from(check(self.glue().port_speeds(self.raw_unit(), port, &mut speeds))?)
            .map_err(|_| Error::INTERNAL)?;
        speeds.get(..n).map(<[u32]>::to_vec).ok_or(Error::INTERNAL)
    }

    /// The largest frame the port accepts, in bytes (the SAI passes MTU + 8).
    pub fn port_frame_max_set(&mut self, port: i32, bytes: u32) -> Result<(), Error> {
        let bytes = c_value(bytes)?;
        check(self.glue().port_frame_max_set(self.raw_unit(), port, bytes)).map(|_| ())
    }

    /// Switches autonegotiation on or off.
    pub fn port_autoneg_set(&mut self, port: i32, on: bool) -> Result<(), Error> {
        check(self.glue().port_autoneg_set(self.raw_unit(), port, c_int::from(on))).map(|_| ())
    }

    /// Whether autonegotiation is on.
    pub fn port_autoneg_get(&self, port: i32) -> Result<bool, Error> {
        let mut on: c_int = 0;
        check(self.glue().port_autoneg_get(self.raw_unit(), port, &mut on))?;
        Ok(on != 0)
    }

    /// Sets the interface type the MAC and its lanes run, by one of [`INTERFACE_NAMES`]
    /// (another name: `OB_E_PARAM` from the glue).
    pub fn port_interface_set(&mut self, port: i32, name: &CStr) -> Result<(), Error> {
        check(self.glue().port_interface_set(self.raw_unit(), port, name)).map(|_| ())
    }

    /// [`Switch::port_interface_set`] by a Rust string: `OB_E_PARAM` unless it is one of
    /// [`INTERFACE_NAMES`] (checked here, before the glue).
    pub fn port_interface_set_name(&mut self, port: i32, name: &str) -> Result<(), Error> {
        if !INTERFACE_NAMES.contains(&name) {
            return Err(Error::PARAM);
        }
        let name = crate::switch::c_text(name, 16)?;
        self.port_interface_set(port, &name)
    }

    /// Sets the port's FEC (RS: clause 91 at 100G and above, clause 108 below).
    pub fn port_fec_set(&mut self, port: i32, fec: Fec) -> Result<(), Error> {
        check(self.glue().port_fec_set(self.raw_unit(), port, fec.raw())).map(|_| ())
    }

    /// Sets the average inter-frame gap in bit times at the port's current speed.
    pub fn port_ifg_set(&mut self, port: i32, bits: u32) -> Result<(), Error> {
        let bits = c_value(bits)?;
        check(self.glue().port_ifg_set(self.raw_unit(), port, bits)).map(|_| ())
    }

    /// Sets the port's loopback.
    pub fn port_loopback_set(&mut self, port: i32, mode: Loopback) -> Result<(), Error> {
        check(self.glue().port_loopback_set(self.raw_unit(), port, mode.raw())).map(|_| ())
    }

    /// Sets the port's untagged VLAN (PVID), 1..=4094 (the glue refuses others).
    pub fn port_pvid_set(&mut self, port: i32, vid: u16) -> Result<(), Error> {
        check(self.glue().port_pvid_set(self.raw_unit(), port, c_int::from(vid))).map(|_| ())
    }

    /// Sets address learning on the port.
    pub fn port_learn_set(&mut self, port: i32, mode: Learn) -> Result<(), Error> {
        check(self.glue().port_learn_set(self.raw_unit(), port, mode.raw())).map(|_| ())
    }

    /// One of the port's counters (`bcm_stat_get` of the SNMP counter [`PortStat`] names).
    pub fn port_stat_get(&self, port: i32, stat: PortStat) -> Result<u64, Error> {
        let mut v = 0u64;
        check(self.glue().port_stat_get(self.raw_unit(), port, stat.raw(), &mut v))?;
        Ok(v)
    }

    /// Clears the port's counters.
    pub fn port_stat_clear(&mut self, port: i32) -> Result<(), Error> {
        check(self.glue().port_stat_clear(self.raw_unit(), port)).map(|_| ())
    }
}

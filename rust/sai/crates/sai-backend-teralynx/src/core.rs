//! Shared state of the backend (`tl_core.{h,cpp}`): the loaded libifcs, the settings, the port map
//! and the sysport handles (immutable once init has read them), the status mapping and logging of
//! failed calls, and the devport / sysport attribute helpers every area uses.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use ifcs::{Attrs, Handle, Ifcs, Status, sys};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::config::PortSpec;
use crate::keys::Settings;

/// The one node (`NODE`).
pub const NODE: u32 = 0;
/// The CPU's devport (`CPU_DEVPORT`; its sysport is 1000).
pub const CPU_DEVPORT: u32 = 0;
/// Documented capacities (`devices/cs8210-32x/fib-capacity.md`): no ILPM in this SDK build, so
/// prefixes live in the 24,576-entry TCAM.
pub const CAP_V4_PREFIX: u32 = 24576;
/// IPv6 prefixes (two slots each).
pub const CAP_V6_PREFIX: u32 = 12288;
/// Host entries (profile A).
pub const CAP_V4_HOST: u32 = 98304;
/// MAC entries (profile A).
pub const CAP_MAC: u32 = 45056;
/// Paths per ECMP group (ours; no IFCS limit found).
pub const ECMP_PATHS: u32 = 64;
/// VRFs: L3VNI values 1..=256 (vrf + 1).
pub const VRF_MAX: u32 = 255;

/// What every area shares once the SDK is up.
#[derive(Debug)]
pub struct Core {
    /// The libifcs table.
    pub lib: Ifcs,
    /// The profile's values.
    pub settings: Settings,
    /// The port map (immutable after init; the callbacks hold a copy of its devports).
    pub specs: Vec<PortSpec>,
    /// devport -> sysport (immutable after init).
    pub sysport: BTreeMap<u32, Handle>,
    /// sysport -> devport.
    pub dev_of_sysport: Arc<BTreeMap<Handle, u32>>,
    /// Keys of the messages logged once (`Core::logged`).
    logged: Mutex<BTreeSet<String>>,
}

impl Core {
    /// A core over a loaded libifcs, before the sysports are known.
    pub fn new(lib: Ifcs, settings: Settings, specs: Vec<PortSpec>) -> Self {
        Self {
            lib,
            settings,
            specs,
            sysport: BTreeMap::new(),
            dev_of_sysport: Arc::new(BTreeMap::new()),
            logged: Mutex::new(BTreeSet::new()),
        }
    }

    /// `Core::fail`: logs `ifcs <what>: <reason>` (`SAI_API_UNSPECIFIED`; info for UNSUPPORTED,
    /// else error) and maps the reason.
    pub fn fail(&self, reason: ifcs::status::Reason, what: &str) -> Error {
        let level = if reason == sys::IFCS_UNSUPPORTED {
            ssys::SAI_LOG_LEVEL_INFO
        } else {
            ssys::SAI_LOG_LEVEL_ERROR
        };
        log::emit(
            ssys::SAI_API_UNSPECIFIED,
            level,
            format_args!("ifcs {what}: {}", ifcs::status::reason_name(reason)),
        );
        ifcs::status::sai_error(reason)
    }

    /// `Core::call`: success, or [`Core::fail`].
    pub fn call(&self, rc: Status, what: &str) -> Result<()> {
        if rc.is_success() {
            Ok(())
        } else {
            Err(self.fail(rc.reason(), what))
        }
    }

    /// `Core::log_once`: a notice (`SAI_API_SWITCH`) the first time `key` is seen.
    pub fn log_once(&self, key: &str, message: core::fmt::Arguments<'_>) {
        if self
            .logged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_owned())
        {
            log::notice(ssys::SAI_API_SWITCH, message);
        }
    }

    /// `Core::not_impl`: `NOT_IMPLEMENTED`, warned once per `what`.
    pub fn not_impl(&self, what: &str) -> Error {
        if self
            .logged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(format!("ni:{what}"))
        {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!("teralynx backend: {what} is NOT_IMPLEMENTED (see sai-teralynx README)"),
            );
        }
        Error::NotImplemented
    }

    /// `Core::usage_free`: `max - current` (0 if not more), or `fallback` with a notice once.
    pub fn usage_free(&self, rc: Status, u: &sys::ifcs_usage_t, fallback: u32, what: &str) -> u32 {
        if rc.is_success() {
            return u.max.saturating_sub(u.current);
        }
        self.log_once(
            &format!("usage:{what}"),
            format_args!("{what} usage unreadable; reporting {fallback} free"),
        );
        fallback
    }

    /// `Core::front`: the port map's devport `hw`.
    pub fn front(&self, hw: u32) -> Option<&PortSpec> {
        self.specs.iter().find(|s| s.devport == hw)
    }

    /// `Core::dp_set`: one devport attribute.
    pub fn dp_set(&self, hw: u32, attrs: Attrs, what: &str) -> Result<()> {
        let mut attrs = attrs;
        self.call(self.lib.ifcs_devport_attr_set(NODE, hw, &mut attrs), what)
    }

    /// `Core::dp_get_u32`: one devport attribute's `value.u32`.
    pub fn dp_get_u32(&self, hw: u32, id: u32) -> Result<u32> {
        let mut a = Attrs::new().of(id);
        let (rc, _) = self.lib.ifcs_devport_attr_get(NODE, hw, &mut a);
        if !rc.is_success() {
            return Err(self.fail(rc.reason(), "devport attr get"));
        }
        Ok(a.u32_at(0))
    }

    /// `Core::sp_set`: one sysport attribute of devport `hw`'s sysport.
    pub fn sp_set(&self, hw: u32, attrs: Attrs, what: &str) -> Result<()> {
        let sp = self.sysport.get(&hw).ok_or(Error::InvalidPortNumber)?;
        let mut attrs = attrs;
        self.call(self.lib.ifcs_sysport_attr_set(NODE, sp.0, &mut attrs), what)
    }
}

/// `speed_from_enum`: `IFCS_DEVPORT_SPEED_*` in Mb/s, 0 for anything else.
pub const fn speed_from_enum(e: u32) -> u32 {
    match e {
        sys::IFCS_DEVPORT_SPEED_10G => 10_000,
        sys::IFCS_DEVPORT_SPEED_25G => 25_000,
        sys::IFCS_DEVPORT_SPEED_40G => 40_000,
        sys::IFCS_DEVPORT_SPEED_50G => 50_000,
        sys::IFCS_DEVPORT_SPEED_100G => 100_000,
        sys::IFCS_DEVPORT_SPEED_200G => 200_000,
        sys::IFCS_DEVPORT_SPEED_400G => 400_000,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_as_tl_core() {
        assert_eq!(speed_from_enum(sys::IFCS_DEVPORT_SPEED_400G), 400_000);
        assert_eq!(speed_from_enum(sys::IFCS_DEVPORT_SPEED_10G), 10_000);
        assert_eq!(speed_from_enum(0), 0);
        assert_eq!(speed_from_enum(8), 0);
    }
}

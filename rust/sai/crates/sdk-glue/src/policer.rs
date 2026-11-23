//! SDK meters (policers) for field entries.

use std::ffi::c_int;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::port::c_value;
use crate::switch::{Switch, count};

/// A meter (`ob_policer_cfg_t`). With `packets` rates are packets/s and bursts packets, else
/// bytes/s and bytes (the glue converts to kbits, rounded up).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools, reason = "the glue's flags")]
pub struct PolicerCfg {
    /// trTCM (RFC 2698), else srTCM (one bucket when `pbs` is 0).
    pub tr_tcm: bool,
    /// Packet mode.
    pub packets: bool,
    /// Colour blind.
    pub color_blind: bool,
    /// Committed rate.
    pub cir: u64,
    /// Committed burst.
    pub cbs: u64,
    /// Peak rate.
    pub pir: u64,
    /// Peak / excess burst.
    pub pbs: u64,
}

impl PolicerCfg {
    pub(crate) fn raw(self) -> sys::ob_policer_cfg_t {
        sys::ob_policer_cfg_t {
            tr_tcm: c_int::from(self.tr_tcm),
            packets: c_int::from(self.packets),
            color_blind: c_int::from(self.color_blind),
            cir: self.cir,
            cbs: self.cbs,
            pir: self.pir,
            pbs: self.pbs,
        }
    }
}

impl Switch {
    /// Creates a meter; its id.
    pub fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<u32, Error> {
        let mut pid: c_int = -1;
        check(self.glue().policer_create(self.raw_unit(), &cfg.raw(), &mut pid))?;
        count(pid)
    }

    /// Changes meter `pid`.
    pub fn policer_set(&mut self, pid: u32, cfg: &PolicerCfg) -> Result<(), Error> {
        let p = c_value(pid)?;
        check(self.glue().policer_set(self.raw_unit(), p, &cfg.raw())).map(|_| ())
    }

    /// Destroys meter `pid`.
    pub fn policer_destroy(&mut self, pid: u32) -> Result<(), Error> {
        let p = c_value(pid)?;
        check(self.glue().policer_destroy(self.raw_unit(), p)).map(|_| ())
    }
}

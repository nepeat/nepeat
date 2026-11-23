//! Clause-45 MDIO through the switch chip's MIIM master, for PHYs the SDK has no driver for.

use std::ffi::c_uint;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::unit::Sdk;

/// A clause-45 device (MMD) number, 1..=31. Zero would be a clause-22 access: it has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mmd(u8);

impl Mmd {
    /// `None` unless `1 <= mmd <= 31`.
    pub const fn new(mmd: u8) -> Option<Self> {
        if mmd >= 1 && mmd <= 31 { Some(Self(mmd)) } else { None }
    }

    /// The device number, 1..=31.
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Sdk {
    /// Reads register `mmd.reg` of the PHY at `phy`, the SDK's address word (bus in bits 8 and
    /// 6:5, address in bits 4:0).
    pub fn mdio_c45_read(&mut self, phy: u32, mmd: Mmd, reg: u16) -> Result<u16, Error> {
        let mut val: c_uint = 0;
        // SAFETY: self exists, so ob_sdk_init_mdio (or the full bring-up) succeeded in this
        // process: the precondition sdk_glue.h sets for MDIO access. &mut self keeps a second
        // access of this handle out. &raw mut val points to a live local the glue writes one
        // unsigned to and does not keep. The other arguments are plain integers; dev is 1..=31
        // by the type.
        let rv = unsafe {
            sys::ob_sdk_mdio_c45_read(
                self.raw_unit(),
                phy,
                c_uint::from(mmd.get()),
                c_uint::from(reg),
                &raw mut val,
            )
        };
        check(rv)?;
        // The glue stores a uint16; anything wider would be a foreign glue.
        u16::try_from(val).map_err(|_| Error::INTERNAL)
    }

    /// Writes `val` to register `mmd.reg` of the PHY at address word `phy`.
    pub fn mdio_c45_write(&mut self, phy: u32, mmd: Mmd, reg: u16, val: u16) -> Result<(), Error> {
        // SAFETY: as for the read: self proves the bring-up, &mut self the exclusive use; all
        // arguments are plain integers, dev 1..=31 and reg, val at most ffff by their types.
        let rv = unsafe {
            sys::ob_sdk_mdio_c45_write(
                self.raw_unit(),
                phy,
                c_uint::from(mmd.get()),
                c_uint::from(reg),
                c_uint::from(val),
            )
        };
        check(rv).map(|_| ())
    }
}

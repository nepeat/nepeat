//! Clause-45 MDIO through the switch chip's MIIM master, for PHYs the SDK has no driver for:
//! methods of [`Sdk`] (`&mut`), and [`MdioBus`], a cloneable handle for other threads.

use std::ffi::{c_int, c_uint};
use std::sync::Arc;

use crate::error::{Error, check};
use crate::glue::Glue;
use crate::unit::{Live, Sdk};

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

/// The largest address word the glue takes (`sdk_glue.c` `OB_CHECK_MDIO`): bus bits 9, 8, 6, 5,
/// internal bus bit 7, broadcast bit 10, address 4:0.
pub const PHY_MAX: u32 = 0x7ff;

fn read(glue: &Glue, unit: c_int, phy: u32, mmd: Mmd, reg: u16) -> Result<u16, Error> {
    if phy > PHY_MAX {
        return Err(Error::PARAM);
    }
    let mut val: c_uint = 0;
    check(glue.mdio_read(unit, phy, c_uint::from(mmd.get()), c_uint::from(reg), &mut val))?;
    // The glue stores a uint16; anything wider would be a foreign glue.
    u16::try_from(val).map_err(|_| Error::INTERNAL)
}

fn write(glue: &Glue, unit: c_int, phy: u32, mmd: Mmd, reg: u16, val: u16) -> Result<(), Error> {
    if phy > PHY_MAX {
        return Err(Error::PARAM);
    }
    check(glue.mdio_write(unit, phy, c_uint::from(mmd.get()), c_uint::from(reg), c_uint::from(val))).map(|_| ())
}

impl Sdk {
    /// Reads register `mmd.reg` of the PHY at `phy`, the SDK's address word (bus in bits 9, 8 and
    /// 6:5, internal bus bit 7, address in bits 4:0; at most [`PHY_MAX`]).
    pub fn mdio_c45_read(&mut self, phy: u32, mmd: Mmd, reg: u16) -> Result<u16, Error> {
        read(self.glue(), self.raw_unit(), phy, mmd, reg)
    }

    /// Writes `val` to register `mmd.reg` of the PHY at address word `phy`.
    pub fn mdio_c45_write(&mut self, phy: u32, mmd: Mmd, reg: u16, val: u16) -> Result<(), Error> {
        write(self.glue(), self.raw_unit(), phy, mmd, reg, val)
    }

    /// A handle on the MDIO master for other threads (the link gate on the SDK's link scan
    /// thread, which cannot borrow the switch): [`MdioBus`].
    pub fn mdio_bus(&self) -> MdioBus {
        MdioBus {
            glue: self.glue().clone(),
            unit: self.raw_unit(),
            live: Arc::clone(&self.core.live),
        }
    }
}

/// The switch chip's MDIO master as a handle of its own: `Clone + Send + Sync`, usable from any
/// thread without the switch (what `sai-core`'s `MdioAccess` needs: `read` / `write` through
/// `&self`). Same accesses, answers and faults as [`Sdk::mdio_c45_read`]; the SDK's MIIM lock
/// serialises the bus. It works while the [`Sdk`] (or [`crate::Switch`]) it came from is alive:
/// afterwards every call is refused with [`crate::Code::Init`] without reaching the glue, and
/// dropping the owner waits for a call in progress. So it can never reach the chip during a
/// later bring-up, when the MDIO clock is at its reset value.
#[derive(Debug, Clone)]
pub struct MdioBus {
    glue: Glue,
    unit: c_int,
    live: Arc<Live>,
}

impl MdioBus {
    /// Reads register `mmd.reg` of the PHY at address word `phy`.
    pub fn read(&self, phy: u32, mmd: Mmd, reg: u16) -> Result<u16, Error> {
        self.live
            .with(|| read(&self.glue, self.unit, phy, mmd, reg))
            .unwrap_or(Err(Error::INIT))
    }

    /// Writes `val` to register `mmd.reg` of the PHY at address word `phy`.
    pub fn write(&self, phy: u32, mmd: Mmd, reg: u16, val: u16) -> Result<(), Error> {
        self.live
            .with(|| write(&self.glue, self.unit, phy, mmd, reg, val))
            .unwrap_or(Err(Error::INIT))
    }

    /// Whether the owner is still alive (its calls may reach the glue).
    pub fn is_live(&self) -> bool {
        self.live.with(|| ()).is_some()
    }
}

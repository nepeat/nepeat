//! Attach only (`ob_sdk_attach`): the SDK probes and attaches the unit, nothing else. No chip
//! reset, no "init all", no misc init: ports and forwarding are untouched (a running switch
//! keeps running), the die temperatures can be read, and the over-temperature power-down is
//! available. **No MDIO**: misc init did not run, so the MDIO clock is not what the PHYs need
//! (the type has no MDIO methods). What `openbcm-asic-temp` (`asic_temp.c`) does.

use std::cell::Cell;
use std::marker::PhantomData;

use crate::error::{Error, check};
use crate::glue::Glue;
use crate::unit::{Boot, Core, InitError, Layer, bring_up};

/// The SDK attached to one unit, nothing initialised. One handle per SDK at a time, with
/// [`crate::Sdk`] and [`crate::Switch`]; `Send`, not `Sync`.
#[derive(Debug)]
pub struct Attached {
    pub(crate) core: Core,
    _not_sync: PhantomData<Cell<()>>,
}

impl Attached {
    /// Attaches unit `unit` (`ob_sdk_attach`). On a Tomahawk nothing has initialised since power-on
    /// the glue also programs the S-bus ring map and the PVTMON trim (routing and sensor trim
    /// only). On success: the handle and the glue's line (chip name). Afterwards, in this
    /// process, the real glue cannot be brought up with a config any more (it boots once, here
    /// without one): [`crate::Switch::init`] / [`crate::Sdk::init_mdio`] are refused.
    pub fn attach(unit: i32) -> Result<(Self, String), InitError> {
        Self::attach_with(Glue::process(), unit)
    }

    /// [`Attached::attach`] on a Rust fake chip (its scenario: [`crate::rust_fake::Chip::load`]).
    #[cfg(feature = "rust-fake")]
    pub fn attach_on(chip: &crate::rust_fake::Chip, unit: i32) -> Result<(Self, String), InitError> {
        Self::attach_with(Glue::Fake(chip.clone()), unit)
    }

    fn attach_with(glue: Glue, unit: i32) -> Result<(Self, String), InitError> {
        let (core, message) = bring_up(glue, unit, Layer::Attach, Boot::Attach, |g, err| g.attach(unit, err))?;
        Ok((
            Self {
                core,
                _not_sync: PhantomData,
            },
            message,
        ))
    }

    /// The unit number.
    pub const fn unit(&self) -> i32 {
        self.core.unit
    }

    /// The over-temperature protection of the attach-only reader
    /// (`ob_sdk_th_overtemp_shutdown`): powers the core PLL down exactly as the SDK's Tomahawk
    /// handler does. **Every S-channel access fails afterwards and the chip needs a power
    /// cycle.** Tomahawk only (not TH2): [`crate::Code::Unavail`] elsewhere. The write is
    /// attempted even when the read before it fails (as the SDK's handler).
    pub fn overtemp_shutdown(&mut self) -> Result<(), Error> {
        check(self.core.glue.th_overtemp_shutdown(self.core.unit)).map(|_| ())
    }
}

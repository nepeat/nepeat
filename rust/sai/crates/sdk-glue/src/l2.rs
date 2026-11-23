//! The L2 table: static and learned entries, flushes, the age timer. Destinations ([`Dest`]) are
//! ports or LAGs (SDK trunks), as the glue's `OB_LAG_DEST_BIT | tid`.

use std::ffi::c_int;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::port::c_value;
use crate::switch::Switch;

/// Where an L2 entry, L3 egress object or L2 event points: a port or a LAG (SDK trunk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dest {
    /// An SDK logical port (non-negative, below `OB_LAG_DEST_BIT`).
    Port(i32),
    /// A trunk id (below `OB_LAG_DEST_BIT`).
    Lag(u32),
}

impl Dest {
    /// `OB_LAG_DEST_BIT` of `sdk_glue.h`.
    pub const LAG_BIT: u32 = sys::OB_LAG_DEST_BIT;

    /// The glue's int: the port, or `OB_LAG_DEST_BIT | tid`. `OB_E_PORT` for a port or trunk id
    /// out of range.
    pub fn raw(self) -> Result<c_int, Error> {
        match self {
            Self::Port(p) if p >= 0 && u32::try_from(p).is_ok_and(|p| p & Self::LAG_BIT == 0) => Ok(p),
            Self::Lag(tid) if tid & Self::LAG_BIT == 0 => c_int::try_from(Self::LAG_BIT | tid).map_err(|_| Error::PORT),
            _ => Err(Error::PORT),
        }
    }

    /// The destination of the glue's int; `None` for a negative one.
    pub fn from_raw(raw: c_int) -> Option<Self> {
        let bits = u32::try_from(raw).ok()?;
        Some(if bits & Self::LAG_BIT == 0 {
            Self::Port(raw)
        } else {
            Self::Lag(bits & !Self::LAG_BIT)
        })
    }
}

/// A VLAN id the glue takes as a destination's VLAN: 1..=4094, else `OB_E_PARAM`.
pub(crate) fn vid(vid: u16) -> Result<c_int, Error> {
    if (1..=4094).contains(&vid) {
        Ok(c_int::from(vid))
    } else {
        Err(Error::PARAM)
    }
}

impl Switch {
    /// Adds (or replaces) the entry `mac` / `vid` pointing to `dest`; `is_static`: never aged, not
    /// moved; `discard`: frames to it are dropped.
    pub fn l2_add(&mut self, mac: [u8; 6], vid: u16, dest: Dest, is_static: bool, discard: bool) -> Result<(), Error> {
        let (v, d) = (self::vid(vid)?, dest.raw()?);
        check(self.glue().l2_add(
            self.raw_unit(),
            &mac,
            v,
            d,
            c_int::from(is_static),
            c_int::from(discard),
        ))
        .map(|_| ())
    }

    /// Deletes the entry `mac` / `vid`.
    pub fn l2_delete(&mut self, mac: [u8; 6], vid: u16) -> Result<(), Error> {
        let v = self::vid(vid)?;
        check(self.glue().l2_delete(self.raw_unit(), &mac, v)).map(|_| ())
    }

    /// Flushes entries, by destination and / or VLAN (`None`: any). The SDK deletes dynamic
    /// entries with static ones, never static alone: `statics` without `dynamics` is
    /// [`crate::Code::Unavail`]; neither does nothing. Everything (no destination, no VLAN): every
    /// port and every trunk this process created.
    pub fn l2_flush(
        &mut self,
        dest: Option<Dest>,
        vid: Option<u16>,
        statics: bool,
        dynamics: bool,
    ) -> Result<(), Error> {
        // The glue does not check these (it declares the function unsafe): checked here.
        let d = dest.map_or(Ok(-1), Dest::raw)?;
        let v = vid.map_or(Ok(-1), self::vid)?;
        check(
            self.glue()
                .l2_flush(self.raw_unit(), d, v, c_int::from(statics), c_int::from(dynamics)),
        )
        .map(|_| ())
    }

    /// Sets the age timer in seconds (0: no aging).
    pub fn l2_age_set(&mut self, seconds: u32) -> Result<(), Error> {
        let s = c_value(seconds)?;
        check(self.glue().l2_age_set(self.raw_unit(), s)).map(|_| ())
    }
}

//! Board settings of the switch chip's side of every front-panel port, from the SAI profile:
//! the C++ SAI's "port quirks" (`sai_port.cpp`, `port_quirks_parse` / `port_quirks_apply`),
//! same keys, same values. On the AS7772 the lanes end at an external PHY's system side:
//! plain CAUI, no autoneg, no FEC in the switch chip, and a wider inter-frame gap.
use sai_core::backend::Profile;
use sai_core::error::{Error, Result};
use sdk_glue::{Fec, Switch};
use std::ffi::CString;

/// `SAI_OPENBCM_PORT_INTERFACE`: one of [`sdk_glue::port::INTERFACE_NAMES`].
pub const INTERFACE_KEY: &str = "SAI_OPENBCM_PORT_INTERFACE";
/// `SAI_OPENBCM_PORT_FEC`: `none`, `rs` or `fc`.
pub const FEC_KEY: &str = "SAI_OPENBCM_PORT_FEC";
/// `SAI_OPENBCM_PORT_AUTONEG`: `off` or `on`.
pub const AUTONEG_KEY: &str = "SAI_OPENBCM_PORT_AUTONEG";
/// `SAI_OPENBCM_PORT_IFG`: bit times, a multiple of 8 in 64..=504.
pub const IFG_KEY: &str = "SAI_OPENBCM_PORT_IFG";

/// What the profile fixes for every front-panel port. A field that is `None` is left as the
/// SDK set it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PortQuirks {
    /// Interface type name.
    pub interface: Option<CString>,
    /// FEC in the switch chip.
    pub fec: Option<Fec>,
    /// Autonegotiation.
    pub autoneg: Option<bool>,
    /// Average inter-frame gap in bit times.
    pub ifg: Option<u32>,
}

/// A key's value; as in the C++, an empty value is the same as an absent key.
fn value<'a>(profile: &'a Profile, key: &str) -> Option<&'a str> {
    profile.get(key).filter(|v| !v.is_empty())
}

impl PortQuirks {
    /// Reads the four keys. A value that is not one of the allowed ones is
    /// `InvalidParameter`, with a line on stderr that names it. Touches no chip.
    pub fn parse(profile: &Profile) -> Result<Self> {
        let mut quirks = Self::default();
        if let Some(name) = value(profile, INTERFACE_KEY) {
            if !sdk_glue::port::INTERFACE_NAMES.contains(&name) {
                crate::log(&format!("{INTERFACE_KEY} '{name}': unknown interface type"));
                return Err(Error::InvalidParameter);
            }
            // A name of the table has no NUL byte.
            quirks.interface = Some(CString::new(name).map_err(|_| Error::InvalidParameter)?);
        }
        quirks.fec = match value(profile, FEC_KEY) {
            None => None,
            Some("none") => Some(Fec::None),
            Some("rs") => Some(Fec::Rs),
            Some("fc") => Some(Fec::Fc),
            Some(other) => {
                crate::log(&format!("{FEC_KEY} '{other}': none|rs|fc"));
                return Err(Error::InvalidParameter);
            }
        };
        quirks.autoneg = match value(profile, AUTONEG_KEY) {
            None => None,
            Some("off") => Some(false),
            Some("on") => Some(true),
            Some(other) => {
                crate::log(&format!("{AUTONEG_KEY} '{other}': off|on"));
                return Err(Error::InvalidParameter);
            }
        };
        if let Some(text) = value(profile, IFG_KEY) {
            // Decimal digits only (the C++ strtol also takes leading blanks and a sign).
            let bits = text
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| text.parse::<u32>().ok())
                .flatten()
                .filter(|n| (64..=504).contains(n) && n % 8 == 0);
            let Some(bits) = bits else {
                crate::log(&format!("{IFG_KEY} '{text}': bit times, a multiple of 8 in 64..504"));
                return Err(Error::InvalidParameter);
            };
            quirks.ifg = Some(bits);
        }
        Ok(quirks)
    }

    /// Whether anything is fixed.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Applies the settings to one port, in the order the diag shell's
    /// `port ... an=off if=...` does (as the C++): autoneg, interface, FEC, inter-frame gap.
    /// The first failure is returned with the name of the setting.
    pub(crate) fn apply(
        &self,
        switch: &mut Switch,
        port: i32,
    ) -> core::result::Result<(), (&'static str, sdk_glue::Error)> {
        if let Some(on) = self.autoneg {
            switch.port_autoneg_set(port, on).map_err(|e| ("autoneg", e))?;
        }
        if let Some(name) = &self.interface {
            switch.port_interface_set(port, name).map_err(|e| ("interface", e))?;
        }
        if let Some(fec) = self.fec {
            switch.port_fec_set(port, fec).map_err(|e| ("fec", e))?;
        }
        if let Some(bits) = self.ifg {
            switch.port_ifg_set(port, bits).map_err(|e| ("ifg", e))?;
        }
        Ok(())
    }
}

impl core::fmt::Display for PortQuirks {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let interface = self.interface.as_ref().map_or("(SDK)".into(), |n| n.to_string_lossy());
        let fec = match self.fec {
            None => "(SDK)",
            Some(Fec::None) => "fixed none",
            Some(Fec::Rs) => "fixed rs",
            Some(Fec::Fc) => "fixed fc",
        };
        let autoneg = match self.autoneg {
            None => "(SDK)",
            Some(true) => "fixed on",
            Some(false) => "fixed off",
        };
        write!(f, "interface {interface}, ASIC FEC {fec}, autoneg {autoneg}, IFG ")?;
        match self.ifg {
            None => f.write_str("(SDK)"),
            Some(bits) => write!(f, "{bits} bits"),
        }
    }
}

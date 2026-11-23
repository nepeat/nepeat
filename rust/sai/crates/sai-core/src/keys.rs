//! The SAI profile keys the library reads, by their C++ names (`SAI_OPENBCM_*`, so a NixOS
//! module's `sai.profile` works for either library), with the Rust names kept as aliases
//! (`SAI_RS_*`, until the C++ is gone). Where both are set, the Rust name wins.
//!
//! The C++ (`sai_switch.cpp` `collect_profile`) hands the backend every key the SAI user's
//! `profile_get_next_value` enumerates plus [`EXPLICIT`], asked one by one with
//! `profile_get_value` (for a user that does not enumerate); the rim does the same.
use crate::backend::Profile;
use crate::error::{Error, Result};

/// `SAI_OPENBCM_BACKEND`: `openbcm` (the default where the library has it) or `fake`.
pub const BACKEND: &str = "SAI_OPENBCM_BACKEND";
/// The Rust alias of [`BACKEND`]: `virtual` or `openbcm`.
pub const RS_BACKEND: &str = "SAI_RS_BACKEND";
/// `SAI_OPENBCM_LOG_STDERR=1`: log lines to stderr too (read at `sai_api_initialize`).
pub const LOG_STDERR: &str = "SAI_OPENBCM_LOG_STDERR";
/// The ASIC temperature file (fan control's), `/run/asic/temperature` on the switches.
pub const TEMP_FILE: &str = "SAI_OPENBCM_TEMP_FILE";
/// The Prometheus text file of the ASIC temperatures.
pub const TEMP_PROM: &str = "SAI_OPENBCM_TEMP_PROM";
/// Seconds between temperature reads, 1..=3600 (5).
pub const TEMP_INTERVAL: &str = "SAI_OPENBCM_TEMP_INTERVAL";
/// The SDK unit (0).
pub const UNIT: &str = "SAI_OPENBCM_UNIT";
/// Board quirk: the ports' interface type.
pub const PORT_INTERFACE: &str = "SAI_OPENBCM_PORT_INTERFACE";
/// Board quirk: the ASIC FEC, fixed (`none` / `rs` / `fc`).
pub const PORT_FEC: &str = "SAI_OPENBCM_PORT_FEC";
/// Board quirk: autonegotiation, fixed (`off` / `on`).
pub const PORT_AUTONEG: &str = "SAI_OPENBCM_PORT_AUTONEG";
/// Board quirk: the average inter-frame gap in bit times (64..=504, a multiple of 8).
pub const PORT_IFG: &str = "SAI_OPENBCM_PORT_IFG";
/// The fake / virtual backend on a host with a switch chip (`1`: allowed).
pub const FAKE_ON_HARDWARE: &str = "SAI_OPENBCM_FAKE_ON_HARDWARE";

/// The keys the C++ asks for one by one (`collect_profile`'s list), and those other C++ files
/// read (`SAI_OPENBCM_DIAG_SOCKET`, the external PHY and `MACsec` tables, the log switch),
/// and the Rust aliases.
pub const EXPLICIT: &[&str] = &[
    "SAI_INIT_CONFIG_FILE",
    BACKEND,
    "SAI_OPENBCM_RC_FILE",
    UNIT,
    "SAI_OPENBCM_KNET",
    "SAI_OPENBCM_FAKE_PORTS",
    "SAI_OPENBCM_FAKE_LANES",
    "SAI_OPENBCM_FAKE_SPEED",
    "SAI_OPENBCM_FAKE_LINK",
    FAKE_ON_HARDWARE,
    "SAI_OPENBCM_RIF_VLAN_BASE",
    TEMP_FILE,
    TEMP_PROM,
    TEMP_INTERVAL,
    PORT_INTERFACE,
    PORT_FEC,
    PORT_AUTONEG,
    PORT_IFG,
    // read by other C++ files (sai_diag, sai_extphy, sai_extmacsec, sai_api)
    "SAI_OPENBCM_DIAG_SOCKET",
    "SAI_OPENBCM_EXT_PHY_FILE",
    "SAI_OPENBCM_EXT_MACSEC_FILE",
    LOG_STDERR,
    // the fake's keys the C++ fake reads from the enumerated profile
    "SAI_OPENBCM_FAKE_NETDEV",
    "SAI_OPENBCM_FAKE_WIRE",
    RS_BACKEND,
];

/// Which backend a switch is created on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    /// The in-memory chip (`fake` / `virtual`).
    Virtual,
    /// The Broadcom SDK (`openbcm`).
    Openbcm,
}

/// Why no backend could be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceError {
    /// No key, and this library has no hardware backend (C++: `FAILURE`).
    NoneAsked,
    /// A name this library does not know (C++: `INVALID_PARAMETER`).
    Unknown(String),
}

impl ChoiceError {
    /// The status the C++ answers.
    pub const fn error(&self) -> Error {
        match self {
            Self::NoneAsked => Error::Failure,
            Self::Unknown(_) => Error::InvalidParameter,
        }
    }
}

/// The backend the profile asks for (`sai_switch.cpp` `pick_backend`): [`RS_BACKEND`], else
/// [`BACKEND`]; `fake` and `virtual` name the in-memory chip on either key. No key: the
/// hardware backend where the library has one (`has_hardware`), as the C++ does; else an error
/// (the in-memory chip is never chosen implicitly). Whether the backend is built in is the
/// caller's check.
pub fn backend_choice(profile: &Profile, has_hardware: bool) -> core::result::Result<BackendChoice, ChoiceError> {
    match profile.get_any(&[RS_BACKEND, BACKEND]).filter(|v| !v.is_empty()) {
        None if has_hardware => Ok(BackendChoice::Openbcm),
        None => Err(ChoiceError::NoneAsked),
        Some("fake" | "virtual") => Ok(BackendChoice::Virtual),
        Some("openbcm") => Ok(BackendChoice::Openbcm),
        Some(other) => Err(ChoiceError::Unknown(other.to_owned())),
    }
}

/// The value of a key as the C++ `cfg.get(key)` reads it: absent is empty.
pub fn get<'a>(profile: &'a Profile, key: &str) -> &'a str {
    profile.get(key).unwrap_or_default()
}

/// `std::atoi` of a key's value: leading blanks, an optional sign, digits; anything else ends
/// the number; no digits is 0; absent is `default` (the C++ `cfg.get(key, default)`).
pub fn atoi(profile: &Profile, key: &str, default: i64) -> i64 {
    profile.get(key).map_or(default, atoi_text)
}

/// `std::atoi` of a text (saturating instead of undefined on overflow).
pub fn atoi_text(text: &str) -> i64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'-') => (-1, text.get(1..).unwrap_or_default()),
        Some(b'+') => (1, text.get(1..).unwrap_or_default()),
        _ => (1, text),
    };
    digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0_i64, |n, d| n.saturating_mul(10).saturating_add(i64::from(d - b'0')))
        .saturating_mul(sign)
}

/// A strict decimal number (the C++ `strtol` with an end check): `None` if anything but an
/// optional sign and digits is there.
pub fn strict_number(text: &str) -> Result<i64> {
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'-') => (-1, text.get(1..).unwrap_or_default()),
        Some(b'+') => (1, text.get(1..).unwrap_or_default()),
        _ => (1, text.trim_start()),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidParameter);
    }
    digits
        .parse::<i64>()
        .map(|n| n * sign)
        .map_err(|_| Error::InvalidParameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_is_chosen_as_the_cpp_chooses_it() {
        let p = |pairs: &[(&str, &str)]| Profile::from_pairs(pairs.iter().copied());
        assert_eq!(backend_choice(&p(&[]), true), Ok(BackendChoice::Openbcm));
        assert_eq!(backend_choice(&p(&[]), false), Err(ChoiceError::NoneAsked));
        assert_eq!(backend_choice(&p(&[(BACKEND, "")]), false), Err(ChoiceError::NoneAsked));
        assert_eq!(
            backend_choice(&p(&[(BACKEND, "fake")]), false),
            Ok(BackendChoice::Virtual)
        );
        assert_eq!(
            backend_choice(&p(&[(RS_BACKEND, "virtual")]), false),
            Ok(BackendChoice::Virtual)
        );
        assert_eq!(
            backend_choice(&p(&[(BACKEND, "fake"), (RS_BACKEND, "openbcm")]), false),
            Ok(BackendChoice::Openbcm)
        );
        assert_eq!(
            backend_choice(&p(&[(BACKEND, "bogus")]), true).map_err(|e| e.error()),
            Err(Error::InvalidParameter)
        );
        assert_eq!(ChoiceError::NoneAsked.error(), Error::Failure);
    }

    #[test]
    fn numbers_parse_as_atoi_and_strtol() {
        assert_eq!(atoi_text("  12x"), 12);
        assert_eq!(atoi_text("-3"), -3);
        assert_eq!(atoi_text("x"), 0);
        assert_eq!(atoi(&Profile::default(), TEMP_INTERVAL, 5), 5);
        assert_eq!(strict_number("128"), Ok(128));
        assert_eq!(strict_number("128x"), Err(Error::InvalidParameter));
        assert_eq!(strict_number(""), Err(Error::InvalidParameter));
    }
}

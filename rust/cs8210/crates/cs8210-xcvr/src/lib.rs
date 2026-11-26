//! Pluggable module memory maps for the CS8210-32X's cages, as plain data and pure
//! functions (no I/O): [`cmis`] (QSFP-DD / OSFP / QSFP+ with CMIS 4.0-5.x) and
//! [`sff8636`] (QSFP28 / QSFP+) identity and monitors, [`memory`] (optoe's flat
//! offset layout -> page select + offsets, as `SONiC`'s `sonic_xcvr` expects),
//! [`bringup`] (the CMIS module / data path state machine as decisions over a
//! [`bringup::ModuleIo`]) and [`codes`] (SFF-8024 tables).
//!
//! Offsets follow the specifications and were cross-checked against sonic-platform-common's
//! `sonic_xcvr` memory maps (the code `SONiC`'s xcvrd runs on the same bytes).
#![forbid(unsafe_code)]

pub mod bringup;
pub mod cmis;
pub mod codes;
pub mod dom;
#[doc(hidden)]
pub mod lab;
pub mod memory;
pub mod sff8636;
pub mod sim;

/// ASCII field, trailing spaces and NULs removed.
#[must_use]
pub fn ascii(bytes: &[u8]) -> String {
    let s: String = bytes
        .iter()
        .map(|&b| if (0x20..0x7f).contains(&b) { char::from(b) } else { '?' })
        .collect();
    s.trim_end_matches([' ', '?']).trim_start().to_string()
}

/// Big-endian u16 at `off` of `b`, if inside.
#[must_use]
pub fn be16(b: &[u8], off: usize) -> Option<u16> {
    let hi = *b.get(off)?;
    let lo = *b.get(off + 1)?;
    Some(u16::from_be_bytes([hi, lo]))
}

/// Byte `off` of `b`, if inside.
#[must_use]
pub fn byte(b: &[u8], off: usize) -> Option<u8> {
    b.get(off).copied()
}

//! optoe's flat EEPROM layout (what `sonic_xcvr` reads through `read_eeprom(offset, n)`)
//! mapped onto I2C transfers: page select (byte 127) + register offsets.
//!
//! QSFP / CMIS (one I2C address, 0x50): 0-127 lower page, 128-255 upper page 00h, upper
//! page n at `128 + 128 * n` (only on a paged module; bank 0). SFP (SFF-8472): A0h at
//! 0-255, A2h (address 0x51) at 256-511, no paging.

use serde::Serialize;

/// Two-wire address of the module memory (A0h).
pub const ADDR_A0: u8 = 0x50;
/// SFF-8472 diagnostics (A2h).
pub const ADDR_A2: u8 = 0x51;
/// Page select register (QSFP / CMIS).
pub const PAGE_SELECT: u8 = 127;
/// Size of a page (and of the lower page).
pub const PAGE: usize = 128;

/// Which memory map a module speaks, from identifier byte 0 and byte 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "spec", rename_all = "kebab-case")]
pub enum Kind {
    /// CMIS (QSFP-DD 0x18, OSFP 0x19, QSFP+ with CMIS 0x1e, OSFP-XD 0x21).
    Cmis {
        /// flat memory (byte 2 bit 7): no upper pages beyond 00h, no page select
        flat: bool,
    },
    /// SFF-8636 / SFF-8436 (QSFP 0x0c, QSFP+ 0x0d, QSFP28 0x11).
    Sff8636 {
        /// flat memory (byte 2 bit 2)
        flat: bool,
    },
    /// SFF-8472 (SFP 0x03, e.g. in a QSA adapter).
    Sfp,
    /// Anything else: lower and upper page 00h only.
    Other {
        /// identifier byte
        id: u8,
    },
}

impl Kind {
    /// From identifier byte 0 and status byte 2.
    #[must_use]
    pub fn from_bytes(id: u8, byte2: u8) -> Self {
        match id {
            0x18 | 0x19 | 0x1e | 0x21 => Self::Cmis {
                flat: byte2 & 0x80 != 0,
            },
            0x0c | 0x0d | 0x11 => Self::Sff8636 {
                flat: byte2 & 0x04 != 0,
            },
            0x03 => Self::Sfp,
            _ => Self::Other { id },
        }
    }

    /// Upper pages beyond 00h exist (page select works).
    #[must_use]
    pub fn paged(self) -> bool {
        matches!(self, Self::Cmis { flat: false } | Self::Sff8636 { flat: false })
    }
}

/// One transfer of a flat-offset access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// two-wire address
    pub addr: u8,
    /// page to select first (None: lower page or a module without paging)
    pub page: Option<u8>,
    /// register offset (0..=255)
    pub offset: u8,
    /// bytes (1..=128, never crossing a page)
    pub len: u8,
}

/// An offset the module has no memory at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beyond(pub usize);

impl std::fmt::Display for Beyond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "offset {} is beyond the module's memory", self.0)
    }
}

impl std::error::Error for Beyond {}

/// The transfers for `len` bytes at flat `offset`.
pub fn segments(kind: Kind, offset: usize, len: usize) -> Result<Vec<Segment>, Beyond> {
    let mut out = Vec::new();
    let mut pos = offset;
    let end = offset.checked_add(len).ok_or(Beyond(offset))?;
    while pos < end {
        let n = (end.min((pos / PAGE + 1) * PAGE) - pos).min(PAGE);
        let seg = |addr: u8, page: Option<u8>, reg: usize| -> Result<Segment, Beyond> {
            Ok(Segment {
                addr,
                page,
                offset: u8::try_from(reg).map_err(|_| Beyond(pos))?,
                len: u8::try_from(n).map_err(|_| Beyond(pos))?,
            })
        };
        let s = match kind {
            Kind::Sfp if pos < 2 * PAGE => seg(ADDR_A0, None, pos)?,
            Kind::Sfp if pos < 4 * PAGE => seg(ADDR_A2, None, pos - 2 * PAGE)?,
            _ if pos < PAGE => seg(ADDR_A0, None, pos)?,
            k if pos < 2 * PAGE => seg(ADDR_A0, k.paged().then_some(0), pos)?,
            k if k.paged() => {
                let page = u8::try_from(pos / PAGE - 1).map_err(|_| Beyond(pos))?;
                seg(ADDR_A0, Some(page), PAGE + pos % PAGE)?
            }
            _ => return Err(Beyond(pos)),
        };
        out.push(s);
        pos += n;
    }
    Ok(out)
}

/// Flat offset of byte `reg` (128..=255) of upper page `page`.
#[must_use]
pub fn flat(page: u8, reg: u8) -> usize {
    if reg < 128 {
        usize::from(reg)
    } else {
        PAGE * (usize::from(page) + 1) + usize::from(reg - 128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGED: Kind = Kind::Cmis { flat: false };

    #[test]
    fn kinds() {
        assert_eq!(Kind::from_bytes(0x18, 0x04), PAGED);
        assert_eq!(Kind::from_bytes(0x18, 0x80), Kind::Cmis { flat: true });
        assert_eq!(Kind::from_bytes(0x11, 0x04), Kind::Sff8636 { flat: true });
        assert_eq!(Kind::from_bytes(0x03, 0), Kind::Sfp);
        assert!(!Kind::from_bytes(0x11, 0x04).paged());
    }

    #[test]
    fn paged_module_like_optoe() {
        let s = segments(PAGED, 0, 256).unwrap();
        assert_eq!(
            s,
            vec![
                Segment {
                    addr: 0x50,
                    page: None,
                    offset: 0,
                    len: 128
                },
                Segment {
                    addr: 0x50,
                    page: Some(0),
                    offset: 128,
                    len: 128
                }
            ]
        );
        // page 11h bytes 128..=131 (data path state) = flat 2304
        assert_eq!(flat(0x11, 128), 2304);
        assert_eq!(
            segments(PAGED, 2304, 4).unwrap(),
            vec![Segment {
                addr: 0x50,
                page: Some(0x11),
                offset: 128,
                len: 4
            }]
        );
        // across the page 01h / 02h boundary
        let s = segments(PAGED, flat(1, 250), 10).unwrap();
        assert_eq!(
            s[0],
            Segment {
                addr: 0x50,
                page: Some(1),
                offset: 250,
                len: 6
            }
        );
        assert_eq!(
            s[1],
            Segment {
                addr: 0x50,
                page: Some(2),
                offset: 128,
                len: 4
            }
        );
    }

    #[test]
    fn flat_and_sfp() {
        let f = Kind::Cmis { flat: true };
        assert_eq!(
            segments(f, 200, 10).unwrap(),
            vec![Segment {
                addr: 0x50,
                page: None,
                offset: 200,
                len: 10
            }]
        );
        assert!(segments(f, 256, 1).is_err());
        let s = segments(Kind::Sfp, 250, 10).unwrap();
        assert_eq!(
            s[0],
            Segment {
                addr: 0x50,
                page: None,
                offset: 250,
                len: 6
            }
        );
        assert_eq!(
            s[1],
            Segment {
                addr: 0x51,
                page: None,
                offset: 0,
                len: 4
            }
        );
        assert!(segments(Kind::Sfp, 512, 1).is_err());
        assert!(segments(PAGED, 128 + 128 * 256, 1).is_err());
    }
}

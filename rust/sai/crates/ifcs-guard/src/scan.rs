//! The inject file's `fscanf(f, "%x %u", &off, &cnt)` of `ifcs_guard.c`, as glibc's scanf reads it.
//!
//! What glibc does (vfscanf-internal.c, numeric conversions): leading white space is skipped
//! (`isspace` in the C locale); an optional `+` / `-`; a leading `0` is a digit, and for `%x` an
//! `x` / `X` right after it is consumed too (also when no hex digit follows: "0x" reads as 0); then
//! digits of the base. A conversion with no digit (only a sign, or nothing) fails; end of input before
//! the first conversion is EOF (-1). The digits go through `strtoul`: a magnitude above
//! `u64::MAX` gives `u64::MAX` whatever the sign, a minus negates modulo 2^64, and the store into an
//! `unsigned int` keeps the low 32 bits. The literal space of the format matches any white space,
//! none included. The character after a number is pushed back (one character of look-ahead).

use std::iter::Peekable;

/// The result of the scan: what `fscanf` returned and stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scan {
    /// `fscanf`'s return value: conversions stored (0..=2), or -1 (EOF before the first one).
    pub got: i32,
    /// The `%x` value (0 unless `got >= 1`).
    pub off: u32,
    /// The `%u` value (0 unless `got == 2`).
    pub cnt: u32,
}

/// Why a conversion stored nothing.
enum Fail {
    /// End of input before the conversion started.
    Eof,
    /// The input does not start with a number.
    Matching,
}

const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn digit(c: u8, base: u32) -> Option<u32> {
    let d = char::from(c).to_digit(base)?;
    Some(d)
}

fn skip_space<I: Iterator<Item = u8>>(it: &mut Peekable<I>) {
    while it.next_if(|&c| is_space(c)).is_some() {}
}

/// One `%x` (base 16) or `%u` (base 10) conversion, stored into an `unsigned int`.
fn number<I: Iterator<Item = u8>>(it: &mut Peekable<I>, base: u32) -> Result<u32, Fail> {
    skip_space(it);
    if it.peek().is_none() {
        return Err(Fail::Eof);
    }
    let negative = matches!(it.next_if(|&c| c == b'-' || c == b'+'), Some(b'-'));
    let mut digits = 0_usize;
    let mut value = 0_u64;
    let mut overflow = false;
    if it.next_if_eq(&b'0').is_some() {
        digits += 1;
        if base == 16 {
            let _ = it.next_if(|&c| c == b'x' || c == b'X');
        }
    }
    while let Some(d) = it.peek().and_then(|&c| digit(c, base)) {
        let _ = it.next();
        digits += 1;
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
    }
    if digits == 0 {
        return Err(Fail::Matching);
    }
    let ul = if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    // the store into an unsigned int: the low 32 bits
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the C stores strtoul's result into an unsigned int"
    )]
    Ok(ul as u32)
}

/// `fscanf(f, "%x %u", &off, &cnt)` over the bytes of the file.
pub fn scan_inject<I: Iterator<Item = u8>>(bytes: I) -> Scan {
    let mut it = bytes.peekable();
    let mut scan = Scan { got: 0, off: 0, cnt: 0 };
    match number(&mut it, 16) {
        Ok(v) => {
            scan.off = v;
            scan.got = 1;
        }
        Err(Fail::Eof) => {
            scan.got = -1;
            return scan;
        }
        Err(Fail::Matching) => return scan,
    }
    // the format's space, then %u (which skips white space itself)
    skip_space(&mut it);
    if let Ok(v) = number(&mut it, 10) {
        scan.cnt = v;
        scan.got = 2;
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::{Scan, scan_inject};

    fn s(text: &str) -> Scan {
        scan_inject(text.bytes())
    }

    #[test]
    fn the_forms_the_hook_documents() {
        assert_eq!(
            s("4860 3"),
            Scan {
                got: 2,
                off: 0x4860,
                cnt: 3
            }
        );
        assert_eq!(
            s("0x4860 3\n"),
            Scan {
                got: 2,
                off: 0x4860,
                cnt: 3
            }
        );
        assert_eq!(
            s("  0X485c\t\n  10"),
            Scan {
                got: 2,
                off: 0x485c,
                cnt: 10
            }
        );
    }

    #[test]
    fn failures_and_partial_conversions() {
        assert_eq!(s("").got, -1);
        assert_eq!(s("   \n").got, -1);
        assert_eq!(s("zz 3").got, 0);
        assert_eq!(s("-").got, 0);
        assert_eq!(s("4860").got, 1);
        assert_eq!(s("4860 x").got, 1);
        assert_eq!(s("4860 -").got, 1);
    }

    #[test]
    fn glibc_details() {
        // "0x" with no hex digit: 0, the x consumed
        assert_eq!(s("0x 5"), Scan { got: 2, off: 0, cnt: 5 });
        // %u does not take a 0x prefix: "0" then the x stops the number
        assert_eq!(s("1 0x5"), Scan { got: 2, off: 1, cnt: 0 });
        // no separator needed: the space of the format matches none
        assert_eq!(
            s("4860g"),
            Scan {
                got: 1,
                off: 0x4860,
                cnt: 0
            }
        );
        assert_eq!(
            s("486 07"),
            Scan {
                got: 2,
                off: 0x486,
                cnt: 7
            }
        );
        // signs: negated modulo 2^64, then the low 32 bits
        assert_eq!(
            s("-1 -2"),
            Scan {
                got: 2,
                off: u32::MAX,
                cnt: u32::MAX - 1
            }
        );
        assert_eq!(
            s("+10 +2"),
            Scan {
                got: 2,
                off: 0x10,
                cnt: 2
            }
        );
        // strtoul's range: above u64::MAX saturates; above u32::MAX keeps the low bits
        assert_eq!(
            s("100004860 1"),
            Scan {
                got: 2,
                off: 0x4860,
                cnt: 1
            }
        );
        assert_eq!(
            s("ffffffffffffffffff 1"),
            Scan {
                got: 2,
                off: u32::MAX,
                cnt: 1
            }
        );
        assert_eq!(
            s("1 99999999999999999999999"),
            Scan {
                got: 2,
                off: 1,
                cnt: u32::MAX
            }
        );
        assert_eq!(s("1 4294967297"), Scan { got: 2, off: 1, cnt: 1 });
    }
}

//! The C library's number and address readers as `sai_script.cpp` uses them: `std::stoll` /
//! `stoul` / `stoi` (base 0 / 10, leading blanks, trailing text ignored), `sscanf("%x:%x:...")` for
//! MACs, `inet_pton`.
#![forbid(unsafe_code)]

use std::net::{Ipv4Addr, Ipv6Addr};

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `strtoull`-style digits of `base` at the start of `s`: the value (wrapping, `None` on
/// overflow) and how many bytes were read.
fn digits(s: &[u8], base: u32) -> (Option<u64>, usize) {
    let mut v: Option<u64> = Some(0);
    let mut n = 0;
    for &c in s {
        let Some(d) = char::from(c).to_digit(base) else { break };
        v = v
            .and_then(|x| x.checked_mul(u64::from(base)))
            .and_then(|x| x.checked_add(u64::from(d)));
        n += 1;
    }
    (v, n)
}

/// Sign, base prefix and digits after leading blanks: (negative, magnitude, digits read).
#[allow(clippy::many_single_char_names, reason = "strtol's locals")]
fn signed(s: &str, base0: bool) -> Option<(bool, Option<u64>)> {
    let b = s.as_bytes();
    let mut i = b.iter().take_while(|c| is_space(**c)).count();
    let neg = match b.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let rest = b.get(i..).unwrap_or_default();
    let (base, skip) = if base0 {
        match rest {
            [b'0', b'x' | b'X', c, ..] if char::from(*c).is_ascii_hexdigit() => (16, 2),
            [b'0', ..] => (8, 0),
            _ => (10, 0),
        }
    } else {
        (10, 0)
    };
    let (v, n) = digits(rest.get(skip..).unwrap_or_default(), base);
    if n == 0 {
        return None;
    }
    Some((neg, v))
}

/// `std::stoll(s, nullptr, 0)`: `None` where it throws (no digits, out of range).
pub fn stoll(s: &str) -> Option<i64> {
    let (neg, v) = signed(s, true)?;
    let v = v?;
    if neg {
        if v <= i64::MIN.unsigned_abs() {
            Some(0i64.wrapping_sub_unsigned(v))
        } else {
            None
        }
    } else {
        i64::try_from(v).ok()
    }
}

/// `std::stoul(s)` (base 10; a minus sign negates, wrapping): `None` where it throws.
pub fn stoul(s: &str) -> Option<u64> {
    let (neg, v) = signed(s, false)?;
    let v = v?;
    Some(if neg { v.wrapping_neg() } else { v })
}

/// `std::stoi(s)` (base 10): `None` where it throws.
pub fn stoi(s: &str) -> Option<i32> {
    let (neg, v) = signed(s, false)?;
    let v = i64::try_from(v?).ok()?;
    i32::try_from(if neg { -v } else { v }).ok()
}

/// `sscanf(s, "%x:%x:%x:%x:%x:%x", ...) == 6`, each value cut to a byte.
#[allow(clippy::cast_possible_truncation, reason = "(uint8_t)b[i]")]
#[allow(clippy::many_single_char_names, reason = "sscanf's locals")]
pub fn mac(s: &str) -> Option<[u8; 6]> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = [0u8; 6];
    for (k, o) in out.iter_mut().enumerate() {
        if k > 0 {
            if b.get(i) != Some(&b':') {
                return None;
            }
            i += 1;
        }
        i += b
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| is_space(**c))
            .count();
        let neg = match b.get(i) {
            Some(b'-') => {
                i += 1;
                true
            }
            Some(b'+') => {
                i += 1;
                false
            }
            _ => false,
        };
        if matches!(b.get(i..i + 2), Some([b'0', b'x' | b'X'])) && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) {
            i += 2;
        }
        let (v, n) = digits(b.get(i..).unwrap_or_default(), 16);
        if n == 0 {
            return None;
        }
        i += n;
        let v = v.unwrap_or(u64::MAX) as u32;
        *o = (if neg { v.wrapping_neg() } else { v }) as u8;
    }
    Some(out)
}

/// `inet_pton(AF_INET, ...)`
pub fn ip4(s: &str) -> Option<[u8; 4]> {
    s.parse::<Ipv4Addr>().ok().map(|a| a.octets())
}

/// `inet_pton(AF_INET6, ...)`
pub fn ip6(s: &str) -> Option<[u8; 16]> {
    s.parse::<Ipv6Addr>().ok().map(|a| a.octets())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_the_c_library() {
        assert_eq!(stoll("0x10"), Some(16));
        assert_eq!(stoll(" 010"), Some(8));
        assert_eq!(stoll("-5abc"), Some(-5));
        assert_eq!(stoll("abc"), None);
        assert_eq!(stoll("99999999999999999999"), None);
        assert_eq!(stoul("3]"), Some(3));
        assert_eq!(stoul("x"), None);
        assert_eq!(stoi("24"), Some(24));
        assert_eq!(mac("02:00:00:00:53:01"), Some([2, 0, 0, 0, 0x53, 1]));
        assert_eq!(mac("0x2:1:2:3:4:5"), Some([2, 1, 2, 3, 4, 5]));
        assert_eq!(mac("02:00:00"), None);
        assert_eq!(ip4("10.0.0.1"), Some([10, 0, 0, 1]));
        assert_eq!(ip4("10.0.0.01"), None);
        assert!(ip6("2001:db8::1").is_some());
    }
}

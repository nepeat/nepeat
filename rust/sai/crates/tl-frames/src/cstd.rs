//! The C library behaviours the tools' command lines and output depend on: `atoi`, `atof`,
//! `strtoul(.., 0)`, `sscanf("%x:%x:..")`, `strtok` on commas, `perror` text and `printf` of
//! floating point (including `nan` / `-nan`).

use std::io;

/// `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `strtol(s, NULL, 10)` saturated at the 64-bit limits.
fn strtol10(s: &str) -> i64 {
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
    let mut v: i64 = 0;
    for c in b.iter().skip(i).take_while(|c| c.is_ascii_digit()) {
        let d = i64::from(c - b'0');
        v = if neg {
            v.saturating_mul(10).saturating_sub(d)
        } else {
            v.saturating_mul(10).saturating_add(d)
        };
    }
    v
}

/// `atoi`: the leading decimal integer (0 without one), as a C `int` (the `long` truncated).
#[allow(clippy::cast_possible_truncation)]
pub fn atoi(s: &str) -> i32 {
    strtol10(s) as i32
}

/// `atoi` converted with a C `(unsigned)` cast (a negative value wraps).
#[allow(clippy::cast_sign_loss)]
pub fn atou(s: &str) -> u32 {
    atoi(s) as u32
}

/// `strtoul(s, NULL, 0)`: decimal, `0x` hex or leading-`0` octal; saturates; a minus sign negates
/// modulo 2^64.
pub fn strtoul0(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = b.iter().take_while(|c| is_space(**c)).count();
    let mut neg = false;
    match b.get(i) {
        Some(b'-') => {
            neg = true;
            i += 1;
        }
        Some(b'+') => i += 1,
        _ => {}
    }
    let mut base = 10u32;
    let rest = b.get(i..).unwrap_or_default();
    if rest.first() == Some(&b'0') {
        if matches!(rest.get(1), Some(b'x' | b'X')) && rest.get(2).is_some_and(u8::is_ascii_hexdigit) {
            base = 16;
            i += 2;
        } else {
            base = 8;
        }
    }
    let mut v: u64 = 0;
    for c in b.get(i..).unwrap_or_default() {
        let Some(d) = char::from(*c).to_digit(base) else { break };
        v = v.saturating_mul(u64::from(base)).saturating_add(u64::from(d));
    }
    if neg { v.wrapping_neg() } else { v }
}

/// `atof`: the longest leading decimal floating point number (0.0 without one); `inf` and
/// `nan` are understood, hexadecimal floats are not.
pub fn atof(s: &str) -> f64 {
    let t = s.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let lower = t.get(i..).unwrap_or_default().to_ascii_lowercase();
    for word in ["infinity", "inf", "nan"] {
        if lower.starts_with(word) {
            return t.get(..i + word.len()).and_then(|p| p.parse().ok()).unwrap_or(0.0);
        }
    }
    let digits = |from: usize| b.iter().skip(from).take_while(|c| c.is_ascii_digit()).count();
    let int = digits(i);
    i += int;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        frac = digits(i + 1);
        if int + frac > 0 {
            i += 1 + frac;
        }
    }
    if int + frac == 0 {
        return 0.0;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let e = digits(j);
        if e > 0 {
            i = j + e;
        }
    }
    t.get(..i).and_then(|p| p.parse().ok()).unwrap_or(0.0)
}

/// One `%x` conversion of `sscanf`: skips white space, takes an optional `0x`, then hex digits;
/// `None` when there is no digit. Advances `s`.
fn scan_hex(s: &mut &[u8]) -> Option<u32> {
    let skip = s.iter().take_while(|c| is_space(**c)).count();
    let mut rest = s.get(skip..).unwrap_or_default();
    if rest.first() == Some(&b'0')
        && matches!(rest.get(1), Some(b'x' | b'X'))
        && rest.get(2).is_some_and(u8::is_ascii_hexdigit)
    {
        rest = rest.get(2..).unwrap_or_default();
    }
    let n = rest.iter().take_while(|c| c.is_ascii_hexdigit()).count();
    if n == 0 {
        return None;
    }
    let mut v: u32 = 0;
    for c in rest.iter().take(n) {
        v = v
            .wrapping_mul(16)
            .wrapping_add(char::from(*c).to_digit(16).unwrap_or(0));
    }
    *s = rest.get(n..).unwrap_or_default();
    Some(v)
}

/// `sscanf(s, "%x:%x:%x:%x:%x:%x", ..) == 6` with each value cut to a byte; trailing text is
/// ignored.
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut rest = s.as_bytes();
    let mut mac = [0u8; 6];
    for (k, slot) in mac.iter_mut().enumerate() {
        if k > 0 {
            if rest.first() != Some(&b':') {
                return None;
            }
            rest = rest.get(1..).unwrap_or_default();
        }
        *slot = scan_hex(&mut rest)?.to_le_bytes()[0];
    }
    Some(mac)
}

/// `strtok(s, ",")`: the non-empty comma-separated tokens.
pub fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split(',').filter(|t| !t.is_empty())
}

/// `printf("%.*f")` of a double, `nan` / `-nan` / `inf` as glibc prints them.
pub fn fmt_f(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return if x.is_sign_negative() {
            "-nan".into()
        } else {
            "nan".into()
        };
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{x:.prec$}")
}

/// What `perror` prints after the prefix: `strerror(errno)`.
pub fn strerror(e: &io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(p) => s.get(..p).unwrap_or(&s).to_owned(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoi_like_c() {
        assert_eq!(atoi("12abc"), 12);
        assert_eq!(atoi("  -7"), -7);
        assert_eq!(atoi("+3"), 3);
        assert_eq!(atoi("abc"), 0);
        assert_eq!(atoi(""), 0);
        assert_eq!(atou("-1"), u32::MAX);
        assert_eq!(atoi("4294967296"), 0);
    }

    #[test]
    fn strtoul_bases() {
        assert_eq!(strtoul0("100000"), 100_000);
        assert_eq!(strtoul0("0x10"), 16);
        assert_eq!(strtoul0("010"), 8);
        assert_eq!(strtoul0("0"), 0);
        assert_eq!(strtoul0("12x"), 12);
        assert_eq!(strtoul0("-1"), u64::MAX);
        assert_eq!(strtoul0("99999999999999999999999"), u64::MAX);
    }

    #[test]
    fn atof_like_c() {
        assert!((atof("100") - 100.0).abs() < f64::EPSILON);
        assert!((atof("2.5e2x") - 250.0).abs() < f64::EPSILON);
        assert!((atof(".5") - 0.5).abs() < f64::EPSILON);
        assert!((atof("5.") - 5.0).abs() < f64::EPSILON);
        assert!(atof("zz").abs() < f64::EPSILON);
        assert_eq!(atof("1e").to_bits(), 1.0f64.to_bits());
        assert!(atof("-inf").is_infinite());
        assert!(atof("nan").is_nan());
    }

    #[test]
    fn mac_like_sscanf() {
        assert_eq!(parse_mac("02:00:00:00:01:20"), Some([2, 0, 0, 0, 1, 0x20]));
        assert_eq!(parse_mac("2:0:0:0:1:aB"), Some([2, 0, 0, 0, 1, 0xab]));
        assert_eq!(parse_mac("02:00:00:00:01:20:99"), Some([2, 0, 0, 0, 1, 0x20]));
        assert_eq!(parse_mac("02:00:00:00:01"), None);
        assert_eq!(parse_mac("02-00-00-00-01-20"), None);
        assert_eq!(parse_mac("0x2:0:0:0:1:20"), Some([2, 0, 0, 0, 1, 0x20]));
        assert_eq!(parse_mac("102:0:0:0:0:0"), Some([2, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn strtok() {
        assert_eq!(tokens("a,,b,").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(tokens(",").count(), 0);
    }

    #[test]
    fn floats_print_like_glibc() {
        assert_eq!(fmt_f(1.0 / 3.0, 3), "0.333");
        assert_eq!(fmt_f(0.5, 0), "0");
        assert_eq!(fmt_f(1.5, 0), "2");
        assert_eq!(fmt_f(f64::NAN, 2), "nan");
        assert_eq!(fmt_f(-f64::NAN, 2), "-nan");
        assert_eq!(fmt_f(f64::INFINITY, 1), "inf");
        assert_eq!(fmt_f(-0.0, 1), "-0.0");
    }

    #[test]
    fn perror_text() {
        let e = io::Error::from_raw_os_error(19);
        assert_eq!(strerror(&e), "No such device");
    }
}

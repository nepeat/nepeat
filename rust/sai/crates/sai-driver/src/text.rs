//! Words, numbers, addresses: the C++ driver's text rules (`words`, `num`, `hexnum`,
//! `parse_mac`, `parse_ip`, `parse_prefix`, ...), the C library's `strtoll` / `sscanf` /
//! `inet_pton` semantics included where a script can tell.
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why a script line was not run (`= PARSE_ERROR <reason>`; nothing was called).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

/// A parse result.
pub type Parsed<T> = Result<T, ParseError>;

/// `fail(...)`: a parse error with that reason.
pub fn fail<T>(why: impl Into<String>) -> Parsed<T> {
    Err(ParseError(why.into()))
}

/// C `isspace` in the "C" locale.
const fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// A line split into words at blanks; a double-quoted part may contain blanks and stays in the
/// word with its quotes (C++ `words`).
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| is_space(*c)).is_some() {}
        if chars.peek().is_none() {
            return out;
        }
        let mut word = String::new();
        while let Some(c) = chars.next_if(|c| !is_space(*c)) {
            word.push(c);
            if c == '"' {
                while let Some(q) = chars.next_if(|q| *q != '"') {
                    word.push(q);
                }
                if let Some(q) = chars.next() {
                    word.push(q);
                }
            }
        }
        out.push(word);
    }
}

/// `s` split at `sep` outside `[...]` (C++ `split`).
pub fn split(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        if c == '[' {
            depth += 1;
        }
        if c == ']' {
            depth -= 1;
        }
        if c == sep && depth == 0 {
            out.push(core::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// What `strtoll` / `strtoull` accept of a whole string: sign and magnitude, or `None` when
/// characters are left over (or there are no digits). `base` 0: `0x` hex, `0` octal, else
/// decimal.
fn c_integer(s: &str, base: u32) -> Option<(bool, u128)> {
    let s = s.trim_start_matches(is_space);
    let (negative, s) = match s.as_bytes().first() {
        Some(b'-') => (true, s.get(1..)?),
        Some(b'+') => (false, s.get(1..)?),
        _ => (false, s),
    };
    let hex_prefix = (s.starts_with("0x") || s.starts_with("0X"))
        && s.get(2..)
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_ascii_hexdigit());
    let (radix, digits) = match base {
        16 if hex_prefix => (16, s.get(2..)?),
        16 => (16, s),
        _ if hex_prefix => (16, s.get(2..)?),
        _ if s.starts_with('0') => (8, s),
        _ => (10, s),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: u128 = 0;
    for c in digits.chars() {
        let d = c.to_digit(radix)?;
        // saturate far above u64: the caller only needs "did it overflow"
        value = value
            .saturating_mul(u128::from(radix))
            .saturating_add(u128::from(d))
            .min(u128::MAX >> 1);
    }
    Some((negative, value))
}

/// A number: decimal, `0x` hex or `0` octal, signed, or else read as unsigned (C++ `num`:
/// `strtoll`, then `strtoull`, both base 0).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "the C driver's (long long) of strtoull's result"
)]
pub fn num(s: &str) -> Parsed<i64> {
    if s.is_empty() {
        return fail("empty number");
    }
    let Some((negative, magnitude)) = c_integer(s, 0) else {
        return fail(format!("bad number '{s}'"));
    };
    let limit: u128 = if negative { 1 << 63 } else { (1 << 63) - 1 };
    if magnitude <= limit {
        let m = magnitude as i128;
        return Ok((if negative { -m } else { m }) as i64);
    }
    // strtoull: ULLONG_MAX on overflow (the driver does not look at errno there)
    let u = u64::try_from(magnitude).unwrap_or(u64::MAX);
    Ok((if negative { u.wrapping_neg() } else { u }) as i64)
}

/// A hex number (C++ `hexnum`: `strtoll(s, 16)`, saturating).
#[allow(clippy::cast_possible_truncation, reason = "clamped to the i64 range first")]
pub fn hexnum(s: &str) -> Parsed<i64> {
    let Some((negative, magnitude)) = c_integer(s, 16).filter(|_| !s.is_empty()) else {
        return fail(format!("bad hex number '{s}'"));
    };
    let m = magnitude.min(1 << 63).cast_signed();
    Ok(if negative {
        (-m).max(i128::from(i64::MIN)) as i64
    } else {
        m.min(i128::from(i64::MAX)) as i64
    })
}

/// The items of `[a,b,...]` (C++ `list_items`).
pub fn list_items(s: &str) -> Parsed<Vec<String>> {
    match s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Some("") => Ok(Vec::new()),
        Some(inner) => Ok(split(inner, ',')),
        None => fail(format!("expected a list [..], got '{s}'")),
    }
}

/// `"text"` -> `text`; anything else unchanged.
pub fn unquote(s: &str) -> &str {
    s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(s)
}

/// `%x` of `sscanf`: blanks, a sign, an optional `0x`, hex digits; the value and the rest.
#[allow(clippy::cast_possible_truncation, reason = "sscanf's unsigned conversion wraps")]
fn scan_hex(s: &str) -> Option<(u32, &str)> {
    let s = s.trim_start_matches(is_space);
    let (negative, s) = match s.as_bytes().first() {
        Some(b'-') => (true, s.get(1..)?),
        Some(b'+') => (false, s.get(1..)?),
        _ => (false, s),
    };
    let s = if (s.starts_with("0x") || s.starts_with("0X"))
        && s.get(2..)
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_ascii_hexdigit())
    {
        s.get(2..)?
    } else {
        s
    };
    let end = s.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    let mut v: u64 = 0;
    for c in s.get(..end)?.chars() {
        v = (v << 4 | u64::from(c.to_digit(16)?)) & 0xffff_ffff;
    }
    let v = v as u32;
    Some((if negative { v.wrapping_neg() } else { v }, s.get(end..)?))
}

/// `aa:bb:cc:dd:ee:ff` (C++ `parse_mac`: `sscanf("%x:%x:%x:%x:%x:%x%c") == 6`; each byte is the
/// field's low eight bits).
#[allow(clippy::cast_possible_truncation, reason = "the C driver's (uint8_t)")]
pub fn parse_mac(s: &str) -> Parsed<[u8; 6]> {
    let bad = || ParseError(format!("bad MAC '{s}'"));
    let mut mac = [0u8; 6];
    let mut rest = s;
    for (i, byte) in mac.iter_mut().enumerate() {
        if i > 0 {
            rest = rest.strip_prefix(':').ok_or_else(bad)?;
        }
        let (v, r) = scan_hex(rest).ok_or_else(bad)?;
        *byte = v as u8;
        rest = r;
    }
    if rest.is_empty() { Ok(mac) } else { Err(bad()) }
}

/// An IPv4 or IPv6 address (C++ `parse_ip`: `inet_pton`, IPv6 when the text has a `:`).
pub fn parse_ip(s: &str) -> Parsed<IpAddr> {
    if s.contains(':') {
        s.parse::<Ipv6Addr>()
            .map(IpAddr::V6)
            .map_err(|_| ParseError(format!("bad IPv6 address '{s}'")))
    } else {
        s.parse::<Ipv4Addr>()
            .map(IpAddr::V4)
            .map_err(|_| ParseError(format!("bad IPv4 address '{s}'")))
    }
}

/// `a.b.c.d/len` or `v6/len`: address and mask (C++ `parse_prefix`).
pub fn parse_prefix(s: &str) -> Parsed<(IpAddr, IpAddr)> {
    let Some((addr, len)) = s.rsplit_once('/') else {
        return fail(format!("bad prefix '{s}'"));
    };
    let addr = parse_ip(addr)?;
    let len = num(len)?;
    match addr {
        IpAddr::V4(_) => {
            let len = u32::try_from(len)
                .ok()
                .filter(|l| *l <= 32)
                .ok_or(ParseError("bad prefix length".into()))?;
            let mask = u32::MAX.checked_shl(32 - len).unwrap_or(0);
            Ok((addr, IpAddr::V4(Ipv4Addr::from_bits(mask))))
        }
        IpAddr::V6(_) => {
            let len = u32::try_from(len)
                .ok()
                .filter(|l| *l <= 128)
                .ok_or(ParseError("bad prefix length".into()))?;
            let mask = u128::MAX.checked_shl(128 - len).unwrap_or(0);
            Ok((addr, IpAddr::V6(Ipv6Addr::from_bits(mask))))
        }
    }
}

/// `aa:bb:cc:dd:ee:ff`, lower case.
pub fn mac_str(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

/// Eight groups of lower-case hex, no leading zeros, no `::` (C++ `ip6_str`).
pub fn ip6_str(a: &Ipv6Addr) -> String {
    a.segments()
        .iter()
        .map(|g| format!("{g:x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// An address as the C++ driver prints it (`ip_str`).
pub fn ip_str(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(a) => a.to_string(),
        IpAddr::V6(a) => ip6_str(a),
    }
}

/// `n` bytes from `2n` hex digits (C++ `hex_bytes`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the C driver's (uint8_t)"
)]
pub fn hex_bytes(s: &str, n: usize) -> Parsed<Vec<u8>> {
    if s.len() != 2 * n || !s.is_ascii() {
        return fail(format!("expected {n} hex bytes"));
    }
    (0..n)
        .map(|i| hexnum(s.get(2 * i..2 * i + 2).unwrap_or_default()).map(|v| v as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_quoted_blanks() {
        assert_eq!(
            words("  create hostif h NAME=\"Ethernet 0\" x  "),
            ["create", "hostif", "h", "NAME=\"Ethernet 0\"", "x"]
        );
        assert_eq!(words(""), Vec::<String>::new());
        assert_eq!(words("a \"b"), ["a", "\"b"]);
    }

    #[test]
    fn numbers_follow_strtoll() {
        assert_eq!(num("10"), Ok(10));
        assert_eq!(num("0x1f"), Ok(31));
        assert_eq!(num("010"), Ok(8));
        assert_eq!(num("-1"), Ok(-1));
        assert_eq!(num("18446744073709551615"), Ok(-1));
        assert_eq!(num("99999999999999999999999"), Ok(-1));
        assert_eq!(num(""), Err(ParseError("empty number".into())));
        assert_eq!(num("12a"), Err(ParseError("bad number '12a'".into())));
        assert_eq!(num("0x"), Err(ParseError("bad number '0x'".into())));
        assert_eq!(hexnum("1f"), Ok(31));
        assert_eq!(hexnum("0x10"), Ok(16));
        assert!(hexnum("").is_err());
        assert!(hexnum("g").is_err());
    }

    #[test]
    fn macs_and_addresses() {
        assert_eq!(parse_mac("00:11:22:33:44:55"), Ok([0, 0x11, 0x22, 0x33, 0x44, 0x55]));
        assert_eq!(parse_mac("0:1:2:3:4:1ff"), Ok([0, 1, 2, 3, 4, 0xff]));
        assert!(parse_mac("00:11:22:33:44").is_err());
        assert!(parse_mac("00:11:22:33:44:55x").is_err());
        assert_eq!(mac_str(&[2, 0, 0, 0, 0xaa, 1]), "02:00:00:00:aa:01");
        assert_eq!(ip_str(&parse_ip("2001:db8::1").unwrap()), "2001:db8:0:0:0:0:0:1");
        assert!(parse_ip("10.0.0.256").is_err());
        let (a, m) = parse_prefix("10.1.0.0/16").unwrap();
        assert_eq!((ip_str(&a), ip_str(&m)), ("10.1.0.0".into(), "255.255.0.0".into()));
        let (_, m) = parse_prefix("0.0.0.0/0").unwrap();
        assert_eq!(ip_str(&m), "0.0.0.0");
        let (_, m) = parse_prefix("2001:db8::/33").unwrap();
        assert_eq!(ip_str(&m), "ffff:ffff:8000:0:0:0:0:0");
        assert_eq!(parse_prefix("10.0.0.0/33"), Err(ParseError("bad prefix length".into())));
    }

    #[test]
    fn lists_split_outside_brackets() {
        assert_eq!(list_items("[]"), Ok(vec![]));
        assert_eq!(
            list_items("[a,[b,c],d]"),
            Ok(vec!["a".into(), "[b,c]".into(), "d".into()])
        );
        assert!(list_items("a,b").is_err());
        assert_eq!(hex_bytes("00ff", 2), Ok(vec![0, 255]));
        assert!(hex_bytes("00f", 2).is_err());
    }
}

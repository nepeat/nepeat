//! The protocol's encodings (header `sai_rs_test.h`): numbers, booleans, MAC and IP addresses,
//! lists and percent-encoded strings, in requests and in reply records.
use core::fmt::{self, Display, Write as _};
use sai_core::backend::types::MacAddr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The bytes a string carries as they are; every other byte is `%HH`.
const fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'/' | b'-')
}

/// A string, percent-encoded (upper-case hex).
pub(crate) fn percent(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if plain(byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// A percent-encoded string; `None` for a byte that must have been encoded, a broken `%HH` or
/// a result that is not UTF-8.
pub(crate) fn unpercent(word: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(word.len());
    let mut rest = word.bytes();
    while let Some(byte) = rest.next() {
        if byte == b'%' {
            let high = hex_digit(rest.next()?)?;
            let low = hex_digit(rest.next()?)?;
            bytes.push((high << 4) | low);
        } else if plain(byte) {
            bytes.push(byte);
        } else {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}

/// `aa:bb:cc:dd:ee:ff`; `None` for anything else.
pub(crate) fn mac(word: &str) -> Option<MacAddr> {
    let mut out = [0u8; 6];
    let mut parts = word.split(':');
    for slot in &mut out {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(MacAddr(out))
}

/// `4/<8 hex>` or `6/<32 hex>` (network order); `None` for anything else.
pub(crate) fn ip(word: &str) -> Option<IpAddr> {
    let (family, hex) = word.split_once('/')?;
    let bytes = hex_bytes(hex)?;
    match (family, bytes.len()) {
        ("4", 4) => <[u8; 4]>::try_from(bytes.as_slice())
            .ok()
            .map(|b| IpAddr::V4(Ipv4Addr::from(b))),
        ("6", 16) => <[u8; 16]>::try_from(bytes.as_slice())
            .ok()
            .map(|b| IpAddr::V6(Ipv6Addr::from(b))),
        _ => None,
    }
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| match pair {
            [high, low] => Some((hex_digit(*high)? << 4) | hex_digit(*low)?),
            _ => None,
        })
        .collect()
}

/// An IP address as the protocol writes it.
pub(crate) struct Ip(pub(crate) IpAddr);

impl Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (family, bytes): (char, Vec<u8>) = match self.0 {
            IpAddr::V4(a) => ('4', a.octets().to_vec()),
            IpAddr::V6(a) => ('6', a.octets().to_vec()),
        };
        write!(f, "{family}/")?;
        bytes.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// A boolean as `0` / `1`.
pub(crate) struct Flag(pub(crate) bool);

impl Display for Flag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0 { "1" } else { "0" })
    }
}

/// A list: elements separated by `,`.
pub(crate) struct List<I>(pub(crate) I);

impl<I, T> Display for List<I>
where
    I: Clone + IntoIterator<Item = T>,
    T: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.0.clone().into_iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}

/// A free text (a string, percent-encoded).
pub(crate) struct Text<'a>(pub(crate) &'a str);

impl Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&percent(self.0))
    }
}

/// The reply text: records of `key=value` fields, one line each.
#[derive(Debug, Default)]
pub(crate) struct Records(String);

impl Records {
    /// A reply of one record.
    pub(crate) fn one(fields: &[(&str, &dyn Display)]) -> Self {
        let mut records = Self::default();
        records.push(fields);
        records
    }

    /// Appends a record.
    pub(crate) fn push(&mut self, fields: &[(&str, &dyn Display)]) {
        for (i, (key, value)) in fields.iter().enumerate() {
            if i > 0 {
                self.0.push(' ');
            }
            let _ = write!(self.0, "{key}={value}");
        }
        self.0.push('\n');
    }

    /// The text.
    pub(crate) fn into_text(self) -> String {
        self.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip_through_percent_encoding() {
        for text in ["", "plain_text-1.2:3/4", "a b%c\n", "caf\u{e9}", "=,"] {
            let encoded = percent(text);
            assert!(encoded.bytes().all(|b| plain(b) || b == b'%'), "{encoded}");
            assert_eq!(unpercent(&encoded).as_deref(), Some(text));
        }
        assert_eq!(percent("a b"), "a%20b");
        assert_eq!(percent("\u{e9}"), "%C3%A9");
        assert_eq!(unpercent("%c3%a9").as_deref(), Some("\u{e9}"));
        assert_eq!(unpercent("a b"), None);
        assert_eq!(unpercent("%4"), None);
        assert_eq!(unpercent("%zz"), None);
        assert_eq!(unpercent("%FF"), None); // not UTF-8
    }

    #[test]
    fn addresses() {
        assert_eq!(mac("00:1a:FF:03:04:05"), Some(MacAddr([0, 0x1a, 0xff, 3, 4, 5])));
        for bad in [
            "",
            "00:11:22:33:44",
            "00:11:22:33:44:55:66",
            "0:11:22:33:44:55",
            "00-11-22-33-44-55",
        ] {
            assert_eq!(mac(bad), None, "{bad}");
        }
        let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(ip("4/0a000001"), Some(v4));
        assert_eq!(Ip(v4).to_string(), "4/0a000001");
        let v6: IpAddr = "fc00::1".parse().unwrap();
        assert_eq!(Ip(v6).to_string(), "6/fc000000000000000000000000000001");
        assert_eq!(ip(&Ip(v6).to_string()), Some(v6));
        for bad in [
            "4/0a0000",
            "6/0a000001",
            "4/0a00000g",
            "0a000001",
            "5/0a000001",
            "4/0a0000011",
        ] {
            assert_eq!(ip(bad), None, "{bad}");
        }
    }

    #[test]
    fn records_and_lists() {
        let mut r = Records::default();
        r.push(&[("a", &1), ("b", &List([1, 2, 3])), ("c", &List(Vec::<u8>::new()))]);
        r.push(&[("t", &Text("x y")), ("f", &Flag(true))]);
        assert_eq!(r.into_text(), "a=1 b=1,2,3 c=\nt=x%20y f=1\n");
    }
}

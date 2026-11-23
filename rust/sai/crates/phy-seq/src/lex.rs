//! Lines, words and numbers, read exactly as the C++ parsers read them.

/// The lines of a table as `std::getline` yields them: split at line feeds, and no empty line
/// after a final line feed (an empty text has no line at all).
pub(crate) fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    let none = text.is_empty();
    let body = text.strip_suffix(b"\n").unwrap_or(text);
    body.split(|&b| b == b'\n').filter(move |_| !none)
}

/// The words of a line as `istream >> std::string` yields them: separated by the white space
/// of the C locale (space, `\t \n \v \f \r`).
pub(crate) fn words(line: &[u8]) -> Vec<&[u8]> {
    line.split(|b| matches!(b, b' ' | b'\t'..=b'\r'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// The base of a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Radix {
    /// Decimal.
    Dec,
    /// Hexadecimal; an `0x` / `0X` prefix is accepted, as `strtoul` does.
    Hex,
}

/// One number word, at most `max`. The C++ `num()`: no sign, then `strtoul` must consume the
/// whole C string.
///
/// Two things follow from "C string" and are kept so that both parsers give the same verdict on
/// every input: the word is read up to its first NUL byte only, and a word that starts with a NUL
/// is the number 0.
pub(crate) fn num(word: &[u8], radix: Radix, max: u32) -> Option<u32> {
    let first = *word.first()?;
    if first == b'-' || first == b'+' {
        return None;
    }
    let text = word.split(|&b| b == 0).next().unwrap_or(word);
    if text.is_empty() {
        return Some(0);
    }
    let digits = match (radix, text) {
        (Radix::Hex, [b'0', b'x' | b'X', rest @ ..]) if rest.first().is_some_and(u8::is_ascii_hexdigit) => rest,
        _ => text,
    };
    let base = match radix {
        Radix::Dec => 10,
        Radix::Hex => 16,
    };
    let mut value: u64 = 0;
    for &d in digits {
        let d = char::from(d).to_digit(base)?;
        value = value.saturating_mul(u64::from(base)).saturating_add(u64::from(d));
    }
    u32::try_from(value).ok().filter(|&v| v <= max)
}

/// [`num`] for a number that fits 16 bits.
pub(crate) fn num16(word: &[u8], radix: Radix, max: u16) -> Option<u16> {
    num(word, radix, u32::from(max)).and_then(|v| u16::try_from(v).ok())
}

/// `strtoul(text, NULL, 16)` cut to 32 bits, for the command line tools' files (C++ takes
/// whatever `strtoul` makes of a word): optional sign, optional `0x`, the longest run of hex
/// digits; no digits is 0, a value beyond 64 bits is all ones.
pub(crate) fn strtoul_hex(text: &[u8]) -> u32 {
    let text = text.split(|&b| b == 0).next().unwrap_or(text);
    let (negative, rest) = match text {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, text),
    };
    let digits = match rest {
        [b'0', b'x' | b'X', tail @ ..] if tail.first().is_some_and(u8::is_ascii_hexdigit) => tail,
        _ => rest,
    };
    let mut value: u64 = 0;
    let mut overflow = false;
    for d in digits.iter().map_while(|&d| char::from(d).to_digit(16)) {
        let (shifted, lost) = (value << 4, value >> 60 != 0);
        overflow |= lost;
        value = shifted | u64::from(d);
    }
    let value = if overflow {
        u64::MAX
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    // the C++ "(unsigned)" cast: the low 32 bits
    u32::try_from(value & 0xffff_ffff).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_getline() {
        let all = |t: &'static [u8]| lines(t).collect::<Vec<_>>();
        assert_eq!(all(b""), Vec::<&[u8]>::new());
        assert_eq!(all(b"\n"), [b""]);
        assert_eq!(all(b"a\nb"), [b"a", b"b"]);
        assert_eq!(all(b"a\n\n"), [&b"a"[..], b""]);
        assert_eq!(all(b"a\r\n"), [b"a\r"]);
    }

    #[test]
    fn words_split_at_c_white_space() {
        assert_eq!(words(b"  W\tp \x0b1\x0c a000\r"), [&b"W"[..], b"p", b"1", b"a000"]);
        assert_eq!(words(b" \t\r"), Vec::<&[u8]>::new());
        assert_eq!(words(b"a\0b c"), [&b"a\0b"[..], b"c"]);
    }

    #[test]
    fn numbers_follow_strtoul() {
        assert_eq!(num(b"ff", Radix::Hex, 0xffff), Some(0xff));
        assert_eq!(num(b"FFFF", Radix::Hex, 0xffff), Some(0xffff));
        assert_eq!(num(b"10000", Radix::Hex, 0xffff), None);
        assert_eq!(num(b"0x1f", Radix::Hex, 31), Some(31));
        assert_eq!(num(b"0X1F", Radix::Hex, 31), Some(31));
        assert_eq!(num(b"0x", Radix::Hex, 31), None);
        assert_eq!(num(b"0xg", Radix::Hex, 31), None);
        assert_eq!(num(b"0x10", Radix::Dec, 100), None);
        assert_eq!(num(b"-1", Radix::Hex, 0xffff), None);
        assert_eq!(num(b"+1", Radix::Hex, 0xffff), None);
        assert_eq!(num(b"1-", Radix::Hex, 0xffff), None);
        assert_eq!(num(b"", Radix::Hex, 0xffff), None);
        assert_eq!(num(b"000000000000000000000000000000012", Radix::Dec, 12), Some(12));
        assert_eq!(num(b"99999999999999999999999999999999", Radix::Dec, u32::MAX), None);
        assert_eq!(num(b"12", Radix::Dec, 11), None);
        assert_eq!(num(b"a", Radix::Dec, 11), None);
        // the C string ends at a NUL
        assert_eq!(num(b"12\0zz", Radix::Hex, 0xffff), Some(0x12));
        assert_eq!(num(b"\0zz", Radix::Hex, 0xffff), Some(0));
        assert_eq!(num(b"0x\0", Radix::Hex, 0xffff), None);
    }

    #[test]
    fn strtoul_takes_what_it_can() {
        assert_eq!(strtoul_hex(b"1f"), 0x1f);
        assert_eq!(strtoul_hex(b"0x1fz"), 0x1f);
        assert_eq!(strtoul_hex(b"zz"), 0);
        assert_eq!(strtoul_hex(b""), 0);
        assert_eq!(strtoul_hex(b"0x"), 0);
        assert_eq!(strtoul_hex(b"-1"), 0xffff_ffff);
        assert_eq!(strtoul_hex(b"+f"), 15);
        assert_eq!(strtoul_hex(b"123456789"), 0x2345_6789);
        assert_eq!(strtoul_hex(b"11111111111111111"), 0xffff_ffff);
        assert_eq!(strtoul_hex(b"1\x002"), 1);
    }
}

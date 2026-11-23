//! Small pieces of C's text handling that the output depends on.

/// Appends formatted text to a byte buffer.
macro_rules! put {
    ($out:expr, $($arg:tt)*) => {
        $out.extend_from_slice(format!($($arg)*).as_bytes())
    };
}
pub(crate) use put;

/// What C's `strerror` says for an I/O error: the system's text without Rust's " (os error N)".
pub fn strerror(e: &std::io::Error) -> String {
    let text = e.to_string();
    match (e.raw_os_error(), text.rfind(" (os error ")) {
        (Some(_), Some(at)) => text.get(..at).unwrap_or(&text).to_owned(),
        _ => text,
    }
}

/// A fixed-size C character buffer written with `snprintf`: holds at most `cap - 1` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CBuf {
    data: Vec<u8>,
    cap: usize,
}

impl CBuf {
    pub(crate) const fn new(cap: usize) -> Self {
        Self { data: Vec::new(), cap }
    }

    /// The C helper `app`: appends, cut at the buffer's end; nothing once it is full.
    pub(crate) fn app(&mut self, text: &[u8]) {
        let room = self.cap.saturating_sub(1).saturating_sub(self.data.len());
        self.data.extend(text.iter().copied().take(room));
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

/// `snprintf(buf, cap, "%s", text)`: the first `cap - 1` bytes.
pub(crate) fn cut(text: &[u8], cap: usize) -> Vec<u8> {
    text.iter().copied().take(cap.saturating_sub(1)).collect()
}

/// `%-*s`: the bytes, then spaces up to `width`.
pub(crate) fn pad(out: &mut Vec<u8>, text: &[u8], width: usize) {
    out.extend_from_slice(text);
    out.extend(std::iter::repeat_n(b' ', width.saturating_sub(text.len())));
}

/// C `isspace` in the "C" locale.
pub(crate) const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `strtoul(s, &end, 16)` on a string without leading space: the value (wrapping as C's does for
/// a sign, saturating on overflow) and what follows the number. Nothing parsed: 0 and all of `s`.
pub(crate) fn strtoul16(s: &[u8]) -> (u64, &[u8]) {
    let (negative, body) = match s.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, s),
    };
    let hex = |b: &u8| char::from(*b).to_digit(16);
    // "0x" is a prefix only when a hex digit follows.
    let digits = match body {
        [b'0', b'x' | b'X', rest @ ..] if rest.first().and_then(hex).is_some() => rest,
        _ => body,
    };
    let count = digits.iter().take_while(|b| hex(b).is_some()).count();
    if count == 0 {
        return (0, s);
    }
    let (number, rest) = digits.split_at_checked(count).unwrap_or((digits, &[]));
    let mut value = 0u64;
    let mut overflow = false;
    for d in number.iter().filter_map(hex) {
        match value.checked_mul(16).and_then(|v| v.checked_add(u64::from(d))) {
            Some(v) => value = v,
            None => overflow = true,
        }
    }
    if overflow {
        return (u64::MAX, rest);
    }
    (if negative { value.wrapping_neg() } else { value }, rest)
}

/// `(unsigned)value` of an `unsigned long`: the low 32 bits.
pub(crate) fn low32(value: u64) -> u32 {
    u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX)
}

/// The part of a path after the last `/`.
pub(crate) fn base(path: &[u8]) -> &[u8] {
    path.rsplit(|&b| b == b'/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtoul16_is_c() {
        assert_eq!(strtoul16(b"24"), (0x24, &b""[..]));
        assert_eq!(strtoul16(b"0x1f.8b00"), (0x1f, &b".8b00"[..]));
        assert_eq!(strtoul16(b"0x"), (0, &b"x"[..]));
        assert_eq!(strtoul16(b"zz"), (0, &b"zz"[..]));
        assert_eq!(strtoul16(b""), (0, &b""[..]));
        assert_eq!(strtoul16(b"-1"), (u64::MAX, &b""[..]));
        assert_eq!(strtoul16(b"+a,"), (10, &b","[..]));
        assert_eq!(strtoul16(b"fffffffffffffffff"), (u64::MAX, &b""[..]));
        assert_eq!(low32(0x1_0000_0024), 0x24);
    }

    #[test]
    fn buffers_cut_like_snprintf() {
        let mut b = CBuf::new(8);
        b.app(b"abcde");
        b.app(b"fghij");
        b.app(b"k");
        assert_eq!(b.into_bytes(), b"abcdefg");
        assert_eq!(cut(b"abcdef", 4), b"abc");
        let mut out = Vec::new();
        pad(&mut out, b"ab", 4);
        pad(&mut out, b"abcdef", 4);
        assert_eq!(out, b"ab  abcdef");
        assert_eq!(base(b"/a/b/c.mdio"), b"c.mdio");
        assert_eq!(base(b"c.mdio"), b"c.mdio");
        assert_eq!(base(b"a/"), b"");
    }

    #[test]
    fn strerror_drops_the_os_error_number() {
        let e = std::io::Error::from_raw_os_error(2);
        assert_eq!(strerror(&e), "No such file or directory");
        let e = std::io::Error::other("plain");
        assert_eq!(strerror(&e), "plain");
    }
}

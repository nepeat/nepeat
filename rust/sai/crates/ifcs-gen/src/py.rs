//! Python's text semantics where the reference generator (`gen_ifcs_api.py`) relies on them:
//! what counts as whitespace (`str.isspace`, the `re` class `\s`) and as a word character
//! (`\w`), `str.splitlines`, the universal newlines of a text-mode read, `int(s, 0)` and the
//! integer part of `format(value, spec)`.

use crate::Fatal;

/// `str.isspace()` / `\s`: Unicode `White_Space` plus the four separators U+001C..U+001F.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `\w` of a `str` pattern: alphanumeric or underscore.
pub(crate) fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `str.strip()`.
pub(crate) fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str.rstrip()`.
pub(crate) fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `str.split()` (no argument): runs of whitespace separate, no empty words.
pub(crate) fn split(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// `os.path.basename`: what follows the last `/`.
pub(crate) fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// `str.splitlines()`: every line boundary Python knows, `\r\n` as one; no empty last line
/// for a text that ends in a boundary.
pub(crate) fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if !matches!(
            c,
            '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            continue;
        }
        out.push(s.get(start..i).unwrap_or_default());
        let mut end = i + c.len_utf8();
        if c == '\r' && it.peek().is_some_and(|&(_, n)| n == '\n') {
            it.next();
            end += 1;
        }
        start = end;
    }
    if start < s.len() {
        out.push(s.get(start..).unwrap_or_default());
    }
    out
}

/// What a text-mode `open(path).read()` hands back: `\r\n` and a lone `\r` become `\n`.
pub(crate) fn universal_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// The digits of an integer literal in `radix`, with Python's underscore rule (single
/// underscores between digits; one may follow a base prefix: `lead`).
fn digits(s: &str, radix: u32, lead: bool) -> Option<i128> {
    let s = if lead { s.strip_prefix('_').unwrap_or(s) } else { s };
    if s.is_empty() || s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    let mut v: i128 = 0;
    for c in s.chars().filter(|&c| c != '_') {
        let d = c.to_digit(radix)?;
        v = v.checked_mul(i128::from(radix))?.checked_add(i128::from(d))?;
    }
    Some(v)
}

/// `int(s, 0)`: optional sign, then `0x` / `0o` / `0b` or a decimal without leading zeros.
/// `None` where Python raises `ValueError`, and for a value outside `i128` (Python's ints
/// are unbounded; no IFCS value comes near).
pub(crate) fn int_base0(s: &str) -> Option<i128> {
    let s = strip(s);
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let lower = body.get(..2).map(str::to_ascii_lowercase);
    let v = match lower.as_deref() {
        Some("0x") => digits(body.get(2..)?, 16, true)?,
        Some("0o") => digits(body.get(2..)?, 8, true)?,
        Some("0b") => digits(body.get(2..)?, 2, true)?,
        _ => {
            let v = digits(body, 10, false)?;
            if body.starts_with('0') && v != 0 {
                return None; // "leading zeros in decimal integer literals are not permitted"
            }
            v
        }
    };
    if neg { v.checked_neg() } else { Some(v) }
}

/// Groups `digits` from the right in runs of `n` with `sep`.
fn group(digits: &str, n: usize, sep: char) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(n) {
            out.push(sep);
        }
        out.push(*c);
    }
    out
}

/// `format(value, spec)` for an int: `[[fill]align][sign][#][0][width][grouping][type]` with the
/// types `d` `x` `X` `o` `b` (or none). Python's errors are `ValueError`s; the parts of the
/// mini-language that no template needs (`c`, `n`, the float types, zero padding together with
/// grouping) are refused as [`Fatal::Limit`].
#[allow(
    clippy::too_many_lines,
    reason = "one pass over the format mini-language, in its order"
)]
pub(crate) fn format_int(v: i128, spec: &str) -> Result<String, Fatal> {
    let unsupported = || Fatal::Limit(format!("format spec {spec:?}: not supported by ifcs-gen"));
    let cs: Vec<char> = spec.chars().collect();
    let is_align = |c: Option<&char>| matches!(c, Some('<' | '>' | '=' | '^'));
    let mut i = 0;
    let (mut fill, mut align) = (None, None);
    if is_align(cs.get(1)) {
        fill = cs.first().copied();
        align = cs.get(1).copied();
        i = 2;
    } else if is_align(cs.first()) {
        align = cs.first().copied();
        i = 1;
    }
    let sign = match cs.get(i) {
        Some(&c @ ('+' | '-' | ' ')) => {
            i += 1;
            c
        }
        _ => '-',
    };
    if cs.get(i) == Some(&'z') {
        return Err(Fatal::Value(
            "Negative zero coercion (z) not allowed in integer format specifier".into(),
        ));
    }
    let alt = cs.get(i) == Some(&'#');
    if alt {
        i += 1;
    }
    let zero = cs.get(i) == Some(&'0');
    if zero {
        i += 1;
        fill = fill.or(Some('0'));
        align = align.or(Some('='));
    }
    let mut width = 0usize;
    while let Some(d) = cs.get(i).and_then(|c| c.to_digit(10)) {
        width = width
            .checked_mul(10)
            .and_then(|w| w.checked_add(d as usize))
            .ok_or_else(unsupported)?;
        i += 1;
    }
    let grouping = match cs.get(i) {
        Some(&c @ (',' | '_')) => {
            i += 1;
            Some(c)
        }
        _ => None,
    };
    if cs.get(i) == Some(&'.') {
        return Err(Fatal::Value("Precision not allowed in integer format specifier".into()));
    }
    let ty = cs.get(i).copied();
    if ty.is_some() {
        i += 1;
    }
    if i != cs.len() {
        return Err(Fatal::Value("Invalid format specifier".into()));
    }
    let (radix, prefix, upper) = match ty {
        None | Some('d') => (10, "", false),
        Some('x') => (16, "0x", false),
        Some('X') => (16, "0X", true),
        Some('o') => (8, "0o", false),
        Some('b') => (2, "0b", false),
        Some(c @ ('c' | 'n' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%')) => {
            return Err(Fatal::Limit(format!("format type {c:?}: not supported by ifcs-gen")));
        }
        Some(c) => {
            return Err(Fatal::Value(format!(
                "Unknown format code '{c}' for object of type 'int'"
            )));
        }
    };
    if grouping == Some(',') && radix != 10 {
        return Err(Fatal::Value(format!(
            "Cannot specify ',' with '{}'.",
            ty.unwrap_or('d')
        )));
    }
    if zero && grouping.is_some() && width > 0 {
        return Err(unsupported());
    }
    let mag = v.unsigned_abs();
    let mut body = match radix {
        16 => format!("{mag:x}"),
        8 => format!("{mag:o}"),
        2 => format!("{mag:b}"),
        _ => mag.to_string(),
    };
    if upper {
        body = body.to_ascii_uppercase();
    }
    if let Some(sep) = grouping {
        body = group(&body, if radix == 10 { 3 } else { 4 }, sep);
    }
    let mut head = match (v < 0, sign) {
        (true, _) => "-".to_owned(),
        (false, '+') => "+".to_owned(),
        (false, ' ') => " ".to_owned(),
        _ => String::new(),
    };
    if alt {
        head.push_str(prefix);
    }
    let len = head.chars().count() + body.chars().count();
    let pad = width.saturating_sub(len);
    let f = fill.unwrap_or(' ').to_string();
    Ok(match align.unwrap_or('>') {
        '<' => format!("{head}{body}{}", f.repeat(pad)),
        '^' => format!("{}{head}{body}{}", f.repeat(pad / 2), f.repeat(pad - pad / 2)),
        '=' => format!("{head}{}{body}", f.repeat(pad)),
        _ => format!("{}{head}{body}", f.repeat(pad)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_like_python() {
        assert_eq!(
            splitlines("a\nb\r\nc\rd\u{b}e\u{2028}f\n"),
            ["a", "b", "c", "d", "e", "f"]
        );
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("\n\n"), ["", ""]);
        assert_eq!(splitlines("x"), ["x"]);
    }

    #[test]
    fn int_base0_like_python() {
        for (s, v) in [
            ("0", 0),
            ("00", 0),
            ("0_0", 0),
            ("111", 111),
            ("-5", -5),
            ("+7", 7),
            ("0x1F", 31),
        ] {
            assert_eq!(int_base0(s), Some(v), "{s}");
        }
        for (s, v) in [
            ("0X_ff", 255),
            ("0o17", 15),
            ("0b101", 5),
            ("1_000", 1000),
            (" 12 ", 12),
        ] {
            assert_eq!(int_base0(s), Some(v), "{s}");
        }
        for s in ["", "01", "1__0", "_1", "1_", "0x", "0xg", "1.0", "--1", "0x__1", "abc"] {
            assert_eq!(int_base0(s), None, "{s}");
        }
    }

    #[test]
    fn format_int_like_python() {
        // expected values: Python 3 format(v, spec)
        let cases: &[(i128, &str, &str)] = &[
            (2_097_151, "#x", "0x1fffff"),
            (2_097_151, "x", "1fffff"),
            (255, "#X", "0XFF"),
            (-5, "#x", "-0x5"),
            (5, "d", "5"),
            (5, "", "5"),
            (5, "05", "00005"),
            (-5, "05", "-0005"),
            (5, "<5", "5    "),
            (5, "^5", "  5  "),
            (5, "*^6", "**5***"),
            (5, "+", "+5"),
            (5, " ", " 5"),
            (1_234_567, ",", "1,234,567"),
            (1_234_567, "_", "1_234_567"),
            (0xdead_beef, "_x", "dead_beef"),
            (10, "#010b", "0b00001010"),
            (8, "#o", "0o10"),
            (5, "x<05", "5xxxx"),
            (5, "<05", "50000"),
        ];
        for (v, spec, want) in cases {
            assert_eq!(format_int(*v, spec).as_deref(), Ok(*want), "format({v}, {spec:?})");
        }
        for spec in [".2", "s", ",x", "q", "dd", "z"] {
            assert!(matches!(format_int(1, spec), Err(Fatal::Value(_))), "{spec}");
        }
        for spec in ["c", "n", "f", "010,"] {
            assert!(matches!(format_int(1, spec), Err(Fatal::Limit(_))), "{spec}");
        }
    }
}

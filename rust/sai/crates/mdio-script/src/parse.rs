//! Text to typed operations: the parse pass.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::types::{DelayUs, Dev, Label, Line, Op, RegAddr, Stats, Tries, Until, bump};

/// What C's `fgets(line, 512, fp)` returns at most: a longer line is refused.
const CHUNK: usize = 511;
const HEADER: &[u8] = b"mdio-script 1";

/// A script whose every line is well formed: the only thing [`Script::run`] accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    lines: Vec<Line>,
    stats: Stats,
}

/// Why a text is not a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// No line at all.
    Empty,
    /// The first line is not exactly `mdio-script 1`.
    NotAScript,
    /// A line of more than 510 bytes (or one with a NUL byte, which C sees the same way).
    LineTooLong,
    /// The first word of the line is longer than one character.
    UnknownOperationWord,
    /// The operation letter (a byte) is none of `D W R E P N`.
    UnknownOperation(u8),
    /// `D` without exactly one decimal number up to 60000000.
    BadDelay,
    /// Wrong number of fields or a number out of range for this operation letter.
    BadArguments(u8),
}

/// A syntax error (`MDIO_SCRIPT_E_SYNTAX`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line; 0 for [`ParseErrorKind::Empty`].
    pub line: u64,
    /// What is wrong with it.
    pub kind: ParseErrorKind,
    /// The counters of the lines before it, as the C parse pass leaves them.
    pub stats: Stats,
}

impl ParseError {
    /// The C return code, [`crate::E_SYNTAX`].
    pub const fn code(&self) -> i32 {
        crate::E_SYNTAX
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = self.line;
        match self.kind {
            ParseErrorKind::Empty => f.write_str("empty file"),
            ParseErrorKind::NotAScript => f.write_str("not an MDIO script (first line must be 'mdio-script 1')"),
            ParseErrorKind::LineTooLong => write!(f, "line {line}: line too long"),
            ParseErrorKind::UnknownOperationWord => write!(f, "line {line}: unknown operation"),
            ParseErrorKind::UnknownOperation(op) => {
                write!(f, "line {line}: unknown operation '{}'", char::from(op))
            }
            ParseErrorKind::BadDelay => write!(f, "line {line}: D <usec up to 60000000>"),
            ParseErrorKind::BadArguments(op) => {
                write!(
                    f,
                    "line {line}: bad arguments for '{}' (clause 45 only: dev 1..1f)",
                    char::from(op)
                )
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Why [`Script::load`] failed.
#[derive(Debug)]
pub enum LoadError {
    /// The file cannot be opened or read (`MDIO_SCRIPT_E_FILE`).
    File(std::io::Error),
    /// The file is not a script (`MDIO_SCRIPT_E_SYNTAX`); an empty file is one of these.
    Syntax(ParseError),
}

impl LoadError {
    /// The C return code: [`crate::E_FILE`] or [`crate::E_SYNTAX`].
    pub const fn code(&self) -> i32 {
        match self {
            Self::File(_) => crate::E_FILE,
            Self::Syntax(_) => crate::E_SYNTAX,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(e) => e.fmt(f),
            Self::Syntax(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(e) => Some(e),
            Self::Syntax(e) => Some(e),
        }
    }
}

impl From<ParseError> for LoadError {
    fn from(e: ParseError) -> Self {
        Self::Syntax(e)
    }
}

impl Script {
    /// Parses a script text.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        Self::parse_bytes(text.as_bytes())
    }

    /// Parses the bytes of a script file. Nothing but the label text may be outside ASCII, and that
    /// is kept as it is.
    pub fn parse_bytes(data: &[u8]) -> Result<Self, ParseError> {
        let mut lines = Vec::new();
        let mut stats = Stats::default();
        // The verification section is the assertions after the last other operation:
        // where it may start (a label may open it) and how many assertions it has so far.
        let (mut candidate, mut checks) = (0u64, 0u64);
        let mut ln = 0u64;
        let mut rest = data;
        while !rest.is_empty() {
            let raw;
            (raw, rest) = next_chunk(rest);
            bump(&mut ln);
            let err = |kind, stats| ParseError { line: ln, kind, stats };
            let Some(text) = raw.text() else {
                return Err(err(ParseErrorKind::LineTooLong, stats));
            };
            if ln == 1 {
                if text != HEADER {
                    return Err(err(ParseErrorKind::NotAScript, stats));
                }
                continue;
            }
            let Some(op) = parse_line(text).map_err(|kind| err(kind, stats))? else {
                continue;
            };
            stats.count(&op);
            match &op {
                Op::Label(_) => {
                    if candidate == 0 {
                        candidate = ln;
                    }
                }
                op if op.is_assertion() => {
                    if candidate == 0 {
                        candidate = ln;
                    }
                    bump(&mut checks);
                    bump(&mut stats.ops);
                }
                _ => {
                    (candidate, checks) = (0, 0);
                    bump(&mut stats.ops);
                }
            }
            lines.push(Line { line: ln, op });
        }
        if ln == 0 {
            return Err(ParseError {
                line: 0,
                kind: ParseErrorKind::Empty,
                stats,
            });
        }
        stats.verify_line = if checks > 0 { candidate } else { 0 };
        stats.verify_checks = checks;
        Ok(Self { lines, stats })
    }

    /// Reads and parses a script file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let data = std::fs::read(path).map_err(LoadError::File)?;
        Ok(Self::parse_bytes(&data)?)
    }

    /// The operations in script order, each with its line.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The operations in script order.
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.lines.iter().map(|l| &l.op)
    }

    /// What the parse pass counted: `lines writes reads delays polls labels delay_us ops`, and the
    /// verification section (`verify_line`, 0 = none, and `verify_checks`).
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The script as text that parses to the same operations (comments, blank lines and so the
    /// line numbers are not kept). Labels are written byte for byte; the one exception to "the
    /// same operations" is a label that the parser cut inside a run of carriage returns (see
    /// [`Label`]), which loses those trailing CRs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = HEADER.to_vec();
        out.push(b'\n');
        for op in self.ops() {
            op.write_to(&mut out);
            out.push(b'\n');
        }
        out
    }
}

/// [`Script::to_bytes`], with labels that are not UTF-8 shown lossily.
impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.to_bytes()))
    }
}

impl FromStr for Script {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, ParseError> {
        Self::parse(s)
    }
}

/// One `fgets` result.
struct Chunk<'a> {
    bytes: &'a [u8],
    /// The read ran into the end of the file (so a missing line feed is not an overlong line).
    at_eof: bool,
}

/// Splits off what `fgets` with a 512-byte buffer would return next. `data` is not empty.
fn next_chunk(data: &[u8]) -> (Chunk<'_>, &[u8]) {
    let window = data.get(..CHUNK).unwrap_or(data);
    let (len, at_eof) = match window.iter().position(|&b| b == b'\n') {
        Some(i) => (i.saturating_add(1), false),
        None => (window.len(), window.len() < CHUNK),
    };
    let (bytes, rest) = data.split_at_checked(len).unwrap_or((data, &[]));
    (Chunk { bytes, at_eof }, rest)
}

impl<'a> Chunk<'a> {
    /// The line without its end, as the C code sees it (a C string: it stops at a NUL byte).
    /// `None`: "line too long".
    fn text(&self) -> Option<&'a [u8]> {
        let nul = self.bytes.iter().position(|&b| b == 0).unwrap_or(self.bytes.len());
        let mut text = self.bytes.get(..nul).unwrap_or(self.bytes);
        if !text.is_empty() && text.last() != Some(&b'\n') && !self.at_eof {
            return None;
        }
        while let Some((&(b'\n' | b'\r'), head)) = text.split_last() {
            text = head;
        }
        Some(text)
    }
}

/// C `isspace` in the "C" locale.
const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `strtoul` as the C code uses it: the whole field, no sign, at most `max`; base 16 takes an
/// optional `0x`.
fn num(field: &[u8], radix: u32, max: u32) -> Option<u32> {
    let digits = match field {
        [b'0', b'x' | b'X', rest @ ..] if radix == 16 => rest,
        _ => field,
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0u32;
    for &b in digits {
        let digit = char::from(b).to_digit(radix)?;
        value = value.checked_mul(radix)?.checked_add(digit)?;
        if value > max {
            return None;
        }
    }
    Some(value)
}

fn hex16(field: &[u8]) -> Option<u16> {
    num(field, 16, u32::from(u16::MAX)).and_then(|v| u16::try_from(v).ok())
}

fn addr(phy: &[u8], dev: &[u8], reg: &[u8]) -> Option<RegAddr> {
    let phy = hex16(phy)?;
    let dev = num(dev, 16, u32::from(Dev::MAX))
        .and_then(|v| u8::try_from(v).ok())
        .and_then(Dev::new)?;
    Some(RegAddr {
        phy,
        dev,
        reg: hex16(reg)?,
    })
}

fn delay(field: &[u8]) -> Option<DelayUs> {
    num(field, 10, DelayUs::MAX).and_then(DelayUs::new)
}

/// One line without its end. `Ok(None)`: comment or blank.
fn parse_line(text: &[u8]) -> Result<Option<Op>, ParseErrorKind> {
    match text {
        [] | [b'#', ..] => return Ok(None),
        [b'L'] => return Ok(Some(Op::Label(Label::default()))),
        [b'L', b' ', label @ ..] => return Ok(Some(Op::Label(Label::cut(label)))),
        _ => {}
    }
    // Nine are enough to know there are more than the eight any operation has.
    let fields: Vec<&[u8]> = text.split(|&b| is_space(b)).filter(|f| !f.is_empty()).take(9).collect();
    let (letter, args) = match fields.split_first() {
        None => return Ok(None),
        Some((&&[letter], args)) => (letter, args),
        Some(_) => return Err(ParseErrorKind::UnknownOperationWord),
    };
    let op = match (letter, args) {
        (b'D', &[us]) => return delay(us).map(|us| Some(Op::Delay(us))).ok_or(ParseErrorKind::BadDelay),
        (b'D', _) => return Err(ParseErrorKind::BadDelay),
        (b'W', &[phy, dev, reg, val]) => addr(phy, dev, reg)
            .zip(hex16(val))
            .map(|(at, val)| Op::Write { at, val }),
        (b'R', &[phy, dev, reg]) => addr(phy, dev, reg).map(|at| Op::Read { at }),
        (b'E', &[phy, dev, reg, val]) => addr(phy, dev, reg).zip(hex16(val)).map(|(at, val)| Op::Expect {
            at,
            val,
            mask: u16::MAX,
        }),
        (b'E', &[phy, dev, reg, val, mask]) => (|| {
            Some(Op::Expect {
                at: addr(phy, dev, reg)?,
                val: hex16(val)?,
                mask: hex16(mask)?,
            })
        })(),
        (b'P' | b'N', &[phy, dev, reg, val, mask, tries, interval]) => (|| {
            Some(Op::Poll {
                at: addr(phy, dev, reg)?,
                until: if letter == b'P' { Until::Equal } else { Until::NotEqual },
                val: hex16(val)?,
                mask: hex16(mask)?,
                tries: num(tries, 10, Tries::MAX).and_then(Tries::new)?,
                interval: delay(interval)?,
            })
        })(),
        (b'W' | b'R' | b'E' | b'P' | b'N', _) => None,
        _ => return Err(ParseErrorKind::UnknownOperation(letter)),
    };
    op.map(Some).ok_or(ParseErrorKind::BadArguments(letter))
}

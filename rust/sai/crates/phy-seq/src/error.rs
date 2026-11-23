//! Why a text is not a table.

use std::fmt;

/// What is wrong with a line. The messages are those of the C++ parsers, byte for byte
/// ([`ParseError::write_to`]); words of the table that a message repeats are kept as bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The first line that is not a comment is not the header (`ext-macsec 1` / `ext-phy 1`).
    NotATable,
    /// No line but comments and empty ones.
    Empty,
    /// The first word of the line is no keyword and no operation of this table.
    UnknownKeyword(Vec<u8>),
    /// An operation before the first sequence started (ext-phy: also after a `port` line).
    OperationOutsideSequence,
    /// More operations in a sequence than the engine accepts (ext-macsec 8192, ext-phy 4096).
    SequenceTooLong,
    /// Wrong number of fields for the operation letter.
    FieldCount(u8),
    /// ext-macsec: the PHY field is neither `p` nor a hex number up to ffff.
    Phy,
    /// The device is not hex 0..1f or the register not hex up to ffff (ext-phy: or the PHY).
    DevReg,
    /// Device 0: a clause-22 access.
    Clause22,
    /// The value of a `W` (ext-macsec: constant, kept value or bytes; ext-phy: a constant), or
    /// the compared value of `E P C`.
    Value,
    /// The compared value of ext-macsec's `E` / `P`.
    CompareValue,
    /// The mask is not hex up to ffff.
    Mask,
    /// The compared value has a bit outside the mask.
    OutsideMask,
    /// `P`: tries not 1..1000 or interval not 0..100000, decimal.
    Poll,
    /// ext-macsec `D`: not one decimal number 0..100000.
    Delay,
    /// ext-macsec `X`: the destination is neither `$<n>` nor `<result>[<word>]`.
    Destination,
    /// ext-macsec `counters`: not `clear-on-read` or `absolute`.
    Counters,
    /// ext-macsec `ifg-bits`: not one decimal number 0..2040.
    IfgBits,
    /// ext-macsec `port`: not a decimal lane and a hex PHY.
    MacsecPort,
    /// ext-macsec: the lane was listed before. Holds the lane as written.
    MacsecPortTwice(Vec<u8>),
    /// ext-macsec `seq`: not one name of `a-z 0-9 . -`, 1..80 characters.
    SeqName,
    /// ext-macsec: a second sequence of that name.
    SeqTwice(String),
    /// ext-macsec: a `W` writes `$slot` before an `X` of the sequence read it.
    KeptBeforeRead {
        /// The sequence.
        seq: String,
        /// The scratch value.
        slot: u8,
    },
    /// ext-phy: `poll-ms`, `up-polls` or `holdoff-ms` (the keyword held) after the first port.
    SettingAfterPort(&'static str),
    /// ext-phy: the setting (the keyword held) is not one decimal number in its range.
    Setting(&'static str),
    /// ext-phy `port`: not one decimal lane.
    PhyPort,
    /// ext-phy: the lane was listed before. Holds the lane as written and the first line.
    PhyPortTwice(Vec<u8>, u32),
    /// ext-phy: `link` / `admin-down` / `admin-up` (held) before any port.
    SectionBeforePort(&'static str),
    /// ext-phy: the section keyword (held) with an argument.
    SectionArgument(&'static str),
    /// ext-phy: the port (its lane held) has that section (held) already.
    SectionTwice(&'static str, u32),
    /// ext-phy: a `C` outside a link sequence.
    CondOutsideLink,
    /// ext-phy: the port (its lane held) has no sequence at all.
    PortWithoutSequence(u32),
    /// ext-phy: the port's (lane held) link sequence has no `C`.
    LinkWithoutCond(u32),
}

/// A table that was refused, with the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line of the text.
    pub line: u32,
    /// What is wrong.
    pub kind: ParseErrorKind,
    /// Which table format was being read: it words two messages.
    pub(crate) format: Format,
}

/// The two table formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `ext-macsec 1`.
    ExtMacsec,
    /// `ext-phy 1`.
    ExtPhy,
}

impl Format {
    /// The first word of the header line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ExtMacsec => "ext-macsec",
            Self::ExtPhy => "ext-phy",
        }
    }
}

impl ParseError {
    /// The table format that was being read.
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The C++ parser's message (`line N: ...`), byte for byte: words of the table are repeated
    /// as the bytes they are.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        use ParseErrorKind as K;
        let name = self.format.name();
        out.extend_from_slice(format!("line {}: ", self.line).as_bytes());
        let text: String = match &self.kind {
            K::NotATable => format!("not an {name} table (first line must be '{name} 1')"),
            K::Empty => format!("empty table (first line must be '{name} 1')"),
            K::UnknownKeyword(word) => {
                out.extend_from_slice(b"unknown keyword '");
                out.extend_from_slice(word);
                "'".into()
            }
            K::OperationOutsideSequence => match self.format {
                Format::ExtMacsec => "operation outside a sequence".into(),
                Format::ExtPhy => "operation outside a link / admin-down / admin-up sequence".into(),
            },
            K::SequenceTooLong => match self.format {
                Format::ExtMacsec => "sequence longer than 8192 operations".into(),
                Format::ExtPhy => "sequence longer than 4096 operations".into(),
            },
            K::FieldCount(op) => {
                out.extend_from_slice(b"wrong number of fields for ");
                out.push(*op);
                String::new()
            }
            K::Phy => "phy: 'p' or hex".into(),
            K::DevReg => match self.format {
                Format::ExtMacsec => "dev / reg: hex, dev 1..1f, reg up to ffff".into(),
                Format::ExtPhy => "phy / dev / reg: hex, dev 1..1f, reg up to ffff".into(),
            },
            K::Clause22 => "dev 0 (clause 22) is not allowed".into(),
            K::Value => match self.format {
                Format::ExtMacsec => {
                    "value: hhhh, $n[&mask][|bits] or <byte>.<byte> with bytes hh or <param>[<index>]".into()
                }
                Format::ExtPhy => "value: hex up to ffff".into(),
            },
            K::CompareValue => "value: hex up to ffff".into(),
            K::Mask => "mask: hex up to ffff".into(),
            K::OutsideMask => "value has bits outside the mask: can never match".into(),
            K::Poll => "P: tries 1..1000 and interval 0..100000 usec, decimal".into(),
            K::Delay => "D <usec, decimal, 0..100000>".into(),
            K::Destination => "X destination: $<n> (0..15) or <result>[<word 0..7>]".into(),
            K::Counters => "counters clear-on-read | absolute".into(),
            K::IfgBits => "ifg-bits <decimal, 0..2040>".into(),
            K::MacsecPort => "port <first lane, decimal> <phy, hex>".into(),
            K::MacsecPortTwice(lane) => {
                out.extend_from_slice(b"port ");
                out.extend_from_slice(lane);
                " twice".into()
            }
            K::SeqName => "seq <name> (a-z 0-9 . -)".into(),
            K::SeqTwice(seq) => format!("sequence {seq} twice"),
            K::KeptBeforeRead { seq, slot } => format!("sequence {seq}: ${slot} is written before an X reads it"),
            K::SettingAfterPort(key) => format!("{key} after the first port"),
            K::Setting(key) => {
                format!("{key}: one decimal number (poll-ms 0..60000, up-polls 1..100, holdoff-ms 0..600000)")
            }
            K::PhyPort => "port <first lane, decimal>".into(),
            K::PhyPortTwice(lane, first) => {
                out.extend_from_slice(b"port ");
                out.extend_from_slice(lane);
                format!(" twice (first at line {first})")
            }
            K::SectionBeforePort(key) => format!("{key} before any port"),
            K::SectionArgument(key) => format!("{key} takes no argument"),
            K::SectionTwice(key, lane) => format!("{key} twice in port {lane}"),
            K::CondOutsideLink => "C (link condition) outside a link sequence".into(),
            K::PortWithoutSequence(lane) => format!("port {lane} has no sequence"),
            K::LinkWithoutCond(lane) => format!("port {lane}: link sequence without a C condition"),
        };
        out.extend_from_slice(text.as_bytes());
    }

    /// [`Self::write_to`] as a new vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_to(&mut out);
        out
    }
}

/// The message; bytes of the table that are not UTF-8 are shown lossily.
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.to_bytes()))
    }
}

impl std::error::Error for ParseError {}

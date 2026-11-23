//! The vocabulary of a sequence: registers, bounded numbers, values, operations.

use std::fmt;

pub use mdio_script::Dev;

/// Which PHY an operation addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhySel {
    /// `p`: the PHY of the port the sequence runs for.
    Port,
    /// A fixed MDIO address word of the bus master.
    Fixed(u16),
}

impl PhySel {
    /// The address word, given the PHY of the port the sequence runs for.
    pub const fn resolve(self, port_phy: u16) -> u16 {
        match self {
            Self::Port => port_phy,
            Self::Fixed(phy) => phy,
        }
    }
}

/// One clause-45 register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg {
    /// The PHY.
    pub phy: PhySel,
    /// Device (MMD), 1..=31: clause 22 cannot be expressed.
    pub dev: Dev,
    /// Register.
    pub reg: u16,
}

macro_rules! bounded {
    ($(#[$doc:meta])* $name:ident($int:ty): $min:literal..=$max:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($int);

        impl $name {
            /// The smallest value.
            pub const MIN: $int = $min;
            /// The largest value.
            pub const MAX: $int = $max;

            /// `None` when out of range.
            pub const fn new(value: $int) -> Option<Self> {
                if value >= Self::MIN && value <= Self::MAX { Some(Self(value)) } else { None }
            }

            /// The number.
            pub const fn get(self) -> $int {
                self.0
            }
        }
    };
}

bounded! {
    /// The reads a `P` may make, 1..=1000.
    Tries(u16): 1..=1000
}
bounded! {
    /// A delay or poll interval in microseconds, at most 0.1 s.
    Usec(u32): 0..=100_000
}
bounded! {
    /// A scratch value of one run, `$0`..`$15`.
    Slot(u8): 0..=15
}
bounded! {
    /// The 16-bit word of a named result an `X` fills, 0..=7, least significant word first.
    Word(u8): 0..=7
}

/// A parameter of a sequence: bytes the caller supplies for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamName {
    /// The SAK, 16 or 32 bytes.
    Key,
    /// The GHASH key AES-ECB(SAK, 0^128), 16 bytes.
    Hkey,
    /// The SCI as on the wire, 8 bytes.
    Sci,
    /// The XPN salt, 12 bytes.
    Salt,
    /// The XPN short SCI, 4 bytes.
    Ssci,
    /// The fixed part of the XPN nonce (salt with the SSCI XOR-ed into bytes 0..3), 12 bytes.
    Iv,
    /// The packet number, 8 bytes, least significant byte first.
    Pn,
    /// The replay window, 4 bytes, least significant byte first.
    Window,
}

impl ParamName {
    /// Every parameter, in the order of the discriminants.
    pub const ALL: [Self; 8] = [
        Self::Key,
        Self::Hkey,
        Self::Sci,
        Self::Salt,
        Self::Ssci,
        Self::Iv,
        Self::Pn,
        Self::Window,
    ];

    /// The name as a table writes it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Hkey => "hkey",
            Self::Sci => "sci",
            Self::Salt => "salt",
            Self::Ssci => "ssci",
            Self::Iv => "iv",
            Self::Pn => "pn",
            Self::Window => "window",
        }
    }

    /// The parameter a table calls `name`.
    pub fn from_name(name: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().as_bytes() == name)
    }

    /// The most bytes the parameter can have: a table cannot name an index at or beyond it.
    pub const fn max_len(self) -> u8 {
        match self {
            Self::Key => 32,
            Self::Hkey => 16,
            Self::Sci | Self::Pn => 8,
            Self::Salt | Self::Iv => 12,
            Self::Ssci | Self::Window => 4,
        }
    }

    /// Whether a bus word made of this parameter is key material (the SAK, what is derived from
    /// it, and the salt that MKA distributes with it). A trace must not show such a word.
    pub const fn is_secret(self) -> bool {
        matches!(self, Self::Key | Self::Hkey | Self::Salt | Self::Iv)
    }

    /// Position in [`Self::ALL`].
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for ParamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One byte of a parameter: `<param>[<index>]`, the index below [`ParamName::max_len`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamByte {
    name: ParamName,
    index: u8,
}

impl ParamByte {
    /// `None` when the parameter can never have a byte `index`.
    pub const fn new(name: ParamName, index: u8) -> Option<Self> {
        if index < name.max_len() {
            Some(Self { name, index })
        } else {
            None
        }
    }

    /// The parameter.
    pub const fn name(self) -> ParamName {
        self.name
    }

    /// The byte's index, 0 first.
    pub const fn index(self) -> u8 {
        self.index
    }
}

/// Where one byte of a written word comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ByteSrc {
    /// `hh`: a constant.
    Const(u8),
    /// `<param>[<index>]`.
    Param(ParamByte),
}

impl ByteSrc {
    /// Whether the byte is key material ([`ParamName::is_secret`]).
    pub const fn is_secret(self) -> bool {
        match self {
            Self::Const(_) => false,
            Self::Param(p) => p.name().is_secret(),
        }
    }
}

/// The value a `W` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    /// `hhhh`.
    Const(u16),
    /// `$<n>[&<mask>][|<bits>]`: a value an `X` of this run kept, masked, then OR-ed.
    Kept {
        /// The scratch value.
        slot: Slot,
        /// AND-ed first (`ffff` when not written).
        mask: u16,
        /// OR-ed after (0 when not written).
        bits: u16,
    },
    /// `<byte>.<byte>`: high byte, low byte.
    Bytes {
        /// Bits 15..8.
        hi: ByteSrc,
        /// Bits 7..0.
        lo: ByteSrc,
    },
}

impl Value {
    /// Whether the written word holds key material: a trace shows such a write without its value.
    pub const fn is_secret(&self) -> bool {
        match self {
            Self::Bytes { hi, lo } => hi.is_secret() || lo.is_secret(),
            Self::Const(_) | Self::Kept { .. } => false,
        }
    }
}

/// A comparison of a read value: `(value & mask) == want`. `want` has no bit outside `mask`
/// (such a comparison could never hold, and a table with one is refused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Compare {
    want: u16,
    mask: u16,
}

impl Compare {
    /// `None` when `want` has a bit outside `mask`.
    pub const fn new(want: u16, mask: u16) -> Option<Self> {
        if want & !mask == 0 {
            Some(Self { want, mask })
        } else {
            None
        }
    }

    /// The expected value.
    pub const fn want(self) -> u16 {
        self.want
    }

    /// The bits compared.
    pub const fn mask(self) -> u16 {
        self.mask
    }

    /// Whether `value` passes.
    pub const fn holds(self, value: u16) -> bool {
        value & self.mask == self.want
    }
}

/// The name of a result: one or more of `a-z 0-9 _`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResultName(Box<str>);

impl ResultName {
    /// `None` when empty or with another character.
    pub fn new(name: &[u8]) -> Option<Self> {
        let ok = !name.is_empty() && name.iter().all(|&b| is_name_byte(b));
        let name = std::str::from_utf8(name).ok().filter(|_| ok)?;
        Some(Self(name.into()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResultName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A byte of a parameter or result name.
pub(crate) const fn is_name_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_lowercase() || b.is_ascii_digit()
}

/// Where an `X` puts what it read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dest {
    /// `$<n>`: scratch of this run.
    Kept(Slot),
    /// `<result>[<word>]`: one 16-bit word of a named result.
    Result {
        /// The result.
        name: ResultName,
        /// Which of its words.
        word: Word,
    },
}

/// What an operation does.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OpKind {
    /// `W`: write.
    Write {
        /// The register.
        at: Reg,
        /// What is written.
        value: Value,
    },
    /// `R`: read, value ignored.
    Read {
        /// The register.
        at: Reg,
    },
    /// `E`: read; a value that does not compare stops the sequence (a fault).
    Expect {
        /// The register.
        at: Reg,
        /// The comparison.
        cmp: Compare,
    },
    /// `P`: read until the value compares; never in `tries` reads: a fault.
    Poll {
        /// The register.
        at: Reg,
        /// The comparison.
        cmp: Compare,
        /// Reads at most.
        tries: Tries,
        /// Wait between two reads.
        interval: Usec,
    },
    /// `X` (ext-macsec): read and keep.
    Keep {
        /// The register.
        at: Reg,
        /// Where the value goes.
        dest: Dest,
    },
    /// `D` (ext-macsec): wait.
    Delay(Usec),
    /// `C` (ext-phy link sequences): read; a condition that must hold for "line side up". Never
    /// a fault: the sequence goes on.
    Cond {
        /// The register.
        at: Reg,
        /// The condition.
        cmp: Compare,
    },
}

/// The letter of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpLetter {
    /// Write.
    W,
    /// Read.
    R,
    /// Read and compare.
    E,
    /// Poll.
    P,
    /// Read and keep.
    X,
    /// Delay.
    D,
    /// Link condition.
    C,
}

impl OpLetter {
    /// The letter as a table writes it.
    pub const fn as_char(self) -> char {
        match self {
            Self::W => 'W',
            Self::R => 'R',
            Self::E => 'E',
            Self::P => 'P',
            Self::X => 'X',
            Self::D => 'D',
            Self::C => 'C',
        }
    }
}

impl fmt::Display for OpLetter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Write::write_char(f, self.as_char())
    }
}

/// One operation of a sequence and the table line it came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Op {
    /// 1-based line of the table text (0 for an operation that came from no text).
    pub line: u32,
    /// The operation.
    pub kind: OpKind,
}

impl Op {
    /// The operation's letter.
    pub const fn letter(&self) -> OpLetter {
        match self.kind {
            OpKind::Write { .. } => OpLetter::W,
            OpKind::Read { .. } => OpLetter::R,
            OpKind::Expect { .. } => OpLetter::E,
            OpKind::Poll { .. } => OpLetter::P,
            OpKind::Keep { .. } => OpLetter::X,
            OpKind::Delay(_) => OpLetter::D,
            OpKind::Cond { .. } => OpLetter::C,
        }
    }

    /// The register the operation accesses (`None` for a delay).
    pub const fn reg(&self) -> Option<Reg> {
        match self.kind {
            OpKind::Write { at, .. }
            | OpKind::Read { at }
            | OpKind::Expect { at, .. }
            | OpKind::Poll { at, .. }
            | OpKind::Keep { at, .. }
            | OpKind::Cond { at, .. } => Some(at),
            OpKind::Delay(_) => None,
        }
    }
}

impl fmt::Display for PhySel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port => f.write_str("p"),
            Self::Fixed(phy) => write!(f, "{phy:x}"),
        }
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:x} {:x}", self.phy, self.dev.get(), self.reg)
    }
}

impl fmt::Display for ByteSrc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Const(c) => write!(f, "{c:02x}"),
            Self::Param(p) => write!(f, "{}[{}]", p.name(), p.index()),
        }
    }
}

/// The value as a table writes it. A table's text holds no key: parameters appear by name.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Const(c) => write!(f, "{c:x}"),
            Self::Kept { slot, mask, bits } => write!(f, "${}&{mask:x}|{bits:x}", slot.get()),
            Self::Bytes { hi, lo } => write!(f, "{hi}.{lo}"),
        }
    }
}

/// The operation as a line of a table (without the line number): parsing it again gives the
/// same operation.
impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = self.letter();
        match &self.kind {
            OpKind::Write { at, value } => write!(f, "{letter} {at} {value}"),
            OpKind::Read { at } => write!(f, "{letter} {at}"),
            OpKind::Expect { at, cmp } | OpKind::Cond { at, cmp } => {
                write!(f, "{letter} {at} {:x} {:x}", cmp.want(), cmp.mask())
            }
            OpKind::Poll {
                at,
                cmp,
                tries,
                interval,
            } => {
                write!(
                    f,
                    "{letter} {at} {:x} {:x} {} {}",
                    cmp.want(),
                    cmp.mask(),
                    tries.get(),
                    interval.get()
                )
            }
            OpKind::Keep {
                at,
                dest: Dest::Kept(slot),
            } => write!(f, "{letter} {at} ${}", slot.get()),
            OpKind::Keep {
                at,
                dest: Dest::Result { name, word },
            } => write!(f, "{letter} {at} {name}[{}]", word.get()),
            OpKind::Delay(us) => write!(f, "{letter} {}", us.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds() {
        assert!(Tries::new(0).is_none() && Tries::new(1001).is_none());
        assert_eq!(Tries::new(1000).map(Tries::get), Some(1000));
        assert!(Usec::new(100_001).is_none() && Usec::new(0).is_some());
        assert!(Slot::new(16).is_none() && Word::new(8).is_none());
        assert!(Compare::new(3, 1).is_none());
        assert!(Compare::new(1, 3).is_some_and(|c| c.holds(0xfffd) && !c.holds(3)));
    }

    #[test]
    fn parameters() {
        for p in ParamName::ALL {
            assert_eq!(ParamName::from_name(p.name().as_bytes()), Some(p));
            assert!(ParamByte::new(p, p.max_len()).is_none());
            assert!(ParamByte::new(p, p.max_len() - 1).is_some());
            assert_eq!(ParamName::ALL.get(p.index()), Some(&p));
        }
        assert_eq!(ParamName::from_name(b"secret"), None);
        assert_eq!(ParamName::from_name(b"Key"), None);
    }

    #[test]
    fn what_counts_as_key_material() {
        let byte = |name, index| ByteSrc::Param(ParamByte::new(name, index).unwrap());
        assert!(
            Value::Bytes {
                hi: ByteSrc::Const(0),
                lo: byte(ParamName::Key, 31)
            }
            .is_secret()
        );
        assert!(
            Value::Bytes {
                hi: byte(ParamName::Hkey, 0),
                lo: ByteSrc::Const(0)
            }
            .is_secret()
        );
        assert!(
            Value::Bytes {
                hi: byte(ParamName::Salt, 0),
                lo: byte(ParamName::Ssci, 0)
            }
            .is_secret()
        );
        assert!(
            Value::Bytes {
                hi: byte(ParamName::Iv, 0),
                lo: byte(ParamName::Iv, 1)
            }
            .is_secret()
        );
        assert!(
            !Value::Bytes {
                hi: byte(ParamName::Sci, 0),
                lo: byte(ParamName::Pn, 1)
            }
            .is_secret()
        );
        assert!(
            !Value::Bytes {
                hi: byte(ParamName::Window, 0),
                lo: byte(ParamName::Ssci, 1)
            }
            .is_secret()
        );
        assert!(!Value::Const(0x1234).is_secret());
    }

    #[test]
    fn result_names() {
        assert!(ResultName::new(b"in_pkts_ok").is_some());
        assert!(ResultName::new(b"").is_none());
        assert!(ResultName::new(b"Pn").is_none());
        assert!(ResultName::new(b"a.b").is_none());
        assert!(ResultName::new(b"a\0").is_none());
    }
}

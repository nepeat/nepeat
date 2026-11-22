//! The vocabulary of a script: addresses, bounded numbers, operations, statistics, the bus.

use std::fmt;

/// A clause-45 device (MMD) number, 1..=31. Zero, which would be a clause-22 access, has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dev(u8);

impl Dev {
    /// The largest device number.
    pub const MAX: u8 = 31;

    /// `None` unless `1 <= dev <= 31`.
    pub const fn new(dev: u8) -> Option<Self> {
        if dev >= 1 && dev <= Self::MAX {
            Some(Self(dev))
        } else {
            None
        }
    }

    /// The device number, 1..=31.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A delay or poll interval in microseconds, at most 60 s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DelayUs(u32);

impl DelayUs {
    /// The longest delay a script may ask for.
    pub const MAX: u32 = 60_000_000;

    /// `None` when longer than [`Self::MAX`].
    pub const fn new(us: u32) -> Option<Self> {
        if us <= Self::MAX { Some(Self(us)) } else { None }
    }

    /// Microseconds.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The number of reads a poll may make, 1..=1000000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tries(u32);

impl Tries {
    /// The largest number of reads.
    pub const MAX: u32 = 1_000_000;

    /// `None` unless `1 <= tries <= 1000000`.
    pub const fn new(tries: u32) -> Option<Self> {
        if tries >= 1 && tries <= Self::MAX {
            Some(Self(tries))
        } else {
            None
        }
    }

    /// The number of reads.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The text of an `L` line: at most 150 bytes, no NUL, no line feed. Bytes, not `str`: the C code
/// never looked at the encoding. [`fmt::Display`] shows it lossily.
///
/// A label made with [`Label::new`] does not end in a carriage return, and so survives being
/// written out and parsed again. One the parser made can: a line's trailing CRs are stripped
/// before the text is cut to 150 bytes, and the cut may land inside a run of CRs (C keeps those
/// bytes, and so does this type, so that messages match). Writing such a label out and parsing
/// it again strips them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Label(Vec<u8>);

impl Label {
    /// Longer label text is cut to this many bytes by the parser.
    pub const MAX_LEN: usize = 150;

    /// `None` when the bytes are too long, hold a NUL or a line feed, or end in a carriage return.
    pub fn new(text: impl AsRef<[u8]>) -> Option<Self> {
        let text = text.as_ref();
        let ok =
            text.len() <= Self::MAX_LEN && !text.iter().any(|&b| b == 0 || b == b'\n') && text.last() != Some(&b'\r');
        ok.then(|| Self(text.to_vec()))
    }

    /// What the parser stores for the rest of an `L` line (already without NUL and line end).
    pub(crate) fn cut(text: &[u8]) -> Self {
        Self(text.iter().copied().take(Self::MAX_LEN).collect())
    }

    /// The label in force before the first `L` line.
    pub(crate) fn start() -> Self {
        Self(b"(start)".to_vec())
    }

    /// The label text.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The first `max` bytes, for a message (C: `%.60s`).
    pub(crate) fn prefix(&self, max: usize) -> String {
        String::from_utf8_lossy(self.0.get(..max).unwrap_or(&self.0)).into_owned()
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

/// One clause-45 register of one PHY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegAddr {
    /// The bus master's address word (`OpenBCM`: bus in bits 8, 6:5, address in bits 4:0).
    pub phy: u16,
    /// Device (MMD).
    pub dev: Dev,
    /// Register.
    pub reg: u16,
}

/// When a poll is satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Until {
    /// `P`: until `(value & mask) == want`.
    Equal,
    /// `N`: until `(value & mask) != want`.
    NotEqual,
}

impl Until {
    /// Whether `got` satisfies the poll.
    pub const fn matches(self, got: u16, mask: u16, want: u16) -> bool {
        match self {
            Self::Equal => got & mask == want,
            Self::NotEqual => got & mask != want,
        }
    }

    /// The operation letter of such a poll.
    pub const fn letter(self) -> OpLetter {
        match self {
            Self::Equal => OpLetter::P,
            Self::NotEqual => OpLetter::N,
        }
    }
}

/// The letter of a bus operation, as a failure record names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpLetter {
    /// Write.
    W,
    /// Read, value ignored.
    R,
    /// Read and compare, mismatches counted.
    E,
    /// Poll until equal.
    P,
    /// Poll until not equal.
    N,
}

impl OpLetter {
    /// The letter as written in a script.
    pub const fn as_char(self) -> char {
        match self {
            Self::W => 'W',
            Self::R => 'R',
            Self::E => 'E',
            Self::P => 'P',
            Self::N => 'N',
        }
    }
}

/// One operation of a script.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op {
    /// `L`: names the step that follows in messages.
    Label(Label),
    /// `D`: wait.
    Delay(DelayUs),
    /// `W`: write `val`.
    Write {
        /// The register.
        at: RegAddr,
        /// The value written.
        val: u16,
    },
    /// `R`: read, value ignored.
    Read {
        /// The register.
        at: RegAddr,
    },
    /// `E`: read; `(v & mask) != val` is counted and logged, never an error.
    Expect {
        /// The register.
        at: RegAddr,
        /// The recorded value.
        val: u16,
        /// The bits compared (`ffff` when the line has no mask).
        mask: u16,
    },
    /// `P` / `N`: read until the condition holds; the script fails after `tries` reads.
    Poll {
        /// The register.
        at: RegAddr,
        /// `P` or `N`.
        until: Until,
        /// The value compared with.
        val: u16,
        /// The bits compared.
        mask: u16,
        /// Reads at most; 1 makes it an assertion.
        tries: Tries,
        /// Wait between two reads.
        interval: DelayUs,
    },
}

impl Op {
    /// A single-try `P` / `N`: the kind of line a verification section is made of.
    pub fn is_assertion(&self) -> bool {
        matches!(self, Self::Poll { tries, .. } if tries.get() == 1)
    }

    /// The script line of this operation, without a line end. A label is written byte for byte.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        let text = match self {
            Self::Label(l) => {
                out.push(b'L');
                if !l.as_bytes().is_empty() {
                    out.push(b' ');
                    out.extend_from_slice(l.as_bytes());
                }
                return;
            }
            Self::Delay(us) => format!("D {}", us.get()),
            Self::Write { at, val } => format!("W {:x} {:x} {:x} {val:x}", at.phy, at.dev.get(), at.reg),
            Self::Read { at } => format!("R {:x} {:x} {:x}", at.phy, at.dev.get(), at.reg),
            Self::Expect { at, val, mask } => {
                format!("E {:x} {:x} {:x} {val:x} {mask:x}", at.phy, at.dev.get(), at.reg)
            }
            Self::Poll {
                at,
                until,
                val,
                mask,
                tries,
                interval,
            } => format!(
                "{} {:x} {:x} {:x} {val:x} {mask:x} {} {}",
                until.letter().as_char(),
                at.phy,
                at.dev.get(),
                at.reg,
                tries.get(),
                interval.get()
            ),
        };
        out.extend_from_slice(text.as_bytes());
    }
}

/// The script line of the operation; a label that is not UTF-8 is shown lossily (see [`Op::write_to`]).
impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bytes = Vec::new();
        self.write_to(&mut bytes);
        f.write_str(&String::from_utf8_lossy(&bytes))
    }
}

/// An operation and the 1-based script line it came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Line {
    /// 1-based line of the script text.
    pub line: u64,
    /// The operation.
    pub op: Op,
}

/// Counters of a parse pass or of a run (the C `mdio_script_stats_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Stats {
    /// `W R E P N` lines.
    pub lines: u64,
    /// `W` lines.
    pub writes: u64,
    /// Reads: one per `R E P N` line, plus the re-reads of polls in a run.
    pub reads: u64,
    /// `D` lines.
    pub delays: u64,
    /// `P N` lines.
    pub polls: u64,
    /// `L` lines.
    pub labels: u64,
    /// Reads beyond the first of a `P` / `N` (run only).
    pub poll_rereads: u64,
    /// `E` mismatches (run only).
    pub differs: u64,
    /// Microseconds requested by `D` lines.
    pub delay_us: u64,
    /// `W R E P N D` lines seen (parse) or completed (run).
    pub ops: u64,
    /// First line of the verification section, 0 = none (after a run: the line the run used, set
    /// only when the script reached its end).
    pub verify_line: u64,
    /// Assertions in the verification section (parse) or read (run).
    pub verify_checks: u64,
    /// Of those, mismatches and read errors (run only).
    pub verify_failed: u64,
}

pub(crate) fn bump(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}

impl Stats {
    /// Counts a line as both passes do when they reach it; `ops` is counted when it is done.
    pub(crate) fn count(&mut self, op: &Op) {
        match op {
            Op::Label(_) => bump(&mut self.labels),
            Op::Delay(us) => {
                bump(&mut self.delays);
                self.delay_us = self.delay_us.saturating_add(u64::from(us.get()));
            }
            Op::Write { .. } => {
                bump(&mut self.lines);
                bump(&mut self.writes);
            }
            Op::Read { .. } | Op::Expect { .. } => {
                bump(&mut self.lines);
                bump(&mut self.reads);
            }
            Op::Poll { .. } => {
                bump(&mut self.lines);
                bump(&mut self.polls);
                bump(&mut self.reads);
            }
        }
    }
}

/// A bus access failed. The interpreter needs no more than the fact; it stops the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BusError;

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MDIO bus access failed")
    }
}

impl std::error::Error for BusError {}

/// The clause-45 MDIO bus master a script runs against.
pub trait MdioBus {
    /// Reads `dev.reg` of the PHY at address word `phy`.
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError>;
    /// Writes `val` to `dev.reg` of the PHY at address word `phy`.
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError>;
}

impl<B: MdioBus + ?Sized> MdioBus for &mut B {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        (**self).read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError> {
        (**self).write(phy, dev, reg, val)
    }
}

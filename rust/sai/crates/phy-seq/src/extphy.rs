//! The external PHY table (`ext-phy 1`): see the crate documentation for the format.

use crate::error::{Format, ParseError, ParseErrorKind as K};
use crate::lex::{self, Radix, num, num16};
use crate::op::{Compare, Dev, Op, OpKind, PhySel, Reg, Tries, Usec, Value};

/// The most operations a sequence may have.
pub const MAX_OPS: usize = 4096;

/// One of the three sequences of a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// `link`: run from the link scan while the port is admin up; its `C` lines are the
    /// conditions for "line side up".
    Link,
    /// `admin-down`: run when the port goes admin down.
    AdminDown,
    /// `admin-up`: run before the ASIC port is enabled.
    AdminUp,
}

impl Section {
    /// The three sections, in the order the C++ `ExtPort` lists them.
    pub const ALL: [Self; 3] = [Self::Link, Self::AdminDown, Self::AdminUp];

    /// The keyword that starts the section.
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::AdminDown => "admin-down",
            Self::AdminUp => "admin-up",
        }
    }
}

/// A port of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtPhyPort {
    lane: u32,
    line: u32,
    link: Vec<Op>,
    admin_down: Vec<Op>,
    admin_up: Vec<Op>,
}

impl ExtPhyPort {
    /// The port's first lane (SDK physical port = `SONiC` lane).
    pub const fn lane(&self) -> u32 {
        self.lane
    }

    /// The table line of its `port` line.
    pub const fn line(&self) -> u32 {
        self.line
    }

    /// The operations of a section; empty when the port has none. Every PHY is fixed
    /// ([`PhySel::Fixed`]), and a non-empty link sequence has at least one `C`.
    pub fn ops(&self, section: Section) -> &[Op] {
        match section {
            Section::Link => &self.link,
            Section::AdminDown => &self.admin_down,
            Section::AdminUp => &self.admin_up,
        }
    }

    fn ops_mut(&mut self, section: Section) -> &mut Vec<Op> {
        match section {
            Section::Link => &mut self.link,
            Section::AdminDown => &mut self.admin_down,
            Section::AdminUp => &mut self.admin_up,
        }
    }
}

/// A parsed external PHY table: the proof that every line is well formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtPhyTable {
    poll_ms: u32,
    up_polls: u32,
    holdoff_ms: u32,
    ports: Vec<ExtPhyPort>,
}

impl Default for ExtPhyTable {
    fn default() -> Self {
        Self {
            poll_ms: 500,
            up_polls: 2,
            holdoff_ms: 3000,
            ports: Vec::new(),
        }
    }
}

impl ExtPhyTable {
    /// Parses the table text (bytes: the C++ parser never looked at the encoding).
    pub fn parse(text: &[u8]) -> Result<Self, ParseError> {
        let mut table = Self::default();
        let mut header = false;
        let mut section: Option<Section> = None;
        let mut n: u32 = 0;
        for line in lex::lines(text) {
            n = n.saturating_add(1);
            let fail = |kind| ParseError {
                line: n,
                kind,
                format: Format::ExtPhy,
            };
            let words = lex::words(line);
            let Some((&key, args)) = words.split_first() else {
                continue;
            };
            if key.first() == Some(&b'#') {
                continue;
            }
            if header {
                table.line(&mut section, n, key, args).map_err(fail)?;
            } else if matches!(args, [b"1"]) && key == b"ext-phy" {
                header = true;
            } else {
                return Err(fail(K::NotATable));
            }
        }
        if !header {
            return Err(ParseError {
                line: 1,
                kind: K::Empty,
                format: Format::ExtPhy,
            });
        }
        table.check_ports()?;
        Ok(table)
    }

    /// One line after the header: `key` and its arguments. `section` is the sequence operations
    /// go to (none before the first section keyword of a port).
    fn line(&mut self, section: &mut Option<Section>, n: u32, key: &[u8], args: &[&[u8]]) -> Result<(), K> {
        match key {
            b"poll-ms" | b"up-polls" | b"holdoff-ms" => {
                let (name, max, min, to): (_, _, _, &mut u32) = match key {
                    b"poll-ms" => ("poll-ms", 60_000, 0, &mut self.poll_ms),
                    b"up-polls" => ("up-polls", 100, 1, &mut self.up_polls),
                    _ => ("holdoff-ms", 600_000, 0, &mut self.holdoff_ms),
                };
                if !self.ports.is_empty() {
                    return Err(K::SettingAfterPort(name));
                }
                let [value] = args else { return Err(K::Setting(name)) };
                *to = num(value, Radix::Dec, max)
                    .filter(|&v| v >= min)
                    .ok_or(K::Setting(name))?;
            }
            b"port" => {
                let [lane_word] = args else { return Err(K::PhyPort) };
                let lane = num(lane_word, Radix::Dec, 100_000).ok_or(K::PhyPort)?;
                if let Some(first) = self.ports.iter().find(|p| p.lane == lane) {
                    return Err(K::PhyPortTwice(lane_word.to_vec(), first.line));
                }
                self.ports.push(ExtPhyPort {
                    lane,
                    line: n,
                    link: Vec::new(),
                    admin_down: Vec::new(),
                    admin_up: Vec::new(),
                });
                *section = None;
            }
            b"link" | b"admin-down" | b"admin-up" => {
                let sec = match key {
                    b"link" => Section::Link,
                    b"admin-down" => Section::AdminDown,
                    _ => Section::AdminUp,
                };
                let Some(port) = self.ports.last() else {
                    return Err(K::SectionBeforePort(sec.keyword()));
                };
                if !args.is_empty() {
                    return Err(K::SectionArgument(sec.keyword()));
                }
                if !port.ops(sec).is_empty() {
                    return Err(K::SectionTwice(sec.keyword(), port.lane));
                }
                *section = Some(sec);
            }
            [letter @ (b'W' | b'R' | b'E' | b'P' | b'C')] => {
                let (Some(sec), Some(port)) = (*section, self.ports.last_mut()) else {
                    return Err(K::OperationOutsideSequence);
                };
                let kind = parse_op(*letter, args, sec)?;
                let ops = port.ops_mut(sec);
                if ops.len() >= MAX_OPS {
                    return Err(K::SequenceTooLong);
                }
                ops.push(Op { line: n, kind });
            }
            _ => return Err(K::UnknownKeyword(key.to_vec())),
        }
        Ok(())
    }

    /// Every port needs a sequence, and a link sequence a condition.
    fn check_ports(&self) -> Result<(), ParseError> {
        for port in &self.ports {
            let fail = |kind| ParseError {
                line: port.line,
                kind,
                format: Format::ExtPhy,
            };
            if Section::ALL.iter().all(|&s| port.ops(s).is_empty()) {
                return Err(fail(K::PortWithoutSequence(port.lane)));
            }
            if !port.link.is_empty() && !port.link.iter().any(|op| matches!(op.kind, OpKind::Cond { .. })) {
                return Err(fail(K::LinkWithoutCond(port.lane)));
            }
        }
        Ok(())
    }

    /// `poll-ms`: the line-side poll period (default 500; 0 = every link scan pass).
    pub const fn poll_ms(&self) -> u32 {
        self.poll_ms
    }

    /// `up-polls`: consecutive good polls before "up", 1..=100 (default 2).
    pub const fn up_polls(&self) -> u32 {
        self.up_polls
    }

    /// `holdoff-ms`: the pause after a raw MDIO command of the diag socket (default 3000).
    pub const fn holdoff_ms(&self) -> u32 {
        self.holdoff_ms
    }

    /// The ports, in table order.
    pub fn ports(&self) -> &[ExtPhyPort] {
        &self.ports
    }
}

/// The fields after the operation letter, checked in the order of the C++ parser.
fn parse_op(letter: u8, a: &[&[u8]], section: Section) -> Result<OpKind, K> {
    let fields = match letter {
        b'W' => a.len() == 4,
        b'R' => a.len() == 3,
        b'P' => a.len() == 7,
        b'C' => a.len() == 5,
        _ => a.len() == 4 || a.len() == 5,
    };
    let (true, Some(phy), Some(dev), Some(reg)) = (fields, a.first(), a.get(1), a.get(2)) else {
        return Err(K::FieldCount(letter));
    };
    let phy = num16(phy, Radix::Hex, 0xffff).ok_or(K::DevReg)?;
    let dev = num(dev, Radix::Hex, 31)
        .and_then(|d| u8::try_from(d).ok())
        .ok_or(K::DevReg)?;
    let reg = num16(reg, Radix::Hex, 0xffff).ok_or(K::DevReg)?;
    let at = Reg {
        phy: PhySel::Fixed(phy),
        dev: Dev::new(dev).ok_or(K::Clause22)?,
        reg,
    };
    let field = |i: usize| a.get(i).copied().unwrap_or(&[]);
    if letter == b'R' {
        return Ok(OpKind::Read { at });
    }
    let value = num16(field(3), Radix::Hex, 0xffff).ok_or(K::Value)?;
    let mask = if a.len() >= 5 {
        num16(field(4), Radix::Hex, 0xffff).ok_or(K::Mask)?
    } else {
        0xffff
    };
    let poll = if letter == b'P' {
        let tries = num16(field(5), Radix::Dec, Tries::MAX)
            .and_then(Tries::new)
            .ok_or(K::Poll)?;
        let interval = num(field(6), Radix::Dec, Usec::MAX)
            .and_then(Usec::new)
            .ok_or(K::Poll)?;
        Some((tries, interval))
    } else {
        None
    };
    if letter == b'W' {
        return Ok(OpKind::Write {
            at,
            value: Value::Const(value),
        });
    }
    let cmp = Compare::new(value, mask).ok_or(K::OutsideMask)?;
    Ok(match (letter, poll) {
        (b'C', _) if section != Section::Link => return Err(K::CondOutsideLink),
        (b'C', _) => OpKind::Cond { at, cmp },
        (_, Some((tries, interval))) => OpKind::Poll {
            at,
            cmp,
            tries,
            interval,
        },
        _ => OpKind::Expect { at, cmp },
    })
}

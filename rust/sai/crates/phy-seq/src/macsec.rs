//! The external `MACsec` table (`ext-macsec 1`): see the crate documentation for the format.

use std::collections::BTreeMap;

use crate::error::{Format, ParseError, ParseErrorKind as K};
use crate::exec::{self, Bus, ExecError, Results};
use crate::lex::{self, Radix, num, num16};
use crate::op::{
    ByteSrc, Compare, Dest, Dev, Op, OpKind, ParamByte, ParamName, PhySel, Reg, ResultName, Slot, Tries, Usec, Value,
    Word, is_name_byte,
};
use crate::params::Params;

/// The most operations a sequence may have.
pub const MAX_OPS: usize = 8192;

/// How the PHY's counters behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CounterMode {
    /// A read clears them: the SAI layer accumulates (the default).
    #[default]
    ClearOnRead,
    /// They keep counting.
    Absolute,
}

/// One named sequence of a parsed table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sequence {
    ops: Vec<Op>,
}

impl Sequence {
    /// The operations, in order.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// Runs the sequence for the port whose PHY has the address word `phy` and returns its
    /// named results. See [`exec::run`] for the rules.
    pub fn run<B: Bus>(&self, bus: &mut B, phy: u16, params: &Params) -> Result<Results, ExecError<B::Error>> {
        exec::run(bus, &self.ops, phy, params).map(|out| out.results)
    }

    /// The parameters the sequence needs and how many bytes of each.
    pub fn required_params(&self) -> Vec<(ParamName, u8)> {
        exec::required_params(&self.ops)
    }
}

/// A parsed external `MACsec` table: the proof that every line is well formed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacsecTable {
    counters: CounterMode,
    ifg_bits: u16,
    ports: BTreeMap<u32, u16>,
    seqs: BTreeMap<String, Sequence>,
}

impl MacsecTable {
    /// Parses the table text. Bytes, not `str`: the C++ parser never looked at the encoding
    /// (comments may hold anything).
    pub fn parse(text: &[u8]) -> Result<Self, ParseError> {
        let mut table = Self::default();
        let mut header = false;
        let mut current: Option<String> = None;
        let mut n: u32 = 0;
        for line in lex::lines(text) {
            n = n.saturating_add(1);
            let fail = |kind| ParseError {
                line: n,
                kind,
                format: Format::ExtMacsec,
            };
            let words = lex::words(line);
            let Some((&key, args)) = words.split_first() else {
                continue;
            };
            if key.first() == Some(&b'#') {
                continue;
            }
            if !header {
                if !matches!(args, [b"1"]) || key != b"ext-macsec" {
                    return Err(fail(K::NotATable));
                }
                header = true;
                continue;
            }
            match key {
                b"counters" => {
                    table.counters = match args {
                        [b"clear-on-read"] => CounterMode::ClearOnRead,
                        [b"absolute"] => CounterMode::Absolute,
                        _ => return Err(fail(K::Counters)),
                    };
                }
                b"ifg-bits" => {
                    let [bits] = args else { return Err(fail(K::IfgBits)) };
                    table.ifg_bits = num16(bits, Radix::Dec, 2040).ok_or_else(|| fail(K::IfgBits))?;
                }
                b"port" => {
                    let [lane_word, phy_word] = args else {
                        return Err(fail(K::MacsecPort));
                    };
                    let lane = num(lane_word, Radix::Dec, 100_000).ok_or_else(|| fail(K::MacsecPort))?;
                    let phy = num16(phy_word, Radix::Hex, 0xffff).ok_or_else(|| fail(K::MacsecPort))?;
                    if table.ports.contains_key(&lane) {
                        return Err(fail(K::MacsecPortTwice(lane_word.to_vec())));
                    }
                    table.ports.insert(lane, phy);
                }
                b"seq" => {
                    let [name] = args else { return Err(fail(K::SeqName)) };
                    let name = seq_name(name).ok_or_else(|| fail(K::SeqName))?;
                    if table.seqs.contains_key(name) {
                        return Err(fail(K::SeqTwice(name.to_owned())));
                    }
                    table.seqs.insert(name.to_owned(), Sequence::default());
                    current = Some(name.to_owned());
                }
                [letter @ (b'W' | b'R' | b'E' | b'P' | b'X' | b'D')] => {
                    let Some(seq) = current.as_ref().and_then(|name| table.seqs.get_mut(name)) else {
                        return Err(fail(K::OperationOutsideSequence));
                    };
                    if seq.ops.len() >= MAX_OPS {
                        return Err(fail(K::SequenceTooLong));
                    }
                    let kind = parse_op(*letter, args).map_err(fail)?;
                    seq.ops.push(Op { line: n, kind });
                }
                _ => return Err(fail(K::UnknownKeyword(key.to_vec()))),
            }
        }
        if !header {
            return Err(ParseError {
                line: 1,
                kind: K::Empty,
                format: Format::ExtMacsec,
            });
        }
        table.check_kept()?;
        Ok(table)
    }

    /// A kept value must be read before it is written, in every sequence.
    fn check_kept(&self) -> Result<(), ParseError> {
        for (name, seq) in &self.seqs {
            let mut have = 0u16;
            for op in &seq.ops {
                match &op.kind {
                    OpKind::Keep {
                        dest: Dest::Kept(slot), ..
                    } => have |= 1 << slot.get(),
                    OpKind::Write {
                        value: Value::Kept { slot, .. },
                        ..
                    } if have & (1 << slot.get()) == 0 => {
                        let kind = K::KeptBeforeRead {
                            seq: name.clone(),
                            slot: slot.get(),
                        };
                        return Err(ParseError {
                            line: op.line,
                            kind,
                            format: Format::ExtMacsec,
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// How the PHY's counters behave (`counters`; default clear-on-read).
    pub const fn counters(&self) -> CounterMode {
        self.counters
    }

    /// `ifg-bits`: the average inter-frame gap of the ASIC MAC, in bit times, while a port has a
    /// `MACsec` port object; 0 (also the default) = leave the gap alone.
    pub const fn ifg_bits(&self) -> u16 {
        self.ifg_bits
    }

    /// The ports that can do `MACsec`: (first lane, the PHY's MDIO address word), by lane.
    pub fn ports(&self) -> impl ExactSizeIterator<Item = (u32, u16)> {
        self.ports.iter().map(|(&lane, &phy)| (lane, phy))
    }

    /// The PHY of the port whose first lane is `lane`.
    pub fn phy_of_lane(&self, lane: u32) -> Option<u16> {
        self.ports.get(&lane).copied()
    }

    /// A sequence by name; `None` when the table has none (many are optional).
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.seqs.get(name)
    }

    /// Every sequence, in name order (bytewise, the order of the C++ `std::map`).
    pub fn sequences(&self) -> impl ExactSizeIterator<Item = (&str, &Sequence)> {
        self.seqs.iter().map(|(name, seq)| (name.as_str(), seq))
    }
}

/// `a-z 0-9 . -`, 1..80 characters.
fn seq_name(name: &[u8]) -> Option<&str> {
    let ok = (1..=80).contains(&name.len())
        && name
            .iter()
            .all(|&b| b == b'.' || b == b'-' || b.is_ascii_lowercase() || b.is_ascii_digit());
    std::str::from_utf8(name).ok().filter(|_| ok)
}

/// `name[index]` with the index decimal and at most `max`.
fn indexed(s: &[u8], max: u32) -> Option<(&[u8], u8)> {
    let open = s.iter().position(|&b| b == b'[')?;
    if open == 0 || s.last() != Some(&b']') {
        return None;
    }
    let name = s.get(..open)?;
    if !name.iter().all(|&b| is_name_byte(b)) {
        return None;
    }
    let index = num(s.get(open + 1..s.len() - 1)?, Radix::Dec, max)?;
    Some((name, u8::try_from(index).ok()?))
}

fn parse_byte(s: &[u8]) -> Option<ByteSrc> {
    if s.len() == 2 {
        if let Some(v) = num(s, Radix::Hex, 0xff) {
            return u8::try_from(v).ok().map(ByteSrc::Const);
        }
    }
    let (name, index) = indexed(s, 31)?;
    ParamByte::new(ParamName::from_name(name)?, index).map(ByteSrc::Param)
}

fn parse_value(s: &[u8]) -> Option<Value> {
    if s.first() == Some(&b'$') {
        let amp = s.iter().position(|&b| b == b'&');
        let bar = s.iter().position(|&b| b == b'|');
        let end = match (amp, bar) {
            (Some(a), Some(o)) => a.min(o),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => s.len(),
        };
        let slot = num(s.get(1..end)?, Radix::Dec, 15)
            .and_then(|v| u8::try_from(v).ok())
            .and_then(Slot::new)?;
        let mut mask = 0xffff;
        if let Some(a) = amp {
            if bar.is_some_and(|o| o < a) {
                return None;
            }
            mask = num16(s.get(a + 1..bar.unwrap_or(s.len()))?, Radix::Hex, 0xffff)?;
        }
        let bits = match bar {
            Some(o) => num16(s.get(o + 1..)?, Radix::Hex, 0xffff)?,
            None => 0,
        };
        return Some(Value::Kept { slot, mask, bits });
    }
    if let Some(dot) = s.iter().position(|&b| b == b'.') {
        let hi = parse_byte(s.get(..dot)?)?;
        let lo = parse_byte(s.get(dot + 1..)?)?;
        return Some(Value::Bytes { hi, lo });
    }
    num16(s, Radix::Hex, 0xffff).map(Value::Const)
}

fn parse_dest(s: &[u8]) -> Option<Dest> {
    if let [b'$', rest @ ..] = s {
        if let Some(slot) = num(rest, Radix::Dec, 15)
            .and_then(|v| u8::try_from(v).ok())
            .and_then(Slot::new)
        {
            return Some(Dest::Kept(slot));
        }
    }
    let (name, word) = indexed(s, 7)?;
    Some(Dest::Result {
        name: ResultName::new(name)?,
        word: Word::new(word)?,
    })
}

/// The fields after the operation letter, checked in the order of the C++ parser (the first
/// thing wrong is the one reported).
fn parse_op(letter: u8, a: &[&[u8]]) -> Result<OpKind, K> {
    if letter == b'D' {
        let [us] = a else { return Err(K::Delay) };
        return num(us, Radix::Dec, Usec::MAX)
            .and_then(Usec::new)
            .map(OpKind::Delay)
            .ok_or(K::Delay);
    }
    let fields = match letter {
        b'W' | b'X' => a.len() == 4,
        b'R' => a.len() == 3,
        b'P' => a.len() == 7,
        _ => a.len() == 4 || a.len() == 5,
    };
    let (true, Some(phy), Some(dev), Some(reg)) = (fields, a.first(), a.get(1), a.get(2)) else {
        return Err(K::FieldCount(letter));
    };
    let phy = if *phy == b"p" {
        PhySel::Port
    } else {
        PhySel::Fixed(num16(phy, Radix::Hex, 0xffff).ok_or(K::Phy)?)
    };
    let dev = num(dev, Radix::Hex, 31)
        .and_then(|d| u8::try_from(d).ok())
        .ok_or(K::DevReg)?;
    let reg = num16(reg, Radix::Hex, 0xffff).ok_or(K::DevReg)?;
    let at = Reg {
        phy,
        dev: Dev::new(dev).ok_or(K::Clause22)?,
        reg,
    };
    let field = |i: usize| a.get(i).copied().unwrap_or(&[]);
    Ok(match letter {
        b'W' => OpKind::Write {
            at,
            value: parse_value(field(3)).ok_or(K::Value)?,
        },
        b'R' => OpKind::Read { at },
        b'X' => OpKind::Keep {
            at,
            dest: parse_dest(field(3)).ok_or(K::Destination)?,
        },
        _ => {
            let want = num16(field(3), Radix::Hex, 0xffff).ok_or(K::CompareValue)?;
            let mask = if letter == b'P' || a.len() == 5 {
                num16(field(4), Radix::Hex, 0xffff).ok_or(K::Mask)?
            } else {
                0xffff
            };
            let cmp = Compare::new(want, mask).ok_or(K::OutsideMask)?;
            if letter == b'P' {
                let tries = num16(field(5), Radix::Dec, Tries::MAX)
                    .and_then(Tries::new)
                    .ok_or(K::Poll)?;
                let interval = num(field(6), Radix::Dec, Usec::MAX)
                    .and_then(Usec::new)
                    .ok_or(K::Poll)?;
                OpKind::Poll {
                    at,
                    cmp,
                    tries,
                    interval,
                }
            } else {
                OpKind::Expect { at, cmp }
            }
        }
    })
}

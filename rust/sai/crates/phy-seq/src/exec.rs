//! Running a sequence over a bus.

use std::collections::BTreeMap;
use std::fmt;

use mdio_script::{BusError, MdioBus, SpinSleeper};

use crate::op::{ByteSrc, Dest, Dev, Op, OpKind, OpLetter, ParamByte, ResultName, Slot, Value};
use crate::params::Params;

/// The clause-45 MDIO bus master (and the clock) a sequence runs against.
///
/// Not `mdio_script::MdioBus` itself: the sequence engines report the bus master's own error
/// (the C++ messages carry its status code, and the SAI call returns it), and they take their
/// delays from the caller (a renderer prints them). [`OverMdio`] adapts an `MdioBus`.
pub trait Bus {
    /// What a failed access reports.
    type Error;

    /// Reads `dev.reg` of the PHY at address word `phy`.
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, Self::Error>;

    /// Writes `value` to `dev.reg` of the PHY at address word `phy`.
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), Self::Error>;

    /// Waits `us` microseconds: a `D` line, and between two reads of a `P`. Also called with 0
    /// (the C++ executor does, and a renderer shows it).
    fn delay_us(&mut self, us: u32);

    /// Called before operation `index` of the sequence starts: for a bus that wants to know
    /// which operation its next accesses belong to (a renderer, a trace). Default: nothing.
    fn begin(&mut self, index: usize, op: &Op) {
        let _ = (index, op);
    }
}

impl<B: Bus + ?Sized> Bus for &mut B {
    type Error = B::Error;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, Self::Error> {
        (**self).read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), Self::Error> {
        (**self).write(phy, dev, reg, value)
    }
    fn delay_us(&mut self, us: u32) {
        (**self).delay_us(us);
    }
    fn begin(&mut self, index: usize, op: &Op) {
        (**self).begin(index, op);
    }
}

/// A [`Bus`] over an `mdio_script::MdioBus`, with real delays (`mdio_script::SpinSleeper`: short
/// waits are spun, the appliance kernels round a sleep up to a 4 ms tick).
#[derive(Debug)]
pub struct OverMdio<B>(pub B);

impl<B: MdioBus> Bus for OverMdio<B> {
    type Error = BusError;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.0.read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusError> {
        self.0.write(phy, dev, reg, value)
    }
    fn delay_us(&mut self, us: u32) {
        SpinSleeper.sleep_us(u64::from(us));
    }
}

/// The named results of a run: what its `X <result>[<word>]` operations read, 16-bit words,
/// least significant word first. A word no `X` filled below a filled one is 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results(BTreeMap<ResultName, Vec<u16>>);

impl Results {
    /// The words of a result, `None` when the run had no `X` for it.
    pub fn words(&self, name: &str) -> Option<&[u16]> {
        self.0
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, w)| w.as_slice())
    }

    /// The first four words of a result as one number (0 when absent): the C++ `ms_result_u64`.
    pub fn u64(&self, name: &str) -> u64 {
        let words = self.words(name).unwrap_or(&[]);
        words.iter().take(4).rev().fold(0, |v, &w| (v << 16) | u64::from(w))
    }

    /// Every result, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&ResultName, &[u16])> {
        self.0.iter().map(|(n, w)| (n, w.as_slice()))
    }

    /// Whether the run kept no result.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn put(&mut self, name: &ResultName, word: usize, value: u16) {
        let words = self.0.entry(name.clone()).or_default();
        if words.len() <= word {
            words.resize(word + 1, 0);
        }
        if let Some(w) = words.get_mut(word) {
            *w = value;
        }
    }
}

/// What a run that reached its end leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The named results.
    pub results: Results,
    /// Every `C` condition held (`true` without one).
    pub cond: bool,
    /// The value of the last `C` read (0 without one).
    pub last: u16,
}

/// Why a run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecErrorKind<E> {
    /// A parameter byte the sequence names is not there. Found before the first bus access:
    /// nothing was read or written.
    MissingParam(ParamByte),
    /// The bus master failed.
    Bus {
        /// The address word accessed.
        phy: u16,
        /// The device.
        dev: Dev,
        /// The register.
        reg: u16,
        /// What the bus reported.
        error: E,
    },
    /// An `E` read another value, or a `P` never read its value: the chip does not follow the
    /// table (a fault).
    Mismatch {
        /// The address word accessed.
        phy: u16,
        /// The device.
        dev: Dev,
        /// The register.
        reg: u16,
        /// The (last) value read.
        read: u16,
        /// The value expected.
        want: u16,
        /// The bits compared.
        mask: u16,
        /// Reads made.
        reads: u16,
    },
}

/// A run that stopped, with the table line of the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecError<E> {
    /// 1-based table line of the operation.
    pub line: u32,
    /// The operation's letter.
    pub op: OpLetter,
    /// What happened.
    pub kind: ExecErrorKind<E>,
}

impl<E> ExecError<E> {
    /// Whether nothing was accessed (a missing parameter).
    pub const fn before_any_access(&self) -> bool {
        matches!(self.kind, ExecErrorKind::MissingParam(_))
    }
}

impl<E: fmt::Display> ExecError<E> {
    /// The message of the ext-phy engine (`sai_extphy.cpp`), which words a mismatch differently
    /// from the ext-macsec one ([`fmt::Display`]).
    pub fn ext_phy_message(&self) -> String {
        let (line, op) = (self.line, self.op);
        match &self.kind {
            ExecErrorKind::Mismatch {
                phy,
                dev,
                reg,
                read,
                want,
                mask,
                reads,
            } if op == OpLetter::P => format!(
                "line {line}: {op} phy 0x{phy:x} {}.{reg:04x} never read 0x{want:04x} (mask 0x{mask:04x}) in {reads} reads, last 0x{read:04x}",
                dev.get()
            ),
            ExecErrorKind::Mismatch {
                phy,
                dev,
                reg,
                read,
                want,
                mask,
                ..
            } => format!(
                "line {line}: {op} phy 0x{phy:x} {}.{reg:04x} read 0x{read:04x}, expected 0x{want:04x} (mask 0x{mask:04x})",
                dev.get()
            ),
            ExecErrorKind::MissingParam(_) | ExecErrorKind::Bus { .. } => self.to_string(),
        }
    }
}

/// The message of the ext-macsec engine (`sai_extmacsec.cpp`), byte for byte when `E` displays
/// as the bus master's status number. No key byte: values read from the chip and the table's
/// own constants only.
impl<E: fmt::Display> fmt::Display for ExecError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, op) = (self.line, self.op);
        match &self.kind {
            ExecErrorKind::MissingParam(p) => {
                write!(f, "line {line}: parameter {}[{}] is not available", p.name(), p.index())
            }
            ExecErrorKind::Bus { phy, dev, reg, error } => {
                write!(
                    f,
                    "line {line}: {op} phy 0x{phy:x} {}.{reg:04x}: MDIO error {error}",
                    dev.get()
                )
            }
            ExecErrorKind::Mismatch {
                phy,
                dev,
                reg,
                read,
                want,
                mask,
                reads,
            } => write!(
                f,
                "line {line}: {op} phy 0x{phy:x} {}.{reg:04x} read 0x{read:04x}, expected 0x{want:04x} (mask 0x{mask:04x}, {reads} reads)",
                dev.get()
            ),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for ExecError<E> {}

/// The first parameter byte `ops` name that `params` does not have, with its operation.
fn missing_param<'a>(ops: &'a [Op], params: &Params) -> Option<(&'a Op, ParamByte)> {
    ops.iter().find_map(|op| {
        let OpKind::Write {
            value: Value::Bytes { hi, lo },
            ..
        } = &op.kind
        else {
            return None;
        };
        [hi, lo].into_iter().find_map(|b| match *b {
            ByteSrc::Param(p) if params.byte(p.name(), p.index()).is_none() => Some((op, p)),
            ByteSrc::Param(_) | ByteSrc::Const(_) => None,
        })
    })
}

/// The values `X $n` kept in this run; a slot never read is 0 (the C++ `kept[16] = {0}`; a parsed
/// table cannot write one before an `X` read it).
struct Kept([u16; 16]);

impl Kept {
    fn get(&self, slot: Slot) -> u16 {
        self.0.get(usize::from(slot.get())).copied().unwrap_or(0)
    }
    fn set(&mut self, slot: Slot, value: u16) {
        if let Some(k) = self.0.get_mut(usize::from(slot.get())) {
            *k = value;
        }
    }
}

fn byte(src: ByteSrc, params: &Params) -> u16 {
    match src {
        ByteSrc::Const(c) => u16::from(c),
        // checked by missing_param before the first access
        ByteSrc::Param(p) => u16::from(params.byte(p.name(), p.index()).unwrap_or(0)),
    }
}

/// Runs `ops` in order for the port whose PHY has the address word `port_phy`.
///
/// * Every parameter byte the operations name must be in `params`; that is checked before the
///   first bus access ([`ExecErrorKind::MissingParam`]): a sequence is never started with a key
///   that is too short.
/// * A bus error, an `E` that reads another value and a `P` that never reads its value stop the
///   run at that operation; nothing after it is accessed.
/// * `C` conditions never stop it: [`Outcome::cond`] says whether all held.
///
/// This is the executor of both C++ engines (`ext_macsec_exec` and `sai_extphy.cpp`'s
/// `run_seq`): the same accesses in the same order.
pub fn run<B: Bus>(bus: &mut B, ops: &[Op], port_phy: u16, params: &Params) -> Result<Outcome, ExecError<B::Error>> {
    if let Some((op, p)) = missing_param(ops, params) {
        return Err(ExecError {
            line: op.line,
            op: op.letter(),
            kind: ExecErrorKind::MissingParam(p),
        });
    }
    let mut kept = Kept([0; 16]);
    let mut out = Outcome {
        results: Results::default(),
        cond: true,
        last: 0,
    };
    for (index, op) in ops.iter().enumerate() {
        bus.begin(index, op);
        let Some(at) = op.reg() else {
            if let OpKind::Delay(us) = op.kind {
                bus.delay_us(us.get());
            }
            continue;
        };
        let (phy, dev, reg) = (at.phy.resolve(port_phy), at.dev, at.reg);
        let fail = |kind| ExecError {
            line: op.line,
            op: op.letter(),
            kind,
        };
        let bus_error = |error| fail(ExecErrorKind::Bus { phy, dev, reg, error });
        let mismatch = |read, cmp: crate::op::Compare, reads| {
            fail(ExecErrorKind::Mismatch {
                phy,
                dev,
                reg,
                read,
                want: cmp.want(),
                mask: cmp.mask(),
                reads,
            })
        };
        match &op.kind {
            OpKind::Write { value, .. } => {
                let word = match *value {
                    Value::Const(c) => c,
                    Value::Kept { slot, mask, bits } => (kept.get(slot) & mask) | bits,
                    Value::Bytes { hi, lo } => (byte(hi, params) << 8) | byte(lo, params),
                };
                bus.write(phy, dev, reg, word).map_err(bus_error)?;
            }
            OpKind::Read { .. } => {
                bus.read(phy, dev, reg).map_err(bus_error)?;
            }
            OpKind::Keep { dest, .. } => {
                let v = bus.read(phy, dev, reg).map_err(bus_error)?;
                match dest {
                    Dest::Kept(slot) => kept.set(*slot, v),
                    Dest::Result { name, word } => out.results.put(name, usize::from(word.get()), v),
                }
            }
            OpKind::Expect { cmp, .. } => {
                let v = bus.read(phy, dev, reg).map_err(bus_error)?;
                if !cmp.holds(v) {
                    return Err(mismatch(v, *cmp, 1));
                }
            }
            OpKind::Poll {
                cmp, tries, interval, ..
            } => {
                let mut last = 0;
                let mut hit = false;
                for attempt in 0..tries.get() {
                    if attempt > 0 {
                        bus.delay_us(interval.get());
                    }
                    last = bus.read(phy, dev, reg).map_err(bus_error)?;
                    if cmp.holds(last) {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    return Err(mismatch(last, *cmp, tries.get()));
                }
            }
            OpKind::Cond { cmp, .. } => {
                let v = bus.read(phy, dev, reg).map_err(bus_error)?;
                out.last = v;
                out.cond &= cmp.holds(v);
            }
            OpKind::Delay(_) => {}
        }
    }
    Ok(out)
}

/// The parameters a sequence needs and how many bytes of each: the largest index its
/// operations name, plus one. In [`crate::ParamName::ALL`] order.
pub fn required_params(ops: &[Op]) -> Vec<(crate::ParamName, u8)> {
    let mut need = [0u8; 8];
    for op in ops {
        let OpKind::Write {
            value: Value::Bytes { hi, lo },
            ..
        } = &op.kind
        else {
            continue;
        };
        for src in [hi, lo] {
            let ByteSrc::Param(p) = *src else { continue };
            if let Some(n) = need.get_mut(p.name().index()) {
                *n = (*n).max(p.index().saturating_add(1));
            }
        }
    }
    crate::ParamName::ALL
        .into_iter()
        .zip(need)
        .filter(|&(_, n)| n > 0)
        .collect()
}

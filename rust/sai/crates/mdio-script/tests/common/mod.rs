//! Shared by the test binaries: the deterministic fake bus of the differential test (the same
//! rules as `tests/differential_driver.c`), the canonical result text, and script generators.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]
#![allow(dead_code, reason = "each test binary uses a part of this module")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::rc::Rc;

use mdio_script::{BusError, Check, Dev, Failure, Hooks, MdioBus, RunOptions, Script, Stats, VerifyMode};
use proptest::prelude::*;

/// The event trace both the bus and the hooks append to.
pub(crate) type Out = Rc<RefCell<String>>;

/// What the driver is asked to do after its parse pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Parse,
    Run,
    Verify,
    Only,
}

impl Mode {
    pub(crate) const RUNS: [Self; 3] = [Self::Run, Self::Verify, Self::Only];

    pub(crate) fn arg(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Run => "run",
            Self::Verify => "verify",
            Self::Only => "only",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Job {
    pub(crate) mode: Mode,
    pub(crate) seed: u32,
    pub(crate) tick: bool,
    pub(crate) abort_k: u64,
}

pub(crate) fn jobs() -> impl Strategy<Value = (u32, bool, u64)> {
    (0u32..1000, any::<bool>(), prop_oneof![3 => Just(0u64), 2 => 1u64..40])
}

/// The bus of `tests/differential_driver.c`.
pub(crate) struct DiffBus {
    seed: u32,
    ready_after: i64,
    written: HashMap<(u16, u8, u16), u16>,
    out: Out,
}

impl DiffBus {
    pub(crate) fn new(seed: u32, out: Out) -> Self {
        Self {
            seed,
            ready_after: i64::from(seed % 7),
            written: HashMap::new(),
            out,
        }
    }

    fn initial(&self, phy: u16, dev: u8, reg: u16) -> u16 {
        let mut h = self
            .seed
            .wrapping_mul(2_654_435_761)
            .wrapping_add(u32::from(phy).wrapping_mul(40503))
            .wrapping_add(u32::from(dev).wrapping_mul(8191))
            .wrapping_add(u32::from(reg).wrapping_mul(131));
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        u16::try_from(h & 3).unwrap()
    }
}

impl MdioBus for DiffBus {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        let dev = dev.get();
        let mut out = self.out.borrow_mut();
        if reg == 0xdead || phy == 0x26 {
            writeln!(out, "R {phy:x} {dev:x} {reg:x} ERR").unwrap();
            return Err(BusError);
        }
        let val = if reg == 0x8221 {
            let before = self.ready_after;
            self.ready_after -= 1;
            if before > 0 { 0 } else { 0x0303 }
        } else if phy == 0x24 {
            0x1900
        } else {
            self.written
                .get(&(phy, dev, reg))
                .copied()
                .unwrap_or_else(|| self.initial(phy, dev, reg))
        };
        writeln!(out, "R {phy:x} {dev:x} {reg:x} = {val:04x}").unwrap();
        Ok(val)
    }

    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError> {
        let dev = dev.get();
        let mut out = self.out.borrow_mut();
        if reg == 0xbad0 {
            writeln!(out, "W {phy:x} {dev:x} {reg:x} {val:x} ERR").unwrap();
            return Err(BusError);
        }
        self.written.insert((phy, dev, reg), val);
        writeln!(out, "W {phy:x} {dev:x} {reg:x} {val:x}").unwrap();
        Ok(())
    }
}

/// The callbacks of `tests/differential_driver.c`.
pub(crate) struct DiffHooks {
    tick: bool,
    abort_k: u64,
    pub(crate) ticks: u64,
    out: Out,
}

impl DiffHooks {
    pub(crate) fn new(tick: bool, abort_k: u64, out: Out) -> Self {
        Self {
            tick,
            abort_k,
            ticks: 0,
            out,
        }
    }
}

impl Hooks for DiffHooks {
    fn sleep_us(&mut self, us: u64) {
        writeln!(self.out.borrow_mut(), "S {us}").unwrap();
    }
    fn log(&mut self, msg: &str) {
        writeln!(self.out.borrow_mut(), "L {}", esc_text(msg)).unwrap();
    }
    fn wants_tick(&self) -> bool {
        self.tick
    }
    fn tick(&mut self, progress: &Stats) -> ControlFlow<()> {
        self.ticks += 1;
        writeln!(self.out.borrow_mut(), "T {} ops={}", self.ticks, progress.ops).unwrap();
        if self.abort_k != 0 && self.ticks >= self.abort_k {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
    fn verify(&mut self, k: &Check<'_>) {
        writeln!(
            self.out.borrow_mut(),
            "V {} {} {:x} {:x} {:x} {:04x} {:04x} {:04x} ok={} io={} {}",
            k.line,
            k.until.letter().as_char(),
            k.at.phy,
            k.at.dev.get(),
            k.at.reg,
            k.want,
            k.mask,
            k.got,
            u8::from(k.ok),
            u8::from(k.io_error),
            esc(k.label.as_bytes())
        )
        .unwrap();
    }
}

/// The driver's `esc`: printable ASCII but the backslash as it is, the rest as \\xNN.
pub(crate) fn esc(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) && b != b'\\' {
            s.push(char::from(b));
        } else {
            write!(s, "\\x{b:02x}").unwrap();
        }
    }
    s
}

/// `esc` of a message: a char up to U+00FF stands for that byte (the crate shows an operation
/// byte that way), anything else for its UTF-8 bytes.
pub(crate) fn esc_text(text: &str) -> String {
    let mut bytes = Vec::new();
    for c in text.chars() {
        match u8::try_from(u32::from(c)) {
            Ok(b) => bytes.push(b),
            Err(_) => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    esc(&bytes)
}

fn result(out: &mut String, tag: &str, rc: i32, st: &Stats, err: &str) {
    writeln!(
        out,
        "{tag} rc={rc} lines={} writes={} reads={} delays={} polls={} labels={} poll_rereads={} differs={} \
         delay_us={} ops={} verify_line={} verify_checks={} verify_failed={}",
        st.lines,
        st.writes,
        st.reads,
        st.delays,
        st.polls,
        st.labels,
        st.poll_rereads,
        st.differs,
        st.delay_us,
        st.ops,
        st.verify_line,
        st.verify_checks,
        st.verify_failed
    )
    .unwrap();
    writeln!(out, "{tag} err={}", esc_text(err)).unwrap();
}

/// What `tests/differential_driver.c` prints for the same script and job.
pub(crate) fn rust_output(data: &[u8], job: Job) -> String {
    let mut text = String::new();
    let script = match Script::parse_bytes(data) {
        Ok(script) => script,
        Err(e) => {
            result(&mut text, "parse", e.code(), &e.stats, &e.to_string());
            return text;
        }
    };
    result(&mut text, "parse", 0, script.stats(), "");
    let verify = match job.mode {
        Mode::Parse => return text,
        Mode::Run => VerifyMode::Stop,
        Mode::Verify => VerifyMode::Report,
        Mode::Only => VerifyMode::Only,
    };
    let out = Out::default();
    let mut bus = DiffBus::new(job.seed, Rc::clone(&out));
    let mut hooks = DiffHooks::new(job.tick, job.abort_k, Rc::clone(&out));
    let run = script.run(&mut bus, &mut hooks, RunOptions { verify });
    text.push_str(&out.borrow());
    let fail = match run {
        Ok(st) => {
            result(&mut text, "run", 0, &st, "");
            Failure::default()
        }
        Err(e) => {
            result(&mut text, "run", e.kind.code(), &e.stats, &e.to_string());
            e.failure
        }
    };
    writeln!(
        text,
        "fail line={} op={} phy={:x} dev={:x} reg={:x} want={:x} mask={:x} got={:x} tries={} have={} label={}",
        fail.line,
        fail.op.map_or('-', mdio_script::OpLetter::as_char),
        fail.phy,
        fail.dev,
        fail.reg,
        fail.want,
        fail.mask,
        fail.got,
        fail.tries,
        u8::from(fail.have_value),
        esc(fail.label.as_bytes())
    )
    .unwrap();
    text
}

// ---- generators: script text over a small domain, so that the fake bus is hit in every way ----

fn phy() -> impl Strategy<Value = u16> {
    prop_oneof![19 => prop::sample::select(vec![0u16, 1, 0x24, 0x25, 0x17c]), 1 => Just(0x26u16)]
}
fn dev() -> impl Strategy<Value = u8> {
    prop::sample::select(vec![1u8, 3, 0x1e, 0x1f])
}
fn reg() -> impl Strategy<Value = u16> {
    prop_oneof![
        18 => prop::sample::select(vec![1u16, 2, 0x8221, 0x8b00, 0xffff]),
        1 => Just(0xdeadu16),
        1 => Just(0xbad0u16)
    ]
}
fn val() -> impl Strategy<Value = u16> {
    prop_oneof![6 => 0u16..4, 1 => Just(0x303u16), 1 => Just(0x1900u16), 1 => any::<u16>()]
}
fn mask() -> impl Strategy<Value = u16> {
    prop_oneof![3 => Just(0xffffu16), 3 => Just(3u16), 1 => Just(0x303u16), 1 => any::<u16>()]
}
fn interval() -> impl Strategy<Value = u32> {
    prop::sample::select(vec![0u32, 10, 1000, 600_000, 1_200_000])
}

/// A hex field in one of the spellings `strtoul(.., 16)` takes.
fn hex(v: u16, style: u8) -> String {
    match style % 5 {
        0 | 1 => format!("{v:x}"),
        2 => format!("{v:04x}"),
        3 => format!("0x{v:X}"),
        _ => format!("{v:X}"),
    }
}

fn join(fields: &[String], sep: u8) -> String {
    let sep = [" ", " ", "  ", "\t", " \t "][usize::from(sep % 5)];
    let lead = if sep == "  " { " " } else { "" };
    format!("{lead}{}", fields.join(sep))
}

fn label_text() -> impl Strategy<Value = String> {
    let ch = prop_oneof![8 => 0x21u8..0x7f, 2 => Just(b' ')];
    prop_oneof![9 => prop::collection::vec(ch.clone(), 0..40), 1 => prop::collection::vec(ch, 140..170)]
        .prop_map(|b| String::from_utf8(b).unwrap())
}

fn label_line() -> impl Strategy<Value = String> {
    prop_oneof![1 => Just("L".to_owned()), 6 => label_text().prop_map(|t| format!("L {t}"))]
}

fn filler_line() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("  \t".to_owned()),
        Just("# note".to_owned()),
        Just("#".to_owned())
    ]
}

/// `P` / `N` with the given tries.
fn poll_line(tries: impl Strategy<Value = u32>) -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        (phy(), dev(), reg()),
        val(),
        mask(),
        tries,
        interval(),
        any::<u8>(),
        any::<u8>(),
    )
        .prop_map(|(p, (phy, dev, reg), val, mask, tries, interval, style, sep)| {
            let fields = [
                if p { "P" } else { "N" }.to_owned(),
                hex(phy, style),
                hex(dev.into(), style),
                hex(reg, style),
                hex(val, style),
                hex(mask, style),
                if style % 7 == 0 {
                    format!("{tries:03}")
                } else {
                    tries.to_string()
                },
                interval.to_string(),
            ];
            join(&fields, sep)
        })
}

fn body_line() -> impl Strategy<Value = String> {
    let write = ((phy(), dev(), reg()), val(), any::<u8>(), any::<u8>()).prop_map(|((p, d, r), v, st, sep)| {
        join(
            &["W".to_owned(), hex(p, st), hex(d.into(), st), hex(r, st), hex(v, st)],
            sep,
        )
    });
    let read = ((phy(), dev(), reg()), any::<u8>(), any::<u8>())
        .prop_map(|((p, d, r), st, sep)| join(&["R".to_owned(), hex(p, st), hex(d.into(), st), hex(r, st)], sep));
    let expect = (
        (phy(), dev(), reg()),
        val(),
        prop::option::of(mask()),
        any::<u8>(),
        any::<u8>(),
    )
        .prop_map(|((p, d, r), v, m, st, sep)| {
            let mut f = vec!["E".to_owned(), hex(p, st), hex(d.into(), st), hex(r, st), hex(v, st)];
            f.extend(m.map(|m| hex(m, st)));
            join(&f, sep)
        });
    let delay = prop::sample::select(vec![0u32, 30, 19_999, 20_000, 500_000, 500_001, 1_700_000, 60_000_000])
        .prop_map(|us| format!("D {us}"));
    prop_oneof![
        4 => write,
        1 => read,
        2 => expect,
        2 => poll_line(prop::sample::select(vec![2u32, 3, 5, 10])),
        1 => poll_line(Just(1u32)),
        1 => delay,
        1 => label_line(),
        1 => filler_line(),
    ]
}

fn tail_line() -> impl Strategy<Value = String> {
    prop_oneof![5 => poll_line(Just(1u32)), 1 => label_line(), 1 => filler_line()]
}

/// A valid script: a body of every kind of line, then (often) a verification section.
pub(crate) fn valid_script() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(body_line(), 0..20),
        prop::collection::vec(tail_line(), 0..7),
        any::<bool>(),
        prop::bool::weighted(0.9),
    )
        .prop_map(|(body, tail, crlf, last_eol)| {
            let eol = if crlf { "\r\n" } else { "\n" };
            let mut text = format!("mdio-script 1{eol}");
            text.push_str(&body.into_iter().chain(tail).collect::<Vec<_>>().join(eol));
            if last_eol {
                text.push_str(eol);
            }
            text
        })
}

#[derive(Debug, Clone)]
enum Mutation {
    Delete(prop::sample::Index),
    Insert(prop::sample::Index, u8),
    Replace(prop::sample::Index, u8),
    Truncate(prop::sample::Index),
    /// A run of one byte, around the 510-byte line limit.
    Long(prop::sample::Index, u8, usize),
}

fn nasty_byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        6 => prop::sample::select(b"\0\n\r \t\x0b\x0c#LDWREPNX-+x0159afg".to_vec()),
        2 => any::<u8>(),
    ]
}

fn mutation() -> impl Strategy<Value = Mutation> {
    let at = any::<prop::sample::Index>;
    prop_oneof![
        3 => at().prop_map(Mutation::Delete),
        3 => (at(), nasty_byte()).prop_map(|(i, b)| Mutation::Insert(i, b)),
        3 => (at(), nasty_byte()).prop_map(|(i, b)| Mutation::Replace(i, b)),
        1 => at().prop_map(Mutation::Truncate),
        1 => (at(), nasty_byte(), 500usize..530).prop_map(|(i, b, n)| Mutation::Long(i, b, n)),
    ]
}

/// A valid script with a few bytes damaged: usually a syntax error, sometimes still valid.
pub(crate) fn corrupted_script() -> impl Strategy<Value = Vec<u8>> {
    (valid_script(), prop::collection::vec(mutation(), 1..4)).prop_map(|(text, mutations)| {
        let mut data = text.into_bytes();
        for m in mutations {
            match m {
                Mutation::Delete(i) if !data.is_empty() => {
                    data.remove(i.index(data.len()));
                }
                Mutation::Replace(i, b) if !data.is_empty() => {
                    let at = i.index(data.len());
                    data[at] = b;
                }
                Mutation::Delete(_) | Mutation::Replace(..) => {}
                Mutation::Insert(i, b) => data.insert(i.index(data.len() + 1), b),
                Mutation::Truncate(i) => data.truncate(i.index(data.len() + 1)),
                Mutation::Long(i, b, n) => {
                    let at = i.index(data.len() + 1);
                    data.splice(at..at, std::iter::repeat_n(b, n));
                }
            }
        }
        data
    })
}

/// Bytes with no structure but the alphabet of a script.
pub(crate) fn noise() -> impl Strategy<Value = Vec<u8>> {
    (any::<bool>(), prop::collection::vec(nasty_byte(), 0..200)).prop_map(|(header, mut data)| {
        if header {
            data.splice(0..0, *b"mdio-script 1\n");
        }
        data
    })
}

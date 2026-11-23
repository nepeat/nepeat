//! The interpreter tests of `tests/test_mdio_script.c` (`main()` and `test_verification()`), ported.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::ControlFlow;

use mdio_script::{
    BusError, Check, Dev, Failure, Hooks, Label, LoadError, MdioBus, OpLetter, ParseErrorKind, RunErrorKind,
    RunOptions, Script, Stats, VerifyMode,
};

/// Fake bus: a log of accesses, a small register file, one register that becomes ready after N
/// reads, one that fails.
#[derive(Default)]
struct FBus {
    trace: String,
    regs: HashMap<u16, u16>,
    ready_after: i32,
    io_calls: u32,
}

impl MdioBus for FBus {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.io_calls += 1;
        if reg == 0xdead {
            return Err(BusError);
        }
        let val = if reg == 0x8221 {
            self.ready_after -= 1;
            if self.ready_after >= 0 { 0 } else { 0x0303 }
        } else {
            self.regs.get(&reg).copied().unwrap_or(0)
        };
        write!(self.trace, "R{phy:x}.{:x}.{reg:x} ", dev.get()).unwrap();
        Ok(val)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError> {
        self.io_calls += 1;
        self.regs.insert(reg, val);
        write!(self.trace, "W{phy:x}.{:x}.{reg:x}={val:x} ", dev.get()).unwrap();
        Ok(())
    }
}

/// Counts sleep, keeps the log; no tick.
#[derive(Default)]
struct FHooks {
    slept: u64,
    log: String,
}

impl Hooks for FHooks {
    fn sleep_us(&mut self, us: u64) {
        self.slept += us;
    }
    fn log(&mut self, msg: &str) {
        self.log += msg;
        self.log.push('\n');
    }
}

struct Outcome {
    rc: i32,
    st: Stats,
    err: String,
    fail: Failure,
}

fn outcome(script: &Script, bus: &mut impl MdioBus, hooks: &mut impl Hooks, verify: VerifyMode) -> Outcome {
    match script.run(bus, hooks, RunOptions { verify }) {
        Ok(st) => Outcome {
            rc: 0,
            st,
            err: String::new(),
            fail: Failure::default(),
        },
        Err(e) => Outcome {
            rc: e.kind.code(),
            st: e.stats,
            err: e.to_string(),
            fail: e.failure,
        },
    }
}

/// C `run(text, 1, ..)`: fresh trace, log and counters; the register file stays.
fn run(text: &str, bus: &mut FBus, hooks: &mut FHooks) -> Outcome {
    bus.trace.clear();
    bus.io_calls = 0;
    *hooks = FHooks::default();
    outcome(&Script::parse(text).unwrap(), bus, hooks, VerifyMode::Stop)
}

fn syntax(text: &str) -> mdio_script::ParseError {
    let e = Script::parse(text).unwrap_err();
    assert_eq!(e.code(), mdio_script::E_SYNTAX);
    e
}

#[test]
fn order_values_delay_labels() {
    let (mut bus, mut hooks) = (FBus::default(), FHooks::default());
    bus.regs.insert(0x8b00, 0x2391);
    let r = run(
        "mdio-script 1\n# c\nL step one\nW 17c 1 8201 7e\nD 30\nR 17c 1 900a\nE 17c 1 8b00 2391\n\
         E 17c 1 8b00 0391 0fff\nE 17c 1 8b00 1111\n\nW 0 1f ffff ffff\n",
        &mut bus,
        &mut hooks,
    );
    assert_eq!(r.rc, 0);
    assert_eq!(
        bus.trace,
        "W17c.1.8201=7e R17c.1.900a R17c.1.8b00 R17c.1.8b00 R17c.1.8b00 W0.1f.ffff=ffff "
    );
    let st = r.st;
    assert_eq!(
        (st.writes, st.reads, st.delays, st.delay_us, st.labels, st.differs),
        (2, 4, 1, 30, 1, 1)
    );
    assert_eq!(hooks.slept, 30);
    assert!(hooks.log.contains("step one"));
    assert!(
        hooks
            .log
            .contains("differs: line 9: 17c 1.8b00 = 2391, script 1111 mask ffff [step one]")
    );
}

#[test]
fn polls() {
    let (mut bus, mut hooks) = (FBus::default(), FHooks::default());
    // P: re-read until the value, with the interval; N: until it changes
    bus.ready_after = 3;
    let r = run("mdio-script 1\nP 24 1 8221 0303 ffff 10 1000\n", &mut bus, &mut hooks);
    assert_eq!(r.rc, 0);
    assert_eq!(
        (r.st.polls, r.st.poll_rereads, r.st.reads, hooks.slept),
        (1, 3, 4, 3000)
    );
    bus.ready_after = 2;
    let r = run("mdio-script 1\nN 24 1 8221 0 ffff 10 500\n", &mut bus, &mut hooks);
    assert_eq!(r.rc, 0);
    assert_eq!((r.st.poll_rereads, hooks.slept), (2, 1000));
    // masked poll, first read
    bus.regs.insert(0x8251, 0x002d);
    let r = run("mdio-script 1\nP 24 1 8251 d d 1 0\n", &mut bus, &mut hooks);
    assert_eq!((r.rc, r.st.poll_rereads), (0, 0));

    // a poll that never matches fails the script, names line, label and value; nothing after it runs
    bus.ready_after = 100;
    let r = run(
        "mdio-script 1\nL download bus 1\nP 24 1 8221 0303 ffff 5 10\nW 24 1 1 1\n",
        &mut bus,
        &mut hooks,
    );
    assert_eq!(r.rc, mdio_script::E_POLL);
    assert_eq!(
        r.err,
        "line 3: phy 24 1.8221 reads 0000, script wants 0303 (mask ffff) after 5 reads [download bus 1]"
    );
    assert!(!bus.trace.contains("W24.1.1=1"));
    assert_eq!(r.st.writes, 0);
    // assertion form (tries 1)
    bus.regs.insert(0x8215, 1);
    let r = run(
        "mdio-script 1\nL verify\nP 24 1 8215 d00b ffff 1 0\n",
        &mut bus,
        &mut hooks,
    );
    assert_eq!(r.rc, mdio_script::E_POLL);
    assert!(r.err.contains("reads 0001") && r.err.contains("d00b"));
    // I/O error
    let r = run("mdio-script 1\nR 24 1 dead\nW 24 1 1 1\n", &mut bus, &mut hooks);
    assert_eq!((r.rc, r.st.writes), (mdio_script::E_IO, 0));
    assert_eq!(r.err, "line 2: read 24 1.dead failed [(start)]");
    assert_eq!(
        (r.fail.line, r.fail.op, r.fail.have_value),
        (2, Some(OpLetter::R), false)
    );
}

#[test]
fn parse_only_no_access_same_statistics() {
    let script = Script::parse("mdio-script 1\nW 1 1 2 3\nP 1 1 2 3 ffff 4 5\nD 100\n").unwrap();
    let st = script.stats();
    assert_eq!((st.writes, st.polls, st.delay_us, st.ops, st.lines), (1, 1, 100, 3, 2));
    assert_eq!(script.lines().iter().map(|l| l.line).collect::<Vec<_>>(), [2, 3, 4]);
}

#[test]
fn refused_files() {
    let e = Script::load("/nonexistent/x.mdio").unwrap_err();
    assert!(matches!(e, LoadError::File(_)));
    assert_eq!(e.code(), mdio_script::E_FILE);

    // SAI_RS_TEST_TMPDIR: where a copied test executable runs, the build tree does not exist.
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        std::path::PathBuf::from,
    );
    let path = dir.join(format!("mdio-script-interp-{}.mdio", std::process::id()));
    std::fs::write(&path, "").unwrap();
    let e = Script::load(&path).unwrap_err();
    assert!(matches!(&e, LoadError::Syntax(p) if p.kind == ParseErrorKind::Empty));
    assert_eq!(
        (e.code(), e.to_string().as_str()),
        (mdio_script::E_SYNTAX, "empty file")
    );
    std::fs::write(&path, "mdio-script 1\r\nW 1 1 2 3\r\n").unwrap();
    assert_eq!(Script::load(&path).unwrap().stats().writes, 1);
    std::fs::remove_file(&path).unwrap();

    assert_eq!(syntax("").kind, ParseErrorKind::Empty);
    assert_eq!(syntax("W 1 1 2 3\n").kind, ParseErrorKind::NotAScript); // no header
    assert_eq!(syntax("mdio-script 2\n").kind, ParseErrorKind::NotAScript);
    assert_eq!(
        syntax("mdio-script 1\nW 1 0 2 3\n").kind,
        ParseErrorKind::BadArguments(b'W')
    ); // clause 22
    assert_eq!(
        syntax("mdio-script 1\nW 1 20 2 3\n").kind,
        ParseErrorKind::BadArguments(b'W')
    ); // dev > 31
    assert_eq!(
        syntax("mdio-script 1\nW 1 1 2\n").kind,
        ParseErrorKind::BadArguments(b'W')
    ); // truncated line
    assert_eq!(
        syntax("mdio-script 1\nW 1 1 2 10000\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    assert_eq!(
        syntax("mdio-script 1\nW 1 1 2 3 4\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    assert_eq!(
        syntax("mdio-script 1\nX 1 1 2 3\n").kind,
        ParseErrorKind::UnknownOperation(b'X')
    );
    assert_eq!(
        syntax("mdio-script 1\nP 1 1 2 3 ffff 0 5\n").kind,
        ParseErrorKind::BadArguments(b'P')
    ); // 0 tries
    assert_eq!(syntax("mdio-script 1\nD -5\n").kind, ParseErrorKind::BadDelay);
    let e = syntax("mdio-script 1\nW 1 1 2 3\nW 1 1 zz 3\n");
    assert_eq!(
        (e.line, e.to_string().as_str()),
        (3, "line 3: bad arguments for 'W' (clause 45 only: dev 1..1f)")
    );
    // C: a syntax error found while running stops there, after one write. Here the file never
    // becomes a script, so nothing runs; the parse pass still reports the write before it.
    let e = syntax("mdio-script 1\nW 1 1 2 3\nbogus\nW 1 1 4 5\n");
    assert_eq!(
        (e.line, e.kind, e.stats.writes),
        (3, ParseErrorKind::UnknownOperationWord, 1)
    );
}

#[test]
fn syntax_details() {
    let ok = |text: &str| Script::parse(text).unwrap();
    // limits
    ok("mdio-script 1\nW ffff 1f ffff ffff\nP 0 1 0 0 0 1000000 60000000\nD 60000000\nD 0\n");
    assert_eq!(
        syntax("mdio-script 1\nP 0 1 0 0 0 1000001 0\n").kind,
        ParseErrorKind::BadArguments(b'P')
    );
    assert_eq!(
        syntax("mdio-script 1\nP 0 1 0 0 0 1 60000001\n").kind,
        ParseErrorKind::BadArguments(b'P')
    );
    assert_eq!(syntax("mdio-script 1\nD 60000001\n").kind, ParseErrorKind::BadDelay);
    assert_eq!(syntax("mdio-script 1\nD\n").kind, ParseErrorKind::BadDelay);
    assert_eq!(syntax("mdio-script 1\nD 1 2\n").kind, ParseErrorKind::BadDelay);
    assert_eq!(syntax("mdio-script 1\nD 1f\n").kind, ParseErrorKind::BadDelay); // decimal
    assert_eq!(
        syntax("mdio-script 1\nP 0 1 0 0 0 a 0\n").kind,
        ParseErrorKind::BadArguments(b'P')
    );
    assert_eq!(
        syntax("mdio-script 1\nW +1 1 2 3\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    assert_eq!(
        syntax("mdio-script 1\nW 1 1 2 -3\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    assert_eq!(
        syntax("mdio-script 1\nW 1 1 2 99999999999999999999999\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    // strtoul spellings: 0x prefix, upper case, leading zeros
    ok("mdio-script 1\nW 0x17C 0X1F 0000000000000000000000ffff 0\n");
    assert_eq!(
        syntax("mdio-script 1\nW 0x 1 2 3\n").kind,
        ParseErrorKind::BadArguments(b'W')
    );
    assert_eq!(syntax("mdio-script 1\nD 0x10\n").kind, ParseErrorKind::BadDelay);
    // E takes 4 or 5 arguments
    ok("mdio-script 1\nE 1 1 2 3\nE 1 1 2 3 4\n");
    assert_eq!(
        syntax("mdio-script 1\nE 1 1 2\n").kind,
        ParseErrorKind::BadArguments(b'E')
    );
    assert_eq!(
        syntax("mdio-script 1\nE 1 1 2 3 4 5\n").kind,
        ParseErrorKind::BadArguments(b'E')
    );
    assert_eq!(
        syntax("mdio-script 1\nR 1 1 2 3\n").kind,
        ParseErrorKind::BadArguments(b'R')
    );
    assert_eq!(
        syntax("mdio-script 1\nN 1 1 2 3 4 5 6 7\n").kind,
        ParseErrorKind::BadArguments(b'N')
    );
}

#[test]
fn labels_lines_and_serialisation() {
    let ok = |text: &str| Script::parse(text).unwrap();
    // labels: "L text", bare "L"; anything else starting with L is an operation
    let s = ok("mdio-script 1\nL\nL \nL  two \nW 1 1 2 3");
    let labels: Vec<_> = s
        .ops()
        .filter_map(|op| {
            if let mdio_script::Op::Label(l) = op {
                Some(l)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        labels,
        [&Label::default(), &Label::default(), &Label::new(" two ").unwrap()]
    );
    assert_eq!(
        syntax("mdio-script 1\nL\tx\n").kind,
        ParseErrorKind::UnknownOperation(b'L')
    );
    assert_eq!(
        syntax("mdio-script 1\nLabel x\n").kind,
        ParseErrorKind::UnknownOperationWord
    );
    assert_eq!(
        syntax("mdio-script 1\n #\n").kind,
        ParseErrorKind::UnknownOperation(b'#')
    );
    let long = "x".repeat(200);
    let s = ok(&format!("mdio-script 1\nL {long}\n"));
    assert_eq!(
        s.ops().next(),
        Some(&mdio_script::Op::Label(Label::new("x".repeat(150)).unwrap()))
    );
    assert!(Label::new("x".repeat(151)).is_none() && Label::new("a\nb").is_none() && Label::new("a\r").is_none());
    // line length: 510 bytes fit, 511 do not, with or without a line end
    let fill = |n: usize| format!("mdio-script 1\n#{}", "c".repeat(n - 1));
    ok(&fill(510));
    ok(&(fill(510) + "\n"));
    ok(&(fill(509) + "\r\n"));
    let e = syntax(&(fill(511) + "\n"));
    assert_eq!(
        (e.line, e.kind, e.to_string().as_str()),
        (2, ParseErrorKind::LineTooLong, "line 2: line too long")
    );
    assert_eq!(syntax(&fill(511)).kind, ParseErrorKind::LineTooLong);
    // line ends, blank lines, leading blanks, header strictness
    ok("mdio-script 1\r\n\r\n  \t \r\n  W 1 1 2 3  \r\n");
    ok("mdio-script 1");
    assert_eq!(syntax(" mdio-script 1\n").kind, ParseErrorKind::NotAScript);
    assert_eq!(syntax("mdio-script 1 \n").kind, ParseErrorKind::NotAScript);
    assert_eq!(syntax("\nmdio-script 1\n").kind, ParseErrorKind::NotAScript);
    // serialisation parses back to the same operations
    let s = ok("mdio-script 1\n# c\nL a b\nL\nW 17c 1 8201 7e\nD 30\nR 1 2 3\nE 1 2 3 4\nN 1 2 3 4 5 6 7\n");
    assert_eq!(
        s.to_string(),
        "mdio-script 1\nL a b\nL\nW 17c 1 8201 7e\nD 30\nR 1 2 3\nE 1 2 3 4 ffff\nN 1 2 3 4 5 6 7\n"
    );
    assert!(s.ops().eq(s.to_string().parse::<Script>().unwrap().ops()));
}

// ---- a bus of several PHYs for the verification tests ----
// phy 0x0 and 0x1 are healthy; 0x24 is "hung": one constant on every register; 0x25 answers but
// is not initialised (1.8215 = 0001); 0x26 makes the bus master fail. Writes are counted.
// Register 1.8221 of phy 0x1 never becomes ready when stuck is set.
#[derive(Default)]
struct BBus {
    writes: u32,
    reads: u32,
    stuck: bool,
}

impl MdioBus for BBus {
    fn read(&mut self, phy: u16, _dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.reads += 1;
        if phy == 0x26 {
            return Err(BusError);
        }
        Ok(match reg {
            _ if phy == 0x24 => 0x1900,
            0x8b00 => 0x2391,
            0x8b01 => 0x80a0,
            0x8215 if phy == 0x25 => 0x0001,
            0x8215 => 0xd00b,
            0x8251 => 0x002d,
            0x8221 if !self.stuck => 0x0303,
            _ => 0,
        })
    }
    fn write(&mut self, _phy: u16, _dev: Dev, _reg: u16, _val: u16) -> Result<(), BusError> {
        self.writes += 1;
        Ok(())
    }
}

#[derive(Default)]
struct BHooks {
    ticks: u32,
    abort_at_tick: u32,
    slept: u64,
    /// `(phy, ok, io_error, label)` of every verification check.
    checks: Vec<(u16, bool, bool, String)>,
}

impl BHooks {
    fn phys(&self) -> Vec<u16> {
        let mut phys: Vec<u16> = self.checks.iter().map(|c| c.0).collect();
        phys.dedup();
        phys
    }
}

impl Hooks for BHooks {
    fn sleep_us(&mut self, us: u64) {
        self.slept += us;
    }
    fn wants_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, _progress: &Stats) -> ControlFlow<()> {
        self.ticks += 1;
        if self.abort_at_tick != 0 && self.ticks >= self.abort_at_tick {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
    fn verify(&mut self, c: &Check<'_>) {
        self.checks.push((c.at.phy, c.ok, c.io_error, c.label.to_string()));
    }
}

fn verify(phy: &str) -> String {
    format!(
        "L verify phy 0x{phy}\nP {phy} 1 8b00 2391 ffff 1 0\nP {phy} 1 8b01 80a0 ffff 1 0\n\
         P {phy} 1 8215 d00b ffff 1 0\nP {phy} 1 8251 000d 000d 1 0\n"
    )
}
const BODY: &str = "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 5 10\nW 1 1 1 1\n";

#[test]
fn verification_section_of_the_parse_pass() {
    // the verification section is the assertions after the last other operation
    let parsed = *Script::parse(&format!("{BODY}{}# note\n{}", verify("0"), verify("1")))
        .unwrap()
        .stats();
    assert_eq!((parsed.verify_line, parsed.verify_checks, parsed.ops), (8, 8, 13));
    // a single-try assertion in the middle is not part of it; no assertions at the end: none
    let parsed = *Script::parse("mdio-script 1\nP 0 1 8b00 2391 ffff 1 0\nW 0 1 1 1\nL x\n")
        .unwrap()
        .stats();
    assert_eq!((parsed.verify_line, parsed.verify_checks), (0, 0));
    let parsed = *Script::parse("mdio-script 1\nW 0 1 1 1\nP 0 1 8b00 2391 ffff 2 0\nP 0 1 8b00 2391 ffff 1 0\n")
        .unwrap()
        .stats();
    assert_eq!(parsed.verify_line, 4);
}

#[test]
fn verification_all_good() {
    // every check is reported, the script succeeds
    let script = Script::parse(&format!("{BODY}{}{}", verify("0"), verify("1"))).unwrap();
    let (mut bus, mut hooks) = (BBus::default(), BHooks::default());
    let r = outcome(&script, &mut bus, &mut hooks, VerifyMode::Report);
    assert_eq!(r.rc, 0);
    assert_eq!(
        (r.st.verify_checks, r.st.verify_failed, r.st.ops, bus.writes),
        (8, 0, script.stats().ops, 3)
    );
    assert_eq!(r.st.verify_line, script.stats().verify_line);
    assert_eq!(hooks.ticks, 13); // once per operation
    assert_eq!(hooks.phys(), [0, 1]);
    assert!(hooks.checks.iter().all(|c| c.1 && !c.2));
    assert_eq!(hooks.checks.iter().filter(|c| c.0 == 1).count(), 4);
    assert_eq!(r.fail.line, 0);
}

#[test]
fn verification_mismatches_and_verify_only() {
    // mismatches in the section do not stop it: all four PHYs are read, then it fails
    let script = Script::parse(&format!(
        "{BODY}{}{}{}{}",
        verify("0"),
        verify("24"),
        verify("25"),
        verify("26")
    ))
    .unwrap();
    let (mut bus, mut hooks) = (BBus::default(), BHooks::default());
    let r = outcome(&script, &mut bus, &mut hooks, VerifyMode::Report);
    assert_eq!(r.rc, mdio_script::E_VERIFY);
    assert_eq!(
        (r.st.verify_checks, r.st.verify_failed, r.st.ops),
        (16, 4 + 1 + 4, script.stats().ops)
    );
    assert_eq!(
        r.err,
        "9 of 16 verification reads are not what the script expects; first: \
         line 14: phy 24 1.8b00 reads 1900, script wants 2391 (mask ffff) [verify phy 0x24]"
    );
    let f = &r.fail;
    assert_eq!(
        (f.line, f.op, f.phy, f.reg, f.want, f.got),
        (14, Some(OpLetter::P), 0x24, 0x8b00, 0x2391, 0x1900)
    );
    assert!(f.have_value && f.label == Label::new("verify phy 0x24").unwrap());
    assert_eq!(hooks.phys(), [0, 0x24, 0x25, 0x26]);
    let bad = |phy| hooks.checks.iter().filter(|c| c.0 == phy && !c.1).count();
    assert_eq!((bad(0), bad(0x24), bad(0x25), bad(0x26)), (0, 4, 1, 4));
    assert!(hooks.checks.iter().all(|c| c.2 == (c.0 == 0x26)));
    assert_eq!(hooks.checks[4].3, "verify phy 0x24");
    // without the verify line (the old behaviour) the first mismatch stops the script
    let r = outcome(&script, &mut bus, &mut BHooks::default(), VerifyMode::Stop);
    assert_eq!((r.rc, r.fail.line, r.fail.tries), (mdio_script::E_POLL, 14, 1));

    // verify_only: nothing before the section runs, no write, only its reads
    let (mut bus, mut hooks) = (BBus::default(), BHooks::default());
    let r = outcome(&script, &mut bus, &mut hooks, VerifyMode::Only);
    assert_eq!(r.rc, mdio_script::E_VERIFY);
    assert_eq!((bus.writes, bus.reads, hooks.slept, r.st.writes), (0, 16, 0, 3));
    assert_eq!(hooks.phys().len(), 4);
    // ... which needs a section
    let none = Script::parse(BODY).unwrap();
    let e = none
        .run(
            &mut bus,
            &mut hooks,
            RunOptions {
                verify: VerifyMode::Only,
            },
        )
        .unwrap_err();
    assert_eq!(
        (e.kind, e.kind.code()),
        (RunErrorKind::NoVerifySection, mdio_script::E_SYNTAX)
    );
    assert!(e.to_string().contains("no verification section"));
    assert_eq!((bus.writes, bus.reads, e.stats), (0, 16, Stats::default()));
}

#[test]
fn poll_failure_record_and_tick() {
    // a poll that never matches: the record says where, what and how often
    let script = Script::parse(&format!("{BODY}{}{}", verify("0"), verify("1"))).unwrap();
    let (mut bus, mut hooks) = (
        BBus {
            stuck: true,
            ..BBus::default()
        },
        BHooks::default(),
    );
    let r = outcome(&script, &mut bus, &mut hooks, VerifyMode::Report);
    assert_eq!(r.rc, mdio_script::E_POLL);
    let f = &r.fail;
    assert_eq!(
        (f.line, f.op, f.phy, f.dev, f.reg),
        (6, Some(OpLetter::P), 1, 1, 0x8221)
    );
    assert_eq!(
        (f.want, f.mask, f.got, f.tries, f.have_value),
        (0x0303, 0xffff, 0, 5, true)
    );
    assert_eq!(f.label, Label::new("init").unwrap());
    assert_eq!((r.st.ops, bus.writes), (3, 2)); // W W D done; the poll and what follows are not
    assert_eq!(r.st.verify_line, 0); // set only when the script reaches its end

    // tick: a stop request ends the script between two operations; long delays are cut into pieces
    let (mut bus, mut hooks) = (
        BBus::default(),
        BHooks {
            abort_at_tick: 2,
            ..BHooks::default()
        },
    );
    let r = outcome(&script, &mut bus, &mut hooks, VerifyMode::Report);
    assert_eq!((r.rc, bus.writes, r.st.ops), (mdio_script::E_ABORT, 1, 1));
    assert_eq!(r.err, "line 4: stopped on request before this line [init]");
    assert_eq!((r.fail.line, r.fail.op, r.fail.mask), (4, None, 0xffff));

    let delay = Script::parse("mdio-script 1\nD 1700000\n").unwrap();
    let mut hooks = BHooks::default();
    let r = outcome(&delay, &mut bus, &mut hooks, VerifyMode::Stop);
    assert_eq!((r.rc, hooks.slept, hooks.ticks), (0, 1_700_000, 1 + 3));
    // without a tick the delay is one sleep
    let mut plain = FHooks::default();
    assert_eq!(outcome(&delay, &mut bus, &mut plain, VerifyMode::Stop).rc, 0);
    assert_eq!(plain.slept, 1_700_000);

    let delay = Script::parse("mdio-script 1\nD 1700000\nW 0 1 1 1\n").unwrap();
    let mut hooks = BHooks {
        abort_at_tick: 3,
        ..BHooks::default()
    };
    let r = outcome(&delay, &mut bus, &mut hooks, VerifyMode::Stop);
    assert_eq!((r.rc, r.st.ops), (mdio_script::E_ABORT, 0));
    assert!(r.err.contains("during a delay"));
    assert_eq!(r.fail, Failure::default());
}

#[test]
fn only_the_first_twenty_mismatches_are_logged() {
    let text = format!("mdio-script 1\n{}", "E 1 1 2 5\n".repeat(25));
    let (mut bus, mut hooks) = (FBus::default(), FHooks::default());
    let r = run(&text, &mut bus, &mut hooks);
    assert_eq!((r.rc, r.st.differs, hooks.log.lines().count()), (0, 25, 20));
}

#[test]
fn default_sleeper_waits() {
    let start = std::time::Instant::now();
    mdio_script::SpinSleeper.sleep_us(2000);
    mdio_script::SpinSleeper.sleep_us(21_000);
    mdio_script::SpinSleeper.sleep_us(0);
    assert!(start.elapsed() >= std::time::Duration::from_millis(23));
    // the default hooks: real time, silent, no tick
    let script = Script::parse("mdio-script 1\nL x\nD 100\nW 1 1 2 3\n").unwrap();
    let st = script
        .run(&mut BBus::default(), &mut mdio_script::NoHooks, RunOptions::default())
        .unwrap();
    assert_eq!((st.ops, st.delay_us), (2, 100));
}

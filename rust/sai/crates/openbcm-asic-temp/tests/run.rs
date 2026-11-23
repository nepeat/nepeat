//! The reader in this process: every exit path, the messages and the files, on `sdk-glue`'s Rust
//! fake chip (through the crate's own glue adapter) or on a scripted sensor, with a clock that
//! does not sleep.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::format_push_string,
    missing_docs,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use openbcm_asic_temp::glue::GlueAttacher;
use openbcm_asic_temp::{AttachError, Attacher, Clock, Env, Reading, Sensor};
use sdk_glue::rust_fake::Chip;

const NOW: i64 = 1_700_000_000;
const ME: &str = "openbcm-asic-temp";

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("asic-temp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A clock that records instead of sleeping; a stop "arrives" during the n-th wait.
struct FakeClock {
    stop_at_wait: Option<usize>,
    stopped: bool,
    settled: u32,
    armed: u32,
    waits: Vec<u32>,
    watch: Vec<PathBuf>,
    /// the watched files' contents at each wait
    seen: Vec<Vec<Option<String>>>,
}

impl FakeClock {
    fn new(stop_at_wait: Option<usize>, watch: &[&Path]) -> Self {
        Self {
            stop_at_wait,
            stopped: false,
            settled: 0,
            armed: 0,
            waits: Vec::new(),
            watch: watch.iter().map(|p| p.to_path_buf()).collect(),
            seen: Vec::new(),
        }
    }
}

impl Clock for FakeClock {
    fn settle(&mut self) {
        self.settled += 1;
    }
    fn arm(&mut self, _err: &mut dyn Write) {
        self.armed += 1;
    }
    fn stopped(&self) -> bool {
        self.stopped
    }
    fn wait(&mut self, seconds: u32) {
        self.waits.push(seconds);
        self.seen
            .push(self.watch.iter().map(|p| std::fs::read_to_string(p).ok()).collect());
        if Some(self.waits.len()) == self.stop_at_wait {
            self.stopped = true;
        }
    }
    fn now(&self) -> i64 {
        NOW
    }
}

struct Outcome {
    code: u8,
    out: String,
    err: String,
}

fn go(args: &[&dyn AsRef<OsStr>], attacher: &mut dyn Attacher, clock: &mut FakeClock) -> Outcome {
    let args: Vec<OsString> = args.iter().map(|a| a.as_ref().to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = openbcm_asic_temp::run(
        OsStr::new(ME),
        &args,
        &mut Env {
            out: &mut out,
            err: &mut err,
            attacher,
            clock,
        },
    );
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn chip(scenario: &str) -> GlueAttacher {
    let c = Chip::new();
    c.load(scenario).unwrap();
    GlueAttacher::on(c)
}

fn lines(err: &str) -> Vec<&str> {
    err.lines().collect()
}

fn prom(unit: i32, cur: &[(&str, &str)], max: &str) -> String {
    let mut s = String::from(
        "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n\
         # TYPE switch_asic_temperature_celsius gauge\n",
    );
    for (i, (c, _)) in cur.iter().enumerate() {
        s += &format!("switch_asic_temperature_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {c}\n");
    }
    s += "# HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n\
          # TYPE switch_asic_temperature_peak_celsius gauge\n";
    for (i, (_, p)) in cur.iter().enumerate() {
        s += &format!("switch_asic_temperature_peak_celsius{{unit=\"{unit}\",sensor=\"pvtmon{i}\"}} {p}\n");
    }
    s += &format!(
        "# HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n\
         # TYPE switch_asic_temperature_max_celsius gauge\n\
         switch_asic_temperature_max_celsius{{unit=\"{unit}\"}} {max}\n\
         # HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n\
         # TYPE switch_asic_temperature_read_timestamp_seconds gauge\n\
         switch_asic_temperature_read_timestamp_seconds{{unit=\"{unit}\"}} {NOW}\n"
    );
    s
}

#[test]
fn a_run_publishes_until_a_signal_and_removes_its_files() {
    let d = scratch("run");
    let (out, pr) = (d.join("asic/temperature"), d.join("textfile/asic.prom"));
    let mut a = chip("temp 413 418 -55");
    let mut clock = FakeClock::new(Some(2), &[&out, &pr]);
    let r = go(
        &[&"--interval", &"7", &"--out", &out, &"--prom", &pr],
        &mut a,
        &mut clock,
    );
    assert_eq!((r.code, r.out.as_str()), (0, ""), "{}", r.err);
    assert_eq!(
        lines(&r.err),
        [
            format!("{ME}: unit 0 attached FAKE56960_A0"),
            format!(
                "{ME}: 3 monitors, max 41.8 C, average 25.9 C; every 7 s to {}; core PLL power-down at 125 C",
                out.display()
            ),
            format!("{ME}: stopped"),
        ]
    );
    assert_eq!(
        (clock.settled, clock.armed, clock.waits.as_slice()),
        (1, 1, &[7, 7][..])
    );
    let golden = prom(0, &[("41.3", "42.8"), ("41.8", "43.3"), ("-5.5", "-4.0")], "41.8");
    for seen in &clock.seen {
        assert_eq!(seen[0].as_deref(), Some("41.8\n"));
        assert_eq!(seen[1].as_deref(), Some(golden.as_str()));
    }
    // removed at the stop; the missing parent directories were made (one level each)
    assert!(!out.exists() && !pr.exists());
    assert!(d.join("asic").is_dir() && d.join("textfile").is_dir());
    assert!(!d.join("asic/temperature.tmp").exists() && !d.join("textfile/asic.prom.tmp").exists());
}

#[test]
fn once_prints_the_monitors() {
    let mut a = chip("temp 413 418 -55 0");
    let mut clock = FakeClock::new(None, &[]);
    let r = go(&[&"--once"], &mut a, &mut clock);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "monitor 0: current 41.3 C, peak 42.8 C\nmonitor 1: current 41.8 C, peak 43.3 C\n\
         monitor 2: current -5.5 C, peak -4.0 C\nmonitor 3: current 0.0 C, peak 1.5 C\n\
         max 41.8 C, average 19.4 C\n"
    );
    assert_eq!(lines(&r.err), [format!("{ME}: unit 0 attached FAKE56960_A0")]);
    assert_eq!((clock.waits.len(), clock.settled), (0, 1));
}

#[test]
fn once_with_a_failed_read_exits_4_and_removes_the_files() {
    let d = scratch("once-fail");
    let out = d.join("temperature");
    std::fs::write(&out, "99.9\n").unwrap();
    let mut a = chip("");
    let mut clock = FakeClock::new(None, &[]);
    let r = go(&[&"--once", &"-o", &out], &mut a, &mut clock);
    assert_eq!(r.code, 4);
    assert_eq!(
        lines(&r.err)[1],
        format!("{ME}: read failed: Feature unavailable (0 monitors)")
    );
    assert!(!out.exists());
    // --once with a good read leaves FILE alone
    std::fs::write(&out, "99.9\n").unwrap();
    let r = go(
        &[&"--once", &"-o", &out],
        &mut chip("temp 400"),
        &mut FakeClock::new(None, &[]),
    );
    assert_eq!(r.code, 0);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "99.9\n");
}

#[test]
fn three_failed_reads_in_a_row_exit_4() {
    let d = scratch("fails");
    let (out, pr) = (d.join("temperature"), d.join("asic.prom"));
    // stale files of an earlier run go at the first failed read
    std::fs::write(&out, "50.0\n").unwrap();
    std::fs::write(&pr, "old\n").unwrap();
    // read 1 is the discarded one; 2, 3, 4 fail
    let mut a = chip("temp 400\ntemp-fail 2");
    let mut clock = FakeClock::new(None, &[&out, &pr]);
    let r = go(&[&"-o", &out, &"-p", &pr], &mut a, &mut clock);
    assert_eq!(r.code, 4);
    let failed = format!("{ME}: read failed: Operation timed out (0 monitors)");
    assert_eq!(&lines(&r.err)[1..], [failed.as_str(); 3]);
    assert_eq!(clock.waits, [5, 5]);
    assert!(clock.seen.iter().all(|s| s == &[None, None]));
    assert!(!out.exists() && !pr.exists());

    // one good read first: written, then removed by the first failure
    let mut a = chip("temp 400\ntemp-fail 3");
    let mut clock = FakeClock::new(None, &[&out]);
    let r = go(&[&"-o", &out, &"-i", &"1"], &mut a, &mut clock);
    assert_eq!(r.code, 4);
    assert_eq!(clock.waits, [1, 1, 1]);
    assert_eq!(clock.seen[0][0].as_deref(), Some("40.0\n"));
    assert_eq!(clock.seen[1][0], None);
    assert_eq!(lines(&r.err).len(), 5, "{}", r.err);
}

#[test]
fn implausible_values_are_failed_reads() {
    for (scenario, n) in [("temp 1501 400", 2), ("temp -401", 1)] {
        let mut a = chip(scenario);
        let r = go(&[&"--once"], &mut a, &mut FakeClock::new(None, &[]));
        assert_eq!(r.code, 4, "{scenario}");
        assert_eq!(
            lines(&r.err)[1],
            format!("{ME}: read failed: ok ({n} monitors, implausible value)")
        );
    }
    // the limits themselves are plausible
    let r = go(
        &[&"--once"],
        &mut chip("temp 1500 -400"),
        &mut FakeClock::new(None, &[]),
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out.lines().last(), Some("max 150.0 C, average 55.0 C"));
}

#[test]
fn over_temperature_twice_powers_the_core_pll_down() {
    let d = scratch("hot");
    let (out, pr) = (d.join("temperature"), d.join("asic.prom"));
    let c = Chip::new();
    c.load("temp 400 1250").unwrap();
    let mut a = GlueAttacher::on(c.clone());
    let mut clock = FakeClock::new(None, &[&out, &pr]);
    let r = go(&[&"-o", &out, &"-p", &pr], &mut a, &mut clock);
    assert_eq!(r.code, 5, "{}", r.err);
    assert_eq!(
        &lines(&r.err)[1..],
        [
            format!(
                "{ME}: monitor 1 at 125.0 C, at or above the 125 C limit (core PLL power-down on the next such read)"
            ),
            format!(
                "{ME}: 2 monitors, max 125.0 C, average 82.5 C; every 5 s to {}; core PLL power-down at 125 C",
                out.display()
            ),
            format!(
                "{ME}: CRITICAL: monitor 1 at 125.0 C (limit 125 C, 2 reads in a row): powering down the switch core PLL \
                 as the SDK's over-temperature handler does; the switch ASIC needs a power cycle"
            ),
        ]
    );
    // the first hot read is still published; the files go before the power-down
    assert_eq!(clock.seen[0][0].as_deref(), Some("125.0\n"));
    assert!(!out.exists() && !pr.exists());
    assert!(c.pll_down());

    // the power-down fails: said, and still exit 5
    let c = Chip::new();
    c.load("temp 1300\nfail overtemp 1 -16").unwrap();
    let r = go(
        &[&"-o", &out],
        &mut GlueAttacher::on(c.clone()),
        &mut FakeClock::new(None, &[]),
    );
    assert_eq!(r.code, 5);
    assert_eq!(
        lines(&r.err).last().copied(),
        Some(format!("{ME}: CRITICAL: core PLL power-down failed: Feature unavailable").as_str())
    );
    assert!(!c.pll_down());
}

#[test]
fn the_limit_is_configurable_and_can_be_off() {
    let d = scratch("limit");
    let out = d.join("temperature");
    // off: 149.0 C for ever
    let c = Chip::new();
    c.load("temp 1490").unwrap();
    let r = go(
        &[&"-o", &out, &"--shutdown-at", &"0"],
        &mut GlueAttacher::on(c.clone()),
        &mut FakeClock::new(Some(4), &[]),
    );
    assert_eq!(r.code, 0);
    assert!(
        lines(&r.err)[1].ends_with("; core PLL power-down at 0 C (off)"),
        "{}",
        r.err
    );
    assert!(!c.pll_down());
    // at 100 C: 99.9 is below, 100.0 is at
    let r = go(
        &[&"-o", &out, &"-s", &"100"],
        &mut chip("temp 999"),
        &mut FakeClock::new(Some(3), &[]),
    );
    assert_eq!(r.code, 0, "{}", r.err);
    let r = go(
        &[&"-o", &out, &"-s", &"100"],
        &mut chip("temp 1000"),
        &mut FakeClock::new(Some(3), &[]),
    );
    assert_eq!(r.code, 5, "{}", r.err);
}

/// A sensor that answers from a list (then the last answer again); records power-downs.
struct Scripted {
    reads: VecDeque<Result<Vec<i32>, String>>,
    last: Result<Vec<i32>, String>,
    log: Rc<RefCell<Vec<String>>>,
}

impl Sensor for Scripted {
    fn read(&mut self) -> Result<Reading, String> {
        if let Some(next) = self.reads.pop_front() {
            self.last = next;
        }
        self.log.borrow_mut().push("read".into());
        self.last.clone().map(|current| Reading {
            peak: current.iter().map(|t| t + 1).collect(),
            current,
        })
    }
    fn shutdown(&mut self) -> Result<(), String> {
        self.log.borrow_mut().push("shutdown".into());
        Ok(())
    }
}

struct Script(Option<Scripted>);

impl Attacher for Script {
    fn attach(&mut self, unit: i32) -> Result<(Box<dyn Sensor>, String), AttachError> {
        assert_eq!(unit, 0);
        Ok((Box::new(self.0.take().unwrap()), "scripted".into()))
    }
}

fn script(reads: &[Result<&[i32], &str>]) -> (Script, Rc<RefCell<Vec<String>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    // the first answer is the discarded read
    let reads: VecDeque<_> = std::iter::once(Ok(vec![0]))
        .chain(reads.iter().map(|r| r.map(<[i32]>::to_vec).map_err(str::to_owned)))
        .collect();
    (
        Script(Some(Scripted {
            reads,
            last: Ok(vec![0]),
            log: Rc::clone(&log),
        })),
        log,
    )
}

#[test]
fn counters_as_the_c_keeps_them() {
    let d = scratch("counters");
    let out = d.join("temperature");
    // a failed read between two hot ones does not reset the hot count (C hot_reads)
    let (mut a, log) = script(&[Ok(&[1300]), Err("Operation timed out"), Ok(&[1300])]);
    let r = go(&[&"-o", &out], &mut a, &mut FakeClock::new(None, &[]));
    assert_eq!(r.code, 5, "{}", r.err);
    assert_eq!(log.borrow().last().map(String::as_str), Some("shutdown"));
    // a cool read does
    let (mut a, log) = script(&[Ok(&[1300]), Ok(&[400]), Ok(&[1300]), Ok(&[400])]);
    let r = go(&[&"-o", &out], &mut a, &mut FakeClock::new(Some(4), &[]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!log.borrow().contains(&"shutdown".to_owned()));
    // a good read resets the failure count
    let (mut a, log) = script(&[Err("x"), Err("x"), Ok(&[400]), Err("x"), Err("x"), Err("x")]);
    let r = go(&[&"-o", &out], &mut a, &mut FakeClock::new(None, &[]));
    assert_eq!(r.code, 4);
    assert_eq!(log.borrow().len(), 7, "the discarded read + 6");
    // a stop that arrives during a read: no more sleep, files removed, exit 0
    let (mut a, _) = script(&[Ok(&[400])]);
    let mut clock = FakeClock::new(Some(1), &[&out]);
    let r = go(&[&"-o", &out], &mut a, &mut clock);
    assert_eq!((r.code, clock.waits.len()), (0, 1));
    assert!(!out.exists());
}

#[test]
fn attach_failures() {
    for (scenario, args, code, text) in [
        (
            "init busy",
            &["--once"][..],
            3,
            "another SDK process owns the switch (lock /run/openbcm-unit0.lock held; e.g. systemctl stop openbcm-asic-temp) (Operation still running)",
        ),
        (
            "init unit",
            &["--once"],
            2,
            "no attached switch unit (soc_ndev 1) (Invalid unit)",
        ),
        (
            "temp 400",
            &["--once", "--unit", "1"],
            2,
            "no attached switch unit (soc_ndev 1) (Invalid unit)",
        ),
        (
            "init attach",
            &["--once"],
            2,
            "fake glue: unknown init failure 'attach' (Invalid configuration)",
        ),
    ] {
        let mut clock = FakeClock::new(None, &[]);
        let args: Vec<&dyn AsRef<OsStr>> = args.iter().map(|a| a as &dyn AsRef<OsStr>).collect();
        let r = go(&args, &mut chip(scenario), &mut clock);
        assert_eq!(r.code, code, "{scenario}");
        assert_eq!(r.err, format!("{ME}: {text}\n"), "{scenario}");
        assert_eq!((clock.settled, clock.armed), (0, 0));
    }
}

struct NoAttach;

impl Attacher for NoAttach {
    fn attach(&mut self, _unit: i32) -> Result<(Box<dyn Sensor>, String), AttachError> {
        panic!("attached although the command line is refused");
    }
}

#[test]
fn a_refused_command_line_attaches_nothing() {
    for (args, code) in [
        (&[][..], 1),
        (&["--help"], 0),
        (&["--out"], 1),
        (&["--once", "-i", "0"], 1),
    ] {
        let args: Vec<&dyn AsRef<OsStr>> = args.iter().map(|a| a as &dyn AsRef<OsStr>).collect();
        let r = go(&args, &mut NoAttach, &mut FakeClock::new(None, &[]));
        assert_eq!(r.code, code);
        assert!(r.err.contains(&format!("usage: {ME} [--unit N]")), "{}", r.err);
    }
}

#[test]
fn files_that_cannot_be_written_are_said_each_read() {
    let d = scratch("unwritable");
    std::fs::write(d.join("plain"), "").unwrap();
    // two missing levels (only one is made), and a parent that is a file
    let (out, pr) = (d.join("a/b/temperature"), d.join("plain/asic.prom"));
    let mut clock = FakeClock::new(Some(2), &[]);
    let r = go(&[&"-o", &out, &"-p", &pr], &mut chip("temp 400"), &mut clock);
    assert_eq!(r.code, 0);
    let l = lines(&r.err);
    // both bits set: each line names its file, with the error of the last failure (C errno)
    let (o, p) = (out.display(), pr.display());
    assert_eq!(l[1], format!("{ME}: cannot write {o}: Not a directory"));
    assert_eq!(l[2], format!("{ME}: cannot write {p}: Not a directory"));
    assert!(l[3].starts_with(&format!("{ME}: 1 monitors, max 40.0 C")), "{}", r.err);
    assert_eq!(l[4], l[1]);
    assert_eq!(l[5], l[2]);
    assert_eq!(l.len(), 7);
    // only the plain file failing: its own error
    let mut clock = FakeClock::new(Some(1), &[]);
    let r = go(
        &[&"-o", &out, &"-p", &d.join("ok.prom")],
        &mut chip("temp 400"),
        &mut clock,
    );
    assert_eq!(
        lines(&r.err)[1],
        format!("{ME}: cannot write {o}: No such file or directory")
    );
}

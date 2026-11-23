//! The cases of sai-openbcm's `tests/test_mdio_script.c` that cover `mdio_report.c`
//! (`test_verification`'s report and classification parts, `test_report_and_status`, the lock
//! holder and the diag socket of `test_system`), on the same fake bus.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use mdio_report::{
    DiagBus, FailClass, Mode, Names, PhyState, Report, ScriptStatus, State, Status, TempStatus, classify_run,
    lock_holder,
};
use mdio_script::{
    BusError, Check, Dev, E_POLL, E_VERIFY, Hooks, MdioBus, RunError, RunOptions, Script, Stats, VerifyMode,
};

/// The C test's bus of several PHYs: 0x0 and 0x1 are healthy; 0x24 is "hung" (one constant on
/// every register); 0x25 answers but is not initialised (1.8215 = 0001); 0x26 makes the bus
/// master fail. Register 1.8221 never becomes ready when `stuck`; `hang`: PHY 0x1 hangs too.
#[derive(Default)]
struct Bus {
    stuck: bool,
    hang: bool,
    writes: u32,
    reads: u32,
}

fn bus_value(phy: u16, reg: u16, stuck: bool, hang: bool) -> Result<u16, BusError> {
    Ok(match (phy, reg) {
        (0x26, _) => return Err(BusError),
        (0x24, _) => 0x1900,
        (1, _) if hang => 0x1900,
        (_, 0x8b00) => 0x2391,
        (_, 0x8b01) => 0x80a0,
        (0x25, 0x8215) => 0x0001,
        (_, 0x8215) => 0xd00b,
        (_, 0x8251) => 0x002d,
        (_, 0x8221) => {
            if stuck {
                0
            } else {
                0x0303
            }
        }
        _ => 0,
    })
}

impl MdioBus for Bus {
    fn read(&mut self, phy: u16, _dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.reads += 1;
        bus_value(phy, reg, self.stuck, self.hang)
    }
    fn write(&mut self, _phy: u16, _dev: Dev, _reg: u16, _val: u16) -> Result<(), BusError> {
        self.writes += 1;
        Ok(())
    }
}

struct Collect<'a>(&'a mut Report);

impl Hooks for Collect<'_> {
    fn sleep_us(&mut self, _us: u64) {}
    fn verify(&mut self, check: &Check<'_>) {
        self.0.add(check);
    }
}

fn verify(phy: &str) -> String {
    format!(
        "L verify phy 0x{phy}\nP {phy} 1 8b00 2391 ffff 1 0\nP {phy} 1 8b01 80a0 ffff 1 0\n\
         P {phy} 1 8215 d00b ffff 1 0\nP {phy} 1 8251 000d 000d 1 0\n"
    )
}

const BODY: &str = "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 5 10\nW 1 1 1 1\n";

fn script(phys: &[&str]) -> Script {
    let section: String = phys.iter().map(|p| verify(p)).collect();
    Script::parse(&format!("{BODY}{section}")).unwrap()
}

fn run(script: &Script, bus: &mut Bus, mode: VerifyMode) -> (Result<Stats, RunError>, Report) {
    let mut report = Report::new();
    let r = script.run(bus, &mut Collect(&mut report), RunOptions { verify: mode });
    report.classify();
    (r, report)
}

fn names() -> Names {
    Names::parse(b"phy 0 cage1\nphy 24 cage5\nphy 25 cage6\nreg 1.8b00 id\nreg 1.8215 fw\n").unwrap()
}

fn has(text: &[u8], needle: &str) {
    let text = String::from_utf8_lossy(text);
    assert!(text.contains(needle), "missing: {needle}\nin: {text}");
}

fn scratch(name: &str) -> PathBuf {
    // Short on purpose: unix socket paths are limited to 107 bytes.
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("mdio-report-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn report_states_and_classes() {
    // All good: every check is reported.
    let (r, rep) = run(&script(&["0", "1"]), &mut Bus::default(), VerifyMode::Report);
    assert!(r.is_ok());
    assert_eq!(
        (rep.phys().len(), rep.count(PhyState::Ok), rep.phys()[1].checks().len()),
        (2, 2, 4)
    );

    // Mismatches do not stop the section: all four PHYs are read and classified.
    let s = script(&["0", "24", "25", "26"]);
    let (r, rep) = run(&s, &mut Bus::default(), VerifyMode::Report);
    let e = r.unwrap_err();
    assert_eq!(e.kind.code(), E_VERIFY);
    let states: Vec<PhyState> = rep.phys().iter().map(|p| p.state).collect();
    assert_eq!(
        states,
        [PhyState::Ok, PhyState::Constant, PhyState::Mismatch, PhyState::Io]
    );
    assert_eq!(rep.phys()[1].constant, 0x1900);
    assert_eq!(
        classify_run(E_VERIFY, Some(e.failure.phy), Some(&rep)),
        Some(FailClass::PhyConstant)
    );
    assert_eq!(rep.find(0x25).unwrap().phy, 0x25);
    assert!(rep.find(0x27).is_none());

    // verify-only: no write, only the section's reads.
    let mut bus = Bus::default();
    let (r, rep) = run(&s, &mut bus, VerifyMode::Only);
    assert_eq!(r.unwrap_err().kind.code(), E_VERIFY);
    assert_eq!((bus.writes, bus.reads, rep.phys().len()), (0, 16, 4));

    // A poll that never matches, then the post-mortem: the PHY still answers -> it left the
    // sequence; it reads a constant -> not answering.
    let s = script(&["0", "1"]);
    let mut bus = Bus {
        stuck: true,
        ..Bus::default()
    };
    let e = run(&s, &mut bus, VerifyMode::Report).0.unwrap_err();
    assert_eq!((e.kind.code(), e.failure.line, e.failure.phy), (E_POLL, 6, 1));
    let (_, rep) = run(&s, &mut bus, VerifyMode::Only);
    assert_eq!(classify_run(E_POLL, Some(1), Some(&rep)), Some(FailClass::Diverged));
    bus.hang = true;
    let (_, rep) = run(&s, &mut bus, VerifyMode::Only);
    assert_eq!(classify_run(E_POLL, Some(1), Some(&rep)), Some(FailClass::PhyConstant));
    // the PHY the script stopped at is not in the list: any constant PHY decides
    assert_eq!(
        classify_run(E_POLL, Some(0x99), Some(&rep)),
        Some(FailClass::PhyConstant)
    );
    assert_eq!(classify_run(E_POLL, None, Some(&rep)), Some(FailClass::PhyConstant));
    // a PHY with I/O errors and no constant one
    let (_, rep) = run(&script(&["0", "26"]), &mut Bus::default(), VerifyMode::Only);
    assert_eq!(classify_run(E_VERIFY, None, Some(&rep)), Some(FailClass::MdioIo));
    let (_, rep) = run(&script(&["0", "25"]), &mut Bus::default(), VerifyMode::Only);
    assert_eq!(classify_run(E_VERIFY, None, Some(&rep)), Some(FailClass::Verify));
}

#[test]
fn table_sizes_never_lose_a_failure() {
    // 13 checks on one PHY: the 13th (a mismatch) is dropped from the table but not forgotten.
    let mut text = String::from("mdio-script 1\nP 0 1 8b00 2391 ffff 1 0\n");
    for reg in 0..11 {
        writeln!(text, "P 0 1 {reg:x} 0 ffff 1 0").unwrap();
    }
    text.push_str("P 0 1 8b01 1 ffff 1 0\n");
    let s = Script::parse(&text).unwrap();
    let (_, rep) = run(&s, &mut Bus::default(), VerifyMode::Only);
    assert_eq!(
        (rep.phys()[0].checks().len(), rep.dropped(), rep.phys()[0].state),
        (12, 1, PhyState::Mismatch)
    );
    let mut out = Vec::new();
    rep.table(&mut out, None);
    has(&out, "(1 verification reads beyond the table size are not shown)\n");
    // 257 PHYs: the last one's checks are dropped
    let mut text = String::from("mdio-script 1\n");
    for phy in 0x100..0x201 {
        writeln!(text, "P {phy:x} 1 8b00 2391 ffff 1 0").unwrap();
    }
    let (_, rep) = run(&Script::parse(&text).unwrap(), &mut Bus::default(), VerifyMode::Only);
    assert_eq!((rep.phys().len(), rep.dropped()), (256, 1));
    let mut rep = rep;
    rep.reset();
    assert_eq!((rep.phys().len(), rep.dropped()), (0, 0));
}

/// A run whose verification finds a hung PHY and an uninitialised one.
fn failed_status() -> Status {
    let s = script(&["0", "24", "25"]);
    let (r, rep) = run(&s, &mut Bus::default(), VerifyMode::Report);
    let e = r.unwrap_err();
    let mut st = Status::new(Mode::Run, 1_795_000_000, 4242);
    st.state = State::Failed;
    st.finished = 1_795_000_096;
    st.seconds = 95.8;
    st.names = Some(names());
    let path = std::ffi::OsStr::new("/var/lib/phy/x \"y\".mdio");
    let mut one = ScriptStatus::new(path, *s.stats());
    one.started = true;
    one.result = e.kind.code();
    one.run = e.stats;
    st.scripts.push(one);
    st.set_sdk(b"BCM56960_B1: init soc + init misc done");
    st.fail_script = Some(b"/var/lib/phy/x \"y\".mdio".to_vec());
    st.fail_class = classify_run(e.kind.code(), Some(e.failure.phy), Some(&rep));
    st.fail = e.failure;
    st.report = Some(rep);
    st.report_when = Some("the end of the script");
    st.temp = Some(TempStatus {
        reads: 19,
        failed: 0,
        last_c: 41.3,
        max_ms: 0.0,
    });
    st.summarize();
    st
}

#[test]
fn status_of_a_failed_verification() {
    let st = failed_status();
    assert_eq!(
        String::from_utf8_lossy(st.summary()),
        "x \"y\".mdio line 14 [verify phy 0x24]: verification: 2 of 3 PHYs differ: \
         cage5 (phy 0x24) reads 0x1900 on every verified register; \
         cage6 (phy 0x25) fw (1.8215) read 0x0001, expected 0xd00b; 17 of 17 operations had run; \
         cage5 (phy 0x24) reads 0x1900 on every verified register: not answering or hung"
    );
    has(st.hint(), "power-cycle");
    let dir = scratch("status");
    let json = dir.join("sub/status.json"); // the directory is created, one level
    st.write(&json).unwrap();
    let text = std::fs::read(dir.join("sub/status.txt")).unwrap();
    assert!(text.starts_with(b"openbcm-mdio-script: FAILED (phy-not-answering): x \"y\".mdio line 14"));
    for needle in [
        "  what to do: a PHY that reads one constant",
        "17 of 17 operations",
        "  run started 2026-11-18 11:06:40 +0000, took 95.8 s\n",
        "  switch chip: BCM56960_B1: init soc + init misc done\n",
        "ASIC temperature published meanwhile: 19 readings, last 41.3 C, 0 failed, longest read 0.00 ms\n",
        "  PHY registers read at the end of the script: 1 of 3 PHYs as expected\n",
        "name   phy    id      1.8b01  fw      1.8251  state\n",
        "cage1  0x000  2391    80a0    d00b    002d    ok\n",
        "cage5  0x024  1900!   1900!   1900!   1900!   CONSTANT 0x1900: not answering or hung\n",
        "cage6  0x025  2391    80a0    0001!   002d    MISMATCH\n",
    ] {
        has(&text, needle);
    }
    let text = std::fs::read(&json).unwrap();
    for needle in [
        "{\n \"version\": 1,\n \"state\": \"failed\",\n \"mode\": \"run\",\n \"summary\": \"x \\\"y\\\".mdio line 14",
        "\"pid\": 4242,\n \"started\": 1795000000,\n \"finished\": 1795000096,\n \"seconds\": 95.8,\n \"operations_done\": 17,\n \"operations_total\": 17,",
        "\"class\": \"phy-not-answering\"",
        "\"path\": \"/var/lib/phy/x \\\"y\\\".mdio\", \"result\": \"verify\"",
        "\"line\": 14, \"label\": \"verify phy 0x24\", \"op\": \"P\",\n  \"phy\": \"0x24\", \"phy_name\": \"cage5\", \"dev\": 1, \"reg\": \"0x8b00\", \
         \"expected\": \"0x2391\", \"mask\": \"0xffff\", \"read\": \"0x1900\", \"reads\": 1}",
        "\"phys_ok\": 1, \"phys_total\": 3",
        "{\"phy\": \"0x25\", \"name\": \"cage6\", \"state\": \"mismatch\"",
        "{\"reg\": \"1.8215\", \"name\": \"fw\", \"read\": \"0x0001\", \"expected\": \"0xd00b\", \"mask\": \"0xffff\", \"ok\": false}",
        "\"asic_temperature\": {\"readings\": 19, \"failed\": 0, \"last_celsius\": 41.3, \"longest_read_ms\": 0.00}",
        "\"when\": \"the end of the script\"",
    ] {
        has(&text, needle);
    }
    assert!(text.ends_with(b"\n  ]}\n}\n"));
    // replaced atomically: no temporary file is left, mode 0644
    let left: Vec<_> = std::fs::read_dir(dir.join("sub"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert_eq!(std::fs::metadata(&json).unwrap().permissions().mode() & 0o777, 0o644);
    // two levels of missing directories are not created
    assert!(st.write(&dir.join("a/b/status.json")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn status_of_a_poll_that_timed_out_on_a_phy_that_still_answers() {
    let s = script(&["0", "1"]);
    let mut bus = Bus {
        stuck: true,
        ..Bus::default()
    };
    let e = run(&s, &mut bus, VerifyMode::Report).0.unwrap_err();
    let (_, rep) = run(&s, &mut bus, VerifyMode::Only);
    let mut st = Status::new(Mode::Run, 0, 1);
    st.state = State::Failed;
    let mut one = ScriptStatus::new(std::ffi::OsStr::new("/p/init.mdio"), *s.stats());
    one.started = true;
    one.result = e.kind.code();
    one.run = e.stats;
    st.scripts.push(one);
    st.fail_script = Some(b"/p/init.mdio".to_vec());
    st.fail_class = classify_run(e.kind.code(), Some(e.failure.phy), Some(&rep));
    st.fail = e.failure;
    st.report = Some(rep);
    st.report_when = Some("after the failure (reads only)");
    st.summarize();
    assert_eq!(
        String::from_utf8_lossy(st.summary()),
        "init.mdio line 6 [init]: phy 0x1 1.8221 read 0x0000, the script expects 0x0303 (mask 0xffff) \
         after 5 reads; 3 of 13 operations had run; the PHY still answers (its registers are not one \
         constant): it left the recorded sequence"
    );
    let (mut text, mut json) = (Vec::new(), Vec::new());
    st.text(&mut text);
    st.json(&mut json);
    has(
        &text,
        "openbcm-mdio-script: FAILED (sequence-diverged): init.mdio line 6",
    );
    has(
        &text,
        "PHY registers read at after the failure (reads only): 2 of 2 PHYs as expected",
    );
    has(&text, "  run, pid 1\n"); // no start time, not finished
    has(&json, "\"reads\": 5");
    has(&json, "\"when\": \"after the failure (reads only)\"");
    has(&json, "\"finished\": null");
    has(&json, "\"asic_temperature\": null");
}

#[test]
fn status_of_success_running_and_nothing_ran() {
    let (_, rep) = run(&script(&["0", "1"]), &mut Bus::default(), VerifyMode::Only);
    let mut st = Status::new(Mode::Run, 0, 1);
    st.state = State::Ok;
    st.seconds = 95.8;
    let mut one = ScriptStatus::new(std::ffi::OsStr::new("/p/init.mdio"), Stats::default());
    one.started = true;
    one.parsed.ops = 963_356;
    one.run.ops = 963_356;
    st.scripts.push(one);
    st.report = Some(rep.clone());
    st.summarize();
    assert_eq!(st.summary(), b"2 PHYs verified after 963356 operations in 95.8 s");
    assert_eq!(st.hint(), b"");
    let (mut text, mut json) = (Vec::new(), Vec::new());
    st.text(&mut text);
    st.json(&mut json);
    assert!(text.starts_with(b"openbcm-mdio-script: ok: 2 PHYs verified"));
    has(&json, "\"failure\": null");
    has(&json, "\"state\": \"ok\"");
    // no verification section; verify mode; running
    st.report = None;
    st.set_summary(b"");
    st.summarize();
    assert_eq!(
        st.summary(),
        b"963356 operations in 95.8 s; the script has no verification section"
    );
    let mut v = Status::new(Mode::Verify, 0, 1);
    v.state = State::Ok;
    v.report = Some(rep);
    v.summarize();
    assert_eq!(v.summary(), b"2 PHYs verified, all as the script expects");
    let mut text = Vec::new();
    v.text(&mut text);
    has(&text, "  verification only, pid 1\n");
    let mut r = Status::new(Mode::Run, 0, 1);
    r.scripts.push(ScriptStatus::new(
        std::ffi::OsStr::new("a"),
        Stats {
            ops: 7,
            ..Stats::default()
        },
    ));
    r.scripts.push(ScriptStatus::new(
        std::ffi::OsStr::new("b"),
        Stats {
            ops: 5,
            ..Stats::default()
        },
    ));
    r.scripts[0].started = true;
    r.scripts[0].run.ops = 3;
    r.summarize();
    assert_eq!(r.summary(), b"3 of 12 operations done");
    let (mut text, mut json) = (Vec::new(), Vec::new());
    r.text(&mut text);
    r.json(&mut json);
    assert!(text.starts_with(b"openbcm-mdio-script: running: 3 of 12 operations done\n"));
    has(&text, "  b: not run (5 operations)\n");
    has(
        &json,
        "{\"path\": \"b\", \"result\": null, \"seconds\": 0.0, \"verify_line\": 0,",
    );
    has(&json, "\"run\": null}");

    // A failure before anything ran: the caller's summary stays, the class brings its hint.
    let mut st = Status::new(Mode::Run, 0, 1);
    st.state = State::Failed;
    st.fail_class = Some(FailClass::SdkBusy);
    st.set_summary(b"the SDK lock /run/openbcm-unit0.lock is held by pid 812 (syncd)");
    st.summarize();
    has(st.summary(), "held by pid 812 (syncd)");
    has(st.hint(), "stop the process");
    // a board's hint is kept
    let mut st = Status::new(Mode::Run, 0, 1);
    st.state = State::Interrupted;
    st.fail_class = Some(FailClass::Interrupted);
    st.set_hint(b"board text");
    st.summarize();
    assert_eq!(st.hint(), b"board text");
    assert_eq!(st.summary(), b"stopped on request (signal); 0 of 0 operations had run");
    // buffers are cut as C's are
    st.set_summary(&[b'x'; 2000]);
    st.set_hint(&[b'y'; 2000]);
    st.set_sdk(&[b'z'; 2000]);
    assert_eq!((st.summary().len(), st.hint().len(), st.sdk().len()), (699, 399, 319));
}

#[test]
fn summary_names_the_failing_operation() {
    // an MDIO write error, and a label with bytes that are not UTF-8 kept as they are
    struct FailWrite;
    impl MdioBus for FailWrite {
        fn read(&mut self, _: u16, _: Dev, _: u16) -> Result<u16, BusError> {
            Ok(0)
        }
        fn write(&mut self, _: u16, _: Dev, _: u16, _: u16) -> Result<(), BusError> {
            Err(BusError)
        }
    }
    let s = Script::parse_bytes(b"mdio-script 1\nL caf\xe9 \"q\"\nW 24 1 8201 7e\n").unwrap();
    let e = s
        .run(&mut FailWrite, &mut mdio_script::NoHooks, RunOptions::default())
        .unwrap_err();
    let mut st = Status::new(Mode::Run, 0, 1);
    st.state = State::Failed;
    st.names = Some(names());
    st.fail_script = Some(b"dir/s.mdio".to_vec());
    st.fail_class = classify_run(e.kind.code(), Some(e.failure.phy), None);
    st.fail = e.failure;
    st.summarize();
    assert_eq!(
        st.summary(),
        b"s.mdio line 3 [caf\xe9 \"q\"]: MDIO write of cage5 (phy 0x24) 1.8201 failed in the switch chip's MDIO master; \
          0 of 0 operations had run"
    );
    let mut json = Vec::new();
    st.json(&mut json);
    let at = json.windows(8).position(|w| w == b"\"label\":").unwrap();
    assert!(json[at..].starts_with(b"\"label\": \"caf\xe9 \\\"q\\\"\", \"op\": \"W\",\n  \"phy\": \"0x24\""));
    has(&json, "\"read\": null, \"reads\": 0}");
}

#[test]
fn who_holds_a_lock() {
    let dir = scratch("lock");
    let lock = dir.join("unit0.lock");
    std::fs::write(&lock, b"").unwrap();
    assert_eq!(lock_holder(&lock), None);
    if !std::path::Path::new("/proc/locks").exists() {
        eprintln!("who_holds_a_lock: no /proc/locks here, the holder is not tested");
        return;
    }
    // flock(1) holds the lock while its child sleeps
    let child = std::process::Command::new("flock")
        .arg(&lock)
        .args(["sleep", "30"])
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("who_holds_a_lock: no flock(1) here, the holder is not tested");
        return;
    };
    let mut holder = None;
    for _ in 0..200 {
        holder = lock_holder(&lock);
        if holder.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let holder = holder.expect("the flock holder in /proc/locks");
    // flock(1) itself or the sleep it started: both have the descriptor
    assert!(holder.comm == b"flock" || holder.comm == b"sleep", "{holder:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A fake of syncd's diag socket: answers "phy raw c45 <phy> <dev> <reg>" from the bus above.
fn diag_server(listener: &UnixListener, connections: usize) {
    for _ in 0..connections {
        let (mut stream, _) = listener.accept().unwrap();
        let mut cmd = String::new();
        stream.read_to_string(&mut cmd).unwrap();
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let num = |i: usize| {
            words
                .get(i)
                .and_then(|w| w.strip_prefix("0x"))
                .and_then(|w| u16::from_str_radix(w, 16).ok())
        };
        let answer = match (words.get(..3), num(3), num(4), num(5)) {
            (Some(["phy", "raw", "c45"]), Some(phy), Some(_), Some(reg)) => match bus_value(phy, reg, false, false) {
                Ok(v) => format!("\t0x{reg:04x}: 0x{v:04x}\n"),
                Err(BusError) => "openbcm-diag: command failed (-1)\n".to_owned(),
            },
            _ => format!("openbcm-diag: refused (would end or re-init the SDK, or chains commands): {cmd}"),
        };
        stream.write_all(answer.as_bytes()).unwrap();
    }
}

#[test]
fn reads_through_a_diag_socket() {
    let dir = scratch("diag");
    let path = dir.join("diag.sock");
    let d1 = Dev::new(1).unwrap();
    let e = DiagBus::new(&path).c45_read(0, d1, 0x8b00).unwrap_err();
    has(&e, "is syncd running");
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| diag_server(&listener, 4));
        let mut bus = DiagBus::new(&path);
        assert_eq!(bus.c45_read(0x17f, d1, 0x8b00), Ok(0x2391));
        assert_eq!(bus.c45_read(0x25, d1, 0x8215), Ok(0x0001));
        has(
            &bus.c45_read(0x26, d1, 0x8215).unwrap_err(),
            "openbcm-diag: command failed",
        );
        // as a bus: the error is kept for the caller
        assert_eq!(bus.read(0x26, d1, 0x8215), Err(BusError));
        assert_eq!(bus.last_error(), Some(&b"openbcm-diag: command failed (-1)"[..]));
        assert_eq!(bus.socket(), path);
    });
    std::fs::remove_dir_all(dir).unwrap();
}

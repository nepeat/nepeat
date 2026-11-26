//! sdk-glue's REAL path offline: the feature `real` (`Glue::Real` -> `real.rs` -> the C ABI of
//! `sdk_glue.h`) linked against a C stub of the header instead of `sdk_glue.c` + libsdk:
//! `tests/c/glue_stub.c` (the recording + replaying stub of the 75 functions sai-backend-openbcm's
//! C++ glue differential used, unchanged since 50a6aa7), included by `tests/c/real_stub_threads.c`,
//! which adds calls into the registered callbacks from threads it creates. Built and run by
//! `tests/real-stub.sh` (`SDK_GLUE_LINK=<the stub archive> cargo test -p sdk-glue --features real --test
//! real_stub`); without the feature this file is empty, so a normal workspace test run skips it.
//!
//! * [`glue_calls_cross_the_ffi_intact`]: one scenario over every public method of [`Attached`],
//!   [`Sdk`], [`Switch`] and the handles (every function of `sdk_glue.h` at least once, successes,
//!   scripted failures with negative codes, refusals that never reach C, callbacks inside calls and
//!   injected ones) runs first on a recording Rust fake chip, here, then in a child process (this
//!   binary again) on the real glue = the stub replaying that recording. The stub's call lines must
//!   equal the recorded ones (the inputs crossed the FFI intact: strings, MACs, addresses, the
//!   structs field by field, arrays) and the child's trace of what every wrapper returned must equal
//!   the fake run's (the outputs crossed back intact: port arrays, table usage, chip limits,
//!   temperatures, VLAN members, message buffers, `ob_sdk_errmsg`'s text, the gates' answers).
//! * [`callbacks_from_foreign_threads`]: the trampolines called by C from threads C created: closures
//!   run there, panics are caught and counted (a gate's answer is then "down"), bad arguments and
//!   stray contexts are refused before any closure runs, nothing runs after clear / unregister /
//!   drop, and concurrent calls during re-registrations each run exactly one closure.
//!
//! Each real-glue test runs in a child of its own (the glue's state is the process's):
//! `SDK_GLUE_STUB_RUNNER` prefixes the child's command line (e.g. `qemu-ppc64 -L <sysroot>`).
#![cfg(ob_glue_linked)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fmt::Debug;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use sdk_glue::rust_fake::Chip;
use sdk_glue::{
    Attached, CallbackStats, Dest, Error, Fec, FpQual, FpRule, Gate, InitError, IpType, L2Event, Learn, Loopback, Mmd,
    PolicerCfg, PortStat, RouteTarget, Sdk, Switch, Temperatures, TrapKind, TrunkMember,
};

/// The C entry points of `tests/c/real_stub_threads.c`.
mod stub {
    use std::ffi::{CStr, c_char, c_int};

    /// The registered pointers and their context.
    pub(crate) const CURRENT: c_int = 0;
    /// The registered pointers with a context no registration has.
    pub(crate) const STRAY: c_int = 1;
    /// The pointers and context `glue_stub_save` saw.
    pub(crate) const SAVED: c_int = 2;
    /// A call that found no pointer to call.
    pub(crate) const NONE: c_int = -100;

    // The signatures are those of tests/c/real_stub_threads.c (C int, uint64_t,
    // const char*). The functions declared safe take scalars only and accept any values: they read
    // the stub's pointers under its lock and call them (this crate's trampolines, which take any
    // arguments from any thread and never unwind) on threads they create and join.
    unsafe extern "C" {
        pub(crate) safe fn glue_stub_fire_link(how: c_int, port: c_int, up: c_int) -> c_int;
        pub(crate) safe fn glue_stub_fire_l2(
            how: c_int,
            mac: u64,
            null_mac: c_int,
            vid: c_int,
            port: c_int,
            learned: c_int,
        ) -> c_int;
        pub(crate) safe fn glue_stub_fire_gate(how: c_int, port: c_int) -> c_int;
        pub(crate) safe fn glue_stub_save();
        pub(crate) safe fn glue_stub_fire_link_storm(threads: c_int, each: c_int, port: c_int) -> c_int;
        pub(crate) safe fn glue_stub_line_buffered();
        fn glue_stub_inject_from_thread(line: *const c_char) -> c_int;
    }

    /// The test injection `line` replayed by the stub (`glue_stub_inject`) on a thread it creates:
    /// the callbacks the recording holds inside the injection of the same text run there.
    pub(crate) fn inject_from_thread(line: &CStr) -> c_int {
        // SAFETY: line is NUL-terminated and alive until the call returns; the C joins the thread
        // that reads it before it returns and keeps no pointer to it.
        unsafe { glue_stub_inject_from_thread(line.as_ptr()) }
    }
}

const CHILD: &str = "SDK_GLUE_REAL_STUB_CHILD";
const DIR: &str = "SDK_GLUE_REAL_STUB_DIR";
const RUNNER: &str = "SDK_GLUE_STUB_RUNNER";
/// The prefix of the stub's lines on stdout.
const GLUE: &str = "  glue ";

/// Every function of `sdk_glue.h` but `ob_sdk_errmsg` (which the stub does not log: its text is
/// checked on every error of the trace), by the stub's line names.
const FUNCTIONS: [&str; 74] = [
    "init",
    "attach",
    "temperatures",
    "th_overtemp_shutdown",
    "init_mdio",
    "mdio_c45_read",
    "mdio_c45_write",
    "linkscan_port_gate",
    "shutdown",
    "shell",
    "shell_to",
    "register_callbacks",
    "unregister_callbacks",
    "hw_info",
    "table_usage",
    "chip_limits",
    "cpu_port",
    "ports",
    "trap_set",
    "port_enable_set",
    "port_link_get",
    "port_speed_set",
    "port_speed_get",
    "port_speeds",
    "port_frame_max_set",
    "port_fec_set",
    "port_autoneg_set",
    "port_autoneg_get",
    "port_interface_set",
    "port_ifg_set",
    "port_loopback_set",
    "port_pvid_set",
    "port_learn_set",
    "port_stat_get",
    "port_stat_clear",
    "vlan_create",
    "vlan_destroy",
    "vlan_port_add",
    "vlan_port_remove",
    "vlan_members",
    "l2_add",
    "l2_delete",
    "l2_flush",
    "l2_age_set",
    "knet_cleanup",
    "netif_create",
    "netif_destroy",
    "netif_filter_set",
    "trunk_create",
    "trunk_destroy",
    "trunk_set",
    "l3_intf_create",
    "l3_intf_update",
    "l3_intf_destroy",
    "l3_station_add",
    "l3_station_delete",
    "l3_egress_create",
    "l3_egress_update",
    "l3_egress_special",
    "l3_egress_destroy",
    "l3_host_add",
    "l3_host_delete",
    "l3_route_add",
    "l3_route_delete",
    "l3_ecmp_set",
    "l3_ecmp_destroy",
    "fp_group_create",
    "fp_group_destroy",
    "fp_entry_create",
    "fp_entry_destroy",
    "fp_stat_get",
    "policer_create",
    "policer_set",
    "policer_destroy",
];

// ------------------------------------------------------------------ helpers

fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!(
        "sdk-glue-real-stub-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A line for whoever runs the suite: stderr, and `$SAI_RS_TEST_SUMMARY` (the target bundle's run.sh).
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Whether the linked glue is the stub (by its `ob_sdk_errmsg` text). This binary does not link
/// with the real `sdk_glue.c` at all: it lacks the stub's extra functions.
fn stub_linked() -> bool {
    Error::from_raw(-4).is_some_and(|e| e.message() == "glue stub error -4")
}

/// An error as the trace shows it. Its text came from `ob_sdk_errmsg` through the FFI (in both
/// runs: with the real glue linked, [`Error::message`] always asks the C).
fn err(e: Error) -> String {
    let text = e.message();
    assert_eq!(text, format!("glue stub error {}", e.raw()), "ob_sdk_errmsg's text");
    assert_eq!(e.to_string(), text);
    format!("Err({:?} {} {text:?})", e.code(), e.raw())
}

fn res<T: Debug>(r: Result<T, Error>) -> String {
    match r {
        Ok(v) => format!("Ok({v:?})"),
        Err(e) => err(e),
    }
}

fn init_err(e: &InitError) -> String {
    match e {
        InitError::Sdk { error, message } => format!("Sdk({} {message:?}) / {e}", err(*error)),
        other => format!("{other:?} / {other}"),
    }
}

fn bring<T>(r: &Result<(T, String), InitError>) -> String {
    match r {
        Ok((_, line)) => format!("Ok({line:?})"),
        Err(e) => init_err(e),
    }
}

fn temps(r: Result<Temperatures, Error>) -> String {
    res(r.map(|t| (t.current().to_vec(), t.peak().to_vec())))
}

fn mac(last: u8) -> [u8; 6] {
    [0x02, 0x11, 0x22, 0x33, 0x44, last]
}

/// A child process: this test binary running only `test`, in its child role, with `dir`; its
/// output (it must succeed).
fn child(test: &str, dir: &Path, replay: Option<&Path>) -> Output {
    let exe = std::env::current_exe().unwrap();
    let runner = std::env::var(RUNNER).unwrap_or_default();
    let mut words = runner.split_whitespace();
    let mut cmd = match words.next() {
        Some(prog) => {
            let mut c = Command::new(prog);
            c.args(words).arg(&exe);
            c
        }
        None => Command::new(&exe),
    };
    cmd.args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, test)
        .env(DIR, dir)
        .env_remove("GLUE_STUB_REPLAY");
    if let Some(r) = replay {
        cmd.env("GLUE_STUB_REPLAY", r);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "the child running {test} failed ({}):\n--- stdout\n{}\n--- stderr\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// The child's role starts: the test harness's "test <name> ... " ended by a newline, the stub's
/// stdout line-buffered, so every one of its lines is whole on stdout.
fn child_starts() -> PathBuf {
    println!();
    stub::glue_stub_line_buffered();
    PathBuf::from(std::env::var_os(DIR).unwrap())
}

/// The stub's lines of a child's stdout, prefix removed (indentation kept).
fn glue_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix(GLUE))
        .map(str::to_owned)
        .collect()
}

/// The function name of a stub line (`None` for a callback or an injection).
fn function(line: &str) -> Option<&str> {
    let l = line.trim_start();
    if l.starts_with("cb ") || l.starts_with("inject ") {
        return None;
    }
    l.split_once('(').map(|(name, _)| name)
}

/// Where two line lists first differ, for a readable failure.
fn first_difference(a: &[String], b: &[String]) -> String {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let at = |v: &[String], i: usize| v.get(i).cloned().unwrap_or_else(|| "<end>".to_owned());
    format!(
        "first difference at line {} of {} / {}:\n  fake: {}\n  real: {}",
        i + 1,
        a.len(),
        b.len(),
        at(a, i),
        at(b, i)
    )
}

// ------------------------------------------------------------------ the scenario

const SCENARIO: &str = "# sdk-glue tests/real_stub.rs
name T3TEST56960_B1
ports 4 4 100000
cpu 0
link admin
temp 450 461 472
knet 2 3
linkscan 4 hw
reg 24 1 8b00 2391
dead 30
";

/// One run of the scenario: on a recording Rust fake chip, or on the real glue (the stub).
struct Run {
    /// `None`: the real glue.
    chip: Option<Chip>,
    dir: PathBuf,
    cfg: PathBuf,
    rc: PathBuf,
    trace: Vec<String>,
    /// What the closures saw, moved into the trace after each step.
    events: Arc<Mutex<Vec<String>>>,
    /// The scenario's thread, and how many closure calls ran on another one.
    home: ThreadId,
    foreign: Arc<AtomicUsize>,
    /// Closure calls during injections.
    injected: usize,
}

impl Run {
    fn new(chip: Option<Chip>, dir: &Path) -> Self {
        Self {
            chip,
            dir: dir.to_path_buf(),
            cfg: dir.join("scenario.cfg"),
            rc: dir.join("init.rc"),
            trace: Vec::new(),
            events: Arc::new(Mutex::new(Vec::new())),
            home: std::thread::current().id(),
            foreign: Arc::new(AtomicUsize::new(0)),
            injected: 0,
        }
    }

    fn events(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// A step's result, then what the closures saw during it.
    fn note(&mut self, label: &str, what: impl Into<String>) {
        self.trace.push(format!("{label}: {}", what.into()));
        let seen = std::mem::take(&mut *self.events.lock().unwrap());
        self.trace.extend(seen.into_iter().map(|e| format!("  {e}")));
    }

    /// The next call of `op` (`rust_fake::OPS`) fails with `code` on the fake; the real glue gets
    /// the same answer from the replay.
    fn fail(&self, op: &str, code: i32) {
        if let Some(c) = &self.chip {
            assert!(c.fail(op, 1, code), "fail {op}");
        }
    }

    /// A test hook of the fake that is no glue call (the real glue's answers come from the replay).
    fn fake(&self, f: impl FnOnce(&Chip)) {
        if let Some(c) = &self.chip {
            f(c);
        }
    }

    /// A test injection: on the fake `f`, on this thread, recorded as `line`; on the real glue the
    /// stub replays the callbacks recorded inside `line` on a thread of its own.
    fn inject(&mut self, line: &str, f: impl FnOnce(&Chip)) {
        let before = self.events();
        match &self.chip {
            Some(c) => c.inject(line, || f(c)),
            None => assert_eq!(stub::inject_from_thread(&CString::new(line).unwrap()), 0),
        }
        self.injected += self.events() - before;
        self.note("inject", line);
    }

    fn init_mdio(&self, unit: i32, cfg: &Path) -> Result<(Sdk, String), InitError> {
        match &self.chip {
            Some(c) => Sdk::init_mdio_on(c, unit, cfg),
            None => Sdk::init_mdio(unit, cfg),
        }
    }

    fn init(&self, unit: i32, cfg: &Path, rc: Option<&Path>) -> Result<(Switch, String), InitError> {
        match &self.chip {
            Some(c) => Switch::init_on(c, unit, cfg, rc),
            None => Switch::init(unit, cfg, rc),
        }
    }

    fn attach(&self, unit: i32) -> Result<(Attached, String), InitError> {
        match &self.chip {
            Some(c) => {
                c.load(SCENARIO).unwrap();
                Attached::attach_on(c, unit)
            }
            None => Attached::attach(unit),
        }
    }

    /// What a closure saw, and whether it ran off the scenario's thread.
    fn recorder(&self) -> impl Fn(String) + Send + Sync + 'static {
        let (events, home, foreign) = (Arc::clone(&self.events), self.home, Arc::clone(&self.foreign));
        move |text| {
            if std::thread::current().id() != home {
                foreign.fetch_add(1, Ordering::Relaxed);
            }
            events.lock().unwrap().push(text);
        }
    }

    fn link_cb(&self, tag: &'static str, panic_on: Option<i32>) -> impl Fn(i32, bool) + Send + Sync + 'static {
        let rec = self.recorder();
        move |port, up| {
            rec(format!("{tag} link port={port} up={up}"));
            assert!(
                Some(port) != panic_on,
                "{tag}: this link closure panics on port {port} (expected)"
            );
        }
    }

    fn l2_cb(&self, tag: &'static str, panics: bool) -> impl Fn(L2Event) + Send + Sync + 'static {
        let rec = self.recorder();
        move |ev| {
            rec(format!("{tag} l2 {ev:?}"));
            assert!(!panics, "{tag}: this L2 closure panics (expected)");
        }
    }

    fn gate(&self, port: i32, answer: Option<Gate>) -> impl Fn() -> Gate + Send + Sync + 'static {
        let rec = self.recorder();
        move || {
            rec(format!("gate port={port} asked"));
            answer.unwrap_or_else(|| panic!("the gate of port {port} panics (expected)"))
        }
    }
}

/// Every public method of the crate's handles, so every function of `sdk_glue.h`.
fn scenario(run: &mut Run) {
    let cfg = run.cfg.clone();
    let d1 = Mmd::new(1).unwrap();
    let d31 = Mmd::new(31).unwrap();
    let devnull = File::options().write(true).open("/dev/null").unwrap();

    // ---- MDIO bring-up, MDIO, temperatures, the handles
    run.note("init_mdio nul", bring(&run.init_mdio(0, Path::new("bad\0path"))));
    run.note("init_mdio unit 1", bring(&run.init_mdio(1, &cfg)));
    let r = run.init_mdio(0, &cfg);
    run.note("init_mdio", bring(&r));
    let (mut sdk, _) = r.unwrap();
    run.note("init_mdio again", bring(&run.init_mdio(0, &cfg)));
    run.note("attach while open", bring(&run.attach(0)));
    run.note("unit", format!("{}", sdk.unit()));
    run.note("mdio read", res(sdk.mdio_c45_read(0x24, d1, 0x8b00)));
    run.note("mdio write", res(sdk.mdio_c45_write(0x17c, d31, 0xffff, 0xfffe)));
    run.note("mdio readback", res(sdk.mdio_c45_read(0x17c, d31, 0xffff)));
    run.note("mdio dead phy", res(sdk.mdio_c45_read(0x30, d1, 0)));
    run.note("mdio write dead phy", res(sdk.mdio_c45_write(0x30, d1, 0, 1)));
    run.note("mdio phy refused", res(sdk.mdio_c45_read(0x800, d1, 0)));
    run.note("mdio write refused", res(sdk.mdio_c45_write(u32::MAX, d1, 0, 0)));
    run.note("temperatures", temps(sdk.temperatures()));
    let (bus, reader) = (sdk.mdio_bus(), sdk.temp_reader());
    let (b2, r2) = (bus.clone(), reader.clone());
    let (read, written, current) = std::thread::spawn(move || {
        (
            b2.read(0x24, d1, 0x8b00),
            b2.write(0x7ff, d31, 1, 0xbeef),
            r2.read().map(|t| t.current().to_vec()),
        )
    })
    .join()
    .unwrap();
    run.note(
        "bus from a thread",
        format!("{} {} {}", res(read), res(written), res(current)),
    );
    run.note("bus readback", res(bus.read(0x7ff, d31, 1)));
    drop(sdk);
    run.note(
        "handles after drop",
        format!(
            "{} {} {} live={}",
            res(bus.read(0x24, d1, 0)),
            res(bus.write(0x24, d1, 0, 0)),
            temps(reader.read()),
            bus.is_live()
        ),
    );

    // ---- the full bring-up
    run.note("init unit 1", bring(&run.init(1, &cfg, None)));
    run.note(
        "init missing rc",
        bring(&run.init(0, &cfg, Some(&run.dir.join("no-such.rc")))),
    );
    run.note("init nul rc", bring(&run.init(0, &cfg, Some(Path::new("r\0c")))));
    let r = run.init(0, &cfg, Some(&run.rc.clone()));
    run.note("init", bring(&r));
    let (mut sw, _) = r.unwrap();
    run.note("init again", bring(&run.init(0, &cfg, None)));

    // ---- switch-wide
    run.note("hw_info", res(sw.hw_info()));
    run.fail("hw-info", -11);
    run.note("hw_info fail", res(sw.hw_info()));
    run.note("table_usage", res(sw.table_usage()));
    run.fail("table-usage", -16);
    run.note("table_usage fail", res(sw.table_usage()));
    run.note("chip_limits", res(sw.chip_limits()));
    run.fail("chip-limits", -16);
    run.note("chip_limits fail", res(sw.chip_limits()));
    run.note("knet_cleanup", res(sw.knet_cleanup()));
    run.fail("knet-cleanup", -77);
    run.note("knet_cleanup fail", res(sw.knet_cleanup()));
    run.note("cpu_port", format!("{}", sw.cpu_port()));
    run.note("ports", res(sw.ports()));
    run.fail("ports", -2);
    run.note("ports fail", res(sw.ports()));

    // ---- ports
    run.note("enable off", res(sw.port_enable_set(1, false)));
    run.note("enable on", res(sw.port_enable_set(1, true)));
    run.note("link_get", res(sw.port_link_get(1)));
    run.note("enable unknown port", res(sw.port_enable_set(99, true)));
    run.fail("enable", -18);
    run.note("enable fail", res(sw.port_enable_set(2, true)));
    run.fail("link-get", -9);
    run.note("link_get fail", res(sw.port_link_get(2)));
    run.note("speed_set", res(sw.port_speed_set(1, 40_000)));
    run.note("speed_set refused", res(sw.port_speed_set(1, u32::MAX)));
    run.note("speed_get", res(sw.port_speed_get(1)));
    run.fail("speed-get", -11);
    run.note("speed_get fail", res(sw.port_speed_get(1)));
    run.note("speeds", res(sw.port_speeds(1)));
    run.fail("speeds", -16);
    run.note("speeds fail", res(sw.port_speeds(1)));
    run.note("frame_max", res(sw.port_frame_max_set(1, 9216)));
    run.note("frame_max refused", res(sw.port_frame_max_set(1, u32::MAX)));
    run.note("autoneg_set", res(sw.port_autoneg_set(1, true)));
    run.note("autoneg_get", res(sw.port_autoneg_get(1)));
    run.fail("autoneg-get", -11);
    run.note("autoneg_get fail", res(sw.port_autoneg_get(1)));
    run.note("interface_set", res(sw.port_interface_set(1, c"kr4")));
    run.note("interface_set unknown", res(sw.port_interface_set(1, c"bogus\x01\x7f")));
    run.note("interface_set_name", res(sw.port_interface_set_name(2, "caui4")));
    run.note("interface_set_name refused", res(sw.port_interface_set_name(2, "nope")));
    for fec in [Fec::None, Fec::Rs, Fec::Fc] {
        run.note(&format!("fec {fec:?}"), res(sw.port_fec_set(1, fec)));
    }
    run.note("ifg", res(sw.port_ifg_set(1, 96)));
    run.note("ifg refused", res(sw.port_ifg_set(1, u32::MAX)));
    for mode in [Loopback::None, Loopback::Mac, Loopback::Phy] {
        run.note(&format!("loopback {mode:?}"), res(sw.port_loopback_set(1, mode)));
    }
    run.note("pvid", res(sw.port_pvid_set(1, 1)));
    run.note("pvid 0", res(sw.port_pvid_set(1, 0)));
    for mode in Learn::ALL {
        run.note(&format!("learn {mode:?}"), res(sw.port_learn_set(1, mode)));
    }
    run.fake(|c| {
        assert!(c.set_counter(1, PortStat::InOctets, 0x1_2345_6789_abcd));
        assert!(c.set_counter(1, PortStat::OutMulticastPkts, u64::MAX));
    });
    for stat in PortStat::ALL {
        run.note(&format!("stat {stat:?}"), res(sw.port_stat_get(1, stat)));
    }
    run.note("stat_clear", res(sw.port_stat_clear(1)));
    run.note("stat after clear", res(sw.port_stat_get(1, PortStat::InOctets)));
    run.fail("stat-get", -7);
    run.note("stat fail", res(sw.port_stat_get(1, PortStat::InOctets)));

    // ---- VLANs
    run.note("vlan_create", res(sw.vlan_create(10)));
    run.note("vlan_create again", res(sw.vlan_create(10)));
    run.note("vlan_create 0", res(sw.vlan_create(0)));
    run.note("vlan add tagged", res(sw.vlan_port_add(10, 1, false)));
    run.note("vlan add untagged", res(sw.vlan_port_add(10, 2, true)));
    run.note("vlan_members 10", res(sw.vlan_members(10)));
    run.note("vlan_members 1", res(sw.vlan_members(1)));
    run.note("vlan remove", res(sw.vlan_port_remove(10, 1)));
    run.note("vlan_members 10 after", res(sw.vlan_members(10)));
    run.fail("vlan-members", -5);
    run.note("vlan_members fail", res(sw.vlan_members(10)));
    run.note("vlan_destroy", res(sw.vlan_destroy(10)));
    run.note("vlan_destroy again", res(sw.vlan_destroy(10)));

    // ---- callbacks: link and L2
    let (link, l2) = (run.link_cb("A", Some(3)), run.l2_cb("A", false));
    run.note("set_callbacks", res(sw.set_callbacks(link, l2)));
    run.note("enable 2 off", res(sw.port_enable_set(2, false)));
    run.note("enable 2 on", res(sw.port_enable_set(2, true)));
    run.inject("inject link(port=3, up=1)", |c| {
        c.link_event(3, true);
    });
    run.inject("inject link(port=1, up=0)", |c| {
        c.link_event(1, false);
    });
    for (m, vid, port, learned) in [
        (Some(mac(1)), 10, 1, 1),
        (Some(mac(2)), 20, 0x4000_0005, 0),
        (None, 10, 1, 1),
        (Some(mac(3)), 0, 1, 1),
        (Some(mac(3)), 4095, 1, 1),
        (Some(mac(3)), 10, -2, 1),
        (Some(mac(3)), 10, 1, 2),
    ] {
        let shown = m.map_or_else(|| "null".to_owned(), |m| format!("{m:02x?}"));
        run.inject(
            &format!("inject l2(mac={shown}, vid={vid}, port={port}, learned={learned})"),
            |c| {
                c.l2_event(m, vid, port, learned);
            },
        );
    }
    run.note("stats", format!("{:?}", sw.callback_stats()));
    let link = run.link_cb("B", None);
    run.note("set_link_callback", res(sw.set_link_callback(link)));
    run.inject("inject link(port=1, up=1)", |c| {
        c.link_event(1, true);
    });
    sw.clear_link_callback();
    run.note("clear_link_callback", "");
    run.inject("inject link(port=1, up=0) unregistered", |c| {
        c.link_event(1, false);
    });
    let l2 = run.l2_cb("C", true);
    run.note("set_l2_callback panicking", res(sw.set_l2_callback(l2)));
    run.inject("inject l2 panics", |c| {
        c.l2_event(Some(mac(4)), 30, 2, 1);
    });
    run.fail("register", -14);
    let (link, l2) = (run.link_cb("D", None), run.l2_cb("D", false));
    run.note("set_callbacks refused", res(sw.set_callbacks(link, l2)));
    run.inject("inject link(port=2, up=0) after a refused registration", |c| {
        c.link_event(2, false);
    });
    let (link, l2) = (run.link_cb("E", None), run.l2_cb("E", false));
    run.note("set_callbacks again", res(sw.set_callbacks(link, l2)));
    run.note("stats", format!("{:?}", sw.callback_stats()));

    // ---- link gates
    let g = run.gate(1, Some(Gate::Up));
    run.note("gate 1", res(sw.set_port_gate(1, g)));
    let g = run.gate(2, Some(Gate::Down));
    run.note("gate 2", res(sw.set_port_gate(2, g)));
    let g = run.gate(3, None);
    run.note("gate 3 panicking", res(sw.set_port_gate(3, g)));
    let g = run.gate(4, Some(Gate::Up));
    run.note("gate 4 hw linkscan", res(sw.set_port_gate(4, g)));
    let g = run.gate(-1, Some(Gate::Up));
    run.note("gate -1 refused", res(sw.set_port_gate(-1, g)));
    run.note("gated", format!("{:?}", sw.gated_ports()));
    run.inject("inject linkscan()", |c| {
        c.linkscan();
    });
    run.note(
        "shell_to fake linkscan",
        res(sw.run_shell_to("fake linkscan", devnull.as_fd())),
    );
    run.fail("gate", -11);
    let g = run.gate(1, Some(Gate::Down));
    run.note("gate 1 refused by the glue", res(sw.set_port_gate(1, g)));
    run.inject("inject linkscan() 2", |c| {
        c.linkscan();
    });
    let g = run.gate(3, Some(Gate::NoOpinion));
    run.note("gate 3 replaced", res(sw.set_port_gate(3, g)));
    let g = run.gate(1, Some(Gate::Up));
    run.note("gate 1 again", res(sw.set_port_gate(1, g)));
    run.note("clear gate 2", res(sw.clear_port_gate(2)));
    run.note("clear gate 4 (none)", res(sw.clear_port_gate(4)));
    run.note("clear gate 99", res(sw.clear_port_gate(99)));
    run.inject("inject linkscan() 3", |c| {
        c.linkscan();
    });
    run.note("gated", format!("{:?}", sw.gated_ports()));
    run.note("stats", format!("{:?}", sw.callback_stats()));

    // ---- L2 table, trunks
    run.note("l2_add static", res(sw.l2_add(mac(10), 1, Dest::Port(1), true, false)));
    run.note("l2_add discard", res(sw.l2_add(mac(11), 1, Dest::Port(2), false, true)));
    run.note("l2_add vid 0", res(sw.l2_add(mac(12), 0, Dest::Port(1), false, false)));
    run.note(
        "l2_add port -1",
        res(sw.l2_add(mac(12), 1, Dest::Port(-1), false, false)),
    );
    run.note(
        "l2_add lag bit",
        res(sw.l2_add(mac(12), 1, Dest::Lag(0x4000_0000), false, false)),
    );
    run.note("l2_delete", res(sw.l2_delete(mac(10), 1)));
    run.note("l2_delete again", res(sw.l2_delete(mac(10), 1)));
    run.note("l2_delete vid 4095", res(sw.l2_delete(mac(10), 4095)));
    let tid = sw.trunk_create();
    run.note("trunk_create", res(tid));
    let tid = tid.unwrap();
    let members = [
        TrunkMember {
            port: 1,
            egress_disable: false,
            ingress_disable: false,
        },
        TrunkMember {
            port: 2,
            egress_disable: true,
            ingress_disable: true,
        },
        TrunkMember {
            port: 3,
            egress_disable: false,
            ingress_disable: true,
        },
    ];
    run.note("trunk_set", res(sw.trunk_set(tid, &members)));
    let many = vec![members[0]; 65];
    run.note("trunk_set 65 refused", res(sw.trunk_set(tid, &many)));
    run.note("trunk_set tid refused", res(sw.trunk_set(u32::MAX, &members)));
    run.note("trunk_set empty", res(sw.trunk_set(tid, &[])));
    run.note("l2_add lag", res(sw.l2_add(mac(13), 1, Dest::Lag(tid), false, false)));
    run.note(
        "l2_flush lag",
        res(sw.l2_flush(Some(Dest::Lag(tid)), None, false, true)),
    );
    run.note(
        "l2_flush port vid",
        res(sw.l2_flush(Some(Dest::Port(1)), Some(1), true, true)),
    );
    run.note("l2_flush statics only", res(sw.l2_flush(None, Some(1), true, false)));
    run.note("l2_flush nothing", res(sw.l2_flush(None, None, false, false)));
    run.note("l2_flush all", res(sw.l2_flush(None, None, true, true)));
    run.note("l2_flush vid refused", res(sw.l2_flush(None, Some(0), true, true)));
    run.note("l2_age", res(sw.l2_age_set(300)));
    run.note("l2_age refused", res(sw.l2_age_set(u32::MAX)));
    run.note("trunk_destroy", res(sw.trunk_destroy(tid)));
    run.note("trunk_destroy again", res(sw.trunk_destroy(tid)));
    run.fail("trunk-create", -14);
    run.note("trunk_create fail", res(sw.trunk_create()));
    run.fail("l2-add", -6);
    run.note("l2_add fail", res(sw.l2_add(mac(14), 1, Dest::Port(1), false, false)));

    // ---- KNET, traps
    let n = sw.netif_create("Ethernet0", 1, 1, mac(20), true);
    run.note("netif_create", res(n));
    let n = n.unwrap();
    run.note(
        "netif_create long name",
        res(sw.netif_create("Ethernet01234567", 1, 1, mac(21), false)),
    );
    run.note("netif_create nul", res(sw.netif_create("a\0b", 1, 1, mac(21), false)));
    run.note(
        "netif_create vid 0",
        res(sw.netif_create("Ethernet4", 1, 0, mac(21), false)),
    );
    run.note(
        "netif_create quoting",
        res(sw.netif_create("e\"\\\u{1}x", 2, 1, mac(22), false)),
    );
    let f = sw.netif_filter_set(n.netif, 2, false, Some(n.filter));
    run.note("netif_filter_set", res(f));
    run.note(
        "netif_filter_set no old",
        res(sw.netif_filter_set(n.netif, 3, true, None)),
    );
    run.note("netif_destroy", res(sw.netif_destroy(Some(n.netif), f.ok())));
    run.note("netif_destroy none", res(sw.netif_destroy(None, None)));
    run.fail("netif-create", -14);
    run.note(
        "netif_create fail",
        res(sw.netif_create("Ethernet8", 3, 1, mac(23), false)),
    );
    for kind in TrapKind::ALL {
        run.note(&format!("trap {kind:?}"), res(sw.trap_set(kind, true)));
    }
    run.note("trap lldp off", res(sw.trap_set(TrapKind::Lldp, false)));
    run.fail("trap", -11);
    run.note("trap fail", res(sw.trap_set(TrapKind::Lacp, true)));

    // ---- L3
    let intf = sw.l3_intf_create(mac(30), 1, 9100, 0);
    run.note("intf_create", res(intf));
    let intf = intf.unwrap();
    run.note("intf_update", res(sw.l3_intf_update(intf, mac(31), 1, 1500, 2)));
    run.note("intf_create vid 0", res(sw.l3_intf_create(mac(30), 0, 1500, 0)));
    run.note(
        "intf_create mtu refused",
        res(sw.l3_intf_create(mac(30), 1, u32::MAX, 0)),
    );
    let st = sw.l3_station_add(mac(32));
    run.note("station_add", res(st));
    let e1 = sw.l3_egress_create(intf, mac(40), Dest::Port(1), 1);
    run.note("egress_create", res(e1));
    let e1 = e1.unwrap();
    let e2 = sw.l3_egress_create(intf, mac(41), Dest::Port(2), 1);
    run.note("egress_create 2", res(e2));
    let e2 = e2.unwrap();
    run.note(
        "egress_update",
        res(sw.l3_egress_update(e1, intf, mac(42), Dest::Port(3), 1)),
    );
    run.note(
        "egress_create refused",
        res(sw.l3_egress_create(intf, mac(43), Dest::Port(-3), 1)),
    );
    run.note("egress cpu", res(sw.l3_egress_cpu()));
    run.note("egress drop", res(sw.l3_egress_drop()));
    run.note("egress cpu again", res(sw.l3_egress_cpu()));
    let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    let v6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0xabcd, 1));
    run.note("host_add v4", res(sw.l3_host_add(0, v4, e1)));
    run.note("host_add v6", res(sw.l3_host_add(1, v6, e2)));
    run.note("host_add vrf refused", res(sw.l3_host_add(u32::MAX, v4, e1)));
    let net4 = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0));
    let mask4 = IpAddr::V4(Ipv4Addr::new(255, 255, 0, 0));
    let net6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0xff, 0, 0, 0, 0, 0));
    let mask6 = IpAddr::V6(Ipv6Addr::new(0xffff, 0xffff, 0xffff, 0xff00, 0, 0, 0, 0));
    run.note(
        "route_add v4",
        res(sw.l3_route_add(0, net4, mask4, RouteTarget::Egress(e1), false, false)),
    );
    run.note(
        "route_add v4 replace drop",
        res(sw.l3_route_add(0, net4, mask4, RouteTarget::Egress(e2), true, true)),
    );
    run.note(
        "route_add mixed refused",
        res(sw.l3_route_add(0, net4, mask6, RouteTarget::Egress(e1), false, false)),
    );
    let ecmp = sw.l3_ecmp_create(&[e1, e2]);
    run.note("ecmp_create", res(ecmp));
    let ecmp = ecmp.unwrap();
    run.note("ecmp_update", res(sw.l3_ecmp_update(ecmp, &[e2])));
    run.note("ecmp_create empty refused", res(sw.l3_ecmp_create(&[])));
    run.note("ecmp_create 257 refused", res(sw.l3_ecmp_create(&[e1; 257])));
    run.note(
        "route_add v6 ecmp",
        res(sw.l3_route_add(3, net6, mask6, RouteTarget::Ecmp(ecmp), false, false)),
    );
    run.note("route_delete v6", res(sw.l3_route_delete(3, net6, mask6)));
    run.note("route_delete v4", res(sw.l3_route_delete(0, net4, mask4)));
    run.note("route_delete again", res(sw.l3_route_delete(0, net4, mask4)));
    run.note("route_delete mixed refused", res(sw.l3_route_delete(0, net6, mask4)));
    run.note("host_delete v4", res(sw.l3_host_delete(0, v4)));
    run.note("host_delete v6", res(sw.l3_host_delete(1, v6)));
    run.note("ecmp_destroy", res(sw.l3_ecmp_destroy(ecmp)));
    run.note("ecmp_destroy again", res(sw.l3_ecmp_destroy(ecmp)));
    run.fail("ecmp-set", -14);
    run.note("ecmp_create fail", res(sw.l3_ecmp_create(&[e1])));

    // ---- policers and field entries
    let cfg1 = PolicerCfg {
        tr_tcm: true,
        packets: false,
        color_blind: true,
        cir: 0x1_0000_0001,
        cbs: 12_345,
        pir: u64::MAX / 3,
        pbs: 7,
    };
    let pid = sw.policer_create(&cfg1);
    run.note("policer_create", res(pid));
    let pid = pid.unwrap();
    let cfg2 = PolicerCfg {
        tr_tcm: false,
        packets: true,
        color_blind: false,
        cir: 1000,
        cbs: 64,
        pir: 0,
        pbs: 0,
    };
    run.note("policer_set", res(sw.policer_set(pid, &cfg2)));
    run.note("policer_set refused", res(sw.policer_set(u32::MAX, &cfg2)));
    run.fail("policer-create", -14);
    run.note("policer_create fail", res(sw.policer_create(&cfg2)));
    let gid = sw.fp_group_create(&FpQual::ALL, 5);
    run.note("fp_group_create", res(gid));
    let gid = gid.unwrap();
    let rule = FpRule {
        in_ports: Some(vec![1, 2, 4]),
        ether_type: Some((0x0800, 0xffff)),
        ip_type: Some(IpType::Ipv4),
        ip_protocol: Some((6, 0xff)),
        src_ip: Some(([10, 0, 0, 1], [255, 255, 255, 0])),
        dst_ip: None,
        src_ip6: None,
        dst_ip6: Some((
            [0x20, 0x01, 0x0d, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            [0xff; 16],
        )),
        l4_src_port: Some((1024, 0xfc00)),
        l4_dst_port: Some((179, 0xffff)),
        tcp_flags: Some((0x12, 0x3f)),
        icmp_type_code: Some((0x0800, 0xff00)),
        tos: Some((0xb8, 0xfc)),
        dst_mac: Some((mac(50), [0xff; 6])),
        src_mac: Some((mac(51), [0xff, 0xff, 0xff, 0, 0, 0])),
        outer_vlan: Some((100, 0x0fff)),
        ttl: Some((1, 0xff)),
        dst_l3_egress: Some(e1),
        priority: 7,
        drop: true,
        drop_cancel: false,
        copy_cpu: true,
        counter: true,
        policer: Some(pid),
        green_drop: false,
        yellow_drop: true,
        red_drop: true,
        cpu_copy_only: false,
    };
    let entry = sw.fp_entry_create(gid, &rule);
    run.note("fp_entry_create", res(entry));
    let entry = entry.unwrap();
    let raw = sdk_glue::field::FpRuleRaw {
        present: FpQual::EtherType.bit() | FpQual::InPorts.bit(),
        ether: 0x88cc,
        ether_mask: 0xffff,
        ip_type: -1,
        ports: vec![3],
        priority: -2,
        copy_cpu: true,
        policer: -1,
        dst_egress: -5,
        ..sdk_glue::field::FpRuleRaw::default()
    };
    let entry2 = sw.fp_entry_create_raw(gid, &raw);
    run.note("fp_entry_create_raw", res(entry2));
    let both = FpRule {
        src_ip: Some(([1, 2, 3, 4], [0xff; 4])),
        src_ip6: Some(([1; 16], [0xff; 16])),
        ..FpRule::default()
    };
    run.note("fp_entry_create refused", res(sw.fp_entry_create(gid, &both)));
    if let Some(stat) = entry.stat {
        run.fake(|c| {
            assert!(c.fp_hit(stat, 10, 0x1_0000_0000));
        });
        run.note("fp_stat_get", res(sw.fp_stat_get(stat)));
    }
    run.note("fp_stat_get refused", res(sw.fp_stat_get(u32::MAX)));
    run.fail("fp-stat", -7);
    run.note("fp_stat_get fail", res(sw.fp_stat_get(0)));
    run.fail("fp-entry-create", -6);
    run.note("fp_entry_create fail", res(sw.fp_entry_create(gid, &rule)));
    run.note("fp_entry_destroy", res(sw.fp_entry_destroy(entry)));
    if let Ok(e) = entry2 {
        run.note("fp_entry_destroy raw", res(sw.fp_entry_destroy(e)));
    }
    run.note("fp_entry_destroy again", res(sw.fp_entry_destroy(entry)));
    run.note("fp_group_destroy", res(sw.fp_group_destroy(gid)));
    run.note("policer_destroy", res(sw.policer_destroy(pid)));
    run.note("egress_destroy", res(sw.l3_egress_destroy(e2)));
    run.note("egress_destroy refused", res(sw.l3_egress_destroy(u32::MAX)));
    if let Ok(s) = st {
        run.note("station_delete", res(sw.l3_station_delete(s)));
    }
    run.note("intf_destroy", res(sw.l3_intf_destroy(intf)));
    run.note("intf_destroy again", res(sw.l3_intf_destroy(intf)));

    // ---- diag shell, MDIO and temperatures of the switch
    run.note("shell", res(sw.run_shell("ps")));
    run.note(
        "shell quoting",
        res(sw.run_shell("echo \"q\" \\ \u{1}\u{7f} caf\u{e9}")),
    );
    run.note("shell nul", res(sw.run_shell("a\0b")));
    run.note("shell 1023", res(sw.run_shell(&"x".repeat(1023))));
    run.note("shell 1024 refused", res(sw.run_shell(&"x".repeat(1024))));
    run.note(
        "shell_to",
        res(sw.run_shell_to("phy raw c45 24 1 8b00", devnull.as_fd())),
    );
    run.fail("shell", -11);
    run.note("shell fail", res(sw.run_shell("bogus")));
    run.note("switch mdio", res(sw.mdio_c45_read(0x24, d1, 0x8b00)));
    run.note("switch temperatures", temps(sw.temperatures()));
    run.note("stats", format!("{:?}", sw.callback_stats()));
    run.note("gated", format!("{:?}", sw.gated_ports()));
    drop(sw);
    run.note("switch dropped", "");
    run.inject("inject link(port=1, up=1) after the switch", |c| {
        c.link_event(1, true);
    });

    // ---- attach only
    let r = run.attach(0);
    run.note("attach", bring(&r));
    let (mut at, _) = r.unwrap();
    run.note("attach unit", format!("{}", at.unit()));
    run.note("attach temperatures", temps(at.temperatures()));
    let reader = at.temp_reader();
    let r2 = reader.clone();
    let peak = std::thread::spawn(move || r2.read().map(|t| t.peak().to_vec()))
        .join()
        .unwrap();
    run.note("attach temperatures from a thread", res(peak));
    run.fail("overtemp", -16);
    run.note("overtemp fail", res(at.overtemp_shutdown()));
    run.note("overtemp", res(at.overtemp_shutdown()));
    run.note("temperatures after overtemp", temps(at.temperatures()));
    drop(at);
    run.note("reader after drop", temps(reader.read()));
}

/// The scenario on a recording fake chip: its glue lines, and the run (trace, counts).
fn fake_run(dir: &Path) -> (Vec<String>, Run) {
    let chip = Chip::new();
    chip.record(true);
    let mut run = Run::new(Some(chip.clone()), dir);
    scenario(&mut run);
    let lines = chip.take_record();
    chip.record(false);
    (lines, run)
}

fn files(dir: &Path) {
    std::fs::write(dir.join("scenario.cfg"), SCENARIO).unwrap();
    std::fs::write(dir.join("init.rc"), "# rc\n").unwrap();
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn glue_calls_cross_the_ffi_intact() {
    const NAME: &str = "glue_calls_cross_the_ffi_intact";
    if !stub_linked() {
        summary(&format!("{NAME}: SKIPPED: the linked glue is not glue_stub.c"));
        return;
    }
    if std::env::var(CHILD).is_ok_and(|c| c == NAME) {
        // the child: the real glue, its answers replayed from the parent's recording
        let dir = child_starts();
        let mut run = Run::new(None, &dir);
        scenario(&mut run);
        // what the real glue's boot rule refuses before any C call (the stub would print a line)
        let other = dir.join("other.cfg");
        run.note("real: init_mdio other config", bring(&run.init_mdio(0, &other)));
        run.note("real: init other config", bring(&run.init(0, &other, None)));
        // the injected callbacks ran on the stub's threads, the ones replayed inside calls on this one
        let foreign = run.foreign.load(Ordering::Relaxed);
        assert!(run.injected > 0);
        assert_eq!(foreign, run.injected, "closure calls off the scenario's thread");
        std::fs::write(dir.join("trace.real.txt"), run.trace.join("\n") + "\n").unwrap();
        std::fs::write(dir.join("foreign.txt"), format!("{foreign}\n")).unwrap();
        return;
    }
    let dir = scratch("ffi");
    files(&dir);
    let (lines, run) = fake_run(&dir);
    assert_eq!(
        run.foreign.load(Ordering::Relaxed),
        0,
        "the fake calls back on the calling thread"
    );
    let replay = dir.join("replay.txt");
    std::fs::write(&replay, lines.join("\n") + "\n").unwrap();
    std::fs::write(dir.join("trace.fake.txt"), run.trace.join("\n") + "\n").unwrap();

    let out = child(NAME, &dir, Some(&replay));
    let real_lines = glue_lines(&out);
    std::fs::write(dir.join("stub-lines.txt"), real_lines.join("\n") + "\n").unwrap();
    assert!(
        real_lines == lines,
        "the stub's lines differ from the recording ({}): {}",
        dir.display(),
        first_difference(&lines, &real_lines)
    );
    let real_trace = read_lines(&dir.join("trace.real.txt"));
    let (shared, real_only) = real_trace.split_at(run.trace.len().min(real_trace.len()));
    assert!(
        shared == run.trace.as_slice(),
        "the wrappers' results differ ({}): {}",
        dir.display(),
        first_difference(&run.trace, shared)
    );
    assert_eq!(real_only.len(), 2, "{real_only:?}");
    for l in real_only {
        assert!(l.contains("Sdk(Err(Config -15") && l.contains("would ignore"), "{l}");
    }

    // every function of sdk_glue.h crossed the FFI
    let crossed: BTreeSet<&str> = real_lines.iter().filter_map(|l| function(l)).collect();
    let missing: Vec<&str> = FUNCTIONS.iter().copied().filter(|f| !crossed.contains(f)).collect();
    assert!(missing.is_empty(), "functions the scenario never called: {missing:?}");
    let unknown: Vec<&&str> = crossed.iter().filter(|f| !FUNCTIONS.contains(f)).collect();
    assert!(unknown.is_empty(), "{unknown:?}");
    let calls = real_lines.iter().filter(|l| function(l).is_some()).count();
    let failed = real_lines
        .iter()
        .filter(|l| function(l).is_some() && l.contains(" -> -"))
        .count();
    let callbacks = real_lines.iter().filter(|l| l.trim_start().starts_with("cb ")).count();
    let errors = run.trace.iter().filter(|l| l.contains("Err(")).count();
    summary(&format!(
        "{NAME}: {} glue lines identical ({calls} calls of {} functions + ob_sdk_errmsg, {failed} with a \
         negative code; {callbacks} callbacks); {} trace lines identical ({errors} with an error, each text from \
         ob_sdk_errmsg); {} closure calls on the stub's threads",
        lines.len(),
        crossed.len(),
        run.trace.len(),
        std::fs::read_to_string(dir.join("foreign.txt")).unwrap().trim()
    ));
}

// ------------------------------------------------------------------ callbacks from C threads

/// What the closures of the thread test saw, with the thread each call ran on.
#[derive(Default)]
struct Seen {
    calls: Mutex<Vec<(String, ThreadId)>>,
}

impl Seen {
    fn push(&self, text: String) {
        self.calls.lock().unwrap().push((text, std::thread::current().id()));
    }

    /// The texts since the last take; each call must have run off `home` (on the C thread).
    fn take(&self, home: ThreadId) -> Vec<String> {
        let calls = std::mem::take(&mut *self.calls.lock().unwrap());
        calls
            .into_iter()
            .map(|(text, at)| {
                assert_ne!(at, home, "{text} ran on the test's thread, not on the C thread");
                text
            })
            .collect()
    }
}

/// No closure call.
const NOTHING: [&str; 0] = [];

fn stats(sw: &Switch, link: u64, l2: u64, gate: u64, rejected: u64) {
    assert_eq!(
        sw.callback_stats(),
        CallbackStats {
            link_panics: link,
            l2_panics: l2,
            gate_panics: gate,
            rejected
        }
    );
}

fn mac64(m: [u8; 6]) -> u64 {
    m.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b))
}

fn l2(m: [u8; 6], vid: u16, dest: Dest, learned: bool) -> String {
    format!(
        "l2 {:?}",
        L2Event {
            mac: m,
            vid,
            dest,
            learned
        }
    )
}

fn callbacks_child(dir: &Path) {
    use stub::{CURRENT, NONE, SAVED, STRAY};
    let home = std::thread::current().id();
    let cfg = dir.join("scenario.cfg");
    let (mut sw, _) = Switch::init(0, &cfg, None).unwrap();
    let seen = Arc::new(Seen::default());
    let (s1, s2) = (Arc::clone(&seen), Arc::clone(&seen));
    sw.set_callbacks(
        move |port, up| s1.push(format!("link {port} {up}")),
        move |ev| s2.push(format!("l2 {ev:?}")),
    )
    .unwrap();

    // link and L2 events from a thread C created
    assert_eq!(stub::glue_stub_fire_link(CURRENT, 3, 1), 0);
    assert_eq!(stub::glue_stub_fire_link(CURRENT, 0, 0), 0);
    assert_eq!(seen.take(home), ["link 3 true", "link 0 false"]);
    assert_eq!(stub::glue_stub_fire_l2(CURRENT, mac64(mac(1)), 0, 10, 1, 1), 0);
    assert_eq!(
        stub::glue_stub_fire_l2(CURRENT, mac64(mac(2)), 0, 4094, 0x4000_0007, 0),
        0
    );
    assert_eq!(
        seen.take(home),
        [
            l2(mac(1), 10, Dest::Port(1), true),
            l2(mac(2), 4094, Dest::Lag(7), false)
        ]
    );
    stats(&sw, 0, 0, 0, 0);
    // arguments out of range: refused and counted, no closure runs
    for (port, up) in [(3, 2), (3, -1), (-1, 1), (i32::MIN, 0)] {
        assert_eq!(stub::glue_stub_fire_link(CURRENT, port, up), 0);
    }
    for (null, vid, port, learned) in [
        (1, 10, 1, 1),
        (0, 0, 1, 1),
        (0, 4095, 1, 1),
        (0, -1, 1, 1),
        (0, 10, -5, 1),
        (0, 10, 1, 2),
        (0, 10, 1, -1),
    ] {
        assert_eq!(
            stub::glue_stub_fire_l2(CURRENT, mac64(mac(3)), null, vid, port, learned),
            0
        );
    }
    assert_eq!(seen.take(home), NOTHING);
    stats(&sw, 0, 0, 0, 11);
    // a context no registration has: nothing runs, nothing is counted
    assert_eq!(stub::glue_stub_fire_link(STRAY, 1, 1), 0);
    assert_eq!(stub::glue_stub_fire_l2(STRAY, mac64(mac(4)), 0, 10, 1, 1), 0);
    assert_eq!(seen.take(home), NOTHING);
    stats(&sw, 0, 0, 0, 11);

    // panicking closures: caught on the C thread, counted, the process goes on
    let s3 = Arc::clone(&seen);
    sw.set_link_callback(move |port, _| {
        s3.push(format!("panicking link {port}"));
        panic!("the link closure panics (expected)");
    })
    .unwrap();
    assert_eq!(stub::glue_stub_fire_link(CURRENT, 2, 1), 0);
    assert_eq!(stub::glue_stub_fire_link(CURRENT, 2, 0), 0);
    assert_eq!(seen.take(home), ["panicking link 2", "panicking link 2"]);
    stats(&sw, 2, 0, 0, 11);
    let s4 = Arc::clone(&seen);
    sw.set_l2_callback(move |ev| {
        s4.push(format!("panicking l2 {}", ev.vid));
        panic!("the L2 closure panics (expected)");
    })
    .unwrap();
    assert_eq!(stub::glue_stub_fire_l2(CURRENT, mac64(mac(5)), 0, 20, 1, 1), 0);
    assert_eq!(seen.take(home), ["panicking l2 20"]);
    stats(&sw, 2, 1, 0, 11);

    // gates: the answers as C sees them
    let gate = |answer: Option<Gate>, port: i32| {
        let s = Arc::clone(&seen);
        move || {
            s.push(format!("gate {port}"));
            answer.unwrap_or_else(|| panic!("the gate closure of port {port} panics (expected)"))
        }
    };
    sw.set_port_gate(5, gate(Some(Gate::Up), 5)).unwrap();
    sw.set_port_gate(6, gate(Some(Gate::Down), 6)).unwrap();
    sw.set_port_gate(7, gate(None, 7)).unwrap();
    sw.set_port_gate(8, gate(Some(Gate::NoOpinion), 8)).unwrap();
    assert_eq!(sw.gated_ports(), [5, 6, 7, 8]);
    assert_eq!(stub::glue_stub_fire_gate(CURRENT, 5), 1);
    assert_eq!(stub::glue_stub_fire_gate(CURRENT, 6), 0);
    assert_eq!(
        stub::glue_stub_fire_gate(CURRENT, 7),
        0,
        "a panicking gate answers down"
    );
    assert_eq!(stub::glue_stub_fire_gate(CURRENT, 8), -1);
    assert_eq!(seen.take(home), ["gate 5", "gate 6", "gate 7", "gate 8"]);
    stats(&sw, 2, 1, 1, 11);
    // no gate, any port, a stray context: no opinion, no closure
    for port in [9, -1, i32::MAX, i32::MIN] {
        assert_eq!(stub::glue_stub_fire_gate(CURRENT, port), -1);
    }
    assert_eq!(stub::glue_stub_fire_gate(STRAY, 5), -1);
    assert_eq!(seen.take(home), NOTHING);
    stats(&sw, 2, 1, 1, 11);

    // 8 C threads calling the link callback while it is replaced 200 times: every call that found
    // the pointer ran exactly one closure (an old or the new one); none was lost or doubled. A
    // counting closure is in place before the storm starts: the C threads may call before the
    // loop's first replacement, and the panicking closure registered above counts no run (it
    // made the assertion below race: runs fell short of found by the calls that came first).
    let runs = Arc::new(AtomicU64::new(0));
    let counting = |r: Arc<AtomicU64>| {
        move |_, _| {
            r.fetch_add(1, Ordering::Relaxed);
        }
    };
    sw.set_link_callback(counting(Arc::clone(&runs))).unwrap();
    let storm = std::thread::spawn(|| stub::glue_stub_fire_link_storm(8, 2000, 4));
    for _ in 0..200 {
        sw.set_link_callback(counting(Arc::clone(&runs))).unwrap();
    }
    let found = storm.join().unwrap();
    assert!(found > 0, "{found}");
    assert_eq!(runs.load(Ordering::Relaxed), u64::try_from(found).unwrap());
    stats(&sw, 2, 1, 1, 11);

    // late calls with the pointers and the context of a registration that is gone
    let s5 = Arc::clone(&seen);
    sw.set_link_callback(move |port, up| s5.push(format!("late link {port} {up}")))
        .unwrap();
    stub::glue_stub_save();
    sw.clear_port_gate(5).unwrap();
    assert_eq!(stub::glue_stub_fire_gate(SAVED, 5), -1);
    assert_eq!(stub::glue_stub_fire_gate(CURRENT, 5), -1);
    assert_eq!(stub::glue_stub_fire_gate(SAVED, 6), 0);
    assert_eq!(seen.take(home), ["gate 6"]);
    sw.clear_callbacks();
    assert_eq!(
        stub::glue_stub_fire_link(CURRENT, 1, 1),
        NONE,
        "the glue holds no link pointer"
    );
    assert_eq!(stub::glue_stub_fire_l2(CURRENT, mac64(mac(6)), 0, 10, 1, 1), NONE);
    assert_eq!(stub::glue_stub_fire_link(SAVED, 1, 1), 0);
    assert_eq!(stub::glue_stub_fire_l2(SAVED, mac64(mac(6)), 0, 10, 1, 1), 0);
    assert_eq!(seen.take(home), NOTHING, "a closure ran after clear_callbacks");
    stats(&sw, 2, 1, 1, 11);
    drop(sw);
    for port in [6, 7, 8] {
        assert_eq!(stub::glue_stub_fire_gate(SAVED, port), -1);
    }
    assert_eq!(stub::glue_stub_fire_link(SAVED, 1, 1), 0);
    assert_eq!(stub::glue_stub_fire_link(SAVED, 1, 9), 0);
    assert_eq!(seen.take(home), NOTHING, "a closure ran after the switch was dropped");

    // the next switch has a new key: the old registration's context still finds nothing
    let (mut sw, _) = Switch::init(0, &cfg, None).unwrap();
    let s6 = Arc::clone(&seen);
    sw.set_link_callback(move |port, up| s6.push(format!("new link {port} {up}")))
        .unwrap();
    assert_eq!(stub::glue_stub_fire_link(SAVED, 1, 1), 0);
    assert_eq!(seen.take(home), NOTHING);
    assert_eq!(stub::glue_stub_fire_link(CURRENT, 1, 1), 0);
    assert_eq!(seen.take(home), ["new link 1 true"]);
    stats(&sw, 0, 0, 0, 0);
    drop(sw);
}

#[test]
fn callbacks_from_foreign_threads() {
    const NAME: &str = "callbacks_from_foreign_threads";
    if !stub_linked() {
        summary(&format!("{NAME}: SKIPPED: the linked glue is not glue_stub.c"));
        return;
    }
    if std::env::var(CHILD).is_ok_and(|c| c == NAME) {
        callbacks_child(&child_starts());
        return;
    }
    let dir = scratch("threads");
    files(&dir);
    let out = child(NAME, &dir, None);
    let lines = glue_lines(&out);
    // what the switches asked the glue (no replay: every call answered 0)
    let count = |prefix: &str| lines.iter().filter(|l| l.starts_with(prefix)).count();
    assert_eq!(count("init(unit=0, "), 2, "{} lines", lines.len());
    assert!(count("register_callbacks(unit=0, link=set, l2=set, ctx=set) -> 0") > 200);
    assert_eq!(count("linkscan_port_gate(unit=0, port=5, gate=set, ctx=set) -> 0"), 1);
    assert_eq!(count("linkscan_port_gate(unit=0, port=5, gate=null, ctx=set) -> 0"), 1);
    assert_eq!(count("shutdown(unit=0) -> void"), 2);
    assert!(count("unregister_callbacks(unit=0) -> void") >= 200);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("panics (expected)"),
        "the panics were reported: {stderr}"
    );
    summary(&format!(
        "{NAME}: {} glue lines; link / L2 / gate closures ran on C threads, panics caught",
        lines.len()
    ));
}

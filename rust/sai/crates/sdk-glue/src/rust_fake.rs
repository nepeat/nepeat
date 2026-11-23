//! A fake of the glue in pure Rust (feature `rust-fake`): an in-memory switch chip behind the same
//! interface as the real glue ([`crate::glue`]), with scripted faults per function and callbacks
//! injected from any thread. No C, no hardware: a program built with only this fake calls no
//! foreign function (Miri runs it), and any number of [`Chip`]s can be used in parallel.
//!
//! It replaced `sdk-glue-sys/fake/fake_glue.c` (deleted) and reads the same scenario files, with the same
//! answers, messages and trace lines for the functions the C fake has; everything else of
//! `sdk_glue.h` is modelled here too (ports in full, L2 table, trunks, L3 tables, field groups /
//! entries / counters, policers, KNET netifs / filters, traps, shell, table usage, chip limits,
//! attach-only mode). It also does the glue's own argument checks (`sdk_glue.c` `OB_CHECK_*`), so
//! a wrapper's refusals are tested against the same codes.
//!
//! # Scenario (the "config file" of a bring-up)
//!
//! One directive per line, `#` comments. MDIO directives take hex numbers (decimal where noted),
//! switch directives decimal. A line that is not a directive fails the bring-up
//! (`OB_E_CONFIG`, "fake glue: <file>:<line>: not a scenario line").
//!
//! | directive | effect |
//! |---|---|
//! | `name <text>` | chip name (default `FAKE56960_A0`) |
//! | `init <busy\|unit\|soc\|misc\|delay\|divisor\|all\|rc\|attach>` | the bring-up fails that way, with the real glue's text |
//! | `reg <phy> <dev> <reg> <val>` | a register's value (default 0; a write is read back) |
//! | `const <phy> <val>` | the PHY reads `<val>` on every register |
//! | `dead <phy>` | every access to the PHY fails (`OB_E_TIMEOUT`) |
//! | `after <n> <phy> <dev> <reg> <val>` | (n decimal) the register reads `<val>` from its (n+1)th read on |
//! | `hang <n> <phy> <val>` | (n decimal) once n writes were made, the PHY reads `<val>` everywhere |
//! | `fail-read <n>` / `fail-write <n>` | (decimal) the n-th read / write fails (`OB_E_FAIL`) |
//! | `temp <t>...` | (decimal, tenths of C) the monitors; peak = current + 15; none: `OB_E_UNAVAIL` |
//! | `temp-fail <n>` | (decimal) temperature reads from the n-th on fail (`OB_E_TIMEOUT`) |
//! | `sigterm <n>` | (decimal) SIGTERM to this process right after the n-th write |
//! | `trace <path>` | every call is appended to that file as one line |
//! | `ports <n> <lanes> <speed>` | n front-panel ports: logical 1..n, first lane 1 + i * lanes, names ce0.. |
//! | `port <logical> <phys> <lanes> <speed> <name>` | one front-panel port |
//! | `cpu <logical>` | the CPU port (default 0) |
//! | `link <admin\|none>` | admin: enabling a port brings its link up (callback from the enabling thread) |
//! | `fail <op> <n> <code>` | the n-th call of `<op>` ([`OPS`]) returns `<code>` (a negative `OB_E_*` number) |
//! | `knet <netifs> <filters>` | stale KNET objects the bring-up (and [`crate::Switch::knet_cleanup`]) destroy |
//! | `linkscan <port> <sw\|hw>` | the port's link scan mode (default sw; a gate needs sw) |
//!
//! After [`crate::Switch::init_on`], as after "init all": every port enabled, link down, VLAN 1 with
//! every front-panel port untagged; MDIO allowed. After [`crate::Attached::attach_on`] (scenario
//! from [`Chip::load`]): temperatures only.
//!
//! # The C fake
//!
//! `fake_glue.c` and `sdk-glue-sys`'s feature `fake` are gone (G2): every user runs on this fake.
//! Its MDIO side lives on as a C test fixture, `mdio-tool/tests/c/mdio_stub.c`, which the C MDIO
//! tool is linked with for mdio-tool's differential; that test holds the two to the same answers.

#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the methods have the glue interface's signatures (crate::glue), shared with the real glue"
)]
#![allow(clippy::struct_excessive_bools, reason = "a chip's flags")]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, c_int, c_uint};
use std::fs::File;
use std::io::Write as _;
use std::os::fd::BorrowedFd;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use sdk_glue_sys as sys;

use crate::link::raw;

/// Every operation name a `fail <op> <n> <code>` directive (or [`Chip::fail`]) can name. The first
/// seventeen are the C fake's.
pub const OPS: &[&str] = &[
    "register",
    "hw-info",
    "ports",
    "enable",
    "link-get",
    "speed-set",
    "speed-get",
    "speeds",
    "fec",
    "autoneg",
    "interface",
    "ifg",
    "vlan-create",
    "vlan-destroy",
    "vlan-add",
    "vlan-remove",
    "vlan-members",
    "frame-max",
    "autoneg-get",
    "loopback",
    "pvid",
    "learn",
    "stat-get",
    "stat-clear",
    "l2-add",
    "l2-delete",
    "l2-flush",
    "l2-age",
    "knet-cleanup",
    "netif-create",
    "netif-destroy",
    "netif-filter",
    "trap",
    "trunk-create",
    "trunk-destroy",
    "trunk-set",
    "intf-create",
    "intf-update",
    "intf-destroy",
    "station-add",
    "station-delete",
    "egress-create",
    "egress-update",
    "egress-special",
    "egress-destroy",
    "host-add",
    "host-delete",
    "route-add",
    "route-delete",
    "ecmp-set",
    "ecmp-destroy",
    "fp-group-create",
    "fp-group-destroy",
    "fp-entry-create",
    "fp-entry-destroy",
    "fp-stat",
    "policer-create",
    "policer-set",
    "policer-destroy",
    "table-usage",
    "chip-limits",
    "shell",
    "gate",
    "overtemp",
];

const NONE: c_int = 0;
const E_INTERNAL: c_int = sys::OB_E_INTERNAL;
const E_UNIT: c_int = sys::OB_E_UNIT;
const E_PARAM: c_int = sys::OB_E_PARAM;
const E_NOT_FOUND: c_int = sys::OB_E_NOT_FOUND;
const E_EXISTS: c_int = sys::OB_E_EXISTS;
const E_TIMEOUT: c_int = sys::OB_E_TIMEOUT;
const E_BUSY: c_int = sys::OB_E_BUSY;
const E_FAIL: c_int = sys::OB_E_FAIL;
const E_BADID: c_int = sys::OB_E_BADID;
const E_RESOURCE: c_int = sys::OB_E_RESOURCE;
const E_CONFIG: c_int = sys::OB_E_CONFIG;
const E_UNAVAIL: c_int = sys::OB_E_UNAVAIL;
const E_INIT: c_int = sys::OB_E_INIT;
const E_PORT: c_int = sys::OB_E_PORT;
const E_MDIO_CLOCK: c_int = sys::OB_E_MDIO_CLOCK;
const MAX_MON: usize = 16;
const MAX_TRUNKS: c_int = 1024;
const LAG_BIT: c_int = 0x4000_0000;

/// One front-panel port of the fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePort {
    /// Logical number.
    pub port: c_int,
    /// First physical port.
    pub phys: c_int,
    /// Lanes.
    pub lanes: c_int,
    /// Speed in Mb/s.
    pub speed: c_int,
    /// Name.
    pub name: String,
    /// Enabled.
    pub enabled: bool,
    /// The link the SDK reports (after a gate).
    pub link: bool,
    /// The ASIC's own link (a cable event sets it).
    pub asic_link: bool,
    /// Autoneg: -1 never set, 0, 1.
    pub autoneg: c_int,
    /// FEC mode: -1 never set, 0 none, 1 RS, 2 FC.
    pub fec: c_int,
    /// Inter-frame gap in bits, -1 never set.
    pub ifg: c_int,
    /// An interface type was set (its name).
    pub interface: Option<String>,
    /// Largest frame, -1 never set.
    pub frame_max: c_int,
    /// Loopback mode, -1 never set.
    pub loopback: c_int,
    /// Untagged VLAN, 1 after the bring-up.
    pub pvid: c_int,
    /// Learn mode, -1 never set.
    pub learn: c_int,
    /// Software link scan (a gate can be registered).
    pub linkscan_sw: bool,
    /// The counters, by [`crate::PortStat`] index.
    pub stats: Vec<u64>,
}

impl FakePort {
    fn new(port: c_int, phys: c_int, lanes: c_int, speed: c_int, name: &str) -> Self {
        Self {
            port,
            phys,
            lanes,
            speed,
            name: name.to_owned(),
            enabled: false,
            link: false,
            asic_link: false,
            autoneg: -1,
            fec: -1,
            ifg: -1,
            interface: None,
            frame_max: -1,
            loopback: -1,
            pvid: 1,
            learn: -1,
            linkscan_sw: true,
            stats: vec![0; crate::PortStat::ALL.len()],
        }
    }
}

/// An L2 entry of the fake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeL2 {
    /// The glue's destination int.
    pub dest: c_int,
    /// Static.
    pub is_static: bool,
    /// Discard.
    pub discard: bool,
}

/// A field entry of the fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeEntry {
    /// Its group.
    pub gid: c_int,
    /// The rule's qualifier bits.
    pub present: u32,
    /// The rule's priority.
    pub priority: c_int,
    /// In-ports.
    pub ports: Vec<c_int>,
    /// Its counter, -1 none.
    pub stat: c_int,
    /// Its policer, -1 none.
    pub policer: c_int,
}

/// Object counts of the L3, field, policer and KNET tables of the fake.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Router interfaces.
    pub intfs: usize,
    /// My-station entries.
    pub stations: usize,
    /// Egress objects (the CPU / drop ones included once created).
    pub egress: usize,
    /// Hosts.
    pub hosts: usize,
    /// Routes.
    pub routes: usize,
    /// ECMP groups.
    pub ecmp: usize,
    /// Field groups.
    pub fp_groups: usize,
    /// Field entries.
    pub fp_entries: usize,
    /// Field counters.
    pub fp_stats: usize,
    /// Policers.
    pub policers: usize,
    /// KNET netifs.
    pub netifs: usize,
    /// KNET filters.
    pub filters: usize,
    /// Trunks.
    pub trunks: usize,
}

#[derive(Debug, Clone)]
struct Fail {
    op: String,
    n: u64,
    seen: u64,
    code: c_int,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_field_names, reason = "an MDIO register's address: phy, dev, reg")]
struct Reg {
    phy: u32,
    dev: u32,
    reg: u32,
}

#[derive(Debug, Clone, Copy)]
struct After {
    n: u64,
    seen: u64,
    at: Reg,
    val: u32,
}

/// A route: VRF, network, mask.
type RouteKey = (c_int, Vec<u8>, Vec<u8>);

/// Everything the fake keeps (one chip).
#[derive(Debug, Default)]
struct State {
    // scenario (MDIO side, as fake_glue.c)
    regs: Vec<(Reg, u32)>,
    consts: Vec<(u32, u32)>,
    dead: Vec<u32>,
    afters: Vec<After>,
    hangs: Vec<(u64, u32, u32)>,
    fail_read: u64,
    fail_write: u64,
    temp_fail: u64,
    sigterm_at: u64,
    reads: u64,
    writes: u64,
    temp_calls: u64,
    temps: Vec<c_int>,
    chip: String,
    init_fail: String,
    trace: Option<File>,
    /// The scenario `attach` uses ([`Chip::load`]).
    preload: String,
    // bring-up state
    attached: bool,
    mdio_ok: bool,
    inited: bool,
    pll_down: bool,
    shutdowns: i32,
    unregisters: i32,
    registers: i32,
    // switch
    ports: Vec<FakePort>,
    cpu: c_int,
    link_admin: bool,
    vlans: BTreeMap<c_int, BTreeMap<c_int, bool>>,
    fails: Vec<Fail>,
    link_cb: bool,
    l2_cb: bool,
    cb_key: usize,
    gates: BTreeSet<c_int>,
    gate_key: usize,
    l2: BTreeMap<([u8; 6], c_int), FakeL2>,
    age: c_int,
    trunks: BTreeMap<c_int, Vec<(c_int, c_int)>>,
    intfs: BTreeMap<c_int, ([u8; 6], c_int, c_int, c_int)>,
    stations: BTreeMap<c_int, [u8; 6]>,
    egress: BTreeMap<c_int, (c_int, [u8; 6], c_int, c_int)>,
    special: [Option<c_int>; 2],
    hosts: BTreeMap<(c_int, Vec<u8>), c_int>,
    routes: BTreeMap<RouteKey, (c_int, bool, bool)>,
    ecmp: BTreeMap<c_int, Vec<c_int>>,
    groups: BTreeMap<c_int, (u32, c_int)>,
    entries: BTreeMap<c_int, FakeEntry>,
    stats: BTreeMap<c_int, (u64, u64)>,
    policers: BTreeMap<c_int, sys::ob_policer_cfg_t>,
    netifs: BTreeMap<c_int, (String, c_int, c_int, [u8; 6], bool)>,
    filters: BTreeMap<c_int, (c_int, c_int, bool)>,
    stale_knet: (c_int, c_int),
    traps: BTreeMap<c_int, bool>,
    shell_log: Vec<String>,
    next_id: c_int,
}

impl State {
    fn tr(&mut self, line: &str) {
        if let Some(f) = self.trace.as_mut() {
            // A trace that cannot be written is the test's problem; the call goes on.
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }

    /// The fresh state of a bring-up: the scenario is read again (`fake_glue.c` `reset()`).
    fn reset(&mut self) {
        let preload = std::mem::take(&mut self.preload);
        let (shutdowns, unregisters, registers) = (0, 0, 0);
        *self = Self {
            preload,
            chip: "FAKE56960_A0".to_owned(),
            shutdowns,
            unregisters,
            registers,
            next_id: 1,
            ..Self::default()
        };
    }

    /// The scripted failure of this call of `op`, or 0 (`fake_glue.c` `sw_fail`).
    fn fail(&mut self, op: &str) -> c_int {
        let mut rv = 0;
        for f in &mut self.fails {
            if f.op == op {
                f.seen += 1;
                if f.seen == f.n {
                    rv = f.code;
                }
            }
        }
        if rv != 0 {
            self.tr(&format!("{op} ERR {rv}"));
        }
        rv
    }

    fn index(&self, port: c_int) -> Option<usize> {
        self.ports.iter().position(|p| p.port == port)
    }

    fn port_mut(&mut self, port: c_int) -> Option<&mut FakePort> {
        self.ports.iter_mut().find(|p| p.port == port)
    }

    /// The checks every port call starts with (`fake_glue.c` `sw_port_call`), as a result.
    fn port_call(&mut self, op: &str, port: c_int) -> Result<usize, c_int> {
        if !self.inited {
            return Err(E_INIT);
        }
        let rv = self.fail(op);
        if rv != 0 {
            return Err(rv);
        }
        self.index(port).ok_or(E_PORT)
    }

    /// The checks every BCM call starts with: "init all" ran, the op's scripted failure.
    fn bcm_call(&mut self, op: &str) -> Result<(), c_int> {
        if !self.inited {
            return Err(E_INIT);
        }
        match self.fail(op) {
            0 => Ok(()),
            rv => Err(rv),
        }
    }

    /// `fake_glue.c` `sw_vlan_call`.
    fn vlan_call(&mut self, op: &str, vid: c_int) -> Result<(), c_int> {
        self.bcm_call(op)?;
        if (1..=4094).contains(&vid) {
            Ok(())
        } else {
            Err(E_PARAM)
        }
    }

    fn id(&mut self) -> c_int {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn valid_dest(&self, dest: c_int) -> bool {
        if dest >= 0 && dest & LAG_BIT != 0 {
            self.trunks.contains_key(&(dest & !LAG_BIT))
        } else {
            self.index(dest).is_some()
        }
    }

    fn is_dead(&self, phy: u32) -> bool {
        self.dead.contains(&phy)
    }

    fn set_reg(&mut self, at: Reg, val: u32) -> bool {
        if let Some(r) = self.regs.iter_mut().find(|(r, _)| *r == at) {
            r.1 = val;
            return true;
        }
        if self.regs.len() >= 8192 {
            return false;
        }
        self.regs.push((at, val));
        true
    }

    fn add_port(&mut self, port: c_int, phys: c_int, lanes: c_int, speed: c_int, name: &str) -> bool {
        if self.ports.len() >= 256 || port < 0 || lanes < 1 || self.index(port).is_some() {
            return false;
        }
        self.ports.push(FakePort::new(port, phys, lanes, speed, name));
        true
    }
}

fn hex(word: &str) -> Option<u32> {
    let w = word
        .strip_prefix("0x")
        .or_else(|| word.strip_prefix("0X"))
        .unwrap_or(word);
    u32::from_str_radix(w, 16).ok()
}

fn dec<T: std::str::FromStr>(word: &str) -> Option<T> {
    word.parse().ok()
}

/// One scenario line into the state; `false` for a line that is not a directive.
#[allow(clippy::too_many_lines, reason = "one arm per directive")]
#[allow(clippy::many_single_char_names, reason = "the scenario's words")]
fn directive(s: &mut State, line: &str) -> bool {
    let w: Vec<&str> = line.split_whitespace().collect();
    let arg = |i: usize| w.get(i).copied().unwrap_or("");
    let n = w.len();
    match (arg(0), n) {
        ("name", 2) => {
            s.chip = arg(1).chars().take(31).collect();
            true
        }
        ("init", 2) => {
            s.init_fail = arg(1).chars().take(15).collect();
            true
        }
        ("reg", 5) => match (hex(arg(1)), hex(arg(2)), hex(arg(3)), hex(arg(4))) {
            (Some(phy), Some(dev), Some(reg), Some(val)) => s.set_reg(Reg { phy, dev, reg }, val),
            _ => false,
        },
        ("const", 3) => match (hex(arg(1)), hex(arg(2))) {
            (Some(phy), Some(val)) => {
                s.consts.push((phy, val));
                true
            }
            _ => false,
        },
        ("dead", 2) => hex(arg(1)).map(|phy| s.dead.push(phy)).is_some(),
        ("after", 6) => match (dec(arg(1)), hex(arg(2)), hex(arg(3)), hex(arg(4)), hex(arg(5))) {
            (Some(n), Some(phy), Some(dev), Some(reg), Some(val)) => {
                s.afters.push(After {
                    n,
                    seen: 0,
                    at: Reg { phy, dev, reg },
                    val,
                });
                true
            }
            _ => false,
        },
        ("hang", 4) => match (dec(arg(1)), hex(arg(2)), hex(arg(3))) {
            (Some(n), Some(phy), Some(val)) => {
                s.hangs.push((n, phy, val));
                true
            }
            _ => false,
        },
        ("fail-read", 2) => dec(arg(1)).map(|v| s.fail_read = v).is_some(),
        ("fail-write", 2) => dec(arg(1)).map(|v| s.fail_write = v).is_some(),
        ("temp-fail", 2) => dec(arg(1)).map(|v| s.temp_fail = v).is_some(),
        ("sigterm", 2) => dec(arg(1)).map(|v| s.sigterm_at = v).is_some(),
        ("trace", 2) => match std::fs::OpenOptions::new().append(true).create(true).open(arg(1)) {
            Ok(f) => {
                s.trace = Some(f);
                true
            }
            Err(_) => false,
        },
        ("temp", k) if k >= 2 => {
            let temps: Option<Vec<c_int>> = w.iter().skip(1).take(MAX_MON).map(|t| dec(t)).collect();
            match temps {
                Some(t) if k - 1 <= MAX_MON => {
                    s.temps = t;
                    true
                }
                _ => false,
            }
        }
        ("ports", 4) => match (dec::<c_int>(arg(1)), dec::<c_int>(arg(2)), dec::<c_int>(arg(3))) {
            (Some(count), Some(lanes), Some(speed)) if count > 0 => (0..count).all(|i| {
                let name = format!("ce{i}");
                s.add_port(i + 1, 1 + i * lanes, lanes, speed, &name)
            }),
            _ => false,
        },
        ("port", 6) => match (dec(arg(1)), dec(arg(2)), dec(arg(3)), dec(arg(4))) {
            (Some(p), Some(phys), Some(lanes), Some(speed)) if arg(5).len() <= 15 => {
                s.add_port(p, phys, lanes, speed, arg(5))
            }
            _ => false,
        },
        ("cpu", 2) => dec(arg(1)).map(|c| s.cpu = c).is_some(),
        ("link", 2) => match arg(1) {
            "admin" => {
                s.link_admin = true;
                true
            }
            "none" => {
                s.link_admin = false;
                true
            }
            _ => false,
        },
        ("fail", 4) => match (dec::<u64>(arg(2)), dec::<c_int>(arg(3))) {
            (Some(n), Some(code)) if n > 0 && code < 0 && OPS.contains(&arg(1)) && s.fails.len() < 64 => {
                s.fails.push(Fail {
                    op: arg(1).to_owned(),
                    n,
                    seen: 0,
                    code,
                });
                true
            }
            _ => false,
        },
        ("knet", 3) => match (dec(arg(1)), dec(arg(2))) {
            (Some(a), Some(b)) => {
                s.stale_knet = (a, b);
                true
            }
            _ => false,
        },
        ("linkscan", 3) => match (dec(arg(1)), arg(2)) {
            (Some(p), mode @ ("sw" | "hw")) => s.port_mut(p).map(|port| port.linkscan_sw = mode == "sw").is_some(),
            _ => false,
        },
        _ => false,
    }
}

/// Reads a scenario: `Err(line)` for the first line that is not a directive.
fn load(s: &mut State, text: &str) -> Result<(), usize> {
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if !directive(s, t) {
            return Err(i + 1);
        }
    }
    Ok(())
}

/// Writes `text` into the glue's message buffer (snprintf: cut to fit, NUL-terminated).
fn message(err: &mut [u8], text: &str) {
    let Some(room) = err.len().checked_sub(1) else {
        return;
    };
    let n = text.len().min(room);
    if let (Some(dst), Some(src)) = (err.get_mut(..n), text.as_bytes().get(..n)) {
        dst.copy_from_slice(src);
    }
    if let Some(nul) = err.get_mut(n) {
        *nul = 0;
    }
}

/// The errno of a failed read, as the real glue's message prints it.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

/// A fake chip: clone it to share it (it is one chip), [`Chip::new`] for another.
#[derive(Debug, Clone, Default)]
pub struct Chip {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    open: AtomicBool,
    state: Mutex<State>,
    /// Held while a callback runs and by register / unregister, as the SDK's link scan does:
    /// after an unregister returns, no call of this chip is in progress.
    cb: Mutex<()>,
    /// The call log while recording ([`Chip::record`]).
    record: Mutex<Option<crate::record::Recorder>>,
}

/// The process's fake: what the plain bring-ups use in a build with only this fake.
pub(crate) fn process_chip() -> &'static Chip {
    static PROCESS: OnceLock<Chip> = OnceLock::new();
    PROCESS.get_or_init(Chip::new)
}

impl Chip {
    /// A new chip, nothing brought up, empty scenario.
    pub fn new() -> Self {
        let chip = Self::default();
        chip.state().reset();
        chip
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn callbacks(&self) -> MutexGuard<'_, ()> {
        self.inner.cb.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn open_flag(&self) -> &AtomicBool {
        &self.inner.open
    }

    fn recorder(&self) -> MutexGuard<'_, Option<crate::record::Recorder>> {
        self.inner.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts (`true`, with an empty log) or stops recording every glue call made on this chip,
    /// every callback it fires and every [`Chip::inject`]ion, one canonical line each (the format:
    /// [`crate::record`]; what sai-backend-openbcm's glue differential compares with the C++ backend
    /// over a recording stub of `sdk_glue.h`).
    pub fn record(&self, on: bool) {
        *self.recorder() = on.then(crate::record::Recorder::default);
    }

    /// The lines recorded since the last call (or since recording started), in order; empty when
    /// not recording.
    pub fn take_record(&self) -> Vec<String> {
        self.recorder()
            .as_mut()
            .map(crate::record::Recorder::take)
            .unwrap_or_default()
    }

    /// Runs a test injection (`f`: calls of this chip's hooks, a cable event, ...) as one recorded
    /// step: while recording, `line` (e.g. `inject link(port=1, up=0)`) is logged after what
    /// happened inside, like a call.
    pub fn inject<T>(&self, line: &str, f: impl FnOnce() -> T) -> T {
        let Some(scope) = self.rec_enter() else {
            return f();
        };
        let out = f();
        self.rec_exit(scope, line);
        out
    }

    /// A recorded step starts (`None`: not recording).
    pub(crate) fn rec_enter(&self) -> Option<crate::record::Scope> {
        self.recorder().as_mut().map(crate::record::Recorder::enter)
    }

    /// The step of `scope` ended: its line.
    pub(crate) fn rec_exit(&self, scope: crate::record::Scope, line: &str) {
        if let Some(r) = self.recorder().as_mut() {
            r.exit(scope, line);
        }
    }

    /// A callback into the glue's caller, recorded as `line` (with ` -> <answer>` when `answer`
    /// gives one).
    fn callback<T>(&self, line: &str, call: impl FnOnce() -> T, answer: impl FnOnce(&T) -> Option<String>) -> T {
        let Some(scope) = self.rec_enter() else {
            return call();
        };
        let out = call();
        let text = answer(&out).map_or_else(|| line.to_owned(), |a| format!("{line} -> {a}"));
        self.rec_exit(scope, &text);
        out
    }

    fn fire_link(&self, key: usize, port: c_int, up: c_int) {
        self.callback(
            &format!("cb link(port={port}, up={up})"),
            || raw::link(key, port, up),
            |()| None,
        );
    }

    fn fire_gate(&self, key: usize, port: c_int) -> c_int {
        self.callback(
            &format!("cb gate(port={port})"),
            || raw::gate(key, port),
            |a| Some(a.to_string()),
        )
    }

    /// The scenario [`crate::Attached::attach_on`] uses (attach takes no config file). `Err(line)`
    /// for a line that is not a directive (nothing kept).
    pub fn load(&self, scenario: &str) -> Result<(), usize> {
        let mut probe = State::default();
        load(&mut probe, scenario)?;
        scenario.clone_into(&mut self.state().preload);
        Ok(())
    }

    /// Scripts the `n`-th (from now, 1-based) call of `op` ([`OPS`]) to return `code`. `false` for
    /// an unknown op, `n` = 0 or a code that is not negative.
    pub fn fail(&self, op: &str, n: u64, code: i32) -> bool {
        if !OPS.contains(&op) || n == 0 || code >= 0 {
            return false;
        }
        let mut s = self.state();
        let seen = 0;
        s.fails.push(Fail {
            op: op.to_owned(),
            n,
            seen,
            code,
        });
        true
    }

    // ---- bring-ups

    fn begin(s: &mut State, config: Option<&CStr>, rc: Option<&CStr>, err: &mut [u8]) -> Result<(), c_int> {
        s.reset();
        let text = match config {
            None => s.preload.clone(),
            Some(path) => {
                let name = path.to_string_lossy().into_owned();
                let read = if name.is_empty() {
                    Err(std::io::Error::from_raw_os_error(2))
                } else {
                    std::fs::read_to_string(&name)
                };
                match read {
                    Ok(t) => t,
                    Err(e) => {
                        message(
                            err,
                            &format!("SDK config file '{name}' missing or unreadable ({})", errno_of(&e)),
                        );
                        return Err(E_CONFIG);
                    }
                }
            }
        };
        // As the real glue: the rc file is checked before anything is booted.
        let rc_name = rc.map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        if !rc_name.is_empty() {
            if let Err(e) = std::fs::metadata(&rc_name) {
                message(
                    err,
                    &format!("rc file '{rc_name}' missing or unreadable ({})", errno_of(&e)),
                );
                return Err(E_CONFIG);
            }
        }
        if let Err(line) = load(s, &text) {
            let name = config.map_or_else(|| "(scenario)".to_owned(), |c| c.to_string_lossy().into_owned());
            message(err, &format!("fake glue: {name}:{line}: not a scenario line"));
            return Err(E_CONFIG);
        }
        Ok(())
    }

    fn refuse(s: &State, unit: c_int, err: &mut [u8], mdio: bool) -> Result<(), c_int> {
        if s.init_fail == "busy" {
            message(
                err,
                &format!(
                    "another SDK process owns the switch (lock /run/openbcm-unit{unit}.lock held; e.g. systemctl stop openbcm-asic-temp)"
                ),
            );
            return Err(E_BUSY);
        }
        if unit != 0 || s.init_fail == "unit" {
            message(err, "no attached switch unit (soc_ndev 1)");
            return Err(E_UNIT);
        }
        let known: &[&str] = if mdio {
            &["soc", "misc", "delay", "divisor"]
        } else {
            &["all", "rc"]
        };
        if !s.init_fail.is_empty() && !known.contains(&s.init_fail.as_str()) {
            message(err, &format!("fake glue: unknown init failure '{}'", s.init_fail));
            return Err(E_CONFIG);
        }
        Ok(())
    }

    pub(crate) fn init(&self, unit: c_int, config: &CStr, rc: &CStr, err: &mut [u8]) -> c_int {
        let mut s = self.state();
        if let Err(rv) = Self::begin(&mut s, Some(config), Some(rc), err) {
            return rv;
        }
        let rc_name = rc.to_string_lossy().into_owned();
        let rc_shown = if rc_name.is_empty() { "-" } else { rc_name.as_str() };
        s.tr(&format!("init-all unit {unit} rc {rc_shown}"));
        if let Err(rv) = Self::refuse(&s, unit, err, false) {
            return rv;
        }
        if s.init_fail == "all" {
            message(err, &format!("init all failed: {E_FAIL}"));
            return E_FAIL;
        }
        if s.init_fail == "rc" {
            message(err, &format!("rc file '{rc_name}' failed (-1)"));
            return E_FAIL;
        }
        let members: BTreeMap<c_int, bool> = s.ports.iter().map(|p| (p.port, true)).collect();
        s.vlans.insert(1, members);
        for p in &mut s.ports {
            p.enabled = true;
        }
        let (nn, nf) = std::mem::take(&mut s.stale_knet);
        s.attached = true;
        s.mdio_ok = true;
        s.inited = true;
        let n = s.ports.len();
        message(
            err,
            &format!("ok: stale KNET netifs {nn} filters {nf} destroyed; IPv4/IPv6 L3 on {n} ports"),
        );
        NONE
    }

    pub(crate) fn attach(&self, unit: c_int, err: &mut [u8]) -> c_int {
        let mut s = self.state();
        if let Err(rv) = Self::begin(&mut s, None, None, err) {
            return rv;
        }
        s.tr(&format!("attach unit {unit}"));
        if let Err(rv) = Self::refuse(&s, unit, err, true) {
            return rv;
        }
        s.attached = true;
        message(err, &format!("attached {}", s.chip));
        NONE
    }

    pub(crate) fn init_mdio(&self, unit: c_int, config: &CStr, err: &mut [u8]) -> c_int {
        let mut s = self.state();
        if let Err(rv) = Self::begin(&mut s, Some(config), None, err) {
            return rv;
        }
        s.tr(&format!("init unit {unit}"));
        if let Err(rv) = Self::refuse(&s, unit, err, true) {
            return rv;
        }
        match s.init_fail.as_str() {
            "soc" | "misc" => {
                message(err, &format!("init {} failed (-1)", s.init_fail));
                return E_FAIL;
            }
            "delay" => {
                message(
                    err,
                    "MDIO hold delay on the chip is 2, the config's mdio_output_delay is 13: refusing MDIO access",
                );
                return E_MDIO_CLOCK;
            }
            "divisor" => {
                message(
                    err,
                    "external MDIO clock divisor on the chip is 1, the config's rate_ext_mdio_divisor is 2: refusing MDIO access",
                );
                return E_MDIO_CLOCK;
            }
            _ => {}
        }
        s.attached = true;
        s.mdio_ok = true;
        message(
            err,
            &format!(
                "{}: init soc + init misc done; external MDIO rate register 0x{:08x}, hold delay {}{}",
                s.chip, 0x000d_0001u32, 13, " (as configured)"
            ),
        );
        NONE
    }

    // ---- temperatures, MDIO

    pub(crate) fn temperatures(&self, unit: c_int, cur: &mut [c_int], peak: &mut [c_int], count: &mut c_int) -> c_int {
        let mut s = self.state();
        *count = 0;
        let max = cur.len().min(peak.len()).min(MAX_MON);
        if max == 0 {
            return E_PARAM;
        }
        if !s.attached || unit != 0 {
            return E_UNIT;
        }
        s.temp_calls += 1;
        if s.pll_down || (s.temp_fail != 0 && s.temp_calls >= s.temp_fail) {
            s.tr("T ERR");
            return E_TIMEOUT;
        }
        if s.temps.is_empty() {
            return E_UNAVAIL;
        }
        let mut i = 0;
        for ((t, c), p) in s.temps.iter().zip(cur.iter_mut()).zip(peak.iter_mut()).take(max) {
            *c = *t;
            *p = t.saturating_add(15);
            i += 1;
        }
        *count = c_int::try_from(i).unwrap_or(0);
        s.tr(&format!("T {i}"));
        NONE
    }

    pub(crate) fn th_overtemp_shutdown(&self, unit: c_int) -> c_int {
        let mut s = self.state();
        if !s.attached || unit != 0 {
            return E_UNIT;
        }
        if s.inited {
            return E_BUSY;
        }
        match s.fail("overtemp") {
            0 => {}
            rv => return rv,
        }
        s.tr("overtemp shutdown");
        s.pll_down = true;
        NONE
    }

    /// The glue's own check before an MDIO access (`sdk_glue.c` `OB_CHECK_MDIO`), then the fake's.
    fn mdio_check(s: &State, unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint) -> Result<(), c_int> {
        if !s.mdio_ok {
            return Err(E_INIT);
        }
        if unit != 0 {
            return Err(E_UNIT);
        }
        if phy > 0x7ff || !(1..=31).contains(&dev) || reg > 0xffff {
            return Err(E_PARAM);
        }
        Ok(())
    }

    pub(crate) fn mdio_read(&self, unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: &mut c_uint) -> c_int {
        let mut s = self.state();
        if let Err(rv) = Self::mdio_check(&s, unit, phy, dev, reg) {
            return rv;
        }
        s.reads += 1;
        let dead = s.is_dead(phy);
        if (s.fail_read != 0 && s.reads == s.fail_read) || dead {
            s.tr(&format!("R {phy:x} {dev:x} {reg:x} ERR"));
            *val = 0;
            return if dead { E_TIMEOUT } else { E_FAIL };
        }
        let at = Reg { phy, dev, reg };
        let mut v = s.regs.iter().find(|(r, _)| *r == at).map_or(0, |(_, v)| *v);
        for a in &mut s.afters {
            if a.at == at {
                let seen = a.seen;
                a.seen += 1;
                if seen >= a.n {
                    v = a.val;
                }
            }
        }
        for (p, c) in &s.consts {
            if *p == phy {
                v = *c;
            }
        }
        for (n, p, h) in &s.hangs {
            if *p == phy && s.writes >= *n {
                v = *h;
            }
        }
        *val = v & 0xffff;
        let shown = *val;
        s.tr(&format!("R {phy:x} {dev:x} {reg:x} = {shown:04x}"));
        NONE
    }

    pub(crate) fn mdio_write(&self, unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: c_uint) -> c_int {
        let raise = {
            let mut s = self.state();
            if let Err(rv) = Self::mdio_check(&s, unit, phy, dev, reg) {
                return rv;
            }
            if val > 0xffff {
                return E_PARAM;
            }
            s.writes += 1;
            let dead = s.is_dead(phy);
            if (s.fail_write != 0 && s.writes == s.fail_write) || dead || !s.set_reg(Reg { phy, dev, reg }, val) {
                s.tr(&format!("W {phy:x} {dev:x} {reg:x} {val:x} ERR"));
                return if dead { E_TIMEOUT } else { E_FAIL };
            }
            s.tr(&format!("W {phy:x} {dev:x} {reg:x} {val:x}"));
            s.sigterm_at != 0 && s.writes == s.sigterm_at
        };
        if raise {
            let _ = signal_hook::low_level::raise(signal_hook::consts::SIGTERM);
        }
        NONE
    }

    // ---- callbacks

    pub(crate) fn register_callbacks(&self, unit: c_int, link: bool, l2: bool, key: usize) -> c_int {
        let _held = self.callbacks();
        let mut s = self.state();
        if !s.inited {
            return E_INIT;
        }
        s.registers += 1;
        s.tr(&format!("register unit {unit}"));
        // As the real glue: the pointers are stored first, the SDK is asked second, and a
        // refusal leaves them stored.
        s.link_cb = link;
        s.l2_cb = l2;
        s.cb_key = key;
        s.fail("register")
    }

    pub(crate) fn unregister_callbacks(&self, unit: c_int) {
        let _held = self.callbacks();
        let mut s = self.state();
        s.unregisters += 1;
        s.tr(&format!("unregister unit {unit}"));
        s.link_cb = false;
        s.l2_cb = false;
    }

    pub(crate) fn shutdown(&self, unit: c_int) {
        let inited = self.state().inited;
        if inited {
            self.unregister_callbacks(unit);
            let mut s = self.state();
            s.shutdowns += 1;
            s.inited = false;
        }
        self.state().tr(&format!("shutdown unit {unit}"));
    }

    pub(crate) fn linkscan_port_gate(&self, unit: c_int, port: c_int, on: bool, key: usize) -> c_int {
        let _held = self.callbacks();
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("gate") {
            return rv;
        }
        let Some(i) = s.index(port) else { return E_PORT };
        if !on {
            s.gates.remove(&port);
            s.tr(&format!("gate {port} off"));
            return NONE;
        }
        if !s.ports.get(i).is_some_and(|p| p.linkscan_sw) {
            return E_CONFIG;
        }
        s.gate_key = key;
        s.gates.insert(port);
        s.tr(&format!("gate {port} on"));
        NONE
    }

    // ---- switch-wide

    pub(crate) fn shell(&self, unit: c_int, cmd: &CStr) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("shell") {
            return rv;
        }
        let cmd = cmd.to_string_lossy().into_owned();
        s.tr(&format!("shell {cmd}"));
        s.shell_log.push(cmd);
        NONE
    }

    /// The diag shell with its output to `fd`: `phy raw c45 <phy> <dev> <reg>` (hex words) reads
    /// the register as an MDIO read does and prints it as the SDK shell does (`\t0x<reg>:
    /// 0x<value>`, what `mdio_report.c` and the smoke program parse); `fake linkscan` runs one
    /// pass of [`Chip::linkscan`] (the fake has no link scan thread: the smoke program's
    /// `--poke-diag`); anything else prints `fake: <cmd>`, as the virtual backend.
    pub(crate) fn shell_to(&self, unit: c_int, cmd: &CStr, fd: BorrowedFd<'_>) -> c_int {
        let rv = self.shell(unit, cmd);
        if rv != NONE {
            return rv;
        }
        let text = cmd.to_string_lossy();
        let words: Vec<&str> = text.split_whitespace().collect();
        let answer = match words.as_slice() {
            ["phy", "raw", "c45", phy, dev, reg] => match (hex(phy), hex(dev), hex(reg)) {
                (Some(phy), Some(dev), Some(reg)) => {
                    let mut val: c_uint = 0;
                    match self.mdio_read(unit, phy, dev, reg, &mut val) {
                        NONE => format!("\t0x{reg:x}: 0x{val:04x}"),
                        rv => format!("fake: {text}: error {rv}"),
                    }
                }
                _ => format!("fake: {text}"),
            },
            ["fake", "linkscan"] => format!("fake: linkscan: {} link changes", self.linkscan()),
            _ => format!("fake: {text}"),
        };
        let written = fd
            .try_clone_to_owned()
            .map(File::from)
            .and_then(|mut f| writeln!(f, "{answer}"));
        if written.is_ok() { NONE } else { E_INTERNAL }
    }

    pub(crate) fn hw_info(&self, unit: c_int, buf: &mut [u8]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        let rv = s.fail("hw-info");
        let chip = s.chip.clone();
        message(buf, if rv != 0 { "" } else { &chip });
        if rv != 0 { rv } else { NONE }
    }

    pub(crate) fn knet_cleanup(&self, unit: c_int, netifs: &mut c_int, filters: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        *netifs = 0;
        *filters = 0;
        if let Err(rv) = s.bcm_call("knet-cleanup") {
            return rv;
        }
        // Everything this chip has, as left by "an earlier process", and the stale scenario ones.
        let (nn, nf) = std::mem::take(&mut s.stale_knet);
        *netifs = nn.saturating_add(c_int::try_from(s.netifs.len()).unwrap_or(c_int::MAX));
        *filters = nf.saturating_add(c_int::try_from(s.filters.len()).unwrap_or(c_int::MAX));
        s.netifs.clear();
        s.filters.clear();
        NONE
    }

    pub(crate) fn table_usage(&self, unit: c_int, u: &mut sys::ob_table_usage_t) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("table-usage") {
            return rv;
        }
        let left = |max: u32, used: usize| max.saturating_sub(u32::try_from(used).unwrap_or(u32::MAX));
        let v4 = s.routes.keys().filter(|(_, a, _)| a.len() == 4).count();
        let v6 = s.routes.len() - v4;
        let host_units: usize = s.hosts.keys().map(|(_, a)| if a.len() == 16 { 2 } else { 1 }).sum();
        let ecmp_members: usize = s.ecmp.values().map(Vec::len).sum();
        *u = sys::ob_table_usage_t {
            v4_routes_free: left(16384, v4),
            v6_routes_free: left(8192, v6),
            hosts_free: left(8192, host_units),
            egress_free: left(32768, s.egress.len()),
            fdb_free: left(32768, s.l2.len()),
            ecmp_groups_free: left(1024, s.ecmp.len()),
            ecmp_members_free: left(16384, ecmp_members),
        };
        NONE
    }

    pub(crate) fn chip_limits(&self, unit: c_int, l: &mut sys::ob_chip_limits_t) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("chip-limits") {
            return rv;
        }
        *l = sys::ob_chip_limits_t {
            buffer_bytes: 16_777_216,
            acl_prio_min: 0,
            acl_prio_max: 0x7fff_ffff,
            max_host: 8192,
            max_route: 16384,
            max_nexthop: 32768,
            max_l2: 32768,
            ecmp_groups: 1024,
            ecmp_paths: 64,
            ecmp_members: 16384,
            vrf_max: 1023,
        };
        NONE
    }

    pub(crate) fn cpu_port(&self, unit: c_int) -> c_int {
        let s = self.state();
        if !s.attached || unit != 0 { E_UNIT } else { s.cpu }
    }

    pub(crate) fn ports(&self, unit: c_int, out: &mut [sys::ob_port_info_t]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        let rv = s.fail("ports");
        if !s.inited {
            return E_INIT;
        }
        if rv != 0 {
            return rv;
        }
        let mut n = 0;
        for (slot, p) in out.iter_mut().zip(&s.ports) {
            let mut name = [0 as std::ffi::c_char; 16];
            for (d, b) in name.iter_mut().zip(p.name.bytes().take(15)) {
                *d = std::ffi::c_char::from_ne_bytes(b.to_ne_bytes());
            }
            *slot = sys::ob_port_info_t {
                port: p.port,
                phys: p.phys,
                lanes: p.lanes,
                speed: p.speed,
                name,
            };
            n += 1;
        }
        c_int::try_from(n).unwrap_or(c_int::MAX)
    }

    // ---- ports

    /// Sets a port's reported link; on a change the link callback runs, on this thread, with
    /// the state released. True if it ran.
    fn set_link(&self, port: c_int, up: bool, asic: bool) -> bool {
        let _held = self.callbacks();
        let key = {
            let mut s = self.state();
            let gated = s.gates.contains(&port);
            let registered = s.link_cb;
            let key = s.cb_key;
            let Some(p) = s.port_mut(port) else { return false };
            if asic {
                p.asic_link = up;
                if gated {
                    return false; // the next link scan pass asks the gate
                }
            }
            if p.link == up {
                return false;
            }
            p.link = up;
            if !registered {
                return false;
            }
            key
        };
        self.fire_link(key, port, c_int::from(up));
        true
    }

    pub(crate) fn port_enable_set(&self, unit: c_int, port: c_int, enable: c_int) -> c_int {
        let link = {
            let mut s = self.state();
            let _ = unit;
            let i = match s.port_call("enable", port) {
                Ok(i) => i,
                Err(rv) => return rv,
            };
            let on = enable != 0;
            s.tr(&format!("port {port} enable {}", c_int::from(on)));
            if let Some(p) = s.ports.get_mut(i) {
                p.enabled = on;
            }
            (!on || s.link_admin).then_some(on)
        };
        if let Some(up) = link {
            self.set_link(port, up, true);
        }
        NONE
    }

    pub(crate) fn port_link_get(&self, unit: c_int, port: c_int, up: &mut c_int) -> c_int {
        let _held = self.callbacks();
        let mut s = self.state();
        let _ = unit;
        *up = 0;
        match s.port_call("link-get", port) {
            Ok(i) => {
                *up = c_int::from(s.ports.get(i).is_some_and(|p| p.link));
                NONE
            }
            Err(rv) => rv,
        }
    }

    fn speeds_of(lanes: c_int) -> &'static [u32] {
        if lanes >= 4 {
            &[40000, 100_000]
        } else if lanes >= 2 {
            &[20000, 50000]
        } else {
            &[1000, 10000, 25000]
        }
    }

    pub(crate) fn port_speed_set(&self, unit: c_int, port: c_int, mbps: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        let i = match s.port_call("speed-set", port) {
            Ok(i) => i,
            Err(rv) => return rv,
        };
        let lanes = s.ports.get(i).map_or(1, |p| p.lanes);
        if !Self::speeds_of(lanes)
            .iter()
            .any(|&v| c_int::try_from(v).is_ok_and(|v| v == mbps))
        {
            return E_PARAM;
        }
        s.tr(&format!("port {port} speed {mbps}"));
        if let Some(p) = s.ports.get_mut(i) {
            p.speed = mbps;
        }
        NONE
    }

    pub(crate) fn port_speed_get(&self, unit: c_int, port: c_int, mbps: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        match s.port_call("speed-get", port) {
            Ok(i) => {
                *mbps = s.ports.get(i).map_or(0, |p| p.speed);
                NONE
            }
            Err(rv) => rv,
        }
    }

    pub(crate) fn port_speeds(&self, unit: c_int, port: c_int, out: &mut [u32]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        match s.port_call("speeds", port) {
            Ok(i) => {
                let lanes = s.ports.get(i).map_or(1, |p| p.lanes);
                let mut n = 0;
                for (o, v) in out.iter_mut().zip(Self::speeds_of(lanes)) {
                    *o = *v;
                    n += 1;
                }
                n
            }
            Err(rv) => rv,
        }
    }

    /// A port setting: the port checks, the scripted failure, the value's range, then `apply`.
    fn port_set(&self, op: &str, port: c_int, ok: bool, trace: &str, apply: impl FnOnce(&mut FakePort)) -> c_int {
        let mut s = self.state();
        let i = match s.port_call(op, port) {
            Ok(i) => i,
            Err(rv) => return rv,
        };
        if !ok {
            return E_PARAM;
        }
        s.tr(trace);
        if let Some(p) = s.ports.get_mut(i) {
            apply(p);
        }
        NONE
    }

    pub(crate) fn port_frame_max_set(&self, unit: c_int, port: c_int, bytes: c_int) -> c_int {
        let _ = unit;
        self.port_set(
            "frame-max",
            port,
            bytes > 0,
            &format!("port {port} frame-max {bytes}"),
            |p| p.frame_max = bytes,
        )
    }

    pub(crate) fn port_fec_set(&self, unit: c_int, port: c_int, mode: c_int) -> c_int {
        let _ = unit;
        self.port_set(
            "fec",
            port,
            (0..=2).contains(&mode),
            &format!("port {port} fec {mode}"),
            |p| p.fec = mode,
        )
    }

    pub(crate) fn port_autoneg_set(&self, unit: c_int, port: c_int, on: c_int) -> c_int {
        let _ = unit;
        let on = c_int::from(on != 0);
        self.port_set("autoneg", port, true, &format!("port {port} autoneg {on}"), |p| {
            p.autoneg = on;
        })
    }

    pub(crate) fn port_autoneg_get(&self, unit: c_int, port: c_int, on: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        match s.port_call("autoneg-get", port) {
            Ok(i) => {
                *on = c_int::from(s.ports.get(i).is_some_and(|p| p.autoneg == 1));
                NONE
            }
            Err(rv) => rv,
        }
    }

    pub(crate) fn port_interface_set(&self, unit: c_int, port: c_int, name: &CStr) -> c_int {
        let _ = unit;
        let name = name.to_string_lossy().into_owned();
        let known = crate::port::INTERFACE_NAMES.contains(&name.as_str());
        let trace = format!("port {port} interface {name}");
        self.port_set("interface", port, known, &trace, |p| p.interface = Some(name))
    }

    pub(crate) fn port_ifg_set(&self, unit: c_int, port: c_int, bits: c_int) -> c_int {
        let _ = unit;
        self.port_set("ifg", port, bits >= 0, &format!("port {port} ifg {bits}"), |p| {
            p.ifg = bits;
        })
    }

    pub(crate) fn port_loopback_set(&self, unit: c_int, port: c_int, mode: c_int) -> c_int {
        let _ = unit;
        self.port_set(
            "loopback",
            port,
            (0..=2).contains(&mode),
            &format!("port {port} loopback {mode}"),
            |p| {
                p.loopback = mode;
            },
        )
    }

    pub(crate) fn port_pvid_set(&self, unit: c_int, port: c_int, vid: c_int) -> c_int {
        let _ = unit;
        self.port_set(
            "pvid",
            port,
            (1..=4094).contains(&vid),
            &format!("port {port} pvid {vid}"),
            |p| p.pvid = vid,
        )
    }

    pub(crate) fn port_learn_set(&self, unit: c_int, port: c_int, mode: c_int) -> c_int {
        let _ = unit;
        self.port_set(
            "learn",
            port,
            (0..=3).contains(&mode),
            &format!("port {port} learn {mode}"),
            |p| p.learn = mode,
        )
    }

    pub(crate) fn port_stat_get(&self, unit: c_int, port: c_int, which: c_int, v: &mut u64) -> c_int {
        let mut s = self.state();
        let _ = unit;
        let Some(stat) = crate::PortStat::from_raw(which) else {
            return E_PARAM;
        };
        match s.port_call("stat-get", port) {
            Ok(i) => {
                *v = s
                    .ports
                    .get(i)
                    .and_then(|p| p.stats.get(usize::try_from(stat.raw()).unwrap_or(usize::MAX)))
                    .copied()
                    .unwrap_or(0);
                NONE
            }
            Err(rv) => rv,
        }
    }

    pub(crate) fn port_stat_clear(&self, unit: c_int, port: c_int) -> c_int {
        let _ = unit;
        self.port_set("stat-clear", port, true, &format!("port {port} stat-clear"), |p| {
            p.stats.iter_mut().for_each(|c| *c = 0);
        })
    }

    // ---- VLANs

    pub(crate) fn vlan_create(&self, unit: c_int, vid: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("vlan-create", vid) {
            return rv;
        }
        if s.vlans.contains_key(&vid) {
            return E_EXISTS;
        }
        s.tr(&format!("vlan {vid} create"));
        s.vlans.insert(vid, BTreeMap::new());
        NONE
    }

    pub(crate) fn vlan_destroy(&self, unit: c_int, vid: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("vlan-destroy", vid) {
            return rv;
        }
        if !s.vlans.contains_key(&vid) {
            return E_NOT_FOUND;
        }
        if vid == 1 {
            return E_BADID; // the SDK refuses to destroy its default VLAN
        }
        s.tr(&format!("vlan {vid} destroy"));
        s.vlans.remove(&vid);
        NONE
    }

    pub(crate) fn vlan_port_add(&self, unit: c_int, vid: c_int, port: c_int, untagged: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("vlan-add", vid) {
            return rv;
        }
        if !s.vlans.contains_key(&vid) {
            return E_NOT_FOUND;
        }
        if s.index(port).is_none() {
            return E_PORT;
        }
        let untagged = untagged != 0;
        s.tr(&format!(
            "vlan {vid} add {port} {}",
            if untagged { "untagged" } else { "tagged" }
        ));
        if let Some(m) = s.vlans.get_mut(&vid) {
            m.insert(port, untagged);
        }
        NONE
    }

    pub(crate) fn vlan_port_remove(&self, unit: c_int, vid: c_int, port: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("vlan-remove", vid) {
            return rv;
        }
        if !s.vlans.contains_key(&vid) {
            return E_NOT_FOUND;
        }
        if s.index(port).is_none() {
            return E_PORT;
        }
        s.tr(&format!("vlan {vid} remove {port}"));
        // removing a non-member is no error (a port bitmap operation)
        if let Some(m) = s.vlans.get_mut(&vid) {
            m.remove(&port);
        }
        NONE
    }

    pub(crate) fn vlan_members(&self, unit: c_int, vid: c_int, ports: &mut [c_int], untagged: &mut [c_int]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("vlan-members", vid) {
            return rv;
        }
        let Some(members) = s.vlans.get(&vid) else {
            return E_NOT_FOUND;
        };
        // In port order (the SDK iterates its bitmap).
        let order: Vec<(c_int, bool)> = s
            .ports
            .iter()
            .filter_map(|p| members.get(&p.port).map(|u| (p.port, *u)))
            .collect();
        let mut n = 0;
        for ((p, u), (op, ou)) in order.iter().zip(ports.iter_mut().zip(untagged.iter_mut())) {
            *op = *p;
            *ou = c_int::from(*u);
            n += 1;
        }
        n
    }

    // ---- L2

    pub(crate) fn l2_add(
        &self,
        unit: c_int,
        mac: &[u8; 6],
        vid: c_int,
        port: c_int,
        is_static: c_int,
        discard: c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("l2-add") {
            return rv;
        }
        if !(1..=4094).contains(&vid) {
            return E_PARAM;
        }
        if !s.valid_dest(port) {
            return E_PORT;
        }
        s.tr(&format!("l2 add {} {vid} {port:#x}", mac_text(mac)));
        s.l2.insert(
            (*mac, vid),
            FakeL2 {
                dest: port,
                is_static: is_static != 0,
                discard: discard != 0,
            },
        );
        NONE
    }

    pub(crate) fn l2_delete(&self, unit: c_int, mac: &[u8; 6], vid: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("l2-delete") {
            return rv;
        }
        s.tr(&format!("l2 delete {} {vid}", mac_text(mac)));
        if s.l2.remove(&(*mac, vid)).is_some() {
            NONE
        } else {
            E_NOT_FOUND
        }
    }

    pub(crate) fn l2_flush(&self, unit: c_int, port: c_int, vid: c_int, statics: c_int, dynamics: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("l2-flush") {
            return rv;
        }
        if statics != 0 && dynamics == 0 {
            return E_UNAVAIL;
        }
        if statics == 0 && dynamics == 0 {
            return NONE;
        }
        s.tr(&format!(
            "l2 flush {port:#x} {vid} {}",
            if statics != 0 { "all" } else { "dynamic" }
        ));
        s.l2.retain(|(_, v), e| {
            let hit = (port < 0 || e.dest == port) && (vid < 0 || *v == vid) && (statics != 0 || !e.is_static);
            !hit
        });
        NONE
    }

    pub(crate) fn l2_age_set(&self, unit: c_int, seconds: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("l2-age") {
            return rv;
        }
        if seconds < 0 {
            return E_PARAM;
        }
        s.tr(&format!("l2 age {seconds}"));
        s.age = seconds;
        NONE
    }

    // ---- KNET

    #[allow(clippy::too_many_arguments, reason = "the glue's signature")]
    pub(crate) fn netif_create(
        &self,
        unit: c_int,
        name: &CStr,
        port: c_int,
        vid: c_int,
        mac: &[u8; 6],
        keep_tag: c_int,
        netif_id: &mut c_int,
        filter_id: &mut c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("netif-create") {
            return rv;
        }
        if s.index(port).is_none() {
            return E_PORT;
        }
        let name = name.to_string_lossy().into_owned();
        let (n, f) = (s.id(), s.id());
        s.tr(&format!("netif create {name} port {port} vid {vid} -> {n} filter {f}"));
        s.netifs.insert(n, (name, port, vid, *mac, keep_tag != 0));
        s.filters.insert(f, (n, port, keep_tag != 0));
        *netif_id = n;
        *filter_id = f;
        NONE
    }

    pub(crate) fn netif_destroy(&self, unit: c_int, netif_id: c_int, filter_id: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("netif-destroy") {
            return rv;
        }
        if netif_id < -1 || filter_id < -1 {
            return E_PARAM;
        }
        s.tr(&format!("netif destroy {netif_id} filter {filter_id}"));
        if filter_id >= 0 {
            s.filters.remove(&filter_id);
        }
        if netif_id >= 0 && s.netifs.remove(&netif_id).is_none() {
            return E_NOT_FOUND;
        }
        NONE
    }

    pub(crate) fn netif_filter_set(
        &self,
        unit: c_int,
        netif_id: c_int,
        port: c_int,
        keep_tag: c_int,
        filter_id: &mut c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("netif-filter") {
            return rv;
        }
        if !s.netifs.contains_key(&netif_id) {
            return E_NOT_FOUND;
        }
        let f = s.id();
        s.filters.insert(f, (netif_id, port, keep_tag != 0));
        let old = *filter_id;
        if old >= 0 {
            s.filters.remove(&old);
        }
        s.tr(&format!("netif {netif_id} filter {old} -> {f}"));
        *filter_id = f;
        NONE
    }

    // ---- traps

    pub(crate) fn trap_set(&self, unit: c_int, kind: c_int, to_cpu: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("trap") {
            return rv;
        }
        let Some(k) = crate::TrapKind::from_raw(kind) else {
            return E_UNAVAIL;
        };
        if matches!(
            k,
            crate::TrapKind::Dhcpv6 | crate::TrapKind::Stp | crate::TrapKind::Other
        ) {
            return E_UNAVAIL; // no switch control on the glue's path (sdk_glue.c ctl[] = -1)
        }
        s.tr(&format!("trap {k:?} {}", if to_cpu != 0 { "cpu" } else { "off" }));
        s.traps.insert(kind, to_cpu != 0);
        NONE
    }

    // ---- trunks

    pub(crate) fn trunk_create(&self, unit: c_int, tid: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("trunk-create") {
            return rv;
        }
        let Some(t) = (0..MAX_TRUNKS).find(|t| !s.trunks.contains_key(t)) else {
            return E_RESOURCE;
        };
        s.tr(&format!("trunk create {t}"));
        s.trunks.insert(t, Vec::new());
        *tid = t;
        NONE
    }

    pub(crate) fn trunk_destroy(&self, unit: c_int, tid: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("trunk-destroy") {
            return rv;
        }
        s.tr(&format!("trunk destroy {tid}"));
        if s.trunks.remove(&tid).is_some() {
            NONE
        } else {
            E_NOT_FOUND
        }
    }

    pub(crate) fn trunk_set(&self, unit: c_int, tid: c_int, ports: &[c_int], flags: &[c_int]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("trunk-set") {
            return rv;
        }
        let n = ports.len().min(flags.len());
        if n > 64 {
            return E_PARAM;
        }
        if !s.trunks.contains_key(&tid) {
            return E_NOT_FOUND;
        }
        if ports.iter().take(n).any(|p| s.index(*p).is_none()) {
            return E_PORT;
        }
        let members: Vec<(c_int, c_int)> = ports.iter().copied().zip(flags.iter().copied()).take(n).collect();
        s.tr(&format!("trunk {tid} members {members:?}"));
        s.trunks.insert(tid, members);
        NONE
    }

    // ---- L3

    pub(crate) fn l3_intf_create(
        &self,
        unit: c_int,
        mac: &[u8; 6],
        vid: c_int,
        mtu: c_int,
        vrf: c_int,
        id: &mut c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("intf-create", vid) {
            return rv;
        }
        if !(0..=1023).contains(&vrf) || mtu < 0 {
            return E_PARAM;
        }
        if s.intfs.values().any(|(_, v, _, _)| *v == vid) {
            return E_EXISTS;
        }
        let i = s.id();
        s.tr(&format!(
            "intf create {} vid {vid} mtu {mtu} vrf {vrf} -> {i}",
            mac_text(mac)
        ));
        s.intfs.insert(i, (*mac, vid, mtu, vrf));
        *id = i;
        NONE
    }

    pub(crate) fn l3_intf_update(
        &self,
        unit: c_int,
        id: c_int,
        mac: &[u8; 6],
        vid: c_int,
        mtu: c_int,
        vrf: c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.vlan_call("intf-update", vid) {
            return rv;
        }
        if !(0..=1023).contains(&vrf) || mtu < 0 {
            return E_PARAM;
        }
        let Some(e) = s.intfs.get_mut(&id) else {
            return E_NOT_FOUND;
        };
        *e = (*mac, vid, mtu, vrf);
        s.tr(&format!("intf {id} update"));
        NONE
    }

    pub(crate) fn l3_intf_destroy(&self, unit: c_int, id: c_int) -> c_int {
        self.remove("intf-destroy", unit, id, |s| s.intfs.remove(&id).is_some())
    }

    pub(crate) fn l3_station_add(&self, unit: c_int, mac: &[u8; 6], id: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("station-add") {
            return rv;
        }
        let i = s.id();
        s.tr(&format!("station add {} -> {i}", mac_text(mac)));
        s.stations.insert(i, *mac);
        *id = i;
        NONE
    }

    pub(crate) fn l3_station_delete(&self, unit: c_int, id: c_int) -> c_int {
        self.remove("station-delete", unit, id, |s| s.stations.remove(&id).is_some())
    }

    fn egress_check(s: &State, intf: c_int, port: c_int, vid: c_int) -> Result<(), c_int> {
        if !s.intfs.contains_key(&intf) {
            return Err(E_NOT_FOUND);
        }
        if !s.valid_dest(port) {
            return Err(E_PORT);
        }
        if (1..=4094).contains(&vid) {
            Ok(())
        } else {
            Err(E_PARAM)
        }
    }

    pub(crate) fn l3_egress_create(
        &self,
        unit: c_int,
        intf: c_int,
        mac: &[u8; 6],
        port: c_int,
        vid: c_int,
        id: &mut c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s
            .bcm_call("egress-create")
            .and_then(|()| Self::egress_check(&s, intf, port, vid))
        {
            return rv;
        }
        let e = 100_000 + s.id();
        s.tr(&format!(
            "egress create intf {intf} {} {port:#x} vid {vid} -> {e}",
            mac_text(mac)
        ));
        s.egress.insert(e, (intf, *mac, port, vid));
        *id = e;
        NONE
    }

    pub(crate) fn l3_egress_update(
        &self,
        unit: c_int,
        id: c_int,
        intf: c_int,
        mac: &[u8; 6],
        port: c_int,
        vid: c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s
            .bcm_call("egress-update")
            .and_then(|()| Self::egress_check(&s, intf, port, vid))
        {
            return rv;
        }
        let Some(e) = s.egress.get_mut(&id) else {
            return E_NOT_FOUND;
        };
        *e = (intf, *mac, port, vid);
        s.tr(&format!("egress {id} update"));
        NONE
    }

    pub(crate) fn l3_egress_special(&self, unit: c_int, drop: c_int, id: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("egress-special") {
            return rv;
        }
        let slot = usize::from(drop != 0);
        if let Some(Some(cached)) = s.special.get(slot) {
            *id = *cached;
            return NONE;
        }
        let e = 100_000 + s.id();
        s.tr(&format!(
            "egress special {} -> {e}",
            if drop != 0 { "drop" } else { "cpu" }
        ));
        let cpu = s.cpu;
        s.egress.insert(e, (-1, [0; 6], cpu, 0));
        if let Some(cache) = s.special.get_mut(slot) {
            *cache = Some(e);
        }
        *id = e;
        NONE
    }

    pub(crate) fn l3_egress_destroy(&self, unit: c_int, id: c_int) -> c_int {
        self.remove("egress-destroy", unit, id, |s| s.egress.remove(&id).is_some())
    }

    pub(crate) fn l3_host_add(&self, unit: c_int, vrf: c_int, addr: &[u8], egress: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("host-add") {
            return rv;
        }
        if !s.egress.contains_key(&egress) {
            return E_NOT_FOUND;
        }
        s.tr(&format!("host add vrf {vrf} {addr:?} -> {egress}"));
        s.hosts.insert((vrf, addr.to_vec()), egress);
        NONE
    }

    pub(crate) fn l3_host_delete(&self, unit: c_int, vrf: c_int, addr: &[u8]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("host-delete") {
            return rv;
        }
        s.tr(&format!("host delete vrf {vrf} {addr:?}"));
        if s.hosts.remove(&(vrf, addr.to_vec())).is_some() {
            NONE
        } else {
            E_NOT_FOUND
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the glue's signature")]
    pub(crate) fn l3_route_add(
        &self,
        unit: c_int,
        vrf: c_int,
        addr: &[u8],
        mask: &[u8],
        egress: c_int,
        ecmp: c_int,
        drop: c_int,
        replace: c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("route-add") {
            return rv;
        }
        let target_ok = if ecmp != 0 {
            s.ecmp.contains_key(&egress)
        } else {
            s.egress.contains_key(&egress)
        };
        if !target_ok {
            return E_NOT_FOUND;
        }
        let net: Vec<u8> = addr.iter().zip(mask).map(|(a, m)| a & m).collect();
        let key = (vrf, net, mask.to_vec());
        if s.routes.contains_key(&key) && replace == 0 {
            return E_EXISTS;
        }
        s.tr(&format!(
            "route add vrf {vrf} {:?}/{:?} -> {egress}{}",
            key.1,
            key.2,
            if ecmp != 0 { " ecmp" } else { "" }
        ));
        s.routes.insert(key, (egress, ecmp != 0, drop != 0));
        NONE
    }

    pub(crate) fn l3_route_delete(&self, unit: c_int, vrf: c_int, addr: &[u8], mask: &[u8]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("route-delete") {
            return rv;
        }
        let net: Vec<u8> = addr.iter().zip(mask).map(|(a, m)| a & m).collect();
        s.tr(&format!("route delete vrf {vrf} {net:?}/{mask:?}"));
        if s.routes.remove(&(vrf, net, mask.to_vec())).is_some() {
            NONE
        } else {
            E_NOT_FOUND
        }
    }

    pub(crate) fn l3_ecmp_set(&self, unit: c_int, id: &mut c_int, replace: c_int, egress: &[c_int]) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("ecmp-set") {
            return rv;
        }
        if egress.is_empty() || egress.len() > 256 {
            return E_PARAM;
        }
        if egress.iter().any(|e| !s.egress.contains_key(e)) {
            return E_NOT_FOUND;
        }
        if replace != 0 {
            let Some(g) = s.ecmp.get_mut(id) else {
                return E_NOT_FOUND;
            };
            *g = egress.to_vec();
            s.tr(&format!("ecmp {} members {egress:?}", *id));
            return NONE;
        }
        let g = 200_000 + s.id();
        s.tr(&format!("ecmp create {egress:?} -> {g}"));
        s.ecmp.insert(g, egress.to_vec());
        *id = g;
        NONE
    }

    pub(crate) fn l3_ecmp_destroy(&self, unit: c_int, id: c_int) -> c_int {
        self.remove("ecmp-destroy", unit, id, |s| s.ecmp.remove(&id).is_some())
    }

    /// A destroy: "init all", the scripted failure, a non-negative id, then `gone` (`E_NOT_FOUND`
    /// unless it removed something).
    fn remove(&self, op: &str, unit: c_int, id: c_int, gone: impl FnOnce(&mut State) -> bool) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call(op) {
            return rv;
        }
        if id < 0 {
            return E_PARAM;
        }
        s.tr(&format!("{op} {id}"));
        if gone(&mut s) { NONE } else { E_NOT_FOUND }
    }

    // ---- field processor, meters

    pub(crate) fn fp_group_create(&self, unit: c_int, qset: u32, priority: c_int, gid: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("fp-group-create") {
            return rv;
        }
        if s.groups.values().any(|(_, p)| *p == priority) {
            return E_EXISTS; // one group per priority, as the SDK's slices
        }
        let g = s.id();
        s.tr(&format!("fp group create qset {qset:#x} prio {priority} -> {g}"));
        s.groups.insert(g, (qset, priority));
        *gid = g;
        NONE
    }

    pub(crate) fn fp_group_destroy(&self, unit: c_int, gid: c_int) -> c_int {
        if self.state().entries.values().any(|e| e.gid == gid) {
            return E_BUSY;
        }
        self.remove("fp-group-destroy", unit, gid, |s| s.groups.remove(&gid).is_some())
    }

    pub(crate) fn fp_entry_create(
        &self,
        unit: c_int,
        gid: c_int,
        rule: &crate::field::RawRule<'_>,
        eid: &mut c_int,
        stat: &mut c_int,
    ) -> c_int {
        let mut s = self.state();
        let _ = unit;
        *stat = -1;
        if let Err(rv) = s.bcm_call("fp-entry-create") {
            return rv;
        }
        let r = rule.raw();
        let Some((qset, _)) = s.groups.get(&gid).copied() else {
            return E_NOT_FOUND;
        };
        // The rule may only use the group's qualifiers; the IP type must be one of the glue's.
        if r.present & !qset != 0 {
            return E_PARAM;
        }
        if r.present & (1 << sys::OB_FPQ_IPTYPE) != 0 && !(0..=8).contains(&r.ip_type) {
            return E_PARAM;
        }
        if usize::try_from(r.nports).ok() != Some(rule.ports().len()) {
            return E_INTERNAL;
        }
        if r.policer >= 0 && !s.policers.contains_key(&r.policer) {
            return E_NOT_FOUND;
        }
        let e = s.id();
        let st = if r.counter != 0 {
            let id = s.id();
            s.stats.insert(id, (0, 0));
            id
        } else {
            -1
        };
        s.tr(&format!(
            "fp entry create group {gid} present {:#x} prio {} -> {e} stat {st}",
            r.present, r.priority
        ));
        s.entries.insert(
            e,
            FakeEntry {
                gid,
                present: r.present,
                priority: r.priority,
                ports: rule.ports().to_vec(),
                stat: st,
                policer: r.policer,
            },
        );
        *eid = e;
        *stat = st;
        NONE
    }

    pub(crate) fn fp_entry_destroy(&self, unit: c_int, eid: c_int, stat: c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("fp-entry-destroy") {
            return rv;
        }
        if eid < 0 || stat < -1 {
            return E_PARAM;
        }
        if stat >= 0 {
            s.stats.remove(&stat);
        }
        s.tr(&format!("fp entry destroy {eid} stat {stat}"));
        if s.entries.remove(&eid).is_some() {
            NONE
        } else {
            E_NOT_FOUND
        }
    }

    pub(crate) fn fp_stat_get(&self, unit: c_int, stat: c_int, packets: &mut u64, bytes: &mut u64) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("fp-stat") {
            return rv;
        }
        let Some((p, b)) = s.stats.get(&stat).copied() else {
            return E_NOT_FOUND;
        };
        *packets = p;
        *bytes = b;
        NONE
    }

    pub(crate) fn policer_create(&self, unit: c_int, cfg: &sys::ob_policer_cfg_t, pid: &mut c_int) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("policer-create") {
            return rv;
        }
        let p = s.id();
        s.tr(&format!(
            "policer create cir {} cbs {} pir {} pbs {} -> {p}",
            cfg.cir, cfg.cbs, cfg.pir, cfg.pbs
        ));
        s.policers.insert(p, *cfg);
        *pid = p;
        NONE
    }

    pub(crate) fn policer_set(&self, unit: c_int, pid: c_int, cfg: &sys::ob_policer_cfg_t) -> c_int {
        let mut s = self.state();
        let _ = unit;
        if let Err(rv) = s.bcm_call("policer-set") {
            return rv;
        }
        let Some(p) = s.policers.get_mut(&pid) else {
            return E_NOT_FOUND;
        };
        *p = *cfg;
        s.tr(&format!("policer {pid} set"));
        NONE
    }

    pub(crate) fn policer_destroy(&self, unit: c_int, pid: c_int) -> c_int {
        if self.state().entries.values().any(|e| e.policer == pid) {
            return E_BUSY;
        }
        self.remove("policer-destroy", unit, pid, |s| s.policers.remove(&pid).is_some())
    }

    // ---- test hooks: callbacks injected, state read back

    /// A cable event (`ob_fake_link_event`): sets `port`'s ASIC link and, unless the port has a
    /// gate, its reported link, calling the registered link callback on the calling thread.
    /// True when a callback was called (the BCM layer is up, the port exists, the state changed
    /// and a callback is registered).
    pub fn link_event(&self, port: i32, up: bool) -> bool {
        if !self.state().inited {
            return false;
        }
        self.set_link(port, up, true)
    }

    /// One pass of the SDK's software link scan: every gated port's gate is asked (through the
    /// glue's gate trampoline, on the calling thread) and its answer, or the ASIC's link without
    /// an opinion, becomes the reported link; changes call the link callback. The number of
    /// link callbacks called.
    pub fn linkscan(&self) -> usize {
        let (gated, key): (Vec<c_int>, usize) = {
            let s = self.state();
            if !s.inited {
                return 0;
            }
            (s.gates.iter().copied().collect(), s.gate_key)
        };
        let mut called = 0;
        for port in gated {
            let answer = {
                let _held = self.callbacks();
                if !self.state().gates.contains(&port) {
                    continue;
                }
                self.fire_gate(key, port)
            };
            let asic = self
                .state()
                .ports
                .iter()
                .find(|p| p.port == port)
                .is_some_and(|p| p.asic_link);
            let up = if answer < 0 { asic } else { answer != 0 };
            if self.set_link(port, up, false) {
                called += 1;
            }
        }
        called
    }

    /// The registered gate of `port` asked once, as the link scan would; its answer (0 down,
    /// 1 up, -1 no opinion), `None` without a gate.
    pub fn gate(&self, port: i32) -> Option<i32> {
        let _held = self.callbacks();
        let key = {
            let s = self.state();
            if !s.gates.contains(&port) {
                return None;
            }
            s.gate_key
        };
        Some(self.fire_gate(key, port))
    }

    /// An L2 event from the SDK's L2X thread, with the glue's raw arguments (any value: the
    /// trampoline checks them; `mac` `None` is a NULL pointer). True when a callback was called
    /// (one is registered).
    pub fn l2_event(&self, mac: Option<[u8; 6]>, vid: i32, port: i32, learned: i32) -> bool {
        let _held = self.callbacks();
        let key = {
            let s = self.state();
            if !s.inited || !s.l2_cb {
                return false;
            }
            s.cb_key
        };
        let shown = mac.as_ref().map_or_else(|| "null".to_owned(), crate::record::mac);
        self.callback(
            &format!("cb l2(mac={shown}, vid={vid}, port={port}, learned={learned})"),
            || raw::l2(key, mac.as_ref(), vid, port, learned),
            |()| None,
        );
        true
    }

    /// A call of the link trampoline with any context and arguments, as a stray SDK call
    /// would make it (for tests: a key of no registration, an old one, ...).
    pub fn stray_link(key: usize, port: i32, up: i32) {
        raw::link(key, port, up);
    }

    /// The same for the gate trampoline; its answer.
    pub fn stray_gate(key: usize, port: i32) -> i32 {
        raw::gate(key, port)
    }

    /// The same for the L2 trampoline.
    pub fn stray_l2(key: usize, mac: Option<[u8; 6]>, vid: i32, port: i32, learned: i32) {
        raw::l2(key, mac.as_ref(), vid, port, learned);
    }

    /// The registration key the glue was given with the link / L2 callbacks (0: never).
    pub fn callback_key(&self) -> usize {
        self.state().cb_key
    }

    /// Whether a link callback is registered.
    pub fn link_callback_registered(&self) -> bool {
        self.state().link_cb
    }

    /// Whether an L2 callback is registered.
    pub fn l2_callback_registered(&self) -> bool {
        self.state().l2_cb
    }

    /// Whether the BCM layer is up: a full bring-up succeeded and no shutdown followed.
    pub fn bcm_layer_up(&self) -> bool {
        self.state().inited
    }

    /// `ob_sdk_shutdown` calls since the last bring-up attempt.
    pub fn shutdown_calls(&self) -> i32 {
        self.state().shutdowns
    }

    /// `ob_sdk_unregister_callbacks` calls since the last bring-up (the one inside a shutdown
    /// included).
    pub fn unregister_calls(&self) -> i32 {
        self.state().unregisters
    }

    /// `ob_sdk_register_callbacks` calls since the last bring-up.
    pub fn register_calls(&self) -> i32 {
        self.state().registers
    }

    /// Whether `port` is enabled; `None` for a port the scenario does not have.
    pub fn port_enabled(&self, port: i32) -> Option<bool> {
        self.state().ports.iter().find(|p| p.port == port).map(|p| p.enabled)
    }

    /// The board settings last written to `port` as the C fake reports them: autoneg (0 / 1),
    /// FEC mode (0 none, 1 RS, 2 FC), inter-frame gap in bits, each -1 while never set, and
    /// whether an interface type was set.
    pub fn port_settings(&self, port: i32) -> Option<(i32, i32, i32, bool)> {
        self.port(port)
            .map(|p| (p.autoneg, p.fec, p.ifg, p.interface.is_some()))
    }

    /// Everything about `port`.
    pub fn port(&self, port: i32) -> Option<FakePort> {
        self.state().ports.iter().find(|p| p.port == port).cloned()
    }

    /// Sets one of `port`'s counters. False for a port the scenario does not have.
    pub fn set_counter(&self, port: i32, stat: crate::PortStat, value: u64) -> bool {
        let mut s = self.state();
        let index = usize::try_from(stat.raw()).unwrap_or(usize::MAX);
        s.port_mut(port)
            .and_then(|p| p.stats.get_mut(index))
            .map(|c| *c = value)
            .is_some()
    }

    /// The members of VLAN `vid`: (port, untagged); `None` without the VLAN.
    pub fn vlan(&self, vid: u16) -> Option<Vec<(i32, bool)>> {
        self.state()
            .vlans
            .get(&c_int::from(vid))
            .map(|m| m.iter().map(|(p, u)| (*p, *u)).collect())
    }

    /// The L2 table.
    pub fn l2_entries(&self) -> Vec<([u8; 6], u16, FakeL2)> {
        self.state()
            .l2
            .iter()
            .map(|((mac, vid), e)| (*mac, u16::try_from(*vid).unwrap_or(0), *e))
            .collect()
    }

    /// The L2 age timer in seconds.
    pub fn l2_age(&self) -> i32 {
        self.state().age
    }

    /// Trunk `tid`'s members: (port, flags).
    pub fn trunk(&self, tid: u32) -> Option<Vec<(i32, i32)>> {
        let t = c_int::try_from(tid).ok()?;
        self.state().trunks.get(&t).cloned()
    }

    /// Object counts of the tables.
    pub fn counts(&self) -> Counts {
        let s = self.state();
        Counts {
            intfs: s.intfs.len(),
            stations: s.stations.len(),
            egress: s.egress.len(),
            hosts: s.hosts.len(),
            routes: s.routes.len(),
            ecmp: s.ecmp.len(),
            fp_groups: s.groups.len(),
            fp_entries: s.entries.len(),
            fp_stats: s.stats.len(),
            policers: s.policers.len(),
            netifs: s.netifs.len(),
            filters: s.filters.len(),
            trunks: s.trunks.len(),
        }
    }

    /// Field entry `eid`.
    pub fn fp_entry(&self, eid: u32) -> Option<FakeEntry> {
        let e = c_int::try_from(eid).ok()?;
        self.state().entries.get(&e).cloned()
    }

    /// Counts hits on a field counter. False without that counter.
    pub fn fp_hit(&self, stat: u32, packets: u64, bytes: u64) -> bool {
        let Ok(st) = c_int::try_from(stat) else { return false };
        let mut s = self.state();
        s.stats
            .get_mut(&st)
            .map(|(p, b)| {
                *p = p.saturating_add(packets);
                *b = b.saturating_add(bytes);
            })
            .is_some()
    }

    /// Policer `pid`'s configuration as the glue got it.
    pub fn policer(&self, pid: u32) -> Option<sys::ob_policer_cfg_t> {
        let p = c_int::try_from(pid).ok()?;
        self.state().policers.get(&p).copied()
    }

    /// Whether trap kind `kind` is steered to the CPU (`None`: never set).
    pub fn trap(&self, kind: crate::TrapKind) -> Option<bool> {
        self.state().traps.get(&kind.raw()).copied()
    }

    /// The diag shell commands run, in order.
    pub fn shell_log(&self) -> Vec<String> {
        self.state().shell_log.clone()
    }

    /// Sets a PHY register as a scenario's `reg` line does (what the next read returns, unless a
    /// `const`, `after` or `hang` line says otherwise). False when the register file is full.
    pub fn set_mdio_reg(&self, phy: u32, dev: u32, reg: u32, val: u32) -> bool {
        self.state().set_reg(Reg { phy, dev, reg }, val)
    }

    /// A PHY register as the fake holds it (`None`: never written nor scripted).
    pub fn mdio_reg(&self, phy: u32, dev: u32, reg: u32) -> Option<u32> {
        let at = Reg { phy, dev, reg };
        self.state().regs.iter().find(|(r, _)| *r == at).map(|(_, v)| *v)
    }

    /// Whether the core PLL was powered down (the over-temperature shutdown).
    pub fn pll_down(&self) -> bool {
        self.state().pll_down
    }
}

fn mac_text(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

pub mod process {
    //! The process's fake ([`super::Chip`]) through the C fake's hooks, by the same names: what
    //! `sdk_glue::fake` is in a build that links no C glue. All of them may be called from any
    //! thread, with or without a [`crate::Switch`].

    use super::process_chip;

    /// The process's fake chip.
    pub fn chip() -> &'static super::Chip {
        process_chip()
    }

    /// A cable event ([`super::Chip::link_event`]).
    pub fn link_event(port: i32, up: bool) -> bool {
        process_chip().link_event(port, up)
    }

    /// Whether a link callback is registered.
    pub fn link_callback_registered() -> bool {
        process_chip().link_callback_registered()
    }

    /// Whether the BCM layer is up.
    pub fn bcm_layer_up() -> bool {
        process_chip().bcm_layer_up()
    }

    /// `ob_sdk_shutdown` calls since the last bring-up attempt.
    pub fn shutdown_calls() -> i32 {
        process_chip().shutdown_calls()
    }

    /// `ob_sdk_unregister_callbacks` calls since the last bring-up.
    pub fn unregister_calls() -> i32 {
        process_chip().unregister_calls()
    }

    /// `ob_sdk_register_callbacks` calls since the last bring-up.
    pub fn register_calls() -> i32 {
        process_chip().register_calls()
    }

    /// Whether `port` is enabled.
    pub fn port_enabled(port: i32) -> Option<bool> {
        process_chip().port_enabled(port)
    }

    /// The board settings last written to `port`.
    pub fn port_settings(port: i32) -> Option<(i32, i32, i32, bool)> {
        process_chip().port_settings(port)
    }
}

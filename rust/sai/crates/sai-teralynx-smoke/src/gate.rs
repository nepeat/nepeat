//! The H1 gate of the CS-B plan: the CS8210's libsai on the chip with `SONiC` stopped, step by step,
//! one line per step: `PASS <step> <values>`, `FAIL <step> <why> | <values>`, `SKIP <step> <why>`; the last
//! line `SMOKE_OK ...` or `SMOKE_FAILED ...`. Exit 0: no step failed; 1: a step failed (after the
//! teardown); 2: usage or refusal (nothing loaded). `--dry` lists the steps and what each touches.
//!
//! CREATING THE SWITCH INITIALISES THE CHIP (`ifcs_init`, node create / enable, devports, the
//! profile's init commands and post-init register list). The program refuses without
//! `--sonic-is-stopped` and while a `syncd` process runs. What it does itself: SAI calls, the diag
//! socket's read-only commands (`regs`, `--diag-cmd`), reading the guard's `.prom` file and its own
//! stderr (the library's lines, passed on; `--no-log-capture`: not read).
//!
//! The cabled loops are devport pairs (`--pairs A-B,...`, default `1-2,3-17,31-32`: the CS8210's
//! 1<->2 and 31<->32 optical, 3<->17 DAC; `--pairs none`: no link steps); a devport's SAI port is the
//! one whose `HW_LANE_LIST[0]` is the devport's first lane in the port map (`SAI_TERALYNX_PORT_MAP`).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sai_client::{Api, Attr, Event, Notify, Sai, Status, Value, sys};

use crate::{read_profile, say, warn};

/// The steps in their order, with what each touches (`--dry`).
pub const STEPS: &[(&str, &str)] = &[
    (
        "load",
        "dlopen the library, sai_api_initialize, sai_api_query (switch, port, hostif): nothing on the chip",
    ),
    (
        "profile",
        "the profile served, the port map read (devport -> first lane), the pairs' lanes: nothing on the chip",
    ),
    (
        "create-switch",
        "INITIALISES THE CHIP: libifcs loaded (shim, GOT redirects), ifcs_init, node create / enable, devports, the init \
         commands, the post-init register list (SAI_TERALYNX_POST_INIT_REGS), link scan and event callbacks, the diag socket",
    ),
    (
        "guard",
        "the library's stderr: the learn-ring guard's GOT redirects (node_pci_reg_read32 / _write32 / im_malloc), the \
         memcpy redirect; <run dir>/ifcs-guard.prom: reads only",
    ),
    (
        "ports",
        "PORT_NUMBER, PORT_LIST, CPU_PORT, per port HW_LANE_LIST / SPEED / ADMIN_STATE / OPER_STATUS: reads only",
    ),
    (
        "regs",
        "diag socket 'regs': the post-init list read back and the board's watch registers (SAI_TERALYNX_DIAG_REGS: the \
         LED blocks): BAR0 reads only, the offsets the profile's files name",
    ),
    (
        "admin-up",
        "the pairs' ports MTU 9100 and admin up (devport admin enable of those cages)",
    ),
    (
        "oper-up",
        "waits for OPER_STATUS up and a PORT_STATE_CHANGE up of every pair port (link scan): reads only",
    ),
    (
        "hold",
        "only with --hold S: every 2 s for S s the pair ports' oper state, counters, MAX_TEMP: reads only",
    ),
    (
        "port-stats",
        "get_port_stats of the pair ports (4 counters): counter reads",
    ),
    (
        "temperature",
        "SWITCH MAX_TEMP / AVERAGE_TEMP / TEMP_LIST: the temperature monitors (a read holds the SDK ~400 ms)",
    ),
    (
        "diag",
        "one read-only command (--diag-cmd, default 'ports') through the diag socket",
    ),
    ("admin-down", "the pairs' ports admin down, oper down awaited"),
    (
        "remove-switch",
        "remove_switch: ports down, ifcs_deinit (SAI_TERALYNX_SHUTDOWN)",
    ),
    (
        "recreate",
        "only with --recreate: create_switch again in this process (INITIALISES THE CHIP AGAIN on the loaded libifcs), \
         ports counted, remove_switch",
    ),
    ("uninitialize", "sai_api_uninitialize"),
];

/// The options.
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "command-line flags")]
pub struct Opt {
    /// the library (default `sai_client::default_libsai`)
    pub libsai: Option<String>,
    /// the profile file
    pub profile: Option<String>,
    /// devport pairs
    pub pairs: Vec<(u32, u32)>,
    /// the port map (default: the profile's `SAI_TERALYNX_PORT_MAP`)
    pub port_map: Option<String>,
    /// expected ports, lanes per port, speed
    pub expect: (u32, u32, u32),
    /// seconds for link up / down, a diag answer, the hold
    pub link_timeout: u64,
    /// seconds for a diag answer
    pub diag_timeout: u64,
    /// seconds of the hold step (0: skipped)
    pub hold: u64,
    /// the diag step's command
    pub diag_cmd: String,
    /// do not read stderr
    pub no_log_capture: bool,
    /// the recreate step
    pub recreate: bool,
    /// list the steps
    pub dry: bool,
    /// --sonic-is-stopped
    pub confirmed: bool,
}

impl Default for Opt {
    fn default() -> Self {
        Self {
            libsai: None,
            profile: None,
            pairs: vec![(1, 2), (3, 17), (31, 32)],
            port_map: None,
            expect: (32, 8, 400_000),
            link_timeout: 60,
            diag_timeout: 10,
            hold: 0,
            diag_cmd: "ports".to_owned(),
            no_log_capture: false,
            recreate: false,
            dry: false,
            confirmed: false,
        }
    }
}

/// The usage text (exit 2).
pub fn usage() -> i32 {
    warn(
        "usage: sai-teralynx-smoke --dry\n\
         \x20      sai-teralynx-smoke --sonic-is-stopped --profile <sai.profile> [--libsai <libsai.so>]\n\
         \x20        [--pairs A-B,...|none] [--port-map FILE] [--expect-ports N] [--expect-lanes N] [--expect-speed MBPS]\n\
         \x20        [--link-timeout S] [--diag-timeout S] [--hold S] [--diag-cmd CMD] [--no-log-capture] [--recreate]\n\
         \x20      sai-teralynx-smoke PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]   (the C++ program's form)\n\
         Creating the switch INITIALISES THE CHIP: stop SONiC (systemctl stop sonic.target) and the vendor stack\n\
         (cs8210-switch) first, then pass --sonic-is-stopped. --dry lists the steps and what each touches.\n\
         \x20 --pairs         cabled loops as devport pairs (default 1-2,3-17,31-32: the CS8210's); none: no link steps\n\
         \x20 --port-map      devport -> lanes (default: the profile's SAI_TERALYNX_PORT_MAP)\n\
         \x20 --expect-*      default 32 ports, 8 lanes, 400000 Mb/s\n\
         \x20 --link-timeout  seconds for oper up / down (default 60); --diag-timeout for a diag answer (default 10)\n\
         \x20 --hold          seconds the links must stay up after oper-up (default 0: step skipped)\n\
         \x20 --diag-cmd      the diag step's command (default ports)\n\
         \x20 --no-log-capture  leave stderr alone (the guard step is skipped)\n\
         \x20 --recreate      a second create_switch / remove_switch in this process (initialises the chip again)",
    );
    2
}

fn num(s: Option<&String>, lo: u64, hi: u64) -> Option<u64> {
    s.and_then(|v| v.parse::<u64>().ok()).filter(|v| (lo..=hi).contains(v))
}

/// Parses `A-B,C-D` (distinct devports) or `none`.
pub fn parse_pairs(s: &str) -> Option<Vec<(u32, u32)>> {
    if s == "none" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for p in s.split(',') {
        let (a, b) = p.split_once('-')?;
        let (a, b) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
        if a == b || seen.contains(&a) || seen.contains(&b) {
            return None;
        }
        seen.extend([a, b]);
        out.push((a, b));
    }
    (!out.is_empty()).then_some(out)
}

/// The options, or the exit code of a usage error.
pub fn parse(args: &[String]) -> Result<Opt, i32> {
    let mut o = Opt::default();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry" => o.dry = true,
            "--sonic-is-stopped" => o.confirmed = true,
            "--recreate" => o.recreate = true,
            "--no-log-capture" => o.no_log_capture = true,
            "--libsai" => o.libsai = Some(it.next().ok_or_else(usage)?.clone()),
            "--profile" => o.profile = Some(it.next().ok_or_else(usage)?.clone()),
            "--port-map" => o.port_map = Some(it.next().ok_or_else(usage)?.clone()),
            "--diag-cmd" => {
                let c = it.next().ok_or_else(usage)?;
                if c.contains('\n') || c.is_empty() {
                    warn("--diag-cmd: one line");
                    return Err(2);
                }
                o.diag_cmd.clone_from(c);
            }
            "--pairs" => {
                o.pairs = it.next().and_then(|v| parse_pairs(v)).ok_or_else(|| {
                    warn("--pairs: A-B,C-D,... (distinct devports) or none");
                    2
                })?;
            }
            "--expect-ports" | "--expect-lanes" | "--expect-speed" | "--link-timeout" | "--diag-timeout" | "--hold" => {
                let (lo, hi) = match a.as_str() {
                    "--expect-ports" => (1, 512),
                    "--expect-lanes" => (1, 16),
                    "--expect-speed" => (1, 1_600_000),
                    "--hold" => (0, 86_400),
                    _ => (1, 600),
                };
                let Some(v) = num(it.next(), lo, hi) else {
                    warn(&format!("{a}: a number {lo}..{hi}"));
                    return Err(2);
                };
                let v32 = u32::try_from(v).unwrap_or(u32::MAX);
                match a.as_str() {
                    "--expect-ports" => o.expect.0 = v32,
                    "--expect-lanes" => o.expect.1 = v32,
                    "--expect-speed" => o.expect.2 = v32,
                    "--link-timeout" => o.link_timeout = v,
                    "--diag-timeout" => o.diag_timeout = v,
                    _ => o.hold = v,
                }
            }
            _ => return Err(usage()),
        }
    }
    if !o.dry && o.profile.is_none() {
        return Err(usage());
    }
    Ok(o)
}

/// The port map's first lane per devport (`devport lanes speed fec`, `#` comments).
pub fn port_map(text: &str) -> BTreeMap<u32, u32> {
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut w = line.split_whitespace();
        let (Some(dp), Some(lanes)) = (w.next(), w.next()) else {
            continue;
        };
        if let (Ok(dp), Some(Ok(l0))) = (dp.parse::<u32>(), lanes.split(',').next().map(str::parse::<u32>)) {
            m.insert(dp, l0);
        }
    }
    m
}

/// Whether a process named `syncd` runs (/proc/*/comm).
fn syncd_running() -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim_end() == "syncd"))
}

/// One command through the diag socket (one line, the answer up to EOF).
pub fn diag(path: &str, cmd: &str, timeout: Duration) -> Result<String, String> {
    let mut c = UnixStream::connect(path).map_err(|e| format!("{path}: {e}"))?;
    let _ = c.set_read_timeout(Some(timeout));
    let _ = c.set_write_timeout(Some(timeout));
    c.write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| format!("{path}: write: {e}"))?;
    let _ = c.shutdown(std::net::Shutdown::Write);
    let mut out = Vec::new();
    c.read_to_end(&mut out)
        .map_err(|e| format!("no complete answer within {} s ({e})", timeout.as_secs()))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The step lines and counts.
#[derive(Default)]
struct Rep {
    pass: u32,
    fail: u32,
    skip: u32,
}

/// One step's values and reasons.
struct Step {
    name: &'static str,
    vals: Vec<String>,
    why: Vec<String>,
}

impl Step {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            vals: Vec::new(),
            why: Vec::new(),
        }
    }
    fn val(&mut self, v: impl Into<String>) {
        self.vals.push(v.into());
    }
    fn bad(&mut self, w: impl Into<String>) {
        self.why.push(w.into());
    }
    /// A SAI call's status: true on success, else the step fails.
    fn ok(&mut self, what: &str, st: Status) -> bool {
        if st == sai_client::SUCCESS {
            return true;
        }
        self.bad(format!("{what}: status {st}"));
        false
    }
}

impl Rep {
    #[allow(clippy::needless_pass_by_value, reason = "a step ends once")]
    fn end(&mut self, s: Step) -> bool {
        let vals = s.vals.join(" ");
        if s.why.is_empty() {
            self.pass += 1;
            say(&format!("PASS {} {vals}", s.name));
            true
        } else {
            self.fail += 1;
            say(&format!(
                "FAIL {} {}{}{vals}",
                s.name,
                s.why.join("; "),
                if vals.is_empty() { "" } else { " | " }
            ));
            false
        }
    }
    fn skip(&mut self, name: &str, why: &str) {
        self.skip += 1;
        say(&format!("SKIP {name} {why}"));
    }
}

type Counts = Arc<Mutex<HashMap<sys::sai_object_id_t, (u32, u32)>>>;

fn events(c: &Counts, port: sys::sai_object_id_t) -> (u32, u32) {
    c.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&port)
        .copied()
        .unwrap_or((0, 0))
}

struct Gate {
    o: Opt,
    sai: Sai,
    profile: BTreeMap<String, String>,
    /// devport -> first lane
    map: BTreeMap<u32, u32>,
    rep: Rep,
    sw: sys::sai_object_id_t,
    /// first lane -> port
    ports: BTreeMap<u32, sys::sai_object_id_t>,
    counts: Counts,
    tee: Option<sai_client::tee::Tee>,
}

fn s32(a: &Attr) -> i32 {
    if let Value::S32(v) = a.value { v } else { 0 }
}

impl Gate {
    fn key(&self, k: &str) -> Option<&str> {
        self.profile.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }

    fn pair_ports(&self) -> Vec<(u32, sys::sai_object_id_t)> {
        let mut v = Vec::new();
        for (a, b) in &self.o.pairs {
            for dp in [a, b] {
                if let Some(p) = self.map.get(dp).and_then(|l| self.ports.get(l)) {
                    v.push((*dp, *p));
                }
            }
        }
        v
    }

    fn oper(&self, port: sys::sai_object_id_t) -> Option<i32> {
        let mut a = [Attr::new(sys::SAI_PORT_ATTR_OPER_STATUS, Value::S32(0))];
        (self.sai.get(sys::SAI_OBJECT_TYPE_PORT, port, &mut a) == sai_client::SUCCESS).then(|| s32(&a[0]))
    }

    fn create_switch(&mut self, name: &'static str) -> bool {
        let mut s = Step::new(name);
        let at = [
            Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
            Attr::new(
                sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
                Value::Mac([0x0e, 0x48, 0xc6, 0x7a, 0x55, 0xce]),
            ),
            Attr::new(
                sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
                Value::Notify(Notify::PortState),
            ),
            Attr::new(
                sys::SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY,
                Value::Notify(Notify::Shutdown),
            ),
        ];
        say("creating the switch: the chip is initialised now");
        let t0 = Instant::now();
        self.sw = 0;
        if s.ok("create_switch", self.sai.create_switch(&mut self.sw, &at)) {
            s.val(format!("switch=0x{:016x} ms={}", self.sw, t0.elapsed().as_millis()));
        } else {
            s.bad("the library's own lines above say why");
        }
        self.rep.end(s)
    }

    fn guard(&mut self) {
        let Some(tee) = &self.tee else {
            self.rep
                .skip("guard", "--no-log-capture: the library's stderr is not read");
            return;
        };
        std::thread::sleep(Duration::from_millis(50));
        let lines = tee.lines();
        let mut s = Step::new("guard");
        let g = lines
            .iter()
            .find(|l| l.contains("learn-ring guard") || l.contains("libsai-teralynx-shim.so guard"));
        match g {
            None if self.key("SAI_TERALYNX_IFCS_GUARD") == Some("off") => {
                self.rep.skip("guard", "SAI_TERALYNX_IFCS_GUARD=off in the profile");
                return;
            }
            None => s.bad("no learn-ring guard line in the library's stderr"),
            Some(l) => {
                let after = |k: &str| {
                    l.split(k).nth(1).and_then(|r| {
                        r.trim_start()
                            .split(|c: char| !c.is_ascii_digit())
                            .next()?
                            .parse::<u32>()
                            .ok()
                    })
                };
                let (rd, wr, im) = (
                    after("node_pci_reg_read32 "),
                    after("node_pci_reg_write32 "),
                    after("im_malloc "),
                );
                s.val(format!(
                    "got_read32={} got_write32={} got_im_malloc={}",
                    rd.map_or("?".to_owned(), |v| v.to_string()),
                    wr.map_or("?".to_owned(), |v| v.to_string()),
                    im.map_or("?".to_owned(), |v| v.to_string())
                ));
                if !l.contains("guard active") || rd.unwrap_or(0) == 0 {
                    s.bad(format!("not active: {l}"));
                }
            }
        }
        if lines.iter().any(|l| l.contains("has no memcpy import to redirect")) {
            s.val("memcpy=no-import");
        }
        for l in lines
            .iter()
            .filter(|l| l.contains("GOT redirect") || l.contains("left alone"))
        {
            s.bad(l.clone());
        }
        let run = self
            .key("SAI_TERALYNX_RUN_DIR")
            .unwrap_or("/run/sai-teralynx")
            .to_owned();
        match std::fs::read_to_string(format!("{run}/ifcs-guard.prom")) {
            Ok(t) => {
                let inst = t
                    .lines()
                    .find_map(|l| l.strip_prefix("sai_teralynx_ifcs_guard_installed "))
                    .map(str::trim);
                s.val(format!("prom_installed={}", inst.unwrap_or("?")));
                if inst == Some("0") {
                    s.bad("ifcs-guard.prom says installed 0");
                }
            }
            Err(_) => s.val("prom=absent"),
        }
        self.rep.end(s);
    }

    fn ports(&mut self) -> bool {
        let mut s = Step::new("ports");
        let mut g = [
            Attr::new(sys::SAI_SWITCH_ATTR_PORT_NUMBER, Value::U32(0)),
            Attr::new(sys::SAI_SWITCH_ATTR_PORT_LIST, Value::OidList(vec![0; 512])),
            Attr::new(sys::SAI_SWITCH_ATTR_CPU_PORT, Value::Oid(0)),
        ];
        if !s.ok(
            "get PORT_NUMBER/PORT_LIST/CPU_PORT",
            self.sai.get(sys::SAI_OBJECT_TYPE_SWITCH, self.sw, &mut g),
        ) {
            return self.rep.end(s);
        }
        let list = if let Value::OidList(l) = &g[1].value {
            l.clone()
        } else {
            Vec::new()
        };
        let number = if let Value::U32(v) = g[0].value { v } else { 0 };
        let cpu = if let Value::Oid(v) = g[2].value { v } else { 0 };
        s.val(format!("ports={} cpu=0x{cpu:016x}", list.len()));
        if usize::try_from(number).ok() != Some(list.len()) {
            s.bad(format!("PORT_NUMBER {number}, PORT_LIST has {}", list.len()));
        }
        if usize::try_from(self.o.expect.0).ok() != Some(list.len()) {
            s.bad(format!("{} ports, expected {}", list.len(), self.o.expect.0));
        }
        say(&format!(
            "{:>5} {:<18} {:<24} {:>7} {:>5} {:>4}",
            "lane", "port", "lanes", "speed", "admin", "oper"
        ));
        self.ports.clear();
        let mut admin_up = 0;
        for p in &list {
            let mut q = [
                Attr::new(sys::SAI_PORT_ATTR_HW_LANE_LIST, Value::U32List(vec![0; 16])),
                Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(0)),
                Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(false)),
                Attr::new(sys::SAI_PORT_ATTR_OPER_STATUS, Value::S32(0)),
            ];
            if !s.ok(
                "get_port_attribute",
                self.sai.get(sys::SAI_OBJECT_TYPE_PORT, *p, &mut q),
            ) {
                continue;
            }
            let lanes = if let Value::U32List(l) = &q[0].value {
                l.clone()
            } else {
                Vec::new()
            };
            let speed = if let Value::U32(v) = q[1].value { v } else { 0 };
            let admin = matches!(q[2].value, Value::Bool(true));
            let oper = s32(&q[3]);
            let l0 = lanes.first().copied().unwrap_or(0);
            let text: Vec<String> = lanes.iter().map(u32::to_string).collect();
            say(&format!(
                "{l0:>5} 0x{p:016x} {:<24} {speed:>7} {:>5} {:>4}",
                text.join(","),
                if admin { "up" } else { "down" },
                if oper == 1 { "up" } else { "down" }
            ));
            if u32::try_from(lanes.len()).ok() != Some(self.o.expect.1) {
                s.bad(format!(
                    "port {l0}: {} lanes, expected {}",
                    lanes.len(),
                    self.o.expect.1
                ));
            }
            if speed != self.o.expect.2 {
                s.bad(format!("port {l0}: speed {speed}, expected {}", self.o.expect.2));
            }
            admin_up += u32::from(admin);
            if self.ports.insert(l0, *p).is_some() {
                s.bad(format!("two ports with first lane {l0}"));
            }
        }
        s.val(format!("admin_up={admin_up}"));
        for (a, b) in self.o.pairs.clone() {
            for dp in [a, b] {
                let l = self.map.get(&dp).copied().unwrap_or(0);
                if !self.ports.contains_key(&l) {
                    s.bad(format!("devport {dp} (first lane {l}): no port has that first lane"));
                }
            }
        }
        s.val(format!("pairs={}", self.o.pairs.len()));
        self.rep.end(s)
    }

    fn regs(&mut self) {
        let has = self.key("SAI_TERALYNX_DIAG_REGS").is_some() || self.key("SAI_TERALYNX_POST_INIT_REGS").is_some();
        let Some(sock) = self.key("SAI_OPENBCM_DIAG_SOCKET").map(str::to_owned) else {
            self.rep.skip("regs", "the profile has no SAI_OPENBCM_DIAG_SOCKET");
            return;
        };
        if !has {
            self.rep.skip(
                "regs",
                "the profile has no SAI_TERALYNX_DIAG_REGS / SAI_TERALYNX_POST_INIT_REGS",
            );
            return;
        }
        let mut s = Step::new("regs");
        match diag(&sock, "regs", Duration::from_secs(self.o.diag_timeout)) {
            Err(e) => s.bad(e),
            Ok(a) if !a.starts_with("post-init registers:") => {
                let first = a.lines().next().unwrap_or("").to_owned();
                self.rep
                    .skip("regs", &format!("the library has no regs command (answer: {first})"));
                return;
            }
            Ok(a) => {
                for l in a.lines() {
                    say(&format!("  regs: {l}"));
                }
                let head = a.lines().next().unwrap_or_default();
                let counts = head.split_once(": ").map_or("", |x| x.1);
                let field = |k: &str| {
                    counts.split(", ").find_map(|p| {
                        p.trim()
                            .strip_suffix(k)
                            .map(str::trim)
                            .and_then(|n| n.parse::<u32>().ok())
                    })
                };
                let (listed, mism, failed) = (field("listed"), field("mismatched"), field("failed"));
                let differs = a.lines().filter(|l| l.ends_with(" DIFFERS")).count();
                let unread = a.lines().filter(|l| l.contains("read failed")).count();
                let watch = a.lines().filter(|l| l.contains(" = 0x")).count();
                s.val(format!(
                    "post_init={} mismatched={} failed={} differs_now={differs} watch_read={watch} unreadable={unread}",
                    listed.unwrap_or(0),
                    mism.unwrap_or(0),
                    failed.unwrap_or(0)
                ));
                if mism.unwrap_or(0) > 0 || failed.unwrap_or(0) > 0 || differs > 0 {
                    s.bad("the post-init register list did not hold (lines above)");
                }
            }
        }
        self.rep.end(s);
    }

    fn admin(&mut self, up: bool) -> bool {
        let mut s = Step::new(if up { "admin-up" } else { "admin-down" });
        let pp = self.pair_ports();
        for (dp, p) in &pp {
            if up {
                let mtu = Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9100));
                s.ok(
                    &format!("devport {dp} mtu 9100"),
                    self.sai.set(sys::SAI_OBJECT_TYPE_PORT, *p, Some(&mtu)),
                );
            }
            let a = Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up));
            s.ok(
                &format!("devport {dp} admin {}", if up { "up" } else { "down" }),
                self.sai.set(sys::SAI_OBJECT_TYPE_PORT, *p, Some(&a)),
            );
        }
        s.val(format!("ports={}", pp.len()));
        if !up {
            let t0 = Instant::now();
            let deadline = t0 + Duration::from_secs(self.o.link_timeout);
            loop {
                let still: Vec<u32> = pp
                    .iter()
                    .filter(|(_, p)| self.oper(*p) == Some(1))
                    .map(|(d, _)| *d)
                    .collect();
                if still.is_empty() {
                    s.val(format!("oper_down_ms={}", t0.elapsed().as_millis()));
                    break;
                }
                if Instant::now() > deadline {
                    s.bad(format!(
                        "devports {still:?} still oper up after {} s",
                        self.o.link_timeout
                    ));
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        self.rep.end(s)
    }

    fn oper_up(&mut self, before: &HashMap<sys::sai_object_id_t, u32>) -> bool {
        let mut s = Step::new("oper-up");
        let t0 = Instant::now();
        let deadline = t0 + Duration::from_secs(self.o.link_timeout);
        let pairs = self.o.pairs.clone();
        let port_of = |dp: u32| self.map.get(&dp).and_then(|l| self.ports.get(l)).copied().unwrap_or(0);
        let mut up_at: BTreeMap<u32, u128> = BTreeMap::new();
        loop {
            for (a, b) in &pairs {
                for dp in [*a, *b] {
                    let p = port_of(dp);
                    let ev = events(&self.counts, p).0 > before.get(&p).copied().unwrap_or(0);
                    if !up_at.contains_key(&dp) && ev && self.oper(p) == Some(1) {
                        up_at.insert(dp, t0.elapsed().as_millis());
                    }
                }
            }
            if up_at.len() == pairs.len() * 2 || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        for (a, b) in &pairs {
            if let (Some(x), Some(y)) = (up_at.get(a), up_at.get(b)) {
                s.val(format!("{a}<->{b}=up({}ms)", x.max(y)));
            } else {
                {
                    let state = |dp: u32| {
                        let p = port_of(dp);
                        let ev = events(&self.counts, p).0 > before.get(&p).copied().unwrap_or(0);
                        format!(
                            "devport {dp} oper {}{}",
                            self.oper(p).map_or("?".to_owned(), |v| if v == 1 {
                                "up".to_owned()
                            } else {
                                "down".to_owned()
                            }),
                            if ev { "" } else { " (no up event)" }
                        )
                    };
                    s.bad(format!(
                        "pair {a}<->{b} not up within {} s: {}, {}",
                        self.o.link_timeout,
                        state(*a),
                        state(*b)
                    ));
                }
            }
        }
        self.rep.end(s)
    }

    fn stats_line(&self, p: sys::sai_object_id_t) -> (Status, String) {
        let ids = [
            sys::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
            sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
            sys::SAI_PORT_STAT_IF_IN_ERRORS,
            sys::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
        ];
        let (st, v) = self.sai.stats(sys::SAI_OBJECT_TYPE_PORT, p, &ids);
        let g = |i: usize| v.get(i).copied().unwrap_or(0);
        (st, format!("in={} out={} err={} nonuc={}", g(0), g(1), g(2), g(3)))
    }

    fn max_temp(&self) -> (Status, i32) {
        let mut a = [Attr::new(sys::SAI_SWITCH_ATTR_MAX_TEMP, Value::S32(0))];
        let st = self.sai.get(sys::SAI_OBJECT_TYPE_SWITCH, self.sw, &mut a);
        (st, s32(&a[0]))
    }

    fn hold(&mut self) {
        if self.o.hold == 0 {
            self.rep.skip("hold", "not asked (--hold S)");
            return;
        }
        let mut s = Step::new("hold");
        let pp = self.pair_ports();
        let downs: HashMap<_, _> = pp.iter().map(|(_, p)| (*p, events(&self.counts, *p).1)).collect();
        let t0 = Instant::now();
        let mut n = 0;
        while t0.elapsed() < Duration::from_secs(self.o.hold) {
            let (ts, temp) = self.max_temp();
            let mut line = format!("  hold t={}s temp({ts})={temp}C", t0.elapsed().as_secs());
            for (dp, p) in &pp {
                let oper = self.oper(*p);
                let (_, st) = self.stats_line(*p);
                let _ = write!(line, " | {dp} oper={} {st}", oper.unwrap_or(-1));
                if oper != Some(1) {
                    s.bad(format!(
                        "devport {dp} oper {} at {} s",
                        oper.unwrap_or(-1),
                        t0.elapsed().as_secs()
                    ));
                }
            }
            say(&line);
            n += 1;
            std::thread::sleep(Duration::from_secs(2));
        }
        for (dp, p) in &pp {
            if events(&self.counts, *p).1 > downs.get(p).copied().unwrap_or(0) {
                s.bad(format!("devport {dp}: a link down event during the hold"));
            }
        }
        s.why.dedup();
        s.val(format!("samples={n} seconds={}", self.o.hold));
        self.rep.end(s);
    }

    fn port_stats(&mut self) {
        let mut s = Step::new("port-stats");
        for (dp, p) in self.pair_ports() {
            let (st, line) = self.stats_line(p);
            if s.ok(&format!("devport {dp} get_port_stats"), st) {
                s.val(format!("{dp}:{line}"));
            }
        }
        self.rep.end(s);
    }

    fn temperature(&mut self) {
        let mut s = Step::new("temperature");
        let (st, max) = self.max_temp();
        if s.ok("MAX_TEMP", st) {
            s.val(format!("max={max}C"));
            if !(1..=110).contains(&max) {
                s.bad(format!("MAX_TEMP {max} C is not plausible (1..110)"));
            }
        }
        let mut a = [
            Attr::new(sys::SAI_SWITCH_ATTR_AVERAGE_TEMP, Value::S32(0)),
            Attr::new(sys::SAI_SWITCH_ATTR_TEMP_LIST, Value::S32List(vec![0; 16])),
        ];
        let st = self.sai.get(sys::SAI_OBJECT_TYPE_SWITCH, self.sw, &mut a);
        if st == sai_client::SUCCESS {
            let list = if let Value::S32List(l) = &a[1].value {
                l.clone()
            } else {
                Vec::new()
            };
            s.val(format!("avg={}C list={list:?}", s32(&a[0])));
        } else {
            s.val(format!("avg/list status {st}"));
        }
        self.rep.end(s);
    }

    fn diag_step(&mut self) {
        let Some(sock) = self.key("SAI_OPENBCM_DIAG_SOCKET").map(str::to_owned) else {
            self.rep.skip("diag", "the profile has no SAI_OPENBCM_DIAG_SOCKET");
            return;
        };
        let mut s = Step::new("diag");
        match diag(&sock, &self.o.diag_cmd, Duration::from_secs(self.o.diag_timeout)) {
            Err(e) => s.bad(e),
            Ok(a) if a.trim().is_empty() => s.bad(format!("'{}': an empty answer", self.o.diag_cmd)),
            Ok(a) => {
                for l in a.lines() {
                    say(&format!("  diag: {l}"));
                }
                s.val(format!(
                    "cmd='{}' lines={} first='{}'",
                    self.o.diag_cmd,
                    a.lines().count(),
                    a.lines().next().unwrap_or("")
                ));
            }
        }
        self.rep.end(s);
    }

    fn remove_switch(&mut self, name: &'static str) -> bool {
        let mut s = Step::new(name);
        s.ok("remove_switch", self.sai.remove_switch(self.sw));
        self.rep.end(s)
    }

    fn recreate(&mut self) {
        if !self.o.recreate {
            self.rep.skip("recreate", "not asked (--recreate)");
            return;
        }
        let mut s = Step::new("recreate");
        let at = [
            Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
            Attr::new(
                sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
                Value::Mac([0x0e, 0x48, 0xc6, 0x7a, 0x55, 0xce]),
            ),
            Attr::new(
                sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
                Value::Notify(Notify::PortState),
            ),
        ];
        let t0 = Instant::now();
        let mut sw = 0;
        if s.ok("create_switch (again)", self.sai.create_switch(&mut sw, &at)) {
            s.val(format!("switch=0x{sw:016x} ms={}", t0.elapsed().as_millis()));
            let mut g = [Attr::new(sys::SAI_SWITCH_ATTR_PORT_LIST, Value::OidList(vec![0; 512]))];
            if s.ok("get PORT_LIST", self.sai.get(sys::SAI_OBJECT_TYPE_SWITCH, sw, &mut g)) {
                s.val(format!("ports={}", g[0].count));
                if g[0].count != self.o.expect.0 {
                    s.bad(format!("{} ports, expected {}", g[0].count, self.o.expect.0));
                }
            }
            s.ok("remove_switch (again)", self.sai.remove_switch(sw));
        }
        self.rep.end(s);
    }

    fn finish(mut self) -> i32 {
        let mut s = Step::new("uninitialize");
        s.ok("sai_api_uninitialize", self.sai.uninitialize());
        self.rep.end(s);
        if let Some(t) = self.tee.take() {
            let _ = t.finish();
        }
        let r = &self.rep;
        if r.fail > 0 {
            say(&format!(
                "SMOKE_FAILED {} FAIL, {} PASS, {} SKIP",
                r.fail, r.pass, r.skip
            ));
            1
        } else {
            say(&format!(
                "SMOKE_OK {} PASS, {} SKIP: {} pairs up and down again, the switch created and removed",
                r.pass,
                r.skip,
                self.o.pairs.len()
            ));
            0
        }
    }
}

fn dry() -> i32 {
    say("sai-teralynx-smoke: the steps, in order (--dry: nothing is loaded or touched)");
    for (i, (n, t)) in STEPS.iter().enumerate() {
        say(&format!("{:>2}. {n:<14} {t}", i + 1));
    }
    say("Every step prints PASS / FAIL / SKIP <step> ...; the last line is SMOKE_OK or SMOKE_FAILED;");
    say("exit 0 = no FAIL, 1 = a FAIL (after the teardown), 2 = usage or refusal.");
    0
}

/// The gate's program.
#[allow(clippy::too_many_lines, reason = "the steps in order")]
pub fn main(args: &[String]) -> i32 {
    let o = match parse(args) {
        Ok(o) => o,
        Err(rc) => return rc,
    };
    if o.dry {
        return dry();
    }
    if !o.confirmed {
        warn(
            "refusing: creating the switch initialises the chip. Stop SONiC (systemctl stop sonic.target) and the\n\
             vendor stack (cs8210-switch), then pass --sonic-is-stopped.",
        );
        return 2;
    }
    let path = o.profile.clone().unwrap_or_default();
    let Some(profile) = read_profile(&path) else {
        warn(&format!("cannot read the profile {path}"));
        return 2;
    };
    let mut map = BTreeMap::new();
    if !o.pairs.is_empty() {
        let mp = o
            .port_map
            .clone()
            .or_else(|| profile.get("SAI_TERALYNX_PORT_MAP").cloned());
        let Some(mp) = mp else {
            warn("no port map: the profile has no SAI_TERALYNX_PORT_MAP (or pass --port-map)");
            return 2;
        };
        let Ok(text) = std::fs::read_to_string(&mp) else {
            warn(&format!("cannot read the port map {mp}"));
            return 2;
        };
        map = port_map(&text);
        for (a, b) in &o.pairs {
            for dp in [a, b] {
                if !map.contains_key(dp) {
                    warn(&format!(
                        "refusing: devport {dp} of --pairs is not in the port map {mp}"
                    ));
                    return 2;
                }
            }
        }
    }
    if syncd_running() {
        warn("refusing: a syncd process runs (SONiC is not stopped)");
        return 2;
    }
    let tee = if o.no_log_capture {
        None
    } else {
        match sai_client::tee::tee_stderr() {
            Ok(t) => Some(t),
            Err(e) => {
                warn(&format!("stderr capture: {e} (the guard step is skipped)"));
                None
            }
        }
    };
    let mut rep = Rep::default();
    let mut s = Step::new("load");
    let lib = o.libsai.clone().unwrap_or_else(sai_client::default_libsai);
    let mut sai = match Sai::load(&lib) {
        Ok(x) => x,
        Err(e) => {
            s.bad(e);
            rep.end(s);
            say(&format!(
                "SMOKE_FAILED {} FAIL, {} PASS, {} SKIP",
                rep.fail, rep.pass, rep.skip
            ));
            return 1;
        }
    };
    let counts: Counts = Arc::new(Mutex::new(HashMap::new()));
    let c2 = Arc::clone(&counts);
    sai_client::set_event_sink(move |e| {
        if let Event::PortState { port, state } = e {
            let mut m = c2.lock().unwrap_or_else(PoisonError::into_inner);
            let x = m.entry(*port).or_insert((0, 0));
            if *state == 1 { x.0 += 1 } else { x.1 += 1 }
        }
    });
    let _ = sai_client::set_profile(profile.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    s.val(format!("libsai={lib}"));
    let ok = s.ok("sai_api_initialize", sai.initialize())
        && [Api::Switch, Api::Port, Api::Hostif]
            .iter()
            .all(|a| s.ok(&format!("sai_api_query {a:?}"), sai.query(*a)));
    let loaded = rep.end(s);
    let mut g = Gate {
        o,
        sai,
        profile,
        map,
        rep,
        sw: 0,
        ports: BTreeMap::new(),
        counts,
        tee,
    };
    if !ok || !loaded {
        return g.finish();
    }
    let mut s = Step::new("profile");
    s.val(format!("keys={}", g.profile.len()));
    for k in [
        "SAI_TERALYNX_LIBIFCS",
        "SAI_TERALYNX_PORT_MAP",
        "SAI_TERALYNX_RUN_DIR",
        "SAI_OPENBCM_DIAG_SOCKET",
    ] {
        if let Some(v) = g.key(k) {
            s.val(format!("{k}={v}"));
        }
    }
    for (a, b) in &g.o.pairs {
        s.val(format!(
            "{a}<->{b}=lanes({},{})",
            g.map.get(a).unwrap_or(&0),
            g.map.get(b).unwrap_or(&0)
        ));
    }
    g.rep.end(s);
    if !g.create_switch("create-switch") {
        for (n, _) in STEPS.iter().skip(3).take(STEPS.len() - 4) {
            g.rep.skip(n, "no switch");
        }
        return g.finish();
    }
    g.guard();
    let ports_ok = g.ports();
    g.regs();
    let links = !g.o.pairs.is_empty() && ports_ok;
    let mut were_up = false;
    if links {
        let before: HashMap<_, _> = g
            .pair_ports()
            .iter()
            .map(|(_, p)| (*p, events(&g.counts, *p).0))
            .collect();
        if g.admin(true) {
            were_up = true;
            if g.oper_up(&before) {
                g.hold();
            } else {
                g.rep.skip("hold", "the links are not up");
            }
        } else {
            were_up = true;
            g.rep.skip("oper-up", "admin-up failed");
            g.rep.skip("hold", "admin-up failed");
        }
        g.port_stats();
    } else {
        let why = if g.o.pairs.is_empty() {
            "--pairs none"
        } else {
            "the ports step failed"
        };
        for n in ["admin-up", "oper-up", "hold", "port-stats"] {
            g.rep.skip(n, why);
        }
    }
    g.temperature();
    g.diag_step();
    if were_up {
        g.admin(false);
    } else {
        g.rep.skip("admin-down", "no port was brought up");
    }
    g.remove_switch("remove-switch");
    g.recreate();
    g.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_and_the_port_map() {
        assert_eq!(parse_pairs("1-2,3-17,31-32"), Some(vec![(1, 2), (3, 17), (31, 32)]));
        assert_eq!(parse_pairs("none"), Some(vec![]));
        assert_eq!(parse_pairs("1-1"), None);
        assert_eq!(parse_pairs("1-2,2-3"), None);
        assert_eq!(parse_pairs("x"), None);
        let m = port_map("# devport lanes speed fec\n1 1,2,3,4,5,6,7,8 400000 rs\n2 9,10 100000 none # c\n");
        assert_eq!(m.get(&1), Some(&1));
        assert_eq!(m.get(&2), Some(&9));
        assert_eq!(STEPS.len(), 16);
    }

    #[test]
    fn options() {
        let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(parse(&a(&["x", "--dry"])).unwrap().dry);
        assert_eq!(parse(&a(&["x", "--sonic-is-stopped"])).unwrap_err(), 2);
        let o = parse(&a(&[
            "x",
            "--profile",
            "p",
            "--pairs",
            "5-6",
            "--hold",
            "4",
            "--recreate",
        ]))
        .unwrap();
        assert_eq!((o.pairs, o.hold, o.recreate), (vec![(5, 6)], 4, true));
        assert_eq!(
            parse(&a(&["x", "--profile", "p", "--link-timeout", "0"])).unwrap_err(),
            2
        );
    }
}

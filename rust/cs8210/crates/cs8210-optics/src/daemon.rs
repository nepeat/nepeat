//! The daemon: the one process that touches the I2C FPGA. Every poll it checks the
//! root port and the FPGA, reads the sideband, applies the cage policy, steps module
//! bring-ups, refreshes monitors, writes the Prometheus textfile; between polls it
//! answers JSON requests on its Unix socket (`SONiC`'s `sonic_platform`, the CLI, the cage
//! guard), one at a time.
//!
//! Cage policy (the "read-only platform" of before is gone; what replaces it):
//! - a declared cage (`--cage N[:PxG]`, i.e. a `SONiC` port uses it) with a module:
//!   channel probed, `ResetL` released, then [`Bringup`] (power class, `ModuleReady`,
//!   application select for the cage's ports);
//! - an undeclared cage with a module, or an empty cage: `ResetL` held (a module is
//!   never powered up in a cage nobody configured; one plugged in later starts held);
//! - `LPMode` is left as it is (deasserted at power-up), except on request.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cs8210_xcvr::bringup::{Bringup, Next, Phase, PortMode};
use cs8210_xcvr::dom;
use cs8210_xcvr::memory::Kind;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::bar::Bar;
use crate::cage::{self, CageIo, CageState};
use crate::fpga::{Fpga, FpgaError, Sideband};
use crate::owner;
use crate::pci;
use crate::regs::Cage;

/// Daemon settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// declared cages and what their ports need
    pub cages: Vec<(Cage, Option<PortMode>)>,
    /// Prometheus textfile (written atomically)
    pub metrics: Option<PathBuf>,
    /// dead marker
    pub dead_file: PathBuf,
    /// poll period (ms)
    pub poll_ms: u64,
    /// monitor refresh period (ms)
    pub dom_ms: u64,
    /// first bring-up step after `ResetL` release (ms)
    pub release_delay_ms: u64,
    /// how long `ResetL` stays asserted on a reset request (ms)
    pub reset_hold_ms: u64,
    /// cache of the root port check for requests (ms)
    pub health_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cages: Vec::new(),
            metrics: None,
            dead_file: PathBuf::from(owner::DEAD_FILE),
            poll_ms: 1_000,
            dom_ms: 10_000,
            release_delay_ms: 1_500,
            reset_hold_ms: 300,
            health_ms: 250,
        }
    }
}

/// A request on the socket (one JSON object per line).
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Sideband,
    Status { cage: Option<u8> },
    Read { cage: u8, offset: usize, len: usize },
    Write { cage: u8, offset: usize, data: String },
    Lpmode { cage: u8, on: bool },
    Reset { cage: u8 },
    Retry { cage: u8 },
}

fn log(msg: &str) {
    eprintln!("cs8210-optics: {msg}");
}

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::with_capacity(2 * b.len()), |mut s, x| {
        let _ = write!(s, "{x:02x}");
        s
    })
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("odd number of hex digits".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            s.get(i..i + 2)
                .and_then(|d| u8::from_str_radix(d, 16).ok())
                .ok_or_else(|| format!("bad hex at {i}"))
        })
        .collect()
}

/// A counter as a sample value (exact below 2^53, which a counter of transfers never reaches).
#[allow(clippy::cast_precision_loss)]
fn count(x: u64) -> f64 {
    x as f64
}

/// A write from `SONiC` that would change what the bring-up owns: CMIS module control
/// (`LowPwrRequestSW`, software reset: lower byte 26) and page 10h's data path
/// deinit (128-129) and control sets / `Apply_DataPathInit` (143-255). xcvrd runs with
/// `--skip_cmis_mgr`, so none of these come from `SONiC` normally; Tx disable (page 10h
/// 130), VDM (2Fh), CDB (9Fh) and the rest stay writable.
fn reserved(kind: Kind, offset: usize, len: usize) -> Option<String> {
    if !matches!(kind, Kind::Cmis { .. }) {
        return None;
    }
    let dp = 128 * usize::from(cs8210_xcvr::cmis::reg::PAGE_DP_CONTROL);
    let owned = |o: usize| o == 26 || (dp + 128..dp + 130).contains(&o) || (dp + 143..dp + 256).contains(&o);
    (offset..offset + len)
        .find(|&o| owned(o))
        .map(|o| format!("flat offset {o} (module control / data path config) belongs to cs8210-optics' bring-up"))
}

fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("@{secs}")
}

/// The daemon state.
pub struct Daemon<B> {
    cfg: Config,
    fpga: Fpga<B>,
    pci: Option<pci::Device>,
    cages: Vec<CageState>,
    sb: Sideband,
    epoch: Instant,
    last_health: Option<u64>,
    next_tick: u64,
    next_metrics: u64,
    marker_written: bool,
}

impl<B: Bar> Daemon<B> {
    /// Over `fpga`; `pci` None in simulation (no root port to check).
    pub fn new(cfg: Config, fpga: Fpga<B>, pci: Option<pci::Device>) -> Self {
        let cages = Cage::all()
            .map(|c| {
                let decl = cfg.cages.iter().find(|(d, _)| *d == c);
                CageState::new(c, decl.is_some(), decl.and_then(|(_, m)| *m))
            })
            .collect();
        Self {
            cfg,
            fpga,
            pci,
            cages,
            sb: Sideband::default(),
            epoch: Instant::now(),
            last_health: None,
            next_tick: 0,
            next_metrics: 0,
            marker_written: false,
        }
    }

    /// Milliseconds since start (monotonic).
    #[must_use]
    pub fn now(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Why the FPGA is unusable, if it is.
    #[must_use]
    pub fn dead(&self) -> Option<&str> {
        self.fpga.dead()
    }

    /// The FPGA (tests).
    pub fn fpga_mut(&mut self) -> &mut Fpga<B> {
        &mut self.fpga
    }

    /// The FPGA stopped answering: no further access by any process (marker file).
    fn die(&mut self, why: &str) {
        self.fpga.mark_dead(why.to_string());
        if self.marker_written {
            return;
        }
        self.marker_written = true;
        log(&format!("I2C FPGA not answering, no further access: {why}"));
        let bdf = self.pci.as_ref().map_or("sim", |p| p.bdf.as_str()).to_string();
        if let Err(e) = owner::mark_dead(&self.cfg.dead_file, &bdf, why, &utc_now()) {
            log(&format!("{}: {e}", self.cfg.dead_file.display()));
        }
    }

    /// Root port check (cached for `health_ms` unless `force`); false when dead.
    fn healthy(&mut self, force: bool) -> bool {
        if self.fpga.dead().is_some() {
            return false;
        }
        let now = self.now();
        if !force
            && self
                .last_health
                .is_some_and(|t| now.saturating_sub(t) < self.cfg.health_ms)
        {
            return true;
        }
        self.last_health = Some(now);
        if let Some(why) = owner::dead_reason(&self.cfg.dead_file) {
            self.fpga.mark_dead(format!("marked dead: {why}"));
            self.marker_written = true;
            return false;
        }
        if let Some(why) = self.pci.as_mut().and_then(pci::Device::upstream_problem) {
            self.die(&format!("root port {why}"));
            return false;
        }
        true
    }

    fn fpga_err(&mut self, i: usize, e: &FpgaError) {
        match e {
            FpgaError::Dead(why) => {
                let why = why.clone();
                self.die(&why);
            }
            FpgaError::Busy(c) => {
                let c = *c;
                if let Some(st) = self.cages.get_mut(i) {
                    st.error(e);
                }
                log(&format!("{c}: transfer stuck, pulsing the channel's master reset"));
                if let Err(e2) = self.fpga.master_reset(c) {
                    log(&format!("{c}: master reset: {e2}"));
                }
            }
            _ => {
                if let Some(st) = self.cages.get_mut(i) {
                    st.error(e);
                }
            }
        }
    }

    /// One poll: health, sideband, policy, bring-ups, monitors, metrics.
    pub fn tick(&mut self) {
        let now = self.now();
        self.next_tick = now + self.cfg.poll_ms;
        if self.healthy(true) {
            match self.fpga.sideband() {
                Ok(sb) => {
                    self.sb = sb;
                    for i in 0..self.cages.len() {
                        if let Err(e) = self.policy(i, now) {
                            self.fpga_err(i, &e);
                        }
                    }
                    self.step_bringups(now);
                    self.refresh(now);
                }
                Err(e) => self.fpga_err(usize::MAX, &e),
            }
        }
        if let Some(t) = self
            .cages
            .iter()
            .filter(|c| c.bringup.is_some())
            .map(|c| c.next_step)
            .min()
        {
            self.next_tick = self.next_tick.min(t.max(now + 20));
        }
        if now >= self.next_metrics {
            self.write_metrics();
            self.next_metrics = now + self.cfg.dom_ms.min(15_000);
        }
    }

    /// When `tick` is due next (ms).
    #[must_use]
    pub fn next_tick(&self) -> u64 {
        self.next_tick
    }

    fn policy(&mut self, i: usize, now: u64) -> Result<(), FpgaError> {
        let Some(st) = self.cages.get_mut(i) else { return Ok(()) };
        let c = st.cage;
        let present = self.sb.present & c.bit() != 0;
        let held = self.sb.reset & c.bit() != 0;
        if present != st.present {
            st.present = present;
            st.forget();
            st.last_error = None;
            log(&format!("{c}: module {}", if present { "inserted" } else { "removed" }));
        }
        if !present || !st.managed {
            if !held {
                self.fpga.set_reset(c, true)?;
                self.sb.reset |= c.bit();
                if present {
                    log(&format!("{c}: module in an undeclared cage: held in reset"));
                }
            }
            return Ok(());
        }
        if held {
            if now < st.hold_until {
                return Ok(());
            }
            if self.fpga.ensure_channel(c)? {
                log(&format!("{c}: I2C channel probed (400 kHz)"));
            }
            self.fpga.set_reset(c, false)?;
            self.sb.reset &= !c.bit();
            st.releases += 1;
            st.forget();
            st.bringup = Some(Bringup::new(st.mode, now));
            st.next_step = now + self.cfg.release_delay_ms;
            log(&format!("{c}: ResetL released"));
        } else if st.bringup.is_none() {
            if self.fpga.ensure_channel(c)? {
                log(&format!("{c}: I2C channel probed (400 kHz)"));
            }
            st.bringup = Some(Bringup::new(st.mode, now));
            st.next_step = now;
            log(&format!("{c}: module already out of reset: adopting it"));
        }
        Ok(())
    }

    fn step_bringups(&mut self, now: u64) {
        for i in 0..self.cages.len() {
            let Some(st) = self.cages.get_mut(i) else { continue };
            let held = self.sb.reset & st.cage.bit() != 0;
            if !st.present || held || now < st.next_step {
                continue;
            }
            let Some(mut b) = st.bringup.take() else { continue };
            let before = b.phase().clone();
            let next = {
                let mut io = CageIo {
                    fpga: &mut self.fpga,
                    cage: st.cage,
                    page: &mut st.page,
                };
                b.step(&mut io, now)
            };
            st.next_step = match next {
                Next::After(ms) => now + ms,
                Next::Idle => u64::MAX,
            };
            st.kind = b.kind().or(st.kind);
            let after = b.phase().clone();
            // A failure retried every minute logs its first attempts and then every 60th.
            let loud = b.attempts() <= 3 || b.attempts() % 60 == 0;
            if after != before && loud {
                match &after {
                    Phase::Failed { reason } => log(&format!(
                        "{}: bring-up failed (attempt {}): {reason}",
                        st.cage,
                        b.attempts()
                    )),
                    p => log(&format!(
                        "{}: {}",
                        st.cage,
                        serde_json::to_string(p).unwrap_or_default()
                    )),
                }
            }
            if after != before && matches!(after, Phase::Ready { .. } | Phase::Failed { .. }) {
                st.next_dom = now;
                st.page0 = None;
                st.degraded = 0;
            }
            st.bringup = Some(b);
            if self.fpga.dead().is_some() {
                return;
            }
        }
    }

    fn refresh(&mut self, now: u64) {
        for i in 0..self.cages.len() {
            let Some(st) = self.cages.get_mut(i) else { continue };
            let settled = matches!(
                st.bringup.as_ref().map(Bringup::phase),
                Some(Phase::Ready { .. } | Phase::Failed { .. })
            );
            if !st.present || self.sb.reset & st.cage.bit() != 0 || !settled || now < st.next_dom {
                continue;
            }
            st.next_dom = now + self.cfg.dom_ms;
            let r = if st.page0.is_none() {
                cage::read_identity(&mut self.fpga, st)
            } else {
                Ok(())
            }
            .and_then(|()| cage::read_monitors(&mut self.fpga, st));
            if let Err(e) = r {
                self.fpga_err(i, &e);
                continue;
            }
            // A module that was up and left ModuleReady / DataPathActivated by itself (a
            // power glitch, an LPMode cycle, firmware) is brought up again; two reads in a
            // row (one monitor period apart) so a transition in flight is not counted.
            // Not while LPMode is asserted on purpose.
            let lp = self.sb.lpmode & st.cage.bit() != 0;
            let up = st.up();
            let off_state = st.module_state.as_deref().is_some_and(|s| s != "Ready")
                || st.datapath.iter().any(|d| d != "Activated");
            st.degraded = if up && off_state && !lp { st.degraded + 1 } else { 0 };
            if st.degraded >= 2 {
                log(&format!(
                    "{}: module left its state (module {:?}, data path {:?}): bringing it up again",
                    st.cage, st.module_state, st.datapath
                ));
                st.degraded = 0;
                st.bringup = Some(Bringup::new(st.mode, now));
                st.next_step = now;
                st.page = None;
            }
        }
    }

    /// Answer one request line.
    pub fn handle(&mut self, line: &str) -> Value {
        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => return json!({"ok": false, "error": format!("bad request: {e}")}),
        };
        match self.request(req) {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.insert("ok".into(), Value::Bool(true));
                }
                v
            }
            Err(e) => json!({"ok": false, "error": e}),
        }
    }

    fn cage_index(cage: u8) -> Result<usize, String> {
        Cage::new(cage)
            .map(Cage::channel)
            .ok_or_else(|| format!("no cage {cage} (1..=32)"))
    }

    fn usable(&mut self, i: usize, write: bool) -> Result<(), String> {
        if !self.healthy(false) {
            return Err(format!("I2C FPGA unavailable: {}", self.fpga.dead().unwrap_or("?")));
        }
        let st = self.cages.get(i).ok_or("no such cage")?;
        let c = st.cage;
        if self.sb.present & c.bit() == 0 {
            return Err(format!("{c}: no module"));
        }
        if self.sb.reset & c.bit() != 0 {
            return Err(format!("{c}: module held in reset"));
        }
        if write && !st.managed {
            return Err(format!("{c}: not a declared cage: writes refused"));
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // one arm per request kind, each short
    fn request(&mut self, req: Request) -> Result<Value, String> {
        match req {
            Request::Sideband => Ok(json!({
                "present": self.sb.present, "reset": self.sb.reset, "lpmode": self.sb.lpmode,
                "interrupt": self.sb.interrupt, "dead": self.fpga.dead(), "version": self.fpga.version(),
            })),
            Request::Status { cage } => {
                let sb = self.sb;
                let list: Vec<Value> = match cage {
                    Some(n) => vec![
                        self.cages
                            .get(Self::cage_index(n)?)
                            .map(|c| c.json(sb))
                            .ok_or("no such cage")?,
                    ],
                    None => self
                        .cages
                        .iter()
                        .filter(|c| c.present || c.managed)
                        .map(|c| c.json(sb))
                        .collect(),
                };
                Ok(json!({"dead": self.fpga.dead(), "version": self.fpga.version(), "cages": list}))
            }
            Request::Read { cage, offset, len } => {
                let i = Self::cage_index(cage)?;
                if len == 0 || len > 4096 {
                    return Err("len must be 1..=4096".into());
                }
                self.usable(i, false)?;
                let st = self.cages.get_mut(i).ok_or("no such cage")?;
                let mut io = CageIo {
                    fpga: &mut self.fpga,
                    cage: st.cage,
                    page: &mut st.page,
                };
                let r = io.kind(st.kind).map_err(|e| e.to_string()).and_then(|k| {
                    st.kind = Some(k);
                    io.read_flat(k, offset, len)
                });
                match r {
                    Ok(data) => Ok(json!({"data": hex(&data)})),
                    Err(e) => {
                        st.i2c_errors += 1;
                        st.page = None;
                        if self.fpga.dead().is_some() {
                            let why = self.fpga.dead().unwrap_or("").to_string();
                            self.die(&why);
                        }
                        Err(e)
                    }
                }
            }
            Request::Write { cage, offset, data } => {
                let i = Self::cage_index(cage)?;
                let bytes = unhex(&data)?;
                if bytes.is_empty() || bytes.len() > 256 {
                    return Err("1..=256 bytes".into());
                }
                self.usable(i, true)?;
                let st = self.cages.get_mut(i).ok_or("no such cage")?;
                let mut io = CageIo {
                    fpga: &mut self.fpga,
                    cage: st.cage,
                    page: &mut st.page,
                };
                let k = io.kind(st.kind).map_err(|e| e.to_string())?;
                st.kind = Some(k);
                if let Some(why) = reserved(k, offset, bytes.len()) {
                    return Err(format!("{}: {why}", st.cage));
                }
                io.write_flat(k, offset, &bytes)?;
                log(&format!(
                    "{}: {} byte(s) written at flat offset {offset} on request",
                    st.cage,
                    bytes.len()
                ));
                Ok(json!({}))
            }
            Request::Lpmode { cage, on } => {
                let i = Self::cage_index(cage)?;
                self.usable(i, true)?;
                let c = Cage::new(cage).ok_or("no such cage")?;
                self.fpga.set_lpmode(c, on).map_err(|e| e.to_string())?;
                if on {
                    self.sb.lpmode |= c.bit();
                } else {
                    self.sb.lpmode &= !c.bit();
                }
                log(&format!(
                    "{c}: LPMode {} on request",
                    if on { "asserted" } else { "deasserted" }
                ));
                Ok(json!({}))
            }
            Request::Reset { cage } => {
                let i = Self::cage_index(cage)?;
                self.usable(i, true)?;
                let now = self.now();
                let c = Cage::new(cage).ok_or("no such cage")?;
                self.fpga.set_reset(c, true).map_err(|e| e.to_string())?;
                self.sb.reset |= c.bit();
                if let Some(st) = self.cages.get_mut(i) {
                    st.forget();
                    st.hold_until = now + self.cfg.reset_hold_ms;
                }
                self.next_tick = now + self.cfg.reset_hold_ms;
                log(&format!("{c}: reset on request"));
                Ok(json!({}))
            }
            Request::Retry { cage } => {
                let i = Self::cage_index(cage)?;
                let now = self.now();
                let st = self.cages.get_mut(i).ok_or("no such cage")?;
                if st.bringup.is_none() {
                    return Err(format!("{}: no bring-up to retry", st.cage));
                }
                st.bringup = Some(Bringup::new(st.mode, now));
                st.next_step = now;
                self.next_tick = now;
                Ok(json!({}))
            }
        }
    }

    /// The Prometheus textfile: the board-neutral `switch_optic_*` names of the `ai_re`
    /// switch-optics dashboard, plus `cs8210_optics_*`.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one block per metric family
    pub fn metrics(&self) -> String {
        let mut m = crate::metrics::Writer::default();
        let alive = self.fpga.dead().is_none();
        m.gauge(
            "switch_optic_scrape_success",
            "1 if the I2C FPGA answers (the cages can be read).",
            &[],
            f64::from(u8::from(alive)),
        );
        m.gauge(
            "cs8210_optics_fpga_up",
            "1 while the I2C FPGA answers; 0 once marked dead (power cycle to recover).",
            &[],
            f64::from(u8::from(alive)),
        );
        for st in &self.cages {
            let port = st.cage.number().to_string();
            let present = alive && self.sb.present & st.cage.bit() != 0;
            m.gauge(
                "switch_optic_present",
                "1 if a module is in the cage.",
                &[("port", &port)],
                f64::from(u8::from(present)),
            );
            m.gauge(
                "cs8210_optics_reset_held",
                "1 while the cage's ResetL is asserted.",
                &[("port", &port)],
                f64::from(u8::from(self.sb.reset & st.cage.bit() != 0)),
            );
            m.counter(
                "cs8210_optics_i2c_errors_total",
                "Failed module transfers.",
                &[("port", &port)],
                count(st.i2c_errors),
            );
            m.counter(
                "cs8210_optics_reset_releases_total",
                "ResetL releases by the daemon.",
                &[("port", &port)],
                count(st.releases),
            );
            if !present {
                continue;
            }
            if let Some(b) = &st.bringup {
                let (ready, failed) = (
                    matches!(b.phase(), Phase::Ready { .. }),
                    matches!(b.phase(), Phase::Failed { .. }),
                );
                m.gauge(
                    "cs8210_optics_module_ready",
                    "1 once the module's bring-up is done.",
                    &[("port", &port)],
                    f64::from(u8::from(ready)),
                );
                m.gauge(
                    "cs8210_optics_bringup_failed",
                    "1 if the module's bring-up failed.",
                    &[("port", &port)],
                    f64::from(u8::from(failed)),
                );
            }
            if let Some(info) = Self::info_labels(st) {
                let labels: Vec<(&str, &str)> = info.iter().map(|(k, v)| (*k, v.as_str())).collect();
                m.gauge(
                    "switch_optic_info",
                    "Front-panel module identity; always 1.",
                    &labels,
                    1.0,
                );
            }
            let Some(mon) = &st.monitors else { continue };
            if let Some(t) = mon.temperature_c {
                m.gauge(
                    "switch_optic_temperature_celsius",
                    "Module case temperature.",
                    &[("port", &port)],
                    t,
                );
            }
            if let Some(v) = mon.voltage_v {
                m.gauge(
                    "switch_optic_supply_voltage_volts",
                    "Module supply voltage.",
                    &[("port", &port)],
                    v,
                );
            }
            for l in &mon.lanes {
                let lane = l.lane.to_string();
                let lab = [("port", port.as_str()), ("lane", lane.as_str())];
                let mut put = |name: &str, help: &str, v: Option<f64>, scale: f64| {
                    if let Some(v) = v {
                        m.gauge(name, help, &lab, v * scale);
                    }
                };
                put(
                    "switch_optic_rx_power_watts",
                    "Per-lane received optical power.",
                    l.rx_power_mw,
                    0.001,
                );
                put(
                    "switch_optic_tx_power_watts",
                    "Per-lane transmitted optical power.",
                    l.tx_power_mw,
                    0.001,
                );
                put(
                    "switch_optic_tx_bias_amperes",
                    "Per-lane laser bias current.",
                    l.tx_bias_ma,
                    0.001,
                );
                put(
                    "cs8210_optics_rx_los",
                    "Per-lane Rx LOS flag (latched by the module).",
                    l.rx_los.map(|b| f64::from(u8::from(b))),
                    1.0,
                );
                put(
                    "cs8210_optics_tx_fault",
                    "Per-lane Tx fault flag (latched).",
                    l.tx_fault.map(|b| f64::from(u8::from(b))),
                    1.0,
                );
                put(
                    "cs8210_optics_rx_power_dbm",
                    "Per-lane received optical power (dBm).",
                    l.rx_power_mw.and_then(dom::dbm),
                    1.0,
                );
            }
        }
        m.finish()
    }

    fn info_labels(st: &CageState) -> Option<Vec<(&'static str, String)>> {
        let p0 = st.page0.as_deref()?;
        let port = st.cage.number().to_string();
        match st.kind? {
            cs8210_xcvr::memory::Kind::Cmis { .. } => {
                let id = cs8210_xcvr::cmis::identity(p0)?;
                let sel = st.active_apsel.first().map_or(1, |c| c >> 4).max(1);
                let compliance = id
                    .applications
                    .iter()
                    .find(|a| a.sel == sel)
                    .map(|a| cs8210_xcvr::cmis::describe(a, id.media_type_code))
                    .unwrap_or_default();
                Some(vec![
                    ("port", port),
                    ("type", id.form_factor),
                    ("vendor", id.vendor),
                    ("part", id.part_number),
                    ("rev", id.revision),
                    ("serial", id.serial),
                    ("compliance", compliance),
                    ("media", id.media_type),
                ])
            }
            cs8210_xcvr::memory::Kind::Sff8636 { .. } => {
                let id = cs8210_xcvr::sff8636::identity(p0)?;
                Some(vec![
                    ("port", port),
                    ("type", id.form_factor),
                    ("vendor", id.vendor),
                    ("part", id.part_number),
                    ("rev", id.revision),
                    ("serial", id.serial),
                    ("compliance", format!("ext {:#04x}", id.ext_compliance)),
                    ("media", id.connector),
                ])
            }
            _ => None,
        }
    }

    fn write_metrics(&mut self) {
        let Some(path) = self.cfg.metrics.clone() else { return };
        let text = self.metrics();
        let tmp = path.with_extension("prom.tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
            log(&format!("{}: {e}", path.display()));
        }
    }

    fn serve_conn(&mut self, stream: UnixStream) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let Ok(mut w) = stream.try_clone() else { return };
        let mut r = BufReader::new(stream);
        for _ in 0..256 {
            let mut line = String::new();
            match r.by_ref().take(65_536).read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let resp = self.handle(&line);
            if writeln!(w, "{resp}").is_err() {
                return;
            }
        }
    }

    /// Run until the process is stopped.
    pub fn run(mut self, listener: &UnixListener) -> Result<(), String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        loop {
            let now = self.now();
            if now >= self.next_tick {
                self.tick();
            }
            match listener.accept() {
                Ok((s, _)) => self.serve_conn(s),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    let wait = self.next_tick.saturating_sub(self.now()).clamp(1, 20);
                    std::thread::sleep(Duration::from_millis(wait));
                }
                Err(e) => {
                    log(&format!("accept: {e}"));
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimFpga;
    use cs8210_xcvr::sim::SimModule;

    fn daemon(sim: SimFpga, cages: &[(u8, Option<PortMode>)]) -> Daemon<SimFpga> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cs8210-optics-d-{}-{n}", std::process::id()));
        let cfg = Config {
            cages: cages.iter().map(|(n, m)| (Cage::new(*n).unwrap(), *m)).collect(),
            dead_file: dir.join("dead"),
            release_delay_ms: 0,
            ..Config::default()
        };
        let _ = std::fs::remove_file(&cfg.dead_file);
        Daemon::new(cfg, Fpga::new(sim, Duration::ZERO), None)
    }

    fn settle(d: &mut Daemon<SimFpga>) {
        for _ in 0..200 {
            d.next_tick = 0;
            d.tick();
            for st in &mut d.cages {
                st.next_step = 0;
                st.next_dom = 0;
            }
            if d.cages
                .iter()
                .filter(|c| c.present && c.managed)
                .all(|c| c.up() && c.monitors.is_some())
            {
                return;
            }
        }
    }

    const M400: Option<PortMode> = Some(PortMode { ports: 1, gbps: 400 });

    fn lab() -> SimFpga {
        let mut sim = SimFpga::new();
        sim.insert(1, SimModule::lab_dr4());
        sim.insert(3, SimModule::from_image(cs8210_xcvr::lab::dac_page0()));
        sim.insert(5, SimModule::lab_dr4()); // undeclared
        sim
    }

    #[test]
    fn declared_cages_come_up_undeclared_stay_in_reset() {
        let mut d = daemon(lab(), &[(1, M400), (3, M400), (2, M400)]);
        settle(&mut d);
        let sim = &d.fpga.bar_ref();
        assert_eq!(
            sim.reset_mask() & 0b111,
            0b010,
            "cages 1 and 3 released, empty cage 2 held"
        );
        assert_ne!(sim.reset_mask() & (1 << 4), 0, "undeclared cage 5 held in reset");
        assert_eq!(sim.module(1).unwrap().active_apsel(), [0x10; 8]);
        let s = d.handle(r#"{"op":"status","cage":1}"#);
        assert_eq!(s["ok"], true);
        let c = &s["cages"][0];
        assert_eq!(c["bringup"]["state"]["phase"], "ready");
        assert_eq!(c["identity"]["vendor"], "MOLEX");
        assert_eq!(c["datapath"][0], "Activated");
        assert_eq!(c["monitors"]["lanes"].as_array().unwrap().len(), 4);
        let m = d.metrics();
        assert!(m.contains("switch_optic_present{port=\"1\"} 1"));
        assert!(m.contains("switch_optic_present{port=\"5\"} 1"));
        assert!(m.contains("switch_optic_temperature_celsius{port=\"1\"}"));
        assert!(m.contains("switch_optic_rx_power_watts{port=\"1\",lane=\"4\"}"));
        assert!(m.contains("part=\"1064281405\""));
        assert!(m.contains("compliance=\"400GAUI-8 C2M (Annex 120E) / 400GBASE-DR4 (Cl 124), 8x4\""));
        assert!(
            !m.contains("switch_optic_temperature_celsius{port=\"3\"}"),
            "passive DAC: no monitor"
        );
    }

    #[test]
    fn optoe_reads_and_request_guards() {
        let mut d = daemon(lab(), &[(1, M400), (3, None)]);
        settle(&mut d);
        let r = d.handle(r#"{"op":"read","cage":1,"offset":2304,"len":4}"#);
        assert_eq!(r["data"], "44444444");
        let r = d.handle(r#"{"op":"read","cage":1,"offset":148,"len":10}"#);
        assert_eq!(unhex(r["data"].as_str().unwrap()).unwrap(), b"1064281405");
        let r = d.handle(r#"{"op":"read","cage":3,"offset":256,"len":1}"#);
        assert_eq!(r["ok"], false, "flat module has no page 1: {r}");
        let r = d.handle(r#"{"op":"read","cage":5,"offset":0,"len":1}"#);
        assert!(r["error"].as_str().unwrap().contains("held in reset"));
        let r = d.handle(r#"{"op":"read","cage":2,"offset":0,"len":1}"#);
        assert!(r["error"].as_str().unwrap().contains("no module"));
        let r = d.handle(r#"{"op":"lpmode","cage":5,"on":true}"#);
        assert_eq!(r["ok"], false);
        let r = d.handle(r#"{"op":"write","cage":1,"offset":2178,"data":"00"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let r = d.handle(r#"{"op":"bogus"}"#);
        assert_eq!(r["ok"], false);
        let sb = d.handle(r#"{"op":"sideband"}"#);
        assert_eq!(sb["present"], 0b10101);
    }

    #[test]
    fn reset_request_and_removal() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        let releases = d.cages[0].releases;
        assert_eq!(d.handle(r#"{"op":"reset","cage":1}"#)["ok"], true);
        assert_ne!(d.fpga.bar_ref().reset_mask() & 1, 0);
        std::thread::sleep(Duration::from_millis(350));
        settle(&mut d);
        assert_eq!(d.cages[0].releases, releases + 1);
        assert!(d.cages[0].up());
        d.fpga.bar_mut().remove(1);
        d.tick();
        assert!(!d.cages[0].present);
        assert_ne!(
            d.fpga.bar_ref().reset_mask() & 1,
            0,
            "an emptied cage is held in reset again"
        );
    }

    #[test]
    fn dead_fpga_stops_everything_and_marks_the_file() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        d.fpga.bar_mut().kill();
        d.tick();
        assert!(d.dead().is_some());
        assert!(owner::dead_reason(&d.cfg.dead_file).is_some());
        let reads = d.fpga.bar_ref().reads();
        d.tick();
        let r = d.handle(r#"{"op":"read","cage":1,"offset":0,"len":1}"#);
        assert_eq!(r["ok"], false);
        assert_eq!(d.fpga.bar_ref().reads(), reads, "nothing touches a dead FPGA");
        assert!(d.metrics().contains("cs8210_optics_fpga_up 0"));
        // a new daemon sees the marker and never touches the FPGA either
        let mut d2 = Daemon::new(d.cfg.clone(), Fpga::new(lab(), Duration::ZERO), None);
        d2.tick();
        assert_eq!(d2.fpga.bar_ref().reads(), 0);
    }

    #[test]
    fn restart_adopts_without_writes() {
        let mut d = daemon(lab(), &[(1, M400), (3, M400)]);
        settle(&mut d);
        let sim = d.fpga.into_bar();
        let (w1, w3, releases) = (
            sim.module(1).unwrap().writes(),
            sim.module(3).unwrap().writes(),
            sim.reset_mask(),
        );
        let mut d2 = daemon(sim, &[(1, M400), (3, M400)]);
        settle(&mut d2);
        assert!(d2.cages[0].up() && d2.cages[2].up());
        let sim = d2.fpga.bar_ref();
        assert_eq!(
            (sim.module(1).unwrap().writes(), sim.module(3).unwrap().writes()),
            (w1, w3),
            "no module write"
        );
        assert_eq!(sim.reset_mask(), releases);
        assert_eq!(d2.cages[0].releases, 0, "adopted, not reset");
    }

    #[test]
    fn a_module_that_drops_out_is_brought_up_again() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        d.fpga.bar_mut().module_mut(1).unwrap().glitch();
        assert!(!d.fpga.bar_ref().module(1).unwrap().dp_activated());
        for _ in 0..3 {
            for st in &mut d.cages {
                st.next_dom = 0;
            }
            d.tick();
        }
        settle(&mut d);
        assert!(d.fpga.bar_ref().module(1).unwrap().dp_activated());
        assert!(d.cages[0].up());
    }

    #[test]
    fn lpmode_on_purpose_is_left_alone() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        assert_eq!(d.handle(r#"{"op":"lpmode","cage":1,"on":true}"#)["ok"], true);
        d.fpga.bar_mut().module_mut(1).unwrap().glitch();
        for _ in 0..4 {
            for st in &mut d.cages {
                st.next_dom = 0;
            }
            d.tick();
        }
        assert!(
            !d.fpga.bar_ref().module(1).unwrap().dp_activated(),
            "no bring-up while LPMode is asserted"
        );
    }

    #[test]
    fn a_page_the_module_does_not_have_is_an_error_not_page_0() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        d.fpga.bar_mut().module_mut(1).unwrap().without_page(0x2f);
        let r = d.handle(r#"{"op":"read","cage":1,"offset":6144,"len":1}"#);
        assert_eq!(r["ok"], false, "{r}");
        assert!(r.to_string().contains("no page 2fh"), "{r}");
        let r = d.handle(r#"{"op":"write","cage":1,"offset":6160,"data":"80"}"#);
        assert_eq!(r["ok"], false, "{r}");
        let r = d.handle(r#"{"op":"read","cage":1,"offset":2304,"len":4}"#);
        assert_eq!(r["ok"], true, "page 11h still reads: {r}");
    }

    #[test]
    fn sonic_cannot_write_what_the_bring_up_owns() {
        let mut d = daemon(lab(), &[(1, M400)]);
        settle(&mut d);
        for (off, data) in [(26, "10"), (2176, "ff"), (2176 + 15, "ff"), (2176 + 17, "10")] {
            let r = d.handle(&format!(r#"{{"op":"write","cage":1,"offset":{off},"data":"{data}"}}"#));
            assert_eq!(r["ok"], false, "offset {off}: {r}");
        }
        let r = d.handle(r#"{"op":"write","cage":1,"offset":2178,"data":"00"}"#);
        assert_eq!(r["ok"], true, "Tx disable stays writable: {r}");
    }
}

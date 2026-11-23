//! The external-PHY runtime of the SAI: what `sai_extphy.cpp` of the C++ sai-openbcm does around
//! its table parser and sequence executor (those are `phy-seq`). A board whose front-panel
//! ports end at another chip the SDK has no driver for (the AS7772's Evora retimer / `MACsec`
//! PHYs) gives a table (`ext-phy 1`, `SAI_OPENBCM_EXT_PHY_FILE`) with up to three clause-45
//! sequences per port; this crate runs them:
//!
//! | C++ (`ext_phy.h`) | here |
//! |---|---|
//! | `ext_phy_load` | [`ExtPhy::load`] (file read and parsed before the backend starts) |
//! | `ext_phy_attach` | [`ExtPhy::attach`]: first lanes to ports, the bus handle; the SAI layer then probes the bus and registers the gates |
//! | `ext_phy_detach` | [`ExtPhy::gated_ports`] (gates to unregister, without this crate's lock) + [`ExtPhy::clear`] |
//! | `ext_phy_gate` | [`ExtPhy::gate`]: the verdict for the backend's link scan |
//! | `ext_phy_admin` | [`ExtPhy::admin`]: admin-up before the ASIC port is enabled, admin-down after it is disabled |
//! | `ext_phy_foreign_mdio` | [`ExtPhy::foreign_mdio`]: raw MDIO of the diag socket, under the lock, starts the hold-off |
//! | `ext_phy_locked` | [`ExtPhy::locked`]: the external `MACsec` table's sequences, under the lock |
//! | `ext_phy_status` | [`ExtPhy::status`]: the diag socket's `extphy` text |
//!
//! # Rules (the C++'s, kept)
//!
//! - **Link** = ASIC link AND the port's line-side conditions: the gate says
//!   [`Verdict::Down`] or [`Verdict::NoOpinion`] (use the ASIC link). No opinion without a
//!   table, for a port without a link sequence, and while the port is admin down.
//! - **Debounce**: one failed poll (a `C` that does not hold, a fault, an MDIO error) takes
//!   the port down at once; up needs `up-polls` (2) good polls in a row, `poll-ms` (500)
//!   apart. Between polls, and during the hold-off, the last verdict answers.
//! - **Hold-off**: raw MDIO on the diag socket pauses the polls `holdoff-ms` (3000) after the
//!   last one.
//! - **Admin**: every admin change starts the port from "line side down". A sequence that
//!   stops is logged and counted; the SAI call still succeeds.
//! - **One lock** of its own for all of it. The gate runs on the backend's link-scan thread:
//!   it takes this lock and the bus handle's, never the SAI lock. Lock order: SAI lock, then
//!   this one, then the bus master's.
//! - **Bus**: clause 45 only. Devices are [`phy_seq::Dev`] (1..=31); the board table's
//!   device-1-only rule is the table's (`ext-phy.nix`).
//!
//! # Differences from the C++
//!
//! - Time comes from an injected [`Clock`] (tests move it; [`SystemClock`] spins waits up to
//!   2 ms and sleeps longer ones, as `wait_us` does).
//! - The state is one value per switch ([`ExtPhy`], cheap to clone: one shared runtime), not
//!   a process global; a new switch gets a new one (the C++ resets its global at load).
//! - The status text always has the "rust (phy-seq)" engine line (the C++ prints it only when
//!   built with the Rust engine; this library has no other).
//! - **Deliberate improvement: the hold-off also defers the external `MACsec` table's polls**
//!   ([`ExtPhy::macsec_poll_deferred`]: `pn.*` and `stats-*` runs that syncd's counter
//!   polls ask for). The C++ held off its link polls only, so a PN / counter read could land
//!   between the requests of a tool's multi-register PHY firmware-message conversation (seen on
//!   the AS7772: the read gets 0x0000, the tool "FW Message failure"). While deferred the model
//!   answers from its last read; one poll sequence is deferred at most
//!   [`MACSEC_POLL_DEFER_MAX_MS`] in a row (then it runs inside the hold-off), so a tool that
//!   polls for minutes cannot hide a PN nearing exhaustion. SA install / remove / gate
//!   sequences and counter clears are never deferred (as in the C++).
#![forbid(unsafe_code)]

mod clock;

pub use clock::{Clock, ManualClock, SystemClock};
pub use phy_seq::{Dev, ExtPhyTable, ParseError};

use phy_seq::{Op, Params, Section};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// A chip logical port (the backend's port number).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChipPort(pub u16);

/// The SAI status a bus master reported for a failed access. Displays as the number: the
/// C++ messages print it with `%d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusStatus(pub i32);

impl fmt::Display for BusStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The clause-45 MDIO master as the runtime uses it: a handle of its own (the gate runs on
/// the backend's link-scan thread, where the SAI layer's backend cannot be borrowed), with
/// its own lock inside.
pub trait PhyBus: Send {
    /// Reads `dev.reg` of the PHY at address word `phy`.
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus>;
    /// Writes `value` to `dev.reg` of the PHY at address word `phy`.
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus>;
}

/// What the gate says about its port on one link-scan pass (C++ `ext_phy_gate`: `0` /
/// `-1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The line side is down: the port is down whatever the ASIC link says.
    Down,
    /// No opinion: the ASIC link decides.
    NoOpinion,
}

/// The SAI API a log line belongs to (the C++ `OB_*` first argument).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// `SAI_API_SWITCH`
    Switch,
    /// `SAI_API_PORT`
    Port,
}

/// Severity of a log line (`OB_ERR`, `OB_WARN`, `OB_NOTICE`, `OB_INFO`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something failed.
    Error,
    /// Something will not behave as asked.
    Warn,
    /// A state change.
    Notice,
    /// Detail.
    Info,
}

/// Receives the runtime's log lines (called under the runtime's lock: it must not call back).
pub type Logger = Arc<dyn Fn(Api, Level, &str) + Send + Sync>;

/// A front-panel port of the chip, as attach resolves the table's lanes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortRef {
    /// The chip's port.
    pub hw: ChipPort,
    /// Its first lane (`None` for a port without lanes).
    pub first_lane: Option<u32>,
    /// The SDK's name (`ce0`), for log lines and the status text.
    pub name: String,
}

/// Why the table could not be loaded; the switch is not created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The file cannot be read (C++: `SAI_STATUS_FAILURE`).
    CannotOpen,
    /// The text is not a table (C++: `SAI_STATUS_INVALID_PARAMETER`); the parser's message.
    Parse(String),
}

/// A table port no chip port starts at (C++: `SAI_STATUS_INVALID_PARAMETER`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoPortAtLane {
    /// The table line of the `port` line.
    pub line: u32,
    /// Its lane.
    pub lane: u32,
}

/// The counters of one port (the status text's columns).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortCounters {
    /// Link polls run.
    pub polls: u64,
    /// Polls that ended in a fault or an MDIO error.
    pub faults: u64,
    /// Debounced line-side changes to down.
    pub downs: u64,
    /// Debounced line-side changes to up.
    pub ups: u64,
    /// Admin sequences run.
    pub hooks: u64,
    /// Admin sequences that stopped.
    pub hook_fail: u64,
    /// Duration of the last poll, us.
    pub last_us: u64,
    /// Longest poll, us.
    pub max_us: u64,
    /// All polls, us (the average's numerator).
    pub total_us: u64,
    /// Longest admin sequence, us.
    pub hook_max_us: u64,
}

/// The runtime state of one port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortState {
    /// Admin state as the SAI layer last set it.
    pub admin: bool,
    /// The debounced line-side state.
    pub ext_up: bool,
    /// The last poll ended in a fault.
    pub fault: bool,
    /// Good polls in a row.
    pub good: u32,
    /// The value of the last `C` read.
    pub last_val: u16,
}

/// The longest one `MACsec` poll sequence (a port's `pn.<dir>.<an>`, `stats-sa.*`,
/// `stats-flow.*`) is deferred in a row by the raw-MDIO hold-off: then it runs anyway, inside
/// the hold-off, and the next deferral starts again. Bounds how stale a PN read can be while
/// a tool polls the PHY continuously (syncd asks for the PN about once a second).
pub const MACSEC_POLL_DEFER_MAX_MS: u64 = 10_000;

/// The most good polls counted (the C++ `1000000`).
const GOOD_MAX: u32 = 1_000_000;

#[derive(Debug, Default)]
struct PortRt {
    hw: Option<ChipPort>,
    name: String,
    state: PortState,
    next_poll_ms: u64,
    counters: PortCounters,
}

struct State {
    table: ExtPhyTable,
    ports: Vec<PortRt>,
    by_hw: BTreeMap<ChipPort, usize>,
    /// Set by attach: the C++ `g_be` (the table is active).
    bus: Option<Box<dyn PhyBus>>,
    hold_until_ms: u64,
    holdoffs: u64,
    /// `MACsec` poll sequences deferred by the current hold-off: when each was first deferred.
    macsec_deferred: BTreeMap<(ChipPort, String), u64>,
    /// The hold-off (its `holdoffs` number) the deferral notice was logged for.
    macsec_deferred_logged: u64,
    macsec_counts: MacsecPollCounts,
}

/// What the hold-off did to the external `MACsec` table's polls (since the runtime was made).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacsecPollCounts {
    /// Polls answered from the model's last read.
    pub deferred: u64,
    /// Polls run inside a hold-off because [`MACSEC_POLL_DEFER_MAX_MS`] had passed.
    pub forced: u64,
}

struct Shared {
    state: Mutex<State>,
    /// Replaceable (tests: a manual clock once the switch is up).
    clock: Mutex<Arc<dyn Clock>>,
    log: Logger,
}

/// The runtime of one switch's external-PHY table. Clones share it (the gates hold clones).
#[derive(Clone)]
pub struct ExtPhy {
    shared: Arc<Shared>,
}

impl fmt::Debug for ExtPhy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let st = self.lock();
        f.debug_struct("ExtPhy")
            .field("ports", &st.ports.len())
            .field("active", &st.bus.is_some())
            .finish_non_exhaustive()
    }
}

/// One sequence's outcome as the runtime counts it (the C++ `SeqResult`).
struct SeqOutcome {
    /// A compare / poll that did not match, or an MDIO error.
    fault: bool,
    /// Every `C` held.
    cond: bool,
    /// The value of the last `C` read.
    last: u16,
    /// Why it stopped (the C++ message).
    why: String,
}

/// The executor's view of the bus: the handle, with the clock's waits.
struct OnBus<'a> {
    bus: &'a mut dyn PhyBus,
    clock: &'a dyn Clock,
}

impl phy_seq::Bus for OnBus<'_> {
    type Error = BusStatus;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        self.bus.read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        self.bus.write(phy, dev, reg, value)
    }
    fn delay_us(&mut self, us: u32) {
        self.clock.wait_us(us);
    }
}

/// Runs an ext-phy sequence (fixed PHYs, constant values: no parameters).
fn run_seq(bus: &mut dyn PhyBus, clock: &dyn Clock, ops: &[Op]) -> SeqOutcome {
    match phy_seq::run(&mut OnBus { bus, clock }, ops, 0, &Params::new()) {
        Ok(out) => SeqOutcome {
            fault: false,
            cond: out.cond,
            last: out.last,
            why: String::new(),
        },
        Err(e) => SeqOutcome {
            fault: true,
            cond: false,
            last: 0,
            why: e.ext_phy_message(),
        },
    }
}

impl ExtPhy {
    /// A switch without a table: inactive, every gate without an opinion.
    pub fn inactive(clock: Arc<dyn Clock>, log: Logger) -> Self {
        Self::with_table(ExtPhyTable::default(), clock, log)
    }

    /// The runtime of a parsed table, not attached yet.
    pub fn with_table(table: ExtPhyTable, clock: Arc<dyn Clock>, log: Logger) -> Self {
        let ports = table.ports().iter().map(|_| PortRt::default()).collect();
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    table,
                    ports,
                    by_hw: BTreeMap::new(),
                    bus: None,
                    hold_until_ms: 0,
                    holdoffs: 0,
                    macsec_deferred: BTreeMap::new(),
                    macsec_deferred_logged: 0,
                    macsec_counts: MacsecPollCounts::default(),
                }),
                clock: Mutex::new(clock),
                log,
            }),
        }
    }

    /// The runtime's clock.
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.shared.clock.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Replaces the runtime's clock (tests).
    pub fn set_clock(&self, clock: Arc<dyn Clock>) {
        *self.shared.clock.lock().unwrap_or_else(PoisonError::into_inner) = clock;
    }

    /// `ext_phy_load`: reads and parses the table at `path` (`SAI_OPENBCM_EXT_PHY_FILE`;
    /// unset or empty: no table). Logs why a table is refused, as the C++ does.
    pub fn load(path: Option<&str>, clock: Arc<dyn Clock>, log: Logger) -> Result<Self, LoadError> {
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return Ok(Self::inactive(clock, log));
        };
        let Ok(text) = std::fs::read(path) else {
            log(
                Api::Switch,
                Level::Error,
                &format!("SAI_OPENBCM_EXT_PHY_FILE {path}: cannot open; refusing to create the switch"),
            );
            return Err(LoadError::CannotOpen);
        };
        match ExtPhyTable::parse(&text) {
            Ok(table) => Ok(Self::with_table(table, clock, log)),
            Err(e) => {
                let mut msg = Vec::new();
                e.write_to(&mut msg);
                let msg = String::from_utf8_lossy(&msg).into_owned();
                log(
                    Api::Switch,
                    Level::Error,
                    &format!("SAI_OPENBCM_EXT_PHY_FILE {path}: {msg}; refusing to create the switch"),
                );
                Err(LoadError::Parse(msg))
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic under the lock (none is possible in this code) leaves whole fields behind.
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn log(&self, api: Api, level: Level, text: &str) {
        (self.shared.log)(api, level, text);
    }

    /// Whether the table has ports at all (a table without ports is never attached).
    pub fn has_ports(&self) -> bool {
        !self.lock().table.ports().is_empty()
    }

    /// `ext_phy_attach`'s first half: each table port is the last chip port whose first lane
    /// is the table's; the bus handle is kept (the table is then active). Returns the ports
    /// with a link sequence (to be gated). A table without ports stays inactive and returns
    /// none. A lane no port starts at: logged, nothing attached.
    pub fn attach(&self, ports: &[PortRef], bus: Box<dyn PhyBus>) -> Result<Vec<ChipPort>, NoPortAtLane> {
        let mut gated = Vec::new();
        let missing = {
            let mut guard = self.lock();
            let st = &mut *guard;
            if st.table.ports().is_empty() {
                return Ok(gated);
            }
            let mut missing = None;
            for (i, (tp, rt)) in st.table.ports().iter().zip(st.ports.iter_mut()).enumerate() {
                if let Some(p) = ports.iter().rfind(|p| p.first_lane == Some(tp.lane())) {
                    rt.hw = Some(p.hw);
                    rt.name.clone_from(&p.name);
                }
                let Some(hw) = rt.hw else {
                    missing = Some(NoPortAtLane {
                        line: tp.line(),
                        lane: tp.lane(),
                    });
                    break;
                };
                st.by_hw.insert(hw, i);
                if !tp.ops(Section::Link).is_empty() {
                    gated.push(hw);
                }
            }
            if missing.is_none() {
                st.bus = Some(bus);
            }
            missing
        };
        if let Some(m) = missing {
            self.log(
                Api::Switch,
                Level::Error,
                &format!("external PHY table line {}: no port starts at lane {}", m.line, m.lane),
            );
            return Err(m);
        }
        Ok(gated)
    }

    /// The attach notice, once the gates are registered.
    pub fn log_attached(&self, gated: usize) {
        let text = {
            let st = self.lock();
            format!(
                "external PHY table: {} ports ({gated} with a line-side link poll), poll {} ms, up after {} good polls, hold-off {} ms",
                st.table.ports().len(),
                st.table.poll_ms(),
                st.table.up_polls(),
                st.table.holdoff_ms()
            )
        };
        self.log(Api::Switch, Level::Notice, &text);
    }

    /// The ports whose gate an active table registered: what detach unregisters (without
    /// this lock: the backend may be waiting for a gate call to return). Empty when inactive.
    pub fn gated_ports(&self) -> Vec<ChipPort> {
        let st = self.lock();
        if st.bus.is_none() {
            return Vec::new();
        }
        st.table
            .ports()
            .iter()
            .zip(&st.ports)
            .filter(|(tp, _)| !tp.ops(Section::Link).is_empty())
            .filter_map(|(_, rt)| rt.hw)
            .collect()
    }

    /// `ext_phy_detach`'s second half: forgets the table and the bus.
    pub fn clear(&self) {
        let mut st = self.lock();
        st.bus = None;
        st.by_hw.clear();
        st.table = ExtPhyTable::default();
        st.ports.clear();
        st.macsec_deferred.clear();
    }

    /// `ext_phy_active`: a table is attached.
    pub fn active(&self) -> bool {
        self.lock().bus.is_some()
    }

    /// `ext_phy_admin`: the port's admin state changes. Call with `up` before enabling the
    /// ASIC port, with `!up` after disabling it. Restarts the debounce from "down" and runs
    /// the port's admin-up / admin-down sequence (if it has one).
    pub fn admin(&self, hw: ChipPort, up: bool) {
        let clock = self.clock();
        let clock = &*clock;
        let mut guard = self.lock();
        let st = &mut *guard;
        let (Some(bus), Some(&i)) = (st.bus.as_mut(), st.by_hw.get(&hw)) else {
            return;
        };
        let (Some(tp), Some(rt)) = (st.table.ports().get(i), st.ports.get_mut(i)) else {
            return;
        };
        rt.state.admin = up;
        rt.state.ext_up = false;
        rt.state.good = 0;
        rt.next_poll_ms = 0;
        let ops = tp.ops(if up { Section::AdminUp } else { Section::AdminDown });
        if ops.is_empty() {
            return;
        }
        let t0 = clock.now_us();
        let r = run_seq(bus.as_mut(), clock, ops);
        let dt = clock.now_us().saturating_sub(t0);
        rt.counters.hooks += 1;
        rt.counters.hook_max_us = rt.counters.hook_max_us.max(dt);
        let dir = if up { "up" } else { "down" };
        let (level, text) = if r.fault {
            rt.counters.hook_fail += 1;
            (
                Level::Error,
                format!("port {}: external PHY admin-{dir} sequence stopped: {}", rt.name, r.why),
            )
        } else {
            (
                Level::Info,
                format!("port {}: external PHY admin-{dir} sequence done ({dt} us)", rt.name),
            )
        };
        drop(guard);
        self.log(Api::Port, level, &text);
    }

    /// `ext_phy_gate`: asked by the backend's link scan, per pass and port.
    pub fn gate(&self, hw: ChipPort) -> Verdict {
        let clock = self.clock();
        let clock = &*clock;
        let mut guard = self.lock();
        let st = &mut *guard;
        let (Some(bus), Some(&i)) = (st.bus.as_mut(), st.by_hw.get(&hw)) else {
            return Verdict::NoOpinion;
        };
        let (Some(tp), Some(rt)) = (st.table.ports().get(i), st.ports.get_mut(i)) else {
            return Verdict::NoOpinion;
        };
        let link = tp.ops(Section::Link);
        if link.is_empty() || !rt.state.admin {
            return Verdict::NoOpinion;
        }
        let verdict = |rt: &PortRt| {
            if rt.state.ext_up {
                Verdict::NoOpinion
            } else {
                Verdict::Down
            }
        };
        let t0 = clock.now_us();
        let now_ms = t0 / 1000;
        if now_ms < st.hold_until_ms || now_ms < rt.next_poll_ms {
            return verdict(rt);
        }
        let r = run_seq(bus.as_mut(), clock, link);
        let dt = clock.now_us().saturating_sub(t0);
        let c = &mut rt.counters;
        c.polls += 1;
        c.last_us = dt;
        c.total_us = c.total_us.saturating_add(dt);
        c.max_us = c.max_us.max(dt);
        rt.next_poll_ms = now_ms.saturating_add(u64::from(st.table.poll_ms()));
        if !r.fault {
            rt.state.last_val = r.last;
        }
        let mut lines: Vec<(Level, String)> = Vec::new();
        if r.fault != rt.state.fault {
            lines.push(if r.fault {
                (
                    Level::Warn,
                    format!("port {}: external PHY link poll fault: {}", rt.name, r.why),
                )
            } else {
                (
                    Level::Notice,
                    format!("port {}: external PHY link poll answers again", rt.name),
                )
            });
            rt.state.fault = r.fault;
        }
        if r.fault {
            rt.counters.faults += 1;
        }
        if !r.fault && r.cond {
            if rt.state.good < GOOD_MAX {
                rt.state.good += 1;
            }
            if !rt.state.ext_up && rt.state.good >= st.table.up_polls() {
                rt.state.ext_up = true;
                rt.counters.ups += 1;
                lines.push((
                    Level::Notice,
                    format!(
                        "port {}: line side up (external PHY, value 0x{:04x}, {} good polls)",
                        rt.name, r.last, rt.state.good
                    ),
                ));
            }
        } else {
            rt.state.good = 0;
            if rt.state.ext_up {
                rt.state.ext_up = false;
                rt.counters.downs += 1;
                lines.push((
                    Level::Notice,
                    if r.fault {
                        format!("port {}: line side down (external PHY, poll fault)", rt.name)
                    } else {
                        format!(
                            "port {}: line side down (external PHY, value 0x{:04x})",
                            rt.name, r.last
                        )
                    },
                ));
            }
        }
        let v = verdict(rt);
        drop(guard);
        for (level, text) in lines {
            self.log(Api::Port, level, &text);
        }
        v
    }

    /// `ext_phy_foreign_mdio`: runs `f` (raw MDIO of the diag socket) under the lock, so it
    /// never lands inside one of the table's sequences, and pauses the polls for `holdoff-ms`
    /// after it.
    pub fn foreign_mdio<R>(&self, f: impl FnOnce() -> R) -> R {
        let mut st = self.lock();
        let holdoff = u64::from(st.table.holdoff_ms());
        let mut paused = None;
        if st.bus.is_some() && holdoff != 0 {
            let now_ms = self.clock().now_us() / 1000;
            if now_ms >= st.hold_until_ms {
                st.holdoffs += 1;
                paused = Some(holdoff);
            }
            st.hold_until_ms = now_ms.saturating_add(holdoff);
        }
        if let Some(ms) = paused {
            self.log(
                Api::Switch,
                Level::Notice,
                &format!(
                    "external PHY link polls paused: raw MDIO on the diag socket (resume {ms} ms after the last one)"
                ),
            );
        }
        f()
    }

    /// Whether the external `MACsec` table's poll sequence `name` (`pn.*`, `stats-*`: what
    /// syncd's counter polls ask for) on chip port `port` is to be skipped now: the raw-MDIO
    /// hold-off is on (globally, as for the link polls), and this sequence has not been
    /// deferred for [`MACSEC_POLL_DEFER_MAX_MS`] in a row yet. The caller then answers from
    /// its last read (the PHY's counters keep counting, the next read catches up). Not for
    /// sequences `SONiC` asks for (install, remove, gate, clear): those are never deferred.
    /// Logs one notice per hold-off that defers a poll, and one per forced run.
    /// A deliberate improvement on the C++ (its hold-off held the link polls only).
    pub fn macsec_poll_deferred(&self, port: ChipPort, name: &str) -> bool {
        let mut st = self.lock();
        let now_ms = self.clock().now_us() / 1000;
        if st.bus.is_none() || now_ms >= st.hold_until_ms {
            st.macsec_deferred.clear();
            return false;
        }
        let key = (port, name.to_owned());
        let since = *st.macsec_deferred.entry(key.clone()).or_insert(now_ms);
        let waited = now_ms.saturating_sub(since);
        let line = if waited >= MACSEC_POLL_DEFER_MAX_MS {
            st.macsec_deferred.insert(key, now_ms); // read now: the next deferral counts from here
            st.macsec_counts.forced += 1;
            Some(format!(
                "external MACsec poll {name} on chip port {} runs inside the raw-MDIO hold-off: deferred {waited} ms, the most allowed",
                port.0
            ))
        } else {
            st.macsec_counts.deferred += 1;
            let episode = st.holdoffs;
            (st.macsec_deferred_logged != episode).then(|| {
                st.macsec_deferred_logged = episode;
                format!(
                    "external MACsec PN / counter polls deferred: raw MDIO on the diag socket (last values answer until the hold-off ends, at most {MACSEC_POLL_DEFER_MAX_MS} ms per poll)"
                )
            })
        };
        drop(st);
        if let Some(text) = line {
            self.log(Api::Switch, Level::Notice, &text);
        }
        waited < MACSEC_POLL_DEFER_MAX_MS
    }

    /// What the hold-off did to the external `MACsec` table's polls.
    pub fn macsec_poll_counts(&self) -> MacsecPollCounts {
        self.lock().macsec_counts
    }

    /// `ext_phy_locked`: runs `f` (another user of the same PHYs' register interfaces: the
    /// external `MACsec` table's sequences) under the lock, without a hold-off.
    pub fn locked<R>(&self, f: impl FnOnce() -> R) -> R {
        let _st = self.lock();
        f()
    }

    /// The runtime state of a port of the table (`None` for another port).
    pub fn port_state(&self, hw: ChipPort) -> Option<PortState> {
        let st = self.lock();
        st.by_hw.get(&hw).and_then(|&i| st.ports.get(i)).map(|rt| rt.state)
    }

    /// The counters of a port of the table.
    pub fn port_counters(&self, hw: ChipPort) -> Option<PortCounters> {
        let st = self.lock();
        st.by_hw.get(&hw).and_then(|&i| st.ports.get(i)).map(|rt| rt.counters)
    }

    /// `ext_phy_status`: the diag socket's `extphy` text.
    pub fn status(&self) -> String {
        let st = self.lock();
        if st.bus.is_none() {
            return "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n".to_owned();
        }
        let now_ms = self.clock().now_us() / 1000;
        let t = &st.table;
        let mut s = format!(
            "extphy: {} ports, poll {} ms, up after {} good polls, hold-off {} ms ({}, {} so far)\n",
            t.ports().len(),
            t.poll_ms(),
            t.up_polls(),
            t.holdoff_ms(),
            if now_ms < st.hold_until_ms {
                "PAUSED now"
            } else {
                "not paused"
            },
            st.holdoffs
        );
        s.push_str("extphy: table parser and sequence executor: rust (phy-seq)\n");
        s.push_str(
            "port      lane   hw admin line  fault  value    polls   faults  ups downs last_us  avg_us  max_us hooks fail hook_max_us\n",
        );
        for (tp, rt) in t.ports().iter().zip(&st.ports) {
            let c = &rt.counters;
            let hw = rt.hw.map_or(-1, |h| i32::from(h.0));
            let line = if tp.ops(Section::Link).is_empty() {
                "-"
            } else if rt.state.ext_up {
                "up"
            } else {
                "down"
            };
            let _ = writeln!(
                s,
                "{:<8} {:>5} {:>4} {:<5} {:<5} {:<5}  0x{:04x} {:>8} {:>8} {:>4} {:>5} {:>7} {:>7} {:>7} {:>5} {:>4} {:>11}",
                rt.name,
                tp.lane(),
                hw,
                if rt.state.admin { "up" } else { "down" },
                line,
                if rt.state.fault { "FAULT" } else { "no" },
                rt.state.last_val,
                c.polls,
                c.faults,
                c.ups,
                c.downs,
                c.last_us,
                c.total_us.checked_div(c.polls).unwrap_or(0),
                c.max_us,
                c.hooks,
                c.hook_fail,
                c.hook_max_us
            );
        }
        s
    }
}

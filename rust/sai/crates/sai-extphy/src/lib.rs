//! The external-PHY runtime of the SAI: what `sai_extphy.cpp` of the C++ sai-openbcm does around
//! its table parser and sequence executor (those are `phy-seq`), and the MDIO arbitration of
//! every library user of those PHYs. A board whose front-panel ports end at another chip the SDK
//! has no driver for (the AS7772's Evora retimer / `MACsec` PHYs) gives a table (`ext-phy 1`,
//! `SAI_OPENBCM_EXT_PHY_FILE`) with up to three clause-45 sequences per port; this crate runs them:
//!
//! | C++ (`ext_phy.h`) | here |
//! |---|---|
//! | `ext_phy_load` | [`ExtPhy::load`] (file read and parsed before the backend starts) |
//! | `ext_phy_attach` | [`ExtPhy::attach`]: first lanes to ports, the bus handle; the SAI layer then probes the bus and registers the gates |
//! | `ext_phy_detach` | [`ExtPhy::gated_ports`] (gates to unregister, without this crate's locks) + [`ExtPhy::clear`] |
//! | `ext_phy_gate` | [`ExtPhy::gate`]: the verdict for the backend's link scan |
//! | `ext_phy_admin` | [`ExtPhy::admin`]: admin-up before the ASIC port is enabled, admin-down after it is disabled |
//! | `ext_phy_foreign_mdio` | [`ExtPhy::diag_access`]: one raw MDIO access of the diag socket, with or without a transaction |
//! | `ext_phy_locked` | [`ExtPhy::macsec_run`]: one external `MACsec` table sequence on its PHYs |
//! | `ext_phy_status` | [`ExtPhy::status`]: the diag socket's `extphy` text |
//! | (none) | [`ExtPhy::txn_begin`], [`ExtPhy::txn_renew`], [`ExtPhy::txn_end`], [`ExtPhy::leases`]: diag transactions |
//!
//! # Rules (the C++'s, kept)
//!
//! - **Link** = ASIC link AND the port's line-side conditions: the gate says
//!   [`Verdict::Down`] or [`Verdict::NoOpinion`] (use the ASIC link). No opinion without a
//!   table, for a port without a link sequence, and while the port is admin down.
//! - **Debounce**: one failed poll (a `C` that does not hold, a fault, an MDIO error) takes
//!   the port down at once; up needs `up-polls` (2) good polls in a row, `poll-ms` (500)
//!   apart. Between polls, and while a diag lease holds one of the poll's PHYs, the last verdict
//!   answers.
//! - **Admin**: every admin change starts the port from "line side down". A sequence that
//!   stops is logged and counted; the SAI call still succeeds.
//! - **Bus**: clause 45 only. Devices are [`phy_seq::Dev`] (1..=31); the board table's
//!   device-1-only rule is the table's (`ext-phy.nix`).
//!
//! # Serialisation (replaces the C++'s one lock and hold-off)
//!
//! The PHYs' firmware-message and `MACsec` register interfaces are conversations (request words,
//! the doorbell 1.8222, the ack 1.8221 that clears on read, result words): a foreign access in the
//! middle breaks both parties. So every user of a PHY takes that PHY's **slot** for its whole
//! conversation, and different PHYs interleave freely (the SDK serialises single MDIO frames):
//!
//! | user | thread | while a diag lease holds one of its PHYs |
//! |---|---|---|
//! | link poll ([`ExtPhy::gate`]) | SDK link scan | never waits: the cached verdict (counted); behind a library run too (counted; the poll runs on the next pass), unless [`GATE_BUSY_MAX`] in a row: then it waits for that one run |
//! | admin sequence ([`ExtPhy::admin`]) | SAI call | its call waits for a transaction to end or run out ([`LEASE_MAX_MS`]), **outside the SAI lock** (below) |
//! | `MACsec` change ([`MacsecUse::Op`]: SA install / remove, gate, enable, clear) | SAI call | waits, as admin: never dropped, never under the SAI lock |
//! | `MACsec` periodic read ([`MacsecUse::Poll`]: PN, SA / flow stats) | SAI call (syncd's counter thread) | skipped: the model's last read answers (counted, no error); after [`MACSEC_POLL_DEFER_MAX_MS`] of skips its get waits for a transaction to end (outside the SAI lock), then reads (never inside one) |
//! | diag `extmacsec run` ([`MacsecUse::DiagRun`]) | diag socket, SAI lock | refused (the transaction's client is the one who would have to end it) |
//! | diag raw access ([`ExtPhy::diag_access`]) | diag socket, **no** SAI lock for clause 45 | in its transaction: served; without one (bare): refused inside another client's transaction, else an implicit lease of `holdoff-ms` on that PHY (the C++ hold-off, per PHY) |
//!
//! A **transaction** (`extphy txn begin <phys|all> [ms]` on the diag socket) is a lease: it lasts
//! [`LEASE_DEFAULT_MS`] (or what was asked, at most [`LEASE_MAX_MS`]) after its begin and each
//! access, never more than [`LEASE_MAX_MS`] from its begin, so a dead client blocks nothing for
//! longer. A request of a transaction that ran out is refused ("expired": what the PHY did since is
//! unknown). Implicit leases (bare accesses) pause the polls only: `MACsec` changes and admin
//! sequences do not wait for them, and a forced PN read runs between two of their accesses (as
//! the C++ and the old tools behaved). Raw MDIO that names no single PHY (broadcast, clause 22,
//! another bus) takes the **global** slot (every PHY) and, without a transaction, an implicit lease
//! on all PHYs.
//!
//! # SAI calls and the SAI lock
//!
//! **No library path waits for a diag lease while it holds the SAI lock** (the AS7772's
//! "txn expired (1001 ms, 0 accesses)": an SA install waited for a lease under the SAI lock, the
//! lease holder's next request queued behind a diag command that needed that lock; only the expiry
//! broke the loop). A SAI call of the rim runs inside [`call`] and under [`under_sai_lock`]; before
//! its first effect each PHY change **claims** the PHYs it will run on ([`ExtPhy::claim`],
//! [`ExtPhy::claim_port`]; module `call`). A claim is taken at once when no transaction holds those
//! PHYs, and from then on no transaction may begin on them until the call returns, so the call
//! never meets one. When a transaction holds them, the claim is deferred: the call returns at once
//! (nothing done), [`call`] lets go of the SAI lock, waits for that transaction (at most
//! [`LEASE_MAX_MS`]), claims, and runs the call again; the second run re-validates by claiming
//! again (an object that moved meanwhile claims its new PHYs: one more deferral at most per PHY,
//! each bounded). A forced `MACsec` read defers its get the same way. Link notifications, port sets
//! on other ports and diag status commands therefore never wait behind a lease; the change itself
//! is never dropped, never runs inside a transaction, and waits at most [`LEASE_MAX_MS`] per
//! transaction in its way. A library user that would still wait under the SAI lock (a change on
//! PHYs its call did not claim: a gap in the claims) is asserted in debug builds, and counted and
//! logged in release builds ([`ArbCounts::locked_waits`], 0 when the claims are complete).
//!
//! **Lock order** (one global order, enforced by construction): a claim (waited for outside the
//! SAI lock only), then the SAI lock, then a PHY's slot, then the bus master's own lock. A diag
//! lease is a record, not a lock: nobody holds anything while it lives. The runtime's state mutex
//! and the arbiter's mutex are leaves (module `leaf`, checked in debug builds); slots are never held
//! while anything waits; claims are never waited for under the SAI lock (asserted); the gate
//! closure and the diag socket's raw path hold no SAI lock (they have no way to take it); log lines
//! are written after every lock is released.
//!
//! **Hazard, documented and not solved**: the `MACsec` counters clear on read. A diag read of them
//! (transaction or not) takes those counts away from `SONiC`'s accumulated statistics.
//!
//! # Differences from the C++
//!
//! - Time comes from an injected [`Clock`] (tests move it; [`SystemClock`] spins waits up to
//!   2 ms and sleeps longer ones, as `wait_us` does).
//! - The state is one value per switch ([`ExtPhy`], cheap to clone: one shared runtime), not
//!   a process global; a new switch gets a new one (the C++ resets its global at load).
//! - The status text always has the "rust (phy-seq)" engine line (the C++ prints it only when
//!   built with the Rust engine; this library has no other), and after the port table the
//!   arbitration lines (leases, counters).
//! - The C++ had **one** mutex for every sequence run (link poll, admin, `MACsec`), the gate took it
//!   blocking, raw diag MDIO ran under it and the SAI lock, and its global `holdoff-ms` hold-off
//!   held the **link polls only** (`MACsec` never: a PN / counter read could land between the
//!   requests of a tool's conversation, seen on the AS7772 as "1.8221 read 0x0000"). Here: per-PHY
//!   slots, leases, the table above.
#![forbid(unsafe_code)]

mod arbiter;
mod call;
mod clock;
#[cfg(feature = "testing")]
pub mod fwphy;
mod leaf;

pub use arbiter::{
    ArbCounts, DiagTarget, DiagWho, LEASE_DEFAULT_MS, LEASE_MAX_MS, LeaseScope, LeaseView, TxnEnded, TxnInfo,
    TxnRefused,
};
pub use call::{call, in_call, sai_locked, under_sai_lock};
pub use clock::{Clock, ManualClock, SystemClock};
pub use phy_seq::{Dev, ExtPhyTable, ParseError};

use arbiter::{Arbiter, Ctx, LibTurn, LibWant};
use leaf::Leaf;
use phy_seq::{Op, Params, Section};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex, PoisonError};

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
/// the backend's link-scan thread, where the SAI layer's backend cannot be borrowed), shared by
/// the threads that run sequences on different PHYs at once, with the master's own lock inside.
pub trait PhyBus: Send + Sync {
    /// Reads `dev.reg` of the PHY at address word `phy`.
    fn read(&self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus>;
    /// Writes `value` to `dev.reg` of the PHY at address word `phy`.
    fn write(&self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus>;
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

/// Receives the runtime's log lines (called with none of the runtime's locks held).
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
    /// Duration of the last poll, us (from when it had its PHYs).
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
/// `stats-flow.*`) goes without reading its PHY while diag leases make it skip: then it waits
/// for the transaction to end (at most [`LEASE_MAX_MS`]) and reads.
/// Bounds how stale a PN read can be while a tool works on the PHY continuously (syncd asks for
/// the PN about once a second).
pub const MACSEC_POLL_DEFER_MAX_MS: u64 = 10_000;

/// How many due link polls in a row a port answers from its last verdict because a library run
/// (a `MACsec` or admin sequence) holds its PHYs, before the next one waits for that run (the
/// gate's starvation bound). The gate runs inside the SDK's link-scan pass (250 ms; a poll costs
/// 0.2-0.36 ms of bus), so it never waits behind a library run otherwise: a run holds its PHY for
/// 1.3 ms (PN read), 14-18 ms (SA statistics) or 80-90 ms (SA install, measured on the AS7772), and
/// waiting would hold every port's link processing up by that much. A busy answer costs that port
/// one pass (its poll stays due and runs on the next one); after this many, the wait is at most one
/// run (the waiting gate goes before every other library user of those PHYs), once.
pub const GATE_BUSY_MAX: u32 = 3;

/// The upper bounds (us) of the gate-time histogram ([`ArbCounts::gate_time_hist`]); the last
/// bucket is everything longer.
pub const GATE_TIME_BUCKETS_US: [u64; 4] = [500, 2_000, 10_000, 100_000];

/// What a `MACsec` table sequence is, for the arbitration (crate documentation's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacsecUse<'a> {
    /// Something `SONiC` asked to change (SA install / remove, gate, enable / disable, a clear),
    /// or the start-up clean-up: waits for a transaction on its PHYs, never dropped.
    Op,
    /// A periodic read for a get (`pn.*`, `stats-sa.*`, `stats-flow.*`), by its sequence name:
    /// skipped while a lease holds its PHY (bounded by [`MACSEC_POLL_DEFER_MAX_MS`]).
    Poll(&'a str),
    /// `extmacsec run` of the diag socket: refused while a transaction holds its PHY.
    DiagRun,
}

/// What became of a `MACsec` sequence ([`ExtPhy::macsec_run`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Turn<R> {
    /// It ran; what it returned.
    Ran(R),
    /// A periodic read skipped: a lease holds its PHY (the caller answers its last read).
    Skipped,
    /// Refused (a diag run inside a transaction): why.
    Refused(String),
}

/// PHYs claimed for a SAI call ([`ExtPhy::claim`]): given back when dropped. The claim of a call
/// made through [`call`] belongs to the call (given back when it returns); this value is then
/// empty.
#[must_use = "the PHYs are claimed while it lives"]
pub struct Claim {
    owned: Option<(Arc<Shared>, Vec<u16>)>,
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some((shared, phys)) = self.owned.take() {
            shared.arb.unclaim(&phys);
        }
    }
}

impl fmt::Debug for Claim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Claim")
            .field("owned", &self.owned.as_ref().map(|(_, phys)| phys))
            .finish()
    }
}

/// A claim that met a transaction inside [`call`]: the SAI call returns now, having done
/// nothing, and runs again after the transaction (outside the SAI lock).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deferred {
    /// The PHYs it could not claim.
    pub phys: Vec<u16>,
    /// The transaction in the way.
    pub txn: u64,
}

impl fmt::Display for Deferred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} in diag transaction {}: the call runs again after it",
            LeaseScope::Phys(self.phys.iter().copied().collect()),
            self.txn
        )
    }
}

/// The most good polls counted (the C++ `1000000`).
const GOOD_MAX: u32 = 1_000_000;

#[derive(Debug, Default)]
struct PortRt {
    hw: Option<ChipPort>,
    name: String,
    state: PortState,
    next_poll_ms: u64,
    counters: PortCounters,
    /// The PHYs of its link sequence (sorted, no doubles).
    link_phys: Vec<u16>,
    /// The PHYs of all its sequences: what an admin change holds (so that it never runs beside
    /// the port's own poll).
    all_phys: Vec<u16>,
    /// Due polls answered from the last verdict in a row because a library run held its PHYs.
    busy_in_row: u32,
}

struct State {
    table: Arc<ExtPhyTable>,
    ports: Vec<PortRt>,
    by_hw: BTreeMap<ChipPort, usize>,
    /// Set by attach: the C++ `g_be` (the table is active).
    bus: Option<Arc<dyn PhyBus>>,
    /// Counts attaches and clears: a gate that waited for its PHYs across one does nothing.
    epoch: u64,
    /// When each `MACsec` poll sequence last read its PHY (or was first skipped).
    last_read: BTreeMap<(ChipPort, String), u64>,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    /// Replaceable (tests: a manual clock once the switch is up).
    clock: Mutex<Arc<dyn Clock>>,
    pub(crate) arb: Arbiter,
    log: Logger,
}

/// The runtime of one switch's external-PHY table and the arbiter of its MDIO bus. Clones share
/// it (the gates hold clones).
#[derive(Clone)]
pub struct ExtPhy {
    pub(crate) shared: Arc<Shared>,
}

impl fmt::Debug for ExtPhy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (ports, active) = {
            let st = self.lock();
            (st.ports.len(), st.bus.is_some())
        };
        f.debug_struct("ExtPhy")
            .field("ports", &ports)
            .field("active", &active)
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
    bus: &'a dyn PhyBus,
    clock: &'a dyn Clock,
}

impl phy_seq::Bus for OnBus<'_> {
    type Error = BusStatus;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        leaf::assert_free("an MDIO read");
        self.bus.read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        leaf::assert_free("an MDIO write");
        self.bus.write(phy, dev, reg, value)
    }
    fn delay_us(&mut self, us: u32) {
        self.clock.wait_us(us);
    }
}

/// Runs an ext-phy sequence (fixed PHYs, constant values: no parameters).
fn run_seq(bus: &dyn PhyBus, clock: &dyn Clock, ops: &[Op]) -> SeqOutcome {
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

/// The PHYs `ops` address (sorted, no doubles), for a sequence run for a port whose PHY is
/// `port_phy` (ext-phy tables name fixed PHYs only; `MACsec` tables also `p`).
pub fn phys_of(ops: &[Op], port_phy: u16) -> Vec<u16> {
    let set: BTreeSet<u16> = ops
        .iter()
        .filter_map(Op::reg)
        .map(|r| r.phy.resolve(port_phy))
        .collect();
    set.into_iter().collect()
}

fn union(a: &[u16], b: &[u16]) -> Vec<u16> {
    let set: BTreeSet<u16> = a.iter().chain(b).copied().collect();
    set.into_iter().collect()
}

/// Counts a link poll's outcome `r` (it took `dt` us) on port `rt`, the next one due at
/// `next_poll_ms`: the debounce; the verdict and the log lines.
fn record_poll(
    rt: &mut PortRt,
    r: &SeqOutcome,
    dt: u64,
    next_poll_ms: u64,
    up_polls: u32,
) -> (Verdict, Vec<(Level, String)>) {
    let mut lines: Vec<(Level, String)> = Vec::new();
    let c = &mut rt.counters;
    c.polls += 1;
    c.last_us = dt;
    c.total_us = c.total_us.saturating_add(dt);
    c.max_us = c.max_us.max(dt);
    rt.next_poll_ms = next_poll_ms;
    if !r.fault {
        rt.state.last_val = r.last;
    }
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
        if !rt.state.ext_up && rt.state.good >= up_polls {
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
    (verdict_of(&rt.state), lines)
}

fn verdict_of(state: &PortState) -> Verdict {
    if state.ext_up {
        Verdict::NoOpinion
    } else {
        Verdict::Down
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
                    table: Arc::new(table),
                    ports,
                    by_hw: BTreeMap::new(),
                    bus: None,
                    epoch: 0,
                    last_read: BTreeMap::new(),
                }),
                clock: Mutex::new(clock),
                arb: Arbiter::default(),
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

    fn lock(&self) -> Leaf<'_, State> {
        leaf::lock(&self.shared.state)
    }

    fn log(&self, api: Api, level: Level, text: &str) {
        leaf::assert_free("a log line");
        (self.shared.log)(api, level, text);
    }

    /// The arbiter's log lines, written now (after its lock).
    fn log_notes(&self) {
        for text in self.shared.arb.take_notes() {
            self.log(Api::Switch, Level::Notice, &text);
        }
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
            let table = Arc::clone(&st.table);
            let mut missing = None;
            for (i, (tp, rt)) in table.ports().iter().zip(st.ports.iter_mut()).enumerate() {
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
                rt.link_phys = phys_of(tp.ops(Section::Link), 0);
                rt.all_phys = union(
                    &rt.link_phys,
                    &union(
                        &phys_of(tp.ops(Section::AdminUp), 0),
                        &phys_of(tp.ops(Section::AdminDown), 0),
                    ),
                );
                st.by_hw.insert(hw, i);
                if !tp.ops(Section::Link).is_empty() {
                    gated.push(hw);
                }
            }
            if missing.is_none() {
                st.bus = Some(Arc::from(bus));
                st.epoch += 1;
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
    /// this crate's locks: the backend may be waiting for a gate call to return). Empty when
    /// inactive.
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

    /// `ext_phy_detach`'s second half: forgets the table and the bus (leases stay: they are
    /// about PHYs, not the table).
    pub fn clear(&self) {
        let mut st = self.lock();
        st.bus = None;
        st.by_hw.clear();
        st.table = Arc::new(ExtPhyTable::default());
        st.ports.clear();
        st.last_read.clear();
        st.epoch += 1;
    }

    /// `ext_phy_active`: a table is attached.
    pub fn active(&self) -> bool {
        self.lock().bus.is_some()
    }

    /// `ext_phy_admin`: the port's admin state changes. Call with `up` before enabling the
    /// ASIC port, with `!up` after disabling it. Restarts the debounce from "down" and runs
    /// the port's admin-up / admin-down sequence (if it has one). Takes the port's PHYs first
    /// (waiting for a diag transaction on them to end or run out).
    pub fn admin(&self, hw: ChipPort, up: bool) {
        let clock = self.clock();
        let clock = &*clock;
        let (i, phys, epoch) = {
            let st = self.lock();
            let (Some(_), Some(&i)) = (st.bus.as_ref(), st.by_hw.get(&hw)) else {
                return;
            };
            let Some(rt) = st.ports.get(i) else {
                return;
            };
            (i, rt.all_phys.clone(), st.epoch)
        };
        let ctx = self.ctx(&phys);
        let turn = self.shared.arb.lib(clock, &phys, LibWant::Wait { op: true }, &ctx);
        let LibTurn::Slot {
            slot,
            waited_us,
            for_txn,
            claimed,
            locked_wait,
            ..
        } = turn
        else {
            return; // a change never answers "leased" or "defer"
        };
        self.after_turn(&ctx, claimed, locked_wait, for_txn);
        self.count_op_wait(waited_us, for_txn);
        let (bus, table, name) = {
            let mut guard = self.lock();
            let st = &mut *guard;
            if st.epoch != epoch {
                return;
            }
            let (Some(bus), Some(rt)) = (st.bus.clone(), st.ports.get_mut(i)) else {
                return;
            };
            rt.state.admin = up;
            rt.state.ext_up = false;
            rt.state.good = 0;
            rt.next_poll_ms = 0;
            (bus, Arc::clone(&st.table), rt.name.clone())
        };
        let Some(tp) = table.ports().get(i) else {
            return;
        };
        let ops = tp.ops(if up { Section::AdminUp } else { Section::AdminDown });
        if ops.is_empty() {
            return;
        }
        let t0 = clock.now_us();
        let r = run_seq(&*bus, clock, ops);
        let dt = clock.now_us().saturating_sub(t0);
        {
            let mut st = self.lock();
            if st.epoch == epoch {
                if let Some(rt) = st.ports.get_mut(i) {
                    rt.counters.hooks += 1;
                    rt.counters.hook_max_us = rt.counters.hook_max_us.max(dt);
                    if r.fault {
                        rt.counters.hook_fail += 1;
                    }
                }
            }
        }
        drop(slot);
        let dir = if up { "up" } else { "down" };
        let (level, text) = if r.fault {
            (
                Level::Error,
                format!("port {name}: external PHY admin-{dir} sequence stopped: {}", r.why),
            )
        } else {
            (
                Level::Info,
                format!("port {name}: external PHY admin-{dir} sequence done ({dt} us)"),
            )
        };
        self.log(Api::Port, level, &text);
    }

    fn count_op_wait(&self, waited_us: u64, for_txn: Option<u64>) {
        if for_txn.is_some() {
            self.shared.arb.count(|c| {
                c.op_waits += 1;
                c.op_wait_us_max = c.op_wait_us_max.max(waited_us);
            });
        }
        self.log_notes();
    }

    /// The context of a library user of this thread on `phys` (module `call`).
    fn ctx(&self, phys: &[u16]) -> Ctx {
        let in_call = call::in_call();
        Ctx {
            in_call,
            missing: if in_call {
                call::missing(&self.shared, phys)
            } else {
                phys.to_vec()
            },
            sai_locked: call::sai_locked(),
        }
    }

    /// After a library user's turn: PHYs claimed for its call stay claimed with the call; a wait
    /// for a transaction under the SAI lock (a gap in the call's claims) is logged.
    fn after_turn(&self, ctx: &Ctx, claimed: bool, locked_wait: bool, for_txn: Option<u64>) {
        if claimed {
            call::keep(&self.shared, &ctx.missing);
        }
        if locked_wait {
            self.log(
                Api::Switch,
                Level::Error,
                &format!(
                    "external PHY {}: a library run waited for diag transaction {} under the SAI lock: \
                     the SAI call did not claim these PHYs first (a bug of the claims)",
                    LeaseScope::Phys(ctx.missing.iter().copied().collect()),
                    for_txn.unwrap_or_default()
                ),
            );
        }
    }

    /// Claims `phys` for the SAI call this thread is in, before the call's first effect (module
    /// `call`): at once when no transaction holds them (or the call claimed them already);
    /// inside [`call`] with a transaction in the way: [`Deferred`] (the caller returns at once,
    /// having done nothing; the call runs again after the transaction, outside the SAI lock).
    /// Outside [`call`] (the runtime used directly) it waits for the transaction (at most
    /// [`LEASE_MAX_MS`]) and the claim lives as long as the returned [`Claim`]. A claim holds no
    /// PHY: link polls and other users run beside it; only new transactions on those PHYs wait.
    pub fn claim(&self, phys: &[u16]) -> Result<Claim, Deferred> {
        let none = Claim { owned: None };
        let clock = self.clock();
        let clock = &*clock;
        if call::in_call() {
            let missing = call::missing(&self.shared, phys);
            if missing.is_empty() {
                return Ok(none);
            }
            return match self.shared.arb.claim_try(clock, &missing) {
                Ok(()) => {
                    call::keep(&self.shared, &missing);
                    Ok(none)
                }
                Err(txn) => {
                    call::defer(self, &missing);
                    self.shared.arb.count(|c| c.calls_deferred += 1);
                    self.log_notes();
                    Err(Deferred { phys: missing, txn })
                }
            };
        }
        if phys.is_empty() {
            return Ok(none);
        }
        let owned = Claim {
            owned: Some((Arc::clone(&self.shared), phys.to_vec())),
        };
        if self.shared.arb.claim_try(clock, phys).is_ok() {
            return Ok(owned);
        }
        if call::sai_locked() {
            // a SAI call that did not come through `call`: it cannot let go of the lock
            debug_assert!(
                false,
                "sai-extphy: a claim of phys {phys:?} meets a transaction under the SAI lock outside call()"
            );
            self.shared.arb.count(|c| c.locked_waits += 1);
            self.log(
                Api::Switch,
                Level::Error,
                &format!(
                    "external PHY {}: a SAI call waits for a diag transaction under the SAI lock (not made through \
                     the rim's call): a bug of the rim",
                    LeaseScope::Phys(phys.iter().copied().collect())
                ),
            );
        }
        let (waited_us, for_txn) = self.shared.arb.claim_wait(clock, phys);
        self.count_op_wait(waited_us, for_txn);
        Ok(owned)
    }

    /// [`ExtPhy::claim`] of the PHYs an admin change of chip port `hw` runs on (none for a port
    /// outside the table, or without a table).
    pub fn claim_port(&self, hw: ChipPort) -> Result<Claim, Deferred> {
        let phys = {
            let st = self.lock();
            match (st.bus.as_ref(), st.by_hw.get(&hw).and_then(|&i| st.ports.get(i))) {
                (Some(_), Some(rt)) => rt.all_phys.clone(),
                _ => Vec::new(),
            }
        };
        self.claim(&phys)
    }

    /// Waits for the transactions on `phys` outside the SAI lock and claims them, for [`call`]
    /// (between two runs of a deferred call).
    pub(crate) fn claim_outside(&self, phys: &[u16]) {
        debug_assert!(!call::sai_locked(), "sai-extphy: a claim waits under the SAI lock");
        let clock = self.clock();
        let (waited_us, for_txn) = self.shared.arb.claim_wait(&*clock, phys);
        self.count_op_wait(waited_us, for_txn);
        if let Some(t) = for_txn {
            self.log(
                Api::Switch,
                Level::Notice,
                &format!(
                    "external PHY {}: a SAI call waited {} ms for diag transaction {t}, outside the SAI lock",
                    LeaseScope::Phys(phys.iter().copied().collect()),
                    waited_us / 1000
                ),
            );
        }
    }

    /// `ext_phy_gate`: asked by the backend's link scan, per pass and port. Never waits for a
    /// diag lease (the cached verdict answers, counted) nor, normally, for a library run on the
    /// poll's PHYs (the cached verdict answers at once, counted; the poll stays due, so it runs on
    /// the next pass): only after [`GATE_BUSY_MAX`] such answers in a row does a due poll wait for
    /// the run (at most one run: it goes first; counted with its time).
    pub fn gate(&self, hw: ChipPort) -> Verdict {
        let clock = self.clock();
        let clock = &*clock;
        // is a poll due? (no PHY taken yet)
        let (i, phys, epoch, starved) = {
            let st = self.lock();
            let (Some(_), Some(&i)) = (st.bus.as_ref(), st.by_hw.get(&hw)) else {
                return Verdict::NoOpinion;
            };
            let (Some(tp), Some(rt)) = (st.table.ports().get(i), st.ports.get(i)) else {
                return Verdict::NoOpinion;
            };
            if tp.ops(Section::Link).is_empty() || !rt.state.admin {
                return Verdict::NoOpinion;
            }
            if clock.now_us() / 1000 < rt.next_poll_ms {
                return verdict_of(&rt.state);
            }
            (i, rt.link_phys.clone(), st.epoch, rt.busy_in_row >= GATE_BUSY_MAX)
        };
        let t_ask = clock.now_us();
        // the wait is measured from before the request, inside the arbiter
        let want = LibWant::Gate { wait: starved };
        let (slot, waited_us) = match self.shared.arb.lib(clock, &phys, want, &Ctx::default()) {
            LibTurn::Leased(_) => {
                self.shared.arb.count(|c| c.gate_cached += 1);
                self.log_notes(); // a lease that ran out meanwhile
                return self.cached(i, epoch, false);
            }
            LibTurn::Busy => {
                self.shared.arb.count(|c| c.gate_busy += 1);
                return self.cached(i, epoch, true);
            }
            LibTurn::Slot { slot, waited_us, .. } => (slot, waited_us),
            LibTurn::Defer(_) => return self.cached(i, epoch, false), // not asked by the gate
        };
        if waited_us > 0 {
            self.shared.arb.count(|c| {
                c.gate_waits += 1;
                c.gate_wait_us_max = c.gate_wait_us_max.max(waited_us);
                c.gate_wait_us_total = c.gate_wait_us_total.saturating_add(waited_us);
            });
        }
        // the PHYs are ours: is the poll still due (an admin change may have run meanwhile)?
        let t1 = clock.now_us();
        let now_ms = t1 / 1000;
        let (bus, table, poll_ms, up_polls) = {
            let mut st = self.lock();
            if st.epoch != epoch {
                return Verdict::NoOpinion;
            }
            let Some(bus) = st.bus.clone() else {
                return Verdict::NoOpinion;
            };
            let (poll_ms, up_polls) = (st.table.poll_ms(), st.table.up_polls());
            let table = Arc::clone(&st.table);
            let Some(rt) = st.ports.get_mut(i) else {
                return Verdict::NoOpinion;
            };
            rt.busy_in_row = 0;
            if !rt.state.admin {
                return Verdict::NoOpinion;
            }
            if now_ms < rt.next_poll_ms {
                return verdict_of(&rt.state);
            }
            (bus, table, poll_ms, up_polls)
        };
        let Some(tp) = table.ports().get(i) else {
            return Verdict::NoOpinion;
        };
        let r = run_seq(&*bus, clock, tp.ops(Section::Link));
        let t2 = clock.now_us();
        let dt = t2.saturating_sub(t1);
        let (v, lines) = {
            let mut st = self.lock();
            if st.epoch != epoch {
                return Verdict::NoOpinion;
            }
            let Some(rt) = st.ports.get_mut(i) else {
                return Verdict::NoOpinion;
            };
            record_poll(rt, &r, dt, now_ms.saturating_add(u64::from(poll_ms)), up_polls)
        };
        drop(slot);
        let took = t2.saturating_sub(t_ask);
        self.shared.arb.count(|c| {
            let bucket = GATE_TIME_BUCKETS_US
                .iter()
                .position(|b| took <= *b)
                .unwrap_or(GATE_TIME_BUCKETS_US.len());
            if let Some(n) = c.gate_time_hist.get_mut(bucket) {
                *n += 1;
            }
            c.gate_time_us_max = c.gate_time_us_max.max(took);
        });
        for (level, text) in lines {
            self.log(Api::Port, level, &text);
        }
        self.log_notes();
        v
    }

    /// The last verdict of port `i` (a lease or, `busy`, a library run holds its PHYs; a busy
    /// answer is counted towards [`GATE_BUSY_MAX`]).
    fn cached(&self, i: usize, epoch: u64, busy: bool) -> Verdict {
        let mut st = self.lock();
        if st.epoch != epoch {
            return Verdict::NoOpinion;
        }
        match st.ports.get_mut(i) {
            Some(rt) if rt.state.admin => {
                if busy {
                    rt.busy_in_row = rt.busy_in_row.saturating_add(1);
                }
                verdict_of(&rt.state)
            }
            _ => Verdict::NoOpinion,
        }
    }

    /// Runs `f` (one external `MACsec` table sequence of chip port `port`, on the PHYs
    /// `phys`: [`phys_of`]) with those PHYs held, as `use_` says (crate documentation's table).
    /// Logs one notice per forced poll and per run that waited for a transaction.
    pub fn macsec_run<R>(&self, port: ChipPort, phys: &[u16], use_: MacsecUse<'_>, f: impl FnOnce() -> R) -> Turn<R> {
        let clock = self.clock();
        let clock = &*clock;
        match use_ {
            MacsecUse::Op => {
                let ctx = self.ctx(phys);
                let LibTurn::Slot {
                    slot,
                    waited_us,
                    for_txn,
                    claimed,
                    locked_wait,
                    ..
                } = self.shared.arb.lib(clock, phys, LibWant::Wait { op: true }, &ctx)
                else {
                    return Turn::Refused("not served".to_owned()); // a change never answers "leased"
                };
                self.after_turn(&ctx, claimed, locked_wait, for_txn);
                self.count_op_wait(waited_us, for_txn);
                let r = f();
                drop(slot);
                if let Some(t) = for_txn {
                    self.log(
                        Api::Switch,
                        Level::Notice,
                        &format!(
                            "external MACsec sequence on chip port {} waited {} ms for diag transaction {t}",
                            port.0,
                            waited_us / 1000
                        ),
                    );
                }
                Turn::Ran(r)
            }
            MacsecUse::DiagRun => match self.shared.arb.lib(clock, phys, LibWant::NoWait, &Ctx::default()) {
                LibTurn::Leased(t) => {
                    self.shared.arb.count(|c| c.diag_run_refused += 1);
                    Turn::Refused(format!(
                        "chip port {}: its PHY is in diag transaction {}: end it first",
                        port.0,
                        t.unwrap_or_default()
                    ))
                }
                LibTurn::Slot { slot, .. } => {
                    let r = f();
                    drop(slot);
                    Turn::Ran(r)
                }
                LibTurn::Busy | LibTurn::Defer(_) => Turn::Refused("not served".to_owned()), // not asked
            },
            MacsecUse::Poll(name) => self.poll_run(clock, (port, name.to_owned()), phys, f),
        }
    }

    /// A periodic read: skipped while a lease holds its PHY, unless it last read the PHY
    /// [`MACSEC_POLL_DEFER_MAX_MS`] ago: then it waits for the transaction to end (decided under
    /// the arbiter's lock with the wait, so no new transaction slips in before it) and reads.
    /// Inside a SAI call ([`call`]) it does not wait there: it skips and defers the call, which
    /// claims the PHYs once the transaction is over (outside the SAI lock) and runs again: the
    /// read then happens, with no transaction in between.
    fn poll_run<R>(&self, clock: &dyn Clock, key: (ChipPort, String), phys: &[u16], f: impl FnOnce() -> R) -> Turn<R> {
        let now_ms = clock.now_us() / 1000;
        let last = self.lock().last_read.get(&key).copied();
        let overdue = last.is_some_and(|t| now_ms.saturating_sub(t) >= MACSEC_POLL_DEFER_MAX_MS);
        let want = if overdue {
            LibWant::Wait { op: false }
        } else {
            LibWant::Skip
        };
        let ctx = self.ctx(phys);
        let (slot, waited_us, for_txn, leased) = match self.shared.arb.lib(clock, phys, want, &ctx) {
            LibTurn::Slot {
                slot,
                waited_us,
                for_txn,
                leased,
                locked_wait,
                ..
            } => {
                self.after_turn(&ctx, false, locked_wait, for_txn);
                (slot, waited_us, for_txn, leased)
            }
            LibTurn::Leased(_) | LibTurn::Busy => {
                self.lock().last_read.entry(key).or_insert(now_ms);
                self.shared.arb.count(|c| c.polls_skipped += 1);
                self.log_notes();
                return Turn::Skipped;
            }
            LibTurn::Defer(txn) => {
                // the get runs again once the transaction is over (its answer now is dropped)
                call::defer(self, &ctx.missing);
                self.shared.arb.count(|c| c.calls_deferred += 1);
                self.log_notes();
                let (port, name) = key;
                self.log(
                    Api::Switch,
                    Level::Notice,
                    &format!(
                        "external MACsec poll {name} on chip port {} skipped for {} ms, the most allowed: \
                         its get waits for diag transaction {txn} outside the SAI lock, then reads",
                        port.0,
                        now_ms.saturating_sub(last.unwrap_or(now_ms))
                    ),
                );
                return Turn::Skipped;
            }
        };
        let read_ms = clock.now_us() / 1000;
        self.lock().last_read.insert(key.clone(), read_ms);
        let r = f();
        drop(slot);
        // a deferred get's second run: its call claimed the PHYs after the transaction
        let after_defer = ctx.in_call && ctx.missing.is_empty();
        if overdue && (leased || after_defer) {
            self.shared.arb.count(|c| {
                c.forced_reads += 1;
                c.forced_wait_us_max = c.forced_wait_us_max.max(waited_us);
            });
            let skipped_ms = now_ms.saturating_sub(last.unwrap_or(now_ms));
            let (port, name) = key;
            let text = match for_txn {
                Some(t) => format!(
                    "external MACsec poll {name} on chip port {} skipped for {skipped_ms} ms, the most allowed: \
                     read after diag transaction {t} (waited {} ms)",
                    port.0,
                    waited_us / 1000
                ),
                None if after_defer => format!(
                    "external MACsec poll {name} on chip port {} skipped for {skipped_ms} ms, the most allowed: \
                     read after the diag transaction (its get waited outside the SAI lock)",
                    port.0
                ),
                None => format!(
                    "external MACsec poll {name} on chip port {} skipped for {skipped_ms} ms, the most allowed: \
                     read between two raw MDIO accesses of a diag client without a transaction",
                    port.0
                ),
            };
            self.log(Api::Switch, Level::Notice, &text);
        }
        self.log_notes();
        Turn::Ran(r)
    }

    /// One raw MDIO access of the diag socket: `f` (the access) runs with `target` held, never
    /// inside a library sequence; a bare access (no transaction) holds its PHY's polls off
    /// `holdoff-ms` after it (the C++ hold-off, per PHY, only while a table is attached).
    pub fn diag_access<R>(&self, who: DiagWho, target: DiagTarget, f: impl FnOnce() -> R) -> Result<R, TxnRefused> {
        let clock = self.clock();
        let holdoff = {
            let st = self.lock();
            if st.bus.is_some() {
                u64::from(st.table.holdoff_ms())
            } else {
                0
            }
        };
        let slot = self.shared.arb.diag(&*clock, who, target, holdoff);
        self.log_notes();
        let slot = slot?;
        let r = f();
        drop(slot);
        Ok(r)
    }

    /// `extphy txn begin`: a transaction on `scope` (its lease `ms` after each access, default
    /// [`LEASE_DEFAULT_MS`], at most [`LEASE_MAX_MS`]).
    pub fn txn_begin(&self, scope: LeaseScope, ms: Option<u64>) -> Result<TxnInfo, TxnRefused> {
        let clock = self.clock();
        let r = self.shared.arb.begin(&*clock, scope, ms.unwrap_or(LEASE_DEFAULT_MS));
        self.log_notes();
        if let Ok(info) = &r {
            self.log(
                Api::Switch,
                Level::Info,
                &format!(
                    "external PHY diag transaction {} begun: {} (lease {} ms, at most {} ms)",
                    info.txn, info.scope, info.step_ms, info.max_left_ms
                ),
            );
        }
        r
    }

    /// `extphy txn renew`: extends a transaction (and changes its step when `ms` is given).
    pub fn txn_renew(&self, txn: u64, ms: Option<u64>) -> Result<TxnInfo, TxnRefused> {
        let clock = self.clock();
        let r = self.shared.arb.renew(&*clock, txn, ms);
        self.log_notes();
        r
    }

    /// `extphy txn end`: ends a transaction; one that expired answers [`TxnRefused::Expired`].
    pub fn txn_end(&self, txn: u64) -> Result<TxnEnded, TxnRefused> {
        let clock = self.clock();
        let r = self.shared.arb.end(&*clock, txn);
        self.log_notes();
        if let Ok(e) = &r {
            self.log(
                Api::Switch,
                Level::Info,
                &format!(
                    "external PHY diag transaction {} ended: {} accesses in {} ms",
                    e.txn, e.accesses, e.held_ms
                ),
            );
        }
        r
    }

    /// The live leases.
    pub fn leases(&self) -> Vec<LeaseView> {
        let clock = self.clock();
        let v = self.shared.arb.leases(&*clock);
        self.log_notes();
        v
    }

    /// How many library users (admin sequences, `MACsec` runs, link polls) wait for their PHYs now:
    /// for tests, which wait for it instead of sleeping (feature `testing`).
    #[cfg(feature = "testing")]
    pub fn waiting(&self) -> u32 {
        self.shared.arb.waiting()
    }

    /// How many PHYs SAI calls hold claimed now: for tests (feature `testing`).
    #[cfg(feature = "testing")]
    pub fn claimed(&self) -> usize {
        self.shared.arb.claimed()
    }

    /// What the arbitration did so far.
    pub fn counts(&self) -> ArbCounts {
        self.shared.arb.counts()
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

    /// `ext_phy_status`: the diag socket's `extphy` text: the C++ table, then the arbitration.
    pub fn status(&self) -> String {
        let leases = self.leases();
        let counts = self.counts();
        let st = self.lock();
        if st.bus.is_none() {
            return "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n".to_owned();
        }
        let t = &st.table;
        let mut s = format!(
            "extphy: {} ports, poll {} ms, up after {} good polls, hold-off {} ms ({}, {} so far)\n",
            t.ports().len(),
            t.poll_ms(),
            t.up_polls(),
            t.holdoff_ms(),
            if leases.is_empty() { "not paused" } else { "PAUSED now" },
            counts.implicit_begun + counts.txn_begun
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
        drop(st);
        status_tail(&mut s, &counts, &leases);
        s
    }
}

/// The arbitration lines of the status text.
fn status_tail(s: &mut String, c: &ArbCounts, leases: &[LeaseView]) {
    let _ = writeln!(
        s,
        "extphy: MDIO per PHY; diag transactions: {LEASE_DEFAULT_MS} ms after each access unless asked, at most {LEASE_MAX_MS} ms; \
         raw MDIO without one: hold-off per PHY"
    );
    let _ = writeln!(
        s,
        "extphy: leases now {}; transactions begun {}, ended {}, expired {}; raw MDIO hold-offs {}; refused: busy {}, void {}",
        leases.len(),
        c.txn_begun,
        c.txn_ended,
        c.txn_expired,
        c.implicit_begun,
        c.busy_refused,
        c.void_refused
    );
    let _ = writeln!(
        s,
        "extphy: link polls answered from the last verdict during a lease {}, behind a library run {}; \
         waits for a library run before a poll {} (after {GATE_BUSY_MAX} busy in a row; max {} us, total {} us)",
        c.gate_cached, c.gate_busy, c.gate_waits, c.gate_wait_us_max, c.gate_wait_us_total
    );
    let [b0, b1, b2, b3] = GATE_TIME_BUCKETS_US;
    let [h0, h1, h2, h3, h4] = c.gate_time_hist;
    let _ = writeln!(
        s,
        "extphy: link gate time per poll (us): <={b0} {h0}, <={b1} {h1}, <={b2} {h2}, <={b3} {h3}, more {h4}; max {}",
        c.gate_time_us_max
    );
    let _ = writeln!(
        s,
        "extphy: MACsec polls skipped during a lease {}; forced reads after {MACSEC_POLL_DEFER_MAX_MS} ms {} (max wait {} us); \
         MACsec / admin runs and SAI calls that waited for a transaction {} (max {} us); extmacsec run refused {}",
        c.polls_skipped, c.forced_reads, c.forced_wait_us_max, c.op_waits, c.op_wait_us_max, c.diag_run_refused
    );
    let _ = writeln!(
        s,
        "extphy: SAI calls that waited for a transaction outside the SAI lock {}; waits under the SAI lock {}",
        c.calls_deferred, c.locked_waits
    );
    for l in leases {
        match l.txn {
            Some(t) => {
                let _ = writeln!(
                    s,
                    "extphy:   txn {t} {}: {} ms left (at most {} ms), {} accesses",
                    l.scope,
                    l.left_ms,
                    l.max_left_ms.unwrap_or_default(),
                    l.accesses
                );
            }
            None => {
                let _ = writeln!(
                    s,
                    "extphy:   raw MDIO hold-off {}: {} ms left, {} accesses",
                    l.scope, l.left_ms, l.accesses
                );
            }
        }
    }
}

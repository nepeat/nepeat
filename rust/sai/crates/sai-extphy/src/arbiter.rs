//! The MDIO arbiter of one switch: who may use which PHY of the bus, and when (the crate's
//! "Serialisation" section). Two kinds of state, both behind **one** mutex of this module (a
//! leaf: nothing else is locked or called while it is held):
//!
//! - **Slots**: a PHY's slot is held by whoever is accessing that PHY now (a library sequence run,
//!   a link poll, one diag access). Holders never wait for anything while they hold a slot, so a
//!   slot is always given back after the holder's bus accesses (at most a `MACsec` stats run,
//!   about 18 ms). The **global** slot (raw diag MDIO that names no single PHY) needs every slot.
//! - **Leases**: a diag client's claim on PHYs across several connections. A lease is a record,
//!   not a held lock: the client holds nothing between its accesses, so a dead client blocks no
//!   thread; its lease simply runs out. Explicit leases (transactions, [`DiagWho::Txn`]) last
//!   [`LEASE_DEFAULT_MS`] (or what was asked) after each access and never past
//!   [`LEASE_MAX_MS`] from their begin; implicit ones (a bare raw access, [`DiagWho::Bare`])
//!   last `holdoff-ms` after the last access on that PHY, as the C++ hold-off did, per PHY.
//!
//! What a library user does while a lease covers one of its PHYs ([`LibWant`]): the link gate and
//! the periodic `MACsec` polls do not wait (cached verdict, last read); `SONiC`'s `MACsec` changes,
//! admin sequences and the forced PN read wait for an **explicit** lease to end or run out
//! (bounded by [`LEASE_MAX_MS`]) and pass an implicit one; a diag-socket `extmacsec run` is refused.
//! Library users that wait are counted as pending on their PHYs: a new transaction on those PHYs
//! waits for them first, so back-to-back transactions cannot starve them.
use crate::clock::Clock;
use crate::leaf::{self, Leaf};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// How long an explicit lease lasts after its begin and after each access, when the client asks
/// for no other time.
pub const LEASE_DEFAULT_MS: u64 = 1000;

/// The longest an explicit lease lasts from its begin, renewals included: what a library user
/// that waits for a transaction (a `SONiC` `MACsec` change, an admin sequence, the forced PN read)
/// waits at most, under the SAI lock. Below [`crate::MACSEC_POLL_DEFER_MAX_MS`], so the PN bound
/// holds with a transaction in the way.
pub const LEASE_MAX_MS: u64 = 2000;

/// Expired transactions remembered (so that their next access says "expired", not "unknown").
const EXPIRED_KEPT: usize = 64;

/// A waiting thread looks at the clock again this often (the injected clock does not wake it).
const RECHECK: Duration = Duration::from_millis(1);

/// The PHYs a transaction holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseScope {
    /// These MDIO address words.
    Phys(BTreeSet<u16>),
    /// Every PHY of the bus (also raw MDIO that names no single PHY).
    All,
}

impl LeaseScope {
    fn covers(&self, phy: u16) -> bool {
        match self {
            Self::Phys(set) => set.contains(&phy),
            Self::All => true,
        }
    }

    fn overlaps_any(&self, phys: &[u16]) -> bool {
        phys.iter().any(|p| self.covers(*p))
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => true,
            (Self::Phys(a), Self::Phys(b)) => a.intersection(b).next().is_some(),
        }
    }

    fn holds(&self, target: DiagTarget) -> bool {
        match target {
            DiagTarget::Phy(p) => self.covers(p),
            DiagTarget::Global => *self == Self::All,
        }
    }
}

impl fmt::Display for LeaseScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all PHYs"),
            Self::Phys(set) => {
                f.write_str("phy ")?;
                for (i, p) in set.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "0x{p:x}")?;
                }
                Ok(())
            }
        }
    }
}

/// What one raw diag access addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagTarget {
    /// One PHY (a clause-45 access to an address word without the broadcast bit).
    Phy(u16),
    /// No single PHY: a broadcast address, clause 22, another bus, a line that does not parse.
    Global,
}

impl fmt::Display for DiagTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phy(p) => write!(f, "phy 0x{p:x}"),
            Self::Global => f.write_str("no single PHY"),
        }
    }
}

/// Who makes a raw diag access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagWho {
    /// A client without a transaction (today's tools): an implicit lease of `holdoff-ms`.
    Bare,
    /// The holder of transaction `n`.
    Txn(u64),
}

/// Why a diag request is not served. `Display` is the client's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxnRefused {
    /// A begin whose PHYs another transaction holds (`txn`), or that library users kept waiting
    /// for longer than [`LEASE_MAX_MS`] (`txn` `None`).
    Busy {
        /// The PHYs asked for.
        scope: LeaseScope,
        /// The transaction in the way.
        txn: Option<u64>,
        /// How long it lasts at most.
        left_ms: u64,
    },
    /// No such transaction (never begun, or ended).
    Unknown(u64),
    /// The transaction's lease ran out before this request: what the PHY did since is unknown.
    Expired {
        /// The transaction.
        txn: u64,
        /// How long it lived.
        lived_ms: u64,
        /// Its accesses.
        accesses: u64,
    },
    /// An access of a transaction to a PHY it does not hold.
    OutOfScope {
        /// The transaction.
        txn: u64,
        /// What it holds.
        scope: LeaseScope,
        /// What the access addresses.
        target: DiagTarget,
    },
    /// An access without a transaction to a PHY another client's transaction holds.
    InTxn {
        /// What the access addresses.
        target: DiagTarget,
        /// The transaction that holds it.
        txn: u64,
        /// How long it lasts at most without another access.
        left_ms: u64,
    },
    /// A begin on no PHY at all.
    NoPhys,
}

impl fmt::Display for TxnRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy {
                scope,
                txn: Some(t),
                left_ms,
            } => write!(
                f,
                "extphy txn: {scope}: held by transaction {t} for at most {left_ms} ms more; try again"
            ),
            Self::Busy { scope, txn: None, .. } => write!(
                f,
                "extphy txn: {scope}: the library's own MDIO users did not get their turn in time; try again"
            ),
            Self::Unknown(t) => write!(f, "extphy txn {t}: no such transaction"),
            Self::Expired {
                txn,
                lived_ms,
                accesses,
            } => write!(
                f,
                "extphy txn {txn}: expired (its lease ran out after {lived_ms} ms, {accesses} accesses): \
                 what the PHY did since is unknown; begin a new transaction"
            ),
            Self::OutOfScope { txn, scope, target } => {
                write!(f, "extphy txn {txn}: holds {scope}; this access addresses {target}")?;
                if *target == DiagTarget::Global {
                    f.write_str(" (broadcast, clause 22, another bus or not parsed): it needs a transaction on all")?;
                }
                Ok(())
            }
            Self::InTxn { target, txn, left_ms } => write!(
                f,
                "{target} is in diag transaction {txn} (for {left_ms} ms more unless it is renewed): \
                 send 'extphy txn {txn} <command>' as its holder, or wait"
            ),
            Self::NoPhys => f.write_str("extphy txn begin: no PHY named"),
        }
    }
}

/// A transaction as it was begun or renewed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxnInfo {
    /// Its number (the client's token).
    pub txn: u64,
    /// What it holds.
    pub scope: LeaseScope,
    /// How long each access (and this request) extends it.
    pub step_ms: u64,
    /// How long it lasts now without another access.
    pub left_ms: u64,
    /// How long it lasts at most.
    pub max_left_ms: u64,
}

/// A transaction that ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxnEnded {
    /// Its number.
    pub txn: u64,
    /// Its accesses.
    pub accesses: u64,
    /// From its begin to its end.
    pub held_ms: u64,
}

/// A lease as the status text shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseView {
    /// The transaction (`None`: an implicit lease of raw MDIO without a transaction).
    pub txn: Option<u64>,
    /// What it holds.
    pub scope: LeaseScope,
    /// How long it lasts without another access.
    pub left_ms: u64,
    /// How long it lasts at most (`None`: an implicit lease has no maximum).
    pub max_left_ms: Option<u64>,
    /// Its accesses.
    pub accesses: u64,
}

/// What the arbitration did since the runtime was made (the status text's counters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArbCounts {
    /// Transactions begun.
    pub txn_begun: u64,
    /// Transactions ended by their client.
    pub txn_ended: u64,
    /// Transactions whose lease ran out.
    pub txn_expired: u64,
    /// Implicit leases begun (raw MDIO without a transaction; the C++ "hold-offs").
    pub implicit_begun: u64,
    /// Begins refused because the PHYs were held.
    pub busy_refused: u64,
    /// Requests of a transaction that had expired, was unknown or addressed a PHY it does not
    /// hold, and bare accesses refused inside another client's transaction.
    pub void_refused: u64,
    /// Link polls answered from the last verdict because a lease held one of their PHYs.
    pub gate_cached: u64,
    /// Link polls that waited for a library holder of their PHYs.
    pub gate_waits: u64,
    /// The longest of those waits, us (measured from before the wait).
    pub gate_wait_us_max: u64,
    /// All of those waits, us.
    pub gate_wait_us_total: u64,
    /// `MACsec` periodic polls answered from the last read because a lease held their PHY.
    pub polls_skipped: u64,
    /// `MACsec` polls run after [`crate::MACSEC_POLL_DEFER_MAX_MS`] of skips.
    pub forced_reads: u64,
    /// The longest wait of a forced read for a transaction to end, us.
    pub forced_wait_us_max: u64,
    /// Library runs (`MACsec` changes, admin sequences) that waited for a transaction.
    pub op_waits: u64,
    /// The longest of those waits, us.
    pub op_wait_us_max: u64,
    /// Diag-socket `extmacsec run`s refused because a transaction held their PHY.
    pub diag_run_refused: u64,
}

/// What a library user does about a lease on one of its PHYs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibWant {
    /// The link gate and the periodic `MACsec` polls: any lease answers "leased" (the cached
    /// verdict, the last read); they wait only for slots.
    Skip,
    /// A `SONiC` change, an admin sequence, a forced poll: waits for an explicit lease to end or
    /// run out, passes implicit ones.
    Wait,
    /// `extmacsec run` from the diag socket: an explicit lease answers "leased" (refused).
    NoWait,
}

/// The outcome of a library user's request.
pub(crate) enum LibTurn<'a> {
    /// The PHYs are the caller's until the slot is dropped.
    Slot {
        slot: Slot<'a>,
        /// From the request to the slot, us; 0 when the PHYs were free at once.
        waited_us: u64,
        /// The transaction it waited for (not only for slots).
        for_txn: Option<u64>,
        /// A lease (a transaction or an implicit one) held one of the PHYs when it asked.
        leased: bool,
    },
    /// A lease holds one of the PHYs (the transaction, `None` for an implicit lease).
    Leased(Option<u64>),
}

/// Held PHYs; given back when dropped.
pub(crate) struct Slot<'a> {
    arb: &'a Arbiter,
    phys: Vec<u16>,
    global: bool,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut a = self.arb.lock();
        for p in &self.phys {
            a.held.remove(p);
        }
        if self.global {
            a.global_held = false;
        }
        drop(a);
        self.arb.cv.notify_all();
    }
}

#[derive(Debug)]
struct Lease {
    scope: LeaseScope,
    begun_ms: u64,
    until_ms: u64,
    /// `u64::MAX` for an implicit lease.
    max_until_ms: u64,
    step_ms: u64,
    accesses: u64,
}

impl Lease {
    fn renew(&mut self, now_ms: u64) {
        self.until_ms = now_ms.saturating_add(self.step_ms).min(self.max_until_ms);
        self.accesses += 1;
    }

    fn view(&self, txn: Option<u64>, now_ms: u64) -> LeaseView {
        LeaseView {
            txn,
            scope: self.scope.clone(),
            left_ms: self.until_ms.saturating_sub(now_ms),
            max_left_ms: txn.map(|_| self.max_until_ms.saturating_sub(now_ms)),
            accesses: self.accesses,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// Explicit leases by transaction.
    txns: BTreeMap<u64, Lease>,
    /// Implicit leases by PHY (`None`: raw MDIO that named no single PHY).
    implicit: BTreeMap<Option<u16>, Lease>,
    /// Transactions that ran out: (number, lived ms, accesses).
    expired: VecDeque<(u64, u64, u64)>,
    held: BTreeSet<u16>,
    global_held: bool,
    /// Library users waiting, by PHY.
    pending: BTreeMap<u16, u32>,
    /// Library users waiting (each once, whatever its PHYs).
    pending_users: u32,
    /// Diag accesses waiting for the global slot.
    waiting_global: u32,
    next_txn: u64,
    counts: ArbCounts,
    /// Log lines made under the lock, for the caller to log after it.
    notes: Vec<String>,
}

impl State {
    /// Forgets what ran out by `now_ms`; whether anything did.
    fn reap(&mut self, now_ms: u64) -> bool {
        let gone: Vec<u64> = self
            .txns
            .iter()
            .filter(|(_, l)| now_ms >= l.until_ms)
            .map(|(t, _)| *t)
            .collect();
        let any = !gone.is_empty();
        for t in gone {
            if let Some(l) = self.txns.remove(&t) {
                let lived = now_ms.saturating_sub(l.begun_ms);
                self.counts.txn_expired += 1;
                self.notes.push(format!(
                    "external PHY diag transaction {t} ({}) expired after {lived} ms ({} accesses) without an end: \
                     its next request is refused",
                    l.scope, l.accesses
                ));
                self.expired.push_back((t, lived, l.accesses));
                while self.expired.len() > EXPIRED_KEPT {
                    self.expired.pop_front();
                }
            }
        }
        let before = self.implicit.len();
        self.implicit.retain(|_, l| now_ms < l.until_ms);
        any || self.implicit.len() != before
    }

    /// The transaction that holds one of `phys`.
    fn txn_over(&self, phys: &[u16]) -> Option<(u64, &Lease)> {
        self.txns
            .iter()
            .find(|(_, l)| l.scope.overlaps_any(phys))
            .map(|(t, l)| (*t, l))
    }

    fn implicit_over(&self, phys: &[u16]) -> bool {
        !phys.is_empty()
            && (self.implicit.contains_key(&None) || phys.iter().any(|p| self.implicit.contains_key(&Some(*p))))
    }

    fn slots_free(&self, phys: &[u16]) -> bool {
        phys.is_empty()
            || (!self.global_held && self.waiting_global == 0 && phys.iter().all(|p| !self.held.contains(p)))
    }

    fn pend(&mut self, phys: &[u16], up: bool) {
        if up {
            self.pending_users += 1;
        } else {
            self.pending_users = self.pending_users.saturating_sub(1);
        }
        for p in phys {
            if up {
                *self.pending.entry(*p).or_insert(0) += 1;
            } else if let Some(n) = self.pending.get_mut(p) {
                *n -= 1;
                if *n == 0 {
                    self.pending.remove(p);
                }
            }
        }
    }

    /// A slot of `scope` is held (a library run or a diag access in progress).
    fn held_in(&self, scope: &LeaseScope) -> bool {
        self.global_held
            || match scope {
                LeaseScope::All => !self.held.is_empty(),
                LeaseScope::Phys(set) => set.iter().any(|p| self.held.contains(p)),
            }
    }

    fn pending_in(&self, scope: &LeaseScope) -> bool {
        match scope {
            LeaseScope::All => !self.pending.is_empty(),
            LeaseScope::Phys(set) => set.iter().any(|p| self.pending.contains_key(p)),
        }
    }

    fn unknown_or_expired(&mut self, txn: u64) -> TxnRefused {
        self.counts.void_refused += 1;
        let found = self.expired.iter().rev().find(|(t, _, _)| *t == txn);
        match found {
            Some(&(_, lived_ms, accesses)) => TxnRefused::Expired {
                txn,
                lived_ms,
                accesses,
            },
            None => TxnRefused::Unknown(txn),
        }
    }
}

/// The arbiter (one per switch, inside [`crate::ExtPhy`]).
#[derive(Debug, Default)]
pub(crate) struct Arbiter {
    m: Mutex<State>,
    cv: Condvar,
}

fn ms(clock: &dyn Clock) -> u64 {
    clock.now_us() / 1000
}

impl Arbiter {
    fn lock(&self) -> Leaf<'_, State> {
        leaf::lock(&self.m)
    }

    /// Waits a little (the lock is given up meanwhile), then the caller looks again.
    fn pause<'a>(&'a self, guard: Leaf<'a, State>) -> Leaf<'a, State> {
        guard.wait(&self.cv, RECHECK)
    }

    /// The log lines made since the last call.
    pub(crate) fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().notes)
    }

    /// Library users waiting for their PHYs now (tests: a deterministic handshake).
    #[cfg(feature = "testing")]
    pub(crate) fn waiting(&self) -> u32 {
        self.lock().pending_users
    }

    pub(crate) fn counts(&self) -> ArbCounts {
        self.lock().counts
    }

    pub(crate) fn count(&self, f: impl FnOnce(&mut ArbCounts)) {
        f(&mut self.lock().counts);
    }

    /// The live leases, transactions first (in their order), implicit ones after.
    pub(crate) fn leases(&self, clock: &dyn Clock) -> Vec<LeaseView> {
        let now = ms(clock);
        let mut a = self.lock();
        let reaped = a.reap(now);
        let mut out: Vec<LeaseView> = a.txns.iter().map(|(t, l)| l.view(Some(*t), now)).collect();
        out.extend(a.implicit.values().map(|l| l.view(None, now)));
        drop(a);
        if reaped {
            self.cv.notify_all();
        }
        out
    }

    /// The PHYs `phys` for a library user (module documentation). `phys` sorted, no doubles;
    /// none: nothing to wait for.
    pub(crate) fn lib(&self, clock: &dyn Clock, phys: &[u16], want: LibWant) -> LibTurn<'_> {
        let t0 = clock.now_us();
        let mut a = self.lock();
        let mut pending = false;
        let mut for_txn = None;
        let mut first = true;
        let mut leased_at_ask = false;
        loop {
            if a.reap(ms(clock)) {
                self.cv.notify_all();
            }
            let txn = a.txn_over(phys).map(|(t, _)| t);
            if first {
                leased_at_ask = txn.is_some() || a.implicit_over(phys);
                first = false;
            }
            let leased = match want {
                LibWant::Skip => txn.is_some() || a.implicit_over(phys),
                LibWant::NoWait => txn.is_some(),
                LibWant::Wait => false,
            };
            if leased {
                if pending {
                    a.pend(phys, false);
                    drop(a);
                    self.cv.notify_all();
                }
                return LibTurn::Leased(txn);
            }
            if txn.is_some() {
                for_txn = txn;
            } else if a.slots_free(phys) {
                if pending {
                    a.pend(phys, false);
                }
                for p in phys {
                    a.held.insert(*p);
                }
                drop(a);
                if pending {
                    self.cv.notify_all();
                }
                return LibTurn::Slot {
                    slot: Slot {
                        arb: self,
                        phys: phys.to_vec(),
                        global: false,
                    },
                    waited_us: if pending {
                        clock.now_us().saturating_sub(t0).max(1)
                    } else {
                        0
                    },
                    for_txn,
                    leased: leased_at_ask,
                };
            }
            if !pending {
                a.pend(phys, true);
                pending = true;
            }
            a = self.pause(a);
        }
    }

    /// Begins a transaction on `scope`, lasting `step_ms` after each access, at most
    /// [`LEASE_MAX_MS`] in all. Refused while another transaction holds one of its PHYs; waits
    /// (at most [`LEASE_MAX_MS`]) for the library runs on those PHYs and for the library users
    /// waiting for them, so that no library run overlaps a transaction.
    pub(crate) fn begin(&self, clock: &dyn Clock, scope: LeaseScope, step_ms: u64) -> Result<TxnInfo, TxnRefused> {
        if scope == LeaseScope::Phys(BTreeSet::new()) {
            return Err(TxnRefused::NoPhys);
        }
        let step_ms = step_ms.clamp(1, LEASE_MAX_MS);
        let start = ms(clock);
        let mut a = self.lock();
        loop {
            let now = ms(clock);
            if a.reap(now) {
                self.cv.notify_all();
            }
            let holder = a
                .txns
                .iter()
                .find(|(_, l)| l.scope.overlaps(&scope))
                .map(|(t, l)| (*t, l.max_until_ms.min(l.until_ms).saturating_sub(now)));
            if let Some((t, left_ms)) = holder {
                a.counts.busy_refused += 1;
                return Err(TxnRefused::Busy {
                    scope,
                    txn: Some(t),
                    left_ms,
                });
            }
            if !a.pending_in(&scope) && !a.held_in(&scope) {
                a.next_txn += 1;
                let txn = a.next_txn;
                let lease = Lease {
                    scope: scope.clone(),
                    begun_ms: now,
                    until_ms: now.saturating_add(step_ms),
                    max_until_ms: now.saturating_add(LEASE_MAX_MS),
                    step_ms,
                    accesses: 0,
                };
                a.txns.insert(txn, lease);
                a.counts.txn_begun += 1;
                return Ok(TxnInfo {
                    txn,
                    scope,
                    step_ms,
                    left_ms: step_ms,
                    max_left_ms: LEASE_MAX_MS,
                });
            }
            if now.saturating_sub(start) >= LEASE_MAX_MS {
                a.counts.busy_refused += 1;
                return Err(TxnRefused::Busy {
                    scope,
                    txn: None,
                    left_ms: 0,
                });
            }
            a = self.pause(a);
        }
    }

    /// Extends transaction `txn` (as an access does, without counting one), with a new step
    /// when one is given.
    pub(crate) fn renew(&self, clock: &dyn Clock, txn: u64, step_ms: Option<u64>) -> Result<TxnInfo, TxnRefused> {
        let now = ms(clock);
        let mut a = self.lock();
        if a.reap(now) {
            self.cv.notify_all();
        }
        let Some(l) = a.txns.get_mut(&txn) else {
            return Err(a.unknown_or_expired(txn));
        };
        if let Some(s) = step_ms {
            l.step_ms = s.clamp(1, LEASE_MAX_MS);
        }
        l.until_ms = now.saturating_add(l.step_ms).min(l.max_until_ms);
        Ok(TxnInfo {
            txn,
            scope: l.scope.clone(),
            step_ms: l.step_ms,
            left_ms: l.until_ms.saturating_sub(now),
            max_left_ms: l.max_until_ms.saturating_sub(now),
        })
    }

    /// Ends transaction `txn`; an expired one is an error (its client's results are void).
    pub(crate) fn end(&self, clock: &dyn Clock, txn: u64) -> Result<TxnEnded, TxnRefused> {
        let now = ms(clock);
        let mut a = self.lock();
        a.reap(now);
        let ended = match a.txns.remove(&txn) {
            Some(l) => {
                a.counts.txn_ended += 1;
                Ok(TxnEnded {
                    txn,
                    accesses: l.accesses,
                    held_ms: now.saturating_sub(l.begun_ms),
                })
            }
            None => Err(a.unknown_or_expired(txn)),
        };
        drop(a);
        self.cv.notify_all();
        ended
    }

    /// One raw diag access to `target` by `who`: the lease checks and renewals, then the slot.
    /// `implicit_ms`: how long a bare access holds its PHY's polls off (`holdoff-ms`; 0: not at
    /// all). The slot is held for the access only.
    pub(crate) fn diag(
        &self,
        clock: &dyn Clock,
        who: DiagWho,
        target: DiagTarget,
        implicit_ms: u64,
    ) -> Result<Slot<'_>, TxnRefused> {
        let phys: Vec<u16> = match target {
            DiagTarget::Phy(p) => vec![p],
            DiagTarget::Global => Vec::new(),
        };
        let mut a = self.lock();
        let mut first = true;
        let mut waiting = false;
        loop {
            let now = ms(clock);
            if a.reap(now) {
                self.cv.notify_all();
            }
            let refused = match who {
                DiagWho::Txn(t) => match a.txns.get_mut(&t) {
                    None => Some(a.unknown_or_expired(t)),
                    Some(l) if !l.scope.holds(target) => {
                        let scope = l.scope.clone();
                        a.counts.void_refused += 1;
                        Some(TxnRefused::OutOfScope { txn: t, scope, target })
                    }
                    Some(l) => {
                        if first {
                            l.renew(now);
                        }
                        None
                    }
                },
                // every time: a transaction begun while this access waited for its slot wins
                DiagWho::Bare => {
                    let holder = match target {
                        DiagTarget::Phy(_) => a.txn_over(&phys),
                        DiagTarget::Global => a.txns.iter().next().map(|(t, l)| (*t, l)),
                    }
                    .map(|(t, l)| (t, l.until_ms.saturating_sub(now)));
                    if let Some((txn, left_ms)) = holder {
                        a.counts.void_refused += 1;
                        Some(TxnRefused::InTxn { target, txn, left_ms })
                    } else {
                        if first && implicit_ms > 0 {
                            Self::implicit(&mut a, target, implicit_ms, now);
                        }
                        None
                    }
                }
            };
            if let Some(refused) = refused {
                if waiting {
                    a.waiting_global -= 1;
                    drop(a);
                    self.cv.notify_all();
                }
                return Err(refused);
            }
            first = false;
            let free = match target {
                DiagTarget::Phy(_) => !a.global_held && phys.iter().all(|p| !a.held.contains(p)),
                DiagTarget::Global => !a.global_held && a.held.is_empty(),
            };
            if free {
                if waiting {
                    a.waiting_global -= 1;
                }
                let global = target == DiagTarget::Global;
                if global {
                    a.global_held = true;
                } else {
                    for p in &phys {
                        a.held.insert(*p);
                    }
                }
                return Ok(Slot {
                    arb: self,
                    phys,
                    global,
                });
            }
            if target == DiagTarget::Global && !waiting {
                a.waiting_global += 1;
                waiting = true;
            }
            a = self.pause(a);
        }
    }

    /// Begins or extends the implicit lease of a bare access.
    fn implicit(a: &mut State, target: DiagTarget, implicit_ms: u64, now: u64) {
        let key = match target {
            DiagTarget::Phy(p) => Some(p),
            DiagTarget::Global => None,
        };
        if let Some(l) = a.implicit.get_mut(&key) {
            l.step_ms = implicit_ms;
            l.renew(now);
            return;
        }
        let mut l = Lease {
            scope: key.map_or(LeaseScope::All, |p| LeaseScope::Phys(BTreeSet::from([p]))),
            begun_ms: now,
            until_ms: now,
            max_until_ms: u64::MAX,
            step_ms: implicit_ms,
            accesses: 0,
        };
        l.renew(now);
        a.notes.push(format!(
            "external PHY {}: link polls and MACsec polls paused: raw MDIO on the diag socket without a \
             transaction (resume {implicit_ms} ms after the last one)",
            l.scope
        ));
        a.implicit.insert(key, l);
        a.counts.implicit_begun += 1;
    }
}

//! The SDK's callbacks: link changes, L2 learn / age events, and the per-port link gate.
//!
//! # How it is made safe
//!
//! The glue takes C function pointers and a `void* ctx` it hands back on every call
//! (`ob_sdk_register_callbacks` for link + L2, `ob_sdk_linkscan_port_gate` for the gate), and calls
//! them on SDK threads (link scan, L2X; also the thread of a port call that changes the link).
//! The glue keeps **one** context for link and L2 and **one** gate function and context for
//! all ports, in process globals (`g_cb_ctx`, `g_gate_cb` / `g_gate_ctx`); the C++ backend
//! passes `this` and looks the port up in a map under `gate_mu_`. Here no memory is handed over:
//!
//! * the closures live in one process-wide table behind a mutex ([`REGISTRY`]), one entry per
//!   [`Switch`], keyed by a number that is never reused; ports map to their gate in the entry;
//! * `ctx` is that key as an address-sized number, compared, never dereferenced;
//! * the C callbacks are this crate's `extern "C"` trampolines ([`on_link`], [`on_l2`],
//!   [`on_gate`]): each looks its entry up, **checks its arguments** (port non-negative, a
//!   LAG destination as `OB_LAG_DEST_BIT | tid`, vid 1..4094, flags 0 / 1, the MAC pointer not
//!   null), takes a clone of the closure's `Arc`, releases the table, and calls the closure under
//!   `catch_unwind`: a panic never unwinds into the SDK; it is counted ([`CallbackStats`]) and,
//!   for the gate, answered "down". A refused call and a panic are also reported to the
//!   switch's reporter, if it has one ([`Switch::set_callback_reporter`], [`CallbackIssue`]):
//!   how the caller logs them; this crate writes nothing itself.
//!
//! So a late or stray call (after unregistering, from an earlier registration, with any `ctx`
//! or argument) finds no entry, no closure, or is refused, and does nothing. No lifetime
//! depends on the SDK's behaviour: the closures are `Send + Sync + 'static` and kept alive by
//! their `Arc` until the last call that cloned it returns. The switch still unregisters (gates
//! first, then link + L2) before it shuts the BCM layer down, so the SDK stops calling.
//!
//! # For the closures
//!
//! They run on SDK threads: quick, no blocking (send on a channel, set a flag). They cannot
//! call the switch: they have no [`Switch`]. Calls are not serialised by this crate (the table is
//! not held during a call): after a `clear_*` returns, a call already running on another thread
//! may still finish; none starts. The gate runs inside the SDK's link scan pass, which waits
//! for it; its MDIO goes through a [`crate::MdioBus`].

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::error::{Error, check};
use crate::l2::Dest;
use crate::switch::Switch;

/// What a link gate says about its port on one link scan pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The port is down whatever the ASIC says (the glue's 0).
    Down,
    /// The port is up (the glue's 1).
    Up,
    /// No opinion: the SDK reads the ASIC's link as usual (the glue's -1).
    NoOpinion,
}

impl Gate {
    const fn raw(self) -> c_int {
        match self {
            Self::Down => 0,
            Self::Up => 1,
            Self::NoOpinion => -1,
        }
    }
}

/// An L2 learn / age event (`ob_l2_cb_t`): static entries are not reported (the glue skips them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct L2Event {
    /// The MAC address.
    pub mac: [u8; 6],
    /// The VLAN, 1..=4094.
    pub vid: u16,
    /// Where it was learned: a port, or a LAG (SDK trunk).
    pub dest: Dest,
    /// Learned (add, learn, move), else aged / deleted by hardware.
    pub learned: bool,
}

/// Callback calls this switch's trampolines handled abnormally.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallbackStats {
    /// Link closures that panicked (the event is lost).
    pub link_panics: u64,
    /// L2 closures that panicked.
    pub l2_panics: u64,
    /// Gate closures that panicked (answered "down").
    pub gate_panics: u64,
    /// Calls with arguments out of range, refused before any closure ran.
    pub rejected: u64,
}

/// Which trampoline a [`CallbackIssue`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callback {
    /// `ob_link_cb_t`
    Link,
    /// `ob_l2_cb_t`
    L2,
    /// `ob_gate_cb_t`
    Gate,
}

impl Callback {
    const fn name(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::L2 => "L2",
            Self::Gate => "link gate",
        }
    }
}

/// A callback call that went wrong, as the switch's reporter is told
/// ([`Switch::set_callback_reporter`]); also counted in [`CallbackStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackIssue {
    /// The closure panicked: a link or L2 event is lost, a gate answered "down".
    Panicked {
        /// Which callback.
        callback: Callback,
        /// The port the SDK named (for L2: the glue's destination, a port or `OB_LAG_DEST_BIT | tid`).
        port: i32,
    },
    /// Arguments out of range: refused before any closure ran.
    Rejected {
        /// Which callback.
        callback: Callback,
        /// The port the SDK named.
        port: i32,
    },
}

impl CallbackIssue {
    /// Which callback it is about.
    pub const fn callback(self) -> Callback {
        match self {
            Self::Panicked { callback, .. } | Self::Rejected { callback, .. } => callback,
        }
    }
}

impl core::fmt::Display for CallbackIssue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Panicked {
                callback: Callback::Gate,
                port,
            } => write!(f, "link gate of port {port} panicked: answered down"),
            Self::Panicked { callback, port } => {
                write!(f, "{} callback of port {port} panicked: event lost", callback.name())
            }
            Self::Rejected { callback, port } => write!(
                f,
                "{} callback of port {port}: arguments out of range, refused",
                callback.name()
            ),
        }
    }
}

#[derive(Debug, Default)]
struct Counters {
    link_panics: AtomicU64,
    l2_panics: AtomicU64,
    gate_panics: AtomicU64,
    rejected: AtomicU64,
}

impl Counters {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

type LinkFn = dyn Fn(i32, bool) + Send + Sync;
type L2Fn = dyn Fn(L2Event) + Send + Sync;
type GateFn = dyn Fn() -> Gate + Send + Sync;
type ReportFn = dyn Fn(CallbackIssue) + Send + Sync;

#[derive(Default)]
struct Entry {
    link: Option<Arc<LinkFn>>,
    l2: Option<Arc<L2Fn>>,
    gates: BTreeMap<c_int, Arc<GateFn>>,
    counters: Arc<Counters>,
    report: Option<Arc<ReportFn>>,
}

/// What a trampoline needs besides its closure: the counters and the reporter.
struct Sinks {
    counters: Arc<Counters>,
    report: Option<Arc<ReportFn>>,
}

impl Sinks {
    /// Counts `issue` and tells the reporter (a reporter that panics is ignored).
    fn note(&self, issue: CallbackIssue) {
        let counter = match issue {
            CallbackIssue::Rejected { .. } => &self.counters.rejected,
            CallbackIssue::Panicked {
                callback: Callback::Link,
                ..
            } => &self.counters.link_panics,
            CallbackIssue::Panicked {
                callback: Callback::L2, ..
            } => &self.counters.l2_panics,
            CallbackIssue::Panicked {
                callback: Callback::Gate,
                ..
            } => &self.counters.gate_panics,
        };
        Counters::bump(counter);
        if let Some(report) = &self.report {
            let _ = catch_unwind(AssertUnwindSafe(|| report(issue)));
        }
    }
}

/// Every registration of this process, by key. No `static mut`: a const-initialised mutex.
static REGISTRY: Mutex<BTreeMap<usize, Entry>> = Mutex::new(BTreeMap::new());
/// The next key; 0 is never one.
static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);

/// The table, also after a panic while it was held (it holds no invariant a panic could
/// break; closures never run under it).
fn registry() -> MutexGuard<'static, BTreeMap<usize, Entry>> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A closure of the entry `key` (cloned) and its counters and reporter, with the table released
/// again.
fn lookup<T>(key: usize, pick: impl FnOnce(&Entry) -> Option<T>) -> Option<(T, Sinks)> {
    let table = registry();
    let entry = table.get(&key)?;
    let sinks = Sinks {
        counters: Arc::clone(&entry.counters),
        report: entry.report.clone(),
    };
    pick(entry).map(|t| (t, sinks))
}

/// `ob_link_cb_t`. Safe to call with any arguments from any thread; never unwinds.
pub(crate) extern "C" fn on_link(ctx: *mut c_void, port: c_int, up: c_int) {
    let key = ctx.addr();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some((handler, sinks)) = lookup(key, |e| e.link.clone()) else {
            return;
        };
        let callback = Callback::Link;
        if port < 0 || !(up == 0 || up == 1) {
            sinks.note(CallbackIssue::Rejected { callback, port });
            return;
        }
        if catch_unwind(AssertUnwindSafe(|| handler(port, up == 1))).is_err() {
            sinks.note(CallbackIssue::Panicked { callback, port });
        }
    }));
}

/// The L2 event of the glue's raw arguments, `None` when one is out of range.
fn l2_event(mac: [u8; 6], vid: c_int, port: c_int, learned: c_int) -> Option<L2Event> {
    let vid = u16::try_from(vid).ok().filter(|v| (1..=4094).contains(v))?;
    let dest = Dest::from_raw(port)?;
    let learned = match learned {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(L2Event {
        mac,
        vid,
        dest,
        learned,
    })
}

/// `ob_l2_cb_t`. Never unwinds.
///
/// # Safety
///
/// `mac` is null or points to 6 bytes readable for the duration of the call (the glue passes
/// the `bcm_mac_t` of the SDK's `bcm_l2_addr_t` it was called with). Any other argument may have
/// any value.
pub(crate) unsafe extern "C" fn on_l2(ctx: *mut c_void, mac: *const u8, vid: c_int, port: c_int, learned: c_int) {
    let key = ctx.addr();
    let bytes = if mac.is_null() {
        None
    } else {
        // SAFETY: not null, and by this function's contract 6 readable bytes; read as an
        // unaligned array (u8 has no alignment anyway), copied out before anything else runs.
        Some(unsafe { mac.cast::<[u8; 6]>().read_unaligned() })
    };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some((handler, sinks)) = lookup(key, |e| e.l2.clone()) else {
            return;
        };
        let callback = Callback::L2;
        let Some(event) = bytes.and_then(|mac| l2_event(mac, vid, port, learned)) else {
            sinks.note(CallbackIssue::Rejected { callback, port });
            return;
        };
        if catch_unwind(AssertUnwindSafe(|| handler(event))).is_err() {
            sinks.note(CallbackIssue::Panicked { callback, port });
        }
    }));
}

/// `ob_gate_cb_t`: the port's gate, -1 (no opinion) without one. A gate that panics answers
/// "down" (0): the port stays out of forwarding until the gate answers again. Safe to call
/// with any arguments from any thread; never unwinds.
pub(crate) extern "C" fn on_gate(ctx: *mut c_void, port: c_int) -> c_int {
    let key = ctx.addr();
    catch_unwind(AssertUnwindSafe(|| {
        let Some((gate, sinks)) = lookup(key, |e| e.gates.get(&port).cloned()) else {
            return Gate::NoOpinion.raw();
        };
        if let Ok(answer) = catch_unwind(AssertUnwindSafe(|| gate())) {
            answer.raw()
        } else {
            sinks.note(CallbackIssue::Panicked {
                callback: Callback::Gate,
                port,
            });
            Gate::Down.raw()
        }
    }))
    .unwrap_or(Gate::Down.raw())
}

/// A switch's registrations: its key in [`REGISTRY`] and what the glue was told.
#[derive(Debug)]
pub(crate) struct Callbacks {
    key: usize,
    /// Link / L2 pointers are stored in the glue (also after a refused registration).
    registered: bool,
    /// Ports with a gate registered in the glue.
    gated: BTreeSet<c_int>,
    counters: Arc<Counters>,
}

impl Callbacks {
    pub(crate) fn new() -> Self {
        let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
        let entry = Entry::default();
        let counters = Arc::clone(&entry.counters);
        registry().insert(key, entry);
        Self {
            key,
            registered: false,
            gated: BTreeSet::new(),
            counters,
        }
    }
}

impl Switch {
    /// Registers `handler` for link changes: called with the SDK's logical port number and the
    /// new state (true = up), on an SDK thread (see the module documentation for what it may
    /// do). Replaces an earlier link handler; an L2 handler stays. On an error nothing is
    /// registered (no link, no L2 handler) and `handler` is dropped.
    pub fn set_link_callback(&mut self, handler: impl Fn(i32, bool) + Send + Sync + 'static) -> Result<(), Error> {
        let l2 = lookup(self.cb.key, |e| e.l2.clone()).map(|(h, _)| h);
        self.apply_callbacks(Some(Arc::new(handler)), l2)
    }

    /// Registers `handler` for L2 learn / age events ([`L2Event`]), on an SDK thread (the L2X
    /// thread). Replaces an earlier L2 handler; a link handler stays. On an error nothing is
    /// registered.
    pub fn set_l2_callback(&mut self, handler: impl Fn(L2Event) + Send + Sync + 'static) -> Result<(), Error> {
        let link = lookup(self.cb.key, |e| e.link.clone()).map(|(h, _)| h);
        self.apply_callbacks(link, Some(Arc::new(handler)))
    }

    /// Registers `link` for link changes and `l2` for L2 learn / age events together, with one
    /// `ob_sdk_register_callbacks` (what the C++ backend's init does): each as
    /// [`Switch::set_link_callback`] / [`Switch::set_l2_callback`] say. Replaces earlier handlers.
    /// On an error nothing is registered and both are dropped.
    pub fn set_callbacks(
        &mut self,
        link: impl Fn(i32, bool) + Send + Sync + 'static,
        l2: impl Fn(L2Event) + Send + Sync + 'static,
    ) -> Result<(), Error> {
        self.apply_callbacks(Some(Arc::new(link)), Some(Arc::new(l2)))
    }

    /// Unregisters the link handler (an L2 handler is registered again alone). No effect
    /// without one.
    pub fn clear_link_callback(&mut self) {
        if !self.cb.registered {
            return;
        }
        let l2 = lookup(self.cb.key, |e| e.l2.clone()).map(|(h, _)| h);
        if self.apply_callbacks(None, l2).is_err() {
            // a refused re-registration has cleared both: nothing is registered
        }
    }

    /// Unregisters the link and the L2 handler and drops them. No effect without either.
    pub fn clear_callbacks(&mut self) {
        let _ = self.apply_callbacks(None, None);
    }

    /// The glue holds one registration of both pointers: unregister, store, register again.
    fn apply_callbacks(&mut self, link: Option<Arc<LinkFn>>, l2: Option<Arc<L2Fn>>) -> Result<(), Error> {
        let (unit, key) = (self.raw_unit(), self.cb.key);
        if self.cb.registered {
            // [I2]: &mut self.
            self.glue().unregister_callbacks(unit);
            self.cb.registered = false;
        }
        let (want_link, want_l2) = (link.is_some(), l2.is_some());
        let old = registry()
            .get_mut(&key)
            .map(|e| (std::mem::replace(&mut e.link, link), std::mem::replace(&mut e.l2, l2)));
        // The old closures go with the table released: a closure's Drop may do anything.
        drop(old);
        if !want_link && !want_l2 {
            return Ok(());
        }
        let rv = self.glue().register_callbacks(unit, want_link, want_l2, key);
        // The glue stores the pointers before it asks the SDK and leaves them stored when the
        // SDK refuses: from here on it is "registered" until unregistered, also on failure.
        self.cb.registered = true;
        if let Err(error) = check(rv) {
            self.glue().unregister_callbacks(unit);
            self.cb.registered = false;
            let old = registry().get_mut(&key).map(|e| (e.link.take(), e.l2.take()));
            drop(old);
            return Err(error);
        }
        Ok(())
    }

    /// Registers a link gate for `port` (`ob_sdk_linkscan_port_gate`): the SDK's link scan asks it
    /// on every pass, before it reads the ASIC's link; [`Gate::Down`] takes the port down through
    /// the SDK's own path (MAC off, out of the forwarding bitmap, LED, link callback). Software
    /// link scan only: [`crate::Code::Config`] for a port in another mode (the gate would never
    /// be asked). Replaces the port's earlier gate. On an error the port has no gate.
    pub fn set_port_gate(&mut self, port: i32, gate: impl Fn() -> Gate + Send + Sync + 'static) -> Result<(), Error> {
        if port < 0 {
            return Err(Error::PORT);
        }
        let (unit, key) = (self.raw_unit(), self.cb.key);
        let old = registry()
            .get_mut(&key)
            .and_then(|e| e.gates.insert(port, Arc::new(gate)));
        drop(old);
        // [I2], [I3]: &mut self; the glue keeps one gate pointer and context for all ports, which
        // are this crate's trampoline and this switch's key every time.
        let rv = self.glue().linkscan_port_gate(unit, port, true, key);
        if let Err(error) = check(rv) {
            let old = registry().get_mut(&key).and_then(|e| e.gates.remove(&port));
            drop(old);
            return Err(error);
        }
        self.cb.gated.insert(port);
        Ok(())
    }

    /// Removes `port`'s link gate (the SDK reads the ASIC's link again). The glue is asked also
    /// for a port without a gate, as the C++ backend asks it (it answers success there: the
    /// SDK's `NOT_FOUND` is mapped to it); an error leaves a registered gate in place.
    pub fn clear_port_gate(&mut self, port: i32) -> Result<(), Error> {
        let (unit, key) = (self.raw_unit(), self.cb.key);
        check(self.glue().linkscan_port_gate(unit, port, false, key))?;
        self.cb.gated.remove(&port);
        let old = registry().get_mut(&key).and_then(|e| e.gates.remove(&port));
        drop(old);
        Ok(())
    }

    /// Sets the reporter of this switch's callbacks: called with every [`CallbackIssue`] (a
    /// refused call, a closure that panicked), on the SDK thread of the call, after it was
    /// counted. Quick and non-blocking, as the closures (a log line). Replaces an earlier one; no
    /// glue call. A reporter that panics is ignored.
    pub fn set_callback_reporter(&mut self, report: impl Fn(CallbackIssue) + Send + Sync + 'static) {
        let old = registry()
            .get_mut(&self.cb.key)
            .and_then(|e| e.report.replace(Arc::new(report)));
        drop(old);
    }

    /// The ports with a link gate.
    pub fn gated_ports(&self) -> Vec<i32> {
        self.cb.gated.iter().copied().collect()
    }

    /// How many callback calls of this switch went wrong ([`CallbackStats`]).
    pub fn callback_stats(&self) -> CallbackStats {
        let c = &self.cb.counters;
        CallbackStats {
            link_panics: c.link_panics.load(Ordering::Relaxed),
            l2_panics: c.l2_panics.load(Ordering::Relaxed),
            gate_panics: c.gate_panics.load(Ordering::Relaxed),
            rejected: c.rejected.load(Ordering::Relaxed),
        }
    }

    /// Everything this switch registered, undone (its `Drop`, before the BCM layer goes).
    pub(crate) fn unregister_all(&mut self) {
        let (unit, key) = (self.raw_unit(), self.cb.key);
        for port in std::mem::take(&mut self.cb.gated) {
            // Best effort: the SDK's NOT_FOUND is success; nothing else can be done here.
            let _ = self.glue().linkscan_port_gate(unit, port, false, key);
        }
        // No ob_sdk_unregister_callbacks here: the owner's drop calls ob_sdk_shutdown next (a
        // Switch is always the BCM layer), which unregisters the link and L2 callbacks inside the
        // glue, as the C++ backend's shutdown relies on. A callback the SDK makes in between finds
        // no entry below ([I3]) and does nothing.
        self.cb.registered = false;
        let entry = registry().remove(&key);
        drop(entry);
    }
}

#[cfg(feature = "rust-fake")]
pub(crate) mod raw {
    //! The trampolines as the Rust fake (and its tests) call them.
    use super::{c_int, c_void};

    pub(crate) fn link(key: usize, port: c_int, up: c_int) {
        super::on_link(std::ptr::without_provenance_mut::<c_void>(key), port, up);
    }

    pub(crate) fn l2(key: usize, mac: Option<&[u8; 6]>, vid: c_int, port: c_int, learned: c_int) {
        let ptr = mac.map_or(std::ptr::null(), |m| m.as_ptr());
        // SAFETY: ptr is null or points to the 6 bytes of a borrowed array, alive across the call:
        // on_l2's contract.
        unsafe { super::on_l2(std::ptr::without_provenance_mut::<c_void>(key), ptr, vid, port, learned) };
    }

    pub(crate) fn gate(key: usize, port: c_int) -> c_int {
        super::on_gate(std::ptr::without_provenance_mut::<c_void>(key), port)
    }
}

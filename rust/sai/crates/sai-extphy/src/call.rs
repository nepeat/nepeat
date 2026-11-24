//! The SAI calls of this thread and the SAI lock (crate documentation, "SAI calls and the SAI
//! lock"): what the rim tells the runtime so that **no library path waits for a diag lease while
//! it holds the SAI lock**.
//!
//! - [`under_sai_lock`]: the rim runs everything it does under the SAI lock inside it (a mark
//!   per thread). A library user that would wait for a transaction while marked is a bug of the
//!   claims below: asserted in debug builds, counted ([`crate::ArbCounts::locked_waits`]) and
//!   logged in release builds, where it waits (bounded, as before) rather than run inside the
//!   transaction or drop the change.
//! - [`call`]: one SAI call of the rim (it takes the SAI lock inside). The call's PHY changes
//!   **claim** their PHYs before their first effect ([`crate::ExtPhy::claim`]): a claim is taken
//!   at once when no transaction holds those PHYs; when one does, the claim is **deferred**: the
//!   call returns at once (nothing done), the rim lets go of the SAI lock, waits for that
//!   transaction to end (outside the lock, at most [`crate::LEASE_MAX_MS`]) holding the claim, and
//!   runs the call again. A forced `MACsec` read (its poll overdue,
//!   [`crate::MACSEC_POLL_DEFER_MAX_MS`]) that meets a transaction defers its get the same way.
//!   The call keeps its claims until it returns: no transaction can begin on them meanwhile, so
//!   the second run meets none, and it re-validates by claiming again (an object that moved to
//!   another port while the call waited claims that port's PHYs: at worst one more deferral per
//!   PHY, each bounded).
//!
//! The order is therefore: **claim (outside the SAI lock) -> SAI lock -> PHY slot -> the MDIO
//! master's lock**; a claim is never waited for under the SAI lock and slots are never held while
//! anything waits, so no cycle can form.
use crate::{ExtPhy, Shared};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::sync::Arc;

thread_local! {
    /// How deep this thread is under the SAI lock (`under_sai_lock`).
    static SAI_LOCKED: Cell<u32> = const { Cell::new(0) };
    /// The SAI call this thread is in (`call`).
    static CALL: RefCell<Option<CallState>> = const { RefCell::new(None) };
}

/// The claims of one SAI call, and the claim it could not take under the SAI lock.
#[derive(Default)]
struct CallState {
    /// Claimed PHYs, per runtime (one per switch: normally one entry).
    held: Vec<(Arc<Shared>, BTreeSet<u16>)>,
    /// The runtime and PHYs to claim outside the SAI lock before the call runs again.
    deferred: Option<(ExtPhy, BTreeSet<u16>)>,
}

impl CallState {
    fn add(&mut self, shared: &Arc<Shared>, phys: &[u16]) {
        if let Some((_, set)) = self.held.iter_mut().find(|(s, _)| Arc::ptr_eq(s, shared)) {
            set.extend(phys.iter().copied());
        } else {
            self.held.push((Arc::clone(shared), phys.iter().copied().collect()));
        }
    }
}

/// Ends this thread's call: gives its claims back.
struct End;
impl Drop for End {
    fn drop(&mut self) {
        let state = CALL.with(|c| c.borrow_mut().take());
        if let Some(state) = state {
            for (shared, set) in state.held {
                let phys: Vec<u16> = set.into_iter().collect();
                shared.arb.unclaim(&phys);
            }
        }
    }
}

/// Runs `f` marked as holding the SAI lock (the rim: everything it runs under that lock).
pub fn under_sai_lock<T>(f: impl FnOnce() -> T) -> T {
    struct Unmark;
    impl Drop for Unmark {
        fn drop(&mut self) {
            SAI_LOCKED.with(|n| n.set(n.get().saturating_sub(1)));
        }
    }
    SAI_LOCKED.with(|n| n.set(n.get() + 1));
    let _unmark = Unmark;
    f()
}

/// Whether this thread holds the SAI lock (as the rim marked it).
pub fn sai_locked() -> bool {
    SAI_LOCKED.with(|n| n.get() != 0)
}

/// Whether this thread is inside [`call`].
pub fn in_call() -> bool {
    CALL.with(|c| c.borrow().is_some())
}

/// One SAI call: runs `locked` (which takes the SAI lock, runs the call under
/// [`under_sai_lock`] and lets go of the lock) until no claim of it was deferred; between two
/// runs, outside the SAI lock, waits for the transaction in the way and claims (module
/// documentation). The result of a deferred run is dropped (it did nothing). The call's claims
/// are given back when it returns. Inside another `call` (a SAI call from a callback of one):
/// runs `locked` once, the outer call owns the claims.
pub fn call<T>(mut locked: impl FnMut() -> T) -> T {
    if in_call() {
        return locked();
    }
    CALL.with(|c| *c.borrow_mut() = Some(CallState::default()));
    let _end = End;
    loop {
        let r = locked();
        let deferred = CALL.with(|c| c.borrow_mut().as_mut().and_then(|s| s.deferred.take()));
        let Some((ext, phys)) = deferred else {
            return r;
        };
        drop(r);
        debug_assert!(
            !sai_locked(),
            "sai-extphy: call(): the closure returned with the SAI lock still marked"
        );
        let phys: Vec<u16> = phys.into_iter().collect();
        let phys = missing(&ext.shared, &phys);
        ext.claim_outside(&phys);
        keep(&ext.shared, &phys);
    }
}

/// The PHYs of `phys` (of the runtime `shared`) this thread's call has not claimed (all of them
/// outside a call).
pub(crate) fn missing(shared: &Arc<Shared>, phys: &[u16]) -> Vec<u16> {
    CALL.with(|c| {
        let c = c.borrow();
        let held = c
            .as_ref()
            .and_then(|s| s.held.iter().find(|(s, _)| Arc::ptr_eq(s, shared)))
            .map(|(_, set)| set);
        phys.iter()
            .copied()
            .filter(|p| held.is_none_or(|set| !set.contains(p)))
            .collect()
    })
}

/// Keeps `phys` claimed for this thread's call (taken by the arbiter).
pub(crate) fn keep(shared: &Arc<Shared>, phys: &[u16]) {
    CALL.with(|c| {
        if let Some(state) = c.borrow_mut().as_mut() {
            state.add(shared, phys);
        }
    });
}

/// Defers this thread's call: `phys` of `ext` are claimed outside the SAI lock, then it runs
/// again. False outside a call.
pub(crate) fn defer(ext: &ExtPhy, phys: &[u16]) -> bool {
    CALL.with(|c| {
        let mut c = c.borrow_mut();
        let Some(state) = c.as_mut() else {
            return false;
        };
        match &mut state.deferred {
            Some((e, set)) if Arc::ptr_eq(&e.shared, &ext.shared) => set.extend(phys.iter().copied()),
            // two switches in one call do not exist; the first deferral wins, the call runs again
            Some(_) => {}
            None => state.deferred = Some((ext.clone(), phys.iter().copied().collect())),
        }
        true
    })
}

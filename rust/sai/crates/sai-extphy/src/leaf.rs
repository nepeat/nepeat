//! The runtime's own mutexes are **leaves**: while one is held, no other lock of this crate is
//! taken, no logger is called, no bus is accessed and nothing waits (but the arbiter's condition
//! wait, which gives its mutex up). [`Leaf`] wraps their guards and counts them per thread, so a
//! debug build (every test) panics at the first place that breaks the rule; a release build only
//! counts. This, and the arbiter's slots never being held while anything waits, is what makes the
//! lock order (SAI lock, then a lease, then a PHY's slot, then the bus master's own lock) hold by
//! construction.
use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

thread_local! {
    static HELD: Cell<u32> = const { Cell::new(0) };
}

/// Whether this thread holds one of the runtime's mutexes.
pub(crate) fn held() -> bool {
    HELD.with(Cell::get) != 0
}

/// Checks (debug builds) that this thread holds none of the runtime's mutexes: before a logger
/// call, a bus access, a wait.
pub(crate) fn assert_free(what: &str) {
    debug_assert!(
        !held(),
        "sai-extphy lock order: {what} while one of the runtime's mutexes is held"
    );
}

/// One held runtime mutex of this thread (counted while it lives).
struct Count;

impl Count {
    fn new() -> Self {
        HELD.with(|h| h.set(h.get() + 1));
        Self
    }
}

impl Drop for Count {
    fn drop(&mut self) {
        HELD.with(|h| h.set(h.get().saturating_sub(1)));
    }
}

/// A guard of one of the runtime's mutexes.
pub(crate) struct Leaf<'a, T> {
    guard: MutexGuard<'a, T>,
    /// Counts the guard while it lives (dropped after it).
    count: Count,
}

/// Takes `m` (a panic under it, none is possible in this crate, leaves whole fields behind).
pub(crate) fn lock<T>(m: &Mutex<T>) -> Leaf<'_, T> {
    assert_free("taking a runtime mutex");
    let guard = m.lock().unwrap_or_else(PoisonError::into_inner);
    Leaf {
        guard,
        count: Count::new(),
    }
}

impl<T> Leaf<'_, T> {
    /// Waits on `cv` at most `dur`, the mutex given up meanwhile.
    pub(crate) fn wait(self, cv: &Condvar, dur: Duration) -> Self {
        let Self { guard, count } = self;
        drop(count);
        let guard = match cv.wait_timeout(guard, dur) {
            Ok((g, _)) => g,
            Err(poisoned) => poisoned.into_inner().0,
        };
        Self {
            guard,
            count: Count::new(),
        }
    }
}

impl<T> Deref for Leaf<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for Leaf<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

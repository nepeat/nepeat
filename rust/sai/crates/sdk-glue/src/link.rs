//! The link callback: the SDK tells the process that a port's link went up or down.
//!
//! # How it is made safe
//!
//! The glue takes a C function pointer and a `void* ctx` it hands back on every call
//! (`ob_sdk_register_callbacks`), and calls it on SDK threads (the link scan thread; also the
//! thread of a port call that changes the link). The usual binding gives the SDK a pointer to
//! a boxed closure as `ctx`, and is then sound only if that box outlives the last call: a
//! property of the SDK's unregister ("no call is in progress when it returns") that cannot
//! be checked from here.
//!
//! This module hands over **no memory at all**:
//!
//! * the closure lives in a process-wide slot behind a mutex (there is one SDK unit per
//!   process, [`crate::Sdk`]);
//! * `ctx` is not a pointer to anything: it carries the registration's generation number as
//!   an address, and is compared, never dereferenced;
//! * the C callback is a safe `extern "C"` function that locks the slot, checks the
//!   generation and calls the closure; a panic of the closure is caught there and does not
//!   cross into C.
//!
//! So a late or stray call from the SDK (after unregistering, from an earlier registration,
//! with any `ctx`) finds an empty slot or another generation and does nothing. No lifetime
//! depends on the SDK's behaviour. The handle still unregisters before it shuts the BCM layer
//! down ([`Switch`]'s `Drop`), so the SDK stops calling, and the closure is dropped there.
//!
//! What the two `unsafe` calls need is stated at each: plain integers, a function pointer
//! with the signature of `ob_link_cb_t` that stays valid for the life of the process (a
//! function of this crate), and a `ctx` the glue only stores and passes back.
//!
//! # For the closure
//!
//! It runs on an SDK thread with the slot locked: it must be quick and must not block (send
//! on a channel, set a flag). It cannot call the switch: it has no [`Switch`]. Calls are
//! serialised. MAC learning events (`ob_l2_cb_t`) are not bound: the glue is given no L2
//! callback.

use std::ffi::{c_int, c_void};
use std::panic::catch_unwind;
use std::sync::{Mutex, MutexGuard, PoisonError};

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::switch::Switch;

type Handler = Box<dyn Fn(i32, bool) + Send + Sync>;

struct Slot {
    /// Counts registrations; the value the current one passed to the glue as `ctx`.
    generation: usize,
    handler: Option<Handler>,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    generation: 0,
    handler: None,
});

/// The slot, also after a closure panicked while it was locked (the slot holds no invariant a
/// panic could break: a number and an optional closure).
fn slot() -> MutexGuard<'static, Slot> {
    SLOT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `ob_link_cb_t`. Safe to call with any arguments from any thread.
extern "C" fn on_link(ctx: *mut c_void, port: c_int, up: c_int) {
    let generation = ctx.addr();
    // A panic must not unwind into the SDK's thread: it ends here, the event is lost.
    let _ = catch_unwind(|| {
        let slot = slot();
        if slot.generation == generation {
            if let Some(handler) = &slot.handler {
                handler(port, up != 0);
            }
        }
    });
}

impl Switch {
    /// Registers `handler` for link changes: called with the SDK's logical port number and
    /// the new state (true = up), on an SDK thread (see the module documentation for what it
    /// may do). Replaces an earlier handler. On an error nothing is registered and `handler`
    /// is dropped.
    pub fn set_link_callback(&mut self, handler: impl Fn(i32, bool) + Send + Sync + 'static) -> Result<(), Error> {
        self.clear_link_callback();
        let generation = {
            let mut slot = slot();
            slot.generation = slot.generation.wrapping_add(1);
            slot.handler = Some(Box::new(handler));
            slot.generation
        };
        let callback: unsafe extern "C" fn(*mut c_void, c_int, c_int) = on_link;
        // Not a pointer to memory: the generation, for on_link to compare.
        let ctx: *mut c_void = std::ptr::without_provenance_mut(generation);
        // SAFETY: self exists, so ob_sdk_init succeeded on this unit (the BCM layer the glue
        // registers with is up), and &mut self keeps other calls of this handle out. callback
        // has the signature of ob_link_cb_t and is a function of this crate: valid for the
        // life of the process, callable from any thread with any arguments (it is a safe
        // function). No L2 callback is given (NULL: the glue checks it before every call).
        // ctx is only stored by the glue and passed back to callback, which never
        // dereferences it.
        let rv = unsafe { sys::ob_sdk_register_callbacks(self.raw_unit(), Some(callback), None, ctx) };
        // The glue stores the pointers before it asks the SDK, and leaves them stored when the
        // SDK refuses: from here on it is "registered" until unregistered, also on failure.
        self.link_registered = true;
        if let Err(error) = check(rv) {
            self.clear_link_callback();
            return Err(error);
        }
        Ok(())
    }

    /// Unregisters the link callback and drops the handler. No effect without one.
    pub fn clear_link_callback(&mut self) {
        if !self.link_registered {
            return;
        }
        // SAFETY: self exists, so the BCM layer of this unit is up; the function takes the
        // unit number only and removes the glue's own handlers from the SDK.
        unsafe { sys::ob_sdk_unregister_callbacks(self.raw_unit()) };
        self.link_registered = false;
        // After the SDK was told: a call that is in progress on another thread holds the slot
        // and finishes first; one that starts later finds it empty.
        let handler = slot().handler.take();
        // Dropped with the slot unlocked: a handler's own Drop may do anything.
        drop(handler);
    }
}

// SPDX-License-Identifier: GPL-2.0

//! Reboot notifier registration (`register_reboot_notifier()`), which the
//! kernel crate has no abstraction for in 7.3. All of the notifier `unsafe` of
//! this module is here, behind a safe interface: implement [`Handler`], keep
//! the pinned [`Registration`] alive.
//!
//! C header: `include/linux/reboot.h`, `include/linux/notifier.h`.
//!
//! `linux/reboot.h` is not in the kernel's bindings (`bindings_helper.h`), so its
//! two functions and `SYS_RESTART` are declared by hand below; abi_check.c makes
//! the C compiler compare them with the header on every build.

use kernel::{
    bindings, container_of,
    error::to_result,
    ffi::{c_int, c_ulong, c_void},
    prelude::*,
    types::Opaque,
};

/// `SYS_RESTART` of `linux/reboot.h` (checked by abi_check.c).
const SYS_RESTART: c_ulong = 0x0001;

extern "C" {
    // include/linux/reboot.h; prototypes checked by abi_check.c.
    fn register_reboot_notifier(nb: *mut bindings::notifier_block) -> c_int;
    fn unregister_reboot_notifier(nb: *mut bindings::notifier_block) -> c_int;
}

/// What the kernel is about to do (`SYS_RESTART`, `SYS_HALT`, `SYS_POWER_OFF`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Action(c_ulong);

impl Action {
    /// The system is restarting.
    pub(crate) const RESTART: Self = Self(SYS_RESTART);
}

/// Code run from the reboot notifier chain: process context, may sleep.
pub(crate) trait Handler: Send + Sync {
    /// Called once per reboot/halt/power-off; the chain always continues.
    fn notify(&self, action: Action);
}

/// A registered reboot notifier; unregistered when dropped.
///
/// # Invariants
///
/// `nb` is initialised and on the reboot notifier chain for as long as the
/// value exists, and its `notifier_call` is [`Self::callback`].
#[pin_data(PinnedDrop)]
pub(crate) struct Registration<T: Handler> {
    handler: T,
    #[pin]
    nb: Opaque<bindings::notifier_block>,
}

// SAFETY: the notifier block is only touched by the notifier chain, under its
// own lock; `handler` is `Send`.
unsafe impl<T: Handler> Send for Registration<T> {}
// SAFETY: as above, and the only shared access is `Handler::notify(&self)` on
// a `Sync` handler.
unsafe impl<T: Handler> Sync for Registration<T> {}

impl<T: Handler> Registration<T> {
    /// Registers `handler`; a lower `priority` runs later in the chain.
    pub(crate) fn new(handler: T, priority: i32) -> impl PinInit<Self, Error> {
        // `handler` is listed first: fields are initialised in the order given, so
        // it is in place before the notifier can be called.
        try_pin_init!(Self {
            handler,
            nb <- Opaque::try_ffi_init(|slot: *mut bindings::notifier_block| {
                // SAFETY: `slot` is valid for writes (uninitialised field of the
                // value being pin-initialised).
                unsafe {
                    slot.write(bindings::notifier_block {
                        notifier_call: Some(Self::callback),
                        next: core::ptr::null_mut(),
                        priority,
                    })
                };
                // SAFETY: `slot` now holds an initialised notifier_block that stays
                // at this address (the value is pinned) until `drop` unregisters it.
                to_result(unsafe { register_reboot_notifier(slot) })
            }),
        })
    }

    /// The `notifier_call` of `nb`.
    ///
    /// # Safety
    ///
    /// Only called by the notifier chain, with `nb` pointing to the `nb` field of
    /// a live `Registration<T>`.
    unsafe extern "C" fn callback(
        nb: *mut bindings::notifier_block,
        action: c_ulong,
        _data: *mut c_void,
    ) -> c_int {
        // SAFETY: by the function's contract `nb` is the `nb` field of a live,
        // pinned `Registration<T>`; the chain's lock is held across this call and
        // `drop` takes it to unregister, so the value outlives the reference.
        let this = unsafe { &*container_of!(Opaque::cast_from(nb), Self, nb) };
        this.handler.notify(Action(action));
        bindings::NOTIFY_DONE as c_int
    }
}

#[pinned_drop]
impl<T: Handler> PinnedDrop for Registration<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: by the type invariant `nb` is registered. After this returns the
        // chain no longer references it and no callback is running.
        unsafe { unregister_reboot_notifier(self.nb.get()) };
    }
}

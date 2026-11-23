// SPDX-License-Identifier: GPL-2.0

//! NAPI contexts (include/linux/netdevice.h), each owning the state its poll
//! function works on, and an IRQ handler that schedules one.

use super::netdev::{NetDev, TxGuard, Unregistered};
use super::skb::SkBuff;
use core::{cell::UnsafeCell, marker::PhantomData, ptr::NonNull};
use kernel::{
    bindings,
    ffi::c_int,
    irq::{self, IrqRequest, IrqReturn},
    prelude::*,
    str::CStr,
    sync::atomic::{ordering, Atomic},
    types::Opaque,
};

/// The poll function of a NAPI context and the state it owns (a ring).
pub(crate) trait Poll: Send {
    /// Private data type of the device the context belongs to.
    type Dev: Send + Sync;
    /// Processes up to budget units of work and returns how many were done
    /// (at most budget). Call [NapiCtx::complete_done] when done before the
    /// budget ran out, then re-enable the interrupt. Softirq context.
    fn poll(&mut self, dev: &NetDev<Self::Dev>, napi: &mut NapiCtx<'_>, budget: u32) -> u32;
}

/// RX contexts take part in busy polling; TX-only ones do not (netif_napi_add_tx).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Rx,
    Tx,
}

const ON: i32 = 0; // enabled: only the poll function touches the state
const PARKED: i32 = 1; // disabled, no guard out
const OFF: i32 = 2; // disabled, a NapiOff guard is out

/// A NAPI context and its poll state P.
///
/// # Invariants
///
/// napi was added with netif_napi_add[_tx] to the device whose private data is
/// P::Dev, with poll_cb::<P> as the poll function, and is deleted in drop.
/// state is accessed mutably only by poll_cb (while the context is enabled, by
/// the NAPI guarantee of a single poller) or through the one NapiOff guard (while
/// disabled); mode tracks which: ON, PARKED, OFF.
#[pin_data(PinnedDrop)]
pub(crate) struct Napi<P: Poll> {
    state: UnsafeCell<P>,
    mode: Atomic<i32>,
    #[pin]
    napi: Opaque<bindings::napi_struct>,
}

// SAFETY: the state is P: Send and is handed between contexts under the
// discipline of the type invariant (one accessor at a time); the napi_struct
// is used by the core from any CPU.
unsafe impl<P: Poll> Send for Napi<P> {}
// SAFETY: shared access only schedules, or takes the state under that discipline.
unsafe impl<P: Poll> Sync for Napi<P> {}

impl<P: Poll> Napi<P> {
    /// A context of dev (being set up), owning state, starting disabled
    /// (take it with [Napi::parked], enable from open).
    ///
    /// # Safety
    ///
    /// The result must be pinned inside the private data of dev's
    /// [Registration](super::netdev::Registration) (so it is dropped after the
    /// device is unregistered and before it is freed) and never forgotten: its
    /// drop deletes the context from the device's list.
    pub(crate) unsafe fn new(dev: &Unregistered<P::Dev>, kind: Kind, state: P) -> impl PinInit<Self, Error> {
        let dev = dev.as_raw();
        try_pin_init!(Self {
            state: UnsafeCell::new(state),
            mode: Atomic::new(PARKED),
            napi <- Opaque::try_ffi_init(move |slot: *mut bindings::napi_struct| {
                // SAFETY: slot is valid for writes (the field being initialised);
                // the core expects a zeroed napi_struct. dev is the allocated, not yet
                // freed device (our caller's contract: this runs inside the device's
                // Registration::new); poll_cb::<P> matches its private data (P::Dev).
                unsafe {
                    slot.write_bytes(0, 1);
                    match kind {
                        Kind::Rx => bindings::netif_napi_add(dev, slot, Some(Self::poll_cb)),
                        Kind::Tx => bindings::netif_napi_add_tx(dev, slot, Some(Self::poll_cb)),
                    }
                }
                Ok::<(), Error>(())
            }),
        })
    }

    fn raw(&self) -> *mut bindings::napi_struct {
        self.napi.get()
    }

    /// napi_schedule(): run the poll function soon (any context, hard IRQ included).
    pub(crate) fn schedule(&self) {
        // SAFETY: the context is added (invariant); a disabled one ignores this.
        unsafe { bindings::napi_schedule(self.raw()) };
    }

    /// The state while the context is disabled and no guard is out (after
    /// creation, or after a guard was dropped without enabling).
    pub(crate) fn parked(&self) -> Option<NapiOff<'_, P>> {
        self.mode.cmpxchg(PARKED, OFF, ordering::Full).ok()?;
        Some(NapiOff { napi: self })
    }

    /// napi_disable() (sleeps until a running poll returns), then the state.
    /// None if not enabled.
    pub(crate) fn disable(&self) -> Option<NapiOff<'_, P>> {
        self.mode.cmpxchg(ON, OFF, ordering::Full).ok()?;
        // SAFETY: the context is added and was enabled (we won the ON -> OFF
        // transition, so nobody else disables it concurrently). Process context.
        unsafe { bindings::napi_disable(self.raw()) };
        // INVARIANT: napi_disable has waited for the poller; until enable, the
        // core will not call poll_cb, so the guard is the state's only accessor.
        Some(NapiOff { napi: self })
    }

    /// The poll function the core calls.
    ///
    /// # Safety
    ///
    /// Only called by the NAPI core for a context added by Napi::<P>::new.
    unsafe extern "C" fn poll_cb(napi: *mut bindings::napi_struct, budget: c_int) -> c_int {
        // SAFETY: napi is the napi field of a live Napi<P> (invariant: deleted in
        // drop before the memory goes away).
        let this = unsafe { &*kernel::container_of!(Opaque::cast_from(napi), Self, napi) };
        // SAFETY: the context was added to a device registered by
        // Registration<P::Dev> (or being torn down by it after its last poll).
        let dev = unsafe { NetDev::<P::Dev>::from_raw((*napi).dev) };
        // SAFETY: the core runs one poller per context at a time, only while the
        // context is enabled, i.e. while no NapiOff exists (type invariant).
        let state = unsafe { &mut *this.state.get() };
        let budget = u32::try_from(budget).unwrap_or(0);
        let mut ctx = NapiCtx { napi, budget, completed: false, _p: PhantomData };
        let mut done = state.poll(dev, &mut ctx, budget).min(budget);
        if ctx.completed && budget > 0 && done == budget {
            // Returning the full budget after napi_complete_done would keep a
            // completed context on the poll list: report one less.
            done = budget - 1;
        }
        done as c_int
    }
}

#[pinned_drop]
impl<P: Poll> PinnedDrop for Napi<P> {
    fn drop(self: Pin<&mut Self>) {
        // No guard can be out (it borrows self): mode is ON or PARKED.
        if self.mode.load(ordering::Acquire) == ON {
            // SAFETY: added and enabled; process context (device teardown).
            unsafe { bindings::napi_disable(self.raw()) };
        }
        // SAFETY: added (invariant); netif_napi_del waits an RCU grace period
        // (synchronize_net), after which the core holds no reference to it.
        unsafe { bindings::netif_napi_del(self.raw()) };
    }
}

/// The state of a disabled NAPI context; [NapiOff::enable] turns it back on.
pub(crate) struct NapiOff<'a, P: Poll> {
    napi: &'a Napi<P>,
}

impl<P: Poll> NapiOff<'_, P> {
    /// napi_enable(): the poll function owns the state again.
    pub(crate) fn enable(self) {
        let napi = self.napi;
        core::mem::forget(self);
        // SAFETY: the context is added and disabled (we hold the only guard).
        unsafe { bindings::napi_enable(napi.raw()) };
        napi.mode.store(ON, ordering::Release);
    }
}

impl<P: Poll> core::ops::Deref for NapiOff<'_, P> {
    type Target = P;
    fn deref(&self) -> &P {
        // SAFETY: disabled with this guard out: no other accessor (invariant).
        unsafe { &*self.napi.state.get() }
    }
}

impl<P: Poll> core::ops::DerefMut for NapiOff<'_, P> {
    fn deref_mut(&mut self) -> &mut P {
        // SAFETY: as deref; &mut self makes this the only reference.
        unsafe { &mut *self.napi.state.get() }
    }
}

impl<P: Poll> Drop for NapiOff<'_, P> {
    fn drop(&mut self) {
        // Left disabled: park it, so open can take it again.
        self.napi.mode.store(PARKED, ordering::Release);
    }
}

/// What a poll function may do with its context (softirq, BH disabled).
pub(crate) struct NapiCtx<'a> {
    napi: *mut bindings::napi_struct,
    budget: u32,
    completed: bool,
    _p: PhantomData<&'a ()>,
}

impl<'a> NapiCtx<'a> {
    /// napi_gro_receive().
    pub(crate) fn gro_receive(&mut self, skb: SkBuff) {
        // SAFETY: inside the context's poll; the stack takes the skb reference.
        unsafe { bindings::napi_gro_receive(self.napi, skb.into_raw()) };
    }

    /// Proof that BH is disabled outside hard IRQ (a NAPI poll with a budget;
    /// None for netpoll's budget-0 call, which may run with IRQs off).
    pub(crate) fn bh(&self) -> Option<Bh<'a>> {
        (self.budget > 0).then_some(Bh(PhantomData))
    }

    /// napi_alloc_skb(): an skb with len bytes of room, for copying small packets.
    pub(crate) fn alloc_skb(&mut self, len: u32) -> Option<SkBuff> {
        // SAFETY: inside the context's poll (softirq), as napi_alloc_skb requires.
        let p = unsafe { bindings::napi_alloc_skb(self.napi, len) };
        // SAFETY: a new skb we own, if non-null.
        (!p.is_null()).then(|| unsafe { SkBuff::from_raw(p) })
    }

    /// napi_consume_skb(): free a transmitted packet (bulk-free in NAPI context).
    pub(crate) fn consume_skb(&mut self, skb: SkBuff) {
        // SAFETY: inside poll; budget as passed by the core (0 = netpoll, handled).
        unsafe { bindings::napi_consume_skb(skb.into_raw(), self.budget as c_int) };
    }

    /// napi_complete_done(): all work done; false if the core wants another poll
    /// (busy polling), in which case the interrupt must stay masked.
    pub(crate) fn complete_done(&mut self, work_done: u32) -> bool {
        self.completed = true;
        // SAFETY: inside the context's poll, which owns the SCHED bit.
        unsafe { bindings::napi_complete_done(self.napi, work_done.min(self.budget) as c_int) }
    }

    /// The budget of this poll (0 for netpoll: TX completion only).
    pub(crate) fn budget(&self) -> u32 {
        self.budget
    }

    /// napi_get_frags(): the context's GRO skb, to build a received packet from
    /// page fragments only and hand it to GRO with [GroFrags::gro] (no skb is
    /// allocated per packet: GRO reuses it when it merges the packet). None if
    /// the context could not allocate one.
    pub(crate) fn frags(&mut self) -> Option<GroFrags<'_>> {
        // SAFETY: inside the context's poll (softirq), as napi_get_frags requires.
        let skb = NonNull::new(unsafe { bindings::napi_get_frags(self.napi) })?;
        Some(GroFrags { napi: self.napi, skb, _ctx: PhantomData })
    }

    /// Takes TX queue index's lock of dev (BH is disabled here).
    pub(crate) fn tx_lock<'d, T>(&mut self, dev: &'d NetDev<T>, index: u32) -> Option<TxGuard<'d>> {
        // SAFETY: reading this CPU's id; poll runs with BH (thus migration) disabled.
        let cpu = unsafe { bindings::raw_smp_processor_id() } as i32;
        dev.tx_lock_in_bh(index, cpu)
    }
}

/// A token: this code runs inside a NAPI poll (softirq or a BH-disabled
/// thread/busy-poll loop, never hard IRQ, budget > 0), for allocators that
/// need that (napi_alloc_frag). Made only by [NapiCtx::bh]; the lifetime is
/// the poll's, so it cannot be stored past the poll (a poll function is
/// generic over it), and it is neither Send nor Sync.
#[derive(Clone, Copy)]
pub(crate) struct Bh<'a>(PhantomData<*const &'a ()>);

/// The context's GRO frags skb, being filled with one received packet.
///
/// # Invariants
///
/// skb is napi->skb, which napi_get_frags returned in this poll of napi; the
/// guard borrows the [NapiCtx] mutably, so nothing else in this poll can use
/// the context (or replace napi->skb) until [GroFrags::gro] hands the skb to
/// GRO (napi_gro_frags, which takes it) or the guard is dropped (the skb is freed
/// and napi->skb cleared, as napi_free_frags does). The skb is never exposed as
/// an owned [SkBuff].
pub(crate) struct GroFrags<'a> {
    napi: *mut bindings::napi_struct,
    skb: NonNull<bindings::sk_buff>,
    _ctx: PhantomData<&'a mut NapiCtx<'a>>,
}

impl GroFrags<'_> {
    /// A non-owning view of the skb for one call of an [SkBuff] method.
    fn view(&self) -> core::mem::ManuallyDrop<SkBuff> {
        // SAFETY: the skb is valid and exclusively ours until gro/drop (type
        // invariant); ManuallyDrop keeps the view from freeing it, and the view
        // stays private to these methods (never moved out or kept).
        core::mem::ManuallyDrop::new(unsafe { SkBuff::from_raw(self.skb.as_ptr()) })
    }

    /// Appends a received buffer (len bytes at its start) as the next fragment;
    /// gives it back if the skb has no fragment slot left.
    pub(crate) fn add_rx_frag(&mut self, frag: super::dma::RxFrag, len: u32) -> core::result::Result<(), super::dma::RxFrag> {
        self.view().add_rx_frag(frag, 0, len)
    }
    /// See [SkBuff::set_csum_unnecessary].
    pub(crate) fn set_csum_unnecessary(&mut self) {
        self.view().set_csum_unnecessary()
    }
    /// See [SkBuff::set_hash].
    pub(crate) fn set_hash(&mut self, hash: u32, l4: bool) {
        self.view().set_hash(hash, l4)
    }
    /// See [SkBuff::record_rx_queue].
    pub(crate) fn record_rx_queue(&mut self, q: u16) {
        self.view().record_rx_queue(q)
    }
    /// Bytes in the packet so far.
    pub(crate) fn len(&self) -> u32 {
        self.view().len()
    }
    /// napi_gro_frags(): GRO takes the packet (it pulls the Ethernet header
    /// and sets the protocol itself). A packet shorter than an Ethernet header
    /// is freed instead (false): napi_gro_frags would hand GRO a NULL skb for
    /// it (net/core/gro.c napi_frags_skb, 7.3).
    pub(crate) fn gro(self) -> bool {
        if self.len() < bindings::ETH_HLEN {
            return false; // dropped here: Drop frees it and clears napi->skb
        }
        let napi = self.napi;
        core::mem::forget(self);
        // SAFETY: napi->skb is the filled frags skb of this poll (invariant).
        unsafe { bindings::napi_gro_frags(napi) };
        true
    }
}

impl Drop for GroFrags<'_> {
    fn drop(&mut self) {
        // napi_free_frags(): not handed to GRO, so free it and let the next
        // napi_get_frags allocate a fresh one.
        // SAFETY: napi is the live context of this poll; its skb field is ours
        // (invariant) and cleared before the skb is freed.
        unsafe { (*self.napi).skb = core::ptr::null_mut() };
        // SAFETY: the skb was owned by the context, whose pointer is cleared:
        // this is now its only reference.
        drop(unsafe { SkBuff::from_raw(self.skb.as_ptr()) });
    }
}

/// The interrupt handler of a [NapiIrq]: schedules its NAPI context.
///
/// # Invariants
///
/// napi is the napi_struct of the Napi in the same NapiIrq, which outlives the
/// IRQ registration (dropped after it).
struct Kick(NonNull<bindings::napi_struct>);

// SAFETY: napi_schedule may be called from any CPU and context.
unsafe impl Send for Kick {}
// SAFETY: as above; the handler only schedules.
unsafe impl Sync for Kick {}

impl irq::Handler for Kick {
    fn handle(&self) -> IrqReturn {
        // SAFETY: the context is alive and added (invariant).
        unsafe { bindings::napi_schedule(self.0.as_ptr()) };
        IrqReturn::Handled
    }
}

/// One interrupt vector paired with the NAPI context it schedules (a queue's
/// completion interrupt). The IRQ is freed before the context is deleted.
#[pin_data]
pub(crate) struct NapiIrq<'b, P: Poll> {
    // Declared first: dropped first (free_irq waits for a running handler).
    #[pin]
    irq: irq::Registration<'b, Kick>,
    #[pin]
    napi: Napi<P>,
}

impl<'b, P: Poll + 'b> NapiIrq<'b, P> {
    /// Adds a NAPI context to dev owning state, then requests irq for it.
    ///
    /// # Safety
    ///
    /// As [Napi::new]: pinned inside dev's private data, never forgotten (the
    /// IRQ handler points at the context until the drop frees the IRQ).
    pub(crate) unsafe fn new(
        dev: &Unregistered<P::Dev>,
        kind: Kind,
        state: P,
        irq: IrqRequest<'b>,
        name: &'static CStr,
    ) -> impl PinInit<Self, Error> + 'b {
        // SAFETY: our caller places us as Napi::new requires.
        let napi = unsafe { Napi::new(dev, kind, state) };
        try_pin_init!(&this in Self {
            // Initialised in this order: the context exists before the IRQ can fire.
            napi <- napi,
            irq <- {
                // SAFETY: only the address of the (just initialised, pinned) napi
                // field is computed; nothing is read.
                let p = unsafe { Opaque::cast_into(&raw const (*this.as_ptr()).napi.napi) };
                // SAFETY: field addresses are never null.
                let kick = Kick(unsafe { NonNull::new_unchecked(p) });
                // SAFETY: the registration is a field of a pinned NapiIrq that our
                // caller promises is dropped, never forgotten.
                unsafe { irq::Registration::new(irq, irq::Flags::TRIGGER_NONE, name, Ok::<Kick, Error>(kick)) }
            },
        })
    }

    /// The NAPI context.
    pub(crate) fn napi(&self) -> &Napi<P> {
        &self.napi
    }
}

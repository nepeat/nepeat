// SPDX-License-Identifier: GPL-2.0

//! `struct net_device`: allocation, private data, registration, the
//! `net_device_ops` a NIC driver implements, carrier and TX queue control, and
//! data guarded by a TX queue's `_xmit_lock`.
//!
//! C headers: `include/linux/netdevice.h`, `include/linux/etherdevice.h`.

use super::{ether::MacAddr, ethtool, skb::SkBuff};
use core::{cell::UnsafeCell, marker::PhantomData, ptr::NonNull};
use kernel::{
    bindings, device,
    error::{from_result, to_result, VTABLE_DEFAULT_ERROR},
    ffi::{c_int, c_uint, c_void},
    prelude::*,
    types::Opaque,
};
use pin_init::PinInit;

/// What `start_xmit` did with the packet.
pub(crate) enum Xmit {
    /// Taken (queued to hardware, or dropped and counted): the driver owns it now.
    Ok,
    /// No room: the stack keeps the packet and retries later. The queue must be
    /// stopped (the core warns otherwise).
    Busy(SkBuff),
}

/// The net_device_ops (and ethtool_ops) of a driver whose private data is `Self`.
///
/// Context of each call (from `Documentation/networking/netdevices.rst`): `open`,
/// `stop`, `change_mtu`, `set_mac_address`, `set_features` and the ethtool calls run
/// in process context under RTNL and may sleep; `start_xmit` runs with BH disabled
/// and the queue's `_xmit_lock` held (handed in as the guard); `set_rx_mode` runs
/// with the address-list spinlock held (atomic: must not sleep); `get_stats64`
/// may run concurrently with anything (atomic); `tx_timeout` runs from the watchdog
/// timer (atomic, the TX lock of every queue held).
#[vtable]
pub(crate) trait Ops: Sized + Send + Sync {
    /// `ndo_open`: bring the interface up.
    fn open(dev: &NetDev<Self>) -> Result;
    /// `ndo_stop`: bring it down. Errors are logged only (the core ignores them).
    fn stop(dev: &NetDev<Self>) -> Result;
    /// `ndo_start_xmit`: `skb` is ours unless returned in [`Xmit::Busy`].
    fn start_xmit(dev: &NetDev<Self>, skb: SkBuff, txq: &mut TxGuard<'_>) -> Xmit;
    /// `ndo_get_stats64`.
    fn get_stats64(_dev: &NetDev<Self>, _stats: &mut Stats64) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ndo_set_rx_mode`: program unicast/multicast filters and promiscuity.
    fn set_rx_mode(_dev: &NetDev<Self>, _addrs: &AddrLists<'_>) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ndo_change_mtu`: `dev.mtu()` already reads `new_mtu` (restored to
    /// `old_mtu` if this fails), so buffer sizing in a restart sees the new value.
    fn change_mtu(_dev: &NetDev<Self>, _new_mtu: u32, _old_mtu: u32) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ndo_set_mac_address`: program `addr` (already checked valid); on `Ok`
    /// the device's address becomes `addr`.
    fn set_mac_address(_dev: &NetDev<Self>, _addr: MacAddr) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ndo_tx_timeout`.
    fn tx_timeout(_dev: &NetDev<Self>, _queue: u32) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ndo_set_features`: `features` is the wanted set; on `Ok` it is applied.
    fn set_features(_dev: &NetDev<Self>, _features: Features) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `get_drvinfo`.
    fn get_drvinfo(_dev: &NetDev<Self>, _info: &mut ethtool::DrvInfo<'_>) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `get_link` (default when absent: `ethtool_op_get_link`, the carrier).
    fn get_link(_dev: &NetDev<Self>) -> bool {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `get_link_ksettings`.
    fn get_link_ksettings(_dev: &NetDev<Self>, _ks: &mut ethtool::LinkKsettings<'_>) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `get_coalesce` (`ethtool -c`).
    fn get_coalesce(_dev: &NetDev<Self>, _c: &mut ethtool::Coalesce) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `set_coalesce` (`ethtool -C`): `c` is the whole wanted setting.
    fn set_coalesce(_dev: &NetDev<Self>, _c: &ethtool::Coalesce) -> Result {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// ethtool `get_ringparam` (`ethtool -g`).
    fn get_ringparam(_dev: &NetDev<Self>, _r: &mut ethtool::Rings) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ethtool -S`: the names (at most 31 bytes each, the order of [Ops::get_stats]).
    fn stats_names(_dev: &NetDev<Self>) -> &'static [&'static str] {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
    /// `ethtool -S`: one value per name. Process context (RTNL not held).
    fn get_stats(_dev: &NetDev<Self>, _out: &mut [u64]) {
        build_error!(VTABLE_DEFAULT_ERROR)
    }
}

/// `netdev_features_t` (a `u64` bit set, `NETIF_F_*_BIT`).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Features(pub(crate) u64);

impl Features {
    /// The set with the single feature bit `bit` (a `bindings::NETIF_F_*_BIT`).
    pub(crate) const fn bit(bit: u32) -> Self {
        Self(1u64 << bit)
    }
    /// Union.
    pub(crate) const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    /// Whether every feature of `other` is in `self`.
    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// `struct rtnl_link_stats64`, filled by [`Ops::get_stats64`].
pub(crate) type Stats64 = bindings::rtnl_link_stats64;

/// A registered network device whose private area holds a `T`.
///
/// Only ever seen by reference, in callbacks and through [`Registration::dev`].
///
/// # Invariants
///
/// `self.0` is a `struct net_device` allocated and registered by a
/// [`Registration<T>`] that is alive for the lifetime of the reference, and its
/// private area holds an initialised, pinned `T`.
#[repr(transparent)]
pub(crate) struct NetDev<T>(Opaque<bindings::net_device>, PhantomData<T>);

// SAFETY: `net_device` is designed to be used from any thread; all mutating
// operations below go through the kernel's own locking or atomics, and `T` is
// only reachable as `&T` (`T: Sync` by `Ops`).
unsafe impl<T: Send + Sync> Sync for NetDev<T> {}
// SAFETY: as above.
unsafe impl<T: Send + Sync> Send for NetDev<T> {}

impl<T> NetDev<T> {
    /// # Safety
    ///
    /// `ptr` must point to a `net_device` for which the type invariant holds for
    /// the returned lifetime.
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::net_device) -> &'a Self {
        // SAFETY: `NetDev<T>` is a transparent wrapper of `Opaque<net_device>`;
        // validity and lifetime are the caller's promise.
        unsafe { &*ptr.cast::<Self>() }
    }

    pub(crate) fn as_raw(&self) -> *mut bindings::net_device {
        self.0.get()
    }

    /// The driver's private data.
    pub(crate) fn data(&self) -> &T {
        // SAFETY: by the type invariant the private area holds an initialised `T`,
        // pinned there until the registration drops (after the last callback).
        unsafe { &*bindings::netdev_priv(self.as_raw()).cast::<T>() }
    }

    /// `netif_carrier_on()`.
    pub(crate) fn carrier_on(&self) {
        // SAFETY: `self` is a valid net_device (type invariant); any context.
        unsafe { bindings::netif_carrier_on(self.as_raw()) }
    }

    /// `netif_carrier_off()`.
    pub(crate) fn carrier_off(&self) {
        // SAFETY: as `carrier_on`.
        unsafe { bindings::netif_carrier_off(self.as_raw()) }
    }

    /// `netif_carrier_ok()`.
    pub(crate) fn carrier_ok(&self) -> bool {
        // SAFETY: as `carrier_on`.
        unsafe { bindings::netif_carrier_ok(self.as_raw()) }
    }

    /// `netif_running()`: the interface is administratively up.
    pub(crate) fn is_running(&self) -> bool {
        // SAFETY: as `carrier_on`.
        unsafe { bindings::netif_running(self.as_raw()) }
    }

    /// The current MTU (`READ_ONCE(dev->mtu)`).
    pub(crate) fn mtu(&self) -> u32 {
        // SAFETY: valid device (type invariant); the core writes `mtu` with
        // WRITE_ONCE, so a volatile read is the C `READ_ONCE`.
        unsafe { core::ptr::read_volatile(&raw const (*self.as_raw()).mtu) }
    }

    /// `dev->flags` (`IFF_*`, e.g. `bindings::net_device_flags_IFF_PROMISC`).
    pub(crate) fn flags(&self) -> u32 {
        // SAFETY: as `mtu`.
        unsafe { core::ptr::read_volatile(&raw const (*self.as_raw()).flags) }
    }

    /// The device's current MAC address.
    pub(crate) fn mac_addr(&self) -> MacAddr {
        let mut a = [0u8; 6];
        // SAFETY: `dev_addr` points to `addr_len` (6 for Ethernet, set by
        // `alloc_etherdev`) readable bytes for the device's lifetime; it changes
        // only under RTNL, and a torn read of an address being replaced is what C
        // readers see too (no UB: plain bytes).
        unsafe {
            core::ptr::copy_nonoverlapping((*self.as_raw()).dev_addr, a.as_mut_ptr(), a.len())
        };
        MacAddr(a)
    }

    /// The device's features (`dev->features`).
    pub(crate) fn features(&self) -> Features {
        // SAFETY: as `mtu`.
        Features(unsafe { core::ptr::read_volatile(&raw const (*self.as_raw()).features) })
    }

    /// TX queue `index`, if the device has it.
    pub(crate) fn tx_queue(&self, index: u32) -> Option<TxQueue<'_>> {
        // SAFETY: valid device; `num_tx_queues` is fixed at allocation.
        let n = unsafe { (*self.as_raw()).num_tx_queues };
        if index >= n {
            return None;
        }
        // SAFETY: `index < num_tx_queues`: `_tx[index]` exists for the device's lifetime.
        let q = unsafe { bindings::netdev_get_tx_queue(self.as_raw(), index) };
        Some(TxQueue { q, index, _dev: PhantomData })
    }

    /// `netif_tx_start_all_queues()`.
    pub(crate) fn tx_start_all(&self) {
        // SAFETY: valid device.
        unsafe { bindings::netif_tx_start_all_queues(self.as_raw()) }
    }

    /// `netif_tx_disable()`: stop every queue, waiting out a running `start_xmit`
    /// (takes each queue's lock with BH disabled; process or softirq context).
    pub(crate) fn tx_disable(&self) {
        // SAFETY: valid device.
        unsafe { bindings::netif_tx_disable(self.as_raw()) }
    }

    /// Takes queue `index`'s `_xmit_lock` with BH disabled (process context).
    pub(crate) fn tx_lock_bh(&self, index: u32) -> Option<TxGuard<'_>> {
        let q = self.tx_queue(index)?;
        // SAFETY: `q.q` is a valid queue of this device; unlocked in `TxGuard::drop`.
        unsafe { bindings::__netif_tx_lock_bh(q.q) };
        Some(TxGuard { q, dev: self.as_raw(), unlock: Unlock::Bh })
    }

    /// Takes queue `index`'s `_xmit_lock`; only where BH is already disabled
    /// (NAPI poll: see [`super::napi::NapiCtx::tx_lock`]).
    pub(super) fn tx_lock_in_bh(&self, index: u32, cpu: i32) -> Option<TxGuard<'_>> {
        let q = self.tx_queue(index)?;
        // SAFETY: valid queue; the caller (NapiCtx) runs with BH disabled, so the
        // xmit path cannot interrupt the holder on this CPU; unlocked in drop.
        unsafe { bindings::__netif_tx_lock(q.q, cpu) };
        Some(TxGuard { q, dev: self.as_raw(), unlock: Unlock::Plain })
    }
}

/// One TX queue (`struct netdev_queue`) of a device: its stopped bit, which is
/// atomic and may be flipped from any context.
pub(crate) struct TxQueue<'a> {
    q: *mut bindings::netdev_queue,
    index: u32,
    _dev: PhantomData<&'a ()>,
}

impl TxQueue<'_> {
    /// The queue's index.
    pub(crate) fn index(&self) -> u32 {
        self.index
    }
    /// `netif_tx_stop_queue()`.
    pub(crate) fn stop(&self) {
        // SAFETY: `q` is a valid queue for `'a` (constructed by `NetDev::tx_queue`).
        unsafe { bindings::netif_tx_stop_queue(self.q) }
    }
    /// `netif_tx_wake_queue()`.
    pub(crate) fn wake(&self) {
        // SAFETY: as `stop`.
        unsafe { bindings::netif_tx_wake_queue(self.q) }
    }
    /// `netif_tx_queue_stopped()`.
    pub(crate) fn stopped(&self) -> bool {
        // SAFETY: as `stop`.
        unsafe { bindings::netif_tx_queue_stopped(self.q) }
    }
}

enum Unlock {
    /// Held by the core around `start_xmit`: nothing to release.
    Core,
    Bh,
    Plain,
}

/// Proof that a TX queue's `_xmit_lock` is held: access to [`TxLocked`] data of
/// that queue, and the queue operations that need the lock (BQL accounting).
///
/// # Invariants
///
/// The `_xmit_lock` of `q` (a queue of `dev`) is held by this context for `'a`,
/// and released by `drop` unless the core holds it (`Unlock::Core`).
pub(crate) struct TxGuard<'a> {
    q: TxQueue<'a>,
    dev: *mut bindings::net_device,
    unlock: Unlock,
}

impl<'a> TxGuard<'a> {
    /// The queue (stop/wake/stopped).
    pub(crate) fn queue(&self) -> &TxQueue<'a> {
        &self.q
    }
    /// `netdev_tx_sent_queue()` (BQL): `bytes` were handed to hardware.
    pub(crate) fn sent(&mut self, bytes: u32) {
        // SAFETY: valid queue; BQL's `dql_queued` is serialised by the held lock.
        unsafe { bindings::netdev_tx_sent_queue(self.q.q, bytes) }
    }
    /// `netdev_tx_completed_queue()` (BQL): `pkts`/`bytes` came back. May wake the queue.
    pub(crate) fn completed(&mut self, pkts: u32, bytes: u32) {
        // SAFETY: valid queue; completions are serialised by the held lock.
        unsafe { bindings::netdev_tx_completed_queue(self.q.q, pkts, bytes) }
    }
    /// `netdev_tx_reset_queue()` (BQL): forget in-flight bytes (ring drained).
    pub(crate) fn reset(&mut self) {
        // SAFETY: valid queue, lock held.
        unsafe { bindings::netdev_tx_reset_queue(self.q.q) }
    }
}

impl Drop for TxGuard<'_> {
    fn drop(&mut self) {
        match self.unlock {
            Unlock::Core => {}
            // SAFETY: taken with `__netif_tx_lock_bh` in `NetDev::tx_lock_bh` (invariant).
            Unlock::Bh => unsafe { bindings::__netif_tx_unlock_bh(self.q.q) },
            // SAFETY: taken with `__netif_tx_lock` in `NetDev::tx_lock_in_bh` (invariant).
            Unlock::Plain => unsafe { bindings::__netif_tx_unlock(self.q.q) },
        }
    }
}

/// Data of one TX queue (its ring), guarded by that queue's `_xmit_lock`: the
/// lock the core already holds around `start_xmit`, so the hot path takes no
/// second lock, and completion takes the same one.
///
/// # Invariants
///
/// `data` is accessed only through `get`, with a [`TxGuard`] of queue `index`
/// of device `dev`, i.e. with that queue's lock held.
pub(crate) struct TxLocked<S> {
    dev: *const bindings::net_device,
    index: u32,
    data: UnsafeCell<S>,
}

// SAFETY: `S` is only reached under the queue lock, from one context at a time
// (a mutex-like discipline), so `S: Send` suffices for both.
unsafe impl<S: Send> Send for TxLocked<S> {}
// SAFETY: as above.
unsafe impl<S: Send> Sync for TxLocked<S> {}

impl<S> TxLocked<S> {
    /// Data for queue `index` of the device being set up.
    pub(crate) fn new<T>(dev: &Unregistered<T>, index: u32, data: S) -> Result<Self> {
        // SAFETY: the device is valid (`Unregistered` invariant); field fixed at alloc.
        if index >= unsafe { (*dev.as_raw()).num_tx_queues } {
            return Err(EINVAL);
        }
        Ok(Self { dev: dev.as_raw(), index, data: UnsafeCell::new(data) })
    }

    /// The data, with the queue's lock held. `None` for a guard of another queue
    /// or device (a driver bug, refused rather than aliased).
    pub(crate) fn get<'g>(&'g self, guard: &'g mut TxGuard<'_>) -> Option<&'g mut S> {
        if guard.q.index != self.index || !core::ptr::eq(guard.dev.cast_const(), self.dev) {
            return None;
        }
        // SAFETY: the guard proves this queue's lock is held (its invariant) and it
        // is borrowed mutably for `'g`, so no other `&mut S` exists (type invariant).
        Some(unsafe { &mut *self.data.get() })
    }

    /// The data, without the lock: only with exclusive access to the whole value.
    pub(crate) fn get_mut(&mut self) -> &mut S {
        self.data.get_mut()
    }
}

/// The unicast and multicast address lists, handed to [`Ops::set_rx_mode`] (the
/// address-list lock is held for its lifetime).
pub(crate) struct AddrLists<'a> {
    dev: *mut bindings::net_device,
    _p: PhantomData<&'a ()>,
}

impl AddrLists<'_> {
    fn walk(list: *const bindings::netdev_hw_addr_list, f: &mut dyn FnMut(MacAddr)) {
        // SAFETY: `list` is a list of the device, stable while the address lock is
        // held (`AddrLists` exists only inside `ndo_set_rx_mode`, called with it).
        let head = unsafe { &raw const (*list).list };
        // SAFETY: as above: `head` is an initialised list head.
        let mut pos = unsafe { (*head).next };
        while !core::ptr::eq(pos, head) {
            // SAFETY: every entry on the list is the `list` member of a
            // `netdev_hw_addr` (`net/core/dev_addr_lists.c`), alive under the lock.
            let ha = unsafe { &*kernel::container_of!(pos, bindings::netdev_hw_addr, list) };
            let mut a = [0u8; 6];
            a.copy_from_slice(&ha.addr[..6]);
            f(MacAddr(a));
            pos = ha.list.next;
        }
    }
    /// Each secondary unicast address (`netdev_for_each_uc_addr`).
    pub(crate) fn for_each_uc(&self, mut f: impl FnMut(MacAddr)) {
        // SAFETY: valid device (callback argument).
        Self::walk(unsafe { &raw const (*self.dev).uc }, &mut f)
    }
    /// Each multicast address (`netdev_for_each_mc_addr`).
    pub(crate) fn for_each_mc(&self, mut f: impl FnMut(MacAddr)) {
        // SAFETY: valid device (callback argument).
        Self::walk(unsafe { &raw const (*self.dev).mc }, &mut f)
    }
}

/// A device being set up: allocated, private data not yet (fully) initialised,
/// not registered. Setters that must happen before `register_netdev` live here.
///
/// # Invariants
///
/// `self.0` is a `net_device` allocated by `Registration::<T>::new`, not
/// registered, owned by the `Registration::new` call in progress.
#[repr(transparent)]
pub(crate) struct Unregistered<T>(Opaque<bindings::net_device>, PhantomData<T>);

impl<T> Unregistered<T> {
    pub(crate) fn as_raw(&self) -> *mut bindings::net_device {
        self.0.get()
    }
    /// Sets the MAC address (`eth_hw_addr_set`). `EINVAL` if not a valid unicast address.
    pub(crate) fn set_mac_addr(&self, addr: MacAddr) -> Result {
        if !addr.is_valid() {
            return Err(EINVAL);
        }
        // SAFETY: unregistered device owned by this setup: no concurrent access.
        unsafe { bindings::eth_hw_addr_set(self.as_raw(), addr.0.as_ptr()) };
        Ok(())
    }
    /// A random locally administered address (`eth_hw_addr_random`).
    pub(crate) fn set_random_mac_addr(&self) {
        // SAFETY: as `set_mac_addr`.
        unsafe { bindings::eth_hw_addr_random(self.as_raw()) }
    }
    /// `dev->features`, `hw_features`, `vlan_features`.
    pub(crate) fn set_features(&self, features: Features, hw_features: Features, vlan_features: Features) {
        // SAFETY: as `set_mac_addr`.
        unsafe {
            let d = self.as_raw();
            (*d).features = features.0;
            (*d).hw_features = hw_features.0;
            (*d).vlan_features = vlan_features.0;
        }
    }
    /// MTU range (`min_mtu`, `max_mtu`) and the current MTU.
    pub(crate) fn set_mtu(&self, mtu: u32, min: u32, max: u32) -> Result {
        if !(min <= mtu && mtu <= max) {
            return Err(EINVAL);
        }
        // SAFETY: as `set_mac_addr`.
        unsafe {
            let d = self.as_raw();
            (*d).min_mtu = min;
            (*d).max_mtu = max;
            (*d).mtu = mtu;
        }
        Ok(())
    }
    /// `watchdog_timeo` (jiffies) before `ndo_tx_timeout`.
    pub(crate) fn set_watchdog_timeo(&self, jiffies: i32) {
        // SAFETY: as `set_mac_addr`.
        unsafe { (*self.as_raw()).watchdog_timeo = jiffies }
    }
    /// `netif_set_real_num_{tx,rx}_queues` (at most the allocated counts).
    pub(crate) fn set_real_num_queues(&self, tx: u32, rx: u32) -> Result {
        // SAFETY: as `set_mac_addr`; RTNL is not needed before registration.
        to_result(unsafe { bindings::netif_set_real_num_tx_queues(self.as_raw(), tx) })?;
        // SAFETY: as above.
        to_result(unsafe { bindings::netif_set_real_num_rx_queues(self.as_raw(), rx) })
    }
}

/// Queue counts of a new device.
pub(crate) struct Config {
    pub(crate) tx_queues: u32,
    pub(crate) rx_queues: u32,
}

/// Ownership of a registered `net_device` whose private area holds a `T`.
///
/// # Invariants
///
/// `dev` was allocated by `alloc_etherdev_mqs` with room for a `T` at
/// `netdev_priv(dev)`, which holds an initialised `T` (pinned: never moved), and
/// is registered. `drop` unregisters it, drops the `T` and frees it, in that order.
pub(crate) struct Registration<T: Ops> {
    dev: NonNull<bindings::net_device>,
    _t: PhantomData<T>,
}

// SAFETY: the registration only frees the device (any process context); `T` is
// `Send + Sync` (`Ops`).
unsafe impl<T: Ops> Send for Registration<T> {}
// SAFETY: `&Registration` only gives `&NetDev<T>`, which is `Sync`.
unsafe impl<T: Ops> Sync for Registration<T> {}

impl<T: Ops> Registration<T> {
    /// Allocates an Ethernet device (child of `parent`), lets `init` configure it
    /// and returns the initialiser of its private data, initialises `T` in place,
    /// registers the device. On any error everything done so far is undone.
    pub(crate) fn new<I>(
        parent: &device::Device<device::Bound>,
        cfg: Config,
        init: impl FnOnce(&Unregistered<T>) -> Result<I>,
    ) -> Result<Self>
    where
        I: PinInit<T, Error>,
    {
        const { assert!(core::mem::align_of::<T>() <= bindings::NETDEV_ALIGN as usize) };
        let size = c_int::try_from(core::mem::size_of::<T>()).map_err(|_| EINVAL)?;
        // SAFETY: plain allocation; the result is checked for NULL below.
        let raw = unsafe { bindings::alloc_etherdev_mqs(size, cfg.tx_queues, cfg.rx_queues) };
        let dev = NonNull::new(raw).ok_or(ENOMEM)?;
        // SAFETY: `raw` is a fresh, unregistered device we own. `SET_NETDEV_DEV`:
        // `parent` is a bound device, which outlives the registration (the driver
        // drops it at unbind at the latest); `device::Device` is a transparent
        // wrapper of `struct device`. The ops tables are `'static`.
        unsafe {
            (*raw).dev.parent = core::ptr::from_ref(parent).cast::<bindings::device>().cast_mut();
            (*raw).netdev_ops = OpsTable::<T>::build_ndo();
            (*raw).ethtool_ops = OpsTable::<T>::build_ethtool();
        }
        // SAFETY: `Unregistered<T>` is a transparent wrapper; invariant: we own the
        // unregistered device until this function returns.
        let u = unsafe { &*raw.cast::<Unregistered<T>>() };
        // SAFETY: `netdev_priv` points to `size_of::<T>()` bytes aligned to
        // NETDEV_ALIGN (>= align_of::<T>(), checked above), owned by the device and
        // never moved: a valid pinned slot. On error the initialiser left it
        // uninitialised, so only the device is freed.
        let p = unsafe { bindings::netdev_priv(raw) }.cast::<T>();
        // The initialiser (a driver's whole private-data state) is consumed right
        // where `init` returned it, never copied into a second local: probe runs on
        // a 16 KiB kernel stack and alloc_etherdev/register_netdev below are deep.
        // SAFETY: p as above; on Err from init nothing was written to it.
        let r = init(u).and_then(|pi| unsafe { pi.__init(p) });
        if let Err(e) = r {
            // SAFETY: not registered; `T` not initialised.
            unsafe { bindings::free_netdev(raw) };
            return Err(e);
        }
        // SAFETY: fully set up device. Callbacks may run as soon as this starts.
        if let Err(e) = to_result(unsafe { bindings::register_netdev(raw) }) {
            // SAFETY: registration failed, so no callback can run any more: drop the
            // initialised `T` in place, then free the device.
            unsafe {
                core::ptr::drop_in_place(p);
                bindings::free_netdev(raw);
            }
            return Err(e);
        }
        Ok(Self { dev, _t: PhantomData })
    }

    /// The registered device.
    pub(crate) fn dev(&self) -> &NetDev<T> {
        // SAFETY: type invariant: registered, private data initialised, alive while `self` is.
        unsafe { NetDev::from_raw(self.dev.as_ptr()) }
    }
}

impl<T: Ops> Drop for Registration<T> {
    fn drop(&mut self) {
        let raw = self.dev.as_ptr();
        // SAFETY: registered (invariant). `unregister_netdev` closes the device
        // (`ndo_stop`) and waits for every reference, so no callback runs after it;
        // then `T` is dropped in place (it may delete NAPI contexts, free IRQs),
        // then the device is freed. Process context (driver unbind / module exit).
        unsafe {
            bindings::unregister_netdev(raw);
            core::ptr::drop_in_place(bindings::netdev_priv(raw).cast::<T>());
            bindings::free_netdev(raw);
        }
    }
}

/// The `net_device_ops` and `ethtool_ops` of `T`, as `'static` constants.
struct OpsTable<T>(PhantomData<T>);

/// # Safety (of every callback below)
///
/// Called by the networking core only, with a `net_device` registered by
/// `Registration<T>` (so `NetDev::<T>::from_raw` is sound for the call).
impl<T: Ops> OpsTable<T> {
    unsafe extern "C" fn open(dev: *mut bindings::net_device) -> c_int {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        from_result(|| T::open(d).map(|()| 0))
    }

    unsafe extern "C" fn stop(dev: *mut bindings::net_device) -> c_int {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        from_result(|| T::stop(d).map(|()| 0))
    }

    unsafe extern "C" fn start_xmit(
        skb: *mut bindings::sk_buff,
        dev: *mut bindings::net_device,
    ) -> bindings::netdev_tx_t {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        // SAFETY: the core passes us its reference to a valid skb.
        let skb = unsafe { SkBuff::from_raw(skb) };
        let index = u32::from(skb.queue_mapping());
        // The core picked a queue this device has (netdev_pick_tx caps it).
        let Some(q) = d.tx_queue(index) else {
            drop(skb);
            return bindings::netdev_tx_NETDEV_TX_OK;
        };
        // INVARIANT: the core holds this queue's `_xmit_lock` around `ndo_start_xmit`
        // (HARD_TX_LOCK; we never set NETIF_F_LLTX) and releases it itself.
        let mut guard = TxGuard { q, dev, unlock: Unlock::Core };
        match T::start_xmit(d, skb, &mut guard) {
            Xmit::Ok => bindings::netdev_tx_NETDEV_TX_OK,
            Xmit::Busy(skb) => {
                // The core keeps its reference and retries: give it back.
                skb.into_raw();
                bindings::netdev_tx_NETDEV_TX_BUSY
            }
        }
    }

    unsafe extern "C" fn get_stats64(
        dev: *mut bindings::net_device,
        stats: *mut bindings::rtnl_link_stats64,
    ) {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        // `stats` may be only 4-byte aligned: rtnl_fill_stats points it into the
        // netlink message (nla_reserve_64bit pads only where unaligned access is
        // slow; arm64 has none), so no reference to it may exist (a misaligned
        // `&mut` is UB; the DMC-12's debug kernel caught it, 2026-11-23). Work on
        // an aligned copy and store it back unaligned.
        // SAFETY: the core passes a valid, exclusive `rtnl_link_stats64` (possibly
        // unaligned) for the duration of the call; read_unaligned needs no alignment.
        let mut s: Stats64 = unsafe { core::ptr::read_unaligned(stats) };
        T::get_stats64(d, &mut s);
        // SAFETY: same pointer, valid and exclusive; write_unaligned needs no alignment.
        unsafe { core::ptr::write_unaligned(stats, s) };
    }

    unsafe extern "C" fn set_rx_mode(dev: *mut bindings::net_device) {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        // INVARIANT: `ndo_set_rx_mode` is called with `netif_addr_lock_bh` held.
        let lists = AddrLists { dev, _p: PhantomData };
        T::set_rx_mode(d, &lists)
    }

    unsafe extern "C" fn change_mtu(dev: *mut bindings::net_device, new_mtu: c_int) -> c_int {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        from_result(|| {
            let new = u32::try_from(new_mtu).map_err(|_| EINVAL)?;
            let old = d.mtu();
            // SAFETY: under RTNL (ndo_change_mtu); WRITE_ONCE as the core does.
            unsafe { core::ptr::write_volatile(&raw mut (*dev).mtu, new) };
            if let Err(e) = T::change_mtu(d, new, old) {
                // SAFETY: as above.
                unsafe { core::ptr::write_volatile(&raw mut (*dev).mtu, old) };
                return Err(e);
            }
            Ok(0)
        })
    }

    unsafe extern "C" fn set_mac_address(dev: *mut bindings::net_device, p: *mut c_void) -> c_int {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        from_result(|| {
            // SAFETY: `p` is a `struct sockaddr` from the core (dev_set_mac_address),
            // with at least ETH_ALEN bytes of `sa_data` for an Ethernet device.
            let sa = unsafe { &*p.cast::<bindings::sockaddr>() };
            let mut a = [0u8; 6];
            for (o, i) in a.iter_mut().zip(sa.sa_data.iter()) {
                *o = *i as u8;
            }
            let addr = MacAddr(a);
            if !addr.is_valid() {
                return Err(EADDRNOTAVAIL);
            }
            T::set_mac_address(d, addr)?;
            // SAFETY: under RTNL (ndo_set_mac_address).
            unsafe { bindings::eth_hw_addr_set(dev, addr.0.as_ptr()) };
            Ok(0)
        })
    }

    unsafe extern "C" fn tx_timeout(dev: *mut bindings::net_device, txqueue: c_uint) {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        T::tx_timeout(d, txqueue)
    }

    unsafe extern "C" fn set_features(dev: *mut bindings::net_device, f: bindings::netdev_features_t) -> c_int {
        // SAFETY: see the impl block.
        let d = unsafe { NetDev::<T>::from_raw(dev) };
        // The core applies `dev->features = features` itself when this returns 0.
        from_result(|| T::set_features(d, Features(f)).map(|()| 0))
    }

    const NDO: bindings::net_device_ops = bindings::net_device_ops {
        ndo_open: Some(Self::open),
        ndo_stop: Some(Self::stop),
        ndo_start_xmit: Some(Self::start_xmit),
        ndo_get_stats64: if T::HAS_GET_STATS64 { Some(Self::get_stats64) } else { None },
        ndo_set_rx_mode: if T::HAS_SET_RX_MODE { Some(Self::set_rx_mode) } else { None },
        ndo_change_mtu: if T::HAS_CHANGE_MTU { Some(Self::change_mtu) } else { None },
        ndo_set_mac_address: if T::HAS_SET_MAC_ADDRESS {
            Some(Self::set_mac_address)
        } else {
            None
        },
        ndo_tx_timeout: if T::HAS_TX_TIMEOUT { Some(Self::tx_timeout) } else { None },
        ndo_set_features: if T::HAS_SET_FEATURES { Some(Self::set_features) } else { None },
        ndo_validate_addr: Some(bindings::eth_validate_addr),
        ..pin_init::zeroed()
    };

    const fn build_ndo() -> &'static bindings::net_device_ops {
        &Self::NDO
    }

    const ETHTOOL: bindings::ethtool_ops = ethtool::ops::<T>();

    const fn build_ethtool() -> &'static bindings::ethtool_ops {
        &Self::ETHTOOL
    }
}

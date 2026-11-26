//! The only unsafe rim for libifcs, the Teralynx 7 SDK of the CS8210-32X (`libifcs.so` 0.12.41,
//! `dlopen`ed at run time from the switch; nothing vendor is linked). The Rust counterpart of
//! `ai_re/nix/packages/sai-teralynx/src/ifcs_lib.{h,cpp}` plus the call forms of `tl_core.h`.
//!
//! - [`load`]: the loader, step for step as `ifcs_lib_load`: absolute path, the shim
//!   `libsai-teralynx-shim.so` from next to `libsai.so` (`RTLD_NOW | RTLD_GLOBAL`, before libifcs),
//!   libifcs (the same flags), libifcs' `memcpy` GOT entries to the shim's `tl_devmem_memcpy`, the
//!   learn-ring guard (`tl_ifcs_guard_init` with libifcs' `node_pci_reg_read32` / `_write32`, their
//!   and `im_malloc`'s GOT entries to `tl_ifcs_reg_read32` / `_write32` / `tl_ifcs_im_malloc`,
//!   `tl_ifcs_guard_installed`, its log line), the call tracer of `SAI_TERALYNX_IFCS_TRACE`
//!   (`ifcs_trace_bind`, symbols looked up there first), every `IFCS_SYMBOLS` entry resolved or
//!   "missing symbol X (wrong libifcs version?)". GOT slots are rewritten by `elf-got` (which
//!   checks each slot holds the address the symbol is bound to first).
//! - [`Ifcs`]: the resolved table (cheap to clone, `Send + Sync`) with one safe method per IFCS
//!   function, named as the C function. One private wrapper per C signature holds the only unsafe
//!   call of that signature (`call.rs`, `entry.rs`, `qos.rs`): 147 of the 148 functions wrapped (all but
//!   `ifcs_status_to_string`, which nothing calls).
//! - [`Attrs`]: `ifcs_attr_t` lists as the C++ builds them (zeroed, member assigned, bitmap 0),
//!   and their three empty forms; [`Handle`]: handles and the `IFCS_HANDLE` macros; [`Status`]:
//!   status reasons, their names and their SAI errors exactly as `tl_core.cpp` maps them.
//! - [`Callbacks`] / [`Handlers`]: link scan, L2 event and log callbacks into closures over a keyed
//!   registry (no Rust memory handed to libifcs; `callback.rs` says how).
//!
//! # Locking
//!
//! This crate takes no lock around IFCS calls (libifcs has its own API lock); the C++ backend's
//! `Core::mu` (a recursive mutex every backend area takes) is, in the Rust SAI, the SAI layer's API
//! lock: every `Backend` method runs under it with `&self` / `&mut self`. What runs outside it, as
//! in the C++: the temperature read of the publisher thread (its own mutex, `temp_mu_` there) and
//! the callbacks (libifcs' threads; they make no IFCS call and only read immutable state).
//!
//! # Unsafe
//!
//! Only in `dl` (dlopen, dlsym, dlerror), `entry` (the L2 / route entry keys and calls: one call per
//! signature shape, generic over the key, and the table walk's callback), `table` (the dlsym answer as the header's function pointer
//! type: one block, in the macro over the symbol list), `call` (one call per signature), `callback`
//! (the event and message reads, the three registrations), `shim` (the guard's and the tracer's
//! functions), `netdev` (the interface ioctls that adopt the SDK's netdevs: socket, its descriptor,
//! one ioctl call), `qos` (the queue / BAM queue counter reads and clears: one call per shape; the
//! key and value initialisers go through `entry`'s shapes), `ipd` (the ipd driver's `sFlow` netlink
//! ioctls on libifcs' `/dev/ipd`: the ioctl call, the test mock's hook call, the hook's address as
//! its function type). Each block has its contract and one unsafe operation.
#![deny(unsafe_code)]
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]

pub mod attr;
mod call;
mod callback;
mod dl;
mod entry;
pub mod handle;
pub mod ipd;
pub mod load;
pub mod netdev;
pub mod qos;
mod shim;
pub mod status;
mod table;

use std::path::Path;
use std::sync::Arc;

pub use attr::Attrs;
pub use callback::{Callbacks, Handlers, L2Event, LinkEvent, callback_panics};
pub use entry::{IpPrefix, L2Key, RouteKey};
pub use handle::Handle;
/// The raw declarations (`ifcs-sys`): constants and structs the backend names.
pub use ifcs_sys as sys;
pub use load::{GuardOptions, load};
pub use qos::{MonitorPolicy, QosMapInfo, QosMapList, QosMapSrc, QueueKey};
pub use status::Status;

/// The loaded libifcs: every `IFCS_SYMBOLS` function, resolved. Cheap to clone; shared by the
/// backend and its temperature reader.
#[derive(Clone)]
pub struct Ifcs(Arc<Inner>);

pub(crate) struct Inner {
    pub(crate) t: table::Table,
    pub(crate) path: std::path::PathBuf,
    pub(crate) ipd_hook: Option<ipd::Hook>,
    pub(crate) regs: Option<(shim::RegRead, shim::RegWrite)>,
}

impl core::fmt::Debug for Ifcs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ifcs")
            .field("path", &self.0.path)
            .finish_non_exhaustive()
    }
}

impl Ifcs {
    pub(crate) fn t(&self) -> &table::Table {
        &self.0.t
    }

    /// The libifcs file this table was resolved from.
    pub fn path(&self) -> &Path {
        &self.0.path
    }

    /// Whether libifcs exports the test mock's `tl_ipd_ioctl` (`tl_sflow`.cpp uses it instead of the
    /// ipd device).
    pub fn has_ipd_ioctl_hook(&self) -> bool {
        self.0.ipd_hook.is_some()
    }
}

/// How many functions the table has (the header's `IFCS_SYMBOLS`).
pub const SYMBOL_COUNT: usize = ifcs_sys::SYMBOL_COUNT;

//! Calls into `libsai-teralynx-shim.so` (the C shim of `sai-teralynx/src/{shim.c,ifcs_guard.c}`,
//! or its Rust replacement `crates/ifcs-shim`, which exports the same symbols) and the call
//! tracer's bind function, by the signatures those files declare.
#![allow(unsafe_code)]

use core::ffi::{CStr, c_int, c_uint, c_ulonglong, c_void};
use core::ptr::NonNull;

/// `int tl_ifcs_guard_init(void* rd, void* wr, const char* stats_path, unsigned ring_size, ...)`
/// (`ifcs_guard.c`; the last argument: `unsigned long long malloc_limit`).
type GuardInit = unsafe extern "C" fn(*mut c_void, *mut c_void, *const core::ffi::c_char, c_uint, c_ulonglong) -> c_int;
/// `void tl_ifcs_guard_installed(int active)`.
type GuardInstalled = unsafe extern "C" fn(c_int);
/// `void ifcs_trace_bind(void* libifcs_handle)` (`oracle/trace_rt.c`).
type TraceBind = unsafe extern "C" fn(*mut c_void);

/// libifcs' `int node_pci_reg_read32(uint32_t node, int32_t off, uint32_t* val)` (the signature
/// `ifcs_guard.c` wraps; 0 = ok, on error `*val` is not written).
pub(crate) type RegRead = unsafe extern "C" fn(u32, i32, *mut u32) -> c_int;
/// libifcs' `int node_pci_reg_write32(uint32_t node, int32_t off, uint32_t val)`.
pub(crate) type RegWrite = unsafe extern "C" fn(u32, i32, u32) -> c_int;

/// libifcs' BAR register accessors at `rd` / `wr` (dlsym answers for those names in libifcs).
pub(crate) fn reg_fns(rd: NonNull<c_void>, wr: NonNull<c_void>) -> (RegRead, RegWrite) {
    // SAFETY: dlsym's address of node_pci_reg_read32 in libifcs, which libifcs 0.12.41 defines with
    // this signature (the one the learn-ring guard wraps and the learn thread calls).
    let r = unsafe { core::mem::transmute::<*mut c_void, RegRead>(rd.as_ptr()) };
    // SAFETY: as above, node_pci_reg_write32.
    let w = unsafe { core::mem::transmute::<*mut c_void, RegWrite>(wr.as_ptr()) };
    (r, w)
}

/// Calls the shim's `tl_ifcs_guard_init` at `f` (a dlsym answer for that name).
pub(crate) fn guard_init(
    f: NonNull<c_void>,
    rd: NonNull<c_void>,
    wr: NonNull<c_void>,
    stats: &CStr,
    ring: u32,
    limit: u64,
) -> c_int {
    // SAFETY: `f` is dlsym's address of tl_ifcs_guard_init in the shim, which ifcs_guard.c (and
    // the Rust shim) define with exactly this signature.
    let f = unsafe { core::mem::transmute::<*mut c_void, GuardInit>(f.as_ptr()) };
    // SAFETY: rd / wr are libifcs' own node_pci_reg_read32 / _write32 (the guard calls them with
    // their signature), the path is NUL-terminated (the guard copies it), the rest scalars.
    unsafe { f(rd.as_ptr(), wr.as_ptr(), stats.as_ptr(), ring, limit) }
}

/// Calls the shim's `tl_ifcs_guard_installed(active)` at `f`.
pub(crate) fn guard_installed(f: NonNull<c_void>, active: bool) {
    // SAFETY: as `guard_init`: dlsym's address of tl_ifcs_guard_installed, void(int).
    let f = unsafe { core::mem::transmute::<*mut c_void, GuardInstalled>(f.as_ptr()) };
    // SAFETY: a scalar argument.
    unsafe { f(c_int::from(active)) }
}

/// Calls the tracer's `ifcs_trace_bind(handle)` at `f` with libifcs' `dlopen` handle.
pub(crate) fn trace_bind(f: NonNull<c_void>, libifcs: *mut c_void) {
    // SAFETY: dlsym's address of ifcs_trace_bind in the tracer, void(void*) (trace_rt.c).
    let f = unsafe { core::mem::transmute::<*mut c_void, TraceBind>(f.as_ptr()) };
    // SAFETY: the tracer dlsyms the real functions from the handle it is given: libifcs' live one.
    unsafe { f(libifcs) }
}

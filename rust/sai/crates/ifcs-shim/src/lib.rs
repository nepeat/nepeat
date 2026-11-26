//! `libsai-teralynx-shim.so` in Rust: the drop-in for `ai_re/nix/packages/sai-teralynx/src/shim.c` +
//! `ifcs_guard.c`. libsai's loader dlopens it (`RTLD_NOW | RTLD_GLOBAL`, from libsai's own directory)
//! before the vendor `libifcs.so` 0.12.41 and points libifcs' GOT entries at its functions:
//!
//! * the 19 `CPython` 2.7 functions libifcs references (its CLI hooks; without them `dlopen` fails):
//!   stubs that log once and fail (NULL / -1). syncd has no Python;
//! * [`tl_devmem_memcpy`], libifcs' `memcpy` GOT target: it copies to and from the ASIC's PCI BAR
//!   (Device memory), where glibc's aarch64 `memcpy` (unaligned, 128-bit SIMD) faults;
//! * the learn-ring read guard's entry points ([`tl_ifcs_reg_read32`], [`tl_ifcs_reg_write32`],
//!   [`tl_ifcs_im_malloc`], [`tl_ifcs_guard_init`], [`tl_ifcs_guard_installed`],
//!   [`tl_ifcs_guard_text`]) over crate `ifcs-guard`, which holds every decision in safe code.
//!
//! This crate is the rim: the exported C functions, the calls through the original accessors'
//! pointers, the volatile copy, `calloc` / `errno` / `clock_gettime`. The exports are exactly the
//! C library's (26 functions; nix/ifcs-shim.nix compares `nm -D`), the soname is the same, the
//! stderr lines are the same text.
//!
//! The hot path (libifcs' learn thread calls the read wrapper about once a second, other threads
//! for every register access): no allocation, no lock beyond the C's, no thread-local with a
//! destructor (the differential harness counts `malloc` and `__cxa_thread_atexit_impl` calls per
//! call). No panic can unwind into C: every export but the copy loop runs inside `catch_unwind`
//! with the fallback each one names (and `extern "C"` aborts on an escaping unwind anyway); the
//! copy loop has no panicking operation (checked in its disassembly, nix/ifcs-shim.nix).

use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulonglong, c_void};
use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::Duration;

use ifcs_guard::{Guard, INJECT_ENV, Io, Scan};

/// The guard: one per process, as the C's statics.
static GUARD: Guard = Guard::new();
/// `G.rd` / `G.wr`: the original `node_pci_reg_read32` / `_write32` given to [`tl_ifcs_guard_init`]
/// (null before).
static RD: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static WR: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// `node_pci_reg_read32`'s signature.
type ReadFn = unsafe extern "C" fn(u32, i32, *mut u32) -> c_int;
/// `node_pci_reg_write32`'s signature.
type WriteFn = unsafe extern "C" fn(u32, i32, u32) -> c_int;

/// What the accessors answer before [`tl_ifcs_guard_init`] gave them (the C called a null pointer
/// there; libsai redirects the GOT only after a successful init, so this is never reached).
const NOT_INITIALISED: c_int = -1;

/// The guard's I/O in this process.
struct Rim;

/// The C's `fputs(s, stderr)` on the unbuffered stderr: `write(2)` until all is written or a call
/// fails (no retry, as glibc). Not `std::io::stderr()`: its re-entrant lock allocates the calling
/// thread's handle on its first use there (the differential harness saw one allocation per thread).
fn write_stderr(mut text: &[u8]) {
    while !text.is_empty() {
        // SAFETY: `text` is readable for `text.len()` bytes during the call; write(2) only reads it.
        let n = unsafe { libc::write(2, text.as_ptr().cast(), text.len()) };
        match usize::try_from(n) {
            Ok(k) if k > 0 => text = text.get(k..).unwrap_or(&[]),
            _ => break,
        }
    }
}

/// `clock_gettime` in seconds, as the C's `now_mono` / `now_real` computed them.
fn clock_seconds(clock: libc::clockid_t) -> f64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` is a live, writable timespec for the whole call; clock_gettime writes only it.
    let rc = unsafe { libc::clock_gettime(clock, &raw mut t) };
    if rc != 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss, reason = "the C's (double)tv_sec + tv_nsec / 1e9")]
    let s = t.tv_sec as f64 + t.tv_nsec as f64 / 1e9;
    s
}

impl Io for Rim {
    fn read(&self, node: u32, off: i32, val: Option<&mut u32>) -> i32 {
        let p = RD.load(Ordering::Acquire);
        if p.is_null() {
            return NOT_INITIALISED;
        }
        // SAFETY: RD holds null (checked above) or the `rd` of a tl_ifcs_guard_init call, whose
        // contract is that it is the address of a function of node_pci_reg_read32's type (ReadFn);
        // a data pointer and a function pointer have the same size and representation on the
        // targets this library is built for (x86_64, aarch64, powerpc64 Linux).
        let f = unsafe { std::mem::transmute::<*mut c_void, ReadFn>(p) };
        let out = val.map_or(null_mut(), std::ptr::from_mut);
        // SAFETY: f is node_pci_reg_read32 (or a stand-in with its contract): reads one register,
        // writes *out only when out is non-null, keeps no pointer. out is null (what libifcs passed,
        // handed on unchanged as the C did) or a unique u32 borrowed for the call.
        unsafe { f(node, off, out) }
    }

    fn write(&self, node: u32, off: i32, val: u32) -> i32 {
        let p = WR.load(Ordering::Acquire);
        if p.is_null() {
            return NOT_INITIALISED;
        }
        // SAFETY: as in `read`: WR holds the `wr` of a tl_ifcs_guard_init call, a function of
        // node_pci_reg_write32's type (WriteFn), same size and representation as a data pointer.
        let f = unsafe { std::mem::transmute::<*mut c_void, WriteFn>(p) };
        // SAFETY: f is node_pci_reg_write32 (or a stand-in): scalar arguments only.
        unsafe { f(node, off, val) }
    }

    fn mono(&self) -> f64 {
        clock_seconds(libc::CLOCK_MONOTONIC)
    }

    fn real(&self) -> f64 {
        clock_seconds(libc::CLOCK_REALTIME)
    }

    fn sleep_ms(&self, ms: u32) {
        // nanosleep, resumed with the remainder after a signal (the C's sleep_ms)
        std::thread::sleep(Duration::from_millis(ms.into()));
    }

    fn log(&self, text: &[u8]) {
        write_stderr(text);
    }

    fn write_stats(&self, tmp: &[u8], path: &[u8], data: &[u8]) -> bool {
        ifcs_guard::fs::write_stats(tmp, path, data)
    }

    fn take_inject(&self, path: &[u8]) -> Option<Scan> {
        ifcs_guard::fs::take_inject(path)
    }
}

// ---- the Device-memory copy

/// Copies `count` elements of `T` with one volatile load and one volatile store each, in order
/// (never merged, widened, split, vectorised or turned into a `memcpy` call).
///
/// # Safety
///
/// `src` must be valid for `count` reads of `T` and `dst` for `count` writes, both aligned for `T`
/// (or `count` zero).
#[allow(
    clippy::inline_always,
    reason = "the loop stays inside tl_devmem_memcpy: no call in its disassembly"
)]
#[inline(always)]
unsafe fn copy_volatile<T: Copy>(dst: *mut T, src: *const T, count: usize) {
    for k in 0..count {
        // SAFETY: k < count, and the caller guarantees count aligned elements readable at src.
        let v = unsafe { src.wrapping_add(k).read_volatile() };
        // SAFETY: k < count, and the caller guarantees count aligned elements writable at dst.
        unsafe { dst.wrapping_add(k).write_volatile(v) };
    }
}

/// libifcs' `memcpy` (GOT target): 8-byte accesses when both pointers and the length are multiples
/// of 8, else 4-byte ones when they are multiples of 4, else bytes (the C's width selection).
/// Returns `dst`.
///
/// # Safety
///
/// `memcpy`'s contract: `src` readable and `dst` writable for `n` bytes, not overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_devmem_memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (dst as usize, src as usize);
    #[allow(clippy::verbose_bit_mask, reason = "the C's test, as written there")]
    let (by8, by4) = ((d | s | n) & 7 == 0, (d | s | n) & 3 == 0);
    if by8 {
        // SAFETY: n bytes per memcpy's contract = n / 8 u64s, both pointers 8-aligned (checked).
        unsafe { copy_volatile(dst.cast::<u64>(), src.cast::<u64>(), n / 8) };
    } else if by4 {
        // SAFETY: as above with n / 4 u32s, both pointers 4-aligned (checked).
        unsafe { copy_volatile(dst.cast::<u32>(), src.cast::<u32>(), n / 4) };
    } else {
        // SAFETY: n bytes per memcpy's contract.
        unsafe { copy_volatile(dst.cast::<u8>(), src.cast::<u8>(), n) };
    }
    dst
}

// ---- the CPython 2.7 stubs

static WARNED: AtomicBool = AtomicBool::new(false);

/// The C's `warn`: one line for the first stub called, then silence.
fn warn(name: &str) {
    if WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut line = [0_u8; 160];
    let mut w = &mut line[..];
    let _ = writeln!(
        w,
        "sai-teralynx: libifcs called {name}: no Python in this process (vendor CLI hooks unavailable)"
    );
    let left = w.len();
    if let Some(text) = line.get(..line.len() - left) {
        write_stderr(text);
    }
}

/// A stub answering NULL (the C's `PTR`).
fn stub_ptr(name: &str) -> *mut c_void {
    let _ = catch_unwind(|| warn(name));
    null_mut()
}

/// A stub answering -1 (the C's `INT`, a `long`).
fn stub_int(name: &str) -> c_long {
    let _ = catch_unwind(|| warn(name));
    -1
}

/// A stub answering nothing.
fn stub_void(name: &str) {
    let _ = catch_unwind(|| warn(name));
}

/// The `CPython` functions `libifcs.so` 0.12.41 imports, as the C shim defines them. Two are variadic
/// in C (`Py_BuildValue(const char*, ...)`, `PyObject_CallFunctionObjArgs(PyObject*, ...)`): stable
/// Rust cannot define a variadic function, so they are defined with their fixed parameter only and
/// read nothing else. Sound on the ABIs this library is built for: on AAPCS64 (aarch64 Linux, not
/// Apple's variant) variadic arguments are passed exactly as named ones, in x0-x7 / v0-v7 and then
/// the caller's stack area, which the caller owns and pops; on `SysV` `x86_64` also in registers then
/// the caller's stack, plus the vector-register count in %al, which a callee may ignore; on 64-bit
/// `ELFv2` powerpc the caller allocates the parameter save area. A callee that reads only its fixed
/// argument (in its usual register) and returns in the usual register therefore sees exactly what
/// a variadic definition would, and leaves the caller's frame alone. The other stubs have the C's
/// parameter lists; none is read.
#[allow(non_snake_case, reason = "CPython's names")]
pub mod python {
    use super::{c_char, c_long, c_void, stub_int, stub_ptr, stub_void};

    /// `PyObject* Py_BuildValue(const char*, ...)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn Py_BuildValue(_format: *const c_char) -> *mut c_void {
        stub_ptr("Py_BuildValue")
    }
    /// `PyCallable_Check(PyObject*)`: -1.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyCallable_Check(_o: *mut c_void) -> c_long {
        stub_int("PyCallable_Check")
    }
    /// `PyErr_Occurred(void)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyErr_Occurred() -> *mut c_void {
        stub_ptr("PyErr_Occurred")
    }
    /// `PyErr_Print(void)`.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyErr_Print() {
        stub_void("PyErr_Print");
    }
    /// `Py_Finalize(void)`.
    #[unsafe(no_mangle)]
    pub extern "C" fn Py_Finalize() {
        stub_void("Py_Finalize");
    }
    /// `PyGILState_Ensure(void)`: -1.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyGILState_Ensure() -> c_long {
        stub_int("PyGILState_Ensure")
    }
    /// `PyGILState_Release(PyGILState_STATE)`.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyGILState_Release(_state: c_long) {
        stub_void("PyGILState_Release");
    }
    /// `PyImport_Import(PyObject*)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyImport_Import(_name: *mut c_void) -> *mut c_void {
        stub_ptr("PyImport_Import")
    }
    /// `Py_Initialize(void)`.
    #[unsafe(no_mangle)]
    pub extern "C" fn Py_Initialize() {
        stub_void("Py_Initialize");
    }
    /// `PyInt_AsLong(PyObject*)`: -1.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyInt_AsLong(_o: *mut c_void) -> c_long {
        stub_int("PyInt_AsLong")
    }
    /// `PyInt_FromLong(long)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyInt_FromLong(_v: c_long) -> *mut c_void {
        stub_ptr("PyInt_FromLong")
    }
    /// `PyList_GetItem(PyObject*, Py_ssize_t)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyList_GetItem(_l: *mut c_void, _i: c_long) -> *mut c_void {
        stub_ptr("PyList_GetItem")
    }
    /// `PyList_New(Py_ssize_t)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyList_New(_n: c_long) -> *mut c_void {
        stub_ptr("PyList_New")
    }
    /// `PyList_SetItem(PyObject*, Py_ssize_t, PyObject*)`: -1.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyList_SetItem(_l: *mut c_void, _i: c_long, _o: *mut c_void) -> c_long {
        stub_int("PyList_SetItem")
    }
    /// `PyLong_AsLong(PyObject*)`: -1.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyLong_AsLong(_o: *mut c_void) -> c_long {
        stub_int("PyLong_AsLong")
    }
    /// `PyObject* PyObject_CallFunctionObjArgs(PyObject*, ...)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyObject_CallFunctionObjArgs(_callable: *mut c_void) -> *mut c_void {
        stub_ptr("PyObject_CallFunctionObjArgs")
    }
    /// `PyObject_CallObject(PyObject*, PyObject*)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyObject_CallObject(_c: *mut c_void, _a: *mut c_void) -> *mut c_void {
        stub_ptr("PyObject_CallObject")
    }
    /// `PyObject_GetAttrString(PyObject*, const char*)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyObject_GetAttrString(_o: *mut c_void, _n: *const c_char) -> *mut c_void {
        stub_ptr("PyObject_GetAttrString")
    }
    /// `PyUnicodeUCS2_FromString(const char*)`: NULL.
    #[unsafe(no_mangle)]
    pub extern "C" fn PyUnicodeUCS2_FromString(_s: *const c_char) -> *mut c_void {
        stub_ptr("PyUnicodeUCS2_FromString")
    }
}

// ---- the learn-ring read guard

/// libifcs' `node_pci_reg_read32` (GOT target): the guard ([`ifcs_guard::Guard::reg_read32`]).
/// Fallback if the guard panicked (no path is known to): the original read, unguarded.
///
/// # Safety
///
/// `node_pci_reg_read32`'s contract: `val` is null or valid for reading and writing a `u32`, and
/// nothing else accesses it during the call. [`tl_ifcs_guard_init`] has run with valid functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_ifcs_reg_read32(node: u32, off: i32, val: *mut u32) -> c_int {
    let guarded = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller's contract: null or a valid, unaliased u32.
        let v = unsafe { val.as_mut() };
        GUARD.reg_read32(&Rim, node, off, v)
    }));
    guarded.unwrap_or_else(|_| {
        // SAFETY: as above (the borrow inside the closure has ended).
        let v = unsafe { val.as_mut() };
        Rim.read(node, off, v)
    })
}

/// libifcs' `node_pci_reg_write32` (GOT target): remembers 0x4864, then the original write.
/// Fallback after a panic: the original write.
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_reg_write32(node: u32, off: i32, val: u32) -> c_int {
    catch_unwind(|| GUARD.reg_write32(&Rim, node, off, val)).unwrap_or_else(|_| Rim.write(node, off, val))
}

/// `im_malloc` with the caller's address (the C logged `__builtin_return_address(0)`): refused
/// above the limit (NULL, `errno = ENOMEM`), else `calloc(1, n)` (`im_malloc` is malloc + memset).
/// Fallback after a panic: the limit alone decides.
extern "C" fn im_malloc_from(n: usize, caller: usize) -> *mut c_void {
    let refused = catch_unwind(|| GUARD.im_malloc_refused(&Rim, n, caller)).unwrap_or(true);
    if refused {
        // SAFETY: __errno_location has no preconditions; it returns this thread's errno.
        let errno = unsafe { libc::__errno_location() };
        // SAFETY: errno points to this thread's errno, valid for writes for the thread's lifetime.
        unsafe { errno.write(libc::ENOMEM) };
        return null_mut();
    }
    // SAFETY: calloc has no preconditions (any count and size; NULL on failure).
    unsafe { libc::calloc(1, n) }
}

/// libifcs' `im_malloc(n)` (GOT target). A tail jump into the Rust function with the return
/// address as its second argument (the C's `__builtin_return_address(0)`, which stable Rust has no
/// intrinsic for): x30 on aarch64.
#[cfg(target_arch = "aarch64")]
// Sound as naked: only the two instructions below, no prologue. x0 (n) is left alone, x1 = x30 (the
// return address), then a plain branch: im_malloc_from returns straight to tl_ifcs_im_malloc's
// caller with the stack and every callee-saved register as they were at entry.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_im_malloc(n: usize) -> *mut c_void {
    std::arch::naked_asm!("mov x1, x30", "b {f}", f = sym im_malloc_from)
}

/// libifcs' `im_malloc(n)` (GOT target): as on aarch64; on `x86_64` the return address is at `[rsp]`.
#[cfg(target_arch = "x86_64")]
// Sound as naked: rdi (n) is left alone, rsi = the return address the call pushed, then a jump:
// im_malloc_from sees the stack as at a call (rsp + 8 aligned) and returns to the caller.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_im_malloc(n: usize) -> *mut c_void {
    std::arch::naked_asm!("mov rsi, qword ptr [rsp]", "jmp {f}", f = sym im_malloc_from)
}

/// libifcs' `im_malloc(n)` elsewhere (never deployed: the CS8210 is aarch64): caller unknown,
/// logged as `(nil)`.
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_im_malloc(n: usize) -> *mut c_void {
    im_malloc_from(n, 0)
}

/// libsai before it redirects the GOT entries: the original accessors, the counters' file (`stats`,
/// NULL or "" = none), the ring size (0 = 0x800) and the `im_malloc` limit (bytes, 0 = none); the test
/// hook's file from [`INJECT_ENV`]. -1 (nothing changed) when `rd` or `wr` is null, else 0.
/// Fallback after a panic: -1 (libsai then leaves the GOT alone: unguarded, and says so).
///
/// # Safety
///
/// `rd` and `wr` are null or the addresses of functions with the signatures and contracts of
/// libifcs' `node_pci_reg_read32` and `node_pci_reg_write32` that stay valid while the guard is
/// used; `stats` is null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tl_ifcs_guard_init(
    rd: *mut c_void,
    wr: *mut c_void,
    stats: *const c_char,
    ring_size: c_uint,
    malloc_limit: c_ulonglong,
) -> c_int {
    if rd.is_null() || wr.is_null() {
        return -1;
    }
    let stats = if stats.is_null() {
        &[][..]
    } else {
        // SAFETY: the caller's contract: a NUL-terminated string, read before this call returns.
        unsafe { CStr::from_ptr(stats) }.to_bytes()
    };
    catch_unwind(|| {
        RD.store(rd, Ordering::Release);
        WR.store(wr, Ordering::Release);
        let inject = std::env::var_os(INJECT_ENV).unwrap_or_default();
        GUARD.init(
            stats,
            ring_size,
            malloc_limit,
            std::os::unix::ffi::OsStrExt::as_bytes(inject.as_os_str()),
        );
        0
    })
    .unwrap_or(-1)
}

/// libsai after the redirect: 1 = the read accessor is redirected (the guard is live). Rewrites the
/// counters' file. Fallback after a panic: nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_guard_installed(on: c_int) {
    let _ = catch_unwind(|| GUARD.installed(&Rim, on));
}

/// The counters as Prometheus text (tests): a NUL-terminated string in a buffer of the guard,
/// rewritten by the next call (not thread-safe across callers, as the C's static buffer).
/// Fallback after a panic: `""`.
#[unsafe(no_mangle)]
pub extern "C" fn tl_ifcs_guard_text() -> *const c_char {
    catch_unwind(|| GUARD.text_ptr(&Rim).cast::<c_char>()).unwrap_or(c"".as_ptr())
}

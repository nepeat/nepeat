//! The mock libifcs's test hooks, for the end-to-end test of the Teralynx libsai
//! (`crates/sai-backend-teralynx/tests/ifcs_e2e`): the C mock of `rust/sai/tests/teralynx-mock` that
//! the libsai loads as its libifcs records every IFCS call and exports small `mock_*` functions
//! (the call log, link / L2 events to fire, state counters). The test attaches to the library the
//! libsai opened (`dlopen(RTLD_NOLOAD)`) and calls those by name, as the C++ `test_ifcs` does with
//! `dlsym`; plus the shim's learn-ring guard (`tl_ifcs_guard_text` / `tl_ifcs_guard_init`, found in
//! the global scope).
//!
//! Every hook is a safe method. The ones that take or return scalars only come from one macro (one
//! call site each); the ones with pointers have their own, with the buffers owned here.
#![allow(unsafe_code)]

use core::ffi::{CStr, c_char, c_int, c_uint, c_ulonglong, c_void};
use std::ffi::CString;

use crate::dl::Library;

/// A route of the mock's table: `fwd` (1: drop / 0: forward), `ctc` (copy to CPU), the next hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// forwarding action (the mock's `fwd`)
    pub fwd: i32,
    /// copy-to-CPU flag
    pub ctc: i32,
    /// next-hop handle
    pub nh: u32,
}

/// A router interface of the mock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intf {
    /// type
    pub ty: u32,
    /// forwarding L3VNI handle
    pub fwd: u32,
    /// transport handle
    pub transport: u32,
    /// MTU
    pub mtu: u32,
    /// source MAC
    pub mac: [u8; 6],
}

/// A next hop of the mock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NextHop {
    /// interface handle
    pub intf: u32,
    /// destination MAC
    pub mac: [u8; 6],
    /// destination (sysport or LAG handle)
    pub dest: u32,
}

macro_rules! scalar_hooks {
    ($($name:ident, $sym:literal, ($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        /// The hooks with scalar arguments and results only.
        struct Scalars {
            $($name: unsafe extern "C" fn($($ty),*) -> $ret,)*
        }

        impl Scalars {
            fn resolve(lib: &Library) -> Result<Self, String> {
                Ok(Self {
                    $($name: lib
                        .function_named::<unsafe extern "C" fn($($ty),*) -> $ret>($sym)
                        .ok_or_else(|| format!("mock hook {} missing", $sym))?,)*
                })
            }
        }

        impl Mock {
            $(
                #[doc = concat!("`", $sym, "`")]
                pub fn $name(&self, $($arg: $ty),*) -> $ret {
                    // SAFETY: the hook's prototype has scalar arguments and a scalar (or no)
                    // result only (the macro's list), so no pointer is passed or returned.
                    unsafe { (self.scalars.$name)($($arg),*) }
                }
            )*
        }
    };
}

scalar_hooks! {
    log_clear, "mock_ifcs_log_clear", () -> ();
    admin, "mock_admin", (dp: u32) -> u32;
    fire_link, "mock_fire_link", (dp: u32, up: i32) -> i32;
    set_stat, "mock_set_stat", (dp: u32, id: u32, v: u64) -> ();
    route_count, "mock_route_count", () -> i32;
    l2_count, "mock_l2_count", () -> i32;
    inited, "mock_inited", () -> i32;
    fec, "mock_fec", (dp: u32) -> u32;
    aging, "mock_aging", () -> u32;
    temp_reads, "mock_temp_reads", () -> i32;
    link_reads, "mock_link_reads", () -> i32;
    l2vni_has, "mock_l2vni_has", (vid: u16, handle: u32) -> i32;
    lag_count, "mock_lag_count", () -> i32;
    efl_count, "mock_efl_count", () -> i32;
    intf_count, "mock_intf_count", () -> i32;
    nh_count, "mock_nh_count", () -> i32;
    ecmp_count, "mock_ecmp_count", () -> i32;
    fail_next_route, "mock_fail_next_route", () -> ();
    acl_aces, "mock_acl_aces", () -> i32;
    acl_meters, "mock_acl_meters", () -> i32;
    acl_bound, "mock_acl_bound", () -> i32;
    acl_set_counter, "mock_acl_set_counter", (v: u64) -> ();
    acl_fail_next_ace, "mock_acl_fail_next_ace", () -> ();
    acl_comparators, "mock_acl_comparators", () -> i32;
    qos_maps, "mock_qos_maps", () -> i32;
    qos_groups, "mock_qos_groups", () -> i32;
    qos_curves, "mock_qos_curves", () -> i32;
    queue_stat, "mock_queue_stat", (dp: u32, q: u32, id: i32, v: u64) -> ();
    bam_stat, "mock_bam_stat", (dp: u32, q: u32, id: i32, v: u64) -> ();
    queue_attr, "mock_queue_attr", (dp: u32, q: u32, id: u32) -> u32;
    ipd_calls, "mock_ipd_calls", () -> i32;
    ipd_fail, "mock_ipd_fail", (nr: u32, err: i32) -> ();
    sflow_egress, "mock_sflow_egress", (on: i32) -> ();
    mirror_dcdps, "mock_mirror_dcdps", () -> i32;
    mirror_collectors, "mock_mirror_collectors", () -> i32;
    mirror_sets, "mock_mirror_sets", () -> i32;
    mirror_members, "mock_mirror_members", () -> i32;
    set_intf_stat, "mock_set_intf_stat", (handle: u32, id: i32, v: u64) -> ();
    learn_pass, "mock_learn_pass", () -> i32;
    learn_pass_unguarded, "mock_learn_pass_unguarded", () -> i32;
    learn_wedged, "mock_learn_wedged", () -> i32;
    reg_set, "mock_reg_set", (off: i32, v: u32) -> ();
    reg_fail, "mock_reg_fail", (off: i32, n: u32) -> ();
    reg_get, "mock_reg_get", (off: i32) -> u32;
    learn_reset, "mock_learn_reset", (a: u32) -> ();
    learn_sw_index, "mock_learn_sw_index", () -> u32;
    learn_last_request, "mock_learn_last_request", () -> u64;
    im_malloc_calls, "mock_im_malloc_calls", () -> u64;
    im_malloc_max, "mock_im_malloc_max", () -> u64;
    learn_read, "mock_learn_read", (off: i32) -> u32;
}

type LogFn = unsafe extern "C" fn() -> *const c_char;
type FireL2Fn = unsafe extern "C" fn(c_int, *const u8, u16, u32) -> c_int;
type L2DestFn = unsafe extern "C" fn(*const u8, u16) -> u32;
type RouteFindFn = unsafe extern "C" fn(*const c_char, *mut c_int, *mut c_int, *mut u32) -> c_int;
type KeyIntFn = unsafe extern "C" fn(*const c_char) -> c_int;
type KeyU32Fn = unsafe extern "C" fn(*const c_char) -> u32;
type LagMembersFn = unsafe extern "C" fn(u32, *mut u32) -> c_int;
type IntfInfoFn = unsafe extern "C" fn(u32, *mut u32, *mut u32, *mut u32, *mut u32, *mut u8) -> c_int;
type NhInfoFn = unsafe extern "C" fn(u32, *mut u32, *mut u8, *mut u32) -> c_int;
type EcmpMembersFn = unsafe extern "C" fn(u32, *mut u32, *mut u32, c_int) -> c_int;
type ImMallocFn = unsafe extern "C" fn(usize) -> *mut c_void;
type GuardTextFn = unsafe extern "C" fn() -> *const c_char;
type GuardInitFn = unsafe extern "C" fn(*const c_void, *const c_void, *const c_char, c_uint, c_ulonglong) -> c_int;

/// The hooks with pointers.
struct Pointers {
    log: LogFn,
    fire_l2: FireL2Fn,
    fire_l2_dest: FireL2Fn,
    l2_dest: L2DestFn,
    route_find: RouteFindFn,
    route_ttlp: KeyIntFn,
    route_trap: KeyU32Fn,
    efl_find: KeyIntFn,
    lag_members: LagMembersFn,
    intf_info: IntfInfoFn,
    nh_info: NhInfoFn,
    ecmp_members: EcmpMembersFn,
    learn_im_malloc: ImMallocFn,
}

/// The mock libifcs the libsai has loaded, and its hooks.
pub struct Mock {
    lib: Library,
    scalars: Scalars,
    ptrs: Pointers,
}

impl std::fmt::Debug for Mock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mock").finish_non_exhaustive()
    }
}

fn key(text: &str) -> Option<CString> {
    CString::new(text).ok()
}

impl Mock {
    /// Whether the library at `path` is loaded in this process (`dlopen(RTLD_NOLOAD)`).
    pub fn is_loaded(path: &str) -> bool {
        Library::attach(path).is_some()
    }

    /// The loaded mock at `path` (the libsai opened it at `create_switch`), every hook resolved.
    pub fn attach(path: &str) -> Result<Self, String> {
        let lib = Library::attach(path).ok_or_else(|| format!("{path}: not loaded"))?;
        let miss = |n: &str| format!("mock hook {n} missing");
        let ptrs = Pointers {
            log: lib
                .function_named("mock_ifcs_log")
                .ok_or_else(|| miss("mock_ifcs_log"))?,
            fire_l2: lib.function_named("mock_fire_l2").ok_or_else(|| miss("mock_fire_l2"))?,
            fire_l2_dest: lib
                .function_named("mock_fire_l2_dest")
                .ok_or_else(|| miss("mock_fire_l2_dest"))?,
            l2_dest: lib.function_named("mock_l2_dest").ok_or_else(|| miss("mock_l2_dest"))?,
            route_find: lib
                .function_named("mock_route_find")
                .ok_or_else(|| miss("mock_route_find"))?,
            route_ttlp: lib
                .function_named("mock_route_ttlp")
                .ok_or_else(|| miss("mock_route_ttlp"))?,
            route_trap: lib
                .function_named("mock_route_trap")
                .ok_or_else(|| miss("mock_route_trap"))?,
            efl_find: lib
                .function_named("mock_efl_find")
                .ok_or_else(|| miss("mock_efl_find"))?,
            lag_members: lib
                .function_named("mock_lag_members")
                .ok_or_else(|| miss("mock_lag_members"))?,
            intf_info: lib
                .function_named("mock_intf_info")
                .ok_or_else(|| miss("mock_intf_info"))?,
            nh_info: lib.function_named("mock_nh_info").ok_or_else(|| miss("mock_nh_info"))?,
            ecmp_members: lib
                .function_named("mock_ecmp_members")
                .ok_or_else(|| miss("mock_ecmp_members"))?,
            learn_im_malloc: lib
                .function_named("mock_learn_im_malloc")
                .ok_or_else(|| miss("mock_learn_im_malloc"))?,
        };
        let scalars = Scalars::resolve(&lib)?;
        Ok(Self { lib, scalars, ptrs })
    }

    /// `mock_ifcs_log()`: every IFCS call so far (one line each), copied.
    pub fn log(&self) -> String {
        // SAFETY: no arguments; the mock returns its log buffer, NUL-terminated (never NULL).
        let p = unsafe { (self.ptrs.log)() };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: non-null and NUL-terminated (above); copied at once, before the mock's next
        // log call can reallocate the buffer (as the C++ test reads it).
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    /// `mock_fire_l2(type, mac, vid, devport)`: an L2 event from the SDK.
    pub fn fire_l2(&self, kind: i32, mac: &[u8; 6], vid: u16, devport: u32) -> i32 {
        // SAFETY: `mac` is 6 readable bytes for the call (the mock copies them).
        unsafe { (self.ptrs.fire_l2)(kind, mac.as_ptr(), vid, devport) }
    }

    /// `mock_fire_l2_dest(type, mac, vid, dest)`: an L2 event with a destination handle.
    pub fn fire_l2_dest(&self, kind: i32, mac: &[u8; 6], vid: u16, dest: u32) -> i32 {
        // SAFETY: as `fire_l2`.
        unsafe { (self.ptrs.fire_l2_dest)(kind, mac.as_ptr(), vid, dest) }
    }

    /// `mock_l2_dest(mac, vid)`: the destination of the L2 entry (0: none).
    pub fn l2_dest(&self, mac: &[u8; 6], vid: u16) -> u32 {
        // SAFETY: `mac` is 6 readable bytes for the call.
        unsafe { (self.ptrs.l2_dest)(mac.as_ptr(), vid) }
    }

    /// `mock_route_find(key)`: the route of the key text, if there is one.
    pub fn route_find(&self, text: &str) -> Option<Route> {
        let k = key(text)?;
        let (mut fwd, mut ctc, mut nh) = (-1, -1, 0u32);
        // SAFETY: a NUL-terminated key and three writable slots.
        let found = unsafe { (self.ptrs.route_find)(k.as_ptr(), &raw mut fwd, &raw mut ctc, &raw mut nh) };
        (found != 0).then_some(Route { fwd, ctc, nh })
    }

    /// `mock_route_ttlp(key)`: the TTL-check profile of the route.
    pub fn route_ttlp(&self, text: &str) -> i32 {
        let Some(k) = key(text) else { return -1 };
        // SAFETY: a NUL-terminated key.
        unsafe { (self.ptrs.route_ttlp)(k.as_ptr()) }
    }

    /// `mock_route_trap(key)`: the hostif trap handle of the route.
    pub fn route_trap(&self, text: &str) -> u32 {
        let Some(k) = key(text) else { return 0 };
        // SAFETY: a NUL-terminated key.
        unsafe { (self.ptrs.route_trap)(k.as_ptr()) }
    }

    /// `mock_efl_find(elements)`: 1 if a live extended field list has exactly these elements.
    pub fn efl_find(&self, elements: &str) -> i32 {
        let Some(k) = key(elements) else { return 0 };
        // SAFETY: a NUL-terminated string.
        unsafe { (self.ptrs.efl_find)(k.as_ptr()) }
    }

    /// `mock_lag_members(handle)`: the member count (-1: no such LAG) and the first member.
    pub fn lag_members(&self, handle: u32) -> (i32, u32) {
        let mut first = 0u32;
        // SAFETY: a writable slot for the first member.
        let n = unsafe { (self.ptrs.lag_members)(handle, &raw mut first) };
        (n, first)
    }

    /// `mock_intf_info(handle)`.
    pub fn intf_info(&self, handle: u32) -> Option<Intf> {
        let (mut ty, mut fwd, mut transport, mut mtu, mut mac) = (0u32, 0u32, 0u32, 0u32, [0u8; 6]);
        // SAFETY: four writable u32 slots and a writable 6-byte MAC.
        let found = unsafe {
            (self.ptrs.intf_info)(
                handle,
                &raw mut ty,
                &raw mut fwd,
                &raw mut transport,
                &raw mut mtu,
                mac.as_mut_ptr(),
            )
        };
        (found != 0).then_some(Intf {
            ty,
            fwd,
            transport,
            mtu,
            mac,
        })
    }

    /// `mock_nh_info(handle)`.
    pub fn nh_info(&self, handle: u32) -> Option<NextHop> {
        let (mut intf, mut dest, mut mac) = (0u32, 0u32, [0u8; 6]);
        // SAFETY: two writable u32 slots and a writable 6-byte MAC.
        let found = unsafe { (self.ptrs.nh_info)(handle, &raw mut intf, mac.as_mut_ptr(), &raw mut dest) };
        (found != 0).then_some(NextHop { intf, mac, dest })
    }

    /// `mock_ecmp_members(handle, max)`: the group's member count (-1: no such group) and the
    /// first `max` members' handles and weights.
    pub fn ecmp_members(&self, handle: u32, max: usize) -> (i32, Vec<u32>, Vec<u32>) {
        let mut m = vec![0u32; max];
        let mut w = vec![0u32; max];
        let room = c_int::try_from(max).unwrap_or(c_int::MAX);
        // SAFETY: two writable arrays of `max` entries (the mock writes at most `room`).
        let n = unsafe { (self.ptrs.ecmp_members)(handle, m.as_mut_ptr(), w.as_mut_ptr(), room) };
        let got = usize::try_from(n).unwrap_or(0).min(max);
        m.truncate(got);
        w.truncate(got);
        (n, m, w)
    }

    /// `mock_learn_im_malloc(n)`: libifcs' `im_malloc` of `n` bytes through its PLT (the shim's
    /// guard answers it). `None` when it returns NULL; else the last byte of the zeroed block
    /// (freed again).
    pub fn learn_im_malloc_last_byte(&self, n: usize) -> Option<u8> {
        // SAFETY: a size; the mock calls im_malloc, which returns NULL or `n` writable bytes.
        let p = unsafe { (self.ptrs.learn_im_malloc)(n) }.cast::<u8>();
        if p.is_null() || n == 0 {
            return None;
        }
        let end = p.wrapping_add(n - 1);
        // SAFETY: `end` is the last byte of the live block of `n` bytes `p` points to (non-null above).
        let last = unsafe { end.read() };
        // SAFETY: the block came from the guard's calloc (libc allocator) and is freed once.
        unsafe { libc::free(p.cast::<c_void>()) };
        Some(last)
    }

    /// `tl_ifcs_guard_text()`: the learn-ring guard's counters text (the shim is loaded
    /// `RTLD_GLOBAL`, so the global scope has it).
    pub fn guard_text(&self) -> Result<String, String> {
        let f = Library::global_function::<GuardTextFn>("tl_ifcs_guard_text")
            .ok_or_else(|| "symbol tl_ifcs_guard_text missing".to_owned())?;
        // SAFETY: no arguments; the guard returns a NUL-terminated buffer of its own.
        let p = unsafe { f() };
        if p.is_null() {
            return Ok(String::new());
        }
        // SAFETY: non-null and NUL-terminated (above); copied at once.
        Ok(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// `tl_ifcs_guard_init(mock_reg_read_impl, mock_reg_write_impl, prom, ring, limit)`: a fresh
    /// guard over the mock's register accessors (as at a new syncd start).
    pub fn guard_reinit(&self, prom: &str, ring: u32, limit: u64) -> Result<i32, String> {
        let f = Library::global_function::<GuardInitFn>("tl_ifcs_guard_init")
            .ok_or_else(|| "symbol tl_ifcs_guard_init missing".to_owned())?;
        let rd = self
            .lib
            .address("mock_reg_read_impl")
            .ok_or("mock_reg_read_impl missing")?;
        let wr = self
            .lib
            .address("mock_reg_write_impl")
            .ok_or("mock_reg_write_impl missing")?;
        let path = CString::new(prom).map_err(|_| "a NUL in the guard file path".to_owned())?;
        // SAFETY: the two accessors are the mock's register read / write functions (the guard's
        // original accessors, as `ifcs_lib` hands them), a NUL-terminated path, two integers.
        Ok(unsafe { f(rd.as_ptr(), wr.as_ptr(), path.as_ptr(), ring, c_ulonglong::from(limit)) })
    }
}

// SPDX-License-Identifier: GPL-2.0

//! ppc64 ABI conformance test: Rust (rustc) against C (GCC) in one module.
//!
//! On load it runs every check and prints one line per failure and a final
//! "ppc64abi: RESULT pass=N fail=M"; loading always succeeds so the module can
//! be unloaded and loaded again. What is checked: ../README.md, "ABI tests".

#![allow(clippy::undocumented_unsafe_blocks, clashing_extern_declarations)]
#![allow(improper_ctypes, improper_ctypes_definitions)] // u128 is part of what is tested

use core::ffi::{c_int, c_void};
use core::hint::black_box;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering as O};
use kernel::prelude::*;
use kernel::sync::atomic::{ordering::Full, Atomic};
use kernel::sync::{Mutex, SpinLock};
use kernel::{new_mutex, new_spinlock};

mod abi_gen;

module! {
    type: Abi,
    name: "rust_ppc64_abi",
    authors: ["ai_re"],
    description: "Rust/C ABI conformance checks for 64-bit big-endian powerpc",
    license: "GPL",
}

pub(crate) const H0: u64 = 0xcbf2_9ce4_8422_2325;

pub(crate) fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

pub(crate) fn sm(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The same "dirty" 64-bit sources as abi_c.c: narrow values are cut from them.
static DIRTY: [u64; 8] = [
    0xdead_beef_0000_00f1, 0xdead_beef_0000_fff1, 0xdead_beef_0000_f2f3, 0xdead_beef_0000_f448,
    0xdead_beef_f4f5_f6f7, 0xdead_beef_fffe_7960, 0x8000_0000_0000_0001, 0xfedc_ba98_7654_3210,
];

fn d(i: usize) -> u64 {
    // SAFETY: in-bounds read of a static; volatile so the compiler cannot
    // fold the narrowing casts into clean constants.
    unsafe { core::ptr::read_volatile(&DIRTY[i]) }
}

#[allow(clippy::too_many_arguments)]
fn hash_scalars(a: u8, b: i8, c: u16, dd: i16, e: u32, f: i32, g: bool, h: u64,
                i: u8, j: i8, k: u16, l: i16, m: u32, n: i32, o: bool, p: u64) -> u64 {
    let mut x = H0;
    for v in [u64::from(a), b as i64 as u64, u64::from(c), dd as i64 as u64, u64::from(e),
              f as i64 as u64, u64::from(g), h, u64::from(i), j as i64 as u64, u64::from(k),
              l as i64 as u64, u64::from(m), n as i64 as u64, u64::from(o), p] {
        x = mix(x, v);
    }
    x
}

fn u128_op(a: u64, b: u128, c: u64, dd: u128) -> u128 {
    b.wrapping_mul(3).wrapping_add(dd ^ (u128::from(a) << 64)).wrapping_add(u128::from(c))
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn abi_rs_ints(a: u8, b: i8, c: u16, dd: i16, e: u32, f: i32, g: bool, h: u64,
                                 i: u8, j: i8, k: u16, l: i16, m: u32, n: i32, o: bool, p: u64) -> u64 {
    hash_scalars(a, b, c, dd, e, f, g, h, i, j, k, l, m, n, o, p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn abi_rs_u128(a: u64, b: u128, c: u64, dd: u128) -> u128 {
    u128_op(a, b, c, dd)
}

macro_rules! rs_ret {
    ($name:ident, $t:ty, $e:expr) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name() -> $t {
            $e
        }
    };
}
rs_ret!(abi_rs_r_u8, u8, d(0) as u8);
rs_ret!(abi_rs_r_s8, i8, d(1) as i8);
rs_ret!(abi_rs_r_u16, u16, d(2) as u16);
rs_ret!(abi_rs_r_s16, i16, d(3) as i16);
rs_ret!(abi_rs_r_u32, u32, d(4) as u32);
rs_ret!(abi_rs_r_s32, i32, d(5) as i32);
rs_ret!(abi_rs_r_bool, bool, d(6) & 1 != 0);

unsafe extern "C" {
    static mut abi_regs: [u64; 32];
    fn abi_capture();
    fn abi_call_ret(f: *const c_void) -> u64;
    #[allow(clippy::too_many_arguments)]
    fn abi_c_ints(a: u8, b: i8, c: u16, d: i16, e: u32, f: i32, g: bool, h: u64,
                  i: u8, j: i8, k: u16, l: i16, m: u32, n: i32, o: bool, p: u64) -> u64;
    fn abi_c_u128(a: u64, b: u128, c: u64, d: u128) -> u128;
    fn abi_c_check_rust_scalars() -> c_int;
    fn abi_c_check_rust_structs() -> c_int;
    fn abi_c_capture_ints(out: *mut u64);
    fn abi_c_capture_u128(out: *mut u64);
    fn abi_c_capture_ptrs(out: *mut u64);
    fn abi_c_r_u8() -> u8;
    fn abi_c_r_s8() -> i8;
    fn abi_c_r_u16() -> u16;
    fn abi_c_r_s16() -> i16;
    fn abi_c_r_u32() -> u32;
    fn abi_c_r_s32() -> i32;
    fn abi_c_r_bool() -> bool;
    fn abi_c_va(n: c_int, ...) -> u64;
    fn abi_c_run_threads(ctx: *mut c_void, f: unsafe extern "C" fn(*mut c_void, c_int, u64) -> u64,
                         iters: u64) -> c_int;
}
// abi_capture under the prototypes the captures use (same symbol).
unsafe extern "C" {
    #[link_name = "abi_capture"]
    #[allow(clippy::too_many_arguments)]
    fn cap_ints(a: u8, b: i8, c: u16, d: i16, e: u32, f: i32, g: bool, h: u64,
                i: u8, j: i8, k: u16, l: i16, m: u32, n: i32, o: bool, p: u64);
    #[link_name = "abi_capture"]
    fn cap_u128(a: u64, b: u128, c: u64, d: u128);
    #[link_name = "abi_capture"]
    fn cap_ptrs(p: *const u8, q: usize, r: isize, f: unsafe extern "C" fn(), b: u8);
}

fn regs() -> [u64; 16] {
    // SAFETY: abi_regs is only written by abi_capture, which the caller ran
    // just before on this CPU; nothing else touches it.
    let all: [u64; 32] = unsafe { core::ptr::read(&raw const abi_regs) };
    let mut out = [0u64; 16];
    out.copy_from_slice(&all[..16]);
    out
}

struct Tally {
    pass: u32,
    fail: u32,
}

impl Tally {
    fn check(&mut self, ok: bool, what: core::fmt::Arguments<'_>) {
        if ok {
            self.pass += 1;
        } else {
            self.fail += 1;
            pr_err!("ppc64abi: FAIL {}\n", what);
        }
    }
    fn count(&mut self, fails: u32, total: u32) {
        self.fail += fails;
        self.pass += total - fails.min(total);
    }
}

/// Compare a captured image with the ELFv2 one at the given doubleword indexes.
fn image(t: &mut Tally, who: &str, what: &str, got: &[u64; 16], want: &[(usize, u64)]) {
    for &(i, w) in want {
        t.check(got[i] == w, format_args!("{} {} doubleword {}: {:#018x} != ELFv2 {:#018x}", who, what, i, got[i], w));
    }
}

fn scalars(t: &mut Tally) {
    // Rust -> C, values
    let (a, b, c, dd, e, f, g, h) = (d(0) as u8, d(1) as i8, d(2) as u16, d(3) as i16, d(4) as u32,
                                     d(5) as i32, d(6) & 1 != 0, d(7));
    let (i, j, k, l, m, n, o, p) = (d(7) as u8, d(0) as i8, d(1) as u16, d(2) as i16, d(5) as u32,
                                    d(4) as i32, d(6) & 1 == 0, d(0));
    let want = hash_scalars(a, b, c, dd, e, f, g, h, i, j, k, l, m, n, o, p);
    // SAFETY: prototypes as in abi_c.c.
    let got = unsafe { abi_c_ints(a, b, c, dd, e, f, g, h, i, j, k, l, m, n, o, p) };
    t.check(got == want, format_args!("Rust->C 16 scalars: {:x} != {:x}", got, want));
    let (q1, q2) = ((u128::from(d(7)) << 64) | u128::from(d(6)), (u128::from(d(6)) << 64) | u128::from(d(4)));
    // SAFETY: as above.
    let got = unsafe { abi_c_u128(d(0), q1, d(1), q2) };
    t.check(got == u128_op(d(0), q1, d(1), q2), format_args!("Rust->C u128"));
    // C -> Rust
    // SAFETY: as above.
    t.count(unsafe { abi_c_check_rust_scalars() } as u32, 2);

    // Caller-side placement and extension: ELFv2 image, rustc and GCC.
    let ints: [(usize, u64); 16] = [
        (0, 0xf1), (1, 0xffff_ffff_ffff_fff1), (2, 0xf2f3), (3, 0xffff_ffff_ffff_f448),
        (4, 0xf4f5_f6f7), (5, 0xffff_ffff_fffe_7960), (6, 1), (7, 0xfedc_ba98_7654_3210),
        (8, 0x10), (9, 0xffff_ffff_ffff_fff1), (10, 0xfff1), (11, 0xffff_ffff_ffff_f2f3),
        (12, 0xfffe_7960), (13, 0xffff_ffff_f4f5_f6f7), (14, 0), (15, 0xdead_beef_0000_00f1),
    ];
    // SAFETY: abi_capture accepts any arguments and only stores registers.
    unsafe { cap_ints(a, b, c, dd, e, f, g, h, i, j, k, l, m, n, o, p) };
    image(t, "rustc", "16 scalars", &regs(), &ints);
    let mut cbuf = [0u64; 16];
    // SAFETY: writes 16 u64 into cbuf.
    unsafe { abi_c_capture_ints(cbuf.as_mut_ptr()) };
    image(t, "gcc", "16 scalars", &cbuf, &ints);

    // __int128: two consecutive doublewords, the high one first (BE). GCC does
    // not start them at an even register (r4:r5 here, not r5:r6); rustc must
    // do the same.
    let q: [(usize, u64); 6] = [(0, d(0)), (1, d(7)), (2, d(6)), (3, d(1)), (4, d(6)), (5, d(4))];
    // SAFETY: as above.
    unsafe { cap_u128(d(0), q1, d(1), q2) };
    image(t, "rustc", "u128 pairs", &regs(), &q);
    // SAFETY: as above.
    unsafe { abi_c_capture_u128(cbuf.as_mut_ptr()) };
    image(t, "gcc", "u128 pairs", &cbuf, &q);

    // pointers, isize, a function pointer (ELFv2: the global entry address)
    let fptr = abi_capture as unsafe extern "C" fn() as usize as u64;
    let pp: [(usize, u64); 5] = [(0, d(7)), (1, d(6)), (2, d(5)), (3, fptr), (4, 0x48)];
    // SAFETY: as above; the pointer is never dereferenced.
    unsafe { cap_ptrs(core::ptr::without_provenance(d(7) as usize), d(6) as usize, d(5) as isize, abi_capture, d(3) as u8) };
    image(t, "rustc", "pointers", &regs(), &pp);
    // SAFETY: as above.
    unsafe { abi_c_capture_ptrs(cbuf.as_mut_ptr()) };
    image(t, "gcc", "pointers", &cbuf, &pp);

    // Narrow return values: r3 must be extended to 64 bits (both compilers)
    let rets: [(&str, *const c_void, *const c_void, u64); 7] = [
        ("u8", abi_rs_r_u8 as *const c_void, abi_c_r_u8 as *const c_void, 0xf1),
        ("i8", abi_rs_r_s8 as *const c_void, abi_c_r_s8 as *const c_void, 0xffff_ffff_ffff_fff1),
        ("u16", abi_rs_r_u16 as *const c_void, abi_c_r_u16 as *const c_void, 0xf2f3),
        ("i16", abi_rs_r_s16 as *const c_void, abi_c_r_s16 as *const c_void, 0xffff_ffff_ffff_f448),
        ("u32", abi_rs_r_u32 as *const c_void, abi_c_r_u32 as *const c_void, 0xf4f5_f6f7),
        ("i32", abi_rs_r_s32 as *const c_void, abi_c_r_s32 as *const c_void, 0xffff_ffff_fffe_7960),
        ("bool", abi_rs_r_bool as *const c_void, abi_c_r_bool as *const c_void, 1),
    ];
    for (name, rs, c, want) in rets {
        // SAFETY: both are functions without arguments; abi_call_ret calls
        // them through r12/ctr as C would and returns r3 untouched.
        let (gr, gc) = unsafe { (abi_call_ret(rs), abi_call_ret(c)) };
        t.check(gr == want, format_args!("rustc returns {} as {:#018x}, ELFv2 {:#018x}", name, gr, want));
        t.check(gc == want, format_args!("gcc returns {} as {:#018x}, ELFv2 {:#018x}", name, gc, want));
    }

    // C varargs from Rust, 12 values (spill to the stack)
    let mut h = H0;
    for x in 0..12u64 {
        h = mix(h, sm(x));
    }
    // SAFETY: abi_c_va reads exactly n u64 varargs.
    let got = unsafe { abi_c_va(12, sm(0), sm(1), sm(2), sm(3), sm(4), sm(5), sm(6), sm(7), sm(8), sm(9), sm(10), sm(11)) };
    t.check(got == h, format_args!("Rust->C varargs"));
}

#[repr(C, align(8))]
struct Bytes([AtomicU8; 8]);
#[repr(C, align(8))]
struct Halves([AtomicU16; 4]);
static B8: Bytes = Bytes([const { AtomicU8::new(0) }; 8]);
static H16: Halves = Halves([const { AtomicU16::new(0) }; 4]);
static CAS16: AtomicU16 = AtomicU16::new(0);
static A32: AtomicU32 = AtomicU32::new(0);
static A64: AtomicU64 = AtomicU64::new(0);

#[pin_data]
struct Ctx {
    #[pin]
    sl: SpinLock<u64>,
    #[pin]
    mx: Mutex<u64>,
    k64: Atomic<i64>,
}

unsafe extern "C" fn hammer(ctx: *mut c_void, idx: c_int, iters: u64) -> u64 {
    // SAFETY: abi_c_run_threads passes the pinned Ctx of atomics(), which
    // outlives every thread (it waits for all of them before returning).
    let ctx = unsafe { &*ctx.cast::<Ctx>() };
    let s = idx as usize;
    for it in 0..iters {
        B8.0[s & 3].fetch_add(1, O::Relaxed);
        B8.0[4].fetch_add(1, O::SeqCst);
        H16.0[s & 1].fetch_add(1, O::Relaxed);
        let mut v = CAS16.load(O::Relaxed);
        while let Err(now) = CAS16.compare_exchange_weak(v, v.wrapping_add(1), O::AcqRel, O::Relaxed) {
            v = now;
        }
        A32.fetch_add(1, O::Relaxed);
        A64.fetch_add(1, O::SeqCst);
        ctx.k64.fetch_add(1i64, Full);
        *ctx.sl.lock() += 1;
        if it % 64 == 0 {
            *ctx.mx.lock() += 1;
        }
    }
    iters
}

fn atomics(t: &mut Tally) -> Result {
    const N: u64 = 20000;
    for b in &B8.0[..5] { b.store(0, O::Relaxed); }
    for b in &B8.0[5..] { b.store(0x5a, O::Relaxed); }
    for h in &H16.0[..2] { h.store(0, O::Relaxed); }
    for h in &H16.0[2..] { h.store(0xa5a5, O::Relaxed); }
    CAS16.store(0, O::Relaxed);
    A32.store(0, O::Relaxed);
    A64.store(0, O::Relaxed);
    let ctx = KBox::pin_init(
        pin_init!(Ctx { sl <- new_spinlock!(0u64), mx <- new_mutex!(0u64), k64: Atomic::new(0i64) }),
        GFP_KERNEL,
    )?;
    let p = core::ptr::from_ref::<Ctx>(&ctx).cast_mut().cast::<c_void>();
    // SAFETY: hammer has the prototype abi_c_run_threads calls; ctx stays
    // alive and pinned until after the call, which waits for all threads.
    let n = unsafe { abi_c_run_threads(p, hammer, N) };
    if n < 1 {
        t.check(false, format_args!("abi_c_run_threads: {}", n));
        return Ok(());
    }
    let n = n as u64;
    pr_info!("ppc64abi: hammer ran on {} CPUs x {} iterations\n", n, N);
    let per = |m: u64, s: u64| (0..n).filter(|i| i % m == s).count() as u64 * N;
    for s in 0..4u64 {
        let got = u64::from(B8.0[s as usize].load(O::Relaxed));
        t.check(got == per(4, s) % 256, format_args!("AtomicU8[{}] = {} want {}", s, got, per(4, s) % 256));
    }
    let got = u64::from(B8.0[4].load(O::Relaxed));
    t.check(got == (n * N) % 256, format_args!("shared AtomicU8 = {}", got));
    for (i, b) in B8.0[5..].iter().enumerate() {
        t.check(b.load(O::Relaxed) == 0x5a, format_args!("AtomicU8 canary {} clobbered", i + 5));
    }
    for s in 0..2u64 {
        let got = u64::from(H16.0[s as usize].load(O::Relaxed));
        t.check(got == per(2, s) % 65536, format_args!("AtomicU16[{}] = {}", s, got));
    }
    for (i, h) in H16.0[2..].iter().enumerate() {
        t.check(h.load(O::Relaxed) == 0xa5a5, format_args!("AtomicU16 canary {} clobbered", i + 2));
    }
    t.check(u64::from(CAS16.load(O::Relaxed)) == (n * N) % 65536, format_args!("AtomicU16 CAS loop"));
    t.check(u64::from(A32.load(O::Relaxed)) == (n * N) % (1 << 32), format_args!("AtomicU32"));
    t.check(A64.load(O::Relaxed) == n * N, format_args!("AtomicU64"));
    t.check(ctx.k64.load(kernel::sync::atomic::ordering::Relaxed) == (n * N) as i64, format_args!("kernel Atomic<i64>"));
    t.check(*ctx.sl.lock() == n * N, format_args!("SpinLock counter"));
    t.check(*ctx.mx.lock() == n * N.div_ceil(64), format_args!("Mutex counter"));
    Ok(())
}

fn alloc(t: &mut Tally) -> Result {
    let mut v = KVec::<u64>::new();
    for i in 0..50_000u64 {
        v.push(i, GFP_KERNEL)?;
    }
    t.check(v.iter().sum::<u64>() == 50_000 * 49_999 / 2, format_args!("KVec<u64> sum"));
    let mut big = VVec::<u8>::with_capacity(4 << 20, GFP_KERNEL)?;
    big.extend_with(4 << 20, 0xa5, GFP_KERNEL)?;
    t.check(big.len() == 4 << 20 && big.iter().all(|&b| b == 0xa5), format_args!("VVec<u8> 4 MiB"));
    let q = KBox::new(abi_gen::SQ { q: u128::MAX - 7 }, GFP_KERNEL)?;
    t.check(core::ptr::from_ref(&*q).addr() % 16 == 0 && { q.q } == u128::MAX - 7, format_args!("KBox<u128 struct> 16-aligned"));
    let a = KBox::new(abi_gen::SA16 { a: 5 }, GFP_KERNEL)?;
    t.check(core::ptr::from_ref(&*a).addr() % 16 == 0 && a.a == 5, format_args!("KBox<align(16)> aligned"));
    let mut qm = KVec::<abi_gen::SQM>::with_capacity(3, GFP_KERNEL)?;
    for i in 0..3u8 {
        qm.push(abi_gen::SQM { a: i, q: u128::from(i), c: 7 }, GFP_KERNEL)?;
    }
    t.check(qm.as_ptr().addr() % 16 == 0 && qm.iter().map(|x| x.q).sum::<u128>() == 3, format_args!("KVec<u128 struct> aligned"));
    Ok(())
}

struct Abi;

impl kernel::Module for Abi {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        let mut t = Tally { pass: 0, fail: 0 };
        pr_info!("ppc64abi: start ({} structs)\n", abi_gen::STRUCTS);
        t.count(abi_gen::check_layout(), 1);
        scalars(&mut t);
        t.count(abi_gen::check_c(), abi_gen::CALLS as u32);
        // SAFETY: C runs the C->Rust direction of the generated calls.
        t.count(unsafe { abi_c_check_rust_structs() } as u32, 1);
        atomics(&mut t)?;
        alloc(&mut t)?;
        pr_info!("ppc64abi: fmt {} {} {:#x} {:?} {}\n", 255u8, -1i64, 0xdead_beef_u64, Some(3u16), "end");
        let line = line!() + 1;
        kernel::warn_on!(black_box(true));
        pr_info!("ppc64abi: survived warn_on at abi_main.rs:{}\n", line);
        pr_info!("ppc64abi: RESULT pass={} fail={}\n", t.pass, t.fail);
        Ok(Abi)
    }
}

impl Drop for Abi {
    fn drop(&mut self) {
        pr_info!("ppc64abi: unloaded\n");
    }
}

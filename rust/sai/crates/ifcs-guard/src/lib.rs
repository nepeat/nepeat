//! The learn-ring read guard of `libsai-teralynx-shim.so`, as safe Rust: the decisions of
//! `ai_re/nix/packages/sai-teralynx/src/ifcs_guard.c` (why: `ai_re/devices/cs8210-32x/learn-wedge.md`
//! sections 1-2), as a state machine over injected I/O ([`Io`]: the original register accessors, the
//! clocks, the 3 ms sleep, the journal, the stats and inject files). The C ABI around it (the GOT
//! targets libifcs calls) is crate `ifcs-shim`.
//!
//! libifcs.so 0.12.41's learn thread polls, about once a second through `node_pci_reg_read32`,
//! 0x4864 and 0x485c (read, then written back with its consumer state) and 0x4860, the learn ring's
//! producer index B (bits 0..11 slot, bit 12 phase). It trusts B: an all-ones read (a `PCIe` completion
//! timeout) becomes a 32 GiB `im_malloc` and a walk that never ends. The guard:
//!
//! * checks reads of 0x4860 (bits 13..31 clear and slot < ring size) and of 0x4864 / 0x485c (not
//!   all-ones); an invalid value is re-read up to 3 times, 3 ms apart; still invalid, 0x4860 answers
//!   the consumer index last written to 0x4864 ("nothing new": the poll is skipped), the
//!   read-modify-write registers their last valid value (0 before any), and that register is not
//!   re-read again until a read is valid (the thread's own 1 Hz read is the probe). Such a read
//!   always answers success;
//! * counts all-ones from any other offset (the first 16 offsets by name, one journal line each),
//!   changing nothing;
//! * remembers writes of 0x4864 (bits 0..12: the consumer index);
//! * refuses `im_malloc` above a limit (default 1 GiB, 0 = none): NULL instead of malloc + memset;
//! * logs (`sai-teralynx: ifcs guard: ...`, at most 20 lines per 10 minutes, formatted under the lock
//!   and written after it) and keeps counters in a Prometheus file rewritten on every event and every
//!   30 s while the thread polls (`sai_teralynx_ifcs_guard_*`);
//! * test hook: with `SAI_TERALYNX_IFCS_GUARD_INJECT` naming a file at init, that file
//!   ("<reg> <count>", read at most once a second, then removed) makes the next reads of that
//!   register all-ones.
//!
//! Equal to the C in every observable (returned values and codes, re-reads, sleeps, journal text,
//! the file's content and when it is written; `ifcs-shim`'s differential runs both): the same
//! fields under the same lock, the same lock-free fields (atomics where the C used `__atomic`), the
//! same buffer sizes and `snprintf` truncation, the same rate limit. One deliberate difference: the
//! counters' text buffer holds the worst case ([`TEXT_CAP`]; the C's 4096 bytes stop the file's
//! rewrites once the 16 all-ones names count in the hundreds). The C's bookkeeping quirks are
//! kept (they are what the deployed guard does): `tl_ifcs_guard_init` resets the counters but not
//! `installed`, the rate limiter, the stats timer or the inject timer; re-reads are not counted in
//! `reads_total`. No allocation, and a valid read of a guarded register takes no lock (unless the
//! inject hook is on, the register was degraded, or the 30 s stats rewrite is due), as in the C.

#![forbid(unsafe_code)]

pub mod fs;
mod scan;
mod text;

use std::fmt;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64};
use std::sync::{Mutex, MutexGuard, PoisonError};

pub use scan::{Scan, scan_inject};
use text::{CPath, PENDING, Pending, Ptr, Snprintf};

/// What an unanswered `PCIe` read returns.
pub const ALL_ONES: u32 = u32::MAX;
/// Learn ring size when `tl_ifcs_guard_init` gets 0 (and before init).
pub const DEFAULT_RING_SIZE: u32 = 0x800;
/// `im_malloc` limit before init (init sets it; 0 = none).
pub const DEFAULT_MALLOC_LIMIT: u64 = 1 << 30;
/// Re-reads of an invalid value.
pub const RETRIES: u32 = 3;
/// Milliseconds before each re-read.
pub const RETRY_SLEEP_MS: u32 = 3;
/// The test hook's environment variable (read by the C ABI's init).
pub const INJECT_ENV: &str = "SAI_TERALYNX_IFCS_GUARD_INJECT";
/// Size of the counters' text buffer: the worst case fits (every counter at `u64::MAX`, all 16
/// all-ones offsets named, `installed` at `i32::MIN`, both timestamps at -2^63 s: test
/// `the_worst_case_counters_fit_the_buffer`), so the text is always written. A deliberate
/// difference from the C (coordinator decision, 2026-11-26): its `render` buffer is
/// `char b[4096]` ([`C_TEXT_CAP`]), which three-digit counts with the 16 names in use overflow,
/// and the C then stops rewriting the file and `tl_ifcs_guard_text` answers "" (exactly in the
/// dead-chip case the file is for). The check below stays (a text this long or longer is not
/// written), it cannot trigger.
pub const TEXT_CAP: usize = 5120;
/// The C's buffer (`ifcs_guard.c` `render`): the one place the Rust guard answers differently
/// (`ifcs-shim`'s differential allows exactly texts at least this long).
pub const C_TEXT_CAP: usize = 4096;

/// Other offsets counted by name.
const NALL: usize = 16;
/// `char stats[512]`, `char inject[512]`, `char tmp[600]`.
const PATH_CAP: usize = 512;
const TMP_CAP: usize = 600;
/// The callers' `out` buffers for the pending journal text.
const OUT_READ: usize = 2048;
const OUT_ALLONES: usize = 512;
const OUT_MALLOC: usize = 1024;
/// Journal: at most `LOG_LINES` lines per `LOG_WINDOW_S`.
const LOG_WINDOW_S: f64 = 600.0;
const LOG_LINES: i32 = 20;
/// Stats rewrite while polling, inject file check.
const STATS_PERIOD_S: f64 = 30.0;
const INJECT_PERIOD_S: f64 = 1.0;
/// Bits of 0x4864 that are the consumer index; bits of 0x4860 that may be set; slot bits.
const CIDX_MASK: u32 = 0x1fff;
const SLOT_MASK: u32 = 0xfff;
/// 0x4860 values below this on startup, invalid only through the ring size.
const STARTUP_LIMIT: u32 = 0x2000;
const STARTUP_LINES_AT: u32 = 10;

/// The guarded registers (`ifcs_guard.c`'s enum, in its order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reg {
    /// 0x4864: read and written back by the thread; its consumer index in bits 0..12.
    R4864,
    /// 0x485c: read and written back by the thread.
    R485c,
    /// 0x4860: the learn ring's producer index.
    R4860,
}

impl Reg {
    /// In the C's order (the order of the counters' lines).
    pub const ALL: [Reg; 3] = [Reg::R4864, Reg::R485c, Reg::R4860];

    /// The register at a BAR offset, if it is guarded.
    pub const fn from_offset(off: i32) -> Option<Reg> {
        match off {
            0x4864 => Some(Reg::R4864),
            0x485c => Some(Reg::R485c),
            0x4860 => Some(Reg::R4860),
            _ => None,
        }
    }

    /// The name in the journal and the counters' label.
    pub const fn name(self) -> &'static str {
        match self {
            Reg::R4864 => "0x4864",
            Reg::R485c => "0x485c",
            Reg::R4860 => "0x4860",
        }
    }

    /// The C's enum value (`inject_reg`).
    const fn code(self) -> i32 {
        match self {
            Reg::R4864 => 0,
            Reg::R485c => 1,
            Reg::R4860 => 2,
        }
    }
}

/// One value per guarded register.
#[derive(Default)]
struct PerReg<T> {
    r4864: T,
    r485c: T,
    r4860: T,
}

impl<T> PerReg<T> {
    #[allow(clippy::similar_names, reason = "named by the register offsets")]
    const fn new(r4864: T, r485c: T, r4860: T) -> Self {
        PerReg { r4864, r485c, r4860 }
    }

    const fn get(&self, r: Reg) -> &T {
        match r {
            Reg::R4864 => &self.r4864,
            Reg::R485c => &self.r485c,
            Reg::R4860 => &self.r4860,
        }
    }

    fn get_mut(&mut self, r: Reg) -> &mut T {
        match r {
            Reg::R4864 => &mut self.r4864,
            Reg::R485c => &mut self.r485c,
            Reg::R4860 => &mut self.r4860,
        }
    }
}

/// What the guard does through the process: implemented by the C ABI (`ifcs-shim`) over libifcs'
/// original functions, libc and `std`, and by tests over scripts.
pub trait Io {
    /// The original `node_pci_reg_read32(node, off, val)`: 0 = ok; on failure `val` is left as it
    /// was. `None` is a null `val` (passed on as the caller gave it).
    fn read(&self, node: u32, off: i32, val: Option<&mut u32>) -> i32;
    /// The original `node_pci_reg_write32(node, off, val)`.
    fn write(&self, node: u32, off: i32, val: u32) -> i32;
    /// `CLOCK_MONOTONIC` in seconds.
    fn mono(&self) -> f64;
    /// `CLOCK_REALTIME` in seconds.
    fn real(&self) -> f64;
    /// Sleeps (`nanosleep`, resumed after a signal).
    fn sleep_ms(&self, ms: u32);
    /// Writes journal text (stderr), in one piece.
    fn log(&self, text: &[u8]);
    /// Writes `data` to `tmp`, then renames it to `path` (or removes `tmp` when writing failed).
    /// Returns false when `tmp` could not be created (nothing else happened). [`fs::write_stats`].
    fn write_stats(&self, tmp: &[u8], path: &[u8], data: &[u8]) -> bool;
    /// Opens the inject file; `None` when it cannot be opened. Else scans it ([`scan_inject`]) and
    /// removes it. [`fs::take_inject`].
    fn take_inject(&self, path: &[u8]) -> Option<Scan>;
}

/// An all-ones offset's counter.
#[derive(Clone, Copy, Default)]
struct AllOnes {
    off: i32,
    n: u64,
}

/// The fields `ifcs_guard.c` keeps under its mutex `mu`.
struct Locked {
    invalid: PerReg<u64>,
    invalid_rc: PerReg<u64>,
    recovered: PerReg<u64>,
    substituted: PerReg<u64>,
    startup_bad: PerReg<u32>,
    allones: [AllOnes; NALL],
    allones_other: u64,
    cidx_written: u32,
    have_cidx: bool,
    malloc_refused: u64,
    malloc_refused_max: u64,
    last_invalid: f64,
    installed: i32,
    next_inject_check: f64,
    log_window: f64,
    log_lines: i32,
    log_suppressed: u32,
    inject_left: u32,
    pending: Pending,
    stats: CPath<PATH_CAP>,
    inject: CPath<PATH_CAP>,
    /// `tl_ifcs_guard_text`'s static buffer (NUL-terminated)
    text: [u8; TEXT_CAP],
}

impl Locked {
    const fn new() -> Self {
        Locked {
            invalid: PerReg::new(0, 0, 0),
            invalid_rc: PerReg::new(0, 0, 0),
            recovered: PerReg::new(0, 0, 0),
            substituted: PerReg::new(0, 0, 0),
            startup_bad: PerReg::new(0, 0, 0),
            allones: [AllOnes { off: 0, n: 0 }; NALL],
            allones_other: 0,
            cidx_written: 0,
            have_cidx: false,
            malloc_refused: 0,
            malloc_refused_max: 0,
            last_invalid: 0.0,
            installed: 0,
            next_inject_check: 0.0,
            log_window: 0.0,
            log_lines: 0,
            log_suppressed: 0,
            inject_left: 0,
            pending: Pending::new(),
            stats: CPath::new(),
            inject: CPath::new(),
            text: [0; TEXT_CAP],
        }
    }

    /// `say_locked`: a journal line into the pending buffer, at most 20 per 10 minutes (the
    /// suppressed ones are counted and reported by the first line of the next window).
    fn say(&mut self, now: f64, args: fmt::Arguments<'_>) {
        if now >= self.log_window {
            if self.log_suppressed != 0 && self.pending.len < PENDING {
                let suppressed = self.log_suppressed;
                let k = self.pending.put(format_args!(
                    "sai-teralynx: ifcs guard: {suppressed} more events not logged\n"
                ));
                self.pending.len = self.pending.len.saturating_add(k);
            }
            self.log_window = now + LOG_WINDOW_S;
            self.log_lines = 0;
            self.log_suppressed = 0;
        }
        self.log_lines = self.log_lines.wrapping_add(1);
        if self.log_lines > LOG_LINES {
            self.log_suppressed = self.log_suppressed.wrapping_add(1);
            return;
        }
        if self.pending.len >= PENDING - 1 {
            return;
        }
        let k = self.pending.put(format_args!("sai-teralynx: ifcs guard: "));
        self.pending.len = self.pending.len.saturating_add(k);
        if self.pending.len < PENDING {
            let k = self.pending.put(args);
            self.pending.len = self.pending.len.saturating_add(k);
        }
        self.pending.end_line();
    }
}

/// The guard's state (one per process in the C ABI: a `static`).
pub struct Guard {
    reads: PerReg<AtomicU64>,
    good: PerReg<AtomicU32>,
    have_good: PerReg<AtomicBool>,
    /// last poll was substituted: no re-reads until a valid read
    degraded: PerReg<AtomicBool>,
    /// `Reg::code` of the injected register, -1: none
    inject_reg: AtomicI32,
    /// `next_write_a`: `f64` bits, read outside the lock
    next_write: AtomicU64,
    ring_size: AtomicU32,
    malloc_limit: AtomicU64,
    /// `G.stats[0] != 0`, `G.inject[0] != 0` (read outside the lock)
    has_stats: AtomicBool,
    has_inject: AtomicBool,
    locked: Mutex<Locked>,
}

impl Default for Guard {
    fn default() -> Self {
        Guard::new()
    }
}

/// The journal text a call collected under the lock, written after it is released.
struct Out {
    buf: [u8; PENDING],
    len: usize,
}

impl Out {
    const fn new() -> Self {
        Out {
            buf: [0; PENDING],
            len: 0,
        }
    }

    /// `take_pending_locked(out, cap)`.
    fn take(&mut self, l: &mut Locked, cap: usize) {
        self.len = l.pending.take(cap, &mut self.buf).len();
    }

    /// `print_out`: nothing when empty.
    fn print(&self, io: &impl Io) {
        if let Some(text) = self.buf.get(..self.len)
            && !text.is_empty()
        {
            io.log(text);
        }
    }
}

impl Guard {
    /// The state before `tl_ifcs_guard_init` (the C's static initialisers).
    pub const fn new() -> Self {
        Guard {
            reads: PerReg::new(AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)),
            good: PerReg::new(AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)),
            have_good: PerReg::new(AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)),
            degraded: PerReg::new(AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)),
            inject_reg: AtomicI32::new(-1),
            next_write: AtomicU64::new(0),
            ring_size: AtomicU32::new(DEFAULT_RING_SIZE),
            malloc_limit: AtomicU64::new(DEFAULT_MALLOC_LIMIT),
            has_stats: AtomicBool::new(false),
            has_inject: AtomicBool::new(false),
            locked: Mutex::new(Locked::new()),
        }
    }

    /// The lock; a panic elsewhere (none is expected) does not stop the guard.
    fn lock(&self) -> MutexGuard<'_, Locked> {
        self.locked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `tl_ifcs_guard_init` after its null checks: `stats` (the counters' file, empty = none),
    /// `ring_size` (0 = 0x800), `malloc_limit` (bytes, 0 = none), `inject` (the value of
    /// [`INJECT_ENV`], empty = no hook). Resets the counters; keeps `installed`, the rate limiter
    /// and the timers (as the C).
    pub fn init(&self, stats: &[u8], ring_size: u32, malloc_limit: u64, inject: &[u8]) {
        let mut l = self.lock();
        self.ring_size
            .store(if ring_size == 0 { DEFAULT_RING_SIZE } else { ring_size }, Relaxed);
        self.malloc_limit.store(malloc_limit, Relaxed);
        l.stats.set(stats);
        self.has_stats.store(!l.stats.is_empty(), Relaxed);
        l.inject.set(inject);
        self.has_inject.store(!l.inject.is_empty(), Relaxed);
        for r in Reg::ALL {
            self.reads.get(r).store(0, Relaxed);
            self.good.get(r).store(0, Relaxed);
            self.have_good.get(r).store(false, Relaxed);
            self.degraded.get(r).store(false, Relaxed);
            *l.invalid.get_mut(r) = 0;
            *l.invalid_rc.get_mut(r) = 0;
            *l.recovered.get_mut(r) = 0;
            *l.substituted.get_mut(r) = 0;
            *l.startup_bad.get_mut(r) = 0;
        }
        l.allones = [AllOnes::default(); NALL];
        l.allones_other = 0;
        l.have_cidx = false;
        l.malloc_refused = 0;
        l.malloc_refused_max = 0;
        l.last_invalid = 0.0;
        self.inject_reg.store(-1, Relaxed);
        l.inject_left = 0;
    }

    /// `tl_ifcs_guard_installed(on)`: the gauge, and the file rewritten.
    pub fn installed(&self, io: &impl Io, on: i32) {
        let mut l = self.lock();
        l.installed = on;
        self.write_stats_locked(io, &mut l);
    }

    /// `tl_ifcs_guard_text`: renders the counters into the guard's text buffer (`""` when they do
    /// not fit) and returns its address: a NUL-terminated string that stays where it is while the
    /// guard exists and is rewritten by the next call (the C's static buffer).
    pub fn text_ptr(&self, io: &impl Io) -> *const u8 {
        let mut l = self.lock();
        self.render_text(io, &mut l);
        l.text.as_ptr()
    }

    /// The same text as bytes (tests).
    pub fn text(&self, io: &impl Io) -> Vec<u8> {
        let mut l = self.lock();
        self.render_text(io, &mut l);
        let end = l.text.iter().position(|&c| c == 0).unwrap_or(TEXT_CAP);
        l.text.get(..end).unwrap_or(&[]).to_vec()
    }

    fn render_text(&self, io: &impl Io, l: &mut Locked) {
        let real = io.real();
        let mut text = [0_u8; TEXT_CAP];
        let n = Snprintf::print(&mut text, format_args!("{}", Render { g: self, l, real }));
        if n >= TEXT_CAP
            && let Some(c) = text.first_mut()
        {
            *c = 0;
        }
        l.text = text;
    }

    fn valid(&self, r: Reg, v: u32) -> bool {
        match r {
            Reg::R4860 => (v & !CIDX_MASK) == 0 && (v & SLOT_MASK) < self.ring_size.load(Relaxed),
            Reg::R4864 | Reg::R485c => v != ALL_ONES,
        }
    }

    /// `write_stats_locked`: the counters to `<stats>.tmp`, renamed over `<stats>`; the next
    /// periodic rewrite 30 s later (only when the file could be created).
    fn write_stats_locked(&self, io: &impl Io, l: &mut Locked) {
        if l.stats.is_empty() {
            return;
        }
        let real = io.real();
        let mut b = [0_u8; TEXT_CAP];
        let n = Snprintf::print(&mut b, format_args!("{}", Render { g: self, l, real }));
        let Some(data) = b.get(..n).filter(|_| n < TEXT_CAP) else {
            return;
        };
        // snprintf(tmp, sizeof(tmp), "%s.tmp", G.stats)
        let mut tmp = CPath::<TMP_CAP>::new();
        tmp.set_joined(l.stats.bytes(), b".tmp");
        if !io.write_stats(tmp.bytes(), l.stats.bytes(), data) {
            return;
        }
        self.next_write.store((io.mono() + STATS_PERIOD_S).to_bits(), Relaxed);
    }

    /// `check_inject_locked`: at most once a second, the inject file arms the hook.
    fn check_inject_locked(&self, io: &impl Io, l: &mut Locked) {
        let t = io.mono();
        if t < l.next_inject_check {
            return;
        }
        l.next_inject_check = t + INJECT_PERIOD_S;
        let Some(scan) = io.take_inject(l.inject.bytes()) else {
            return;
        };
        #[allow(clippy::cast_possible_wrap, reason = "the C's (int32_t) cast of the scanned offset")]
        let reg = Reg::from_offset(scan.off as i32);
        if let (2, Some(r)) = (scan.got, reg) {
            l.inject_left = scan.cnt;
            self.inject_reg.store(r.code(), Relaxed);
            let now = io.mono();
            l.say(
                now,
                format_args!(
                    "TEST: next {} reads of {} return 0xffffffff (injected)",
                    scan.cnt,
                    r.name()
                ),
            );
        }
    }

    /// `raw_read`: the original read, then the hook.
    fn raw_read(&self, io: &impl Io, r: Reg, node: u32, off: i32, val: &mut u32) -> i32 {
        let mut rc = io.read(node, off, Some(val));
        if self.inject_reg.load(Relaxed) == r.code() {
            let mut l = self.lock();
            if self.inject_reg.load(Relaxed) == r.code() && l.inject_left > 0 {
                l.inject_left -= 1;
                *val = ALL_ONES;
                rc = 0;
            }
        }
        rc
    }

    /// `count_allones`: an all-ones read of an offset that is not guarded.
    fn count_allones(&self, io: &impl Io, off: i32) {
        let mut out = Out::new();
        {
            let mut l = self.lock();
            let now_needed = l.allones.iter().position(|a| a.n == 0 || a.off == off);
            match now_needed.and_then(|i| l.allones.get(i).copied().map(|a| (i, a))) {
                Some((i, a)) => {
                    if a.n == 0 {
                        let now = io.mono();
                        #[allow(clippy::cast_sign_loss, reason = "the C prints (unsigned)off")]
                        let shown = off as u32;
                        l.say(
                            now,
                            format_args!("register 0x{shown:x} read 0xffffffff (first time; counted, not changed)"),
                        );
                    }
                    if let Some(slot) = l.allones.get_mut(i) {
                        slot.off = off;
                        slot.n = slot.n.wrapping_add(1);
                    }
                }
                None => l.allones_other = l.allones_other.wrapping_add(1),
            }
            out.take(&mut l, OUT_ALLONES);
        }
        out.print(io);
    }

    /// `tl_ifcs_reg_read32`: libifcs' `node_pci_reg_read32`, guarded. `val`: `None` for a null
    /// pointer (passed on unchanged, unguarded).
    pub fn reg_read32(&self, io: &impl Io, node: u32, off: i32, val: Option<&mut u32>) -> i32 {
        match (Reg::from_offset(off), val) {
            (Some(r), Some(val)) => {
                let mut v = *val;
                let rc = self.guarded_read(io, r, node, off, &mut v);
                *val = v;
                rc
            }
            (None, Some(val)) => {
                let rc = io.read(node, off, Some(&mut *val));
                if rc == 0 && *val == ALL_ONES {
                    self.count_allones(io, off);
                }
                rc
            }
            (_, None) => io.read(node, off, None),
        }
    }

    #[allow(clippy::too_many_lines, reason = "one function in the C, kept in its order")]
    fn guarded_read(&self, io: &impl Io, r: Reg, node: u32, off: i32, val: &mut u32) -> i32 {
        let mut out = Out::new();
        if self.has_inject.load(Relaxed) {
            {
                let mut l = self.lock();
                self.check_inject_locked(io, &mut l);
                out.take(&mut l, OUT_READ);
            }
            out.print(io);
        }
        // node_pci_reg_read32 leaves it untouched on failure; the thread ignores rc
        *val = ALL_ONES;
        let mut rc = self.raw_read(io, r, node, off, val);
        self.reads.get(r).fetch_add(1, Relaxed);
        if rc == 0 && self.valid(r, *val) {
            self.good.get(r).store(*val, Relaxed);
            self.have_good.get(r).store(true, Relaxed);
            if self.degraded.get(r).load(Relaxed) {
                {
                    let mut l = self.lock();
                    self.degraded.get(r).store(false, Relaxed);
                    let (v, substituted, now) = (*val, *l.substituted.get(r), io.mono());
                    l.say(
                        now,
                        format_args!(
                            "{} valid again (0x{v:08x}) after {substituted} substituted polls",
                            r.name()
                        ),
                    );
                    self.write_stats_locked(io, &mut l);
                    out.take(&mut l, OUT_READ);
                }
                out.print(io);
                return 0;
            }
            let due = |g: &Guard| io.mono() >= f64::from_bits(g.next_write.load(Relaxed));
            if self.has_stats.load(Relaxed) && due(self) {
                let mut l = self.lock();
                if due(self) {
                    self.write_stats_locked(io, &mut l);
                }
            }
            return 0;
        }
        let first = *val;
        let first_rc = rc;
        let mut tries = 0;
        let mut ok = false;
        // Re-read only while not degraded: a register that stayed bad last poll is probed by
        // the thread's own next read, not hammered (each unanswered read can stall the SoC).
        if !self.degraded.get(r).load(Relaxed) {
            for t in 1..=RETRIES {
                tries = t;
                io.sleep_ms(RETRY_SLEEP_MS);
                *val = ALL_ONES;
                rc = self.raw_read(io, r, node, off, val);
                if rc == 0 && self.valid(r, *val) {
                    ok = true;
                    break;
                }
            }
            if !ok {
                tries = RETRIES;
            }
        }
        {
            let mut l = self.lock();
            let inv = l.invalid.get_mut(r);
            *inv = inv.wrapping_add(1);
            if first_rc != 0 {
                let inv_rc = l.invalid_rc.get_mut(r);
                *inv_rc = inv_rc.wrapping_add(1);
            }
            l.last_invalid = io.real();
            if ok {
                let rec = l.recovered.get_mut(r);
                *rec = rec.wrapping_add(1);
                self.good.get(r).store(*val, Relaxed);
                self.have_good.get(r).store(true, Relaxed);
                let (v, invalid, recovered, substituted, now) = (
                    *val,
                    *l.invalid.get(r),
                    *l.recovered.get(r),
                    *l.substituted.get(r),
                    io.mono(),
                );
                l.say(
                    now,
                    format_args!(
                        "{} read 0x{first:08x} (rc {first_rc}), re-read {tries} gave 0x{v:08x}: recovered (invalid \
                         {invalid}, recovered {recovered}, substituted {substituted})",
                        r.name()
                    ),
                );
            } else {
                let was_degraded = self.degraded.get(r).load(Relaxed);
                let sub = l.substituted.get_mut(r);
                *sub = sub.wrapping_add(1);
                self.degraded.get(r).store(true, Relaxed);
                let from = if r == Reg::R4860 && l.have_cidx {
                    *val = l.cidx_written;
                    "the consumer index last written to 0x4864"
                } else if self.have_good.get(r).load(Relaxed) {
                    *val = self.good.get(r).load(Relaxed);
                    "the last valid value"
                } else {
                    *val = 0;
                    "0 (no valid value yet)"
                };
                rc = 0;
                if r == Reg::R4860 && !self.have_good.get(r).load(Relaxed) && first_rc == 0 && first < STARTUP_LIMIT {
                    let bad = l.startup_bad.get_mut(r);
                    *bad = bad.wrapping_add(1);
                    if *bad == STARTUP_LINES_AT {
                        let (ring, now) = (self.ring_size.load(Relaxed), io.mono());
                        l.say(
                            now,
                            format_args!(
                                "0x4860 invalid on the first 10 reads with indexes below 0x2000 (e.g. 0x{first:08x}): \
                                 is the learn ring size (SAI_TERALYNX_LEARN_RING_SIZE = {ring}) wrong? learning is \
                                 stopped"
                            ),
                        );
                    }
                }
                if !was_degraded {
                    let (v, invalid, recovered, substituted, now) = (
                        *val,
                        *l.invalid.get(r),
                        *l.recovered.get(r),
                        *l.substituted.get(r),
                        io.mono(),
                    );
                    let what = if r == Reg::R4860 {
                        "learn poll skipped"
                    } else {
                        "write-back keeps its bits"
                    };
                    l.say(
                        now,
                        format_args!(
                            "{} read 0x{first:08x} (rc {first_rc}), still invalid after {tries} re-reads: {what}, \
                             returned 0x{v:08x} = {from}; no re-reads until it is valid again (invalid {invalid}, \
                             recovered {recovered}, substituted {substituted})",
                            r.name()
                        ),
                    );
                }
            }
            self.write_stats_locked(io, &mut l);
            out.take(&mut l, OUT_READ);
        }
        out.print(io);
        rc
    }

    /// `tl_ifcs_reg_write32`: libifcs' `node_pci_reg_write32`; a write of 0x4864 is remembered.
    pub fn reg_write32(&self, io: &impl Io, node: u32, off: i32, val: u32) -> i32 {
        if off == 0x4864 {
            let mut l = self.lock();
            l.cidx_written = val & CIDX_MASK;
            l.have_cidx = true;
        }
        io.write(node, off, val)
    }

    /// `tl_ifcs_im_malloc`'s decision: true = refuse `n` bytes (counted, logged with the
    /// caller's address `caller`, 0 = unknown, the file rewritten); false = allocate (`calloc`).
    pub fn im_malloc_refused(&self, io: &impl Io, n: usize, caller: usize) -> bool {
        let limit = self.malloc_limit.load(Relaxed);
        let bytes = u64::try_from(n).unwrap_or(u64::MAX);
        if limit == 0 || bytes <= limit {
            return false;
        }
        let mut out = Out::new();
        {
            let mut l = self.lock();
            l.malloc_refused = l.malloc_refused.wrapping_add(1);
            if bytes > l.malloc_refused_max {
                l.malloc_refused_max = bytes;
            }
            let now = io.mono();
            l.say(
                now,
                format_args!(
                    "im_malloc({n}) refused (limit {limit} bytes; caller {}): returning NULL",
                    Ptr(caller)
                ),
            );
            self.write_stats_locked(io, &mut l);
            out.take(&mut l, OUT_MALLOC);
        }
        out.print(io);
        true
    }
}

/// `render`: the counters as Prometheus text.
struct Render<'a> {
    g: &'a Guard,
    l: &'a Locked,
    real: f64,
}

/// The five per-register families: name, help.
const FAMILIES: [(&str, &str); 5] = [
    ("reads", "learn-ring register reads by libifcs"),
    (
        "invalid",
        "reads that returned an invalid value (all-ones / out of range: a PCIe read the ASIC did not answer)",
    ),
    (
        "invalid_rc",
        "invalid reads where node_pci_reg_read32 itself returned an error (an SDK path, not the link)",
    ),
    ("recovered", "invalid reads where a re-read gave a valid value"),
    (
        "substituted",
        "invalid reads still invalid after the re-reads (or while degraded): that learn poll was skipped",
    ),
];

impl fmt::Display for Render<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const P: &str = "sai_teralynx_ifcs_guard_";
        let (g, l) = (self.g, self.l);
        write!(
            f,
            "# HELP {P}installed libifcs' learn-ring register reads go through the guard (1) or not (0)\n\
             # TYPE {P}installed gauge\n{P}installed {}\n",
            l.installed
        )?;
        for (w, (what, help)) in FAMILIES.iter().enumerate() {
            write!(f, "# HELP {P}{what}_total {help}\n# TYPE {P}{what}_total counter\n")?;
            for r in Reg::ALL {
                let v = match w {
                    0 => g.reads.get(r).load(Relaxed),
                    1 => *l.invalid.get(r),
                    2 => *l.invalid_rc.get(r),
                    3 => *l.recovered.get(r),
                    _ => *l.substituted.get(r),
                };
                writeln!(f, "{P}{what}_total{{reg=\"{}\"}} {v}", r.name())?;
            }
        }
        write!(
            f,
            "# HELP {P}allones_reads_total libifcs register reads (any offset) that returned 0xffffffff\n\
             # TYPE {P}allones_reads_total counter\n"
        )?;
        for a in l.allones.iter().take_while(|a| a.n != 0) {
            #[allow(clippy::cast_sign_loss, reason = "the C prints (unsigned)off")]
            let off = a.off as u32;
            writeln!(f, "{P}allones_reads_total{{reg=\"0x{off:x}\"}} {}", a.n)?;
        }
        writeln!(f, "{P}allones_reads_total{{reg=\"other\"}} {}", l.allones_other)?;
        write!(
            f,
            "# HELP {P}last_invalid_timestamp_seconds time of the last invalid read (0: none)\n\
             # TYPE {P}last_invalid_timestamp_seconds gauge\n{P}last_invalid_timestamp_seconds {:.3}\n",
            l.last_invalid
        )?;
        write!(
            f,
            "# HELP {P}im_malloc_refused_total im_malloc requests above the limit, refused\n\
             # TYPE {P}im_malloc_refused_total counter\n{P}im_malloc_refused_total {}\n",
            l.malloc_refused
        )?;
        write!(
            f,
            "# HELP {P}im_malloc_refused_max_bytes largest refused im_malloc request\n\
             # TYPE {P}im_malloc_refused_max_bytes gauge\n{P}im_malloc_refused_max_bytes {}\n",
            l.malloc_refused_max
        )?;
        write!(
            f,
            "# HELP {P}updated_timestamp_seconds when the guard last wrote this file\n\
             # TYPE {P}updated_timestamp_seconds gauge\n{P}updated_timestamp_seconds {:.3}\n",
            self.real
        )
    }
}

#[cfg(test)]
mod tests;

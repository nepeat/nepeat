//! The guard's rules one by one over a scripted [`Io`], the C test's learn-thread scenario
//! (`t_learn.cpp`) on a Rust replay of the mock's learn pass, and property tests.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::similar_names,
    clippy::unnecessary_wraps,
    clippy::assert_is_empty,
    reason = "tests"
)]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use proptest::prelude::*;

use super::{ALL_ONES, C_TEXT_CAP, Guard, Io, Scan, TEXT_CAP, scan_inject};

/// Registers with scripted answers, a settable clock, and everything the guard did.
#[derive(Default)]
struct Fake {
    regs: RefCell<HashMap<i32, u32>>,
    answers: RefCell<HashMap<i32, VecDeque<(i32, u32)>>>,
    reads: RefCell<Vec<i32>>,
    writes: RefCell<Vec<(i32, u32)>>,
    mono: Cell<f64>,
    real: Cell<f64>,
    sleeps: RefCell<Vec<u32>>,
    log: RefCell<String>,
    files: RefCell<HashMap<Vec<u8>, Vec<u8>>>,
    stats_writes: Cell<usize>,
    stats_fail: Cell<bool>,
    inject: RefCell<Option<String>>,
    inject_opens: Cell<usize>,
}

impl Fake {
    fn new() -> Self {
        let f = Fake::default();
        f.mono.set(1000.0);
        f.real.set(1_764_000_000.25);
        f
    }
    fn set(&self, off: i32, v: u32) {
        self.regs.borrow_mut().insert(off, v);
    }
    fn answer(&self, off: i32, rc: i32, v: u32) {
        self.answers.borrow_mut().entry(off).or_default().push_back((rc, v));
    }
    fn fail(&self, off: i32, n: usize) {
        for _ in 0..n {
            self.answer(off, 0, ALL_ONES);
        }
    }
    fn advance(&self, s: f64) {
        self.mono.set(self.mono.get() + s);
        self.real.set(self.real.get() + s);
    }
    fn take_log(&self) -> String {
        std::mem::take(&mut *self.log.borrow_mut())
    }
    fn take_reads(&self) -> Vec<i32> {
        std::mem::take(&mut *self.reads.borrow_mut())
    }
    fn take_sleeps(&self) -> Vec<u32> {
        std::mem::take(&mut *self.sleeps.borrow_mut())
    }
    fn prom(&self) -> String {
        String::from_utf8(
            self.files
                .borrow()
                .get(b"/run/g.prom".as_slice())
                .cloned()
                .unwrap_or_default(),
        )
        .unwrap()
    }
}

impl Io for Fake {
    fn read(&self, _node: u32, off: i32, val: Option<&mut u32>) -> i32 {
        self.reads.borrow_mut().push(off);
        let (rc, v) = self
            .answers
            .borrow_mut()
            .get_mut(&off)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| (0, self.regs.borrow().get(&off).copied().unwrap_or(0)));
        if let Some(val) = val
            && rc == 0
        {
            *val = v;
        }
        rc
    }
    fn write(&self, _node: u32, off: i32, val: u32) -> i32 {
        self.writes.borrow_mut().push((off, val));
        self.set(off, val);
        0
    }
    fn mono(&self) -> f64 {
        self.mono.get()
    }
    fn real(&self) -> f64 {
        self.real.get()
    }
    fn sleep_ms(&self, ms: u32) {
        self.sleeps.borrow_mut().push(ms);
        self.advance(f64::from(ms) / 1000.0);
    }
    fn log(&self, text: &[u8]) {
        self.log.borrow_mut().push_str(std::str::from_utf8(text).unwrap());
    }
    fn write_stats(&self, tmp: &[u8], path: &[u8], data: &[u8]) -> bool {
        assert_eq!([path, b".tmp"].concat(), tmp);
        if self.stats_fail.get() {
            return false;
        }
        self.stats_writes.set(self.stats_writes.get() + 1);
        self.files.borrow_mut().insert(path.to_vec(), data.to_vec());
        true
    }
    fn take_inject(&self, path: &[u8]) -> Option<Scan> {
        assert_eq!(path, b"/run/inject");
        self.inject_opens.set(self.inject_opens.get() + 1);
        self.inject.borrow_mut().take().map(|t| scan_inject(t.bytes()))
    }
}

fn guard(io: &Fake, ring: u32) -> Guard {
    let g = Guard::new();
    g.init(b"/run/g.prom", ring, 1 << 30, b"");
    g.installed(io, 1);
    g
}

fn rd(g: &Guard, io: &Fake, off: i32) -> (i32, u32) {
    let mut v = 0x5a5a_5a5a;
    let rc = g.reg_read32(io, 0, off, Some(&mut v));
    (rc, v)
}

fn metric(text: &str, name: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("sai_teralynx_ifcs_guard_{name} ")))
        .map(str::to_string)
}

fn m(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn a_valid_read_passes_untouched_and_takes_no_lock_worth_of_work() {
    let io = Fake::new();
    let g = guard(&io, 0);
    let writes = io.stats_writes.get();
    io.set(0x4860, 0x1005);
    assert_eq!(rd(&g, &io, 0x4860), (0, 0x1005));
    assert_eq!(io.take_reads(), [0x4860]);
    assert!(io.take_sleeps().is_empty());
    assert_eq!(io.take_log(), "");
    assert_eq!(io.stats_writes.get(), writes, "no rewrite before 30 s");
    io.advance(30.0);
    assert_eq!(rd(&g, &io, 0x4860), (0, 0x1005));
    assert_eq!(io.stats_writes.get(), writes + 1, "the periodic rewrite");
    assert_eq!(metric(&io.prom(), "reads_total{reg=\"0x4860\"}"), m("2"));
}

#[test]
fn one_bad_read_is_recovered_by_a_re_read() {
    let io = Fake::new();
    let g = guard(&io, 0);
    io.set(0x4860, 9);
    io.fail(0x4860, 1);
    assert_eq!(rd(&g, &io, 0x4860), (0, 9));
    assert_eq!(io.take_reads(), [0x4860, 0x4860]);
    assert_eq!(io.take_sleeps(), [3]);
    assert_eq!(
        io.take_log(),
        "sai-teralynx: ifcs guard: 0x4860 read 0xffffffff (rc 0), re-read 1 gave 0x00000009: recovered (invalid 1, \
         recovered 1, substituted 0)\n"
    );
    let p = io.prom();
    assert_eq!(metric(&p, "invalid_total{reg=\"0x4860\"}"), m("1"));
    assert_eq!(metric(&p, "recovered_total{reg=\"0x4860\"}"), m("1"));
    assert_eq!(
        metric(&p, "reads_total{reg=\"0x4860\"}"),
        m("1"),
        "re-reads are not counted"
    );
    assert!(p.contains("last_invalid_timestamp_seconds 1764000000.253\n"), "{p}");
}

#[test]
fn a_register_that_stays_bad_is_substituted_and_not_re_read_until_valid() {
    let io = Fake::new();
    let g = guard(&io, 0);
    // the thread's write-back of its consumer index
    assert_eq!(g.reg_write32(&io, 0, 0x4864, 0xa5a0_1003), 0);
    io.set(0x4860, 12);
    io.fail(0x4860, 10);
    assert_eq!(rd(&g, &io, 0x4860), (0, 0x1003), "the consumer index: nothing new");
    assert_eq!(io.take_reads().len(), 4);
    assert_eq!(io.take_sleeps(), [3, 3, 3]);
    assert_eq!(
        io.take_log(),
        "sai-teralynx: ifcs guard: 0x4860 read 0xffffffff (rc 0), still invalid after 3 re-reads: learn poll skipped, \
         returned 0x00001003 = the consumer index last written to 0x4864; no re-reads until it is valid again \
         (invalid 1, recovered 0, substituted 1)\n"
    );
    for _ in 0..6 {
        assert_eq!(rd(&g, &io, 0x4860), (0, 0x1003));
        assert_eq!(io.take_reads().len(), 1, "degraded: the thread's read only");
        assert!(io.take_sleeps().is_empty());
    }
    assert_eq!(io.take_log(), "", "one line per bad stretch");
    assert_eq!(rd(&g, &io, 0x4860), (0, 12));
    assert_eq!(
        io.take_log(),
        "sai-teralynx: ifcs guard: 0x4860 valid again (0x0000000c) after 7 substituted polls\n"
    );
    let p = io.prom();
    assert_eq!(metric(&p, "substituted_total{reg=\"0x4860\"}"), m("7"));
    assert_eq!(metric(&p, "invalid_total{reg=\"0x4860\"}"), m("7"));
    assert_eq!(metric(&p, "reads_total{reg=\"0x4860\"}"), m("8"));
}

#[test]
fn substitutes_in_order_consumer_index_then_last_valid_then_zero() {
    let io = Fake::new();
    let g = guard(&io, 0);
    // no valid value yet, no consumer index: 0
    io.fail(0x4860, 4);
    assert_eq!(rd(&g, &io, 0x4860), (0, 0));
    assert!(io.take_log().contains("returned 0x00000000 = 0 (no valid value yet)"));
    // the last valid value of a read-modify-write register
    io.set(0x485c, 0x0030_0042);
    assert_eq!(rd(&g, &io, 0x485c), (0, 0x0030_0042));
    io.fail(0x485c, 4);
    assert_eq!(rd(&g, &io, 0x485c), (0, 0x0030_0042));
    assert!(io.take_log().contains(
        "0x485c read 0xffffffff (rc 0), still invalid after 3 re-reads: write-back keeps its bits, returned \
         0x00300042 = the last valid value;"
    ));
    // 0x4864 is never substituted by the consumer index (only 0x4860 is)
    g.reg_write32(&io, 0, 0x4864, 0x7);
    io.fail(0x4864, 4);
    assert_eq!(rd(&g, &io, 0x4864), (0, 0));
}

#[test]
fn error_returns_count_as_invalid_by_rc_and_leave_the_value_all_ones() {
    let io = Fake::new();
    let g = guard(&io, 0);
    io.set(0x4864, 0xa5a0_0000);
    io.answer(0x4864, 11, 0);
    assert_eq!(rd(&g, &io, 0x4864), (0, 0xa5a0_0000));
    assert!(
        io.take_log()
            .contains("0x4864 read 0xffffffff (rc 11), re-read 1 gave 0xa5a00000: recovered")
    );
    assert_eq!(metric(&io.prom(), "invalid_rc_total{reg=\"0x4864\"}"), m("1"));
    // an error on every try: substituted, success returned
    for _ in 0..4 {
        io.answer(0x4864, -5, 0);
    }
    assert_eq!(rd(&g, &io, 0x4864), (0, 0xa5a0_0000));
    assert_eq!(metric(&io.prom(), "invalid_rc_total{reg=\"0x4864\"}"), m("2"));
}

#[test]
fn out_of_range_slots_and_high_bits_are_invalid() {
    let io = Fake::new();
    let g = guard(&io, 0x400);
    for bad in [0x400, 0x7ff, 0x1400, 0x2000, 0x8000_0010] {
        io.answer(0x4860, 0, bad);
        io.answer(0x4860, 0, 0x3ff);
        assert_eq!(rd(&g, &io, 0x4860), (0, 0x3ff), "{bad:#x}");
    }
    io.set(0x4860, 0x13ff);
    assert_eq!(rd(&g, &io, 0x4860), (0, 0x13ff), "phase bit + last slot");
}

#[test]
fn a_too_small_ring_is_reported_once_at_the_tenth_startup_read() {
    let io = Fake::new();
    let g = guard(&io, 0x400);
    io.set(0x4860, 0x500);
    for k in 1..=12 {
        assert_eq!(rd(&g, &io, 0x4860), (0, 0));
        let log = io.take_log();
        assert_eq!(
            log.contains("is the learn ring size (SAI_TERALYNX_LEARN_RING_SIZE = 1024) wrong?"),
            k == 10,
            "{k}"
        );
    }
}

#[test]
fn other_offsets_are_counted_sixteen_by_name() {
    let io = Fake::new();
    let g = guard(&io, 0);
    for off in 0..20 {
        io.set(0x100 + off, ALL_ONES);
        assert_eq!(rd(&g, &io, 0x100 + off), (0, ALL_ONES));
        assert_eq!(rd(&g, &io, 0x100 + off), (0, ALL_ONES));
    }
    io.set(0x2000, 7);
    assert_eq!(rd(&g, &io, 0x2000), (0, 7));
    let log = io.take_log();
    assert_eq!(log.lines().count(), 16);
    assert!(
        log.starts_with(
            "sai-teralynx: ifcs guard: register 0x100 read 0xffffffff (first time; counted, not changed)\n"
        )
    );
    assert!(io.take_sleeps().is_empty(), "no retry");
    let t = String::from_utf8(g.text(&io)).unwrap();
    assert_eq!(metric(&t, "allones_reads_total{reg=\"0x10f\"}"), m("2"));
    assert_eq!(metric(&t, "allones_reads_total{reg=\"other\"}"), m("8"));
    // a null value pointer is passed on, nothing counted
    assert_eq!(g.reg_read32(&io, 0, 0x4860, None), 0);
    assert_eq!(*io.take_reads().last().unwrap(), 0x4860);
    assert_eq!(
        metric(&String::from_utf8(g.text(&io)).unwrap(), "reads_total{reg=\"0x4860\"}"),
        m("0")
    );
}

#[test]
fn the_journal_keeps_to_twenty_lines_per_ten_minutes() {
    let io = Fake::new();
    let g = guard(&io, 0);
    for k in 0..25 {
        io.set(0x3000 + k, ALL_ONES);
        rd(&g, &io, 0x3000 + k);
    }
    assert_eq!(io.take_log().lines().count(), 16, "16 names, inside the 20 lines");
    for _ in 0..30 {
        assert!(g.im_malloc_refused(&io, 2 << 30, 0x1234));
    }
    assert_eq!(io.take_log().lines().count(), 4, "lines 17..20 of the window");
    io.advance(599.0);
    assert!(g.im_malloc_refused(&io, 2 << 30, 0));
    assert_eq!(io.take_log(), "");
    io.advance(1.0);
    assert!(g.im_malloc_refused(&io, 2 << 30, 0));
    assert_eq!(
        io.take_log(),
        "sai-teralynx: ifcs guard: 27 more events not logged\nsai-teralynx: ifcs guard: im_malloc(2147483648) \
         refused (limit 1073741824 bytes; caller (nil)): returning NULL\n"
    );
}

#[test]
fn im_malloc_limit() {
    let io = Fake::new();
    let g = guard(&io, 0);
    assert!(!g.im_malloc_refused(&io, 1 << 30, 1));
    assert!(g.im_malloc_refused(&io, (1 << 30) + 1, 0xabc));
    assert!(
        io.take_log()
            .contains("im_malloc(1073741825) refused (limit 1073741824 bytes; caller 0xabc)")
    );
    assert!(g.im_malloc_refused(&io, 0x7_ffff_c008, 1));
    let p = io.prom();
    assert_eq!(metric(&p, "im_malloc_refused_total"), m("2"));
    assert_eq!(metric(&p, "im_malloc_refused_max_bytes"), m("34359721992"));
    g.init(b"/run/g.prom", 0, 0, b"");
    assert!(!g.im_malloc_refused(&io, usize::MAX, 1), "limit 0: none");
}

#[test]
fn the_counters_file() {
    let io = Fake::new();
    let g = guard(&io, 0);
    io.set(0x4860, 1);
    rd(&g, &io, 0x4860);
    io.fail(0x485c, 4);
    rd(&g, &io, 0x485c);
    io.set(0x4321, ALL_ONES);
    rd(&g, &io, 0x4321);
    g.im_malloc_refused(&io, 3 << 30, 1);
    let expect = r#"# HELP sai_teralynx_ifcs_guard_installed libifcs' learn-ring register reads go through the guard (1) or not (0)
# TYPE sai_teralynx_ifcs_guard_installed gauge
sai_teralynx_ifcs_guard_installed 1
# HELP sai_teralynx_ifcs_guard_reads_total learn-ring register reads by libifcs
# TYPE sai_teralynx_ifcs_guard_reads_total counter
sai_teralynx_ifcs_guard_reads_total{reg="0x4864"} 0
sai_teralynx_ifcs_guard_reads_total{reg="0x485c"} 1
sai_teralynx_ifcs_guard_reads_total{reg="0x4860"} 1
# HELP sai_teralynx_ifcs_guard_invalid_total reads that returned an invalid value (all-ones / out of range: a PCIe read the ASIC did not answer)
# TYPE sai_teralynx_ifcs_guard_invalid_total counter
sai_teralynx_ifcs_guard_invalid_total{reg="0x4864"} 0
sai_teralynx_ifcs_guard_invalid_total{reg="0x485c"} 1
sai_teralynx_ifcs_guard_invalid_total{reg="0x4860"} 0
# HELP sai_teralynx_ifcs_guard_invalid_rc_total invalid reads where node_pci_reg_read32 itself returned an error (an SDK path, not the link)
# TYPE sai_teralynx_ifcs_guard_invalid_rc_total counter
sai_teralynx_ifcs_guard_invalid_rc_total{reg="0x4864"} 0
sai_teralynx_ifcs_guard_invalid_rc_total{reg="0x485c"} 0
sai_teralynx_ifcs_guard_invalid_rc_total{reg="0x4860"} 0
# HELP sai_teralynx_ifcs_guard_recovered_total invalid reads where a re-read gave a valid value
# TYPE sai_teralynx_ifcs_guard_recovered_total counter
sai_teralynx_ifcs_guard_recovered_total{reg="0x4864"} 0
sai_teralynx_ifcs_guard_recovered_total{reg="0x485c"} 0
sai_teralynx_ifcs_guard_recovered_total{reg="0x4860"} 0
# HELP sai_teralynx_ifcs_guard_substituted_total invalid reads still invalid after the re-reads (or while degraded): that learn poll was skipped
# TYPE sai_teralynx_ifcs_guard_substituted_total counter
sai_teralynx_ifcs_guard_substituted_total{reg="0x4864"} 0
sai_teralynx_ifcs_guard_substituted_total{reg="0x485c"} 1
sai_teralynx_ifcs_guard_substituted_total{reg="0x4860"} 0
# HELP sai_teralynx_ifcs_guard_allones_reads_total libifcs register reads (any offset) that returned 0xffffffff
# TYPE sai_teralynx_ifcs_guard_allones_reads_total counter
sai_teralynx_ifcs_guard_allones_reads_total{reg="0x4321"} 1
sai_teralynx_ifcs_guard_allones_reads_total{reg="other"} 0
# HELP sai_teralynx_ifcs_guard_last_invalid_timestamp_seconds time of the last invalid read (0: none)
# TYPE sai_teralynx_ifcs_guard_last_invalid_timestamp_seconds gauge
sai_teralynx_ifcs_guard_last_invalid_timestamp_seconds 1764000000.259
# HELP sai_teralynx_ifcs_guard_im_malloc_refused_total im_malloc requests above the limit, refused
# TYPE sai_teralynx_ifcs_guard_im_malloc_refused_total counter
sai_teralynx_ifcs_guard_im_malloc_refused_total 1
# HELP sai_teralynx_ifcs_guard_im_malloc_refused_max_bytes largest refused im_malloc request
# TYPE sai_teralynx_ifcs_guard_im_malloc_refused_max_bytes gauge
sai_teralynx_ifcs_guard_im_malloc_refused_max_bytes 3221225472
# HELP sai_teralynx_ifcs_guard_updated_timestamp_seconds when the guard last wrote this file
# TYPE sai_teralynx_ifcs_guard_updated_timestamp_seconds gauge
sai_teralynx_ifcs_guard_updated_timestamp_seconds 1764000000.259
"#;
    assert_eq!(io.prom(), expect);
    assert_eq!(String::from_utf8(g.text(&io)).unwrap(), expect);
}

#[test]
fn a_file_that_cannot_be_created_is_retried_on_the_next_read() {
    let io = Fake::new();
    let g = guard(&io, 0);
    io.stats_fail.set(true);
    io.advance(31.0);
    io.set(0x4860, 1);
    rd(&g, &io, 0x4860);
    rd(&g, &io, 0x4860);
    io.stats_fail.set(false);
    let before = io.stats_writes.get();
    rd(&g, &io, 0x4860);
    assert_eq!(io.stats_writes.get(), before + 1, "the timer only moves after a write");
    rd(&g, &io, 0x4860);
    assert_eq!(io.stats_writes.get(), before + 1);
}

#[test]
fn the_inject_hook() {
    let io = Fake::new();
    let g = Guard::new();
    g.init(b"/run/g.prom", 0, 1 << 30, b"/run/inject");
    io.set(0x4860, 5);
    *io.inject.borrow_mut() = Some("4860 2\n".into());
    // 2 injected: the read and the first re-read; the second re-read gives the register
    assert_eq!(rd(&g, &io, 0x4860), (0, 5));
    assert_eq!(
        io.take_log(),
        "sai-teralynx: ifcs guard: TEST: next 2 reads of 0x4860 return 0xffffffff (injected)\nsai-teralynx: ifcs \
         guard: 0x4860 read 0xffffffff (rc 0), re-read 2 gave 0x00000005: recovered (invalid 1, recovered 1, \
         substituted 0)\n"
    );
    assert_eq!(io.inject_opens.get(), 1);
    rd(&g, &io, 0x4860);
    assert_eq!(io.inject_opens.get(), 1, "at most once a second");
    io.advance(1.0);
    rd(&g, &io, 0x4860);
    assert_eq!(io.inject_opens.get(), 2);
    // a register the guard does not have, or a short file: nothing armed
    *io.inject.borrow_mut() = Some("1234 5".into());
    io.advance(1.0);
    assert_eq!(rd(&g, &io, 0x4860), (0, 5));
    assert_eq!(io.take_log(), "");
}

#[test]
fn init_resets_the_counters_but_not_installed_or_the_rate_limit() {
    let io = Fake::new();
    let g = guard(&io, 0);
    io.fail(0x4860, 4);
    rd(&g, &io, 0x4860);
    g.init(b"/run/g.prom", 0, 1 << 30, b"");
    let t = String::from_utf8(g.text(&io)).unwrap();
    assert_eq!(metric(&t, "installed"), m("1"));
    assert_eq!(metric(&t, "substituted_total{reg=\"0x4860\"}"), m("0"));
    assert_eq!(metric(&t, "last_invalid_timestamp_seconds"), m("0.000"));
    // not degraded any more: the next bad read is re-read
    io.fail(0x4860, 4);
    io.take_sleeps();
    rd(&g, &io, 0x4860);
    assert_eq!(io.take_sleeps(), [3, 3, 3]);
}

#[test]
fn text_ptr_is_a_c_string_of_the_text() {
    let io = Fake::new();
    let g = guard(&io, 0);
    let p = g.text_ptr(&io);
    let t = g.text(&io);
    assert!(!p.is_null());
    assert!(t.len() < TEXT_CAP);
}

/// Every counter at `v`, 16 all-ones offsets named, the widest timestamps.
fn widest(v: u64) -> (Fake, Guard) {
    let io = Fake::new();
    let g = guard(&io, 0);
    {
        let mut l = g.lock();
        for r in super::Reg::ALL {
            g.reads.get(r).store(v, std::sync::atomic::Ordering::Relaxed);
            *l.invalid.get_mut(r) = v;
            *l.invalid_rc.get_mut(r) = v;
            *l.recovered.get_mut(r) = v;
            *l.substituted.get_mut(r) = v;
        }
        l.allones.fill(super::AllOnes { off: -1, n: v });
        l.allones_other = v;
        l.malloc_refused = v;
        l.malloc_refused_max = v;
        l.last_invalid = 9_999_999_999.999;
        l.installed = i32::MIN;
    }
    io.real.set(9_999_999_999.999);
    (io, g)
}

/// The widest values the counters' text can hold: every counter at `u64::MAX` (20 digits), the 16
/// names at 0xffffffff, `installed` at `i32::MIN`, both timestamps at -2^63 s (the most negative
/// `CLOCK_REALTIME` a `timespec` gives: 24 characters with the 3 decimals).
#[test]
fn the_worst_case_counters_fit_the_buffer() {
    let (io, g) = widest(u64::MAX);
    g.lock().last_invalid = -9_223_372_036_854_775_808.0;
    io.real.set(-9_223_372_036_854_775_808.0);
    let t = g.text(&io);
    assert_eq!(t.len(), WORST_TEXT, "{}", String::from_utf8_lossy(&t));
    assert!(t.len() < TEXT_CAP);
    let writes = io.stats_writes.get();
    g.installed(&io, i32::MIN);
    assert_eq!(io.stats_writes.get(), writes + 1, "written");
    assert_eq!(io.prom().len(), WORST_TEXT);
}

/// The worst case's length (measured; the buffer holds it with room to spare).
const WORST_TEXT: usize = 4720;

#[test]
fn counters_beyond_the_c_buffer_are_written() {
    // Found by the port, fixed in Rust as a deliberate difference (README "Teralynx shim"): with all
    // 16 all-ones names in use (here the widest, 0xffffffff) the text needs ~4050 bytes at one-digit
    // counters, so three-digit counters overflow the C's 4096-byte buffer; the C then no longer
    // rewrites the file and tl_ifcs_guard_text answers "". The Rust guard writes and answers it.
    let (io, g) = widest(9);
    assert_eq!(g.text(&io).len(), 4054);
    let (io, g) = widest(99);
    assert_eq!(g.text(&io).len(), 4088); // below the C's limit: the same text in both
    let (io, g) = widest(999);
    let t = g.text(&io);
    assert!(t.len() >= C_TEXT_CAP && t.len() < TEXT_CAP, "{}", t.len());
    let writes = io.stats_writes.get();
    g.installed(&io, 1);
    assert_eq!(io.stats_writes.get(), writes + 1, "written");
    assert!(io.prom().len() >= C_TEXT_CAP);
}

/// The mock libifcs' learn pass (`rust/sai/tests/teralynx-mock/mock_learn_thread.c`) over the guard.
struct Learn {
    a: u32,
    wedged: bool,
    last_request: u64,
}

impl Learn {
    fn pass(&mut self, g: &Guard, io: &Fake) -> i32 {
        const RING: u32 = 0x800;
        let (mut r4864, mut r485c, mut b) = (0x5a5a_5a5a, 0x5a5a_5a5a, 0x5a5a_5a5a);
        g.reg_read32(io, 0, 0x4864, Some(&mut r4864));
        g.reg_read32(io, 0, 0x485c, Some(&mut r485c));
        g.reg_read32(io, 0, 0x4860, Some(&mut b));
        let n = if (self.a >> 12) == (b >> 12) {
            RING.wrapping_add(b).wrapping_sub(self.a)
        } else {
            RING.wrapping_sub(b & 0xfff).wrapping_add(self.a & 0xfff)
        };
        self.last_request = u64::from(n) << 3;
        if n != 0 {
            let refused = g.im_malloc_refused(io, usize::try_from(self.last_request).unwrap(), 1);
            assert_eq!(refused, self.last_request > 1 << 30);
        }
        let mut steps = 0;
        while self.a != b {
            steps += 1;
            if steps > 2 * 4096 {
                self.wedged = true;
                steps = -1;
                break;
            }
            self.a += 1;
            if (self.a & 0xfff) == RING {
                self.a = !self.a & 0x1000;
            }
        }
        g.reg_write32(io, 0, 0x4864, (r4864 & !0x1fff) | (self.a & 0x1fff));
        g.reg_write32(io, 0, 0x485c, (r485c & !0x1f_ffff) | 0x42);
        steps
    }
}

#[test]
fn the_c_tests_learn_scenario() {
    let io = Fake::new();
    let g = guard(&io, 0);
    let mut t = Learn {
        a: 0,
        wedged: false,
        last_request: 0,
    };
    io.set(0x4864, 0xa5a0_0000);
    io.set(0x485c, 0x0030_0000);
    io.set(0x4860, 5);
    assert_eq!(t.pass(&g, &io), 5);
    assert_eq!(io.regs.borrow()[&0x4864], 0xa5a0_0005);
    assert_eq!(t.pass(&g, &io), 0);
    io.set(0x4860, 9);
    io.fail(0x4860, 1);
    assert_eq!(t.pass(&g, &io), 4);
    io.set(0x4860, 12);
    io.fail(0x4860, 10);
    assert_eq!(t.pass(&g, &io), 0);
    assert!(t.last_request <= 0x800 * 8);
    for _ in 0..6 {
        assert_eq!(t.pass(&g, &io), 0);
    }
    assert_eq!(t.pass(&g, &io), 3);
    assert!(!t.wedged);
    let p = io.prom();
    assert_eq!(metric(&p, "substituted_total{reg=\"0x4860\"}"), m("7"));
    assert_eq!(metric(&p, "invalid_total{reg=\"0x4860\"}"), m("8"));
    assert_eq!(metric(&p, "reads_total{reg=\"0x4860\"}"), m("11"));
    // phase wrap through the guard
    t.a = 0x7fe;
    io.set(0x4860, 0x1003);
    assert_eq!(t.pass(&g, &io), 5);
    // RMW registers all-ones: the write-back keeps the bits
    io.fail(0x4864, 6);
    io.fail(0x485c, 6);
    assert_eq!(t.pass(&g, &io), 0);
    assert_eq!(io.regs.borrow()[&0x4864], 0xa5a0_1003);
    assert_eq!(io.regs.borrow()[&0x485c] >> 21, 0x0030_0000 >> 21);
}

/// One scripted read answer: rc, value.
fn answer() -> impl Strategy<Value = (i32, u32)> {
    prop_oneof![
        4 => (Just(0), 0_u32..0x2000),
        2 => (Just(0), Just(ALL_ONES)),
        1 => (Just(0), any::<u32>()),
        1 => (-3_i32..3, any::<u32>()),
    ]
}

/// One step of the learn thread's traffic: a read of a guarded register (with the answers its tries
/// will see), a 0x4864 write, a pause.
#[derive(Clone, Debug)]
enum Step {
    Read(i32, Vec<(i32, u32)>),
    Write(u32),
    Wait(f64),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => (prop::sample::select(vec![0x4864, 0x485c, 0x4860]), prop::collection::vec(answer(), 0..5))
            .prop_map(|(o, a)| Step::Read(o, a)),
        2 => any::<u32>().prop_map(Step::Write),
        2 => prop_oneof![Just(0.5), Just(1.0), Just(31.0), Just(601.0)].prop_map(Step::Wait),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Whatever the registers answer: a guarded read succeeds; 0x4860 never comes back with bits
    /// 13..31 set (the thread's count stays below twice the ring); the read-modify-write registers
    /// never come back all-ones; at most 3 re-reads, none while degraded; the journal keeps its rate.
    #[test]
    fn the_guards_promises(ring in prop_oneof![Just(0_u32), Just(0x400), Just(0x800), 1_u32..0x1000],
                           steps in prop::collection::vec(step(), 1..120)) {
        let io = Fake::new();
        let g = guard(&io, ring);
        let mut degraded = [false; 3];
        let mut lines: Vec<(f64, bool)> = Vec::new();
        for s in steps {
            match s {
                Step::Read(off, answers) => {
                    for (rc, v) in answers {
                        io.answer(off, rc, v);
                    }
                    io.take_reads();
                    io.take_sleeps();
                    let (rc, v) = rd(&g, &io, off);
                    prop_assert_eq!(rc, 0);
                    let reads = io.take_reads().len();
                    let sleeps = io.take_sleeps();
                    let i = usize::try_from(off - 0x485c).unwrap() / 4;   // 0x485c 0, 0x4860 1, 0x4864 2
                    prop_assert!(reads <= 4);
                    prop_assert_eq!(sleeps.len(), reads - 1);
                    if degraded[i] {
                        prop_assert_eq!(reads, 1);
                    }
                    if off == 0x4860 {
                        prop_assert_eq!(v & !0x1fff, 0, "0x4860 answered {:#x}", v);
                    } else {
                        prop_assert_ne!(v, ALL_ONES);
                    }
                    let log = io.take_log();
                    let t = io.mono.get();
                    lines.extend(log.lines().map(|l| (t, l.ends_with("more events not logged"))));
                    degraded[i] = g.degraded.get(super::Reg::from_offset(off).unwrap()).load(std::sync::atomic::Ordering::Relaxed);
                }
                Step::Write(v) => {
                    g.reg_write32(&io, 0, 0x4864, v);
                }
                Step::Wait(s) => io.advance(s),
            }
        }
        // at most 20 lines per window (plus the "more events" line opening the next one): a
        // window opens with the first line at or after the previous one's end and lasts 600 s
        let mut end = f64::NEG_INFINITY;
        let mut count = 0;
        for (t, summary) in lines {
            if t >= end {
                end = t + 600.0;
                count = 0;
            }
            if !summary {
                count += 1;
                prop_assert!(count <= 20, "{} lines in one window", count);
            }
        }
    }
}

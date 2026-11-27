//! t_learn.cpp: the learn-ring read guard (src/ifcs_guard.c in libsai-teralynx-shim.so, installed
//! by ifcs_lib.cpp over the mock libifcs' GOT): the mock replays libifcs' learn pass
//! (tests/mock/mock_learn_thread.c) against mock registers with injected all-ones reads.
use crate::support::*;

/// `metric`: the value of the line `<name> <number>` of the guard's text (atoll), -1 if none.
fn metric(text: &str, name: &str) -> i64 {
    for l in lines(text) {
        if l.get(..name.len() + 1) == Some(&format!("{name} ")) {
            return atoll(&l[name.len() + 1..]);
        }
    }
    -1
}

/// `std::atoll`: optional blanks and sign, then the leading digits (0 if none).
fn atoll(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i64 = 0;
    for c in rest.bytes().take_while(u8::is_ascii_digit) {
        v = v.wrapping_mul(10).wrapping_add(i64::from(c - b'0'));
    }
    if neg { v.wrapping_neg() } else { v }
}

const R: &str = "sai_teralynx_ifcs_guard_";

/// `m(n)`: the guard's metric `R + n` now.
fn m(e: &Env, n: &str) -> i64 {
    let text = e.m().guard_text().expect("symbol tl_ifcs_guard_text");
    metric(&text, &format!("{R}{n}"))
}

pub fn test_learn_guard(e: &mut Env) {
    e.ck.section("learn_guard");

    // installed at create_switch, before ifcs_init; the stats file is in the run dir
    check_eq!(e, "t_learn.cpp:45", m(e, "installed"), 1);
    let mut prom = get(&format!("{}/ifcs-guard.prom", e.run));
    check!(e, "t_learn.cpp:47", has(&prom, &format!("{R}installed 1")));
    check!(
        e,
        "t_learn.cpp:48",
        has(&prom, &format!("{R}substituted_total{{reg=\"0x4860\"}} 0"))
    );

    // normal polls: 5 new entries, then none
    e.m().learn_reset(0);
    e.m().reg_set(0x4864, 0xa5a0_0000);
    e.m().reg_set(0x4860, 5);
    check_eq!(e, "t_learn.cpp:54", e.m().learn_pass(), 5);
    check_eq!(e, "t_learn.cpp:55", e.m().learn_sw_index(), 5u32);
    check_eq!(e, "t_learn.cpp:56", e.m().reg_get(0x4864), 0xa5a0_0005u32); // consumer index written back, upper bits kept
    check!(e, "t_learn.cpp:57", e.m().learn_last_request() < (1u64 << 20));
    check_eq!(e, "t_learn.cpp:58", e.m().learn_pass(), 0);
    check_eq!(e, "t_learn.cpp:59", m(e, "reads_total{reg=\"0x4860\"}"), 2);
    check_eq!(e, "t_learn.cpp:60", m(e, "invalid_total{reg=\"0x4860\"}"), 0);

    // one all-ones read of B: a re-read gives the real value
    e.m().reg_set(0x4860, 9);
    e.m().reg_fail(0x4860, 1);
    check_eq!(e, "t_learn.cpp:65", e.m().learn_pass(), 4);
    check_eq!(e, "t_learn.cpp:66", e.m().learn_sw_index(), 9u32);
    check_eq!(e, "t_learn.cpp:67", m(e, "invalid_total{reg=\"0x4860\"}"), 1);
    check_eq!(e, "t_learn.cpp:68", m(e, "recovered_total{reg=\"0x4860\"}"), 1);
    check!(
        e,
        "t_learn.cpp:69",
        m(e, "last_invalid_timestamp_seconds") > 1_000_000_000
    );

    // B all-ones for longer than the re-reads: the first poll re-reads 3 times (4 reads), then the
    // register is degraded: one read per poll, no re-reads, until a valid read (7 polls skipped:
    // nothing new, no 32 GiB request, no endless walk), then the backlog is consumed
    e.m().reg_set(0x4860, 12);
    e.m().reg_fail(0x4860, 10);
    let mut calls0 = e.m().im_malloc_calls();
    check_eq!(e, "t_learn.cpp:77", e.m().learn_pass(), 0);
    check_eq!(e, "t_learn.cpp:78", e.m().learn_sw_index(), 9u32);
    check!(e, "t_learn.cpp:79", e.m().learn_last_request() <= 0x800u64 * 8);
    for _ in 0..6 {
        check_eq!(e, "t_learn.cpp:80", e.m().learn_pass(), 0);
    }
    check_eq!(e, "t_learn.cpp:81", e.m().learn_pass(), 3); // valid again: 12
    check_eq!(e, "t_learn.cpp:82", e.m().learn_sw_index(), 12u32);
    check_eq!(e, "t_learn.cpp:83", e.m().learn_wedged(), 0);
    // libifcs' im_malloc went to the guard (calloc), never the mock's
    check_eq!(e, "t_learn.cpp:84", e.m().im_malloc_calls(), calls0);
    check_eq!(e, "t_learn.cpp:85", m(e, "substituted_total{reg=\"0x4860\"}"), 7);
    check_eq!(e, "t_learn.cpp:86", m(e, "recovered_total{reg=\"0x4860\"}"), 1);
    check_eq!(e, "t_learn.cpp:87", m(e, "invalid_total{reg=\"0x4860\"}"), 8);
    check_eq!(e, "t_learn.cpp:88", m(e, "invalid_rc_total{reg=\"0x4860\"}"), 0);
    // the thread's reads (polls); re-reads are not counted
    check_eq!(e, "t_learn.cpp:89", m(e, "reads_total{reg=\"0x4860\"}"), 11);
    prom = get(&format!("{}/ifcs-guard.prom", e.run)); // rewritten at each event
    check!(
        e,
        "t_learn.cpp:91",
        has(&prom, &format!("{R}substituted_total{{reg=\"0x4860\"}} 7"))
    );

    // out-of-range B (slot >= ring size, the other garbage class) and garbage upper bits
    e.m().reg_set(0x4860, 0x0e05);
    check_eq!(e, "t_learn.cpp:95", e.m().learn_pass(), 0);
    e.m().reg_set(0x4860, 0x8000_0010);
    check_eq!(e, "t_learn.cpp:97", e.m().learn_pass(), 0);
    check_eq!(e, "t_learn.cpp:98", m(e, "substituted_total{reg=\"0x4860\"}"), 9);
    e.m().reg_set(0x4860, 0x10);
    check_eq!(e, "t_learn.cpp:100", e.m().learn_pass(), 4);

    // phase wrap still works through the guard: 0x7fe -> 0x1003
    e.m().learn_reset(0x7fe);
    e.m().reg_set(0x4860, 0x1003);
    check_eq!(e, "t_learn.cpp:105", e.m().learn_pass(), 5);
    check_eq!(e, "t_learn.cpp:106", e.m().learn_sw_index(), 0x1003u32);

    // 0x4864 / 0x485c all-ones (read-modify-write registers): the last valid value is used,
    // so the write-back does not set every control bit
    e.m().reg_fail(0x4864, 6);
    e.m().reg_fail(0x485c, 6);
    check_eq!(e, "t_learn.cpp:112", e.m().learn_pass(), 0);
    check_eq!(e, "t_learn.cpp:113", e.m().reg_get(0x4864), 0xa5a0_1003u32);
    check_eq!(e, "t_learn.cpp:114", e.m().reg_get(0x485c) >> 21, 0x0030_0000u32 >> 21);
    check_eq!(e, "t_learn.cpp:115", m(e, "substituted_total{reg=\"0x4864\"}"), 1);
    check_eq!(e, "t_learn.cpp:116", m(e, "substituted_total{reg=\"0x485c\"}"), 1);
    e.m().learn_pass();
    e.m().learn_pass(); // degraded: one bad read each per poll, the write-back keeps its bits
    check_eq!(e, "t_learn.cpp:119", e.m().reg_get(0x4864), 0xa5a0_1003u32);
    check_eq!(e, "t_learn.cpp:120", e.m().reg_get(0x485c) >> 21, 0x0030_0000u32 >> 21);
    check_eq!(e, "t_learn.cpp:121", m(e, "substituted_total{reg=\"0x4864\"}"), 3);
    e.m().learn_pass(); // valid again
    check_eq!(e, "t_learn.cpp:123", m(e, "substituted_total{reg=\"0x4864\"}"), 3);
    check_eq!(e, "t_learn.cpp:124", m(e, "recovered_total{reg=\"0x4864\"}"), 0);

    // any other register: all-ones counted per offset, value untouched (no retry)
    e.m().reg_set(0x1234, 0xffff_ffff);
    e.m().reg_set(0x2000, 7);
    check_eq!(e, "t_learn.cpp:131", e.m().learn_read(0x1234), 0xffff_ffffu32);
    check_eq!(e, "t_learn.cpp:132", e.m().learn_read(0x1234), 0xffff_ffffu32);
    check_eq!(e, "t_learn.cpp:133", e.m().learn_read(0x2000), 7u32);
    check_eq!(e, "t_learn.cpp:134", m(e, "allones_reads_total{reg=\"0x1234\"}"), 2);
    check_eq!(e, "t_learn.cpp:135", m(e, "allones_reads_total{reg=\"other\"}"), 0);

    // im_malloc above the limit (1 GiB): refused, counted, the mock never asked
    calls0 = e.m().im_malloc_calls();
    check!(
        e,
        "t_learn.cpp:139",
        e.m().learn_im_malloc_last_byte(0x7_FFFF_C008).is_none()
    );
    check_eq!(e, "t_learn.cpp:140", e.m().im_malloc_calls(), calls0);
    check_eq!(e, "t_learn.cpp:141", m(e, "im_malloc_refused_total"), 1);
    check_eq!(
        e,
        "t_learn.cpp:142",
        m(e, "im_malloc_refused_max_bytes"),
        0x7_FFFF_C008i64
    );
    let p = e.m().learn_im_malloc_last_byte(4096);
    check!(e, "t_learn.cpp:144", p.is_some());
    if let Some(last) = p {
        check_eq!(e, "t_learn.cpp:145", last, 0u8);
    }

    // control: the same all-ones read without the guard is the 2026-11-16 wedge
    e.m().learn_reset(0);
    e.m().reg_fail(0x4860, 1);
    check_eq!(e, "t_learn.cpp:150", e.m().learn_pass_unguarded(), -1);
    check_eq!(e, "t_learn.cpp:151", e.m().learn_wedged(), 1);
    // n = 0xFFFFF801: the 32 GiB request
    check_eq!(e, "t_learn.cpp:152", e.m().im_malloc_max(), 0x7_FFFF_C008u64);
    e.m().learn_reset(0);
    e.m().reg_set(0x4860, 0);

    // fresh guard (as at a new syncd start): no valid 0x4864 yet -> 0, not all-ones, in the write-back;
    // a too-small ring size makes every index invalid: counted (and logged once at 10)
    // (guard_reinit resolves the symbols and the mock's register accessors itself: an Err is the
    // C++'s "mrd && mwr" missing, whose early return is part of the test)
    let prom_path = format!("{}/ifcs-guard.prom", e.run);
    let r = e.m().guard_reinit(&prom_path, 0x400, 1u64 << 30);
    check!(e, "t_learn.cpp:162", r.is_ok());
    let Ok(r) = r else {
        return;
    };
    check_eq!(e, "t_learn.cpp:164", r, 0);
    e.m().learn_reset(0x3);
    e.m().reg_set(0x4864, 0xa5a0_0003);
    e.m().reg_set(0x4860, 0x3);
    e.m().reg_fail(0x4864, 4);
    check_eq!(e, "t_learn.cpp:169", e.m().learn_pass(), 0);
    check_eq!(e, "t_learn.cpp:170", e.m().reg_get(0x4864), 0x3u32); // upper bits 0, not 0xffffe000
    e.m().reg_set(0x4860, 0x500); // valid for 2048, not for the configured 1024
    for _ in 0..10 {
        check_eq!(e, "t_learn.cpp:172", e.m().learn_pass(), 0);
    }
    check_eq!(e, "t_learn.cpp:173", m(e, "substituted_total{reg=\"0x4860\"}"), 10);
    let r = e.m().guard_reinit(&prom_path, 0x800, 1u64 << 30).unwrap_or(-1);
    check_eq!(e, "t_learn.cpp:174", r, 0);
    e.m().learn_reset(0);
    e.m().reg_set(0x4860, 0);
}

// SPDX-License-Identifier: Apache-2.0
// The learn-ring read guard (src/ifcs_guard.c in libsai-teralynx-shim.so,
// installed by ifcs_lib.cpp over the mock libifcs' GOT): the mock replays
// libifcs' learn pass (tests/mock/mock_learn_thread.c) against mock
// registers with injected all-ones reads.
#include "harness.h"

template <typename T> static void sym(T& fn, const char* name)
{
    fn = reinterpret_cast<T>(dlsym(RTLD_DEFAULT, name));   // the shim is loaded RTLD_GLOBAL
    if (!fn) { std::fprintf(stderr, "symbol %s missing\n", name); std::exit(2); }
}
static long long metric(const std::string& text, const std::string& name)
{
    for (auto& l : lines(text))
        if (l.compare(0, name.size() + 1, name + " ") == 0) return std::atoll(l.c_str() + name.size() + 1);
    return -1;
}

void test_learn_guard(Env& e)
{
    int (*pass)(), (*pass_raw)(), (*wedged)();
    void (*reg_set)(int32_t, uint32_t), (*reg_fail)(int32_t, unsigned), (*reset)(uint32_t);
    uint32_t (*reg_get)(int32_t), (*sw)();
    uint64_t (*last_req)(), (*mcalls)(), (*mmax)();
    void* (*call_im_malloc)(size_t);
    const char* (*text)();
    hook(g_mock, pass, "mock_learn_pass");
    hook(g_mock, pass_raw, "mock_learn_pass_unguarded");
    hook(g_mock, wedged, "mock_learn_wedged");
    hook(g_mock, reg_set, "mock_reg_set");
    hook(g_mock, reg_fail, "mock_reg_fail");
    hook(g_mock, reset, "mock_learn_reset");
    hook(g_mock, reg_get, "mock_reg_get");
    hook(g_mock, sw, "mock_learn_sw_index");
    hook(g_mock, last_req, "mock_learn_last_request");
    hook(g_mock, mcalls, "mock_im_malloc_calls");
    hook(g_mock, mmax, "mock_im_malloc_max");
    hook(g_mock, call_im_malloc, "mock_learn_im_malloc");
    sym(text, "tl_ifcs_guard_text");
    const std::string R = "sai_teralynx_ifcs_guard_";
    auto m = [&](const std::string& n) { return metric(text(), R + n); };

    // installed at create_switch, before ifcs_init; the stats file is in the run dir
    CHECK_EQ(m("installed"), 1);
    std::string prom = get(e.run + "/ifcs-guard.prom");
    CHECK(has(prom, R + "installed 1"));
    CHECK(has(prom, R + "substituted_total{reg=\"0x4860\"} 0"));

    // normal polls: 5 new entries, then none
    reset(0);
    reg_set(0x4864, 0xa5a00000u);
    reg_set(0x4860, 5);
    CHECK_EQ(pass(), 5);
    CHECK_EQ(sw(), 5u);
    CHECK_EQ(reg_get(0x4864), 0xa5a00005u);      // consumer index written back, upper bits kept
    CHECK(last_req() < (1u << 20));
    CHECK_EQ(pass(), 0);
    CHECK_EQ(m("reads_total{reg=\"0x4860\"}"), 2);
    CHECK_EQ(m("invalid_total{reg=\"0x4860\"}"), 0);

    // one all-ones read of B: a re-read gives the real value
    reg_set(0x4860, 9);
    reg_fail(0x4860, 1);
    CHECK_EQ(pass(), 4);
    CHECK_EQ(sw(), 9u);
    CHECK_EQ(m("invalid_total{reg=\"0x4860\"}"), 1);
    CHECK_EQ(m("recovered_total{reg=\"0x4860\"}"), 1);
    CHECK(m("last_invalid_timestamp_seconds") > 1000000000);

    // B all-ones for longer than the re-reads: the first poll re-reads 3 times (4 reads), then the
    // register is degraded: one read per poll, no re-reads, until a valid read (7 polls skipped:
    // nothing new, no 32 GiB request, no endless walk), then the backlog is consumed
    reg_set(0x4860, 12);
    reg_fail(0x4860, 10);
    uint64_t calls0 = mcalls();
    CHECK_EQ(pass(), 0);
    CHECK_EQ(sw(), 9u);
    CHECK(last_req() <= 0x800u * 8);
    for (int k = 0; k < 6; k++) CHECK_EQ(pass(), 0);
    CHECK_EQ(pass(), 3);                           // valid again: 12
    CHECK_EQ(sw(), 12u);
    CHECK_EQ(wedged(), 0);
    CHECK_EQ(mcalls(), calls0);                    // libifcs' im_malloc went to the guard (calloc), never the mock's
    CHECK_EQ(m("substituted_total{reg=\"0x4860\"}"), 7);
    CHECK_EQ(m("recovered_total{reg=\"0x4860\"}"), 1);
    CHECK_EQ(m("invalid_total{reg=\"0x4860\"}"), 8);
    CHECK_EQ(m("invalid_rc_total{reg=\"0x4860\"}"), 0);
    CHECK_EQ(m("reads_total{reg=\"0x4860\"}"), 11);   // the thread's reads (polls); re-reads are not counted
    prom = get(e.run + "/ifcs-guard.prom");        // rewritten at each event
    CHECK(has(prom, R + "substituted_total{reg=\"0x4860\"} 7"));

    // out-of-range B (slot >= ring size, the other garbage class) and garbage upper bits
    reg_set(0x4860, 0x0e05);
    CHECK_EQ(pass(), 0);
    reg_set(0x4860, 0x80000010u);
    CHECK_EQ(pass(), 0);
    CHECK_EQ(m("substituted_total{reg=\"0x4860\"}"), 9);
    reg_set(0x4860, 0x10);
    CHECK_EQ(pass(), 4);

    // phase wrap still works through the guard: 0x7fe -> 0x1003
    reset(0x7fe);
    reg_set(0x4860, 0x1003);
    CHECK_EQ(pass(), 5);
    CHECK_EQ(sw(), 0x1003u);

    // 0x4864 / 0x485c all-ones (read-modify-write registers): the last valid value is used,
    // so the write-back does not set every control bit
    reg_fail(0x4864, 6);
    reg_fail(0x485c, 6);
    CHECK_EQ(pass(), 0);
    CHECK_EQ(reg_get(0x4864), 0xa5a01003u);
    CHECK_EQ(reg_get(0x485c) >> 21, 0x00300000u >> 21);
    CHECK_EQ(m("substituted_total{reg=\"0x4864\"}"), 1);
    CHECK_EQ(m("substituted_total{reg=\"0x485c\"}"), 1);
    pass();
    pass();   // degraded: one bad read each per poll, the write-back keeps its bits
    CHECK_EQ(reg_get(0x4864), 0xa5a01003u);
    CHECK_EQ(reg_get(0x485c) >> 21, 0x00300000u >> 21);
    CHECK_EQ(m("substituted_total{reg=\"0x4864\"}"), 3);
    pass();   // valid again
    CHECK_EQ(m("substituted_total{reg=\"0x4864\"}"), 3);
    CHECK_EQ(m("recovered_total{reg=\"0x4864\"}"), 0);

    // any other register: all-ones counted per offset, value untouched (no retry)
    uint32_t (*rd_other)(int32_t);
    hook(g_mock, rd_other, "mock_learn_read");
    reg_set(0x1234, 0xffffffffu);
    reg_set(0x2000, 7);
    CHECK_EQ(rd_other(0x1234), 0xffffffffu);
    CHECK_EQ(rd_other(0x1234), 0xffffffffu);
    CHECK_EQ(rd_other(0x2000), 7u);
    CHECK_EQ(m("allones_reads_total{reg=\"0x1234\"}"), 2);
    CHECK_EQ(m("allones_reads_total{reg=\"other\"}"), 0);

    // im_malloc above the limit (1 GiB): refused, counted, the mock never asked
    calls0 = mcalls();
    CHECK(call_im_malloc(0x7FFFFC008ull) == nullptr);
    CHECK_EQ(mcalls(), calls0);
    CHECK_EQ(m("im_malloc_refused_total"), 1);
    CHECK_EQ(m("im_malloc_refused_max_bytes"), 0x7FFFFC008ll);
    void* p = call_im_malloc(4096);
    CHECK(p != nullptr);
    if (p) { CHECK_EQ(static_cast<unsigned char*>(p)[4095], 0); std::free(p); }

    // control: the same all-ones read without the guard is the 2026-11-16 wedge
    reset(0);
    reg_fail(0x4860, 1);
    CHECK_EQ(pass_raw(), -1);
    CHECK_EQ(wedged(), 1);
    CHECK_EQ(mmax(), 0x7FFFFC008ull);              // n = 0xFFFFF801: the 32 GiB request
    reset(0);
    reg_set(0x4860, 0);

    // fresh guard (as at a new syncd start): no valid 0x4864 yet -> 0, not all-ones, in the write-back;
    // a too-small ring size makes every index invalid: counted (and logged once at 10)
    int (*ginit)(void*, void*, const char*, unsigned, unsigned long long);
    sym(ginit, "tl_ifcs_guard_init");
    void* mrd = dlsym(g_mock, "mock_reg_read_impl");
    void* mwr = dlsym(g_mock, "mock_reg_write_impl");
    CHECK(mrd && mwr);
    if (!mrd || !mwr) return;
    CHECK_EQ(ginit(mrd, mwr, (e.run + "/ifcs-guard.prom").c_str(), 0x400, 1ull << 30), 0);
    reset(0x3);
    reg_set(0x4864, 0xa5a00003u);
    reg_set(0x4860, 0x3);
    reg_fail(0x4864, 4);
    CHECK_EQ(pass(), 0);
    CHECK_EQ(reg_get(0x4864), 0x3u);               // upper bits 0, not 0xffffe000
    reg_set(0x4860, 0x500);                         // valid for 2048, not for the configured 1024
    for (int k = 0; k < 10; k++) CHECK_EQ(pass(), 0);
    CHECK_EQ(m("substituted_total{reg=\"0x4860\"}"), 10);
    CHECK_EQ(ginit(mrd, mwr, (e.run + "/ifcs-guard.prom").c_str(), 0x800, 1ull << 30), 0);
    reset(0);
    reg_set(0x4860, 0);
}

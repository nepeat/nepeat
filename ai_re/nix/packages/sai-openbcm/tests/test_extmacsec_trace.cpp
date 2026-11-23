// SPDX-License-Identifier: Apache-2.0
// The sequence trace of the external MACsec table (src/ext_macsec.h, diag socket
// "extmacsec trace"): what it records, against a golden text. The same text is
// expected from the C++ engine and from the Rust one (make RUST_PHY_SEQ=...), and
// it must not contain a byte of the key material. Links libsai.a; no switch.
#include "ext_macsec.h"

#include <cstdio>
#include <map>
#include <string>
#include <vector>

using namespace ob;

static int g_checks, g_fail;
#define CHECK(c)                                                                \
    do {                                                                        \
        g_checks++;                                                             \
        if (!(c)) {                                                             \
            g_fail++;                                                           \
            std::fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #c);   \
        }                                                                       \
    } while (0)

static void same(const std::string& got, const std::string& want, int line)
{
    g_checks++;
    if (got == want) return;
    g_fail++;
    std::fprintf(stderr, "FAIL %s:%d: trace\n--- got\n%s--- want\n%s", __FILE__, line, got.c_str(), want.c_str());
}
#define SAME(got, want) same(got, want, __LINE__)

int main()
{
    const char* table =
        "ext-macsec 1\n"
        "seq sa\n"
        "R p 1 900a\n"
        "W p 1 a000 key[1].key[0]\n"
        "W p 1 a001 00.sci[7]\n"
        "W p 1 a002 hkey[0].00\n"
        "W p 1 a003 salt[0].ssci[0]\n"
        "W p 1 a004 iv[0].iv[1]\n"
        "W p 1 a005 pn[1].pn[0]\n"
        "W p 1 a006 window[1].ssci[3]\n"
        "X p 1 9005 $3\n"
        "W 22 1 9005 $3&7fff|1\n"
        "D 7\n"
        "E p 1 a100 2 f\n"
        "P p 1 8221 12e3 ffff 3 5\n"
        "X p 1 a101 pn[0]\n"
        "seq ro\n"
        "X p 1 a101 pn[0]\n"
        "R p 1 a102\n";
    MsTable t;
    std::string err;
    CHECK(ext_macsec_parse(table, t, err));
    const std::vector<MsOp>* sa = t.find("sa");
    const std::vector<MsOp>* ro = t.find("ro");
    CHECK(sa && ro);
    if (!sa || !ro) return 1;

    std::map<unsigned, std::vector<unsigned>> answers;
    MsIo io;
    io.rd = [&](unsigned, unsigned, unsigned r, unsigned& v, const MsOp&) {
        auto& q = answers[r];
        v = q.empty() ? 0 : q.front();
        if (!q.empty()) q.erase(q.begin());
        return SAI_STATUS_SUCCESS;
    };
    io.wr = [&](unsigned, unsigned, unsigned, unsigned) { return SAI_STATUS_SUCCESS; };
    io.delay = [&](unsigned) {};
    // Key material: bytes c0.. (key), d0.. (hkey), e0.. (salt), f0.. (iv), so that a leak is easy to see.
    auto bytes = [](unsigned first, size_t n) {
        std::vector<uint8_t> v;
        for (size_t i = 0; i < n; i++) v.push_back((uint8_t)(first + i));
        return v;
    };
    MsParams par = {{"key", bytes(0xc0, 32)}, {"hkey", bytes(0xd0, 16)}, {"salt", bytes(0xe0, 12)}, {"iv", bytes(0xf0, 12)},
                    {"sci", bytes(0x0a, 8)},  {"ssci", bytes(0x21, 4)},  {"pn", bytes(0x31, 8)},    {"window", bytes(0x41, 4)}};
    auto good = [&] { answers = {{0x9005, {0x8001}}, {0xa100, {0x12}}, {0x8221, {0, 0x12e3}}, {0xa101, {0xbeef}}}; };
    std::string engine = ext_macsec_engine();
    CHECK(engine == "c++" || engine == "rust");
    auto head = [&](const char* mode, int runs, int lines) {
        return std::string("extmacsec trace: ") + mode + ", engine " + engine + ", " + std::to_string(runs) + " runs, " +
               std::to_string(lines) + " lines kept, 0 dropped\n";
    };

    // Off by default: nothing is recorded.
    MsResult res;
    good();
    CHECK(ext_macsec_exec(io, *sa, 0x30, par, &res, err) == SAI_STATUS_SUCCESS);
    SAME(ext_macsec_trace(""), head("off", 0, 0));
    SAME(ext_macsec_trace("dump"), head("off", 0, 0));

    // On: a sequence that writes is recorded; words of key material are not shown, the others are.
    SAME(ext_macsec_trace("on"), head("on (sequences that write)", 0, 0));
    good();
    res.clear();
    CHECK(ext_macsec_exec(io, *sa, 0x30, par, &res, err) == SAI_STATUS_SUCCESS);
    CHECK(ms_result_u64(res, "pn") == 0xbeef);
    CHECK(ext_macsec_exec(io, *ro, 0x30, par, &res, err) == SAI_STATUS_SUCCESS);   // reads only: not recorded
    const std::string run1 =
        "# run 1: phy 0x30, 14 operations from line 3\n"
        "R 030 1 900a\n"
        "W 030 1 a000 ****\n"
        "W 030 1 a001 0011\n"
        "W 030 1 a002 ****\n"
        "W 030 1 a003 ****\n"
        "W 030 1 a004 ****\n"
        "W 030 1 a005 3231\n"
        "W 030 1 a006 4224\n"
        "R 030 1 9005\n"
        "W 022 1 9005 0001\n"
        "D 7\n"
        "R 030 1 a100\n"
        "R 030 1 8221\n"
        "D 5\n"
        "R 030 1 8221\n"
        "R 030 1 a101\n"
        "= 0\n";
    SAME(ext_macsec_trace("dump"), head("on (sequences that write)", 1, 18) + run1);

    // A run that stops: what reached the bus, then the status. A missing parameter: nothing reached it.
    good();
    answers[0xa100] = {0x13};
    CHECK(ext_macsec_exec(io, *sa, 0x17c, par, &res, err) == SAI_STATUS_FAILURE);
    MsParams shortkey = par;
    shortkey["key"] = bytes(0xc0, 1);
    CHECK(ext_macsec_exec(io, *sa, 0x30, shortkey, &res, err) == SAI_STATUS_INVALID_PARAMETER);
    const std::string run23 =
        "# run 2: phy 0x17c, 14 operations from line 3\n"
        "R 17c 1 900a\n"
        "W 17c 1 a000 ****\n"
        "W 17c 1 a001 0011\n"
        "W 17c 1 a002 ****\n"
        "W 17c 1 a003 ****\n"
        "W 17c 1 a004 ****\n"
        "W 17c 1 a005 3231\n"
        "W 17c 1 a006 4224\n"
        "R 17c 1 9005\n"
        "W 022 1 9005 0001\n"
        "D 7\n"
        "R 17c 1 a100\n"
        "= " + std::to_string(SAI_STATUS_FAILURE) + "\n"
        "# run 3: phy 0x30, 14 operations from line 3\n"
        "= " + std::to_string(SAI_STATUS_INVALID_PARAMETER) + "\n";
    SAME(ext_macsec_trace("dump"), head("on (sequences that write)", 3, 34) + run1 + run23);

    // No byte of key, hash key, salt or iv anywhere in the trace (c0c1.., d0, e0, f0f1).
    std::string all = ext_macsec_trace("dump");
    for (const char* leak : {"c0", "c1", "d0", "e0", "f0", "f1"}) CHECK(all.find(leak) == std::string::npos);

    // "on all" records the read-only sequences too; "clear" empties; "off" stops.
    SAME(ext_macsec_trace("clear"), head("on (sequences that write)", 0, 0));
    SAME(ext_macsec_trace("on all"), head("on (all sequences)", 0, 0));
    CHECK(ext_macsec_exec(io, *ro, 0x30, par, &res, err) == SAI_STATUS_SUCCESS);
    SAME(ext_macsec_trace("dump"), head("on (all sequences)", 1, 4) + "# run 1: phy 0x30, 2 operations from line 18\nR 030 1 a101\nR 030 1 a102\n= 0\n");
    SAME(ext_macsec_trace("off"), head("off", 1, 4));
    good();
    CHECK(ext_macsec_exec(io, *sa, 0x30, par, &res, err) == SAI_STATUS_SUCCESS);
    SAME(ext_macsec_trace("status"), "usage: extmacsec trace [on [all] | off | dump | clear]   (on: sequences that write; on all: every sequence)\n");
    SAME(ext_macsec_trace(""), head("off", 1, 4));
    SAME(ext_macsec_trace("clear"), head("off", 0, 0));
    ms_wipe(par);
    ms_wipe(shortkey);

    std::printf("test_extmacsec_trace (%s engine): %d checks, %d failed\n", engine.c_str(), g_checks, g_fail);
    return g_fail ? 1 : 0;
}

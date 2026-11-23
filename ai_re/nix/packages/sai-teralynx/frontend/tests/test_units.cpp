// SPDX-License-Identifier: Apache-2.0
// Unit tests of library internals that have no meaning for another libsai: the frontend's
// attribute-indexed status helpers, and the external-PHY / external-MACsec table parsers and
// the MACsec sequence executor (src/ext_phy.h, src/ext_macsec.h; with RUST_PHY_SEQ the Rust
// engine behind the same C++ interface, which phy-seq also tests on its own). Moved here from
// test_sai.cpp and its .inc files when those went onto the test port (tests/test_port.h):
// 3 + 63 + 74 = 140 checks, and test_sai's count went down by exactly that.
#include "ext_macsec.h"
#include "ext_phy.h"
#include "sai_internal.h"
#include "test_tables.h"

#include <cstdio>
#include <cstring>
#include <map>
#include <string>
#include <vector>

using namespace ob;

static int g_fail = 0, g_checks = 0;
#define CHECK(cond)                                                                    \
    do {                                                                               \
        g_checks++;                                                                    \
        if (!(cond)) {                                                                 \
            g_fail++;                                                                  \
            std::fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);       \
        }                                                                              \
    } while (0)
#define CHECK_EQ(a, b)                                                                 \
    do {                                                                               \
        g_checks++;                                                                    \
        auto _a = (a);                                                                 \
        auto _b = (b);                                                                 \
        if (!(_a == _b)) {                                                             \
            g_fail++;                                                                  \
            std::fprintf(stderr, "FAIL %s:%d: %s == %s (%lld vs %lld)\n", __FILE__,     \
                         __LINE__, #a, #b, (long long)_a, (long long)_b);              \
        }                                                                              \
    } while (0)
#define OK(x) CHECK_EQ((x), SAI_STATUS_SUCCESS)

// attribute-indexed status codes stay inside SAI's ranges (3 checks)
static void test_attr_status()
{
    CHECK(SAI_STATUS_IS_ATTR_NOT_SUPPORTED(attr_not_supported(3)));
    CHECK(SAI_STATUS_IS_INVALID_ATTR_VALUE(attr_invalid(7)));
    CHECK_EQ(attr_not_supported(3), SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 3);
}

// the external-PHY table parser (63 checks: 13 + 25 bad tables x 2)
static void test_ext_phy_parser()
{
    ExtPhyTable t;
    std::string err;
    const char* good = kExtPhyGood;
    CHECK(ext_phy_parse(good, t, err));
    CHECK_EQ(t.ports.size(), 2u);
    CHECK_EQ(t.poll_ms, 0u);
    CHECK_EQ(t.up_polls, 2u);
    CHECK_EQ(t.holdoff_ms, 50u);
    CHECK_EQ(t.ports[0].link.size(), 6u);
    CHECK_EQ(t.ports[0].down.size(), 1u);
    CHECK_EQ(t.ports[0].up.size(), 2u);
    CHECK_EQ(t.ports[0].up[0].mask, 0xff7fu);
    CHECK_EQ(t.ports[0].link[3].tries, 3u);
    CHECK(ext_phy_parse("ext-phy 1\n", t, err));   // no ports: valid, nothing to do
    CHECK_EQ(t.poll_ms, 500u);
    CHECK_EQ(t.up_polls, 2u);
    struct { const char* text; const char* in_err; } bad[] = {
        {"", "line 1"},
        {"mdio-script 1\n", "line 1"},
        {"ext-phy 2\n", "line 1"},
        {"ext-phy 1\nport 1\nlink\nC 10 0 1 3 3\n", "clause 22"},              // dev 0
        {"ext-phy 1\nport 1\nlink\nC 10 20 1 3 3\n", "line 4"},                // dev > 31
        {"ext-phy 1\nport 1\nlink\nC 10 1 10000 3 3\n", "line 4"},             // reg > ffff
        {"ext-phy 1\nport 1\nlink\nC 10 1 a101 3\n", "line 4"},                // C without mask
        {"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 1\n", "outside the mask"},
        {"ext-phy 1\nport 1\nlink\nW 10 1 a000 6\n", "without a C"},
        {"ext-phy 1\nport 1\nadmin-down\nC 10 1 a101 3 3\n", "outside a link"},
        {"ext-phy 1\nport 1\n", "no sequence"},
        {"ext-phy 1\nW 10 1 a000 6\n", "outside"},
        {"ext-phy 1\nport 1\nW 10 1 a000 6\n", "outside"},
        {"ext-phy 1\nlink\n", "before any port"},
        {"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nport 1\nlink\nC 10 1 a101 3 3\n", "twice"},
        {"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nlink\nC 10 1 a101 3 3\n", "twice"},
        {"ext-phy 1\nport x\n", "line 2"},
        {"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 0 0\nC 10 1 a101 3 3\n", "tries"},
        {"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 5 200000\nC 10 1 a101 3 3\n", "interval"},
        {"ext-phy 1\nport 1\nlink\nX 10 1 8221\n", "unknown keyword"},
        {"ext-phy 1\nport 1\nlink\nW 10 1 a000 zz\n", "line 4"},
        {"ext-phy 1\nport 1\nlink\nW 10 1 a000 -6\n", "line 4"},
        {"ext-phy 1\npoll-ms 70000\n", "poll-ms"},
        {"ext-phy 1\nup-polls 0\n", "up-polls"},
        {"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\npoll-ms 100\n", "after the first port"},
    };
    for (auto& b : bad) {
        err.clear();
        CHECK(!ext_phy_parse(b.text, t, err));
        if (err.find(b.in_err) == std::string::npos) {
            g_fail++;
            std::fprintf(stderr, "FAIL ext-phy parse: '%s' gave '%s', wanted '%s' in it\n", b.text, err.c_str(), b.in_err);
        }
        g_checks++;
    }
}

// the external-MACsec table parser and the sequence executor (74 checks: 9 + 27 bad tables x 2 + 11)
static void test_ext_macsec_parser()
{
    MsTable t;
    std::string err;
    CHECK(ext_macsec_parse(ms_table(), t, err));
    CHECK_EQ(t.ports.size(), 2u);
    CHECK_EQ(t.ports[5], 0x11u);
    CHECK_EQ(t.ifg_bits, 384u);
    CHECK(t.clear_on_read);
    CHECK(t.find("sa-add-first.ingress.3.gcm-aes-xpn-256") != nullptr);
    CHECK(t.find("sa-add.ingress.3.gcm-aes-xpn-512") == nullptr);
    CHECK(ext_macsec_parse("ext-macsec 1\n", t, err));   // empty: valid
    CHECK(ext_macsec_parse("ext-macsec 1\ncounters absolute\nseq a\nW 10 1 a000 $0|1\n", t, err) == false);   // $0 never read
    struct { const char* text; const char* in_err; } bad[] = {
        {"", "line 1"},
        {"ext-phy 1\n", "line 1"},
        {"ext-macsec 2\n", "line 1"},
        {"ext-macsec 1\nW p 1 a000 6\n", "outside a sequence"},
        {"ext-macsec 1\nseq a\nW p 0 a000 6\n", "clause 22"},
        {"ext-macsec 1\nseq a\nW p 20 a000 6\n", "line 3"},
        {"ext-macsec 1\nseq a\nW q 1 a000 6\n", "phy"},
        {"ext-macsec 1\nseq a\nW p 1 a000 10000\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 key[32].00\n", "value"},        // beyond the longest key
        {"ext-macsec 1\nseq a\nW p 1 a000 hkey[16].00\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 secret[0].00\n", "value"},      // unknown parameter
        {"ext-macsec 1\nseq a\nW p 1 a000 0.key[0]\n", "value"},          // a byte is two hex digits
        {"ext-macsec 1\nseq a\nW p 1 a000 $16\n", "value"},
        {"ext-macsec 1\nseq a\nW p 1 a000 $0\n", "before an X"},
        {"ext-macsec 1\nseq a\nX p 1 a000 $16\n", "destination"},
        {"ext-macsec 1\nseq a\nX p 1 a000 pn[8]\n", "destination"},
        {"ext-macsec 1\nseq a\nX p 1 a000 pn\n", "destination"},
        {"ext-macsec 1\nseq a\nE p 1 a000 3 1\n", "outside the mask"},
        {"ext-macsec 1\nseq a\nP p 1 a000 1 1 0 0\n", "tries"},
        {"ext-macsec 1\nseq a\nD 200000\n", "D <usec"},
        {"ext-macsec 1\nseq a\nC p 1 a000 1 1\n", "unknown keyword"},
        {"ext-macsec 1\nseq a\nseq a\n", "twice"},
        {"ext-macsec 1\nseq A\n", "seq <name>"},
        {"ext-macsec 1\nport 1 10\nport 1 11\n", "twice"},
        {"ext-macsec 1\nport 1\n", "port <first lane"},
        {"ext-macsec 1\ncounters sometimes\n", "counters"},
        {"ext-macsec 1\nifg-bits 9999\n", "ifg-bits"},
    };
    for (auto& b : bad) {
        err.clear();
        CHECK(!ext_macsec_parse(b.text, t, err));
        if (err.find(b.in_err) == std::string::npos) {
            g_fail++;
            std::fprintf(stderr, "FAIL ext-macsec parse: '%s' gave '%s', wanted '%s' in it\n", b.text, err.c_str(), b.in_err);
        }
        g_checks++;
    }

    // ---- executor against a golden rendering (no switch needed)
    {
        CHECK(ext_macsec_parse("ext-macsec 1\nseq s\nR p 1 900a\nW p 1 a000 key[1].key[0]\nW 22 1 a001 00.sci[7]\nX p 1 9005 $3\nX p 1 9005 $4\n"
                               "W p 1 9005 $3&7fff\nW p 1 9005 $4|8002\nD 7\nE p 1 a100 2 f\nP p 1 8221 12e3 ffff 3 5\nX p 1 a101 pn[0]\nX p 1 a102 pn[2]\n", t, err));
        std::vector<std::string> log;
        std::map<unsigned, std::vector<unsigned>> answers = {{0x9005, {0x8001, 0x0010}}, {0xa100, {0x12}}, {0x8221, {0, 0x12e3}}, {0xa101, {0xbeef}}, {0xa102, {0x1}}};
        MsIo io;
        char b[64];
        io.rd = [&](unsigned p, unsigned d, unsigned r, unsigned& v, const MsOp&) {
            auto& q = answers[r];
            v = q.empty() ? 0 : q.front();
            if (!q.empty()) q.erase(q.begin());
            std::snprintf(b, sizeof(b), "R %x %x.%04x", p, d, r);
            log.push_back(b);
            return SAI_STATUS_SUCCESS;
        };
        io.wr = [&](unsigned p, unsigned d, unsigned r, unsigned v) {
            std::snprintf(b, sizeof(b), "W %x %x.%04x %04x", p, d, r, v);
            log.push_back(b);
            return SAI_STATUS_SUCCESS;
        };
        io.delay = [&](unsigned us) { log.push_back("D " + std::to_string(us)); };
        MsParams par = {{"key", {0x11, 0x22}}, {"sci", {1, 2, 3, 4, 5, 6, 7, 8}}};
        MsResult res;
        OK(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err));
        const char* golden = "R 30 1.900a|W 30 1.a000 2211|W 22 1.a001 0008|R 30 1.9005|R 30 1.9005|W 30 1.9005 0001|W 30 1.9005 8012|D 7|R 30 1.a100|"
                             "R 30 1.8221|D 5|R 30 1.8221|R 30 1.a101|R 30 1.a102";
        std::string got;
        for (auto& l : log) got += (got.empty() ? "" : "|") + l;
        CHECK(got == golden);
        CHECK_EQ(ms_result_u64(res, "pn"), 0x10000beefull);
        // a missing parameter byte: nothing is accessed
        log.clear();
        par["key"] = {0x11};
        CHECK_EQ(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err), SAI_STATUS_INVALID_PARAMETER);
        CHECK(log.empty());
        CHECK(err.find("key[1]") != std::string::npos);
        // a compare that fails stops the sequence there and names the line, not a value of ours
        par["key"] = {0x11, 0x22};
        answers = {{0xa100, {0x13}}};
        log.clear();
        CHECK_EQ(ext_macsec_exec(io, *t.find("s"), 0x30, par, &res, err), SAI_STATUS_FAILURE);
        CHECK(log.back() == "R 30 1.a100");
        CHECK(err.find("line 11") != std::string::npos);
        ms_wipe(par);
        CHECK(par.empty());
    }
}

int main()
{
    test_attr_status();
    test_ext_phy_parser();
    test_ext_macsec_parser();
    std::printf("test_units: %d checks, %d failures\n", g_checks, g_fail);
    return g_fail ? 1 : 0;
}

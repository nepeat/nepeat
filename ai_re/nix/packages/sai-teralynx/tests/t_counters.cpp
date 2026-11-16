// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, counters breadth (tl_counters.cpp, PortArea): IF_OUT_OCTETS
// on TX_BYTES_OK (isai's choice), every mapped SAI port counter through the
// diag shell ("counters <devport>"), CPU queue counters ("copp"), the ACL
// report ("acl") (harness: tests/harness.h).
#include "harness.h"

#include <sys/socket.h>
#include <sys/un.h>

static std::string diag(const std::string& path, const std::string& cmd)
{
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    sockaddr_un sa;
    std::memset(&sa, 0, sizeof(sa));
    sa.sun_family = AF_UNIX;
    std::snprintf(sa.sun_path, sizeof(sa.sun_path), "%s", path.c_str());
    if (fd < 0 || connect(fd, (sockaddr*)&sa, sizeof(sa)) != 0) {
        if (fd >= 0) close(fd);
        return "(no diag socket)";
    }
    std::string c = cmd + "\n";
    (void)!write(fd, c.data(), c.size());
    shutdown(fd, SHUT_WR);
    std::string out;
    char b[4096];
    ssize_t n;
    while ((n = read(fd, b, sizeof(b))) > 0) out.append(b, (size_t)n);
    close(fd);
    return out;
}

void test_counters(Env& e)
{
    USE_ENV(e);
    // ---- IF_OUT_OCTETS = TX_BYTES_OK (107), not TX_BYTES_ALL (106); IN stays RX_BYTES_ALL
    m_set_stat(1, 106, 9999);
    m_set_stat(1, 107, 7777);
    m_set_stat(1, 47, 5555);
    OK(pa->clear_port_all_stats(p1));
    m_set_stat(1, 106, 9999);
    m_set_stat(1, 107, 7777);
    m_set_stat(1, 47, 5555);
    sai_stat_id_t ids[2] = {SAI_PORT_STAT_IF_OUT_OCTETS, SAI_PORT_STAT_IF_IN_OCTETS};
    uint64_t v[2] = {0, 0};
    OK(pa->get_port_stats(p1, 2, ids, v));
    CHECK_EQ(v[0], 7777u);
    CHECK_EQ(v[1], 5555u);

    // ---- every mapped SAI port counter (diag shell): FEC, PFC, size buckets, pause
    m_set_stat(1, 52, 11);    // RX_KRPFEC_CORRECTED_CW_COUNT
    m_set_stat(1, 53, 12);    // UNCORRECTED
    m_set_stat(1, 54, 13);    // CW_SYMBOL_ERR
    m_set_stat(1, 24, 3);     // RX_FRAMES_PFC3
    m_set_stat(1, 87, 4);     // TX_FRAMES_PFC3
    m_set_stat(1, 10, 20);    // RX_FRAMES_LEN_64
    m_set_stat(1, 9, 1);      // RX_FRAMES_LEN_LT_64 (runts)
    m_set_stat(1, 73, 30);    // TX_FRAMES_LEN_64
    m_set_stat(1, 18, 5);     // RX 4096-8191
    m_set_stat(1, 19, 6);     // RX 8192-9215
    m_set_stat(1, 6, 7);      // RX pause
    std::string sock = e.dir + "/diag.sock";
    if (sock.size() >= 100) {   // test_ifcs.cpp sets the key only when the path fits a unix socket
        std::fprintf(stderr, "t_counters: scratch dir too long for the diag socket: diag checks SKIPPED\n");
        return;
    }
    std::string out = diag(sock, "counters 1");
    char b[64];
    auto want = [&](int sai, unsigned long long val) {
        std::snprintf(b, sizeof(b), "%d=%llu", sai, val);
        bool ok = out.find(std::string(" ") + b) != std::string::npos || out.find(std::string("\n") + b) != std::string::npos;
        if (!ok) std::fprintf(stderr, "counters: want %s\n", b);
        return ok;
    };
    CHECK(has(out, "devport 1:"));
    CHECK(want(SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES, 11));
    CHECK(want(SAI_PORT_STAT_IF_IN_FEC_NOT_CORRECTABLE_FRAMES, 12));
    CHECK(want(SAI_PORT_STAT_IF_IN_FEC_SYMBOL_ERRORS, 13));
    CHECK(want(SAI_PORT_STAT_PFC_3_RX_PKTS, 3));
    CHECK(want(SAI_PORT_STAT_PFC_3_TX_PKTS, 4));
    CHECK(want(SAI_PORT_STAT_ETHER_IN_PKTS_64_OCTETS, 21));            // 64 B + runts, as isai
    CHECK(want(SAI_PORT_STAT_ETHER_OUT_PKTS_64_OCTETS, 30));
    CHECK(want(SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS, 50));          // rx + tx
    CHECK(want(SAI_PORT_STAT_ETHER_IN_PKTS_4096_TO_9216_OCTETS, 11));
    CHECK(want(SAI_PORT_STAT_PAUSE_RX_PKTS, 7));
    CHECK(want(SAI_PORT_STAT_IF_OUT_OCTETS, 7777));
    CHECK(want(SAI_PORT_STAT_ETHER_STATS_OCTETS, 5555 + 9999));
    CHECK(has(out, "cpu queue 3: tx 103 bytes 6592 drops 3"));
    CHECK(has(diag(sock, "counters 999"), "unreadable"));

    // ---- CPU queue counters in the CoPP report, ACL report
    CHECK(has(diag(sock, "copp"), "cpu queue 0: tx 100"));
    CHECK(has(diag(sock, "acl"), "ACL: 0 tables, 0 entries"));
    CHECK(has(diag(sock, "help"), "counters <devport>"));
}

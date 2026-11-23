// SPDX-License-Identifier: Apache-2.0
// Table texts shared by the SAI-level tests (test_extphy.inc / test_macsec.inc, through
// test_sai.cpp) and the C++ unit tests of the table parsers (test_units.cpp). No library
// header: plain text builders.
#pragma once

#include <cstdio>
#include <string>

// The external-PHY table of test_ext_phy: ports with lanes 1 and 5, a link condition with every
// step kind, admin sequences on port 1.
static const char* const kExtPhyGood =
    "ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\n"
    "port 1\nlink\n"
    "  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n"
    "  C 10 1 a101 3 3\n"
    "admin-down\n  W 10 1 d139 351\n"
    "admin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\n"
    "port 5\nlink\n  C 11 1 a101 3 3\n";

// A made-up PHY: one register per thing, so the bus log reads like a story.
//   1.100 / 1.101   engine bypass (egress / ingress), 1.110 / 1.111 "reset"
//   1.120 / 1.121   gate (1 = open)
//   1.2d0..         SA install, d = direction (0 egress, 1 ingress): 2d0 = marker (first<<8 | cipher<<4 | an),
//                   2d1.. key / hash key / sci / pn / window / iv bytes
//   1.300 + an      SA removal (bit 15: "last"), ingress: read-modify-write of 1.310
//   1.4xx           counters, 1.500.. PN
inline std::string ms_table(bool gates = true, bool probe = true)
{
    std::string t = "ext-macsec 1\n# test table\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n";
    const char* dirs[2] = {"egress", "ingress"};
    const char* ciphers[4] = {"gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"};
    char b[256];
    for (int d = 0; d < 2; d++) {
        std::string dn = dirs[d];
        if (probe) t += "seq probe." + dn + "\nX p 1 10" + std::to_string(d) + " bypass[0]\n";
        t += "seq reset." + dn + "\nW p 1 11" + std::to_string(d) + " 1\n";
        t += "seq enable." + dn + "\nW p 1 10" + std::to_string(d) + " 0\nD 10\n";
        t += "seq disable." + dn + "\nW p 1 10" + std::to_string(d) + " 1\n";
        if (gates) {
            t += "seq gate-open." + dn + "\nW p 1 12" + std::to_string(d) + " 1\n";
            t += "seq gate-close." + dn + "\nW p 1 12" + std::to_string(d) + " 0\n";
        }
        t += "seq stats-flow." + dn + "\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 x_unknown[0]\n";
        for (int an = 0; an < 4; an++) {
            for (int c = 0; c < 4; c++) {
                bool xpn = c >= 2, k256 = c & 1;
                for (int first = 0; first < 2; first++) {
                    std::snprintf(b, sizeof(b), "seq sa-add%s.%s.%d.%s\nW p 1 2%d0 %x\n", first ? "-first" : "", dirs[d], an, ciphers[c], d,
                                  (first << 8) | (c << 4) | an);
                    t += b;
                    std::snprintf(b, sizeof(b), "W p 1 2%d1 key[1].key[0]\nW p 1 2%d2 key[%d].key[%d]\nW p 1 2%d3 hkey[15].hkey[0]\n", d, d,
                                  k256 ? 31 : 15, k256 ? 30 : 14, d);
                    t += b;
                    std::snprintf(b, sizeof(b), "W p 1 2%d4 sci[0].sci[7]\nW p 1 2%d5 pn[1].pn[0]\nW p 1 2%d6 %s\nW p 1 2%d7 window[1].window[0]\n", d, d, d,
                                  xpn ? "pn[5].pn[4]" : "0", d);
                    t += b;
                    if (xpn) {
                        std::snprintf(b, sizeof(b), "W p 1 2%d8 iv[0].iv[3]\nW p 1 2%d9 iv[4].iv[11]\nW p 1 2%da ssci[0].salt[0]\n", d, d, d);
                        t += b;
                    }
                    if (d == 1) t += "X p 1 310 $0\nW p 1 310 $0&ff00|" + std::string(1, (char)('1' + an)) + "\n";
                    std::snprintf(b, sizeof(b), "P p 1 2%df 1 1 3 0\n", d);
                    t += b;
                }
            }
            if (d == 1 && an == 0) t += "seq sa-add-first.ingress.0.gcm-aes-128.replay\nW p 1 210 f00\nW p 1 217 window[1].window[0]\nP p 1 21f 1 1 3 0\n";
            std::snprintf(b, sizeof(b), "seq sa-del.%s.%d\nW p 1 3%d%d 1\nseq sa-del-last.%s.%d\nW p 1 3%d%d 8001\n", dirs[d], an, d, an, dirs[d], an, d, an);
            t += b;
            std::snprintf(b, sizeof(b), "seq pn.%s.%d\nX p 1 5%d%d pn[0]\nX p 1 5%d%d pn[1]\nX p 1 5%d%d pn[2]\nX p 1 5%d%d pn[3]\n", dirs[d], an, d, an * 4,
                          d, an * 4 + 1, d, an * 4 + 2, d, an * 4 + 3);
            t += b;
            std::snprintf(b, sizeof(b), "seq stats-sa.%s.%d\nX p 1 4%d%d %s[0]\nX p 1 4%d%d %s[1]\n", dirs[d], an, d, an * 2,
                          d ? "in_pkts_ok" : "out_pkts_encrypted", d, an * 2 + 1, d ? "in_pkts_ok" : "out_pkts_encrypted");
            t += b;
        }
    }
    return t;
}

// SPDX-License-Identifier: Apache-2.0
// openbcm-ext-macsec: parse an external MACsec table (ext_macsec.h) as libsai
// will, and render sequences without hardware.
//   --check FILE                        parse; print what it holds
//   --render FILE SEQ PHY [PARAMS]      the bus operations of one sequence for PHY (hex), in order:
//                                       "W phy dev reg val", "R phy dev reg [want mask]", "D usec".
//                                       Reads that are compared return the expected value, others 0.
//                                       PARAMS: lines "<param> <hex bytes>" (key, hkey, sci, salt, ssci, iv, pn, window);
//                                       "-" = none.
//   --render FILE SEQ PHY PARAMS READS  the same, but the n-th read returns the n-th hex value of READS (a recorded
//                                       trace): compared reads are then really compared, kept values are the recording's.
// For build-time checks of generated board tables and for comparing a table
// with a recorded bus trace. The output of --render contains the key bytes.
#include "ext_macsec.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>

static bool slurp(const char* path, std::string& out)
{
    std::ifstream f(path);
    if (!f) return false;
    std::stringstream ss;
    ss << f.rdbuf();
    out = ss.str();
    return true;
}

int main(int argc, char** argv)
{
    bool check = argc == 3 && std::strcmp(argv[1], "--check") == 0;
    bool render = (argc >= 5 && argc <= 7) && std::strcmp(argv[1], "--render") == 0;
    if (!check && !render) {
        std::fprintf(stderr, "usage: %s --check FILE | --render FILE SEQ PHY [PARAMS|- [READS]]\n", argv[0]);
        return 2;
    }
    std::string text, err;
    if (!slurp(argv[2], text)) {
        std::fprintf(stderr, "%s: cannot open\n", argv[2]);
        return 1;
    }
    ob::MsTable t;
    if (!ob::ext_macsec_parse(text, t, err)) {
        std::fprintf(stderr, "%s: %s\n", argv[2], err.c_str());
        return 1;
    }
    if (check) {
        size_t ops = 0;
        for (auto& s : t.seqs) ops += s.second.size();
        std::printf("%s: %zu ports, %zu sequences, %zu operations, counters %s, ifg-bits %u\n", argv[2], t.ports.size(),
                    t.seqs.size(), ops, t.clear_on_read ? "clear-on-read" : "absolute", t.ifg_bits);
        return 0;
    }
    const std::vector<ob::MsOp>* seq = t.find(argv[3]);
    if (!seq) {
        std::fprintf(stderr, "%s: no sequence %s\n", argv[2], argv[3]);
        return 1;
    }
    unsigned phy = (unsigned)std::strtoul(argv[4], nullptr, 16);
    ob::MsParams par;
    std::vector<unsigned> reads;
    size_t next_read = 0;
    bool replay = argc == 7;
    if (replay) {
        std::string rtext;
        if (!slurp(argv[6], rtext)) {
            std::fprintf(stderr, "%s: cannot open\n", argv[6]);
            return 1;
        }
        std::istringstream in(rtext);
        for (std::string h; in >> h;) reads.push_back((unsigned)std::strtoul(h.c_str(), nullptr, 16));
    }
    if (argc >= 6 && std::strcmp(argv[5], "-") != 0) {
        std::string ptext;
        if (!slurp(argv[5], ptext)) {
            std::fprintf(stderr, "%s: cannot open\n", argv[5]);
            return 1;
        }
        std::istringstream in(ptext);
        std::string name, hex;
        while (in >> name >> hex) {
            std::vector<uint8_t> v;
            for (size_t i = 0; i + 1 < hex.size(); i += 2) v.push_back((uint8_t)std::strtoul(hex.substr(i, 2).c_str(), nullptr, 16));
            par[name] = v;
        }
    }
    ob::MsIo io;
    io.rd = [&](unsigned p, unsigned d, unsigned r, unsigned& v, const ob::MsOp& o) {
        if (replay) {
            v = next_read < reads.size() ? reads[next_read] : 0;
            next_read++;
            std::printf("R %03x %x %04x\n", p, d, r);
        } else if (o.op == 'E' || o.op == 'P') {
            v = o.want;
            std::printf("R %03x %x %04x %04x %04x\n", p, d, r, o.want, o.mask);
        } else {
            v = 0;
            std::printf("R %03x %x %04x\n", p, d, r);
        }
        return SAI_STATUS_SUCCESS;
    };
    io.wr = [](unsigned p, unsigned d, unsigned r, unsigned v) {
        std::printf("W %03x %x %04x %04x\n", p, d, r, v);
        return SAI_STATUS_SUCCESS;
    };
    io.delay = [](unsigned us) { std::printf("D %u\n", us); };
    sai_status_t st = ob::ext_macsec_exec(io, *seq, phy, par, nullptr, err);
    ob::ms_wipe(par);
    if (st != SAI_STATUS_SUCCESS) {
        std::fprintf(stderr, "%s: %s: %s\n", argv[2], argv[3], err.c_str());
        return 1;
    }
    return 0;
}

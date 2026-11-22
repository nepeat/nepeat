// SPDX-License-Identifier: Apache-2.0
// External MACsec table: parser and sequence executor (see ext_macsec.h).
// No SAI objects and no global state here: sai_macsec.cpp owns those.
#include "ext_macsec.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sstream>

namespace ob {

namespace {
bool num(const std::string& s, int base, unsigned long max, unsigned& out)
{
    if (s.empty() || s[0] == '-' || s[0] == '+') return false;
    char* end = nullptr;
    unsigned long v = std::strtoul(s.c_str(), &end, base);
    if (*end || v > max) return false;
    out = (unsigned)v;
    return true;
}

// Parameter name -> bytes it may have (0 = 16 or 32).
const std::map<std::string, unsigned>& param_sizes()
{
    static const std::map<std::string, unsigned> m = {{"key", 32}, {"hkey", 16}, {"sci", 8},  {"salt", 12},
                                                      {"ssci", 4}, {"iv", 12},   {"pn", 8},   {"window", 4}};
    return m;
}

// name[idx]
bool indexed(const std::string& s, std::string& name, unsigned& idx, unsigned max_idx)
{
    size_t b = s.find('[');
    if (b == std::string::npos || b == 0 || s.back() != ']') return false;
    name = s.substr(0, b);
    for (char ch : name)
        if (!(ch == '_' || (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9'))) return false;
    return num(s.substr(b + 1, s.size() - b - 2), 10, max_idx, idx);
}

bool parse_byte(const std::string& s, MsByte& b)
{
    unsigned v = 0;
    if (s.size() == 2 && num(s, 16, 0xff, v)) {
        b.param = false;
        b.c = (uint8_t)v;
        return true;
    }
    if (!indexed(s, b.name, b.idx, 31)) return false;
    auto it = param_sizes().find(b.name);
    if (it == param_sizes().end() || b.idx >= it->second) return false;
    b.param = true;
    return true;
}

bool parse_value(const std::string& s, MsVal& v)
{
    if (!s.empty() && s[0] == '$') {
        v.kind = MsVal::Kept;
        size_t a = s.find('&'), o = s.find('|');
        size_t e = std::min(a, o);
        if (!num(s.substr(1, e == std::string::npos ? e : e - 1), 10, 15, v.kept)) return false;
        if (a != std::string::npos) {
            if (o != std::string::npos && o < a) return false;
            if (!num(s.substr(a + 1, o == std::string::npos ? o : o - a - 1), 16, 0xffff, v.mask)) return false;
        }
        if (o != std::string::npos && !num(s.substr(o + 1), 16, 0xffff, v.bits)) return false;
        return true;
    }
    size_t dot = s.find('.');
    if (dot != std::string::npos) {
        v.kind = MsVal::Bytes;
        return parse_byte(s.substr(0, dot), v.hi) && parse_byte(s.substr(dot + 1), v.lo);
    }
    v.kind = MsVal::Const;
    return num(s, 16, 0xffff, v.c);
}

bool seq_name_ok(const std::string& s)
{
    if (s.empty() || s.size() > 80) return false;
    for (char ch : s)
        if (!(ch == '.' || ch == '-' || (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9'))) return false;
    return true;
}
} // namespace

bool ext_macsec_parse(const std::string& text, MsTable& t, std::string& err)
{
    t = MsTable();
    std::istringstream in(text);
    std::string line;
    unsigned n = 0;
    bool header = false;
    std::vector<MsOp>* sec = nullptr;
    auto fail = [&](const std::string& m) {
        err = "line " + std::to_string(n) + ": " + m;
        return false;
    };
    while (std::getline(in, line)) {
        n++;
        std::istringstream ls(line);
        std::vector<std::string> w;
        for (std::string x; ls >> x;) w.push_back(x);
        if (w.empty() || w[0][0] == '#') continue;
        if (!header) {
            if (w.size() != 2 || w[0] != "ext-macsec" || w[1] != "1")
                return fail("not an ext-macsec table (first line must be 'ext-macsec 1')");
            header = true;
            continue;
        }
        const std::string& k = w[0];
        if (k == "counters") {
            if (w.size() != 2 || (w[1] != "clear-on-read" && w[1] != "absolute")) return fail("counters clear-on-read | absolute");
            t.clear_on_read = w[1] == "clear-on-read";
            continue;
        }
        if (k == "ifg-bits") {
            if (w.size() != 2 || !num(w[1], 10, 2040, t.ifg_bits)) return fail("ifg-bits <decimal, 0..2040>");
            continue;
        }
        if (k == "port") {
            unsigned lane = 0, phy = 0;
            if (w.size() != 3 || !num(w[1], 10, 100000, lane) || !num(w[2], 16, 0xffff, phy)) return fail("port <first lane, decimal> <phy, hex>");
            if (t.ports.count(lane)) return fail("port " + w[1] + " twice");
            t.ports[lane] = phy;
            continue;
        }
        if (k == "seq") {
            if (w.size() != 2 || !seq_name_ok(w[1])) return fail("seq <name> (a-z 0-9 . -)");
            if (t.seqs.count(w[1])) return fail("sequence " + w[1] + " twice");
            sec = &t.seqs[w[1]];
            continue;
        }
        if (k.size() == 1 && std::string("WREPXD").find(k[0]) != std::string::npos) {
            if (!sec) return fail("operation outside a sequence");
            MsOp o;
            o.op = k[0];
            o.line = n;
            if (sec->size() >= 8192) return fail("sequence longer than 8192 operations");
            if (o.op == 'D') {
                if (w.size() != 2 || !num(w[1], 10, 100000, o.usec)) return fail("D <usec, decimal, 0..100000>");
                sec->push_back(o);
                continue;
            }
            size_t want = o.op == 'W' ? 5 : o.op == 'R' ? 4 : o.op == 'P' ? 8 : o.op == 'X' ? 5 : 0;
            if (o.op == 'E' ? (w.size() != 5 && w.size() != 6) : w.size() != want) return fail(std::string("wrong number of fields for ") + o.op);
            if (w[1] == "p") o.port_phy = true;
            else if (num(w[1], 16, 0xffff, o.phy)) o.port_phy = false;
            else return fail("phy: 'p' or hex");
            if (!num(w[2], 16, 31, o.dev) || !num(w[3], 16, 0xffff, o.reg)) return fail("dev / reg: hex, dev 1..1f, reg up to ffff");
            if (o.dev == 0) return fail("dev 0 (clause 22) is not allowed");
            if (o.op == 'W' && !parse_value(w[4], o.val))
                return fail("value: hhhh, $n[&mask][|bits] or <byte>.<byte> with bytes hh or <param>[<index>]");
            if (o.op == 'E' || o.op == 'P') {
                if (!num(w[4], 16, 0xffff, o.want)) return fail("value: hex up to ffff");
                if ((o.op == 'P' || w.size() == 6) && !num(w[5], 16, 0xffff, o.mask)) return fail("mask: hex up to ffff");
                if (o.want & ~o.mask) return fail("value has bits outside the mask: can never match");
            }
            if (o.op == 'P' && (!num(w[6], 10, 1000, o.tries) || o.tries < 1 || !num(w[7], 10, 100000, o.interval_us)))
                return fail("P: tries 1..1000 and interval 0..100000 usec, decimal");
            if (o.op == 'X') {
                unsigned kn = 0;
                if (w[4].size() > 1 && w[4][0] == '$' && num(w[4].substr(1), 10, 15, kn)) o.kept = (int)kn;
                else if (!indexed(w[4], o.res, o.res_idx, 7)) return fail("X destination: $<n> (0..15) or <result>[<word 0..7>]");
            }
            sec->push_back(o);
            continue;
        }
        return fail("unknown keyword '" + k + "'");
    }
    if (!header) {
        n = 1;
        return fail("empty table (first line must be 'ext-macsec 1')");
    }
    // A kept value must be read before it is written, in every sequence.
    for (auto& s : t.seqs) {
        bool have[16] = {false};
        for (auto& o : s.second) {
            n = o.line;
            if (o.op == 'X' && o.kept >= 0) have[o.kept] = true;
            if (o.op == 'W' && o.val.kind == MsVal::Kept && !have[o.val.kept])
                return fail("sequence " + s.first + ": $" + std::to_string(o.val.kept) + " is written before an X reads it");
        }
    }
    return true;
}

sai_status_t ext_macsec_exec(const MsIo& io, const std::vector<MsOp>& ops, unsigned phy, const MsParams& par, MsResult* res,
                             std::string& why)
{
    char b[200];
    auto byte_of = [&](const MsByte& x, uint8_t& out) {
        if (!x.param) {
            out = x.c;
            return true;
        }
        auto it = par.find(x.name);
        if (it == par.end() || x.idx >= it->second.size()) return false;
        out = it->second[x.idx];
        return true;
    };
    for (const MsOp& o : ops) {   // every parameter first: never start what cannot finish
        if (o.op != 'W' || o.val.kind != MsVal::Bytes) continue;
        uint8_t x;
        for (const MsByte* y : {&o.val.hi, &o.val.lo})
            if (!byte_of(*y, x)) {
                std::snprintf(b, sizeof(b), "line %u: parameter %s[%u] is not available", o.line, y->name.c_str(), y->idx);
                why = b;
                return SAI_STATUS_INVALID_PARAMETER;
            }
    }
    unsigned kept[16] = {0};
    for (const MsOp& o : ops) {
        unsigned p = o.port_phy ? phy : o.phy, v = 0;
        sai_status_t st = SAI_STATUS_SUCCESS;
        bool mismatch = false;
        switch (o.op) {
        case 'D':
            io.delay(o.usec);
            continue;
        case 'W': {
            if (o.val.kind == MsVal::Const) v = o.val.c;
            else if (o.val.kind == MsVal::Kept) v = (kept[o.val.kept] & o.val.mask) | o.val.bits;
            else {
                uint8_t hi = 0, lo = 0;
                byte_of(o.val.hi, hi);
                byte_of(o.val.lo, lo);
                v = ((unsigned)hi << 8) | lo;
            }
            st = io.wr(p, o.dev, o.reg, v);
            break;
        }
        case 'R':
            st = io.rd(p, o.dev, o.reg, v, o);
            break;
        case 'X':
            st = io.rd(p, o.dev, o.reg, v, o);
            if (st != SAI_STATUS_SUCCESS) break;
            if (o.kept >= 0) kept[o.kept] = v & 0xffff;
            else if (res) {
                auto& r = (*res)[o.res];
                if (r.size() <= o.res_idx) r.resize(o.res_idx + 1, 0);
                r[o.res_idx] = (uint16_t)v;
            }
            break;
        case 'E':
            st = io.rd(p, o.dev, o.reg, v, o);
            mismatch = st == SAI_STATUS_SUCCESS && (v & o.mask) != o.want;
            break;
        case 'P': {
            bool hit = false;
            for (unsigned i = 0; i < o.tries && !hit; i++) {
                if (i) io.delay(o.interval_us);
                st = io.rd(p, o.dev, o.reg, v, o);
                if (st != SAI_STATUS_SUCCESS) break;
                hit = (v & o.mask) == o.want;
            }
            mismatch = st == SAI_STATUS_SUCCESS && !hit;
            break;
        }
        default:
            break;
        }
        if (st != SAI_STATUS_SUCCESS) {
            std::snprintf(b, sizeof(b), "line %u: %c phy 0x%x %u.%04x: MDIO error %d", o.line, o.op, p, o.dev, o.reg, st);
            why = b;
            return st;
        }
        if (mismatch) {
            std::snprintf(b, sizeof(b), "line %u: %c phy 0x%x %u.%04x read 0x%04x, expected 0x%04x (mask 0x%04x, %u reads)", o.line,
                          o.op, p, o.dev, o.reg, v, o.want, o.mask, o.op == 'P' ? o.tries : 1);
            why = b;
            return SAI_STATUS_FAILURE;
        }
    }
    return SAI_STATUS_SUCCESS;
}

uint64_t ms_result_u64(const MsResult& r, const std::string& name)
{
    auto it = r.find(name);
    uint64_t v = 0;
    if (it == r.end()) return 0;
    for (size_t i = 0; i < it->second.size() && i < 4; i++) v |= (uint64_t)it->second[i] << (16 * i);
    return v;
}

void ms_wipe(MsParams& p)
{
    for (auto& e : p)
        if (!e.second.empty()) explicit_bzero(e.second.data(), e.second.size());
    p.clear();
}

} // namespace ob

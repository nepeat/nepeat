// SPDX-License-Identifier: Apache-2.0
// External MACsec table: parser and sequence executor (see ext_macsec.h).
// No SAI objects here: sai_macsec.cpp owns those. The only state is the
// operation trace (off by default, see "trace" below).
//
// Two engines, chosen at build time: the C++ one in this file (default), or,
// with -DOB_RUST_PHY_SEQ (make RUST_PHY_SEQ=...), the Rust crate phy-seq called
// through phy_seq.h; the structs of ext_macsec.h stay what the rest of the
// library and the tests use. The trace wraps the bus callbacks of whichever
// engine runs, so it is the same code for both.
#include "ext_macsec.h"

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <mutex>
#include <sstream>

#ifdef OB_RUST_PHY_SEQ
#include "phy_seq.h"
#endif

namespace ob {

#ifndef OB_RUST_PHY_SEQ
// ---- the C++ engine ----
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

namespace {
sai_status_t exec_engine(const MsIo& io, const std::vector<MsOp>& ops, unsigned phy, const MsParams& par, MsResult* res,
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
} // namespace

const char* ext_macsec_engine() { return "c++"; }

#else
// ---- the Rust engine (rust/sai/crates/phy-seq, through phy_seq.h) ----
namespace {
// index = PHY_SEQ_PARAM_*
const char* const kParam[] = {nullptr, "key", "hkey", "sci", "salt", "ssci", "iv", "pn", "window"};

unsigned param_id(const std::string& name)
{
    for (unsigned i = PHY_SEQ_PARAM_KEY; i <= PHY_SEQ_PARAM_WINDOW; i++)
        if (name == kParam[i]) return i;
    return PHY_SEQ_PARAM_NONE;
}

MsByte byte_from(uint8_t param, uint8_t value)
{
    MsByte b;
    if (param == PHY_SEQ_PARAM_NONE || param > PHY_SEQ_PARAM_WINDOW) {
        b.c = value;
    } else {
        b.param = true;
        b.name = kParam[param];
        b.idx = value;
    }
    return b;
}

// A parsed operation as the struct the C++ parser fills (fields an operation
// does not use keep their defaults, as there).
MsOp op_from(const phy_seq_op& f, const char* names)
{
    MsOp o;
    o.op = (char)f.op;
    o.line = f.line;
    if (f.op == 'D') {
        o.usec = f.usec;
        return o;
    }
    o.port_phy = f.port_phy != 0;
    o.phy = f.phy;
    o.dev = f.dev;
    o.reg = f.reg;
    switch (f.op) {
    case 'W':
        if (f.value_kind == PHY_SEQ_VALUE_BYTES) {
            o.val.kind = MsVal::Bytes;
            o.val.hi = byte_from(f.hi_param, f.hi_value);
            o.val.lo = byte_from(f.lo_param, f.lo_value);
        } else if (f.value_kind == PHY_SEQ_VALUE_KEPT) {
            o.val.kind = MsVal::Kept;
            o.val.kept = f.kept;
            o.val.mask = f.mask;
            o.val.bits = f.bits;
        } else {
            o.val.c = f.value;
        }
        break;
    case 'P':
        o.tries = f.tries;
        o.interval_us = f.usec;
        // fall through
    case 'E':
        o.want = f.value;
        o.mask = f.mask;
        break;
    case 'X':
        if (f.dest_kind == PHY_SEQ_DEST_RESULT) {
            o.res.assign(names + f.name_off, f.name_len);
            o.res_idx = f.word;
        } else {
            o.kept = f.kept;
        }
        break;
    default:
        break;
    }
    return o;
}

bool byte_to(const MsByte& b, uint8_t& param, uint8_t& value)
{
    if (!b.param) {
        param = PHY_SEQ_PARAM_NONE;
        value = b.c;
        return true;
    }
    param = (uint8_t)param_id(b.name);
    value = (uint8_t)b.idx;
    return param != PHY_SEQ_PARAM_NONE && b.idx <= 0xff;
}

// The operation for phy_seq_exec, which checks every field again; false when a
// value does not even fit the field (no parsed table has such an operation).
bool op_to(const MsOp& o, phy_seq_op& f, std::string& names)
{
    f = phy_seq_op();
    f.line = o.line;
    f.op = (uint8_t)o.op;
    f.tries = 1;
    f.mask = 0xffff;
    if (o.op == 'D') {
        f.usec = o.usec;
        return true;
    }
    if (o.dev > 0xff || o.reg > 0xffff || (!o.port_phy && o.phy > 0xffff)) return false;
    f.port_phy = o.port_phy ? 1 : 0;
    f.phy = (uint16_t)(o.port_phy ? 0 : o.phy);
    f.dev = (uint8_t)o.dev;
    f.reg = (uint16_t)o.reg;
    switch (o.op) {
    case 'W':
        if (o.val.kind == MsVal::Const) {
            if (o.val.c > 0xffff) return false;
            f.value_kind = PHY_SEQ_VALUE_CONST;
            f.value = (uint16_t)o.val.c;
        } else if (o.val.kind == MsVal::Kept) {
            if (o.val.kept > 0xff || o.val.mask > 0xffff || o.val.bits > 0xffff) return false;
            f.value_kind = PHY_SEQ_VALUE_KEPT;
            f.kept = (uint8_t)o.val.kept;
            f.mask = (uint16_t)o.val.mask;
            f.bits = (uint16_t)o.val.bits;
        } else {
            f.value_kind = PHY_SEQ_VALUE_BYTES;
            if (!byte_to(o.val.hi, f.hi_param, f.hi_value) || !byte_to(o.val.lo, f.lo_param, f.lo_value)) return false;
        }
        return true;
    case 'P':
        f.tries = o.tries;
        f.usec = o.interval_us;
        // fall through
    case 'E':
        if (o.want > 0xffff || o.mask > 0xffff) return false;
        f.value = (uint16_t)o.want;
        f.mask = (uint16_t)o.mask;
        return true;
    case 'X':
        if (o.kept >= 0) {
            if (o.kept > 0xff) return false;
            f.dest_kind = PHY_SEQ_DEST_KEPT;
            f.kept = (uint8_t)o.kept;
        } else {
            if (o.res_idx > 0xff) return false;
            f.dest_kind = PHY_SEQ_DEST_RESULT;
            f.word = (uint8_t)o.res_idx;
            f.name_off = (uint32_t)names.size();
            f.name_len = (uint32_t)o.res.size();
            names += o.res;
        }
        return true;
    default:
        return true;   // 'R'; an unknown letter is refused by phy_seq_exec
    }
}

// What the callbacks of one phy_seq_exec call work on. The callbacks are
// called by Rust code: no exception may leave them.
struct RsRun {
    const MsIo* io;
    const std::vector<MsOp>* ops;
    MsResult* res;
};

extern "C" {
static int32_t rs_read(void* ctx, uint32_t op, uint16_t phy, uint8_t dev, uint16_t reg, uint16_t* value) noexcept
{
    RsRun& r = *static_cast<RsRun*>(ctx);
    try {
        if (op >= r.ops->size()) return SAI_STATUS_FAILURE;
        unsigned v = 0;
        sai_status_t st = r.io->rd(phy, dev, reg, v, (*r.ops)[op]);
        *value = (uint16_t)v;
        return st;
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
}

static int32_t rs_write(void* ctx, uint32_t, uint16_t phy, uint8_t dev, uint16_t reg, uint16_t value) noexcept
{
    RsRun& r = *static_cast<RsRun*>(ctx);
    try {
        return r.io->wr(phy, dev, reg, value);
    } catch (...) {
        return SAI_STATUS_FAILURE;
    }
}

static void rs_delay(void* ctx, uint32_t, uint32_t usec) noexcept
{
    RsRun& r = *static_cast<RsRun*>(ctx);
    try {
        r.io->delay(usec);
    } catch (...) {
    }
}

static void rs_result(void* ctx, const char* name, size_t name_len, uint32_t word, uint16_t value) noexcept
{
    RsRun& r = *static_cast<RsRun*>(ctx);
    try {
        auto& w = (*r.res)[std::string(name, name_len)];
        if (w.size() <= word) w.resize(word + 1, 0);
        w[word] = value;
    } catch (...) {
    }
}
} // extern "C"

sai_status_t exec_engine(const MsIo& io, const std::vector<MsOp>& ops, unsigned phy, const MsParams& par, MsResult* res,
                         std::string& why)
{
    char msg[256];
    if (phy > 0xffff) {
        std::snprintf(msg, sizeof(msg), "phy 0x%x is not an MDIO address word", phy);
        why = msg;
        return SAI_STATUS_INVALID_PARAMETER;
    }
    std::vector<phy_seq_op> flat(ops.size());
    std::string names;
    for (size_t i = 0; i < ops.size(); i++)
        if (!op_to(ops[i], flat[i], names)) {
            std::snprintf(msg, sizeof(msg), "line %u: operation %c cannot be expressed", ops[i].line, ops[i].op);
            why = msg;
            return SAI_STATUS_FAILURE;
        }
    // The parameters are passed by pointer: the Rust side makes its own copy and wipes it.
    std::vector<phy_seq_param> params;
    for (auto& p : par) {
        unsigned id = param_id(p.first);
        if (id == PHY_SEQ_PARAM_NONE) continue;   // a name no table can use
        phy_seq_param pp;
        pp.bytes = p.second.data();
        pp.len = p.second.size();
        pp.param = (uint8_t)id;
        params.push_back(pp);
    }
    RsRun run = {&io, &ops, res};
    phy_seq_io cio = {&run, rs_read, rs_write, rs_delay, res ? rs_result : nullptr};
    phy_seq_outcome out = {0, 0, 0};
    msg[0] = 0;
    int32_t rc = phy_seq_exec(PHY_SEQ_FORMAT_EXT_MACSEC, flat.data(), flat.size(), names.data(), names.size(), (uint16_t)phy,
                              params.data(), params.size(), &cio, &out, msg, sizeof(msg));
    if (rc == PHY_SEQ_OK) return SAI_STATUS_SUCCESS;
    why = msg;
    switch (rc) {
    case PHY_SEQ_E_PARAM:
        return SAI_STATUS_INVALID_PARAMETER;
    case PHY_SEQ_E_BUS:
        return out.bus_status ? (sai_status_t)out.bus_status : SAI_STATUS_FAILURE;
    case PHY_SEQ_E_FAULT:
        return SAI_STATUS_FAILURE;
    default:
        if (why.empty()) why = "phy-seq: error " + std::to_string(rc);
        return SAI_STATUS_FAILURE;
    }
}
} // namespace

bool ext_macsec_parse(const std::string& text, MsTable& t, std::string& err)
{
    t = MsTable();
    if (phy_seq_abi() != PHY_SEQ_ABI) {
        err = "phy-seq library has interface version " + std::to_string(phy_seq_abi()) + ", this build expects " + std::to_string(PHY_SEQ_ABI);
        return false;
    }
    phy_seq_table* tab = nullptr;
    char msg[1024];
    int32_t rc = phy_seq_parse(PHY_SEQ_FORMAT_EXT_MACSEC, (const uint8_t*)text.data(), text.size(), &tab, msg, sizeof(msg));
    if (rc != PHY_SEQ_OK) {
        err = rc == PHY_SEQ_E_PARSE ? std::string(msg) : "phy-seq: error " + std::to_string(rc);
        return false;
    }
    struct Free {
        phy_seq_table* t;
        ~Free() { phy_seq_free(t); }
    } free_it{tab};
    phy_seq_info info;
    if ((rc = phy_seq_table_info(tab, &info)) != PHY_SEQ_OK) {
        err = "phy-seq: error " + std::to_string(rc);
        return false;
    }
    MsTable out;
    out.clear_on_read = info.clear_on_read != 0;
    out.ifg_bits = info.ifg_bits;
    for (uint32_t i = 0; i < info.ports; i++) {
        phy_seq_port p;
        if ((rc = phy_seq_table_port(tab, i, &p)) != PHY_SEQ_OK) {
            err = "phy-seq: error " + std::to_string(rc);
            return false;
        }
        out.ports[p.lane] = p.phy;
    }
    for (uint32_t i = 0; i < info.sequences; i++) {
        phy_seq_seq s;
        if ((rc = phy_seq_table_seq(tab, i, &s)) != PHY_SEQ_OK) {
            err = "phy-seq: error " + std::to_string(rc);
            return false;
        }
        std::vector<MsOp>& ops = out.seqs[std::string(s.name, s.name_len)];
        ops.reserve(s.n_ops);
        for (size_t k = 0; k < s.n_ops; k++) ops.push_back(op_from(s.ops[k], s.names));
    }
    t = std::move(out);
    return true;
}

const char* ext_macsec_engine() { return "rust"; }
#endif

// ---- trace: the bus operations of sequence runs, for comparing engines and
// builds on hardware (diag socket "extmacsec trace ...", ext_macsec.h). Off by
// default. It wraps the caller's bus callbacks, so it records what reached the
// bus whichever engine ran. A word made of key material is never recorded:
// such a write shows as "****". Read values are not recorded either (counters
// and packet numbers differ from run to run, and a chip may read keys back).
namespace {
struct Trace {
    std::mutex mu;
    std::deque<std::string> lines;
    uint64_t runs = 0, dropped = 0;
};
Trace& g_trace = *new Trace;            // never destroyed: syncd's threads may still run at exit()
std::atomic<int> g_trace_mode{0};       // 0 off, 1 sequences that write, 2 every sequence
const size_t kTraceLines = 50000;       // the oldest lines are dropped beyond this

// SAK, hash key, salt and what is derived from them (iv = salt ^ ssci): never shown.
bool secret_byte(const MsByte& b) { return b.param && (b.name == "key" || b.name == "hkey" || b.name == "salt" || b.name == "iv"); }
bool secret_value(const MsVal& v) { return v.kind == MsVal::Bytes && (secret_byte(v.hi) || secret_byte(v.lo)); }
} // namespace

sai_status_t ext_macsec_exec(const MsIo& io, const std::vector<MsOp>& ops, unsigned phy, const MsParams& par, MsResult* res,
                             std::string& why)
{
    int mode = g_trace_mode.load(std::memory_order_relaxed);
    if (mode == 1) {
        bool writes = false;
        for (auto& o : ops) writes = writes || o.op == 'W';
        if (!writes) mode = 0;
    }
    if (mode == 0) return exec_engine(io, ops, phy, par, res, why);

    std::vector<std::string> lines;
    char b[96];
    // A write belongs to the next W of the sequence: the operations run in table
    // order and a W writes once. Past the end (cannot happen): not shown.
    size_t next_w = 0;
    MsIo tio;
    tio.rd = [&](unsigned p, unsigned d, unsigned r, unsigned& v, const MsOp& o) {
        std::snprintf(b, sizeof(b), "R %03x %x %04x", p, d, r);
        lines.push_back(b);
        return io.rd(p, d, r, v, o);
    };
    tio.wr = [&](unsigned p, unsigned d, unsigned r, unsigned v) {
        while (next_w < ops.size() && ops[next_w].op != 'W') next_w++;
        bool show = next_w < ops.size() && !secret_value(ops[next_w].val);
        next_w++;
        if (show) std::snprintf(b, sizeof(b), "W %03x %x %04x %04x", p, d, r, v);
        else std::snprintf(b, sizeof(b), "W %03x %x %04x ****", p, d, r);
        lines.push_back(b);
        return io.wr(p, d, r, v);
    };
    tio.delay = [&](unsigned us) {
        std::snprintf(b, sizeof(b), "D %u", us);
        lines.push_back(b);
        io.delay(us);
    };
    sai_status_t st = exec_engine(tio, ops, phy, par, res, why);

    std::lock_guard<std::mutex> g(g_trace.mu);
    std::snprintf(b, sizeof(b), "# run %llu: phy 0x%x, %zu operations from line %u", (unsigned long long)++g_trace.runs, phy,
                  ops.size(), ops.empty() ? 0 : ops[0].line);
    g_trace.lines.push_back(b);
    for (auto& l : lines) g_trace.lines.push_back(std::move(l));
    std::snprintf(b, sizeof(b), "= %d", st);
    g_trace.lines.push_back(b);
    while (g_trace.lines.size() > kTraceLines) {
        g_trace.lines.pop_front();
        g_trace.dropped++;
    }
    return st;
}

std::string ext_macsec_trace(const std::string& args)
{
    std::istringstream is(args);
    std::string cmd, arg;
    is >> cmd >> arg;
    bool dump = cmd == "dump";
    if (cmd == "on") g_trace_mode.store(arg == "all" ? 2 : 1);
    else if (cmd == "off") g_trace_mode.store(0);
    else if (!cmd.empty() && cmd != "clear" && !dump)
        return "usage: extmacsec trace [on [all] | off | dump | clear]   (on: sequences that write; on all: every sequence)\n";
    std::lock_guard<std::mutex> g(g_trace.mu);
    if (cmd == "clear") {
        g_trace.lines.clear();
        g_trace.runs = g_trace.dropped = 0;
    }
    int mode = g_trace_mode.load();
    char b[200];
    std::snprintf(b, sizeof(b), "extmacsec trace: %s, engine %s, %llu runs, %zu lines kept, %llu dropped\n",
                  mode == 0 ? "off" : mode == 1 ? "on (sequences that write)" : "on (all sequences)", ext_macsec_engine(),
                  (unsigned long long)g_trace.runs, g_trace.lines.size(), (unsigned long long)g_trace.dropped);
    std::string s = b;
    if (dump)
        for (auto& l : g_trace.lines) s += l + "\n";
    return s;
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

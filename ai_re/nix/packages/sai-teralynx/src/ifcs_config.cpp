// SPDX-License-Identifier: Apache-2.0
#include "ifcs_config.h"
#include "ifcs_api.h"

#include <cerrno>
#include <cstdlib>
#include <fstream>
#include <set>
#include <sstream>

namespace tl {

static std::string trim(const std::string& s)
{
    size_t a = s.find_first_not_of(" \t\r\n");
    if (a == std::string::npos) return "";
    size_t b = s.find_last_not_of(" \t\r\n");
    return s.substr(a, b - a + 1);
}

static bool to_int(const std::string& s, int64_t& v)
{
    // Decimal only, like Python's int() in the vendor CLI setters.
    if (s.empty()) return false;
    errno = 0;
    char* end = nullptr;
    long long x = std::strtoll(s.c_str(), &end, 10);
    if (errno != 0 || !end || *end != '\0') return false;
    v = x;
    return true;
}

bool parse_port_map(const std::string& path, std::vector<PortSpec>& out, std::string& err)
{
    std::ifstream f(path);
    if (!f) {
        err = path + ": cannot open";
        return false;
    }
    std::set<uint32_t> seen_dp, seen_lane;
    std::string line;
    for (int n = 1; std::getline(f, line); n++) {
        std::string where = path + ":" + std::to_string(n) + ": ";
        size_t h = line.find('#');
        if (h != std::string::npos) line.resize(h);
        line = trim(line);
        if (line.empty()) continue;
        std::istringstream is(line);
        std::string dp, lanes, speed, fec, extra;
        if (!(is >> dp >> lanes >> speed >> fec) || (is >> extra)) {
            err = where + "expected \"devport lanes speed_mbps fec\"";
            return false;
        }
        PortSpec p;
        int64_t v;
        if (!to_int(dp, v) || v < 1 || v > 1023) { err = where + "bad devport '" + dp + "'"; return false; }
        p.devport = (uint32_t)v;
        if (!seen_dp.insert(p.devport).second) { err = where + "devport " + dp + " listed twice"; return false; }
        std::stringstream ls(lanes);
        std::string l;
        while (std::getline(ls, l, ',')) {
            if (!to_int(l, v) || v < 1 || v > 256) { err = where + "bad lane '" + l + "' (1..256)"; return false; }
            if (!seen_lane.insert((uint32_t)v).second) { err = where + "lane " + l + " used twice"; return false; }
            p.lanes.push_back((uint32_t)v);
        }
        if (p.lanes.empty() || p.lanes.size() > 8) { err = where + "1..8 lanes expected"; return false; }
        if (!to_int(speed, v) || v <= 0 || v > 400000) { err = where + "bad speed '" + speed + "'"; return false; }
        p.speed = (uint32_t)v;
        if (fec != "none" && fec != "rs" && fec != "fc") { err = where + "fec must be none|rs|fc"; return false; }
        p.fec = fec;
        out.push_back(p);
    }
    if (out.empty()) {
        err = path + ": no ports";
        return false;
    }
    return true;
}

namespace {
struct AttrDef { const char* name; uint32_t id; ValKind kind; };
// Types from the vendor CLI's devport attribute list (ifcs_cmds/devport.py)
// and its setters: 'enum'/'data'/'u32' are written to value.u32.
const AttrDef k_attrs[] = {
    {"rx_eq_fine_tune", IFCS_DEVPORT_ATTR_RX_EQ_FINE_TUNE, ValKind::U32},
    {"link_training", IFCS_DEVPORT_ATTR_LINK_TRAINING, ValKind::U32},
    {"tx_eq_pre1", IFCS_DEVPORT_ATTR_TX_EQ_PRE1, ValKind::I8List},
    {"tx_eq_pre2", IFCS_DEVPORT_ATTR_TX_EQ_PRE2, ValKind::I8List},
    {"tx_eq_pre3", IFCS_DEVPORT_ATTR_TX_EQ_PRE3, ValKind::I8List},
    {"tx_eq_attn", IFCS_DEVPORT_ATTR_TX_EQ_ATTN, ValKind::I8List},
    {"tx_eq_post", IFCS_DEVPORT_ATTR_TX_EQ_POST, ValKind::I8List},
    {"rx_ctle_lf", IFCS_DEVPORT_ATTR_RX_CTLE_LF, ValKind::U8List},
    {"rx_ctle_hf", IFCS_DEVPORT_ATTR_RX_CTLE_HF, ValKind::U8List},
    {"rx_ctle_bw", IFCS_DEVPORT_ATTR_RX_CTLE_BW, ValKind::U8List},
    {"rx_gainshape1", IFCS_DEVPORT_ATTR_RX_GAINSHAPE1, ValKind::U32List},
    {"rx_gainshape2", IFCS_DEVPORT_ATTR_RX_GAINSHAPE2, ValKind::U32List},
    {"rx_agc_target", IFCS_DEVPORT_ATTR_RX_AGC_TARGET, ValKind::U32List},
    {"rx_eye_disqualify_threshold_25g", IFCS_DEVPORT_ATTR_RX_EYE_DISQUALIFY_THRESHOLD_25G, ValKind::U32List},
};
} // namespace

bool devport_attr_lookup(const std::string& name, uint32_t& id, ValKind& kind)
{
    for (const auto& a : k_attrs)
        if (name == a.name) {
            id = a.id;
            kind = a.kind;
            return true;
        }
    return false;
}

bool parse_init_cmds(const std::string& path, InitCmds& out, std::string& err)
{
    std::ifstream f(path);
    if (!f) {
        err = path + ": cannot open";
        return false;
    }
    std::string line;
    for (int n = 1; std::getline(f, line); n++) {
        std::string where = path + ":" + std::to_string(n);
        line = trim(line);
        if (line.empty() || line[0] == '#') continue;
        std::istringstream is(line);
        std::vector<std::string> tok;
        std::string t;
        while (is >> t) tok.push_back(t);
        bool ours = tok.size() >= 3 && tok[0] == "ifcs" && tok[1] == "set" && tok[2] == "devport";
        if (!ours) {
            std::string kind = tok[0] + (tok.size() > 1 ? " " + tok[1] : "");
            if (tok.size() > 2 && tok[0] == "ifcs") kind += " " + tok[2];
            out.skipped[kind]++;
            continue;
        }
        if (tok.size() < 5) {
            err = where + ": expected \"ifcs set devport <id> <attr> <value>\"";
            return false;
        }
        DevportCmd c;
        c.where = where;
        c.name = tok[4];
        if (!devport_attr_lookup(c.name, c.attr, c.kind)) {
            out.skipped["ifcs set devport " + c.name]++;
            continue;
        }
        int64_t v;
        if (!to_int(tok[3], v) || v < 0 || v > 1023) {
            err = where + ": bad devport '" + tok[3] + "'";
            return false;
        }
        c.devport = (uint32_t)v;
        // The value may contain spaces inside the brackets: rejoin the rest.
        std::string val;
        for (size_t i = 5; i < tok.size(); i++) val += tok[i];
        if (val.empty()) {
            err = where + ": " + c.name + ": missing value";
            return false;
        }
        if (c.kind == ValKind::U32) {
            if (!to_int(val, v) || v < 0 || v > 0xffffffffLL) {
                err = where + ": " + c.name + ": bad value '" + val + "'";
                return false;
            }
            c.values.push_back(v);
        } else {
            if (val.size() < 2 || val.front() != '[' || val.back() != ']') {
                err = where + ": " + c.name + ": list value [a,b,...] expected";
                return false;
            }
            std::stringstream ls(val.substr(1, val.size() - 2));
            std::string e;
            while (std::getline(ls, e, ',')) {
                int64_t lo = c.kind == ValKind::I8List ? -128 : 0;
                int64_t hi = c.kind == ValKind::I8List ? 127 : c.kind == ValKind::U8List ? 255 : 0xffffffffLL;
                if (!to_int(e, v) || v < lo || v > hi) {
                    err = where + ": " + c.name + ": bad list element '" + e + "'";
                    return false;
                }
                c.values.push_back(v);
            }
            if (c.values.empty() || c.values.size() > 8) {
                err = where + ": " + c.name + ": 1..8 list elements expected";
                return false;
            }
        }
        out.cmds.push_back(c);
    }
    return true;
}

} // namespace tl

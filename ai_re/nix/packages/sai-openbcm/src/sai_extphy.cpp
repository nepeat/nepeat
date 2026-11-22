// SPDX-License-Identifier: Apache-2.0
// External PHY table: see ext_phy.h. Sequences run under one mutex (link
// polls come from the backend's link scan thread, admin sequences from SAI
// calls, raw MDIO of the diag socket from its thread). Lock order: SAI lock
// -> g_mu -> the bus master's own lock; nothing here calls back into the SAI
// layer or a port API of the backend while holding g_mu.
#include "ext_phy.h"
#include "sai_internal.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <mutex>
#include <sstream>
#include <thread>

namespace ob {

namespace {
std::mutex g_mu;
ExtPhyTable g_tab;
std::map<int, size_t> g_by_hw;
Backend* g_be = nullptr;
uint64_t g_hold_until = 0;
uint64_t g_holdoffs = 0;

uint64_t now_us()
{
    return (uint64_t)std::chrono::duration_cast<std::chrono::microseconds>(
               std::chrono::steady_clock::now().time_since_epoch()).count();
}

bool num(const std::string& s, int base, unsigned long max, unsigned& out)
{
    if (s.empty() || s[0] == '-' || s[0] == '+') return false;
    char* end = nullptr;
    unsigned long v = std::strtoul(s.c_str(), &end, base);
    if (*end || v > max) return false;
    out = (unsigned)v;
    return true;
}

struct SeqResult {
    bool fault = false;      // E / P mismatch or MDIO error
    bool cond = true;        // all C held (meaningful without fault)
    std::string why;
    unsigned last = 0;
};

// Short waits are spun: appliance kernels round a sleep up to a 4 ms tick.
void wait_us(unsigned us)
{
    if (us == 0) return;
    if (us > 2000) {
        std::this_thread::sleep_for(std::chrono::microseconds(us));
        return;
    }
    uint64_t end = now_us() + us;
    while (now_us() < end) {}
}

std::string op_str(const ExtOp& o)
{
    char b[96];
    std::snprintf(b, sizeof(b), "line %u: %c phy 0x%x %u.%04x", o.line, o.op, o.phy, o.dev, o.reg);
    return b;
}

SeqResult run_seq(Backend& be, const std::vector<ExtOp>& ops)
{
    SeqResult r;
    char b[160];
    for (const ExtOp& o : ops) {
        unsigned v = 0;
        sai_status_t st = SAI_STATUS_SUCCESS;
        switch (o.op) {
        case 'W':
            st = be.mdio_c45_write(o.phy, o.dev, o.reg, o.val);
            break;
        case 'R':
            st = be.mdio_c45_read(o.phy, o.dev, o.reg, v);
            break;
        case 'E':
            st = be.mdio_c45_read(o.phy, o.dev, o.reg, v);
            if (st == SAI_STATUS_SUCCESS && (v & o.mask) != o.val) {
                std::snprintf(b, sizeof(b), "%s read 0x%04x, expected 0x%04x (mask 0x%04x)", op_str(o).c_str(), v, o.val, o.mask);
                r.fault = true;
                r.why = b;
                return r;
            }
            break;
        case 'P': {
            bool hit = false;
            for (unsigned i = 0; i < o.tries && !hit; i++) {
                if (i) wait_us(o.interval_us);
                st = be.mdio_c45_read(o.phy, o.dev, o.reg, v);
                if (st != SAI_STATUS_SUCCESS) break;
                hit = (v & o.mask) == o.val;
            }
            if (st == SAI_STATUS_SUCCESS && !hit) {
                std::snprintf(b, sizeof(b), "%s never read 0x%04x (mask 0x%04x) in %u reads, last 0x%04x", op_str(o).c_str(),
                              o.val, o.mask, o.tries, v);
                r.fault = true;
                r.why = b;
                return r;
            }
            break;
        }
        case 'C':
            st = be.mdio_c45_read(o.phy, o.dev, o.reg, v);
            if (st == SAI_STATUS_SUCCESS) {
                r.last = v;
                if ((v & o.mask) != o.val) r.cond = false;
            }
            break;
        default:
            break;
        }
        if (st != SAI_STATUS_SUCCESS) {
            std::snprintf(b, sizeof(b), "%s: MDIO error %d", op_str(o).c_str(), st);
            r.fault = true;
            r.why = b;
            return r;
        }
    }
    return r;
}

ExtPort* port_of_hw(int hw)
{
    auto it = g_by_hw.find(hw);
    return it == g_by_hw.end() ? nullptr : &g_tab.ports[it->second];
}
} // namespace

bool ext_phy_parse(const std::string& text, ExtPhyTable& t, std::string& err)
{
    t = ExtPhyTable();
    std::istringstream in(text);
    std::string line;
    unsigned n = 0;
    bool header = false;
    std::vector<ExtOp>* sec = nullptr;
    bool in_link = false;
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
            if (w.size() != 2 || w[0] != "ext-phy" || w[1] != "1") return fail("not an ext-phy table (first line must be 'ext-phy 1')");
            header = true;
            continue;
        }
        const std::string& k = w[0];
        if (k == "poll-ms" || k == "up-polls" || k == "holdoff-ms") {
            if (!t.ports.empty()) return fail(k + " after the first port");
            unsigned v = 0;
            bool ok = w.size() == 2;
            if (k == "poll-ms") ok = ok && num(w[1], 10, 60000, v);
            else if (k == "up-polls") ok = ok && num(w[1], 10, 100, v) && v >= 1;
            else ok = ok && num(w[1], 10, 600000, v);
            if (!ok) return fail(k + ": one decimal number (poll-ms 0..60000, up-polls 1..100, holdoff-ms 0..600000)");
            (k == "poll-ms" ? t.poll_ms : k == "up-polls" ? t.up_polls : t.holdoff_ms) = v;
            continue;
        }
        if (k == "port") {
            unsigned lane = 0;
            if (w.size() != 2 || !num(w[1], 10, 100000, lane)) return fail("port <first lane, decimal>");
            for (auto& p : t.ports)
                if (p.lane == lane) return fail("port " + w[1] + " twice (first at line " + std::to_string(p.line) + ")");
            ExtPort p;
            p.lane = lane;
            p.line = n;
            t.ports.push_back(p);
            sec = nullptr;
            continue;
        }
        if (k == "link" || k == "admin-down" || k == "admin-up") {
            if (t.ports.empty()) return fail(k + " before any port");
            if (w.size() != 1) return fail(k + " takes no argument");
            ExtPort& p = t.ports.back();
            sec = k == "link" ? &p.link : k == "admin-down" ? &p.down : &p.up;
            if (!sec->empty()) return fail(k + " twice in port " + std::to_string(p.lane));
            in_link = k == "link";
            continue;
        }
        if (k.size() == 1 && std::string("WREPC").find(k[0]) != std::string::npos) {
            if (!sec) return fail("operation outside a link / admin-down / admin-up sequence");
            ExtOp o;
            o.op = k[0];
            o.line = n;
            size_t want = o.op == 'W' ? 5 : o.op == 'R' ? 4 : o.op == 'P' ? 8 : o.op == 'C' ? 6 : 0;
            if (o.op == 'E' ? (w.size() != 5 && w.size() != 6) : w.size() != want) return fail(std::string("wrong number of fields for ") + o.op);
            if (!num(w[1], 16, 0xffff, o.phy) || !num(w[2], 16, 31, o.dev) || !num(w[3], 16, 0xffff, o.reg))
                return fail("phy / dev / reg: hex, dev 1..1f, reg up to ffff");
            if (o.dev == 0) return fail("dev 0 (clause 22) is not allowed");
            if (o.op != 'R' && !num(w[4], 16, 0xffff, o.val)) return fail("value: hex up to ffff");
            if ((o.op == 'P' || o.op == 'C' || (o.op == 'E' && w.size() == 6)) && !num(w[5], 16, 0xffff, o.mask))
                return fail("mask: hex up to ffff");
            if (o.op == 'P' && (!num(w[6], 10, 1000, o.tries) || o.tries < 1 || !num(w[7], 10, 100000, o.interval_us)))
                return fail("P: tries 1..1000 and interval 0..100000 usec, decimal");
            if (o.op != 'W' && o.op != 'R' && (o.val & ~o.mask)) return fail("value has bits outside the mask: can never match");
            if (o.op == 'C' && !in_link) return fail("C (link condition) outside a link sequence");
            if (sec->size() >= 4096) return fail("sequence longer than 4096 operations");
            sec->push_back(o);
            continue;
        }
        return fail("unknown keyword '" + k + "'");
    }
    if (!header) {
        n = 1;
        return fail("empty table (first line must be 'ext-phy 1')");
    }
    for (auto& p : t.ports) {
        n = p.line;
        if (p.link.empty() && p.down.empty() && p.up.empty()) return fail("port " + std::to_string(p.lane) + " has no sequence");
        bool c = false;
        for (auto& o : p.link) c = c || o.op == 'C';
        if (!p.link.empty() && !c) return fail("port " + std::to_string(p.lane) + ": link sequence without a C condition");
    }
    return true;
}

sai_status_t ext_phy_load(const BackendConfig& cfg)
{
    std::lock_guard<std::mutex> g(g_mu);
    g_tab = ExtPhyTable();
    g_by_hw.clear();
    g_be = nullptr;
    g_hold_until = 0;
    g_holdoffs = 0;
    std::string path = cfg.get("SAI_OPENBCM_EXT_PHY_FILE");
    if (path.empty()) return SAI_STATUS_SUCCESS;
    std::ifstream f(path);
    if (!f) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_PHY_FILE %s: cannot open; refusing to create the switch", path.c_str());
        return SAI_STATUS_FAILURE;
    }
    std::stringstream ss;
    ss << f.rdbuf();
    std::string err;
    if (!ext_phy_parse(ss.str(), g_tab, err)) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_PHY_FILE %s: %s; refusing to create the switch", path.c_str(), err.c_str());
        g_tab = ExtPhyTable();
        return SAI_STATUS_INVALID_PARAMETER;
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t ext_phy_attach(Backend& be)
{
    std::vector<int> gated;
    std::vector<PortInfo> ports = be.ports();   // before g_mu: the backend's own lock comes first elsewhere
    {
        std::lock_guard<std::mutex> g(g_mu);
        if (g_tab.ports.empty()) return SAI_STATUS_SUCCESS;
        for (size_t i = 0; i < g_tab.ports.size(); i++) {
            ExtPort& p = g_tab.ports[i];
            for (auto& pi : ports)
                if (!pi.lanes.empty() && pi.lanes[0] == p.lane) {
                    p.hw = pi.hw;
                    p.name = pi.name;
                }
            if (p.hw < 0) {
                OB_ERR(SAI_API_SWITCH, "external PHY table line %u: no port starts at lane %u", p.line, p.lane);
                return SAI_STATUS_INVALID_PARAMETER;
            }
            g_by_hw[p.hw] = i;
            if (!p.link.empty()) gated.push_back(p.hw);
        }
        g_be = &be;
    }
    unsigned v = 0;
    if (be.mdio_c45_read(0, 1, 0, v) == SAI_STATUS_NOT_SUPPORTED) {
        OB_ERR(SAI_API_SWITCH, "external PHY table: backend %s has no MDIO access", be.name());
        ext_phy_detach();
        return SAI_STATUS_NOT_SUPPORTED;
    }
    for (int hw : gated) {
        sai_status_t st = be.port_link_gate(hw, [hw]() { return ext_phy_gate(hw); });
        if (st != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_SWITCH, "external PHY table: hw port %d: the backend cannot gate its link (%d)", hw, st);
            ext_phy_detach();
            return st;
        }
    }
    OB_NOTICE(SAI_API_SWITCH, "external PHY table: %zu ports (%zu with a line-side link poll), poll %u ms, up after %u good polls, "
              "hold-off %u ms", g_tab.ports.size(), gated.size(), g_tab.poll_ms, g_tab.up_polls, g_tab.holdoff_ms);
    return SAI_STATUS_SUCCESS;
}

void ext_phy_detach()
{
    Backend* be = nullptr;
    std::vector<int> gated;
    {
        std::lock_guard<std::mutex> g(g_mu);
        be = g_be;
        for (auto& p : g_tab.ports)
            if (p.hw >= 0 && !p.link.empty()) gated.push_back(p.hw);
    }
    // Without g_mu: the backend may be waiting for a gate call to return.
    if (be)
        for (int hw : gated) be->port_link_gate(hw, nullptr);
    std::lock_guard<std::mutex> g(g_mu);
    g_be = nullptr;
    g_by_hw.clear();
    g_tab = ExtPhyTable();
}

bool ext_phy_active()
{
    std::lock_guard<std::mutex> g(g_mu);
    return g_be != nullptr;
}

void ext_phy_admin(int hw, bool up)
{
    std::lock_guard<std::mutex> g(g_mu);
    ExtPort* p = g_be ? port_of_hw(hw) : nullptr;
    if (!p) return;
    p->admin = up;
    p->ext_up = false;
    p->good = 0;
    p->next_poll_ms = 0;
    const std::vector<ExtOp>& ops = up ? p->up : p->down;
    if (ops.empty()) return;
    uint64_t t0 = now_us();
    SeqResult r = run_seq(*g_be, ops);
    uint64_t dt = now_us() - t0;
    p->hooks++;
    if (dt > p->hook_max_us) p->hook_max_us = dt;
    if (r.fault) {
        p->hook_fail++;
        OB_ERR(SAI_API_PORT, "port %s: external PHY admin-%s sequence stopped: %s", p->name.c_str(), up ? "up" : "down",
               r.why.c_str());
    } else {
        OB_INFO(SAI_API_PORT, "port %s: external PHY admin-%s sequence done (%llu us)", p->name.c_str(), up ? "up" : "down",
                (unsigned long long)dt);
    }
}

int ext_phy_gate(int hw)
{
    std::lock_guard<std::mutex> g(g_mu);
    ExtPort* p = g_be ? port_of_hw(hw) : nullptr;
    if (!p || p->link.empty() || !p->admin) return -1;
    uint64_t t0 = now_us(), now_ms = t0 / 1000;
    if (now_ms < g_hold_until || now_ms < p->next_poll_ms) return p->ext_up ? -1 : 0;
    SeqResult r = run_seq(*g_be, p->link);
    uint64_t dt = now_us() - t0;
    p->polls++;
    p->last_us = dt;
    p->total_us += dt;
    if (dt > p->max_us) p->max_us = dt;
    p->next_poll_ms = now_ms + g_tab.poll_ms;
    if (!r.fault) p->last_val = r.last;
    if (r.fault != p->fault) {
        if (r.fault) OB_WARN(SAI_API_PORT, "port %s: external PHY link poll fault: %s", p->name.c_str(), r.why.c_str());
        else OB_NOTICE(SAI_API_PORT, "port %s: external PHY link poll answers again", p->name.c_str());
        p->fault = r.fault;
    }
    if (r.fault) p->faults++;
    if (!r.fault && r.cond) {
        if (p->good < 1000000) p->good++;
        if (!p->ext_up && p->good >= g_tab.up_polls) {
            p->ext_up = true;
            p->ups++;
            OB_NOTICE(SAI_API_PORT, "port %s: line side up (external PHY, value 0x%04x, %u good polls)", p->name.c_str(),
                      r.last, p->good);
        }
    } else {
        p->good = 0;
        if (p->ext_up) {
            p->ext_up = false;
            p->downs++;
            if (r.fault) OB_NOTICE(SAI_API_PORT, "port %s: line side down (external PHY, poll fault)", p->name.c_str());
            else OB_NOTICE(SAI_API_PORT, "port %s: line side down (external PHY, value 0x%04x)", p->name.c_str(), r.last);
        }
    }
    return p->ext_up ? -1 : 0;
}

void ext_phy_foreign_mdio(const std::function<void()>& fn)
{
    std::lock_guard<std::mutex> g(g_mu);
    if (g_be && g_tab.holdoff_ms) {
        uint64_t now_ms = now_us() / 1000;
        if (now_ms >= g_hold_until) {
            g_holdoffs++;
            OB_NOTICE(SAI_API_SWITCH, "external PHY link polls paused: raw MDIO on the diag socket (resume %u ms after the last one)",
                      g_tab.holdoff_ms);
        }
        g_hold_until = now_ms + g_tab.holdoff_ms;
    }
    fn();
}

std::string ext_phy_status()
{
    std::lock_guard<std::mutex> g(g_mu);
    std::string s;
    char b[320];
    if (!g_be) return "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n";
    uint64_t now_ms = now_us() / 1000;
    std::snprintf(b, sizeof(b), "extphy: %zu ports, poll %u ms, up after %u good polls, hold-off %u ms (%s, %llu so far)\n",
                  g_tab.ports.size(), g_tab.poll_ms, g_tab.up_polls, g_tab.holdoff_ms,
                  now_ms < g_hold_until ? "PAUSED now" : "not paused", (unsigned long long)g_holdoffs);
    s += b;
    s += "port      lane   hw admin line  fault  value    polls   faults  ups downs last_us  avg_us  max_us hooks fail hook_max_us\n";
    for (auto& p : g_tab.ports) {
        std::snprintf(b, sizeof(b), "%-8s %5u %4d %-5s %-5s %-5s  0x%04x %8llu %8llu %4llu %5llu %7llu %7llu %7llu %5llu %4llu %11llu\n",
                      p.name.c_str(), p.lane, p.hw, p.admin ? "up" : "down", p.link.empty() ? "-" : p.ext_up ? "up" : "down",
                      p.fault ? "FAULT" : "no", p.last_val, (unsigned long long)p.polls, (unsigned long long)p.faults,
                      (unsigned long long)p.ups, (unsigned long long)p.downs, (unsigned long long)p.last_us,
                      (unsigned long long)(p.polls ? p.total_us / p.polls : 0), (unsigned long long)p.max_us,
                      (unsigned long long)p.hooks, (unsigned long long)p.hook_fail, (unsigned long long)p.hook_max_us);
        s += b;
    }
    return s;
}

ExtPhyTable& ext_phy_table() { return g_tab; }

} // namespace ob

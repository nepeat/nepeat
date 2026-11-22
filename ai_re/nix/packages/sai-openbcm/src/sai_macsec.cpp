// SPDX-License-Identifier: Apache-2.0
// SAI MACsec (sai_macsec_api_t) for ports whose MACsec engine is an external
// PHY described by board data: the external MACsec table (ext_macsec.h). No
// chip knowledge here. The object model is SONiC MACsecOrch's:
//
//   MACSEC (one per direction)      capabilities only
//   MACSEC_PORT (port, direction)   "enable.<dir>" on the port's PHY, ASIC inter-frame gap from the table
//   ACL table of stage *_MACSEC     kept here, never in the ASIC: the PHY's classification is fixed
//     bound with PORT_ATTR_{INGRESS,EGRESS}_MACSEC_ACL; its entries are what ties a MACSEC_FLOW to a port:
//     an enabled entry with ACTION_MACSEC_FLOW opens the gate (data frames through the SC),
//     without one data frames are dropped (PACKET_ACTION DROP, MACsecOrch's default) -- or pass in clear
//     only if the board table has no gate sequences
//   MACSEC_FLOW                     counters of the port it is bound to
//   MACSEC_SC (flow, SCI, cipher)   parameters for its SAs; one SC per port and direction
//   MACSEC_SA (SC, AN, keys, PN)    "sa-add..." / "sa-del..."; the newest egress SA transmits
//
// An SA can be created before its flow is bound to a port (MACsecOrch does
// that while the port is not enabled yet): it is then kept, key included,
// and installed when the binding appears. Keys are wiped once installed and
// never logged.
#include "ext_macsec.h"
#include "ext_phy.h"
#include "sai_internal.h"

#include <algorithm>
#include <chrono>
#include <fstream>
#include <sstream>
#include <sys/stat.h>
#include <thread>

namespace ob {

namespace {
enum { EG = 0, IN = 1 };
const char* const kDir[2] = {"egress", "ingress"};
const char* cipher_name(int32_t c)
{
    switch (c) {
    case SAI_MACSEC_CIPHER_SUITE_GCM_AES_128: return "gcm-aes-128";
    case SAI_MACSEC_CIPHER_SUITE_GCM_AES_256: return "gcm-aes-256";
    case SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_128: return "gcm-aes-xpn-128";
    case SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256: return "gcm-aes-xpn-256";
    }
    return nullptr;
}
bool cipher_xpn(int32_t c) { return c == SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_128 || c == SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256; }
unsigned cipher_klen(int32_t c) { return c == SAI_MACSEC_CIPHER_SUITE_GCM_AES_128 || c == SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_128 ? 16 : 32; }

using Counters = std::map<std::string, uint64_t>;

struct MsObj { int dir = EG; };
struct MsPort { int dir = EG; sai_object_id_t port = 0; int hw = -1; };
struct MsFlow { int dir = EG; int hw = -1; Counters acc; };            // hw: where its counters were last read
struct MsSc {
    int dir = EG;
    sai_object_id_t flow = 0;
    uint8_t sci[8] = {0};
    int32_t cipher = SAI_MACSEC_CIPHER_SUITE_GCM_AES_128;
    bool replay = false;
    uint32_t window = 0;
    int hw = -1;                    // the port it was bound to (through its flow's ACL entry) when its first SA was installed
    StubMap stub;
};
struct MsSa {
    int dir = EG;
    sai_object_id_t sc = 0;
    uint8_t an = 0;
    MsParams par;                   // until installed
    uint64_t pn = 1;                // as configured; read back from the PHY once installed
    uint64_t order = 0;             // creation order
    bool installed = false;
    Counters acc;
};
struct MsAclTable { int dir = EG; sai_object_id_t port = 0; std::set<sai_attr_id_t> fields; std::vector<sai_object_id_t> entries; };
struct MsAclEntry {
    sai_object_id_t table = 0;
    uint32_t priority = 0;
    bool admin = true;
    bool flow_on = false;
    sai_object_id_t flow = 0;
    bool action_on = false;
    int32_t action = SAI_PACKET_ACTION_FORWARD;
    std::map<sai_attr_id_t, sai_acl_field_data_t> fields;
};
// What the PHY of one port holds, per direction.
struct Hw {
    bool enabled = false, gate = false;
    sai_object_id_t sa[4] = {0, 0, 0, 0};   // installed SA per AN
    sai_object_id_t tx = 0;                 // egress: the transmitting SA
    uint64_t runs = 0, fails = 0, max_us = 0;
    std::string last_fail;
};
struct PortHw { unsigned phy = 0; std::string name; Hw d[2]; };

struct State {
    MsTable tab;
    bool loaded = false;
    Backend* be = nullptr;
    std::map<int, PortHw> ports;            // by hw port
    std::map<sai_object_id_t, MsObj> objs;
    std::map<sai_object_id_t, MsPort> mports;
    std::map<sai_object_id_t, MsFlow> flows;
    std::map<sai_object_id_t, MsSc> scs;
    std::map<sai_object_id_t, MsSa> sas;
    std::map<sai_object_id_t, MsAclTable> tables;
    std::map<sai_object_id_t, MsAclEntry> entries;
    uint64_t order = 0;
};
State& S()
{
    static State* s = new State;
    return *s;
}

uint64_t now_us()
{
    return (uint64_t)std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}
void wait_us(unsigned us)
{
    if (us == 0) return;
    if (us > 2000) {
        std::this_thread::sleep_for(std::chrono::microseconds(us));
        return;
    }
    uint64_t end = now_us() + us;   // short waits are spun: appliance kernels round a sleep up to a tick
    while (now_us() < end) {}
}

// Run one sequence on the PHY of hw under the external PHY sequence lock.
// missing_ok: a sequence the table does not have is success (optional ones).
sai_status_t run(int hw, int dir, const std::string& seq, const MsParams& par, MsResult* res, bool missing_ok = false)
{
    State& s = S();
    auto it = s.ports.find(hw);
    if (!s.be || it == s.ports.end()) return SAI_STATUS_NOT_SUPPORTED;
    const std::vector<MsOp>* ops = s.tab.find(seq);
    if (!ops) {
        if (missing_ok) return SAI_STATUS_SUCCESS;
        OB_ERR(SAI_API_MACSEC, "port %s: the external MACsec table has no sequence %s", it->second.name.c_str(), seq.c_str());
        return SAI_STATUS_NOT_SUPPORTED;
    }
    Backend& be = *s.be;
    MsIo io;
    io.rd = [&be](unsigned p, unsigned d, unsigned r, unsigned& v, const MsOp&) { return be.mdio_c45_read(p, d, r, v); };
    io.wr = [&be](unsigned p, unsigned d, unsigned r, unsigned v) { return be.mdio_c45_write(p, d, r, v); };
    io.delay = wait_us;
    std::string why;
    sai_status_t st = SAI_STATUS_FAILURE;
    uint64_t t0 = now_us();
    unsigned phy = it->second.phy;
    ext_phy_locked([&] { st = ext_macsec_exec(io, *ops, phy, par, res, why); });
    uint64_t dt = now_us() - t0;
    Hw& h = it->second.d[dir];
    h.runs++;
    if (dt > h.max_us) h.max_us = dt;
    if (st != SAI_STATUS_SUCCESS) {
        h.fails++;
        h.last_fail = seq + ": " + why;
        OB_ERR(SAI_API_MACSEC, "port %s: external MACsec sequence %s stopped: %s", it->second.name.c_str(), seq.c_str(), why.c_str());
    } else {
        OB_INFO(SAI_API_MACSEC, "port %s: external MACsec sequence %s done (%zu operations, %llu us)", it->second.name.c_str(),
                seq.c_str(), ops->size(), (unsigned long long)dt);
    }
    return st;
}

// The first of the candidate sequences that the table has.
std::string pick(const std::vector<std::string>& names)
{
    for (auto& n : names)
        if (S().tab.find(n)) return n;
    return names.back();
}

// ---- model -> port ----
sai_object_id_t table_of_port(sai_object_id_t port, int dir)
{
    for (auto& t : S().tables)
        if (t.second.port == port && t.second.dir == dir) return t.first;
    return 0;
}
// The flow an enabled entry of the port's MACsec ACL table sends data frames to (0: none).
sai_object_id_t bound_flow(sai_object_id_t port, int dir)
{
    State& s = S();
    auto t = s.tables.find(table_of_port(port, dir));
    if (t == s.tables.end()) return 0;
    for (sai_object_id_t eid : t->second.entries) {
        MsAclEntry& e = s.entries[eid];
        if (e.admin && e.flow_on && e.flow && s.flows.count(e.flow)) return e.flow;
    }
    return 0;
}
int hw_of_flow(sai_object_id_t flow)
{
    State& s = S();
    for (auto& e : s.entries) {
        if (!(e.second.admin && e.second.flow_on && e.second.flow == flow)) continue;
        auto t = s.tables.find(e.second.table);
        if (t == s.tables.end() || !t->second.port) continue;
        PortObj* p = lookup(ctx().ports, t->second.port);
        if (p) return p->hw;
    }
    return -1;
}
int installed_count(const Hw& h)
{
    int n = 0;
    for (auto x : h.sa) n += x != 0;
    return n;
}

sai_status_t sa_install(int hw, MsSa& sa, sai_object_id_t oid)
{
    State& s = S();
    Hw& h = s.ports[hw].d[sa.dir];
    MsSc& sc = s.scs[sa.sc];
    const char* cn = cipher_name(sc.cipher);
    if (!cn) return SAI_STATUS_NOT_SUPPORTED;
    std::string base = std::string(".") + kDir[sa.dir] + "." + std::to_string(sa.an) + "." + cn;
    bool first = installed_count(h) == 0;
    std::vector<std::string> names;
    bool rp = sa.dir == IN && sc.replay;
    if (first && rp) names.push_back("sa-add-first" + base + ".replay");
    if (first) names.push_back("sa-add-first" + base);
    if (rp) names.push_back("sa-add" + base + ".replay");
    names.push_back("sa-add" + base);
    MsParams par = sa.par;
    par["sci"].assign(sc.sci, sc.sci + 8);
    uint32_t w = sc.replay ? sc.window : 0;
    par["window"] = {(uint8_t)w, (uint8_t)(w >> 8), (uint8_t)(w >> 16), (uint8_t)(w >> 24)};
    sai_status_t st = run(hw, sa.dir, pick(names), par, nullptr);
    ms_wipe(par);
    if (st != SAI_STATUS_SUCCESS) return st;
    ms_wipe(sa.par);                // the PHY has the key now
    sa.installed = true;
    h.sa[sa.an] = oid;
    if (sa.dir == EG) h.tx = oid;
    sc.hw = hw;
    OB_NOTICE(SAI_API_MACSEC, "port %s: %s SA AN %u installed (%s%s)", s.ports[hw].name.c_str(), kDir[sa.dir], sa.an, cn,
              first ? ", first of its SC" : "");
    return SAI_STATUS_SUCCESS;
}

void gate_set(int hw, int dir, bool open)
{
    Hw& h = S().ports[hw].d[dir];
    if (!h.enabled || h.gate == open) return;
    if (run(hw, dir, std::string(open ? "gate-open." : "gate-close.") + kDir[dir], MsParams(), nullptr, true) == SAI_STATUS_SUCCESS) {
        h.gate = open;
        OB_NOTICE(SAI_API_MACSEC, "port %s: %s data frames %s", S().ports[hw].name.c_str(), kDir[dir],
                  open ? "go through the secure channel" : "are dropped");
    }
}

sai_status_t sa_uninstall(int hw, MsSa& sa, sai_object_id_t oid)
{
    State& s = S();
    Hw& h = s.ports[hw].d[sa.dir];
    if (h.sa[sa.an] != oid) return SAI_STATUS_SUCCESS;
    bool last = installed_count(h) == 1;
    // Never transmit with, or accept through, what is about to go: close the gate
    // before the last SA and before the transmitting one is removed.
    if (last || (sa.dir == EG && h.tx == oid)) gate_set(hw, sa.dir, false);
    std::string base = std::string(".") + kDir[sa.dir] + "." + std::to_string(sa.an);
    sai_status_t st = run(hw, sa.dir, pick(last ? std::vector<std::string>{"sa-del-last" + base, "sa-del" + base}
                                                : std::vector<std::string>{"sa-del" + base}), MsParams(), nullptr);
    h.sa[sa.an] = 0;                // gone from the model either way
    if (h.tx == oid) h.tx = 0;
    sa.installed = false;
    return st;
}

// Bring the PHY of (hw, dir) to what the objects say. only: report the status
// of installing this SA (create_macsec_sa), everything else is logged.
sai_status_t reconcile(int hw, int dir, sai_object_id_t only = 0)
{
    State& s = S();
    auto pit = s.ports.find(hw);
    if (pit == s.ports.end()) return SAI_STATUS_SUCCESS;
    Hw& h = pit->second.d[dir];
    sai_status_t result = SAI_STATUS_SUCCESS;
    if (!h.enabled) return result;
    sai_object_id_t port = ctx().hw2port.count(hw) ? ctx().hw2port[hw] : 0;
    sai_object_id_t flow = bound_flow(port, dir);
    // pending SAs of the SC(s) on the bound flow, oldest first
    std::vector<std::pair<uint64_t, sai_object_id_t>> pending;
    if (flow)
        for (auto& e : s.sas) {
            MsSa& sa = e.second;
            if (sa.dir != dir || sa.installed) continue;
            auto sc = s.scs.find(sa.sc);
            if (sc == s.scs.end() || sc->second.flow != flow) continue;
            pending.emplace_back(sa.order, e.first);
        }
    std::sort(pending.begin(), pending.end());
    for (auto& pe : pending) {
        MsSa& sa = s.sas[pe.second];
        sai_object_id_t old = h.sa[sa.an];
        if (old && old != pe.second) {   // the AN is held by an SA of another SC: one SC per port
            OB_ERR(SAI_API_MACSEC, "port %s: %s AN %u is in use by another SA (one secure channel per port and direction)",
                   pit->second.name.c_str(), kDir[dir], sa.an);
            if (pe.second == only) result = SAI_STATUS_INSUFFICIENT_RESOURCES;
            continue;
        }
        sai_status_t st = sa_install(hw, sa, pe.second);
        if (pe.second == only) result = st;
    }
    bool want = flow != 0 && installed_count(h) > 0 && (dir == IN || h.tx != 0);
    gate_set(hw, dir, want);
    return result;
}

void reconcile_port(sai_object_id_t port, int dir)
{
    PortObj* p = lookup(ctx().ports, port);
    if (p) reconcile(p->hw, dir);
}

// ---- counters ----
struct StatName { sai_stat_id_t id; const char* name; };
const StatName kSaStats[] = {
    {SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED, "octets_encrypted"},   {SAI_MACSEC_SA_STAT_OCTETS_PROTECTED, "octets_protected"},
    {SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED, "out_pkts_encrypted"}, {SAI_MACSEC_SA_STAT_OUT_PKTS_PROTECTED, "out_pkts_protected"},
    {SAI_MACSEC_SA_STAT_IN_PKTS_UNCHECKED, "in_pkts_unchecked"}, {SAI_MACSEC_SA_STAT_IN_PKTS_DELAYED, "in_pkts_delayed"},
    {SAI_MACSEC_SA_STAT_IN_PKTS_LATE, "in_pkts_late"},           {SAI_MACSEC_SA_STAT_IN_PKTS_INVALID, "in_pkts_invalid"},
    {SAI_MACSEC_SA_STAT_IN_PKTS_NOT_VALID, "in_pkts_not_valid"}, {SAI_MACSEC_SA_STAT_IN_PKTS_NOT_USING_SA, "in_pkts_not_using_sa"},
    {SAI_MACSEC_SA_STAT_IN_PKTS_UNUSED_SA, "in_pkts_unused_sa"}, {SAI_MACSEC_SA_STAT_IN_PKTS_OK, "in_pkts_ok"},
    {SAI_MACSEC_SA_STAT_IN_OCTETS_DECRYPTED, "in_octets_decrypted"}, {SAI_MACSEC_SA_STAT_IN_OCTETS_VALIDATED, "in_octets_validated"},
};
const StatName kFlowStats[] = {
    {SAI_MACSEC_FLOW_STAT_OTHER_ERR, "other_err"},
    {SAI_MACSEC_FLOW_STAT_OCTETS_UNCONTROLLED, "octets_uncontrolled"},
    {SAI_MACSEC_FLOW_STAT_OCTETS_CONTROLLED, "octets_controlled"},
    {SAI_MACSEC_FLOW_STAT_OUT_OCTETS_COMMON, "out_octets_common"},
    {SAI_MACSEC_FLOW_STAT_UCAST_PKTS_UNCONTROLLED, "ucast_pkts_uncontrolled"},
    {SAI_MACSEC_FLOW_STAT_UCAST_PKTS_CONTROLLED, "ucast_pkts_controlled"},
    {SAI_MACSEC_FLOW_STAT_MULTICAST_PKTS_UNCONTROLLED, "multicast_pkts_uncontrolled"},
    {SAI_MACSEC_FLOW_STAT_MULTICAST_PKTS_CONTROLLED, "multicast_pkts_controlled"},
    {SAI_MACSEC_FLOW_STAT_BROADCAST_PKTS_UNCONTROLLED, "broadcast_pkts_uncontrolled"},
    {SAI_MACSEC_FLOW_STAT_BROADCAST_PKTS_CONTROLLED, "broadcast_pkts_controlled"},
    {SAI_MACSEC_FLOW_STAT_CONTROL_PKTS, "control_pkts"},
    {SAI_MACSEC_FLOW_STAT_PKTS_UNTAGGED, "pkts_untagged"},
    {SAI_MACSEC_FLOW_STAT_IN_TAGGED_CONTROL_PKTS, "in_tagged_control_pkts"},
    {SAI_MACSEC_FLOW_STAT_OUT_PKTS_TOO_LONG, "out_pkts_too_long"},
    {SAI_MACSEC_FLOW_STAT_IN_PKTS_NO_TAG, "in_pkts_no_tag"},
    {SAI_MACSEC_FLOW_STAT_IN_PKTS_BAD_TAG, "in_pkts_bad_tag"},
    {SAI_MACSEC_FLOW_STAT_IN_PKTS_NO_SCI, "in_pkts_no_sci"},
    {SAI_MACSEC_FLOW_STAT_IN_PKTS_UNKNOWN_SCI, "in_pkts_unknown_sci"},
    {SAI_MACSEC_FLOW_STAT_IN_PKTS_OVERRUN, "in_pkts_overrun"},
};
template <size_t N> const char* stat_name(const StatName (&t)[N], sai_stat_id_t id)
{
    for (auto& e : t)
        if (e.id == id) return e.name;
    return nullptr;
}

// Read the counters of a sequence into acc (accumulating if the PHY clears on read).
sai_status_t poll_counters(int hw, int dir, const std::string& seq, Counters& acc)
{
    if (!S().tab.find(seq)) return SAI_STATUS_SUCCESS;
    MsResult res;
    sai_status_t st = run(hw, dir, seq, MsParams(), &res);
    if (st != SAI_STATUS_SUCCESS) return st;
    for (auto& r : res) {
        uint64_t v = ms_result_u64(res, r.first);
        if (S().tab.clear_on_read) acc[r.first] += v;
        else acc[r.first] = v;
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t sa_poll(MsSa& sa)
{
    auto sc = S().scs.find(sa.sc);
    if (!sa.installed || sc == S().scs.end() || sc->second.hw < 0) return SAI_STATUS_SUCCESS;
    return poll_counters(sc->second.hw, sa.dir, std::string("stats-sa.") + kDir[sa.dir] + "." + std::to_string(sa.an), sa.acc);
}

sai_status_t stats_fill(const Counters& acc, const char* name, uint64_t& out)
{
    auto it = name ? acc.find(name) : acc.end();
    out = it == acc.end() ? 0 : it->second;
    return SAI_STATUS_SUCCESS;
}

// ---- attribute helpers ----
int dir_of(int32_t v) { return v == SAI_MACSEC_DIRECTION_INGRESS ? IN : EG; }
sai_status_t need_table() { return S().loaded ? SAI_STATUS_SUCCESS : SAI_STATUS_NOT_SUPPORTED; }

// ============================================================ MACSEC
sai_status_t create_macsec(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (need_table() != SAI_STATUS_SUCCESS) {
            OB_ERR(SAI_API_MACSEC, "MACsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE): this switch has no MACsec engine");
            return SAI_STATUS_NOT_SUPPORTED;
        }
        const sai_attribute_t* d = find_attr(n, attrs, SAI_MACSEC_ATTR_DIRECTION);
        if (!d) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_MACSEC_ATTR_DIRECTION:
            case SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE:   // ports without a MACSEC_PORT stay in bypass, which is all we do
            case SAI_MACSEC_ATTR_WARM_BOOT_ENABLE:
            case SAI_MACSEC_ATTR_STATS_MODE:
                break;
            case SAI_MACSEC_ATTR_ENABLE_POST:
                if (attrs[i].value.booldata) return attr_not_supported(i);
                break;
            default:
                return attr_not_supported(i);
            }
        }
        for (auto& o : S().objs)
            if (o.second.dir == dir_of(d->value.s32)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        *id = ctx().alloc(SAI_OBJECT_TYPE_MACSEC);
        S().objs[*id].dir = dir_of(d->value.s32);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t remove_macsec(sai_object_id_t id)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().objs.find(id);
        if (it == S().objs.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& p : S().mports)
            if (p.second.dir == it->second.dir) return SAI_STATUS_OBJECT_IN_USE;
        S().objs.erase(it);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t set_macsec_attribute(sai_object_id_t id, const sai_attribute_t* attr)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().objs.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if (attr->id == SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE || attr->id == SAI_MACSEC_ATTR_WARM_BOOT_ENABLE) return SAI_STATUS_SUCCESS;
        return attr_not_supported(0);
    });
}
sai_status_t get_macsec_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().objs.find(id);
        if (it == S().objs.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MACSEC_ATTR_DIRECTION: a.value.s32 = it->second.dir == IN ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS; break;
            // false: the SC is found by the PHY (one per port), not by an SCI match in the ACL
            case SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL: a.value.booldata = false; break;
            case SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC: a.value.s32 = SAI_MACSEC_MAX_SECURE_ASSOCIATIONS_PER_SC_FOUR; break;
            case SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED:
            case SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED:
            case SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED:
            case SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED: {
                // what the board table has sequences for
                bool xpn = a.id == SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED, k256 = a.id == SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED;
                bool any = false;
                for (auto& sq : S().tab.seqs) {
                    if (sq.first.compare(0, 7, "sa-add.") != 0) continue;
                    bool sx = sq.first.find("-xpn-") != std::string::npos, s256 = sq.first.find("-256") != std::string::npos;
                    if (a.id == SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED || xpn) any = any || sx == xpn;
                    else any = any || s256 == k256;
                }
                a.value.booldata = any;
                break;
            }
            case SAI_MACSEC_ATTR_STATS_MODE_READ_SUPPORTED: a.value.booldata = true; break;
            case SAI_MACSEC_ATTR_STATS_MODE_READ_CLEAR_SUPPORTED:
            case SAI_MACSEC_ATTR_WARM_BOOT_SUPPORTED:
            case SAI_MACSEC_ATTR_WARM_BOOT_ENABLE:
            case SAI_MACSEC_ATTR_ENABLE_POST:
            case SAI_MACSEC_ATTR_SWITCHING_MODE_CUT_THROUGH_SUPPORTED: a.value.booldata = false; break;
            case SAI_MACSEC_ATTR_SWITCHING_MODE_STORE_AND_FORWARD_SUPPORTED:
            case SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE: a.value.booldata = true; break;
            case SAI_MACSEC_ATTR_STATS_MODE: a.value.s32 = SAI_STATS_MODE_READ; break;
            case SAI_MACSEC_ATTR_AVAILABLE_MACSEC_FLOW:
            case SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SC: a.value.u32 = (uint32_t)S().ports.size(); break;
            case SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA: a.value.u32 = (uint32_t)S().ports.size() * 4; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}

// ============================================================ MACSEC_PORT
sai_status_t create_macsec_port(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (need_table() != SAI_STATUS_SUCCESS) return SAI_STATUS_NOT_SUPPORTED;
        uint32_t pi = 0;
        const sai_attribute_t* d = find_attr(n, attrs, SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION);
        const sai_attribute_t* pa = find_attr(n, attrs, SAI_MACSEC_PORT_ATTR_PORT_ID, &pi);
        if (!d || !pa) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (uint32_t i = 0; i < n; i++) {
            switch (attrs[i].id) {
            case SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION:
            case SAI_MACSEC_PORT_ATTR_PORT_ID:
            case SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE:
            case SAI_MACSEC_PORT_ATTR_STATS_COUNT_MODE:
                break;
            case SAI_MACSEC_PORT_ATTR_CTAG_ENABLE:
            case SAI_MACSEC_PORT_ATTR_STAG_ENABLE:      // no VLAN tag in clear
                if (attrs[i].value.booldata) return attr_not_supported(i);
                break;
            default:
                return attr_not_supported(i);
            }
        }
        PortObj* p = lookup(ctx().ports, pa->value.oid);
        if (!p || p->cpu) return attr_invalid(pi);
        auto ph = S().ports.find(p->hw);
        if (ph == S().ports.end()) {
            OB_ERR(SAI_API_MACSEC, "MACsec port: port %s has no MACsec engine in the external MACsec table", p->sdk_name.c_str());
            return SAI_STATUS_NOT_SUPPORTED;
        }
        int dir = dir_of(d->value.s32);
        for (auto& m : S().mports)
            if (m.second.port == p->oid && m.second.dir == dir) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        // "reset" (optional) empties the SA tables first: nothing a previous process installed may stay valid
        sai_status_t st = run(p->hw, dir, std::string("reset.") + kDir[dir], MsParams(), nullptr, true);
        if (st == SAI_STATUS_SUCCESS) st = run(p->hw, dir, std::string("enable.") + kDir[dir], MsParams(), nullptr);
        if (st != SAI_STATUS_SUCCESS) return st;
        Hw& h = ph->second.d[dir];
        h.enabled = true;
        h.gate = false;             // "enable" leaves data frames dropped (when the table has a gate at all)
        for (auto& x : h.sa) x = 0;
        h.tx = 0;
        *id = ctx().alloc(SAI_OBJECT_TYPE_MACSEC_PORT);
        MsPort& m = S().mports[*id];
        m.dir = dir;
        m.port = p->oid;
        m.hw = p->hw;               // (before the gap: macsec_port_ifg looks for this object)
        if (port_ifg_apply(*p) != SAI_STATUS_SUCCESS)   // room for the SecTAG + ICV the PHY adds to every frame
            OB_ERR(SAI_API_MACSEC, "port %s: MACsec is on without the wider inter-frame gap", ph->second.name.c_str());
        OB_NOTICE(SAI_API_MACSEC, "port %s: MACsec %s on (external PHY 0x%x)", ph->second.name.c_str(), kDir[dir], ph->second.phy);
        reconcile(p->hw, dir);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t remove_macsec_port(sai_object_id_t id)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().mports.find(id);
        if (it == S().mports.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        MsPort m = it->second;
        Hw& h = S().ports[m.hw].d[m.dir];
        // SAs still installed (their objects outlive the port object only if the caller is out of order)
        for (auto& e : S().sas)
            if (e.second.installed && h.sa[e.second.an] == e.first) sa_uninstall(m.hw, e.second, e.first);
        sai_status_t st = run(m.hw, m.dir, std::string("disable.") + kDir[m.dir], MsParams(), nullptr);
        h = Hw{.runs = h.runs, .fails = h.fails, .max_us = h.max_us, .last_fail = h.last_fail};
        S().mports.erase(it);
        if (PortObj* p = lookup(ctx().ports, m.port)) port_ifg_apply(*p);
        OB_NOTICE(SAI_API_MACSEC, "port %s: MACsec %s off%s", S().ports[m.hw].name.c_str(), kDir[m.dir],
                  st == SAI_STATUS_SUCCESS ? "" : " (the disable sequence FAILED)");
        return st;
    });
}
sai_status_t set_macsec_port_attribute(sai_object_id_t id, const sai_attribute_t* attr)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().mports.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        if ((attr->id == SAI_MACSEC_PORT_ATTR_CTAG_ENABLE || attr->id == SAI_MACSEC_PORT_ATTR_STAG_ENABLE) && !attr->value.booldata)
            return SAI_STATUS_SUCCESS;
        return attr_not_supported(0);
    });
}
sai_status_t get_macsec_port_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().mports.find(id);
        if (it == S().mports.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION: a.value.s32 = it->second.dir == IN ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS; break;
            case SAI_MACSEC_PORT_ATTR_PORT_ID: a.value.oid = it->second.port; break;
            case SAI_MACSEC_PORT_ATTR_CTAG_ENABLE:
            case SAI_MACSEC_PORT_ATTR_STAG_ENABLE: a.value.booldata = false; break;
            case SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE: a.value.s32 = SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD; break;
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_port_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().mports.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return n ? SAI_STATUS_NOT_SUPPORTED : SAI_STATUS_SUCCESS;   // the flow carries the counters
    });
}
sai_status_t get_macsec_port_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t, uint64_t* out)
{
    return get_macsec_port_stats(id, n, ids, out);
}
sai_status_t clear_macsec_port_stats(sai_object_id_t, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }

// ============================================================ MACSEC_FLOW
sai_status_t create_macsec_flow(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (need_table() != SAI_STATUS_SUCCESS) return SAI_STATUS_NOT_SUPPORTED;
        const sai_attribute_t* d = find_attr(n, attrs, SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION);
        if (!d) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        for (uint32_t i = 0; i < n; i++)
            if (attrs[i].id != SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION && attrs[i].id != SAI_MACSEC_FLOW_ATTR_STATS_COUNT_MODE)
                return attr_not_supported(i);
        *id = ctx().alloc(SAI_OBJECT_TYPE_MACSEC_FLOW);
        S().flows[*id].dir = dir_of(d->value.s32);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t remove_macsec_flow(sai_object_id_t id)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().flows.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& sc : S().scs)
            if (sc.second.flow == id) return SAI_STATUS_OBJECT_IN_USE;
        for (auto& e : S().entries)
            if (e.second.flow_on && e.second.flow == id) return SAI_STATUS_OBJECT_IN_USE;
        S().flows.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t set_macsec_flow_attribute(sai_object_id_t id, const sai_attribute_t*)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        return S().flows.count(id) ? attr_not_supported(0) : SAI_STATUS_INVALID_OBJECT_ID;
    });
}
sai_status_t get_macsec_flow_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().flows.find(id);
        if (it == S().flows.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            if (a.id == SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION) {
                a.value.s32 = it->second.dir == IN ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS;
            } else if (a.id == SAI_MACSEC_FLOW_ATTR_SC_LIST || a.id == SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST) {
                std::vector<sai_object_id_t> v;
                if (a.id == SAI_MACSEC_FLOW_ATTR_SC_LIST) {
                    for (auto& sc : S().scs) if (sc.second.flow == id) v.push_back(sc.first);
                } else {
                    for (auto& e : S().entries) if (e.second.flow_on && e.second.flow == id) v.push_back(e.first);
                }
                sai_status_t st = fill_list(a.value.objlist, v);
                if (st != SAI_STATUS_SUCCESS) return st;
            } else {
                return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_flow_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().flows.find(id);
        if (it == S().flows.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !out)) return SAI_STATUS_INVALID_PARAMETER;
        MsFlow& f = it->second;
        int hw = hw_of_flow(id);
        if (hw < 0)   // not bound right now: the port its SC was installed on
            for (auto& sc : S().scs)
                if (sc.second.flow == id && sc.second.hw >= 0) hw = sc.second.hw;
        if (hw >= 0 && S().ports[hw].d[f.dir].enabled) {
            sai_status_t st = poll_counters(hw, f.dir, std::string("stats-flow.") + kDir[f.dir], f.acc);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        for (uint32_t i = 0; i < n; i++) stats_fill(f.acc, stat_name(kFlowStats, ids[i]), out[i]);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_flow_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t mode, uint64_t* out)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;
    return get_macsec_flow_stats(id, n, ids, out);
}
sai_status_t clear_macsec_flow_stats(sai_object_id_t id, uint32_t, const sai_stat_id_t*)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().flows.find(id);
        if (it == S().flows.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!S().tab.clear_on_read) return SAI_STATUS_NOT_SUPPORTED;
        uint64_t x = 0;
        sai_stat_id_t one = SAI_MACSEC_FLOW_STAT_OTHER_ERR;
        get_macsec_flow_stats(id, 1, &one, &x);   // empty the PHY's counters
        it->second.acc.clear();
        return SAI_STATUS_SUCCESS;
    });
}

// ============================================================ MACSEC_SC
const StubDef kScStubs[] = {
    {SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE, Kind::Bool, 1, false},
    {SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET, Kind::U8, 0, true},
    {SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, Kind::Bool, 1, false},
};
// What the PHY sequences are recorded for: SCI in the SecTAG, confidentiality on, no offset.
sai_status_t sc_fixed(const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE:
    case SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE:
        if (!a.value.booldata) {
            OB_ERR(SAI_API_MACSEC, "MACsec SC: %s off is not supported (the PHY sequences send the SCI and encrypt)",
                   a.id == SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE ? "encryption" : "explicit SCI");
            return attr_not_supported(i);
        }
        return SAI_STATUS_SUCCESS;
    case SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET:
        return a.value.u8 == 0 ? SAI_STATUS_SUCCESS : attr_not_supported(i);
    case SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET:
        return a.value.u8 == 0 ? SAI_STATUS_SUCCESS : attr_not_supported(i);
    case SAI_MACSEC_SC_ATTR_USE_ES:
    case SAI_MACSEC_SC_ATTR_USE_SCB:
        return a.value.booldata ? attr_not_supported(i) : SAI_STATUS_SUCCESS;
    }
    return SAI_STATUS_ITEM_NOT_FOUND;
}
sai_status_t create_macsec_sc(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (need_table() != SAI_STATUS_SUCCESS) return SAI_STATUS_NOT_SUPPORTED;
        MsSc sc;
        bool has_dir = false, has_flow = false, has_sci = false;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            sai_status_t fx = sc_fixed(a, i);
            if (fx == SAI_STATUS_SUCCESS) continue;
            if (fx != SAI_STATUS_ITEM_NOT_FOUND) return fx;
            switch (a.id) {
            case SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION: sc.dir = dir_of(a.value.s32); has_dir = true; break;
            case SAI_MACSEC_SC_ATTR_FLOW_ID:
                if (!S().flows.count(a.value.oid)) return attr_invalid(i);
                sc.flow = a.value.oid;
                has_flow = true;
                break;
            // The SCI as SONiC passes it: the eight bytes of the SecTAG in memory order.
            case SAI_MACSEC_SC_ATTR_MACSEC_SCI: std::memcpy(sc.sci, &a.value.u64, 8); has_sci = true; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE:
                if (!cipher_name(a.value.s32)) return attr_invalid(i);
                sc.cipher = a.value.s32;
                break;
            case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE: sc.replay = a.value.booldata; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW: sc.window = a.value.u32; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_PORT_ID: break;
            default: return attr_not_supported(i);
            }
        }
        if (!has_dir || !has_flow || !has_sci) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (S().flows[sc.flow].dir != sc.dir) return SAI_STATUS_INVALID_PARAMETER;
        for (auto& o : S().scs)
            if (o.second.flow == sc.flow) {
                OB_ERR(SAI_API_MACSEC, "MACsec SC: its flow already has a secure channel (one per port and direction)");
                return SAI_STATUS_INSUFFICIENT_RESOURCES;
            }
        *id = ctx().alloc(SAI_OBJECT_TYPE_MACSEC_SC);
        S().scs[*id] = sc;
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t remove_macsec_sc(sai_object_id_t id)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().scs.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        for (auto& sa : S().sas)
            if (sa.second.sc == id) return SAI_STATUS_OBJECT_IN_USE;
        S().scs.erase(id);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t set_macsec_sc_attribute(sai_object_id_t id, const sai_attribute_t* attr)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().scs.find(id);
        if (it == S().scs.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        sai_status_t fx = sc_fixed(*attr, 0);
        if (fx != SAI_STATUS_ITEM_NOT_FOUND) return fx;
        MsSc& sc = it->second;
        bool has_sa = false;
        for (auto& sa : S().sas) has_sa = has_sa || (sa.second.sc == id && sa.second.installed);
        switch (attr->id) {
        // These take effect for SAs installed from now on (MACsecOrch sets them before the first SA).
        case SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE:
            if (!cipher_name(attr->value.s32)) return attr_invalid(0);
            if (has_sa && attr->value.s32 != sc.cipher) return SAI_STATUS_OBJECT_IN_USE;
            sc.cipher = attr->value.s32;
            return SAI_STATUS_SUCCESS;
        case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE:
            if (has_sa && attr->value.booldata != sc.replay) return SAI_STATUS_OBJECT_IN_USE;
            sc.replay = attr->value.booldata;
            return SAI_STATUS_SUCCESS;
        case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW:
            if (has_sa && sc.replay && attr->value.u32 != sc.window) return SAI_STATUS_OBJECT_IN_USE;
            sc.window = attr->value.u32;
            return SAI_STATUS_SUCCESS;
        }
        return attr_not_supported(0);
    });
}
sai_status_t get_macsec_sc_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().scs.find(id);
        if (it == S().scs.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        MsSc& sc = it->second;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION: a.value.s32 = sc.dir == IN ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS; break;
            case SAI_MACSEC_SC_ATTR_FLOW_ID: a.value.oid = sc.flow; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_SCI: std::memcpy(&a.value.u64, sc.sci, 8); break;
            case SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE: a.value.s32 = sc.cipher; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE: a.value.booldata = sc.replay; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW: a.value.u32 = sc.window; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE:
            case SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE: a.value.booldata = true; break;
            case SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET:
            case SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET: a.value.u8 = 0; break;
            case SAI_MACSEC_SC_ATTR_USE_ES:
            case SAI_MACSEC_SC_ATTR_USE_SCB: a.value.booldata = false; break;
            case SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID:
                a.value.oid = sc.dir == EG && sc.hw >= 0 ? S().ports[sc.hw].d[EG].tx : SAI_NULL_OBJECT_ID;
                break;
            case SAI_MACSEC_SC_ATTR_SA_LIST: {
                std::vector<sai_object_id_t> v;
                for (auto& sa : S().sas) if (sa.second.sc == id) v.push_back(sa.first);
                sai_status_t st = fill_list(a.value.objlist, v);
                if (st != SAI_STATUS_SUCCESS) return st;
                break;
            }
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_sc_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t*, uint64_t*)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!S().scs.count(id)) return SAI_STATUS_INVALID_OBJECT_ID;
        return n ? SAI_STATUS_NOT_SUPPORTED : SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_sc_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t, uint64_t* out)
{
    return get_macsec_sc_stats(id, n, ids, out);
}
sai_status_t clear_macsec_sc_stats(sai_object_id_t, uint32_t, const sai_stat_id_t*) { return SAI_STATUS_NOT_SUPPORTED; }

// ============================================================ MACSEC_SA
void put_le64(std::vector<uint8_t>& v, uint64_t x)
{
    v.resize(8);
    for (int i = 0; i < 8; i++) v[i] = (uint8_t)(x >> (8 * i));
}
sai_status_t create_macsec_sa(sai_object_id_t* id, sai_object_id_t, uint32_t n, const sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        if (!id || (n && !attrs)) return SAI_STATUS_INVALID_PARAMETER;
        if (need_table() != SAI_STATUS_SUCCESS) return SAI_STATUS_NOT_SUPPORTED;
        MsSa sa;
        const sai_attribute_t *d = nullptr, *sc_a = nullptr, *an = nullptr, *sak = nullptr, *salt = nullptr, *auth = nullptr, *ssci = nullptr;
        for (uint32_t i = 0; i < n; i++) {
            const sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION: d = &a; break;
            case SAI_MACSEC_SA_ATTR_SC_ID: sc_a = &a; break;
            case SAI_MACSEC_SA_ATTR_AN:
                if (a.value.u8 > 3) return attr_invalid(i);
                an = &a;
                break;
            case SAI_MACSEC_SA_ATTR_SAK: sak = &a; break;
            case SAI_MACSEC_SA_ATTR_SALT: salt = &a; break;
            case SAI_MACSEC_SA_ATTR_AUTH_KEY: auth = &a; break;
            case SAI_MACSEC_SA_ATTR_MACSEC_SSCI: ssci = &a; break;
            case SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN:
            case SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN: sa.pn = a.value.u64; break;
            default: return attr_not_supported(i);
            }
        }
        if (!d || !sc_a || !an || !sak || !auth) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        auto sci = S().scs.find(sc_a->value.oid);
        if (sci == S().scs.end()) return SAI_STATUS_INVALID_PARAMETER;
        MsSc& sc = sci->second;
        sa.dir = dir_of(d->value.s32);
        if (sa.dir != sc.dir) return SAI_STATUS_INVALID_PARAMETER;
        sa.sc = sc_a->value.oid;
        sa.an = an->value.u8;
        for (auto& o : S().sas)
            if (o.second.sc == sa.sc && o.second.an == sa.an) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        bool xpn = cipher_xpn(sc.cipher);
        if (xpn && (!salt || !ssci)) return SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING;
        if (!xpn && sa.pn > 0xffffffffull) return SAI_STATUS_INVALID_PARAMETER;
        // SAI: a 128-bit SAK is the last 16 bytes of the 32
        unsigned kl = cipher_klen(sc.cipher);
        const uint8_t* k = sak->value.macsecsak + (kl == 16 ? 16 : 0);
        sa.par["key"].assign(k, k + kl);
        sa.par["hkey"].assign(auth->value.macsecauthkey, auth->value.macsecauthkey + 16);
        put_le64(sa.par["pn"], sa.pn);
        if (xpn) {
            const uint8_t* st = salt->value.macsecsalt;
            uint32_t sv = ssci->value.u32;   // the SSCI as a number; on the wire most significant byte first
            std::vector<uint8_t> sb = {(uint8_t)(sv >> 24), (uint8_t)(sv >> 16), (uint8_t)(sv >> 8), (uint8_t)sv};
            sa.par["salt"].assign(st, st + 12);
            sa.par["ssci"] = sb;
            std::vector<uint8_t> iv(st, st + 12);
            for (int i = 0; i < 4; i++) iv[i] ^= sb[i];
            sa.par["iv"] = iv;
        }
        sa.order = ++S().order;
        sai_object_id_t oid = ctx().alloc(SAI_OBJECT_TYPE_MACSEC_SA);
        S().sas[oid] = sa;
        ms_wipe(sa.par);
        // Install now if the SC's flow is bound to a port, else when it is (reconcile).
        int hw = hw_of_flow(sc.flow);
        if (hw >= 0 && S().ports.count(hw) && S().ports[hw].d[sa.dir].enabled) {
            sai_status_t st = reconcile(hw, sa.dir, oid);
            if (st != SAI_STATUS_SUCCESS) {
                ms_wipe(S().sas[oid].par);
                S().sas.erase(oid);
                return st;
            }
        } else {
            OB_INFO(SAI_API_MACSEC, "MACsec %s SA AN %u: kept until its flow is bound to a port", kDir[sa.dir], sa.an);
        }
        *id = oid;
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t remove_macsec_sa(sai_object_id_t id)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().sas.find(id);
        if (it == S().sas.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        MsSa& sa = it->second;
        sai_status_t st = SAI_STATUS_SUCCESS;
        auto sc = S().scs.find(sa.sc);
        int hw = sc == S().scs.end() ? -1 : sc->second.hw;
        if (sa.installed && hw >= 0) {
            st = sa_uninstall(hw, sa, id);
            OB_NOTICE(SAI_API_MACSEC, "port %s: %s SA AN %u removed%s", S().ports[hw].name.c_str(), kDir[sa.dir], sa.an,
                      st == SAI_STATUS_SUCCESS ? "" : " (the sequence FAILED)");
        }
        int dir = sa.dir;
        ms_wipe(sa.par);
        S().sas.erase(it);
        if (hw >= 0) reconcile(hw, dir);
        return st;
    });
}
sai_status_t sa_pn(MsSa& sa, uint64_t& pn)
{
    pn = sa.pn;
    auto sc = S().scs.find(sa.sc);
    if (!sa.installed || sc == S().scs.end() || sc->second.hw < 0) return SAI_STATUS_SUCCESS;
    std::string seq = std::string("pn.") + kDir[sa.dir] + "." + std::to_string(sa.an);
    if (!S().tab.find(seq)) return SAI_STATUS_SUCCESS;
    MsResult res;
    sai_status_t st = run(sc->second.hw, sa.dir, seq, MsParams(), &res);
    if (st != SAI_STATUS_SUCCESS) return st;
    pn = ms_result_u64(res, "pn");
    if (!cipher_xpn(sc->second.cipher)) pn &= 0xffffffffull;
    sa.pn = pn;
    return SAI_STATUS_SUCCESS;
}
sai_status_t set_macsec_sa_attribute(sai_object_id_t id, const sai_attribute_t* attr)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().sas.find(id);
        if (it == S().sas.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!attr) return SAI_STATUS_INVALID_PARAMETER;
        // The lowest acceptable PN of an installed ingress SA moves with the PHY's replay
        // window; a KaY-driven update is accepted and not written (no sequence for it).
        if (attr->id == SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN && it->second.dir == IN) {
            if (!it->second.installed) {
                it->second.pn = attr->value.u64;
                if (!it->second.par.empty()) put_le64(it->second.par["pn"], attr->value.u64);
            }
            return SAI_STATUS_SUCCESS;
        }
        return attr_not_supported(0);
    });
}
sai_status_t get_macsec_sa_attribute(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().sas.find(id);
        if (it == S().sas.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && !attrs) return SAI_STATUS_INVALID_PARAMETER;
        MsSa& sa = it->second;
        for (uint32_t i = 0; i < n; i++) {
            sai_attribute_t& a = attrs[i];
            switch (a.id) {
            case SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION: a.value.s32 = sa.dir == IN ? SAI_MACSEC_DIRECTION_INGRESS : SAI_MACSEC_DIRECTION_EGRESS; break;
            case SAI_MACSEC_SA_ATTR_SC_ID: a.value.oid = sa.sc; break;
            case SAI_MACSEC_SA_ATTR_AN: a.value.u8 = sa.an; break;
            case SAI_MACSEC_SA_ATTR_CURRENT_XPN:          // egress: next PN; ingress: lowest acceptable PN
            case SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN:
            case SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN: {
                sai_status_t st = sa_pn(sa, a.value.u64);
                if (st != SAI_STATUS_SUCCESS) return st;
                break;
            }
            // SAK, SALT, AUTH_KEY, SSCI: write-only here (the key is not kept once it is in the PHY)
            default: return attr_not_supported(i);
            }
        }
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_sa_stats(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().sas.find(id);
        if (it == S().sas.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (n && (!ids || !out)) return SAI_STATUS_INVALID_PARAMETER;
        sai_status_t st = sa_poll(it->second);
        if (st != SAI_STATUS_SUCCESS) return st;
        for (uint32_t i = 0; i < n; i++) stats_fill(it->second.acc, stat_name(kSaStats, ids[i]), out[i]);
        return SAI_STATUS_SUCCESS;
    });
}
sai_status_t get_macsec_sa_stats_ext(sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids, sai_stats_mode_t mode, uint64_t* out)
{
    if (mode != SAI_STATS_MODE_READ) return SAI_STATUS_NOT_SUPPORTED;
    return get_macsec_sa_stats(id, n, ids, out);
}
sai_status_t clear_macsec_sa_stats(sai_object_id_t id, uint32_t, const sai_stat_id_t*)
{
    return guarded(SAI_API_MACSEC, __func__, [&]() -> sai_status_t {
        auto it = S().sas.find(id);
        if (it == S().sas.end()) return SAI_STATUS_INVALID_OBJECT_ID;
        if (!S().tab.clear_on_read) return SAI_STATUS_NOT_SUPPORTED;
        sa_poll(it->second);
        it->second.acc.clear();
        return SAI_STATUS_SUCCESS;
    });
}

sai_macsec_api_t make_macsec_api()
{
    sai_macsec_api_t a;
    std::memset(&a, 0, sizeof(a));
    a.create_macsec = create_macsec;
    a.remove_macsec = remove_macsec;
    a.set_macsec_attribute = set_macsec_attribute;
    a.get_macsec_attribute = get_macsec_attribute;
    a.create_macsec_port = create_macsec_port;
    a.remove_macsec_port = remove_macsec_port;
    a.set_macsec_port_attribute = set_macsec_port_attribute;
    a.get_macsec_port_attribute = get_macsec_port_attribute;
    a.get_macsec_port_stats = get_macsec_port_stats;
    a.get_macsec_port_stats_ext = get_macsec_port_stats_ext;
    a.clear_macsec_port_stats = clear_macsec_port_stats;
    a.create_macsec_flow = create_macsec_flow;
    a.remove_macsec_flow = remove_macsec_flow;
    a.set_macsec_flow_attribute = set_macsec_flow_attribute;
    a.get_macsec_flow_attribute = get_macsec_flow_attribute;
    a.get_macsec_flow_stats = get_macsec_flow_stats;
    a.get_macsec_flow_stats_ext = get_macsec_flow_stats_ext;
    a.clear_macsec_flow_stats = clear_macsec_flow_stats;
    a.create_macsec_sc = create_macsec_sc;
    a.remove_macsec_sc = remove_macsec_sc;
    a.set_macsec_sc_attribute = set_macsec_sc_attribute;
    a.get_macsec_sc_attribute = get_macsec_sc_attribute;
    a.get_macsec_sc_stats = get_macsec_sc_stats;
    a.get_macsec_sc_stats_ext = get_macsec_sc_stats_ext;
    a.clear_macsec_sc_stats = clear_macsec_sc_stats;
    a.create_macsec_sa = create_macsec_sa;
    a.remove_macsec_sa = remove_macsec_sa;
    a.set_macsec_sa_attribute = set_macsec_sa_attribute;
    a.get_macsec_sa_attribute = get_macsec_sa_attribute;
    a.get_macsec_sa_stats = get_macsec_sa_stats;
    a.get_macsec_sa_stats_ext = get_macsec_sa_stats_ext;
    a.clear_macsec_sa_stats = clear_macsec_sa_stats;
    return a;
}
} // namespace

const sai_macsec_api_t macsec_api = make_macsec_api();

// ============================================================ ACL tables of the MACsec stages (called from sai_acl.cpp)
bool macsec_acl_stage(uint32_t n, const sai_attribute_t* attrs)
{
    const sai_attribute_t* a = find_attr(n, attrs, SAI_ACL_TABLE_ATTR_ACL_STAGE);
    return a && (a->value.s32 == SAI_ACL_STAGE_INGRESS_MACSEC || a->value.s32 == SAI_ACL_STAGE_EGRESS_MACSEC);
}
bool macsec_acl_table(sai_object_id_t id) { return S().tables.count(id) != 0; }
bool macsec_acl_entry(sai_object_id_t id) { return S().entries.count(id) != 0; }

sai_status_t macsec_acl_table_create(sai_object_id_t* id, uint32_t n, const sai_attribute_t* attrs)
{
    if (need_table() != SAI_STATUS_SUCCESS) return SAI_STATUS_NOT_SUPPORTED;
    MsAclTable t;
    for (uint32_t i = 0; i < n; i++) {
        const sai_attribute_t& a = attrs[i];
        switch (a.id) {
        case SAI_ACL_TABLE_ATTR_ACL_STAGE: t.dir = a.value.s32 == SAI_ACL_STAGE_INGRESS_MACSEC ? IN : EG; break;
        case SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST:
            for (uint32_t k = 0; k < a.value.s32list.count; k++)
                if (a.value.s32list.list[k] != SAI_ACL_BIND_POINT_TYPE_PORT) return attr_not_supported(i);
            break;
        case SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST:
        case SAI_ACL_TABLE_ATTR_SIZE:
            break;
        // What the PHY's fixed classification can stand for: EAPOL by EtherType (the DMAC is not looked at), the SC by its SCI.
        case SAI_ACL_TABLE_ATTR_FIELD_DST_MAC:
        case SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE:
        case SAI_ACL_TABLE_ATTR_FIELD_MACSEC_SCI:
            if (a.value.booldata) t.fields.insert(a.id);
            break;
        default:
            if (a.id >= SAI_ACL_TABLE_ATTR_FIELD_START && a.id <= SAI_ACL_TABLE_ATTR_FIELD_END && !a.value.booldata) break;
            return attr_not_supported(i);
        }
    }
    *id = ctx().alloc(SAI_OBJECT_TYPE_ACL_TABLE);
    S().tables[*id] = t;
    return SAI_STATUS_SUCCESS;
}
sai_status_t macsec_acl_table_remove(sai_object_id_t id)
{
    MsAclTable& t = S().tables[id];
    if (!t.entries.empty() || t.port) return SAI_STATUS_OBJECT_IN_USE;
    S().tables.erase(id);
    return SAI_STATUS_SUCCESS;
}
sai_status_t macsec_acl_table_get(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    MsAclTable& t = S().tables[id];
    for (uint32_t i = 0; i < n; i++) {
        sai_attribute_t& a = attrs[i];
        if (a.id == SAI_ACL_TABLE_ATTR_ACL_STAGE) a.value.s32 = t.dir == IN ? SAI_ACL_STAGE_INGRESS_MACSEC : SAI_ACL_STAGE_EGRESS_MACSEC;
        else if (a.id >= SAI_ACL_TABLE_ATTR_FIELD_START && a.id <= SAI_ACL_TABLE_ATTR_FIELD_END) a.value.booldata = t.fields.count(a.id) != 0;
        else if (a.id == SAI_ACL_TABLE_ATTR_ENTRY_LIST) {
            sai_status_t st = fill_list(a.value.objlist, t.entries);
            if (st != SAI_STATUS_SUCCESS) return st;
        } else if (a.id == SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY) a.value.u32 = 16;
        else return attr_not_supported(i);
    }
    return SAI_STATUS_SUCCESS;
}

static sai_status_t entry_attr(MsAclEntry& e, MsAclTable& t, const sai_attribute_t& a, uint32_t i)
{
    switch (a.id) {
    case SAI_ACL_ENTRY_ATTR_PRIORITY: e.priority = a.value.u32; return SAI_STATUS_SUCCESS;
    case SAI_ACL_ENTRY_ATTR_ADMIN_STATE: e.admin = a.value.booldata; return SAI_STATUS_SUCCESS;
    case SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC:
    case SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE:
    case SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI:
        if (a.value.aclfield.enable) e.fields[a.id] = a.value.aclfield;
        else e.fields.erase(a.id);
        return SAI_STATUS_SUCCESS;
    case SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION:
        if (a.value.aclaction.enable && a.value.aclaction.parameter.s32 != SAI_PACKET_ACTION_FORWARD &&
            a.value.aclaction.parameter.s32 != SAI_PACKET_ACTION_DROP)
            return attr_not_supported(i);
        e.action_on = a.value.aclaction.enable;
        e.action = a.value.aclaction.parameter.s32;
        return SAI_STATUS_SUCCESS;
    case SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW:
        if (a.value.aclaction.enable) {
            auto f = S().flows.find(a.value.aclaction.parameter.oid);
            if (f == S().flows.end() || f->second.dir != t.dir) return attr_invalid(i);
        }
        e.flow_on = a.value.aclaction.enable;
        e.flow = a.value.aclaction.enable ? a.value.aclaction.parameter.oid : 0;
        return SAI_STATUS_SUCCESS;
    }
    return attr_not_supported(i);
}
// Entries are a model of the PHY's fixed rules; say so once when one cannot be honoured.
static void entry_note(const MsAclEntry& e, const MsAclTable& t)
{
    bool eapol = e.fields.count(SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE) != 0;
    if (eapol && e.action_on && e.action == SAI_PACKET_ACTION_DROP)
        OB_WARN(SAI_API_ACL, "MACsec ACL: an entry that drops an EtherType is not programmed (the PHY passes EAPOL in clear, always)");
    if (!eapol && e.admin && e.action_on && e.action == SAI_PACKET_ACTION_FORWARD && !e.flow_on && S().tab.find(std::string("gate-close.") + kDir[t.dir]))
        OB_WARN(SAI_API_ACL, "MACsec ACL: a data entry that forwards in clear is not programmed: data frames are dropped until a "
                             "secure channel carries them");
}
sai_status_t macsec_acl_entry_create(sai_object_id_t* id, uint32_t n, const sai_attribute_t* attrs)
{
    const sai_attribute_t* ta = find_attr(n, attrs, SAI_ACL_ENTRY_ATTR_TABLE_ID);
    MsAclTable& t = S().tables[ta->value.oid];
    MsAclEntry e;
    e.table = ta->value.oid;
    for (uint32_t i = 0; i < n; i++) {
        if (attrs[i].id == SAI_ACL_ENTRY_ATTR_TABLE_ID) continue;
        sai_status_t st = entry_attr(e, t, attrs[i], i);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    entry_note(e, t);
    *id = ctx().alloc(SAI_OBJECT_TYPE_ACL_ENTRY);
    S().entries[*id] = e;
    t.entries.push_back(*id);
    if (t.port) reconcile_port(t.port, t.dir);
    return SAI_STATUS_SUCCESS;
}
sai_status_t macsec_acl_entry_remove(sai_object_id_t id)
{
    MsAclEntry e = S().entries[id];
    S().entries.erase(id);
    auto t = S().tables.find(e.table);
    if (t != S().tables.end()) {
        auto& v = t->second.entries;
        v.erase(std::remove(v.begin(), v.end(), id), v.end());
        if (t->second.port) reconcile_port(t->second.port, t->second.dir);
    }
    return SAI_STATUS_SUCCESS;
}
sai_status_t macsec_acl_entry_set(sai_object_id_t id, const sai_attribute_t* attr)
{
    MsAclEntry& e = S().entries[id];
    auto t = S().tables.find(e.table);
    if (t == S().tables.end()) return SAI_STATUS_FAILURE;
    MsAclEntry n = e;
    sai_status_t st = entry_attr(n, t->second, *attr, 0);
    if (st != SAI_STATUS_SUCCESS) return st;
    e = n;
    entry_note(e, t->second);
    if (t->second.port) reconcile_port(t->second.port, t->second.dir);
    return SAI_STATUS_SUCCESS;
}
sai_status_t macsec_acl_entry_get(sai_object_id_t id, uint32_t n, sai_attribute_t* attrs)
{
    MsAclEntry& e = S().entries[id];
    for (uint32_t i = 0; i < n; i++) {
        sai_attribute_t& a = attrs[i];
        switch (a.id) {
        case SAI_ACL_ENTRY_ATTR_TABLE_ID: a.value.oid = e.table; break;
        case SAI_ACL_ENTRY_ATTR_PRIORITY: a.value.u32 = e.priority; break;
        case SAI_ACL_ENTRY_ATTR_ADMIN_STATE: a.value.booldata = e.admin; break;
        case SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION:
            a.value.aclaction.enable = e.action_on;
            a.value.aclaction.parameter.s32 = e.action;
            break;
        case SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW:
            a.value.aclaction.enable = e.flow_on;
            a.value.aclaction.parameter.oid = e.flow;
            break;
        default: {
            auto f = e.fields.find(a.id);
            if (f == e.fields.end()) {
                if (a.id < SAI_ACL_ENTRY_ATTR_FIELD_START || a.id > SAI_ACL_ENTRY_ATTR_FIELD_END) return attr_not_supported(i);
                a.value.aclfield.enable = false;
            } else {
                a.value.aclfield = f->second;
            }
        }
        }
    }
    return SAI_STATUS_SUCCESS;
}

// PORT INGRESS_MACSEC_ACL / EGRESS_MACSEC_ACL / MACSEC_PORT_LIST (called from sai_port.cpp)
sai_status_t port_macsec_set(PortObj& p, const sai_attribute_t& a, uint32_t i, bool& handled)
{
    if (a.id != SAI_PORT_ATTR_INGRESS_MACSEC_ACL && a.id != SAI_PORT_ATTR_EGRESS_MACSEC_ACL) return SAI_STATUS_SUCCESS;
    handled = true;
    int dir = a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL ? IN : EG;
    sai_object_id_t old = table_of_port(p.oid, dir);
    if (a.value.oid != SAI_NULL_OBJECT_ID) {
        auto t = S().tables.find(a.value.oid);
        if (t == S().tables.end() || t->second.dir != dir || (t->second.port && t->second.port != p.oid)) return attr_invalid(i);
        if (!S().ports.count(p.hw)) return attr_not_supported(i);
        if (old && old != a.value.oid) S().tables[old].port = 0;
        t->second.port = p.oid;
    } else if (old) {
        S().tables[old].port = 0;
    }
    reconcile(p.hw, dir);
    return SAI_STATUS_SUCCESS;
}
sai_status_t port_macsec_get(PortObj& p, sai_attribute_t& a, uint32_t i, bool& handled)
{
    if (a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL || a.id == SAI_PORT_ATTR_EGRESS_MACSEC_ACL) {
        handled = true;
        a.value.oid = table_of_port(p.oid, a.id == SAI_PORT_ATTR_INGRESS_MACSEC_ACL ? IN : EG);
    } else if (a.id == SAI_PORT_ATTR_MACSEC_PORT_LIST) {
        handled = true;
        std::vector<sai_object_id_t> v;
        for (auto& m : S().mports) if (m.second.port == p.oid) v.push_back(m.first);
        return fill_list(a.value.objlist, v);
    }
    return SAI_STATUS_SUCCESS;
}
// Average inter-frame gap (bit times) the port needs while it has a MACsec port object; 0 = none.
uint32_t macsec_port_ifg(int hw)
{
    if (!S().loaded || !S().tab.ifg_bits) return 0;
    for (auto& m : S().mports)
        if (m.second.hw == hw) return S().tab.ifg_bits;
    return 0;
}

sai_status_t macsec_stats_capability(sai_object_type_t t, sai_stat_capability_list_t* out)
{
    std::vector<sai_stat_capability_t> v;
    auto add = [&](sai_stat_id_t id, const char* name) {
        bool have = false;   // a statistic the board table reads somewhere
        for (auto& sq : S().tab.seqs) {
            if (sq.first.compare(0, 6, "stats-") != 0) continue;
            for (auto& o : sq.second) have = have || (o.op == 'X' && o.res == name);
        }
        if (!have) return;
        sai_stat_capability_t s;
        s.stat_enum = id;
        s.stat_modes = SAI_STATS_MODE_READ;
        v.push_back(s);
    };
    std::lock_guard<std::recursive_mutex> g(ctx().mu);
    if (t == SAI_OBJECT_TYPE_MACSEC_SA) for (auto& e : kSaStats) add(e.id, e.name);
    else if (t == SAI_OBJECT_TYPE_MACSEC_FLOW) for (auto& e : kFlowStats) add(e.id, e.name);
    else return SAI_STATUS_NOT_SUPPORTED;
    return fill_list(*out, v);
}

// ============================================================ table life cycle
sai_status_t ext_macsec_load(const BackendConfig& cfg)
{
    State& s = S();
    s = State();
    std::string path = cfg.get("SAI_OPENBCM_EXT_MACSEC_FILE");
    if (path.empty()) return SAI_STATUS_SUCCESS;
    std::ifstream f(path);
    if (!f) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_MACSEC_FILE %s: cannot open; refusing to create the switch", path.c_str());
        return SAI_STATUS_FAILURE;
    }
    std::stringstream ss;
    ss << f.rdbuf();
    std::string err;
    if (!ext_macsec_parse(ss.str(), s.tab, err)) {
        OB_ERR(SAI_API_SWITCH, "SAI_OPENBCM_EXT_MACSEC_FILE %s: %s; refusing to create the switch", path.c_str(), err.c_str());
        s.tab = MsTable();
        return SAI_STATUS_INVALID_PARAMETER;
    }
    s.loaded = true;
    return SAI_STATUS_SUCCESS;
}

sai_status_t ext_macsec_attach(Backend& be)
{
    State& s = S();
    if (!s.loaded) return SAI_STATUS_SUCCESS;
    std::vector<PortInfo> ports = be.ports();   // (a backend without MDIO access fails the first sequence: NOT_SUPPORTED)
    for (auto& e : s.tab.ports) {
        int hw = -1;
        std::string name;
        for (auto& pi : ports)
            if (!pi.lanes.empty() && pi.lanes[0] == e.first) {
                hw = pi.hw;
                name = pi.name;
            }
        if (hw < 0) {
            OB_ERR(SAI_API_SWITCH, "external MACsec table: no port starts at lane %u", e.first);
            return SAI_STATUS_INVALID_PARAMETER;
        }
        s.ports[hw].phy = e.second;
        s.ports[hw].name = name;
    }
    s.be = &be;
    // A previous process (a syncd that was stopped or died) may have left engines on, with its keys. This
    // process has no MACsec objects yet, so every engine goes back to bypass ("probe" tells which are not).
    unsigned left = 0;
    for (auto& p : s.ports)
        for (int d = 0; d < 2; d++) {
            std::string probe = std::string("probe.") + kDir[d];
            if (!s.tab.find(probe)) continue;
            MsResult res;
            if (run(p.first, d, probe, MsParams(), &res) != SAI_STATUS_SUCCESS) continue;   // logged; the port's MACsec calls will fail too
            if (ms_result_u64(res, "bypass") != 0) continue;
            left++;
            run(p.first, d, std::string("reset.") + kDir[d], MsParams(), nullptr, true);
            run(p.first, d, std::string("disable.") + kDir[d], MsParams(), nullptr);
        }
    if (left) OB_NOTICE(SAI_API_SWITCH, "external MACsec table: %u engines were left on by a previous process: reset and back in bypass", left);
    OB_NOTICE(SAI_API_SWITCH, "external MACsec table: %zu ports, %zu sequences, counters %s, inter-frame gap %u bit times with MACsec on",
              s.ports.size(), s.tab.seqs.size(), s.tab.clear_on_read ? "clear on read" : "absolute", s.tab.ifg_bits);
    return SAI_STATUS_SUCCESS;
}

void ext_macsec_detach()
{
    State& s = S();
    for (auto& sa : s.sas) ms_wipe(sa.second.par);
    s = State();
}
bool ext_macsec_active() { return S().be != nullptr; }

std::string ext_macsec_status()
{
    State& s = S();
    if (!s.be) return "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n";
    std::string out;
    char b[400];
    std::snprintf(b, sizeof(b), "extmacsec: %zu ports, %zu sequences; objects: %zu macsec, %zu ports, %zu flows, %zu SCs, %zu SAs, %zu ACL tables, %zu ACL entries\n",
                  s.ports.size(), s.tab.seqs.size(), s.objs.size(), s.mports.size(), s.flows.size(), s.scs.size(), s.sas.size(),
                  s.tables.size(), s.entries.size());
    out += b;
    out += "port      phy  dir     engine gate   AN0 AN1 AN2 AN3 tx   runs fails  max_us last failure\n";
    for (auto& p : s.ports)
        for (int d = 0; d < 2; d++) {
            Hw& h = p.second.d[d];
            if (!h.enabled && !h.fails) continue;   // engines in bypass that never failed: nothing to say
            int tx = -1;
            for (int an = 0; an < 4; an++) if (h.sa[an] && h.sa[an] == h.tx) tx = an;
            std::snprintf(b, sizeof(b), "%-8s %4x  %-7s %-6s %-6s %-3s %-3s %-3s %-3s %-3s %5llu %5llu %7llu %s\n", p.second.name.c_str(),
                          p.second.phy, kDir[d], h.enabled ? "on" : "bypass", !h.enabled ? "-" : h.gate ? "open" : "closed",
                          h.sa[0] ? "sa" : "-", h.sa[1] ? "sa" : "-", h.sa[2] ? "sa" : "-", h.sa[3] ? "sa" : "-",
                          d == EG ? (tx >= 0 ? std::to_string(tx).c_str() : "-") : "", (unsigned long long)h.runs,
                          (unsigned long long)h.fails, (unsigned long long)h.max_us, h.last_fail.c_str());
            out += b;
        }
    size_t bypass = 0;
    for (auto& p : s.ports) for (int d = 0; d < 2; d++) bypass += !p.second.d[d].enabled;
    out += "engines in bypass: " + std::to_string(bypass) + " of " + std::to_string(s.ports.size() * 2) + "\n";
    size_t pending = 0;
    for (auto& sa : s.sas) pending += !sa.second.installed;
    if (pending) out += "SAs waiting for their flow to be bound to a port: " + std::to_string(pending) + "\n";
    return out;
}

std::string ext_macsec_diag_run(const std::string& port, const std::string& seq, const std::string& param_file)
{
    State& s = S();
    if (!s.be) return "extmacsec: no external MACsec table\n";
    int hw = -1;
    for (auto& p : s.ports)
        if (p.second.name == port) hw = p.first;
    if (hw < 0) return "extmacsec: no such port in the table: " + port + "\n";
    int dir = seq.find(".ingress") != std::string::npos ? IN : EG;
    MsParams par;
    if (!param_file.empty()) {
        struct stat sb;
        if (::stat(param_file.c_str(), &sb) != 0 || (sb.st_mode & 077) || sb.st_uid != 0)
            return "extmacsec: parameter file must exist, belong to root and be mode 0600 or 0400\n";
        std::ifstream f(param_file);
        std::string name, hex;
        while (f >> name >> hex) {
            std::vector<uint8_t> v;
            for (size_t i = 0; i + 1 < hex.size(); i += 2) v.push_back((uint8_t)std::strtoul(hex.substr(i, 2).c_str(), nullptr, 16));
            par[name] = v;
            explicit_bzero(&hex[0], hex.size());
        }
    }
    MsResult res;
    uint64_t t0 = now_us();
    sai_status_t st = run(hw, dir, seq, par, &res);
    ms_wipe(par);
    std::string out = "extmacsec: " + port + " " + seq + ": " + (st == SAI_STATUS_SUCCESS ? "ok" : "FAILED (" + std::to_string(st) + "): " + s.ports[hw].d[dir].last_fail) +
                      ", " + std::to_string(now_us() - t0) + " us\n";
    for (auto& r : res) out += "  " + r.first + " " + std::to_string(ms_result_u64(res, r.first)) + "\n";
    return out;
}

} // namespace ob

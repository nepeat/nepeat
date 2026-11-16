// SPDX-License-Identifier: Apache-2.0
// Field processor / policers / CoPP ACL (tl_acl.h).
//
// The frontend's ACL tables (fp_group/fp_entry) are not mapped yet:
// NOT_IMPLEMENTED. Policers are backend objects; a policer used by a CoPP
// trap (HostifArea::trap_program) becomes one IFCS acl_meter per inno
// block, created on first use and updated by policer_set.
//
// CoPP table, as isai 6.2.0 builds it on this chip (oracle traces
// 2026-11-16; call forms of SDK/scripts/sonic_l3snake_acl.py): an ingress
// match profile (DMAC, ether type, L4 protocol, L4 ports, L4 byte 1, hostif
// trap), an action profile (drop, copy to CPU, copy override, meter,
// counter), one control table with scope PER_IB, and one ACL per inno block
// added to it (the number of IBs is probed: ACL create with IB n until the
// SDK refuses). A trap rule is one ACE per IB: its matches, actions
// COPY_TO_CPU (the trap's hostif trap, all colours unless policed),
// COPY_TO_CPU_OVERRIDE (policed colours lose the copy), DROP (SAI TRAP),
// METER and a colour-aware counter. Meter units as isai uses them: packet
// policers at 84 B per packet (CIR = pps * 672 bit/s, CBS = packets * 84 B),
// byte policers at CIR * 8 bit/s, CBS bytes (I: the vendor's conversion).
#include "tl_acl.h"

namespace ob {
namespace tlb {

static constexpr int MAX_IBS = 8;               // probe limit
static constexpr uint32_t COPP_TABLE_SIZE = 128;   // entries per IB (isai: 128)
static constexpr uint32_t COPP_TABLE_PRIORITY = 11;

sai_status_t AclArea::group_create(const std::vector<FpQual>&, int, int& gid) { gid = -1; return c_.not_impl("ACL / field processor"); }
sai_status_t AclArea::group_destroy(int) { return c_.not_impl("ACL / field processor"); }
sai_status_t AclArea::entry_create(int, const FpRule&, int& eid) { eid = -1; return c_.not_impl("ACL / field processor"); }
sai_status_t AclArea::entry_destroy(int) { return c_.not_impl("ACL / field processor"); }
sai_status_t AclArea::entry_counter(int, uint64_t&, uint64_t&) { return c_.not_impl("ACL / field processor"); }

sai_status_t AclArea::policer_create(const PolicerCfg& cfg, int& pid)
{
    Guard g(c_.mu);
    pid = next_policer_++;
    policers_[pid] = cfg;
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::policer_set(int pid, const PolicerCfg& cfg)
{
    Guard g(c_.mu);
    auto it = policers_.find(pid);
    if (it == policers_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    it->second = cfg;
    std::vector<ifcs_attr_t> a;
    meter_attrs(cfg, a);
    sai_status_t worst = SAI_STATUS_SUCCESS;
    for (auto& m : meters_) {
        if (m.first.first != pid) continue;
        // one attribute per call, as isai updates a meter
        for (auto& x : a) {
            sai_status_t st = c_.call(c_.L.ifcs_acl_meter_attr_set(NODE, m.second, 1, &x), "acl_meter_attr_set");
            if (st != SAI_STATUS_SUCCESS) worst = st;
        }
    }
    return worst;
}

sai_status_t AclArea::policer_destroy(int pid)
{
    Guard g(c_.mu);
    for (auto& i : installs_)
        if (i.second.policer == pid) return SAI_STATUS_OBJECT_IN_USE;
    for (auto it = meters_.begin(); it != meters_.end();) {
        if (it->first.first != pid) { ++it; continue; }
        c_.call(c_.L.ifcs_acl_meter_delete(NODE, it->second), "acl_meter_delete");
        it = meters_.erase(it);
    }
    return policers_.erase(pid) ? SAI_STATUS_SUCCESS : SAI_STATUS_ITEM_NOT_FOUND;
}

// --------------------------------------------------------------- CoPP table
static ifcs_attr_t attr_u32list(uint32_t id, std::vector<uint32_t>& v)
{
    ifcs_attr_t a = attr_of(id);
    a.value.u32_list.count = (uint32_t)v.size();
    a.value.u32_list.arr = v.data();
    return a;
}

sai_status_t AclArea::copp_init()
{
    if (copp_up_) return SAI_STATUS_SUCCESS;
    std::vector<uint32_t> mt = {IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS, IFCS_ACL_MATCH_TYPE_ETHER_TYPE,
                                IFCS_ACL_MATCH_TYPE_L4_PROTOCOL,            IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT,
                                IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT,    IFCS_ACL_MATCH_TYPE_L4_BYTE1,
                                IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP};
    std::vector<uint32_t> at = {IFCS_ACL_ACTION_TYPE_DROP, IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
                                IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE, IFCS_ACL_ACTION_TYPE_METER,
                                IFCS_ACL_ACTION_TYPE_ACL_COUNTER};
    ifcs_attr_t mp[3] = {attr_u32list(IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES, mt),
                         attr_u32(IFCS_ACL_MATCH_PROFILE_ATTR_DIRECTION, IFCS_ACL_DIRECTION_INGRESS),
                         attr_u32(IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH, 320)};
    mprof_ = aprof_ = table_ = IFCS_NULL_HANDLE;   // IFCS creates need a null handle to allocate one
    sai_status_t st = c_.call(c_.L.ifcs_acl_match_profile_create(NODE, &mprof_, 3, mp), "acl_match_profile_create (copp)");
    if (st != SAI_STATUS_SUCCESS) {
        mprof_ = IFCS_NULL_HANDLE;
        return st;
    }
    ifcs_attr_t ap[2] = {attr_u32list(IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES, at),
                         attr_u32(IFCS_ACL_ACTION_PROFILE_ATTR_DIRECTION, IFCS_ACL_DIRECTION_INGRESS)};
    st = c_.call(c_.L.ifcs_acl_action_profile_create(NODE, &aprof_, 2, ap), "acl_action_profile_create (copp)");
    if (st != SAI_STATUS_SUCCESS) {
        aprof_ = IFCS_NULL_HANDLE;
        copp_rollback();
        return st;
    }
    // The table as isai creates it, then simpler forms (this SDK refuses some
    // attributes isai 6.2 uses: each refused form is logged, I).
    struct Form { const char* what; bool type, maxes, node; };
    static const Form forms[] = {{"per IB, isai form (type control, meter/counter maxima)", true, true, false},
                                 {"per IB, without type", false, true, false},
                                 {"node scope, isai form", true, true, true},
                                 {"node scope, without type", false, true, true}};
    int r = IFCS_PARAM;
    for (auto& f : forms) {
        std::vector<ifcs_attr_t> tv = {attr_handle(IFCS_ACL_TABLE_ATTR_MATCH_PROFILE, mprof_),
                                       attr_handle(IFCS_ACL_TABLE_ATTR_ACTION_PROFILE, aprof_),
                                       attr_u32(IFCS_ACL_TABLE_ATTR_DIRECTION, IFCS_ACL_DIRECTION_INGRESS),
                                       attr_u32(IFCS_ACL_TABLE_ATTR_SCOPE, f.node ? IFCS_ACL_SCOPE_NODE : IFCS_ACL_SCOPE_PER_IB),
                                       attr_u32(IFCS_ACL_TABLE_ATTR_SIZE, COPP_TABLE_SIZE)};
        if (f.maxes) {
            // own counters per entry need the table's default counters off
            tv.push_back(attr_u32(IFCS_ACL_TABLE_ATTR_MAX_METERS, COPP_TABLE_SIZE));
            tv.push_back(attr_u32(IFCS_ACL_TABLE_ATTR_MAX_COUNTERS, COPP_TABLE_SIZE));
            tv.push_back(attr_data(IFCS_ACL_TABLE_ATTR_DEFAULT_STATS, false));
        }
        if (f.type) tv.push_back(attr_u32(IFCS_ACL_TABLE_ATTR_TYPE, IFCS_ACL_TABLE_TYPE_CONTROL));
        tv.push_back(attr_u32(IFCS_ACL_TABLE_ATTR_PRIORITY, COPP_TABLE_PRIORITY));
        table_ = IFCS_NULL_HANDLE;
        r = IFCS_STATUS_REASON(c_.L.ifcs_acl_table_create(NODE, &table_, (uint32_t)tv.size(), tv.data()));
        if (r == IFCS_SUCCESS) {
            OB_NOTICE(SAI_API_HOSTIF, "CoPP ACL table: %s", f.what);
            node_scope_ = f.node;
            break;
        }
        OB_WARN(SAI_API_HOSTIF, "CoPP ACL table, %s: %s", f.what, c_.reason(r));
        table_ = IFCS_NULL_HANDLE;
    }
    if (r != IFCS_SUCCESS) {
        copp_rollback();
        return map_reason(r);
    }
    // One ACL per inno block: probe until the SDK refuses an IB number
    // (node scope: one ACL).
    acls_.clear();
    for (int ib = 0; ib < (node_scope_ ? 1 : MAX_IBS); ib++) {
        ifcs_attr_t a[2] = {attr_u32(IFCS_ACL_ATTR_SCOPE, node_scope_ ? IFCS_ACL_SCOPE_NODE : IFCS_ACL_SCOPE_PER_IB),
                            attr_u32(IFCS_ACL_ATTR_IB, (uint32_t)ib)};
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        int rr = IFCS_STATUS_REASON(c_.L.ifcs_acl_create(NODE, &h, node_scope_ ? 1 : 2, a));
        if (rr != IFCS_SUCCESS) {
            if (ib == 0) OB_ERR(SAI_API_HOSTIF, "CoPP: acl_create (IB 0): %s", c_.reason(rr));
            break;
        }
        acls_.push_back(h);
    }
    if (acls_.empty()) {
        OB_ERR(SAI_API_HOSTIF, "CoPP: no ACL could be created (IB 0 refused)");
        copp_rollback();
        return SAI_STATUS_FAILURE;
    }
    st = c_.call(c_.L.ifcs_acl_table_acl_add(NODE, table_, (uint32_t)acls_.size(), acls_.data(), 0, nullptr),
                 "acl_table_acl_add (copp)");
    if (st != SAI_STATUS_SUCCESS) {
        copp_rollback();
        return st;
    }
    copp_up_ = true;
    OB_NOTICE(SAI_API_HOSTIF, "CoPP ACL table ready: %zu inno blocks", acls_.size());
    return SAI_STATUS_SUCCESS;
}

// A failed CoPP table setup: take back what was created (the next trap tries again).
void AclArea::copp_rollback()
{
    for (auto h : acls_) c_.call(c_.L.ifcs_acl_delete(NODE, h), "acl_delete (copp rollback)");
    acls_.clear();
    if (table_) c_.call(c_.L.ifcs_acl_table_delete(NODE, table_), "acl_table_delete (copp rollback)");
    if (aprof_) c_.call(c_.L.ifcs_acl_action_profile_delete(NODE, aprof_), "acl_action_profile_delete (copp rollback)");
    if (mprof_) c_.call(c_.L.ifcs_acl_match_profile_delete(NODE, mprof_), "acl_match_profile_delete (copp rollback)");
    mprof_ = aprof_ = table_ = IFCS_NULL_HANDLE;
}

uint32_t AclArea::copp_prio(uint32_t offset)
{
    ChipLimits l;
    c_.sw_limits(l);
    uint32_t p = l.acl_prio_min + offset;
    if (l.acl_prio_max > l.acl_prio_min && p > l.acl_prio_max) p = l.acl_prio_max;
    return p;
}

void AclArea::meter_attrs(const PolicerCfg& p, std::vector<ifcs_attr_t>& a)
{
    // packets at 84 B (64 B frame + preamble + IPG) per packet, as isai converts
    auto rate = [&](uint64_t r) -> uint64_t { return p.packets ? r * 84 * 8 : r * 8; };
    auto burst = [&](uint64_t b) -> uint64_t { return p.packets ? b * 84 : b; };
    a.clear();
    ifcs_attr_t x = attr_of(IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE);
    x.value.u64 = rate(p.cir);
    a.push_back(x);
    x = attr_of(IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE);
    x.value.u64 = burst(p.cbs ? p.cbs : 1);
    a.push_back(x);
    if (p.tr_tcm) {
        x = attr_of(IFCS_ACL_METER_ATTR_EXCESS_INFORMATION_RATE);
        x.value.u64 = rate(p.pir);
        a.push_back(x);
    }
    x = attr_of(IFCS_ACL_METER_ATTR_EXCESS_BURST_SIZE);
    x.value.u64 = p.pbs ? burst(p.pbs) : 1;   // sr_tcm without PBS: 1 (isai)
    a.push_back(x);
}

sai_status_t AclArea::meter_for(int pid, int ib, ifcs_handle_t& h)
{
    auto k = std::make_pair(pid, ib);
    auto it = meters_.find(k);
    if (it != meters_.end()) { h = it->second; return SAI_STATUS_SUCCESS; }
    auto p = policers_.find(pid);
    if (p == policers_.end()) return SAI_STATUS_INVALID_PARAMETER;
    std::vector<ifcs_attr_t> a = {attr_u32(IFCS_ACL_METER_ATTR_TYPE, IFCS_ACL_METER_TYPE_BYTE),
                                  attr_u32(IFCS_ACL_METER_ATTR_MODE, p->second.tr_tcm ? IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2698
                                                                                       : IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2697),
                                  attr_data(IFCS_ACL_METER_ATTR_COLOR_AWARE, !p->second.color_blind)};
    std::vector<ifcs_attr_t> r;
    meter_attrs(p->second, r);
    a.insert(a.end(), r.begin(), r.end());
    a.push_back(attr_handle(IFCS_ACL_METER_ATTR_ACL_TABLE, table_));
    a.push_back(attr_u32(IFCS_ACL_METER_ATTR_SCOPE, node_scope_ ? IFCS_ACL_SCOPE_NODE : IFCS_ACL_SCOPE_PER_IB));
    if (!node_scope_) a.push_back(attr_u32(IFCS_ACL_METER_ATTR_IB, (uint32_t)ib));
    h = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_acl_meter_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_meter_create");
    if (st == SAI_STATUS_SUCCESS) meters_[k] = h;
    return st;
}

static ifcs_attr_t attr_bytes(uint32_t id, std::vector<uint8_t>& v)
{
    ifcs_attr_t a = attr_of(id);
    a.value.u8_list.count = (uint32_t)v.size();
    a.value.u8_list.arr = v.data();
    return a;
}

sai_status_t AclArea::install_ib(const std::vector<CoppRule>& rules, uint32_t prio, const CoppActions& act, int ib,
                                 Install::PerIb& out)
{
    out.ib = ib;
    // counter (colour-aware), shared by this install's ACEs in this IB
    uint32_t scope = node_scope_ ? IFCS_ACL_SCOPE_NODE : IFCS_ACL_SCOPE_PER_IB;
    std::vector<ifcs_attr_t> ca = {attr_handle(IFCS_ACL_COUNTER_ATTR_ACL_TABLE, table_), attr_u32(IFCS_ACL_COUNTER_ATTR_SCOPE, scope),
                                   attr_u32(IFCS_ACL_COUNTER_ATTR_MODE, IFCS_ACL_COUNTER_MODE_COLOR_AWARE)};
    if (!node_scope_) ca.push_back(attr_u32(IFCS_ACL_COUNTER_ATTR_IB, (uint32_t)ib));
    sai_status_t st = c_.call(c_.L.ifcs_acl_counter_create(NODE, &out.counter, (uint32_t)ca.size(), ca.data()), "acl_counter_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_handle_t meter = IFCS_NULL_HANDLE;
    if (act.policer >= 0) {
        st = meter_for(act.policer, ib, meter);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    for (auto& rule : rules) {
        std::vector<ifcs_handle_t> ms, as;
        for (auto& m : rule) {
            std::vector<uint8_t> v = m.value;
            std::vector<ifcs_attr_t> a = {attr_u32(IFCS_ACL_MATCH_ATTR_TYPE, m.type)};
            if (m.object) a.push_back(attr_handle(IFCS_ACL_MATCH_ATTR_OBJECT, m.object));
            else a.push_back(attr_bytes(IFCS_ACL_MATCH_ATTR_VALUE, v));
            a.push_back(attr_handle(IFCS_ACL_MATCH_ATTR_MATCH_PROFILE, mprof_));
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            st = c_.call(c_.L.ifcs_acl_match_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_match_create");
            if (st != SAI_STATUS_SUCCESS) return st;
            ms.push_back(h);
            out.matches.push_back(h);
        }
        auto action = [&](uint32_t type, const void* val, size_t n, ifcs_handle_t obj) -> sai_status_t {
            std::vector<ifcs_attr_t> a = {attr_handle(IFCS_ACL_ACTION_ATTR_ACTION_PROFILE, aprof_),
                                          attr_u32(IFCS_ACL_ACTION_ATTR_TYPE, type)};
            if (val) {
                ifcs_attr_t v = attr_of(IFCS_ACL_ACTION_ATTR_VALUE);
                std::memcpy(&v.value, val, n);
                a.push_back(v);
            } else {
                a.push_back(attr_handle(IFCS_ACL_ACTION_ATTR_OBJECT, obj));
            }
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            sai_status_t s = c_.call(c_.L.ifcs_acl_action_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_action_create");
            if (s == SAI_STATUS_SUCCESS) { as.push_back(h); out.actions.push_back(h); }
            return s;
        };
        bool policed = act.policer >= 0;
        if (act.trap) {
            ifcs_acl_ctc_action_t ctc;
            std::memset(&ctc, 0, sizeof(ctc));
            ctc.hostif = act.trap;
            ctc.green = ctc.yellow = ctc.red = IFCS_ACL_ACTION_ENABLE;
            if (policed && act.yellow_drop) ctc.yellow = IFCS_ACL_ACTION_DISABLE;
            if (policed && act.red_drop) ctc.red = IFCS_ACL_ACTION_DISABLE;
            ctc.dirty = 0x0f;
            st = action(IFCS_ACL_ACTION_TYPE_COPY_TO_CPU, &ctc, sizeof(ctc), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
            if (policed && (act.yellow_drop || act.red_drop)) {
                ifcs_acl_ctc_override_action_t o;
                std::memset(&o, 0, sizeof(o));
                o.yellow = act.yellow_drop ? IFCS_ACL_ACTION_ENABLE : IFCS_ACL_ACTION_DISABLE;
                o.red = act.red_drop ? IFCS_ACL_ACTION_ENABLE : IFCS_ACL_ACTION_DISABLE;
                o.dirty = 0x07;
                st = action(IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE, &o, sizeof(o), 0);
                if (st != SAI_STATUS_SUCCESS) return st;
            }
        }
        if (act.drop) {
            ifcs_acl_drop_action_t d;
            std::memset(&d, 0, sizeof(d));
            d.green = d.yellow = d.red = IFCS_ACL_ACTION_ENABLE;
            d.dirty = 0x07;
            st = action(IFCS_ACL_ACTION_TYPE_DROP, &d, sizeof(d), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (meter) {
            st = action(IFCS_ACL_ACTION_TYPE_METER, nullptr, 0, meter);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        st = action(IFCS_ACL_ACTION_TYPE_ACL_COUNTER, nullptr, 0, out.counter);
        if (st != SAI_STATUS_SUCCESS) return st;
        std::vector<ifcs_attr_t> e = {attr_u32(IFCS_ACE_ATTR_SCOPE, scope), attr_u32(IFCS_ACE_ATTR_PRIORITY, prio),
                                      attr_data(IFCS_ACE_ATTR_ENABLE, true), attr_of(IFCS_ACE_ATTR_MATCH_SET),
                                      attr_of(IFCS_ACE_ATTR_ACTION_SET)};
        e[3].value.handle_list.count = (uint32_t)ms.size();
        e[3].value.handle_list.list = ms.data();
        e[4].value.handle_list.count = (uint32_t)as.size();
        e[4].value.handle_list.list = as.data();
        if (!node_scope_) e.push_back(attr_u32(IFCS_ACE_ATTR_IB, (uint32_t)ib));
        ifcs_handle_t ace = IFCS_NULL_HANDLE;
        st = c_.call(c_.L.ifcs_ace_create(NODE, &ace, (uint32_t)e.size(), e.data()), "ace_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        out.aces.push_back(ace);
    }
    if (!out.aces.empty())
        st = c_.call(c_.L.ifcs_acl_ace_add(NODE, acls_[(size_t)ib], (uint32_t)out.aces.size(), out.aces.data(), 0, nullptr),
                     "acl_ace_add");
    if (st != SAI_STATUS_SUCCESS) out.aces.clear();   // not in the ACL: remove_ib must not acl_ace_remove them
    return st;
}

void AclArea::remove_ib(Install::PerIb& p)
{
    if (!p.aces.empty() && (size_t)p.ib < acls_.size())
        c_.call(c_.L.ifcs_acl_ace_remove(NODE, acls_[(size_t)p.ib], (uint32_t)p.aces.size(), p.aces.data()), "acl_ace_remove");
    for (auto h : p.aces) c_.call(c_.L.ifcs_ace_delete(NODE, h), "ace_delete");
    for (auto h : p.matches) c_.call(c_.L.ifcs_acl_match_delete(NODE, h), "acl_match_delete");
    for (auto h : p.actions) c_.call(c_.L.ifcs_acl_action_delete(NODE, h), "acl_action_delete");
    if (p.counter) c_.call(c_.L.ifcs_acl_counter_delete(NODE, p.counter), "acl_counter_delete");
    p = Install::PerIb();
}

sai_status_t AclArea::copp_install(const std::vector<CoppRule>& rules, uint32_t prio, const CoppActions& a, int& id)
{
    Guard g(c_.mu);
    sai_status_t st = copp_init();
    if (st != SAI_STATUS_SUCCESS) return st;
    if (a.policer >= 0 && !policers_.count(a.policer)) return SAI_STATUS_INVALID_PARAMETER;
    Install in;
    in.policer = a.policer;
    for (int ib = 0; ib < (int)acls_.size(); ib++) {
        Install::PerIb p;
        st = install_ib(rules, prio, a, ib, p);
        in.ibs.push_back(p);
        if (st != SAI_STATUS_SUCCESS) {
            for (auto& x : in.ibs) remove_ib(x);
            return st;
        }
    }
    id = next_install_++;
    installs_[id] = in;
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::copp_remove(int id)
{
    Guard g(c_.mu);
    auto it = installs_.find(id);
    if (it == installs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (auto& p : it->second.ibs) remove_ib(p);
    installs_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::copp_counters(int id, uint64_t& packets, uint64_t& green, uint64_t& yellow, uint64_t& red)
{
    Guard g(c_.mu);
    auto it = installs_.find(id);
    if (it == installs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    packets = green = yellow = red = 0;
    for (auto& p : it->second.ibs) {
        ifcs_acl_counter_stats_id_t ids[4] = {IFCS_ACL_COUNTER_STATS_ID_PACKETS, IFCS_ACL_COUNTER_STATS_ID_GREEN_PACKETS,
                                              IFCS_ACL_COUNTER_STATS_ID_YELLOW_PACKETS, IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS};
        uint64_t v[4] = {0, 0, 0, 0};
        uint32_t n = 0;
        sai_status_t st = c_.call(c_.L.ifcs_acl_counter_stats_get(NODE, p.counter, 4, ids, v, &n), "acl_counter_stats_get");
        if (st != SAI_STATUS_SUCCESS) return st;
        packets += v[0];
        green += v[1];
        yellow += v[2];
        red += v[3];
    }
    return SAI_STATUS_SUCCESS;
}

void AclArea::forget()
{
    Guard g(c_.mu);
    installs_.clear();
    meters_.clear();
    acls_.clear();
    copp_up_ = false;
    mprof_ = aprof_ = table_ = 0;
}

} // namespace tlb
} // namespace ob

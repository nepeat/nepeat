// SPDX-License-Identifier: Apache-2.0
// Field processor / policers / CoPP ACL (tl_acl.h).
//
// The frontend's ACL tables (fp_group/fp_entry): see "SAI ACL tables"
// below. Policers are backend objects; a policer used by a CoPP
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

#include "tl_l3.h"
#include "tl_lag.h"
#include "tl_mirror.h"

#include <algorithm>
#include <cstdio>

namespace ob {
namespace tlb {

static constexpr int MAX_IBS = 8;               // probe limit
static constexpr uint32_t COPP_TABLE_SIZE = 128;   // entries per IB (isai: 128)
static constexpr uint32_t COPP_TABLE_PRIORITY = 11;


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
    std::vector<std::pair<std::pair<int, int>, ifcs_handle_t>> all(meters_.begin(), meters_.end());
    all.insert(all.end(), umeters_.begin(), umeters_.end());
    for (auto& m : all) {
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
    for (auto& e : uentries_)
        if (e.second.rule.policer == pid) return SAI_STATUS_OBJECT_IN_USE;
    for (auto it = umeters_.begin(); it != umeters_.end();) {
        if (it->first.first != pid) { ++it; continue; }
        c_.call(c_.L.ifcs_acl_meter_delete(NODE, it->second), "acl_meter_delete");
        it = umeters_.erase(it);
    }
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

// --------------------------------------------------------- SAI ACL tables
// As isai 6.2.0 does on this chip (oracle acl, 2026-11-16): per table a match
// profile (the table's fields, PORT_SHARING), an action profile (drop, copy,
// copy override, counter; meter for SET_POLICER), a node-scope table (size,
// DEFAULT_STATS off, MAX_COUNTERS) and one ACL in it; the ACL is bound to
// ports with acl_port_add (the frontend gives each entry the ports it
// applies to; the backend binds the union). An entry is one ACE (matches,
// actions, a counter), or one ACE per port with an INGRESS_L2_INTERFACE
// match when it covers only part of the table's ports (IN_PORTS). Values in
// network order with a mask. ACE priority 0xffffffff - 2 x SAI priority
// (lower wins; isai's mapping); table priority 12..15 (CoPP's table is 11).
static constexpr uint32_t UACL_SIZE = 256;
static constexpr int COPP_FP_PRIORITY = 1000;    // the frontend's own CoPP group (not used with trap_program)

bool AclArea::ugroup_has(const UGroup& g, FpQual q) const
{
    for (auto x : g.qset)
        if (x == q) return true;
    return false;
}

static void push_types(std::vector<uint32_t>& v, std::initializer_list<uint32_t> t)
{
    for (uint32_t x : t)
        if (std::find(v.begin(), v.end(), x) == v.end()) v.push_back(x);
}

sai_status_t AclArea::group_create(const std::vector<FpQual>& qset, int priority, int& gid)
{
    return group_make(qset, priority, FpGroupExtra(), false, gid);
}

sai_status_t AclArea::group_create_ex(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, int& gid)
{
    if (x.out_ports && !x.egress) {
        OB_ERR(SAI_API_ACL, "ACL table: OUT_PORTS only in egress tables here");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    if (x.egress && (x.redirect || x.set_tc || x.set_dscp || x.mirror)) {
        OB_ERR(SAI_API_ACL, "ACL table: egress tables take drop / forward / counter actions only");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    return group_make(qset, priority, x, false, gid);
}

// Egress tables: direction EGRESS in the profiles and the table; the port a
// per-port ACE matches is the egress sysport (OUT_PORTS too), the ACL is
// bound with acl_port_add as for ingress (isai, oracle acl). Ranges: the
// profile's SRC/DST_L4_PORT_COMPARATOR flags, one comparator group per entry.
sai_status_t AclArea::group_make(const std::vector<FpQual>& qset, int priority, const FpGroupExtra& x, bool dmac_hit, int& gid)
{
    Guard g(c_.mu);
    gid = -1;
    const uint32_t dir = x.egress ? (uint32_t)IFCS_ACL_DIRECTION_EGRESS : (uint32_t)IFCS_ACL_DIRECTION_INGRESS;
    if (priority >= COPP_FP_PRIORITY) {
        // CoPP is programmed by HostifArea::trap_program; the frontend's field
        // entry CoPP (traps without a mapping) would police without a copy.
        c_.log_once("acl:copp-fp", "CoPP field group refused: trap types without an IFCS mapping stay unpoliced");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    UGroup u;
    u.qset = qset;
    u.priority = priority;
    u.x = x;
    u.dmac_hit = dmac_hit;
    for (auto q : qset)
        if (q == FpQual::SrcIp6 || q == FpQual::DstIp6) u.v6 = true;
    std::vector<uint32_t> mt;
    for (auto q : qset) {
        switch (q) {
        case FpQual::InPorts:   // egress tables: no port match (isai's egress profile has none; ours with one: PARAM)
            if (!x.egress) push_types(mt, {IFCS_ACL_MATCH_TYPE_INGRESS_L2_INTERFACE});
            break;
        case FpQual::EtherType: push_types(mt, {IFCS_ACL_MATCH_TYPE_ETHER_TYPE}); break;
        case FpQual::IpType:
            push_types(mt, {IFCS_ACL_MATCH_TYPE_L3_TYPE, IFCS_ACL_MATCH_TYPE_ARP_OPERATION, IFCS_ACL_MATCH_TYPE_ETHER_TYPE});
            break;
        case FpQual::IpProtocol: push_types(mt, {IFCS_ACL_MATCH_TYPE_L4_PROTOCOL}); break;
        case FpQual::SrcIp: push_types(mt, {IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS}); break;
        case FpQual::DstIp: push_types(mt, {IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS}); break;
        case FpQual::SrcIp6: push_types(mt, {IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS}); break;
        case FpQual::DstIp6: push_types(mt, {IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS}); break;
        case FpQual::L4SrcPort: push_types(mt, {IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT}); break;
        case FpQual::L4DstPort: push_types(mt, {IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT}); break;
        case FpQual::TcpFlags: push_types(mt, {IFCS_ACL_MATCH_TYPE_TCP_FLAGS}); break;
        case FpQual::Tos: push_types(mt, {IFCS_ACL_MATCH_TYPE_DSCP, IFCS_ACL_MATCH_TYPE_ECN}); break;
        case FpQual::DstMac: push_types(mt, {IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS}); break;
        case FpQual::SrcMac: push_types(mt, {IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS}); break;
        case FpQual::OuterVlan: push_types(mt, {IFCS_ACL_MATCH_TYPE_VLAN_ID}); break;
        case FpQual::Ttl: push_types(mt, {IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL}); break;
        case FpQual::IcmpTypeCode:
            // isai: ICMP type/code in the L4 source port field (v4 tables),
            // L4 bytes 1 and 2 (v6 tables); the protocol is matched too
            if (u.v6) push_types(mt, {IFCS_ACL_MATCH_TYPE_L4_BYTE1, IFCS_ACL_MATCH_TYPE_L4_BYTE2});
            else push_types(mt, {IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT});
            push_types(mt, {IFCS_ACL_MATCH_TYPE_L4_PROTOCOL});
            break;
        default:
            OB_ERR(SAI_API_ACL, "ACL table: qualifier %d has no IFCS match here", (int)q);
            return SAI_STATUS_NOT_SUPPORTED;
        }
    }
    if (dmac_hit) push_types(mt, {IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT});
    // 0.12.41 needs MAX_WIDTH (isai's 0.16 does not): the smallest width the
    // fields fit by our estimate of their key bits, wider ones on refusal
    uint32_t bits = 0;
    for (uint32_t t : mt) {
        switch (t) {
        case IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS: case IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS: bits += 128; break;
        case IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS: case IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS: bits += 48; break;
        case IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS: case IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS: bits += 32; break;
        case IFCS_ACL_MATCH_TYPE_L3_TYPE: case IFCS_ACL_MATCH_TYPE_ECN: bits += 4; break;
        case IFCS_ACL_MATCH_TYPE_L4_PROTOCOL: case IFCS_ACL_MATCH_TYPE_TCP_FLAGS: case IFCS_ACL_MATCH_TYPE_DSCP:
        case IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL: case IFCS_ACL_MATCH_TYPE_L4_BYTE1: case IFCS_ACL_MATCH_TYPE_L4_BYTE2: bits += 8; break;
        case IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT: bits += 1; break;
        default: bits += 16; break;   // ports, ether type, VLAN, ARP operation, the in-port
        }
    }
    bits += (x.src_range ? 8 : 0) + (x.dst_range ? 8 : 0);   // comparator results (I)
    static const uint32_t widths[] = {80, 160, 320, 480};
    std::vector<uint32_t> try_widths;
    for (uint32_t w : widths)
        if (w >= bits) try_widths.push_back(w);
    if (try_widths.empty()) try_widths.push_back(480);
    // With a meter (SET_POLICER) first, then without it, then smaller; per key width.
    struct Form { bool meters; uint32_t size; };
    static const Form forms[] = {{true, UACL_SIZE}, {false, UACL_SIZE}, {false, 128}};
    uint32_t tprio = x.egress ? 6u : (uint32_t)std::max(12, std::min(15, 16 - priority));   // isai: egress 6
    int r = IFCS_PARAM;
    uint32_t width = 0;
    for (uint32_t w : try_widths) {
        std::vector<ifcs_attr_t> mp = {attr_u32(IFCS_ACL_MATCH_PROFILE_ATTR_DIRECTION, dir),
                                       attr_u32(IFCS_ACL_MATCH_PROFILE_ATTR_PORT_SHARING, 1),
                                       attr_u32list(IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES, mt),
                                       attr_u32(IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH, w)};
        if (x.src_range) mp.push_back(attr_data(IFCS_ACL_MATCH_PROFILE_ATTR_SRC_L4_PORT_COMPARATOR, true));
        if (x.dst_range) mp.push_back(attr_data(IFCS_ACL_MATCH_PROFILE_ATTR_DST_L4_PORT_COMPARATOR, true));
        u.mprof = IFCS_NULL_HANDLE;
        r = IFCS_STATUS_REASON(c_.L.ifcs_acl_match_profile_create(NODE, &u.mprof, (uint32_t)mp.size(), mp.data()));
        if (r != IFCS_SUCCESS) {
            OB_WARN(SAI_API_ACL, "ACL match profile, %zu types, width %u: %s", mt.size(), w, c_.reason(r));
            u.mprof = 0;
            continue;
        }
        for (auto& f : forms) {
            std::vector<uint32_t> at = {IFCS_ACL_ACTION_TYPE_DROP, IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
                                        IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE, IFCS_ACL_ACTION_TYPE_ACL_COUNTER};
            if (f.meters) at.push_back(IFCS_ACL_ACTION_TYPE_METER);
            if (x.egress) at = {IFCS_ACL_ACTION_TYPE_DROP, IFCS_ACL_ACTION_TYPE_ACL_COUNTER};
            if (x.redirect) at.push_back(IFCS_ACL_ACTION_TYPE_REDIRECT);
            if (x.set_tc) at.push_back(IFCS_ACL_ACTION_TYPE_SET_TC);
            if (x.set_dscp) at.push_back(IFCS_ACL_ACTION_TYPE_SET_DSCP);
            if (x.mirror) at.push_back(IFCS_ACL_ACTION_TYPE_MIRROR);
            if (x.egress && f.meters) continue;   // no meters on egress tables
            ifcs_attr_t ap[2] = {attr_u32(IFCS_ACL_ACTION_PROFILE_ATTR_DIRECTION, dir),
                                 attr_u32list(IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES, at)};
            u.aprof = IFCS_NULL_HANDLE;
            r = IFCS_STATUS_REASON(c_.L.ifcs_acl_action_profile_create(NODE, &u.aprof, 2, ap));
            if (r != IFCS_SUCCESS) { u.aprof = 0; continue; }
            std::vector<ifcs_attr_t> tv = {attr_u32(IFCS_ACL_TABLE_ATTR_DIRECTION, dir),
                                           attr_u32(IFCS_ACL_TABLE_ATTR_SIZE, f.size),
                                           attr_data(IFCS_ACL_TABLE_ATTR_DEFAULT_STATS, false),
                                           attr_u32(IFCS_ACL_TABLE_ATTR_MAX_COUNTERS, f.size),
                                           attr_handle(IFCS_ACL_TABLE_ATTR_MATCH_PROFILE, u.mprof),
                                           attr_handle(IFCS_ACL_TABLE_ATTR_ACTION_PROFILE, u.aprof),
                                           attr_u32(IFCS_ACL_TABLE_ATTR_SCOPE, IFCS_ACL_SCOPE_NODE),
                                           attr_u32(IFCS_ACL_TABLE_ATTR_PRIORITY, tprio)};
            if (f.meters) tv.push_back(attr_u32(IFCS_ACL_TABLE_ATTR_MAX_METERS, f.size));
            u.table = IFCS_NULL_HANDLE;
            r = IFCS_STATUS_REASON(c_.L.ifcs_acl_table_create(NODE, &u.table, (uint32_t)tv.size(), tv.data()));
            if (r == IFCS_SUCCESS) {
                u.meters = f.meters;
                u.size = f.size;
                break;
            }
            OB_WARN(SAI_API_ACL, "ACL table (width %u, size %u%s): %s", w, f.size, f.meters ? ", meters" : "", c_.reason(r));
            u.table = 0;
            c_.call(c_.L.ifcs_acl_action_profile_delete(NODE, u.aprof), "acl_action_profile_delete");
            u.aprof = 0;
        }
        if (r == IFCS_SUCCESS) { width = w; break; }
        c_.call(c_.L.ifcs_acl_match_profile_delete(NODE, u.mprof), "acl_match_profile_delete");
        u.mprof = 0;
    }
    if (!u.mprof) return map_reason(r);
    auto undo = [&]() {
        if (u.acl) c_.call(c_.L.ifcs_acl_delete(NODE, u.acl), "acl_delete");
        if (u.table) c_.call(c_.L.ifcs_acl_table_delete(NODE, u.table), "acl_table_delete");
        if (u.aprof) c_.call(c_.L.ifcs_acl_action_profile_delete(NODE, u.aprof), "acl_action_profile_delete");
        if (u.mprof) c_.call(c_.L.ifcs_acl_match_profile_delete(NODE, u.mprof), "acl_match_profile_delete");
    };
    if (r != IFCS_SUCCESS) {
        undo();
        return map_reason(r);
    }
    ifcs_attr_t aa = attr_u32(IFCS_ACL_ATTR_SCOPE, IFCS_ACL_SCOPE_NODE);
    u.acl = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_acl_create(NODE, &u.acl, 1, &aa), "acl_create (acl)");
    if (st == SAI_STATUS_SUCCESS)
        st = c_.call(c_.L.ifcs_acl_table_acl_add(NODE, u.table, 1, &u.acl, 0, nullptr), "acl_table_acl_add (acl)");
    else
        u.acl = 0;
    if (st != SAI_STATUS_SUCCESS) {
        undo();
        return st;
    }
    gid = next_ugroup_++;
    ugroups_[gid] = u;
    OB_NOTICE(SAI_API_ACL, "ACL table %d: IFCS %s table 0x%x, %zu match types, width %u, size %u, priority %u%s%s%s", gid,
              x.egress ? "egress" : "ingress", u.table, mt.size(), width, u.size, tprio,
              u.meters || x.egress ? "" : ", no meters (SET_POLICER refused)", x.src_range || x.dst_range ? ", ranges" : "",
              x.redirect || x.set_tc || x.set_dscp || x.mirror ? ", redirect/tc/dscp/mirror actions" : "");
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::group_destroy(int gid)
{
    Guard g(c_.mu);
    auto it = ugroups_.find(gid);
    if (it == ugroups_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (auto& e : uentries_)
        if (e.second.gid == gid) return SAI_STATUS_OBJECT_IN_USE;
    UGroup& u = it->second;
    std::vector<ifcs_handle_t> sp;
    for (int hw : u.bound)
        if (c_.sysport.count(hw)) sp.push_back(c_.sysport.at(hw));
    if (!sp.empty()) c_.call(c_.L.ifcs_acl_port_remove(NODE, u.acl, (uint32_t)sp.size(), sp.data()), "acl_port_remove");
    for (auto m = umeters_.begin(); m != umeters_.end();) {
        if (m->first.second != gid) { ++m; continue; }
        c_.call(c_.L.ifcs_acl_meter_delete(NODE, m->second), "acl_meter_delete");
        m = umeters_.erase(m);
    }
    c_.call(c_.L.ifcs_acl_table_acl_remove(NODE, u.table, 1, &u.acl), "acl_table_acl_remove");
    c_.call(c_.L.ifcs_acl_delete(NODE, u.acl), "acl_delete");
    c_.call(c_.L.ifcs_acl_table_delete(NODE, u.table), "acl_table_delete");
    c_.call(c_.L.ifcs_acl_match_profile_delete(NODE, u.mprof), "acl_match_profile_delete");
    c_.call(c_.L.ifcs_acl_action_profile_delete(NODE, u.aprof), "acl_action_profile_delete");
    ugroups_.erase(it);
    return SAI_STATUS_SUCCESS;
}

static CoppMatch mbytes(uint32_t type, const uint8_t* v, const uint8_t* m, size_t n)
{
    CoppMatch x;
    x.type = type;
    x.value.assign(v, v + n);
    x.mask.assign(m, m + n);
    return x;
}
static CoppMatch mu8(uint32_t type, uint8_t v, uint8_t m) { return mbytes(type, &v, &m, 1); }
static CoppMatch mu16(uint32_t type, uint16_t v, uint16_t m)
{
    uint8_t a[2] = {(uint8_t)(v >> 8), (uint8_t)v}, b[2] = {(uint8_t)(m >> 8), (uint8_t)m};
    return mbytes(type, a, b, 2);
}

// The matches of a rule (without the port), validated against the table.
sai_status_t AclArea::uentry_matches(const UGroup& g, const FpRule& r, std::vector<CoppMatch>& out)
{
    const FpMatch& m = r.m;
    out.clear();
    for (int q = 0; q < (int)FpQual::Count; q++) {
        FpQual fq = (FpQual)q;
        if (!m.has(fq) || fq == FpQual::InPorts) continue;
        if (!ugroup_has(g, fq)) {
            OB_ERR(SAI_API_ACL, "ACL entry: qualifier %d is not in its table", q);
            return SAI_STATUS_INVALID_PARAMETER;
        }
    }
    bool ether_set = m.has(FpQual::EtherType), proto_set = m.has(FpQual::IpProtocol);
    if (ether_set) out.push_back(mu16(IFCS_ACL_MATCH_TYPE_ETHER_TYPE, m.ether, m.ether_mask));
    if (m.has(FpQual::IpType)) {
        switch (m.ip_type) {
        case SAI_ACL_IP_TYPE_IP: out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L3_TYPE, IFCS_ACL_MATCH_L3_TYPE_IPV4, 0x04)); break;
        case SAI_ACL_IP_TYPE_NON_IP: out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L3_TYPE, 0x00, 0x04)); break;
        case SAI_ACL_IP_TYPE_IPV4ANY: out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L3_TYPE, IFCS_ACL_MATCH_L3_TYPE_IPV4, 0x06)); break;
        case SAI_ACL_IP_TYPE_IPV6ANY: out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L3_TYPE, IFCS_ACL_MATCH_L3_TYPE_IPV6, 0x06)); break;
        case SAI_ACL_IP_TYPE_ARP: case SAI_ACL_IP_TYPE_ARP_REQUEST: case SAI_ACL_IP_TYPE_ARP_REPLY:
            if (!ether_set) out.push_back(mu16(IFCS_ACL_MATCH_TYPE_ETHER_TYPE, 0x0806, 0xffff));
            if (m.ip_type != SAI_ACL_IP_TYPE_ARP)
                out.push_back(mu16(IFCS_ACL_MATCH_TYPE_ARP_OPERATION, m.ip_type == SAI_ACL_IP_TYPE_ARP_REQUEST ? 1 : 2, 0xffff));
            break;
        default:
            OB_ERR(SAI_API_ACL, "ACL entry: ACL_IP_TYPE %d has no IFCS L3 type match (NON_IPV4 / NON_IPV6)", m.ip_type);
            return SAI_STATUS_NOT_SUPPORTED;
        }
    }
    if (proto_set) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, m.proto, m.proto_mask));
    if (m.has(FpQual::SrcIp)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS, m.src.addr, m.src_mask.addr, 4));
    if (m.has(FpQual::DstIp)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS, m.dst.addr, m.dst_mask.addr, 4));
    if (m.has(FpQual::SrcIp6)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS, m.src.addr, m.src_mask.addr, 16));
    if (m.has(FpQual::DstIp6)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS, m.dst.addr, m.dst_mask.addr, 16));
    if (m.has(FpQual::L4SrcPort)) out.push_back(mu16(IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, m.l4_src, m.l4_src_mask));
    if (m.has(FpQual::L4DstPort)) out.push_back(mu16(IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT, m.l4_dst, m.l4_dst_mask));
    if (m.has(FpQual::TcpFlags)) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_TCP_FLAGS, m.tcp, m.tcp_mask));
    if (m.has(FpQual::IcmpTypeCode)) {
        if (m.has(FpQual::L4SrcPort) && !g.v6) {
            OB_ERR(SAI_API_ACL, "ACL entry: ICMP type/code and L4 source port share one IFCS field");
            return SAI_STATUS_NOT_SUPPORTED;
        }
        if (!proto_set) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, g.v6 ? 58 : 1, 0xff));
        if (g.v6) {
            if (m.icmp_mask >> 8) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L4_BYTE1, (uint8_t)(m.icmp >> 8), (uint8_t)(m.icmp_mask >> 8)));
            if (m.icmp_mask & 0xff) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_L4_BYTE2, (uint8_t)m.icmp, (uint8_t)m.icmp_mask));
        } else {
            out.push_back(mu16(IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, m.icmp, m.icmp_mask));
        }
    }
    if (m.has(FpQual::Tos)) {
        if (m.tos_mask >> 2) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_DSCP, (uint8_t)(m.tos >> 2), (uint8_t)(m.tos_mask >> 2)));
        if (m.tos_mask & 3) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_ECN, (uint8_t)(m.tos & 3), (uint8_t)(m.tos_mask & 3)));
    }
    if (m.has(FpQual::DstMac)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS, m.dmac, m.dmac_mask, 6));
    if (m.has(FpQual::SrcMac)) out.push_back(mbytes(IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS, m.smac, m.smac_mask, 6));
    if (m.has(FpQual::OuterVlan)) out.push_back(mu16(IFCS_ACL_MATCH_TYPE_VLAN_ID, m.vlan & 0xfff, m.vlan_mask & 0xfff));
    if (m.has(FpQual::Ttl)) out.push_back(mu8(IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL, m.ttl, m.ttl_mask));
    return SAI_STATUS_SUCCESS;
}

// The shared hostif trap ACL COPY / TRAP actions copy with (queue 0, as isai's).
sai_status_t AclArea::acl_trap(ifcs_handle_t& h)
{
    if (!acl_trap_) {
        ifcs_attr_t q = attr_u32(IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
        ifcs_handle_t t = IFCS_NULL_HANDLE;
        sai_status_t st = c_.call(c_.L.ifcs_hostif_trap_create(NODE, &t, 1, &q), "hostif_trap_create (acl)");
        if (st != SAI_STATUS_SUCCESS) return st;
        acl_trap_ = t;
    }
    h = acl_trap_;
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::umeter_for(int pid, int gid, UGroup& g, ifcs_handle_t& h)
{
    auto k = std::make_pair(pid, gid);
    auto it = umeters_.find(k);
    if (it != umeters_.end()) { h = it->second; return SAI_STATUS_SUCCESS; }
    auto p = policers_.find(pid);
    if (p == policers_.end()) return SAI_STATUS_INVALID_PARAMETER;
    if (!g.meters) return SAI_STATUS_NOT_SUPPORTED;
    std::vector<ifcs_attr_t> a = {attr_u32(IFCS_ACL_METER_ATTR_TYPE, IFCS_ACL_METER_TYPE_BYTE),
                                  attr_u32(IFCS_ACL_METER_ATTR_MODE, p->second.tr_tcm ? IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2698
                                                                                       : IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2697),
                                  attr_data(IFCS_ACL_METER_ATTR_COLOR_AWARE, !p->second.color_blind)};
    std::vector<ifcs_attr_t> r;
    meter_attrs(p->second, r);
    a.insert(a.end(), r.begin(), r.end());
    a.push_back(attr_handle(IFCS_ACL_METER_ATTR_ACL_TABLE, g.table));
    a.push_back(attr_u32(IFCS_ACL_METER_ATTR_SCOPE, IFCS_ACL_SCOPE_NODE));
    h = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_acl_meter_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_meter_create (acl)");
    if (st == SAI_STATUS_SUCCESS) umeters_[k] = h;
    return st;
}

std::set<int> AclArea::ugroup_ports(int gid) const
{
    std::set<int> s;
    auto g = ugroups_.find(gid);
    bool egress = g != ugroups_.end() && g->second.x.egress;
    for (auto& e : uentries_) {
        if (e.second.gid != gid) continue;
        const FpMatch& m = e.second.rule.m;
        if (egress && !m.out_ports.empty()) {   // OUT_PORTS: the bound ports it names
            for (int hw : m.out_ports)
                if (std::find(m.ports.begin(), m.ports.end(), hw) != m.ports.end()) s.insert(hw);
        } else {
            s.insert(m.ports.begin(), m.ports.end());
        }
    }
    return s;
}

// Bind the table's ACL to exactly the ports its entries use.
sai_status_t AclArea::ugroup_bind(int gid, UGroup& g)
{
    std::set<int> want = ugroup_ports(gid);
    std::vector<ifcs_handle_t> add, del;
    std::vector<int> addp, delp;
    for (int hw : want)
        if (!g.bound.count(hw) && c_.sysport.count(hw)) { add.push_back(c_.sysport.at(hw)); addp.push_back(hw); }
    for (int hw : g.bound)
        if (!want.count(hw) && c_.sysport.count(hw)) { del.push_back(c_.sysport.at(hw)); delp.push_back(hw); }
    if (!add.empty()) {
        sai_status_t st = c_.call(c_.L.ifcs_acl_port_add(NODE, g.acl, (uint32_t)add.size(), add.data(), 0, nullptr), "acl_port_add");
        if (st != SAI_STATUS_SUCCESS) return st;
        g.bound.insert(addp.begin(), addp.end());
    }
    if (!del.empty()) {
        c_.call(c_.L.ifcs_acl_port_remove(NODE, g.acl, (uint32_t)del.size(), del.data()), "acl_port_remove");
        for (int hw : delp) g.bound.erase(hw);
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::uentry_install(UGroup& g, UEntry& e)
{
    const FpRule& r = e.rule;
    std::vector<CoppMatch> base;
    sai_status_t st = uentry_matches(g, r, base);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (e.dmac_hit >= 0) base.push_back(mu8(IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT, (uint8_t)e.dmac_hit, 0x01));
    if (r.m.l4_src_range || r.m.l4_dst_range) {
        // One comparator per range (OPERATION RANGE, VALUE / HI_VALUE inclusive)
        // in a group matched as COMPARATOR (isai, oracle acl).
        if ((r.m.l4_src_range && !g.x.src_range) || (r.m.l4_dst_range && !g.x.dst_range)) {
            OB_ERR(SAI_API_ACL, "ACL entry: L4 port range on a table without that range type");
            return SAI_STATUS_INVALID_PARAMETER;
        }
        const uint32_t dir = g.x.egress ? (uint32_t)IFCS_ACL_DIRECTION_EGRESS : (uint32_t)IFCS_ACL_DIRECTION_INGRESS;
        auto cmp = [&](uint32_t type, uint16_t lo, uint16_t hi) -> sai_status_t {
            ifcs_attr_t ca[5] = {attr_u32(IFCS_ACL_COMPARATOR_ATTR_TYPE, type), attr_u32(IFCS_ACL_COMPARATOR_ATTR_VALUE, lo),
                                 attr_u32(IFCS_ACL_COMPARATOR_ATTR_HI_VALUE, hi),
                                 attr_u32(IFCS_ACL_COMPARATOR_ATTR_OPERATION, IFCS_ACL_COMPARATOR_OP_TYPE_RANGE),
                                 attr_u32(IFCS_ACL_COMPARATOR_ATTR_DIRECTION, dir)};
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            sai_status_t s = c_.call(c_.L.ifcs_acl_comparator_create(NODE, &h, 5, ca), "acl_comparator_create");
            if (s == SAI_STATUS_SUCCESS) e.cmps.push_back(h);
            return s;
        };
        if (r.m.l4_src_range) st = cmp(IFCS_ACL_COMPARATOR_TYPE_SOURCE_L4_PORT, r.m.l4_src_min, r.m.l4_src_max);
        if (st == SAI_STATUS_SUCCESS && r.m.l4_dst_range)
            st = cmp(IFCS_ACL_COMPARATOR_TYPE_DESTINATION_L4_PORT, r.m.l4_dst_min, r.m.l4_dst_max);
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_attr_t ga = attr_of(IFCS_ACL_COMPARATOR_GROUP_ATTR_COMPARATORS);
        ga.value.handle_list.count = (uint32_t)e.cmps.size();
        ga.value.handle_list.list = e.cmps.data();
        e.cmp_group = IFCS_NULL_HANDLE;
        st = c_.call(c_.L.ifcs_acl_comparator_group_create(NODE, &e.cmp_group, 1, &ga), "acl_comparator_group_create");
        if (st != SAI_STATUS_SUCCESS) { e.cmp_group = 0; return st; }
        CoppMatch cm;
        cm.type = IFCS_ACL_MATCH_TYPE_COMPARATOR;
        cm.object = e.cmp_group;
        base.push_back(cm);
    }
    std::set<int> ports(r.m.ports.begin(), r.m.ports.end());
    if (g.x.egress && !r.m.out_ports.empty()) {   // OUT_PORTS: those of the bound ports
        std::set<int> o;
        for (int hw : r.m.out_ports)
            if (ports.count(hw)) o.insert(hw);
        ports.swap(o);
        if (ports.empty()) return SAI_STATUS_SUCCESS;   // matches nothing here
    }
    e.per_port = ports != g.bound;
    if (e.per_port && g.x.egress) {
        // egress ACEs have no port match here: an entry applies to every port
        // the table is bound to (the union of its entries' OUT_PORTS / bindings)
        OB_ERR(SAI_API_ACL, "egress ACL entry: covers part of the table's ports (needs a per-port match IFCS refuses)");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    std::vector<int> aces_for;   // one ACE per port when per_port, else one (-1)
    if (e.per_port) aces_for.assign(ports.begin(), ports.end());
    else aces_for.push_back(-1);
    if (r.counter && !e.counter) {
        // storm control entries count per colour (diag "acl": what the meter let through)
        bool colours = e.gid == storm_gid_;
        ifcs_attr_t ca[2] = {attr_handle(IFCS_ACL_COUNTER_ATTR_ACL_TABLE, g.table),
                             attr_u32(IFCS_ACL_COUNTER_ATTR_MODE, colours ? IFCS_ACL_COUNTER_MODE_COLOR_AWARE
                                                                          : IFCS_ACL_COUNTER_MODE_COLOR_BLIND)};
        e.counter = IFCS_NULL_HANDLE;
        st = c_.call(c_.L.ifcs_acl_counter_create(NODE, &e.counter, 2, ca), "acl_counter_create (acl)");
        if (st != SAI_STATUS_SUCCESS) { e.counter = 0; return st; }
    }
    ifcs_handle_t meter = 0, trap = 0;
    if (r.policer >= 0) {
        st = umeter_for(r.policer, e.gid, g, meter);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    if (r.copy_cpu) {
        st = acl_trap(trap);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    uint32_t sp = r.priority > 0x7ffffffeu ? 0x7ffffffeu : r.priority;
    uint32_t prio = 0xffffffffu - 2 * sp;
    for (int hw : aces_for) {
        std::vector<ifcs_handle_t> ms, as;
        std::vector<CoppMatch> mm = base;
        if (hw >= 0) {
            CoppMatch pm;
            pm.type = g.x.egress ? IFCS_ACL_MATCH_TYPE_EGRESS_SYSPORT : IFCS_ACL_MATCH_TYPE_INGRESS_L2_INTERFACE;
            pm.object = c_.sysport.count(hw) ? c_.sysport.at(hw) : 0;
            if (!pm.object) return SAI_STATUS_INVALID_PORT_NUMBER;
            mm.push_back(pm);
        }
        for (auto& m : mm) {
            std::vector<uint8_t> v = m.value, k = m.mask;
            std::vector<ifcs_attr_t> a = {attr_u32(IFCS_ACL_MATCH_ATTR_TYPE, m.type)};
            if (m.object) {
                a.push_back(attr_handle(IFCS_ACL_MATCH_ATTR_OBJECT, m.object));
            } else {
                a.push_back(attr_bytes(IFCS_ACL_MATCH_ATTR_MASK, k));
                a.push_back(attr_bytes(IFCS_ACL_MATCH_ATTR_VALUE, v));
            }
            a.push_back(attr_handle(IFCS_ACL_MATCH_ATTR_MATCH_PROFILE, g.mprof));
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            st = c_.call(c_.L.ifcs_acl_match_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_match_create (acl)");
            if (st != SAI_STATUS_SUCCESS) return st;
            ms.push_back(h);
            e.matches.push_back(h);
        }
        auto action = [&](uint32_t type, const void* val, size_t n, ifcs_handle_t obj) -> sai_status_t {
            std::vector<ifcs_attr_t> a = {attr_u32(IFCS_ACL_ACTION_ATTR_TYPE, type)};
            if (obj) {
                a.push_back(attr_handle(IFCS_ACL_ACTION_ATTR_OBJECT, obj));
            } else {
                ifcs_attr_t v = attr_of(IFCS_ACL_ACTION_ATTR_VALUE);
                if (val) std::memcpy(&v.value, val, n);
                a.push_back(v);
            }
            a.push_back(attr_handle(IFCS_ACL_ACTION_ATTR_ACTION_PROFILE, g.aprof));
            ifcs_handle_t h = IFCS_NULL_HANDLE;
            sai_status_t s = c_.call(c_.L.ifcs_acl_action_create(NODE, &h, (uint32_t)a.size(), a.data()), "acl_action_create (acl)");
            if (s == SAI_STATUS_SUCCESS) { as.push_back(h); e.actions.push_back(h); }
            return s;
        };
        if (r.copy_cpu) {
            ifcs_acl_ctc_action_t ctc;
            std::memset(&ctc, 0, sizeof(ctc));
            ctc.hostif = trap;
            ctc.green = ctc.yellow = ctc.red = IFCS_ACL_ACTION_ENABLE;
            ctc.dirty = 0x0f;
            st = action(IFCS_ACL_ACTION_TYPE_COPY_TO_CPU, &ctc, sizeof(ctc), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        // DROP: all colours (SAI DROP / TRAP), the policer's dropped colours, or
        // none at all (SAI FORWARD: a zero drop action, as isai programs it)
        ifcs_acl_drop_action_t d;
        std::memset(&d, 0, sizeof(d));
        bool any = r.drop || r.drop_cancel;
        if (r.drop) d.green = d.yellow = d.red = IFCS_ACL_ACTION_ENABLE;
        else if (r.policer >= 0 && (r.green_drop || r.yellow_drop || r.red_drop)) {
            d.green = r.green_drop ? IFCS_ACL_ACTION_ENABLE : IFCS_ACL_ACTION_DISABLE;
            d.yellow = r.yellow_drop ? IFCS_ACL_ACTION_ENABLE : IFCS_ACL_ACTION_DISABLE;
            d.red = r.red_drop ? IFCS_ACL_ACTION_ENABLE : IFCS_ACL_ACTION_DISABLE;
            any = true;
        }
        if (any || (!r.copy_cpu && !r.counter && r.policer < 0)) {
            if (d.green || d.yellow || d.red) d.dirty = 0x07;
            st = action(IFCS_ACL_ACTION_TYPE_DROP, d.dirty ? &d : nullptr, sizeof(d), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (r.redirect >= 0) {
            ifcs_handle_t dh = 0;
            bool ok = r.redirect_egress ? c_.l3->nexthop_handle(r.redirect, dh) : c_.lag->dest_handle(r.redirect, dh);
            if (!ok) {
                OB_ERR(SAI_API_ACL, "ACL entry: redirect target %d has no IFCS destination (unresolved next hop?)", r.redirect);
                return SAI_STATUS_INVALID_PARAMETER;
            }
            st = action(IFCS_ACL_ACTION_TYPE_REDIRECT, nullptr, 0, dh);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (r.set_tc >= 0) {
            ifcs_acl_set_tc_action_t t;
            std::memset(&t, 0, sizeof(t));
            t.green = t.yellow = t.red = IFCS_ACL_ACTION_ENABLE;
            t.green_value = t.yellow_value = t.red_value = (uint32_t)r.set_tc;
            t.dirty = 0x3f;
            st = action(IFCS_ACL_ACTION_TYPE_SET_TC, &t, sizeof(t), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (r.set_dscp >= 0) {
            ifcs_acl_set_dscp_action_t d2;
            std::memset(&d2, 0, sizeof(d2));
            d2.addr_family = g.v6 ? IFCS_IP_ADDR_FAMILY_IPV6 : IFCS_IP_ADDR_FAMILY_IPV4;
            d2.value = (uint32_t)r.set_dscp;
            d2.dirty = 0x03;
            st = action(IFCS_ACL_ACTION_TYPE_SET_DSCP, &d2, sizeof(d2), 0);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        for (int dirn = 0; dirn < 2; dirn++) {   // mirror: the session's collector set (MirrorArea)
            const std::vector<int>& ids = dirn == 0 ? r.mirror_ingress : r.mirror_egress;
            if (ids.empty()) continue;
            if (ids.size() > 1) c_.log_once("acl:mirror-multi", "ACL entry with several mirror sessions: the first one is used");
            bool ing = dirn == 0;
            ifcs_handle_t set = 0;
            if (hw < 0 || hw == aces_for.front()) {   // one hold per entry and direction
                st = c_.mirror->collector_set(ids[0], ing, set);
                if (st != SAI_STATUS_SUCCESS) return st;
                e.mirrors.push_back({ids[0], ing});
            } else {
                st = c_.mirror->collector_set(ids[0], ing, set);   // held already: same set
                if (st != SAI_STATUS_SUCCESS) return st;
                c_.mirror->collector_set_release(ids[0], ing);
            }
            st = action(IFCS_ACL_ACTION_TYPE_MIRROR, nullptr, 0, set);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (meter) {
            st = action(IFCS_ACL_ACTION_TYPE_METER, nullptr, 0, meter);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        if (e.counter) {
            st = action(IFCS_ACL_ACTION_TYPE_ACL_COUNTER, nullptr, 0, e.counter);
            if (st != SAI_STATUS_SUCCESS) return st;
        }
        std::vector<ifcs_attr_t> ea = {attr_u32(IFCS_ACE_ATTR_PRIORITY, prio), attr_data(IFCS_ACE_ATTR_ENABLE, e.enabled),
                                       attr_of(IFCS_ACE_ATTR_MATCH_SET), attr_of(IFCS_ACE_ATTR_ACTION_SET)};
        ea[2].value.handle_list.count = (uint32_t)ms.size();
        ea[2].value.handle_list.list = ms.data();
        ea[3].value.handle_list.count = (uint32_t)as.size();
        ea[3].value.handle_list.list = as.data();
        ifcs_handle_t ace = IFCS_NULL_HANDLE;
        st = c_.call(c_.L.ifcs_ace_create(NODE, &ace, (uint32_t)ea.size(), ea.data()), "ace_create (acl)");
        if (st != SAI_STATUS_SUCCESS) return st;
        st = c_.call(c_.L.ifcs_acl_ace_add(NODE, g.acl, 1, &ace, 0, nullptr), "acl_ace_add (acl)");
        if (st != SAI_STATUS_SUCCESS) {
            c_.call(c_.L.ifcs_ace_delete(NODE, ace), "ace_delete");
            return st;
        }
        e.aces.push_back(ace);
    }
    return SAI_STATUS_SUCCESS;
}

void AclArea::uentry_uninstall(UGroup& g, UEntry& e, bool keep_counter)
{
    if (!e.aces.empty()) c_.call(c_.L.ifcs_acl_ace_remove(NODE, g.acl, (uint32_t)e.aces.size(), e.aces.data()), "acl_ace_remove");
    for (auto h : e.aces) c_.call(c_.L.ifcs_ace_delete(NODE, h), "ace_delete");
    for (auto h : e.actions) c_.call(c_.L.ifcs_acl_action_delete(NODE, h), "acl_action_delete");
    for (auto h : e.matches) c_.call(c_.L.ifcs_acl_match_delete(NODE, h), "acl_match_delete");
    e.aces.clear();
    e.actions.clear();
    e.matches.clear();
    if (e.cmp_group) c_.call(c_.L.ifcs_acl_comparator_group_delete(NODE, e.cmp_group), "acl_comparator_group_delete");
    for (auto h : e.cmps) c_.call(c_.L.ifcs_acl_comparator_delete(NODE, h), "acl_comparator_delete");
    e.cmp_group = 0;
    e.cmps.clear();
    for (auto& m : e.mirrors) c_.mirror->collector_set_release(m.first, m.second);
    e.mirrors.clear();
    if (!keep_counter && e.counter) {
        c_.call(c_.L.ifcs_acl_counter_delete(NODE, e.counter), "acl_counter_delete");
        e.counter = 0;
    }
}

sai_status_t AclArea::entry_create(int gid, const FpRule& r, int& eid) { return entry_make(gid, r, -1, eid); }

sai_status_t AclArea::entry_make(int gid, const FpRule& r, int dmac_hit, int& eid)
{
    Guard g(c_.mu);
    eid = -1;
    auto git = ugroups_.find(gid);
    if (git == ugroups_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    const FpGroupExtra& gx = git->second.x;
    if (((!r.mirror_ingress.empty() || !r.mirror_egress.empty()) && !gx.mirror) || (r.redirect >= 0 && !gx.redirect) ||
        (r.set_tc >= 0 && !gx.set_tc) || (r.set_dscp >= 0 && !gx.set_dscp)) {
        OB_ERR(SAI_API_ACL, "ACL entry: an action its table was not created with (ACTION_TYPE_LIST)");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    if (r.cpu_copy_only || r.m.has(FpQual::DstCpu)) return SAI_STATUS_NOT_SUPPORTED;
    if (r.m.ports.empty()) return SAI_STATUS_INVALID_PARAMETER;
    for (int hw : r.m.ports)
        if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    if (r.policer >= 0 && !policers_.count(r.policer)) return SAI_STATUS_INVALID_PARAMETER;
    UGroup& u = git->second;
    int id = next_uentry_++;
    UEntry& e = uentries_[id];
    e.gid = gid;
    e.rule = r;
    e.dmac_hit = dmac_hit;
    std::set<int> before = u.bound;
    sai_status_t st = ugroup_bind(gid, u);
    if (st == SAI_STATUS_SUCCESS) st = uentry_install(u, e);
    if (st != SAI_STATUS_SUCCESS) {
        uentry_uninstall(u, e, false);
        uentries_.erase(id);
        ugroup_bind(gid, u);
        return st;
    }
    // The table now spans more ports: entries that covered all of them before
    // must not match on the new ones (re-installed with per-port ACEs).
    if (u.bound != before)
        for (auto& o : uentries_) {
            if (o.first == id || o.second.gid != gid || o.second.per_port) continue;
            uentry_uninstall(u, o.second, true);
            sai_status_t s2 = uentry_install(u, o.second);
            if (s2 != SAI_STATUS_SUCCESS) OB_ERR(SAI_API_ACL, "ACL entry %d: re-install for the new ports failed (%d)", o.first, s2);
        }
    eid = id;
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::entry_destroy(int eid)
{
    Guard g(c_.mu);
    auto it = uentries_.find(eid);
    if (it == uentries_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    auto git = ugroups_.find(it->second.gid);
    if (git == ugroups_.end()) { uentries_.erase(it); return SAI_STATUS_SUCCESS; }
    uentry_uninstall(git->second, it->second, false);
    int gid = it->second.gid;
    uentries_.erase(it);
    ugroup_bind(gid, git->second);   // fewer ports: the others' ACEs stay correct
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::entry_counter(int eid, uint64_t& packets, uint64_t& bytes)
{
    Guard g(c_.mu);
    packets = bytes = 0;
    auto it = uentries_.find(eid);
    if (it == uentries_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (!it->second.counter) return SAI_STATUS_SUCCESS;
    ifcs_acl_counter_stats_id_t ids[2] = {IFCS_ACL_COUNTER_STATS_ID_PACKETS, IFCS_ACL_COUNTER_STATS_ID_BYTES};
    uint64_t v[2] = {0, 0};
    uint32_t n = 0;
    sai_status_t st = c_.call(c_.L.ifcs_acl_counter_stats_get(NODE, it->second.counter, 2, ids, v, &n), "acl_counter_stats_get (acl)");
    if (st != SAI_STATUS_SUCCESS) return st;
    packets = v[0];
    bytes = v[1];
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::entry_update(int eid, const FpRule& r)
{
    Guard g(c_.mu);
    auto it = uentries_.find(eid);
    if (it == uentries_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    auto git = ugroups_.find(it->second.gid);
    if (git == ugroups_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    const FpGroupExtra& gx = git->second.x;
    if (((!r.mirror_ingress.empty() || !r.mirror_egress.empty()) && !gx.mirror) || (r.redirect >= 0 && !gx.redirect) ||
        (r.set_tc >= 0 && !gx.set_tc) || (r.set_dscp >= 0 && !gx.set_dscp))
        return SAI_STATUS_NOT_SUPPORTED;
    if (r.cpu_copy_only || r.m.has(FpQual::DstCpu) || r.m.ports.empty()) return SAI_STATUS_INVALID_PARAMETER;
    for (int hw : r.m.ports)
        if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    UGroup& u = git->second;
    UEntry& e = it->second;
    FpRule old = e.rule;
    std::set<int> before = u.bound;
    uentry_uninstall(u, e, true);   // the counter stays (and keeps its counts)
    if (r.counter && !old.counter) e.counter = 0;
    e.rule = r;
    sai_status_t st = ugroup_bind(e.gid, u);
    if (st == SAI_STATUS_SUCCESS) st = uentry_install(u, e);
    if (st != SAI_STATUS_SUCCESS) {   // put the old rule back
        uentry_uninstall(u, e, true);
        e.rule = old;
        ugroup_bind(e.gid, u);
        if (uentry_install(u, e) != SAI_STATUS_SUCCESS) OB_ERR(SAI_API_ACL, "ACL entry %d: restoring the old rule failed", eid);
        return st;
    }
    if (!r.counter && e.counter) {
        c_.call(c_.L.ifcs_acl_counter_delete(NODE, e.counter), "acl_counter_delete");
        e.counter = 0;
    }
    if (u.bound != before)
        for (auto& o : uentries_) {
            if (o.first == eid || o.second.gid != e.gid || o.second.per_port) continue;
            uentry_uninstall(u, o.second, true);
            if (uentry_install(u, o.second) != SAI_STATUS_SUCCESS)
                OB_ERR(SAI_API_ACL, "ACL entry %d: re-install for the new ports failed", o.first);
        }
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::entry_enable(int eid, bool on)
{
    Guard g(c_.mu);
    auto it = uentries_.find(eid);
    if (it == uentries_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (auto h : it->second.aces) {
        ifcs_attr_t a = attr_data(IFCS_ACE_ATTR_ENABLE, on);
        sai_status_t st = c_.call(c_.L.ifcs_ace_attr_set(NODE, h, 1, &a), "ace_attr_set ENABLE");
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    it->second.enabled = on;
    return SAI_STATUS_SUCCESS;
}

sai_status_t AclArea::group_free(int gid, uint32_t& entries)
{
    Guard g(c_.mu);
    auto git = ugroups_.find(gid);
    if (git == ugroups_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    size_t used = 0;
    for (auto& e : uentries_)
        if (e.second.gid == gid) used += e.second.aces.size();
    entries = used >= git->second.size ? 0 : git->second.size - (uint32_t)used;
    return SAI_STATUS_SUCCESS;
}

// Storm control as ACL entries (0.12.41 has no sysport storm attribute):
// table {in-port, DMAC, DMAC hit}, entry per (port, kind), the policer's
// meter with yellow and red dropped (green passes). Unknown unicast: a
// unicast DMAC that missed the L2 table (DESTINATION_MAC_HIT 0, I).
sai_status_t AclArea::storm_set(int hw, StormKind kind, int policer)
{
    Guard g(c_.mu);
    if (!c_.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    auto key = std::make_pair(hw, (int)kind);
    auto it = storm_.find(key);
    if (policer < 0) {
        if (it == storm_.end()) return SAI_STATUS_SUCCESS;
        sai_status_t st = entry_destroy(it->second);
        storm_.erase(it);
        return st;
    }
    if (!policers_.count(policer)) return SAI_STATUS_INVALID_PARAMETER;
    if (storm_gid_ < 0) {
        int gid = -1;
        sai_status_t st = group_make({FpQual::InPorts, FpQual::DstMac}, 1, FpGroupExtra(), true, gid);
        if (st != SAI_STATUS_SUCCESS) return st;
        storm_gid_ = gid;
    }
    FpRule r;
    r.priority = kind == StormKind::Broadcast ? 200 : 100;   // broadcast is multicast too: its own entry wins
    r.m.ports = {hw};
    r.m.set(FpQual::InPorts);
    r.m.set(FpQual::DstMac);
    int hit = -1;
    switch (kind) {
    case StormKind::Broadcast: std::memset(r.m.dmac, 0xff, 6); std::memset(r.m.dmac_mask, 0xff, 6); break;
    case StormKind::Multicast: r.m.dmac[0] = 0x01; r.m.dmac_mask[0] = 0x01; break;   // includes broadcast (IEEE group bit)
    case StormKind::UnknownUnicast: r.m.dmac[0] = 0x00; r.m.dmac_mask[0] = 0x01; hit = 0; break;
    }
    r.policer = policer;
    r.yellow_drop = r.red_drop = true;
    r.counter = true;
    if (it != storm_.end()) {
        UEntry& e = uentries_[it->second];
        if (e.dmac_hit == hit) return entry_update(it->second, r);
    }
    int eid = -1;
    sai_status_t st = entry_make(storm_gid_, r, hit, eid);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (it != storm_.end()) entry_destroy(it->second);
    storm_[key] = eid;
    OB_NOTICE(SAI_API_PORT, "storm control: devport %d kind %d policer %d (ACL entry %d)", hw, (int)kind, policer, eid);
    return SAI_STATUS_SUCCESS;
}

std::string AclArea::report()
{
    Guard g(c_.mu);
    std::string out;
    char b[256];
    std::snprintf(b, sizeof(b), "ACL: %zu tables, %zu entries (CoPP: %zu installs)\n", ugroups_.size(), uentries_.size(),
                  installs_.size());
    out += b;
    for (auto& gr : ugroups_) {
        std::string ports;
        for (int hw : gr.second.bound) ports += (ports.empty() ? "" : ",") + std::to_string(hw);
        std::snprintf(b, sizeof(b), "table %d: ifcs 0x%x acl 0x%x size %u%s ports [%s]\n", gr.first, gr.second.table,
                      gr.second.acl, gr.second.size, gr.second.meters ? " meters" : "", ports.c_str());
        out += b;
        for (auto& e : uentries_) {
            if (e.second.gid != gr.first) continue;
            uint64_t p = 0, by = 0;
            entry_counter(e.first, p, by);
            const FpRule& r = e.second.rule;
            std::snprintf(b, sizeof(b), "  entry %d: priority %u, %zu ACEs%s, %s%s%s%s packets %llu bytes %llu\n", e.first,
                          r.priority, e.second.aces.size(), e.second.per_port ? " (per port)" : "", r.drop ? "drop " : "",
                          r.drop_cancel ? "forward " : "", r.copy_cpu ? "copy " : "", r.policer >= 0 ? "policer " : "",
                          (unsigned long long)p, (unsigned long long)by);
            out += b;
            if (gr.first == storm_gid_ && e.second.counter) {
                ifcs_acl_counter_stats_id_t ids[3] = {IFCS_ACL_COUNTER_STATS_ID_GREEN_PACKETS,
                                                      IFCS_ACL_COUNTER_STATS_ID_YELLOW_PACKETS,
                                                      IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS};
                uint64_t v[3] = {0, 0, 0};
                uint32_t n = 0;
                c_.L.ifcs_acl_counter_stats_get(NODE, e.second.counter, 3, ids, v, &n);
                std::snprintf(b, sizeof(b), "    storm: green %llu yellow %llu red %llu\n", (unsigned long long)v[0],
                              (unsigned long long)v[1], (unsigned long long)v[2]);
                out += b;
            }
        }
    }
    return out;
}

void AclArea::forget()
{
    Guard g(c_.mu);
    installs_.clear();
    meters_.clear();
    acls_.clear();
    copp_up_ = false;
    mprof_ = aprof_ = table_ = 0;
    ugroups_.clear();
    uentries_.clear();
    umeters_.clear();
    acl_trap_ = 0;
    storm_gid_ = -1;
    storm_.clear();
}

} // namespace tlb
} // namespace ob

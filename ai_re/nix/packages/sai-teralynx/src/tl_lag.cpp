// SPDX-License-Identifier: Apache-2.0
// LAGs and the switch hash (tl_lag.h).
#include "tl_lag.h"

#include "tl_l2.h"

#include <cstdlib>
#include <sstream>

namespace ob {
namespace tlb {

void LagArea::reset()
{
    lags_.clear();
    next_tid_ = 1;
    hash_lists_.assign(5, IFCS_NULL_HANDLE);   // the old node's lists are gone with it
    publish();
}

sai_status_t LagArea::create(int& tid)
{
    Guard g(c_.mu);
    ifcs_handle_t h = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_lag_create(NODE, &h, 0, nullptr), "lag_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    tid = next_tid_++;
    lags_[tid].h = h;
    publish();
    OB_NOTICE(SAI_API_LAG, "LAG %d = IFCS lag 0x%x", tid, h);
    return SAI_STATUS_SUCCESS;
}

sai_status_t LagArea::destroy(int tid)
{
    Guard g(c_.mu);
    auto it = lags_.find(tid);
    if (it == lags_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    for (int hw : std::set<int>(it->second.in_ifcs)) member_remove(it->second, hw);
    sai_status_t st = c_.call(c_.L.ifcs_lag_delete(NODE, it->second.h), "lag_delete");
    if (st != SAI_STATUS_SUCCESS) return st;
    lags_.erase(it);
    publish();
    return SAI_STATUS_SUCCESS;
}

// The frontend passes the whole member list. In IFCS: every member that is
// not both egress- and ingress-disabled (isai's rule; teamd sets both while a
// member is not collecting/distributing). Leaving members go first.
sai_status_t LagArea::set_members(int tid, const std::vector<LagMember>& members)
{
    Guard g(c_.mu);
    auto it = lags_.find(tid);
    if (it == lags_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    Lag& l = it->second;
    std::set<int> want;
    for (auto& m : members) {
        if (!c_.front(m.hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
        for (auto& o : lags_)
            if (o.first != tid)
                for (auto& x : o.second.members)
                    if (x.hw == m.hw) return SAI_STATUS_ITEM_ALREADY_EXISTS;
        if (!(m.egress_disable && m.ingress_disable)) want.insert(m.hw);
    }
    sai_status_t res = SAI_STATUS_SUCCESS;
    for (int hw : std::set<int>(l.in_ifcs))
        if (!want.count(hw)) {
            sai_status_t st = member_remove(l, hw);
            if (st != SAI_STATUS_SUCCESS) res = st;
        }
    // Disabled members (in the list, not in IFCS) do not learn: their frames
    // would otherwise create entries on the member sysport, which then
    // blocks its return to the LAG.
    for (auto& m : members)
        if (!want.count(m.hw)) c_.sp_set(m.hw, attr_data(IFCS_SYSPORT_ATTR_LEARN_ENABLE, false), "lag member learn off");
    for (int hw : want)
        if (!l.in_ifcs.count(hw)) {
            sai_status_t st = member_add(l, hw);
            if (st != SAI_STATUS_SUCCESS && res == SAI_STATUS_SUCCESS) res = st;
        }
    l.members = members;
    publish();
    return res;
}

sai_status_t LagArea::member_add(Lag& l, int hw)
{
    auto sp = c_.sysport.find(hw);
    if (sp == c_.sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
    // libifcs refuses a member some L2 entry points at: drop what was learned on it.
    c_.l2->l2_flush(hw, -1, false, true);
    ifcs_handle_t s = sp->second;
    sai_status_t st = c_.call(c_.L.ifcs_lag_member_add(NODE, l.h, 1, &s, 0, nullptr), "lag_member_add");
    if (st != SAI_STATUS_SUCCESS) return st;
    l.in_ifcs.insert(hw);
    c_.sp_set(hw, attr_data(IFCS_SYSPORT_ATTR_LEARN_ENABLE, true), "lag member learn on");
    return SAI_STATUS_SUCCESS;
}

sai_status_t LagArea::member_remove(Lag& l, int hw)
{
    ifcs_handle_t s = c_.sysport.at(hw);
    sai_status_t st = c_.call(c_.L.ifcs_lag_member_remove(NODE, l.h, 1, &s), "lag_member_remove");
    if (st == SAI_STATUS_SUCCESS) l.in_ifcs.erase(hw);
    return st;
}

int LagArea::lag_of_port(int hw) const
{
    for (auto& l : lags_)
        for (auto& m : l.second.members)
            if (m.hw == hw) return l.first;
    return -1;
}

bool LagArea::dest_handle(int dest, ifcs_handle_t& h) const
{
    if (is_lag_dest(dest)) {
        auto it = lags_.find(lag_tid(dest));
        if (it == lags_.end()) return false;
        h = it->second.h;
        return true;
    }
    auto sp = c_.sysport.find(dest);
    if (sp == c_.sysport.end()) return false;
    h = sp->second;
    return true;
}

int LagArea::dest_of_handle(ifcs_handle_t h) const
{
    {
        std::lock_guard<std::mutex> g(map_mu_);
        auto it = dest_map_.find(h);
        if (it != dest_map_.end()) return it->second;
    }
    auto it = c_.dev_of_sysport.find(h);   // immutable after init
    return it == c_.dev_of_sysport.end() ? -1 : it->second;
}

void LagArea::publish()
{
    std::map<ifcs_handle_t, int> m;
    for (auto& l : lags_) {
        m[l.second.h] = lag_dest(l.first);
        for (int hw : l.second.in_ifcs) m[c_.sysport.at(hw)] = lag_dest(l.first);
    }
    std::lock_guard<std::mutex> g(map_mu_);
    dest_map_.swap(m);
}

// ------------------------------------------------------------------- hash
// Field names: SAI_NATIVE_HASH_FIELD_* in lower case without the prefix.
// The list is kept in the given order; per packet type only the fields that
// exist for it are used (isai's mapping: IPv4 / IPv6 address and protocol
// fields per family, L4 ports for TCP/UDP only, the rest everywhere).
enum Fld { F_SRC_IP, F_DST_IP, F_PROTO, F_L4_SRC, F_L4_DST, F_SRC_MAC, F_DST_MAC, F_ETHERTYPE, F_VLAN, F_IN_PORT,
           F_FLOW_LABEL };

bool LagArea::parse_fields(const std::string& s, std::vector<uint32_t>& out)
{
    static const std::pair<const char*, Fld> names[] = {
        {"src_ip", F_SRC_IP}, {"dst_ip", F_DST_IP}, {"ip_protocol", F_PROTO}, {"l4_src_port", F_L4_SRC},
        {"l4_dst_port", F_L4_DST}, {"src_mac", F_SRC_MAC}, {"dst_mac", F_DST_MAC}, {"ethertype", F_ETHERTYPE},
        {"vlan_id", F_VLAN}, {"in_port", F_IN_PORT}, {"ipv6_flow_label", F_FLOW_LABEL}};
    out.clear();
    std::stringstream ss(s);
    std::string t;
    while (std::getline(ss, t, ',')) {
        while (!t.empty() && t.front() == ' ') t.erase(t.begin());
        while (!t.empty() && t.back() == ' ') t.pop_back();
        if (t.empty()) continue;
        bool found = false;
        for (auto& n : names)
            if (t == n.first) { out.push_back(n.second); found = true; }
        if (!found) return false;
    }
    return !out.empty();
}

sai_status_t LagArea::hash_fields_set(const std::vector<uint32_t>& fields)
{
    Guard g(c_.mu);
    if (hash_lists_.size() != 5) hash_lists_.assign(5, IFCS_NULL_HANDLE);
    static const uint32_t node_attr[5] = {IFCS_NODE_ATTR_ENTROPY_SELECT_IPV4_TCP_OR_UDP,
                                          IFCS_NODE_ATTR_ENTROPY_SELECT_IPV4_NON_TCP_OR_UDP,
                                          IFCS_NODE_ATTR_ENTROPY_SELECT_IPV6_TCP_OR_UDP,
                                          IFCS_NODE_ATTR_ENTROPY_SELECT_IPV6_NON_TCP_OR_UDP,
                                          IFCS_NODE_ATTR_ENTROPY_SELECT_NON_IP};
    for (int t = 0; t < 5; t++) {
        bool v6 = t == 2 || t == 3, l4 = t == 0 || t == 2, ip = t < 4;
        std::vector<uint32_t> el;
        for (uint32_t f : fields) {
            switch (f) {
            case F_SRC_IP: if (ip) el.push_back(v6 ? IFCS_EXTENDED_FIELD_IPV6_SOURCE_IP : IFCS_EXTENDED_FIELD_IPV4_SOURCE_IP); break;
            case F_DST_IP: if (ip) el.push_back(v6 ? IFCS_EXTENDED_FIELD_IPV6_DESTINATION_IP : IFCS_EXTENDED_FIELD_IPV4_DESTINATION_IP); break;
            case F_PROTO: if (ip) el.push_back(v6 ? IFCS_EXTENDED_FIELD_IPV6_NEXTHEADER : IFCS_EXTENDED_FIELD_IPV4_PROTOCOL); break;
            case F_FLOW_LABEL: if (v6) el.push_back(IFCS_EXTENDED_FIELD_IPV6_FLOW_LABEL); break;
            case F_L4_SRC: if (l4) el.push_back(IFCS_EXTENDED_FIELD_TCP_OR_UDP_SOURCE_PORT); break;
            case F_L4_DST: if (l4) el.push_back(IFCS_EXTENDED_FIELD_TCP_OR_UDP_DESTINATION_PORT); break;
            case F_SRC_MAC: el.push_back(IFCS_EXTENDED_FIELD_MAC_SA); break;
            case F_DST_MAC: el.push_back(IFCS_EXTENDED_FIELD_MAC_DA); break;
            case F_ETHERTYPE: el.push_back(IFCS_EXTENDED_FIELD_ETHERTYPE); break;
            case F_VLAN: el.push_back(IFCS_EXTENDED_FIELD_CVID); break;
            case F_IN_PORT: el.push_back(IFCS_EXTENDED_FIELD_SOURCE_SYSPORT); break;
            default: return SAI_STATUS_INVALID_PARAMETER;
            }
        }
        if (el.empty()) continue;   // nothing for this packet type: the SDK's selection stays
        ifcs_attr_t a = attr_of(IFCS_EXTENDED_FIELD_LIST_ATTR_ELEMENTS);
        a.value.u32_list.count = (uint32_t)el.size();
        a.value.u32_list.arr = el.data();
        ifcs_handle_t h = IFCS_NULL_HANDLE;
        sai_status_t st = c_.call(c_.L.ifcs_extended_field_list_create(NODE, &h, 1, &a), "extended_field_list_create");
        if (st != SAI_STATUS_SUCCESS) return st;
        ifcs_attr_t n = attr_handle(node_attr[t], h);
        st = c_.call(c_.L.ifcs_node_attr_set(NODE, 1, &n), "node entropy_select");
        if (st != SAI_STATUS_SUCCESS) {
            c_.L.ifcs_extended_field_list_delete(NODE, h);
            return st;
        }
        if (hash_lists_[t] != IFCS_NULL_HANDLE)
            c_.call(c_.L.ifcs_extended_field_list_delete(NODE, hash_lists_[t]), "extended_field_list_delete");
        hash_lists_[t] = h;
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t LagArea::hash_seed_set(uint32_t seed)
{
    Guard g(c_.mu);
    ifcs_attr_t a = attr_u32(IFCS_NODE_ATTR_ENTROPY_SEED, seed);
    return c_.call(c_.L.ifcs_node_attr_set(NODE, 1, &a), "node entropy_seed");
}

sai_status_t LagArea::hash_set(bool lag, const std::vector<int32_t>& sai_fields, uint32_t seed)
{
    Guard g(c_.mu);
    std::vector<uint32_t> f;
    for (int32_t s : sai_fields) {
        switch (s) {
        case SAI_NATIVE_HASH_FIELD_SRC_IP: f.push_back(F_SRC_IP); break;
        case SAI_NATIVE_HASH_FIELD_DST_IP: f.push_back(F_DST_IP); break;
        case SAI_NATIVE_HASH_FIELD_IP_PROTOCOL: f.push_back(F_PROTO); break;
        case SAI_NATIVE_HASH_FIELD_L4_SRC_PORT: f.push_back(F_L4_SRC); break;
        case SAI_NATIVE_HASH_FIELD_L4_DST_PORT: f.push_back(F_L4_DST); break;
        case SAI_NATIVE_HASH_FIELD_SRC_MAC: f.push_back(F_SRC_MAC); break;
        case SAI_NATIVE_HASH_FIELD_DST_MAC: f.push_back(F_DST_MAC); break;
        case SAI_NATIVE_HASH_FIELD_ETHERTYPE: f.push_back(F_ETHERTYPE); break;
        case SAI_NATIVE_HASH_FIELD_VLAN_ID: f.push_back(F_VLAN); break;
        case SAI_NATIVE_HASH_FIELD_IN_PORT: f.push_back(F_IN_PORT); break;
        case SAI_NATIVE_HASH_FIELD_IPV6_FLOW_LABEL: f.push_back(F_FLOW_LABEL); break;
        default:
            OB_ERR(SAI_API_HASH, "hash field %d has no IFCS entropy element here", s);
            return SAI_STATUS_INVALID_ATTR_VALUE_0;   // not NOT_SUPPORTED: that means "no hash control" to the frontend
        }
    }
    int other = lag ? 0 : 1;
    if (!f.empty()) {
        if (!hash_sai_[other].empty() && hash_sai_[other] != sai_fields)
            OB_WARN(SAI_API_HASH, "%s hash fields differ from the %s ones: IFCS has one entropy for both, this set wins",
                    lag ? "LAG" : "ECMP", lag ? "ECMP" : "LAG");
        sai_status_t st = hash_fields_set(f);
        if (st != SAI_STATUS_SUCCESS) return st;
        hash_sai_[lag ? 1 : 0] = sai_fields;
    }
    if (seed || seed_set_) {
        sai_status_t st = hash_seed_set(seed);
        if (st != SAI_STATUS_SUCCESS) return st;
        seed_set_ = true;
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t LagArea::apply_profile()
{
    std::string f = c_.key("SAI_TERALYNX_HASH_FIELDS"), s = c_.key("SAI_TERALYNX_HASH_SEED");
    if (!f.empty()) {
        std::vector<uint32_t> v;
        if (!parse_fields(f, v)) {
            OB_ERR(SAI_API_SWITCH, "SAI_TERALYNX_HASH_FIELDS: cannot parse '%s'", f.c_str());
            return SAI_STATUS_INVALID_PARAMETER;
        }
        sai_status_t st = hash_fields_set(v);
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    if (!s.empty()) return hash_seed_set((uint32_t)std::strtoul(s.c_str(), nullptr, 0));
    return SAI_STATUS_SUCCESS;
}

} // namespace tlb
} // namespace ob

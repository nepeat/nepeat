// SPDX-License-Identifier: Apache-2.0
// L3 (tl_l3.h).
#include "tl_l3.h"

#include "tl_hostif.h"
#include "tl_l2.h"
#include "tl_lag.h"

#include <algorithm>

namespace ob {
namespace tlb {

static uint32_t be32(const uint8_t* b)
{
    return ((uint32_t)b[0] << 24) | ((uint32_t)b[1] << 16) | ((uint32_t)b[2] << 8) | b[3];
}

static bool is_host(const IpPrefix& p)
{
    for (int i = 0; i < (p.addr.v6 ? 16 : 4); i++)
        if (p.mask.addr[i] != 0xff) return false;
    return true;
}

static IpPrefix host_prefix(const IpAddr& ip)
{
    IpPrefix p;
    p.addr = ip;
    p.mask.v6 = ip.v6;
    std::memset(p.mask.addr, 0xff, ip.v6 ? 16 : 4);
    return p;
}

// VRF n = IFCS_HANDLE_L3VNI(n + 1), created on first use with no
// attributes (SDK/scripts/l3snake_cel.py creates its L3VNIs that way).
// An L3 interface sits on the frontend's VLAN: intf TYPE = SVI on the
// L2VNI (I: the vendor sample uses L3PORT on a sysport), FORWARDING_INSTANCE
// = the VRF's L3VNI, TRANSPORT_MAC, IPv4/IPv6 unicast on, MTU.
sai_status_t L3Area::intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id)
{
    Guard g(c_.mu);
    ifcs_handle_t l2vni = 0;
    if (!c_.l2->vlan_l2vni(vid, l2vni)) return SAI_STATUS_INVALID_PARAMETER;
    ifcs_handle_t vni = 0;
    sai_status_t st = l3vni(vrf, vni);
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_attr_t a[7] = {attr_u32(IFCS_INTF_ATTR_TYPE, IFCS_INTF_TYPE_SVI),
                        attr_handle(IFCS_INTF_ATTR_FORWARDING_INSTANCE, vni),
                        attr_handle(IFCS_INTF_ATTR_TRANSPORT_INSTANCE, l2vni),
                        attr_mac(IFCS_INTF_ATTR_TRANSPORT_MAC, mac),
                        attr_data(IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE, true),
                        attr_data(IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE, true),
                        attr_u32(IFCS_INTF_ATTR_MTU, mtu)};
    ifcs_handle_t h = IFCS_NULL_HANDLE;
    st = c_.call(c_.L.ifcs_intf_create(NODE, &h, 7, a), "intf_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    Intf i;
    i.handle = h;
    std::memcpy(i.mac, mac, 6);
    i.vid = vid;
    i.mtu = mtu;
    i.vrf = vrf;
    intf_id = next_intf_++;
    intfs_[intf_id] = i;
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf)
{
    Guard g(c_.mu);
    auto it = intfs_.find(intf_id);
    if (it == intfs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    Intf& i = it->second;
    if (vid != i.vid || vrf != i.vrf) return c_.not_impl("moving an L3 interface to another VLAN/VRF");
    if (std::memcmp(mac, i.mac, 6) != 0) {
        ifcs_attr_t a = attr_mac(IFCS_INTF_ATTR_TRANSPORT_MAC, mac);
        sai_status_t st = c_.call(c_.L.ifcs_intf_attr_set(NODE, i.handle, 1, &a), "intf transport_mac");
        if (st != SAI_STATUS_SUCCESS) return st;
        std::memcpy(i.mac, mac, 6);
    }
    if (mtu != i.mtu) {
        ifcs_attr_t a = attr_u32(IFCS_INTF_ATTR_MTU, mtu);
        sai_status_t st = c_.call(c_.L.ifcs_intf_attr_set(NODE, i.handle, 1, &a), "intf mtu");
        if (st != SAI_STATUS_SUCCESS) return st;
        i.mtu = mtu;
    }
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::intf_destroy(int intf_id)
{
    Guard g(c_.mu);
    auto it = intfs_.find(intf_id);
    if (it == intfs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = c_.call(c_.L.ifcs_intf_delete(NODE, it->second.handle), "intf_delete");
    if (st == SAI_STATUS_SUCCESS) intfs_.erase(it);
    return st;
}

// IFCS has no my-station table in the calls we use: an interface
// terminates its own TRANSPORT_MAC (I). Stations are bookkeeping only.
sai_status_t L3Area::station_add(const uint8_t mac[6], int& station_id)
{
    Guard g(c_.mu);
    station_id = next_station_++;
    c_.log_once("station", "my-station %s: no IFCS object (interfaces terminate their own MAC)", mac_str(mac).c_str());
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::station_delete(int) { return SAI_STATUS_SUCCESS; }

// Egress object = IFCS nexthop: INTF, DEST_MAC, LOCAL_DESTINATION = the
// port's sysport (l3snake_cel.py) or the LAG's handle (a port RIF on a LAG:
// LagArea::dest_handle, as isai points next hops at a LAG). Shared CPU / drop egress: no nexthop,
// they turn into route policies (see route_attrs).
sai_status_t L3Area::egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id)
{
    Guard g(c_.mu);
    auto in = intfs_.find(intf_id);
    if (in == intfs_.end()) return SAI_STATUS_INVALID_PARAMETER;
    ifcs_handle_t dest = IFCS_NULL_HANDLE;
    sai_status_t st0 = dest_of(hw, dest);
    if (st0 != SAI_STATUS_SUCCESS) return st0;
    ifcs_attr_t a[3] = {attr_handle(IFCS_NEXTHOP_ATTR_INTF, in->second.handle), attr_mac(IFCS_NEXTHOP_ATTR_DEST_MAC, dmac),
                        attr_handle(IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, dest)};
    ifcs_handle_t h = IFCS_NULL_HANDLE;
    sai_status_t st = c_.call(c_.L.ifcs_nexthop_create(NODE, &h, 3, a), "nexthop_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    Egress e;
    e.kind = Egress::Nh;
    e.handle = h;
    e.intf = intf_id;
    std::memcpy(e.dmac, dmac, 6);
    e.hw = hw;
    e.vid = vid;
    egress_id = next_egress_++;
    egress_[egress_id] = e;
    return SAI_STATUS_SUCCESS;
}

// In-place nexthop_attr_set of what changed (I), so routes keep their handle.
sai_status_t L3Area::egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid)
{
    Guard g(c_.mu);
    auto it = egress_.find(egress_id);
    if (it == egress_.end() || it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
    Egress& e = it->second;
    auto in = intfs_.find(intf_id);
    if (in == intfs_.end()) return SAI_STATUS_INVALID_PARAMETER;
    ifcs_handle_t dest = IFCS_NULL_HANDLE;
    sai_status_t st0 = dest_of(hw, dest);
    if (st0 != SAI_STATUS_SUCCESS) return st0;
    if (intf_id != e.intf) {
        ifcs_attr_t a = attr_handle(IFCS_NEXTHOP_ATTR_INTF, in->second.handle);
        sai_status_t st = c_.call(c_.L.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop intf");
        if (st != SAI_STATUS_SUCCESS) return st;
        e.intf = intf_id;
    }
    if (std::memcmp(dmac, e.dmac, 6) != 0) {
        ifcs_attr_t a = attr_mac(IFCS_NEXTHOP_ATTR_DEST_MAC, dmac);
        sai_status_t st = c_.call(c_.L.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop dest_mac");
        if (st != SAI_STATUS_SUCCESS) return st;
        std::memcpy(e.dmac, dmac, 6);
    }
    if (hw != e.hw) {
        ifcs_attr_t a = attr_handle(IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, dest);
        sai_status_t st = c_.call(c_.L.ifcs_nexthop_attr_set(NODE, e.handle, 1, &a), "nexthop local_destination");
        if (st != SAI_STATUS_SUCCESS) return st;
        e.hw = hw;
    }
    e.vid = vid;
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::egress_destroy(int egress_id)
{
    Guard g(c_.mu);
    auto it = egress_.find(egress_id);
    if (it == egress_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
    sai_status_t st = c_.call(c_.L.ifcs_nexthop_delete(NODE, it->second.handle), "nexthop_delete");
    if (st == SAI_STATUS_SUCCESS) egress_.erase(it);
    return st;
}

sai_status_t L3Area::egress_cpu(int& egress_id)
{
    Guard g(c_.mu);
    if (cpu_egress_ < 0) {
        cpu_egress_ = next_egress_++;
        Egress e;
        e.kind = Egress::Cpu;
        egress_[cpu_egress_] = e;
    }
    egress_id = cpu_egress_;
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::egress_glean(int& egress_id)
{
    Guard g(c_.mu);
    if (glean_egress_ < 0) {
        glean_egress_ = next_egress_++;
        Egress e;
        e.kind = Egress::Glean;
        egress_[glean_egress_] = e;
    }
    egress_id = glean_egress_;
    return SAI_STATUS_SUCCESS;
}

bool L3Area::nexthop_handle(int egress_id, ifcs_handle_t& h) const
{
    auto it = egress_.find(egress_id);
    if (it == egress_.end() || it->second.kind != Egress::Nh || !it->second.handle) return false;
    h = it->second.handle;
    return true;
}

sai_status_t L3Area::intf_admin_set(int intf_id, bool v4, bool v6)
{
    Guard g(c_.mu);
    auto it = intfs_.find(intf_id);
    if (it == intfs_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    ifcs_attr_t a[2] = {attr_data(IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE, v4), attr_data(IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE, v6)};
    return c_.call(c_.L.ifcs_intf_attr_set(NODE, it->second.handle, 2, a), "intf ipv4/ipv6 unicast enable");
}

bool L3Area::intf_stat_supported(sai_stat_id_t id)
{
    return id == SAI_ROUTER_INTERFACE_STAT_IN_PACKETS || id == SAI_ROUTER_INTERFACE_STAT_IN_OCTETS ||
           id == SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS || id == SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS;
}

// isai sets the L3 interface counter modes before node enable (oracle acl /
// l3-basic: INGRESS mode + flex config, EGRESS mode ALL). 0.12.41 has the
// mode attributes; ALL for both, no flex config (I). A refusal is logged and
// leaves the counters as the SDK has them.
void L3Area::pre_enable()
{
    if (c_.key("SAI_TERALYNX_L3INTF_STATS", "on") == "off") return;
    // isai: per direction a flex counter block (CONFIG: counter mode 0, offset
    // 0, 4096 counters), then the MODE. 0.12.41 refuses mode ALL ("count
    // mode 1 not supported") and takes IPV4_IPV6_ANY (4) for both directions
    // (chip 2026-11-16); the modes are tried in SAI_TERALYNX_L3INTF_STATS_MODES
    // order (default 4,1,2,3), the first success kept.
    uint32_t n = (uint32_t)std::strtoul(c_.key("SAI_TERALYNX_L3INTF_STATS_COUNTERS", "4096").c_str(), nullptr, 0);
    std::vector<uint32_t> modes;
    {
        std::string s = c_.key("SAI_TERALYNX_L3INTF_STATS_MODES", "4,1,2,3");
        size_t p = 0;
        while (p < s.size()) {
            size_t q = s.find(',', p);
            if (q == std::string::npos) q = s.size();
            if (q > p) modes.push_back((uint32_t)std::strtoul(s.substr(p, q - p).c_str(), nullptr, 0));
            p = q + 1;
        }
    }
    const uint32_t cfg_id[2] = {IFCS_NODE_ATTR_INGRESS_L3INTF_STATS_CONFIG, IFCS_NODE_ATTR_EGRESS_L3INTF_STATS_CONFIG};
    const uint32_t mode_id[2] = {IFCS_NODE_ATTR_INGRESS_L3INTF_STATS_MODE, IFCS_NODE_ATTR_EGRESS_L3INTF_STATS_MODE};
    for (int d = 0; d < 2; d++) {
        if (n) {
            ifcs_attr_t c = attr_of(cfg_id[d]);
            ifcs_flex_stats_attributes_t f;
            std::memset(&f, 0, sizeof(f));
            f.number_of_counters = n;
            f.dirty = 0x07;
            std::memcpy(&c.value, &f, sizeof(f));
            int r = IFCS_STATUS_REASON(c_.L.ifcs_node_attr_set(NODE, 1, &c));
            if (r != IFCS_SUCCESS)
                OB_WARN(SAI_API_ROUTER_INTERFACE, "node L3INTF stats config (attr %u, %u counters): %s", cfg_id[d], n, c_.reason(r));
        }
        bool ok = false;
        for (uint32_t m : modes) {
            ifcs_attr_t x = attr_u32(mode_id[d], m);
            int r = IFCS_STATUS_REASON(c_.L.ifcs_node_attr_set(NODE, 1, &x));
            if (r == IFCS_SUCCESS) {
                OB_NOTICE(SAI_API_ROUTER_INTERFACE, "node %s L3INTF stats mode %u", d ? "egress" : "ingress", m);
                if (!d) in_mode_ = m;
                ok = true;
                break;
            }
        }
        if (!ok) OB_WARN(SAI_API_ROUTER_INTERFACE, "node %s L3INTF stats: no mode accepted (RIF counters stay 0)", d ? "egress" : "ingress");
    }
}

sai_status_t L3Area::egress_drop(int& egress_id)
{
    Guard g(c_.mu);
    if (drop_egress_ < 0) {
        drop_egress_ = next_egress_++;
        Egress e;
        e.kind = Egress::Drop;
        egress_[drop_egress_] = e;
    }
    egress_id = drop_egress_;
    return SAI_STATUS_SUCCESS;
}

// Host entries are /32 (/128) route entries: IFCS picks the host table
// itself (route_entry has a HOST_RESOURCE_TYPE attribute; I).
sai_status_t L3Area::host_add(int vrf, const IpAddr& ip, int egress_id)
{
    Guard g(c_.mu);
    HostKey k{vrf, ip};
    if (hosts_.count(k)) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    IpPrefix p = host_prefix(ip);
    sai_status_t st = route_program(vrf, p, egress_id, false, nullptr);
    if (st == SAI_STATUS_SUCCESS) hosts_[k] = egress_id;
    return st;
}

sai_status_t L3Area::host_delete(int vrf, const IpAddr& ip)
{
    Guard g(c_.mu);
    auto it = hosts_.find(HostKey{vrf, ip});
    if (it == hosts_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = route_remove(vrf, host_prefix(ip));
    if (st == SAI_STATUS_SUCCESS) hosts_.erase(it);
    return st;
}

sai_status_t L3Area::route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace)
{
    Guard g(c_.mu);
    RouteKey k{vrf, p};
    auto it = routes_.find(k);
    if (it != routes_.end() && !replace) return SAI_STATUS_ITEM_ALREADY_EXISTS;
    if (it == routes_.end() && replace) return SAI_STATUS_ITEM_NOT_FOUND;
    Route r{target, ecmp};
    sai_status_t st = route_program(vrf, p, target, ecmp, it == routes_.end() ? nullptr : &it->second);
    if (st == SAI_STATUS_SUCCESS) routes_[k] = r;
    return st;
}

sai_status_t L3Area::route_delete(int vrf, const IpPrefix& p)
{
    Guard g(c_.mu);
    auto it = routes_.find(RouteKey{vrf, p});
    if (it == routes_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    sai_status_t st = route_remove(vrf, p);
    if (st == SAI_STATUS_SUCCESS) routes_.erase(it);
    return st;
}

// ECMP group: ifcs_ecmp_create without attributes (cmds/ecmp.py with no
// options), then the members in one ifcs_ecmp_member_add; a member's weight
// is its nexthop repeated (the frontend repeats the egress object), changes
// are one ifcs_ecmp_member_replace of the whole list. All as isai does
// (oracle l3-ecmpw); IFCS_ECMP_MEMBER_ATTR_WEIGHT is accepted but has no
// effect (chip 2026-11-16: weight 3:1 forwarded 1:1).
sai_status_t L3Area::ecmp_create(const std::vector<int>& egress, int& ecmp_id)
{
    Guard g(c_.mu);
    std::vector<ifcs_handle_t> want;
    sai_status_t st = ecmp_members(egress, want);
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_handle_t h = IFCS_NULL_HANDLE;
    ifcs_attr_t none = attr_of(0);
    st = c_.call(c_.L.ifcs_ecmp_create(NODE, &h, 0, &none), "ecmp_create");
    if (st != SAI_STATUS_SUCCESS) return st;
    st = c_.call(c_.L.ifcs_ecmp_member_add(NODE, h, (uint32_t)want.size(), want.data(), 0, nullptr), "ecmp_member_add");
    if (st != SAI_STATUS_SUCCESS) {
        c_.L.ifcs_ecmp_delete(NODE, h);
        return st;
    }
    Ecmp e;
    e.handle = h;
    e.members = want;
    ecmp_id = next_ecmp_++;
    ecmps_[ecmp_id] = e;
    return SAI_STATUS_SUCCESS;
}

sai_status_t L3Area::ecmp_update(int ecmp_id, const std::vector<int>& egress)
{
    Guard g(c_.mu);
    auto it = ecmps_.find(ecmp_id);
    if (it == ecmps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    std::vector<ifcs_handle_t> want;
    sai_status_t st = ecmp_members(egress, want);
    if (st != SAI_STATUS_SUCCESS) return st;
    if (want == it->second.members) return SAI_STATUS_SUCCESS;
    st = c_.call(c_.L.ifcs_ecmp_member_replace(NODE, it->second.handle, (uint32_t)want.size(), want.data(), 0, nullptr),
                 "ecmp_member_replace");
    if (st == SAI_STATUS_SUCCESS) it->second.members = want;
    return st;
}

sai_status_t L3Area::ecmp_destroy(int ecmp_id)
{
    Guard g(c_.mu);
    auto it = ecmps_.find(ecmp_id);
    if (it == ecmps_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    Ecmp& e = it->second;
    if (!e.members.empty()) {
        sai_status_t st = c_.call(c_.L.ifcs_ecmp_member_remove(NODE, e.handle, (uint32_t)e.members.size(), e.members.data()),
                                  "ecmp_member_remove");
        if (st != SAI_STATUS_SUCCESS) return st;
        e.members.clear();
    }
    sai_status_t st = c_.call(c_.L.ifcs_ecmp_delete(NODE, e.handle), "ecmp_delete");
    if (st == SAI_STATUS_SUCCESS) ecmps_.erase(it);
    return st;
}

sai_status_t L3Area::dest_of(int hw, ifcs_handle_t& h)
{
    if (is_lag_dest(hw)) return c_.lag->dest_handle(hw, h) ? SAI_STATUS_SUCCESS : SAI_STATUS_INVALID_PARAMETER;
    auto sp = c_.sysport.find(hw);
    if (sp == c_.sysport.end()) return SAI_STATUS_INVALID_PORT_NUMBER;
    h = sp->second;
    return SAI_STATUS_SUCCESS;
}

// RIF counters: ingress = L3INTF_INGRESS_*; egress = the IPv4 + IPv6
// egress counters (IFCS has no combined one). One stats_get per call.
sai_status_t L3Area::intf_stats(int intf_id, uint32_t n, const sai_stat_id_t* ids, uint64_t* out, bool clear)
{
    Guard g(c_.mu);
    auto it = intfs_.find(intf_id);
    if (it == intfs_.end()) return SAI_STATUS_INVALID_OBJECT_ID;
    std::vector<std::vector<ifcs_intf_stats_id_t>> want(n);
    std::vector<ifcs_intf_stats_id_t> q;
    for (uint32_t i = 0; i < n; i++) {
        switch (ids[i]) {
        // ingress: the counters of the node's ingress mode (pre_enable): ALL /
        // UC_MC_BC total, UC_ONLY unicast, IPV4_IPV6_ANY v4 + v6 (0.12.41 takes
        // the latter, chip 2026-11-16)
        case SAI_ROUTER_INTERFACE_STAT_IN_PACKETS:
            want[i] = in_mode_ == 4 ? std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV4_PKTS,
                                                                       IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV6_PKTS}
                    : in_mode_ == 3 ? std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_UCAST_PKTS}
                                    : std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_PKTS};
            break;
        case SAI_ROUTER_INTERFACE_STAT_IN_OCTETS:
            want[i] = in_mode_ == 4 ? std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV4_BYTES,
                                                                       IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV6_BYTES}
                    : in_mode_ == 3 ? std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_UCAST_BYTES}
                                    : std::vector<ifcs_intf_stats_id_t>{IFCS_INTF_STATS_ID_L3INTF_INGRESS_BYTES};
            break;
        case SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS:
            want[i] = {IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV4_PKTS, IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV6_PKTS};
            break;
        case SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS:
            want[i] = {IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV4_BYTES, IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV6_BYTES};
            break;
        default: return SAI_STATUS_NOT_SUPPORTED;
        }
        for (auto s : want[i])
            if (std::find(q.begin(), q.end(), s) == q.end()) q.push_back(s);
    }
    if (q.empty()) return SAI_STATUS_SUCCESS;
    std::vector<uint64_t> v(q.size(), 0);
    uint32_t got = 0;
    sai_status_t st = c_.call(c_.L.ifcs_intf_stats_get(NODE, it->second.handle, (uint32_t)q.size(), q.data(), v.data(), &got),
                              "intf_stats_get");
    if (st != SAI_STATUS_SUCCESS) return st;
    for (uint32_t i = 0; i < n; i++) {
        out[i] = 0;
        for (auto s : want[i]) out[i] += v[(size_t)(std::find(q.begin(), q.end(), s) - q.begin())];
    }
    if (clear) st = c_.call(c_.L.ifcs_intf_stats_clear(NODE, it->second.handle, (uint32_t)q.size(), q.data()), "intf_stats_clear");
    return st;
}

L3Area::Counts L3Area::counts() const
{
    Counts n;
    // A host-length route lives in the host hash like a neighbor's host entry
    // (chip 2026-11-16: 95,043 /32 routes fitted with the 24,576-entry TCAM full).
    for (auto& r : routes_) {
        bool v6 = r.first.second.addr.v6, host = is_host(r.first.second);
        (host ? (v6 ? n.v6_hosts : n.v4_hosts) : (v6 ? n.v6_prefixes : n.v4_prefixes))++;
    }
    for (auto& h : hosts_) (h.first.second.v6 ? n.v6_hosts : n.v4_hosts)++;
    for (auto& e : ecmps_) n.ecmp_members += (uint32_t)e.second.members.size();
    return n;
}

// Route key as scripts/l3snake_cel.py builds it. IPv4 goes in as a
// host-order value, IPv6 as 16 bytes.
void L3Area::route_key(ifcs_route_entry_key_t& k, ifcs_handle_t vni, const IpPrefix& p)
{
    ifcs_ip_addr_t a, m;
    std::memset(&a, 0, sizeof(a));
    std::memset(&m, 0, sizeof(m));
    if (p.addr.v6) {
        std::memcpy(a.ipv6, p.addr.addr, 16);
        std::memcpy(m.ipv6, p.mask.addr, 16);
    } else {
        a.ipv4 = be32(p.addr.addr);
        m.ipv4 = be32(p.mask.addr);
    }
    ifcs_ip_prefix_t pf;
    std::memset(&pf, 0, sizeof(pf));
    c_.L.ifcs_ip_prefix_t_init(&pf);
    c_.L.ifcs_ip_prefix_t_addr_family_set(&pf, p.addr.v6 ? IFCS_IP_ADDR_FAMILY_IPV6 : IFCS_IP_ADDR_FAMILY_IPV4);
    c_.L.ifcs_ip_prefix_t_addr_set(&pf, &a);
    c_.L.ifcs_ip_prefix_t_mask_set(&pf, &m);
    std::memset(&k, 0, sizeof(k));
    c_.L.ifcs_route_entry_key_t_init(&k);
    c_.L.ifcs_route_entry_key_t_key_type_set(&k, IFCS_ROUTE_ENTRY_KEY_TYPE_IP_DEST_L3VNI);
    c_.L.ifcs_route_entry_key_t_ip_dest_l3vni_set(&k, k.key.ip_dest_l3vni);
    c_.L.ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&k.key.ip_dest_l3vni, vni);
    c_.L.ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&k.key.ip_dest_l3vni, &pf);
}

sai_status_t L3Area::l3vni(int vrf, ifcs_handle_t& h)
{
    if (vrf < 0 || (uint32_t)vrf > VRF_MAX) return SAI_STATUS_INVALID_PARAMETER;
    auto it = l3vnis_.find(vrf);
    if (it != l3vnis_.end()) {
        h = it->second;
        return SAI_STATUS_SUCCESS;
    }
    ifcs_handle_t v = IFCS_HANDLE_L3VNI((uint32_t)vrf + 1);
    // Routed packets whose TTL check fails are copied to the CPU (the system
    // trap FWD_L3_TTL_CHECK_FAIL then decides queue / whether: TTL_ERROR), as
    // isai creates its L3VNIs (oracle l3-basic; without it TTL 1 is dropped
    // silently, chip 2026-11-16).
    // IPv4 with options: copied to the CPU and dropped (isai's L3VNI
    // attributes; the system trap FWD_IPV4_OPTIONS_DETECTED, enabled at init,
    // takes them to its queue).
    ifcs_attr_t a[4] = {attr_data(IFCS_L3VNI_ATTR_IPV4_UNICAST_TTL_CHECK_FAIL_CTC, true),
                        attr_data(IFCS_L3VNI_ATTR_IPV6_UNICAST_TTL_CHECK_FAIL_CTC, true),
                        attr_data(IFCS_L3VNI_ATTR_IPV4_OPTIONS_CTC, true),
                        attr_data(IFCS_L3VNI_ATTR_IPV4_OPTIONS_DROP, true)};
    int r = IFCS_STATUS_REASON(c_.L.ifcs_l3vni_create(NODE, &v, 4, a));
    if (r != IFCS_SUCCESS && r != IFCS_EXIST) return c_.fail(r, "l3vni_create");
    if (!ttl_profile_set_) {
        // TTL check profile 1 passes everything but TTL 0: IP2ME routes use it,
        // so TTL 1 packets to the switch itself (single-hop eBGP, traceroute's
        // last hop) reach the CPU (isai: same threshold, same route attribute).
        ifcs_attr_t t = attr_u32(IFCS_NODE_ATTR_TTL_CHECK_PROFILE_1_THRESHOLD, 0);
        if (c_.call(c_.L.ifcs_node_attr_set(NODE, 1, &t), "node TTL_CHECK_PROFILE_1_THRESHOLD") == SAI_STATUS_SUCCESS)
            ttl_profile_set_ = true;
    }
    l3vnis_[vrf] = v;
    h = v;
    return SAI_STATUS_SUCCESS;
}

// Route attributes per target kind:
//   nexthop / ECMP: NEXTHOP + FWD_POLICY forward (l3snake_cel.py; an ECMP
//     handle as NEXTHOP is I);
//   drop: FWD_POLICY drop, no NEXTHOP (I);
//   CPU (TRAP / ip2me / glean): FWD_POLICY drop + CTC_POLICY copy-to-cpu with
//     the hostif area's IP2ME trap (HostifArea::ip2me_trap; drop + copy as
//     isai does, oracle l3-basic); a host prefix (IP2ME) also takes TTL
//     check profile 1 (see l3vni); glean (Egress::Glean: connected subnets,
//     unresolved next hops) copies to its own hostif trap, as isai.
sai_status_t L3Area::route_attrs(int target, bool ecmp, bool host, std::vector<ifcs_attr_t>& a, int& kind)
{
    a.clear();
    if (ecmp) {
        auto it = ecmps_.find(target);
        if (it == ecmps_.end()) return SAI_STATUS_INVALID_PARAMETER;
        a.push_back(attr_handle(IFCS_ROUTE_ENTRY_ATTR_NEXTHOP, it->second.handle));
        a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_FORWARD));
        kind = Egress::Nh;
        return SAI_STATUS_SUCCESS;
    }
    auto it = egress_.find(target);
    if (it == egress_.end()) return SAI_STATUS_INVALID_PARAMETER;
    kind = it->second.kind;
    if (kind == Egress::Nh) {
        a.push_back(attr_handle(IFCS_ROUTE_ENTRY_ATTR_NEXTHOP, it->second.handle));
        a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_FORWARD));
    } else if (kind == Egress::Drop) {
        a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_DROP));
    } else {
        ifcs_handle_t trap = IFCS_NULL_HANDLE;
        sai_status_t st = kind == Egress::Glean ? c_.hostif->glean_trap(trap) : c_.hostif->ip2me_trap(trap);
        if (st != SAI_STATUS_SUCCESS) return st;
        a.push_back(attr_fwd(IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY, IFCS_FWD_ACTION_DROP));
        a.push_back(attr_ctc(IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY, true, trap));
        if (host && ttl_profile_set_ && kind == Egress::Cpu)
            a.push_back(attr_u32(IFCS_ROUTE_ENTRY_ATTR_TTL_CHECK_PROFILE, 1));   // IP2ME
    }
    return SAI_STATUS_SUCCESS;
}

// Create, or (old != nullptr) replace: same kind (nexthop/ECMP to
// nexthop/ECMP) is an attr_set of NEXTHOP + FWD_POLICY; a change of kind
// deletes and re-creates the entry (no attribute "unset" in IFCS we know).
sai_status_t L3Area::route_program(int vrf, const IpPrefix& p, int target, bool ecmp, const Route* old)
{
    ifcs_handle_t vni = 0;
    sai_status_t st = l3vni(vrf, vni);
    if (st != SAI_STATUS_SUCCESS) return st;
    std::vector<ifcs_attr_t> a;
    int kind = 0;
    st = route_attrs(target, ecmp, is_host(p), a, kind);
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_route_entry_key_t k;
    route_key(k, vni, p);
    if (old) {
        int okind = old->ecmp ? (int)Egress::Nh : egress_.count(old->target) ? (int)egress_[old->target].kind : -1;
        if (okind == kind && kind == Egress::Nh)
            return c_.call(c_.L.ifcs_route_entry_attr_set(NODE, &k, (uint32_t)a.size(), a.data()), "route_entry_attr_set");
        st = c_.call(c_.L.ifcs_route_entry_delete(NODE, &k), "route_entry_delete (replace)");
        if (st != SAI_STATUS_SUCCESS) return st;
    }
    return c_.call(c_.L.ifcs_route_entry_create(NODE, &k, (uint32_t)a.size(), a.data()), "route_entry_create");
}

sai_status_t L3Area::route_remove(int vrf, const IpPrefix& p)
{
    ifcs_handle_t vni = 0;
    sai_status_t st = l3vni(vrf, vni);
    if (st != SAI_STATUS_SUCCESS) return st;
    ifcs_route_entry_key_t k;
    route_key(k, vni, p);
    return c_.call(c_.L.ifcs_route_entry_delete(NODE, &k), "route_entry_delete");
}

sai_status_t L3Area::ecmp_members(const std::vector<int>& egress, std::vector<ifcs_handle_t>& want)
{
    if (egress.empty() || egress.size() > ECMP_PATHS) return SAI_STATUS_INVALID_PARAMETER;
    want.clear();
    for (int e : egress) {
        auto it = egress_.find(e);
        if (it == egress_.end() || it->second.kind != Egress::Nh) return SAI_STATUS_INVALID_PARAMETER;
        want.push_back(it->second.handle);
    }
    return SAI_STATUS_SUCCESS;
}

} // namespace tlb
} // namespace ob

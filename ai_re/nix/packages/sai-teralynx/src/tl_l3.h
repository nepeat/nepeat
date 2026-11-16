// SPDX-License-Identifier: Apache-2.0
// L3: VRFs (L3VNIs), interfaces, my-station, next hops, host and route
// entries, ECMP groups.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class L3Area {
public:
    explicit L3Area(Core& c) : c_(c) {}

    sai_status_t intf_create(const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf, int& intf_id);
    sai_status_t intf_update(int intf_id, const uint8_t mac[6], uint16_t vid, uint32_t mtu, int vrf);
    sai_status_t intf_destroy(int intf_id);
    sai_status_t station_add(const uint8_t mac[6], int& station_id);
    sai_status_t station_delete(int station_id);
    sai_status_t egress_create(int intf_id, const uint8_t dmac[6], int hw, uint16_t vid, int& egress_id);
    sai_status_t egress_update(int egress_id, int intf_id, const uint8_t dmac[6], int hw, uint16_t vid);
    sai_status_t egress_destroy(int egress_id);
    sai_status_t egress_cpu(int& egress_id);
    sai_status_t egress_drop(int& egress_id);
    sai_status_t host_add(int vrf, const IpAddr& ip, int egress_id);
    sai_status_t host_delete(int vrf, const IpAddr& ip);
    sai_status_t route_add(int vrf, const IpPrefix& p, int target, bool ecmp, bool replace);
    sai_status_t route_delete(int vrf, const IpPrefix& p);
    sai_status_t ecmp_create(const std::vector<int>& egress, int& ecmp_id);
    sai_status_t ecmp_update(int ecmp_id, const std::vector<int>& egress);
    sai_status_t ecmp_destroy(int ecmp_id);

    // Our own counts for the switch's table usage (no trusted IFCS call).
    struct Counts { uint32_t v4_prefixes = 0, v6_prefixes = 0, v4_hosts = 0, v6_hosts = 0, ecmp_members = 0; };
    Counts counts() const;

private:
    struct Intf { ifcs_handle_t handle = 0; uint8_t mac[6] = {0}; uint16_t vid = 0; uint32_t mtu = 0; int vrf = 0; };
    struct Egress {
        enum Kind { Nh, Cpu, Drop } kind = Nh;
        ifcs_handle_t handle = 0;
        int intf = -1, hw = -1;
        uint8_t dmac[6] = {0};
        uint16_t vid = 0;
    };
    struct Ecmp { ifcs_handle_t handle = 0; std::map<ifcs_handle_t, uint32_t> members; };
    struct Route { int target = -1; bool ecmp = false; };
    using HostKey = std::pair<int, IpAddr>;
    using RouteKey = std::pair<int, IpPrefix>;

    void route_key(ifcs_route_entry_key_t& k, ifcs_handle_t vni, const IpPrefix& p);
    sai_status_t l3vni(int vrf, ifcs_handle_t& h);
    sai_status_t route_attrs(int target, bool ecmp, std::vector<ifcs_attr_t>& a, int& kind);
    sai_status_t route_program(int vrf, const IpPrefix& p, int target, bool ecmp, const Route* old);
    sai_status_t route_remove(int vrf, const IpPrefix& p);
    sai_status_t ecmp_members(const std::vector<int>& egress, std::map<ifcs_handle_t, uint32_t>& want);
    sai_status_t ecmp_apply(Ecmp& e, const std::map<ifcs_handle_t, uint32_t>& want);

    Core& c_;
    std::map<int, ifcs_handle_t> l3vnis_;
    std::map<int, Intf> intfs_;
    std::map<int, Egress> egress_;
    std::map<int, Ecmp> ecmps_;
    std::map<HostKey, int> hosts_;
    std::map<RouteKey, Route> routes_;
    int next_intf_ = 1, next_station_ = 1, next_egress_ = 100001, next_ecmp_ = 200001;
    int cpu_egress_ = -1, drop_egress_ = -1;
};

} // namespace tlb
} // namespace ob

// SPDX-License-Identifier: Apache-2.0
// L2: VLANs (L2VNI + STP + flood group of per-port mc_dests), FDB entries,
// flush, aging, L2 events.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class L2Area {
public:
    explicit L2Area(Core& c) : c_(c) {}

    // At init (after the sysports are known): one STP instance with every
    // port FORWARDING, one mc_dest per port, VLAN 1 with the CPU.
    sai_status_t base_init(const std::vector<uint32_t>& devports);

    sai_status_t vlan_create(uint16_t vid);
    sai_status_t vlan_destroy(uint16_t vid);
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged);
    sai_status_t vlan_port_remove(uint16_t vid, int hw);
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out);
    bool vlan_l2vni(uint16_t vid, ifcs_handle_t& l2vni) const;   // false: no such VLAN

    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int hw, bool is_static, bool discard);
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid);
    sai_status_t l2_flush(int hw, int vid, bool statics, bool dynamics);
    sai_status_t l2_age_set(uint32_t seconds);

    // L2 event callback (SDK thread), user = Core*.
    static void on_l2(ifcs_node_id_t, void* user, ifcs_l2_event_info_t* info);

private:
    struct Vlan { ifcs_handle_t l2vni = 0, mdg = 0; std::map<int, bool> members; };
    sai_status_t vlan_make(uint16_t vid);
    void untagged_off(int hw);
    void l2_key(ifcs_l2_entry_key_t& k, const uint8_t mac[6], uint16_t vid);
    static void on_l2_entry(ifcs_node_id_t, ifcs_l2_entry_key_t* k, uint32_t, ifcs_attr_t*, void* user);

    Core& c_;
    ifcs_handle_t stp_ = 0;
    std::map<int, ifcs_handle_t> mcdest_;    // devport -> mc_dest (immutable after init)
    std::map<uint16_t, Vlan> vlans_;
    std::map<int, uint16_t> untagged_vid_;
};

} // namespace tlb
} // namespace ob

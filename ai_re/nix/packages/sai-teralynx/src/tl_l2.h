// SPDX-License-Identifier: Apache-2.0
// L2: VLANs (L2VNI + STP + flood group of per-member mc_dests), FDB entries,
// flush, aging, L2 events; LAG members join VLANs as their LAG.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class L2Area {
public:
    explicit L2Area(Core& c) : c_(c) {}

    // At init (after the sysports are known): one STP instance with every
    // port FORWARDING, VLAN 1 (empty), the LAG area reset, the hash profile.
    sai_status_t base_init(const std::vector<uint32_t>& devports);
    // Between ifcs_node_create and node ENABLE: multicast LAG resolution
    // (libifcs: an mc_dest on a LAG in a flood group needs it, and it cannot
    // be set once the node is enabled). SAI_TERALYNX_LAG_FLOOD=off skips it.
    void pre_enable();

    sai_status_t vlan_create(uint16_t vid);
    sai_status_t vlan_destroy(uint16_t vid);
    sai_status_t vlan_port_add(uint16_t vid, int hw, bool untagged);
    sai_status_t vlan_port_remove(uint16_t vid, int hw);
    sai_status_t vlan_members(uint16_t vid, std::vector<std::pair<int, bool>>& out);
    bool vlan_l2vni(uint16_t vid, ifcs_handle_t& l2vni) const;   // false: no such VLAN

    // dest: a port or lag_dest(tid)
    sai_status_t l2_add(const uint8_t mac[6], uint16_t vid, int dest, bool is_static, bool discard);
    sai_status_t l2_delete(const uint8_t mac[6], uint16_t vid);
    sai_status_t l2_flush(int dest, int vid, bool statics, bool dynamics);
    sai_status_t l2_age_set(uint32_t seconds);
    // VLAN flood control (Backend::vlan_flood_set): IFCS has one flood group
    // per L2VNI for unknown unicast, unknown multicast and broadcast, so all
    // three on = the members' flood group, all three off = an empty one;
    // a mixed setting is recorded, logged once and programmed only when the
    // three agree again (orchagent sets them one by one).
    sai_status_t vlan_flood_set(uint16_t vid, FloodKind kind, bool on);

    bool cpu_flood() const { return cpu_flood_; }

    // L2 event callback (SDK thread), user = Core*.
    static void on_l2(ifcs_node_id_t, void* user, ifcs_l2_event_info_t* info);

private:
    // A VLAN member's IFCS destination: its sysport, or its LAG's handle
    // while the port is in a LAG (shared by all members, counted).
    struct Dest { int refs = 0; ifcs_handle_t mcd = 0; };
    struct Member { bool untagged = false; ifcs_handle_t dest = 0; };
    struct Vlan {
        ifcs_handle_t l2vni = 0, mdg = 0, cpu_mcd = 0;
        ifcs_handle_t empty_mdg = 0;                // flood control off: an empty flood group
        bool flood[3] = {true, true, true};         // FloodKind order
        std::map<int, Member> members;              // devport -> member
        std::map<ifcs_handle_t, Dest> dests;        // L2VNI member / mc_dest per destination
    };
    sai_status_t vlan_make(uint16_t vid);
    sai_status_t dest_join(Vlan& v, ifcs_handle_t dest);
    void dest_leave(Vlan& v, ifcs_handle_t dest);
    ifcs_handle_t member_dest(int hw) const;
    bool is_lag_handle(ifcs_handle_t h) const { return !c_.dev_of_sysport.count(h); }
    void untagged_off(int hw);
    void l2_key(ifcs_l2_entry_key_t& k, const uint8_t mac[6], uint16_t vid);
    static void on_l2_entry(ifcs_node_id_t, ifcs_l2_entry_key_t* k, uint32_t, ifcs_attr_t*, void* user);

    Core& c_;
    ifcs_handle_t stp_ = 0;
    bool cpu_flood_ = false;          // SAI_TERALYNX_CPU_FLOOD=on: the CPU in every VLAN (the old model)
    std::map<uint16_t, Vlan> vlans_;
    std::map<int, uint16_t> untagged_vid_;
};

} // namespace tlb
} // namespace ob

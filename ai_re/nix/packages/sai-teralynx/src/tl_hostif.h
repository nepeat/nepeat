// SPDX-License-Identifier: Apache-2.0
// Host interface: one Linux netdev per port (ifcs_netdev or the SDK's own,
// adopted), protocol traps, the shared IP2ME trap of TRAP routes.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class HostifArea {
public:
    explicit HostifArea(Core& c) : c_(c) {}

    void configure(bool adopt) { netdev_adopt_ = adopt; }   // SAI_TERALYNX_NETDEV, at init

    sai_status_t netif_create(const std::string& ifname, int hw, uint16_t vid, const uint8_t mac[6], bool keep_tag,
                              int& netif_id, int& filter_id);
    sai_status_t netif_destroy(int netif_id, int filter_id);
    sai_status_t netif_carrier_set(const std::string& ifname, bool up);
    sai_status_t netif_set_keep_tag(int netif_id, int hw, bool keep_tag, int& filter_id);
    sai_status_t trap_set(TrapKind kind, bool to_cpu);
    // Full trap (Backend::trap_program): hostif trap with the group's queue,
    // CoPP entries (match, copy, drop, the group's policer). NOT_SUPPORTED for
    // trap types without a mapping (the frontend keeps them unprogrammed).
    sai_status_t trap_program(const TrapSpec& t, bool remove);
    std::string trap_report();                 // diag shell "copp": traps, queues, counters
    void teardown_all();                       // shutdown: every netdev deleted / given back

    // The hostif trap TRAP routes (ip2me, glean) copy to: created on first use.
    sai_status_t ip2me_trap(ifcs_handle_t& h);
    // A separate hostif trap for glean (neighbor miss) routes, as isai has
    // (NEIGHBOR_MISS programs its queue / policer); created on first use,
    // queue 0; the L3 area's glean egress copies to it.
    sai_status_t glean_trap(ifcs_handle_t& h);
    // System (exception) traps as isai sets them at init: TTL check and IPv4
    // options to the CPU, L2 source miss/move and L3 MTU check off. Once,
    // before the first netdev or trap (SAI_TERALYNX_SYSTEM_TRAPS=sdk: skip).
    void system_traps_init();
    // CPU queue counters (ifcs_cpu_queue_stats_get): frames / bytes the
    // queue sent to the CPU, frames it dropped.
    sai_status_t cpu_queue_stats(uint32_t q, uint64_t& packets, uint64_t& bytes, uint64_t& drops);

private:
    struct Netif {
        std::string name, orig;
        int hw = -1;
        ifcs_handle_t handle = 0;
        bool adopted = false, keep_tag = false, carrier_wish = false;
    };
    struct Trap {
        TrapSpec spec;
        ifcs_handle_t handle = 0;   // hostif trap (user trap, the shared ip2me one, or a system trap)
        bool user = false;          // ours to delete
        int copp = -1;              // AclArea install
    };
    sai_status_t netif_teardown(Netif& n);
    sai_status_t trap_handle_for(Trap& t, uint32_t sys_reason, bool ip2me);
    sai_status_t rename_netdev(const std::string& from, const std::string& to, const uint8_t* mac);

    Core& c_;
    bool netdev_adopt_ = false;
    ifcs_handle_t trap_ = 0;
    ifcs_handle_t glean_ = 0;
    bool sys_traps_done_ = false;
    std::map<int32_t, Trap> traps_;      // by SAI trap type
    std::map<int, Netif> netifs_;
    int next_netif_ = 1;
};

} // namespace tlb
} // namespace ob

// SPDX-License-Identifier: Apache-2.0
// Ports: admin/link state, speed, FEC, autoneg, loopback, MTU, PVID,
// learning, counters (devport and sysport attributes).
#pragma once

#include "tl_core.h"

#include <array>
#include <atomic>
#include <chrono>

namespace ob {
namespace tlb {

class PortArea {
public:
    explicit PortArea(Core& c) : c_(c) { for (auto& l : link_) l = LINK_UNKNOWN; }

    // At init, after every port went admin-down: SAI_TERALYNX_LINK_STATUS
    // and SAI_TERALYNX_STATS modes (see tl_port.cpp), link cache = down.
    void configure();

    std::vector<PortInfo> infos();
    sai_status_t admin_set(int hw, bool up);
    sai_status_t link_get(int hw, bool& up);
    sai_status_t speed_set(int hw, uint32_t mbps);
    sai_status_t speed_get(int hw, uint32_t& mbps);
    sai_status_t supported_speeds(int hw, std::vector<uint32_t>& out);
    sai_status_t frame_max_set(int hw, uint32_t bytes);
    sai_status_t fec_set(int hw, Fec f);
    sai_status_t autoneg_set(int hw, bool on);
    sai_status_t loopback_set(int hw, Loopback lb);
    sai_status_t pvid_set(int hw, uint16_t vid);
    sai_status_t learn_set(int hw, Learn mode);
    sai_status_t stat_get(int hw, PortStat s, uint64_t& v);
    sai_status_t stat_clear(int hw);

    void all_down();                                 // every port-map devport admin-down (no checks)
    // Link scan callback (SDK thread): forwards to the frontend.
    static void on_link(ifcs_node_id_t, void* user, ifcs_linkscan_event_info_t* info);
    void link_event(int hw, bool up);                // cache update (SDK thread; atomics only)

private:
    struct StatCache { bool valid = false; std::chrono::steady_clock::time_point when; uint64_t v[IFCS_DEVPORT_STATS_ID_ALL] = {0}; };
    static constexpr int MAX_DEVPORT = 512;
    static constexpr int8_t LINK_UNKNOWN = -1, LINK_DOWN = 0, LINK_UP = 1;
    Core& c_;
    std::map<int, StatCache> stats_;
    std::array<std::atomic<int8_t>, MAX_DEVPORT> link_;   // per devport; written by the link scan thread
    bool link_cached_ = false;                            // SAI_TERALYNX_LINK_STATUS=read (default) | cached
    bool stats_by_id_ = true;                             // SAI_TERALYNX_STATS=ids (default) | all
};

} // namespace tlb
} // namespace ob

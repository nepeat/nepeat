// SPDX-License-Identifier: Apache-2.0
// Counters breadth: SAI port counters by SAI id on the IFCS MAC (devport)
// counters, with isai 6.2.0's mapping on this chip (oracle hostif-counters,
// 2026-11-16, sai-oracle.md). The frontend asks only its 15 PortStats
// (PortArea::stat_get); everything else here is reachable through the diag
// shell ("counters <devport>") until the frontend has a by-id hook
// (sai-features.md, "Frontend requests").
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

// True when the backend has the SAI port stat (a constant 0 counts: isai
// reports those as supported too).
bool port_stat_mapped(sai_stat_id_t id);
std::vector<sai_stat_id_t> port_stat_ids();
// One devport_stats_get for all the IFCS counters the ids need.
sai_status_t port_stats_by_id(Core& c, int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out);
std::string counters_report(Core& c, int hw);   // diag shell "counters <devport>"

} // namespace tlb
} // namespace ob

// SPDX-License-Identifier: Apache-2.0
// LAGs (tl_lag.h). Not mapped: IFCS needs members detached from every
// L2VNI / mc_dest first and the LAG handle's use as L2VNI member / mc_dest /
// STP port is unknown (switchd/DESIGN.md U13). The L2/L3 areas refuse LAG
// destinations (is_lag_dest) with NOT_IMPLEMENTED until this exists.
#include "tl_lag.h"

namespace ob {
namespace tlb {

sai_status_t LagArea::create(int& tid) { tid = -1; return c_.not_impl("LAG"); }
sai_status_t LagArea::destroy(int) { return c_.not_impl("LAG"); }
sai_status_t LagArea::set_members(int, const std::vector<LagMember>&) { return c_.not_impl("LAG"); }

} // namespace tlb
} // namespace ob

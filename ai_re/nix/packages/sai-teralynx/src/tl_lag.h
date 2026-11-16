// SPDX-License-Identifier: Apache-2.0
// LAGs (IFCS lag + lag members). Not mapped yet: see tl_lag.cpp.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class LagArea {
public:
    explicit LagArea(Core& c) : c_(c) {}
    sai_status_t create(int& tid);
    sai_status_t destroy(int tid);
    sai_status_t set_members(int tid, const std::vector<LagMember>& members);

private:
    Core& c_;
};

} // namespace tlb
} // namespace ob

// SPDX-License-Identifier: Apache-2.0
// QoS and buffers (IFCS qos_map / queue / bam / dtm objects): a slot for
// the phase-B work. The frontend (sai-openbcm) has no backend calls for
// QoS maps, schedulers, buffer pools or profiles yet; its SAI objects for
// them are kept in the frontend only. When the frontend grows them, their
// IFCS side lives here.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class QosArea {
public:
    explicit QosArea(Core& c) : c_(c) {}

private:
    Core& c_;
};

} // namespace tlb
} // namespace ob

// SPDX-License-Identifier: Apache-2.0
// Mirror sessions and sampling (IFCS collector / visibility_sampler / ACL
// MIRROR action): a slot for the phase-B work. The frontend has no backend
// calls for mirror sessions or samplepacket yet.
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class MirrorArea {
public:
    explicit MirrorArea(Core& c) : c_(c) {}

private:
    Core& c_;
};

} // namespace tlb
} // namespace ob

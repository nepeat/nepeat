// SPDX-License-Identifier: Apache-2.0
// Fake-only build: there is no SDK backend in this libsai.
#include "backend.h"

namespace ob {
Backend* make_openbcm_backend() { return nullptr; }
} // namespace ob

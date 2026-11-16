// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, LAG (harness: tests/harness.h).
#include "harness.h"

void test_lag(Env& e)
{
    USE_ENV(e);
    // ---- NOT_IMPLEMENTED stays honest
    sai_object_id_t lag = 0;
    CHECK(la->create_lag(&lag, s, 0, nullptr) != SAI_STATUS_SUCCESS);

}

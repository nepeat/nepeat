// SPDX-License-Identifier: Apache-2.0
// isai-version: load the amd64 libsai (all its libraries, constructors) and
// print sai_query_api_version(). Touches no device: the first thing to run on
// the box under isai-qemu, with QEMU_IPD_TRACE=1 showing no ipd access.
#include <sai.h>
#include <stdio.h>
int main(void) {
    sai_api_version_t v = 0;
    sai_status_t s = sai_query_api_version(&v);
    printf("sai_query_api_version %d: %llu (headers %llu)\n", (int)s,
           (unsigned long long)v, (unsigned long long)SAI_API_VERSION);
    return s != SAI_STATUS_SUCCESS;
}

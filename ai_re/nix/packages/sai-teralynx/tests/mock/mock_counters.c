/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: CPU queue counters (mock.h). Queue q reads 100 + q frames
 * (64 B each) and q drops. */
#include "mock.h"

ifcs_status_t ifcs_cpu_queue_stats_get(ifcs_node_id_t n, ifcs_cpu_queue_t q, uint32_t c, ifcs_cpu_queue_stats_id_t* ids,
                                       uint64_t* v, uint32_t* got)
{
    (void)n;
    if (q >= 48) return IFCS_PARAM;
    for (uint32_t i = 0; i < c; i++)
        v[i] = ids[i] == IFCS_CPU_QUEUE_STATS_ID_PACKET_TX_COUNT ? 100 + q
             : ids[i] == IFCS_CPU_QUEUE_STATS_ID_BYTE_TX_COUNT ? (100 + q) * 64
             : ids[i] == IFCS_CPU_QUEUE_STATS_ID_PACKET_DROP_COUNT ? q : 0;
    if (got) *got = c;
    return IFCS_SUCCESS;
}

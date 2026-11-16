// SPDX-License-Identifier: Apache-2.0
// Counters breadth (tl_counters.h).
#include "tl_counters.h"

#include "tl_hostif.h"

#include <algorithm>
#include <cstdio>

namespace ob {
namespace tlb {

namespace {
// SAI port stat -> the IFCS devport counters summed for it (isai's choice
// on this chip). An empty list: supported, always 0 (no IFCS counter; isai
// answers 0 for these too).
struct StatMap { sai_stat_id_t sai; std::vector<int> ifcs; };
#define D(x) IFCS_DEVPORT_STATS_ID_##x
#define P(x) SAI_PORT_STAT_##x
const std::vector<StatMap>& stat_map()
{
    static const std::vector<StatMap> m = [] {
        std::vector<StatMap> v = {
            {P(IF_IN_OCTETS), {D(RX_BYTES_ALL)}}, {P(IF_IN_UCAST_PKTS), {D(RX_FRAMES_UNICAST)}},
            {P(IF_IN_NON_UCAST_PKTS), {D(RX_FRAMES_MULTICAST), D(RX_FRAMES_BROADCAST)}},
            {P(IF_IN_DISCARDS), {D(RX_FRAMES_DROP)}},          // isai: sysport drop reasons + the ACL drop counter
            {P(IF_IN_ERRORS), {D(RX_FRAMES_ERR_ANY)}}, {P(IF_IN_UNKNOWN_PROTOS), {}},
            {P(IF_IN_BROADCAST_PKTS), {D(RX_FRAMES_BROADCAST)}}, {P(IF_IN_MULTICAST_PKTS), {D(RX_FRAMES_MULTICAST)}},
            {P(IF_OUT_OCTETS), {D(TX_BYTES_OK)}}, {P(IF_OUT_UCAST_PKTS), {D(TX_FRAMES_UNICAST)}},
            {P(IF_OUT_NON_UCAST_PKTS), {D(TX_FRAMES_MULTICAST), D(TX_FRAMES_BROADCAST)}},
            {P(IF_OUT_DISCARDS), {D(TX_FRAMES_DROP)}},         // isai: the egress drop counter
            {P(IF_OUT_ERRORS), {D(TX_FRAMES_ERR_ANY)}},
            {P(IF_OUT_BROADCAST_PKTS), {D(TX_FRAMES_BROADCAST)}}, {P(IF_OUT_MULTICAST_PKTS), {D(TX_FRAMES_MULTICAST)}},
            {P(ETHER_STATS_DROP_EVENTS), {}}, {P(ETHER_STATS_FRAGMENTS), {}}, {P(ETHER_STATS_JABBERS), {}},
            {P(ETHER_STATS_COLLISIONS), {}},
            {P(ETHER_STATS_MULTICAST_PKTS), {D(RX_FRAMES_MULTICAST), D(TX_FRAMES_MULTICAST)}},
            {P(ETHER_STATS_BROADCAST_PKTS), {D(RX_FRAMES_BROADCAST), D(TX_FRAMES_BROADCAST)}},
            {P(ETHER_STATS_UNDERSIZE_PKTS), {D(RX_FRAMES_ERR_RUNT)}},
            {P(ETHER_STATS_OVERSIZE_PKTS), {D(RX_FRAMES_ERR_OVER), D(TX_FRAMES_ERR_OVER)}},
            {P(ETHER_RX_OVERSIZE_PKTS), {D(RX_FRAMES_ERR_OVER)}}, {P(ETHER_TX_OVERSIZE_PKTS), {D(TX_FRAMES_ERR_OVER)}},
            {P(ETHER_STATS_OCTETS), {D(RX_BYTES_ALL), D(TX_BYTES_ALL)}},
            {P(ETHER_STATS_PKTS), {D(RX_FRAMES_ALL), D(TX_FRAMES_ALL)}},
            {P(ETHER_STATS_CRC_ALIGN_ERRORS), {D(RX_FRAMES_ERR_CRC), D(TX_FRAMES_ERR_CRC)}},
            {P(ETHER_STATS_TX_NO_ERRORS), {D(TX_FRAMES_OK)}}, {P(ETHER_STATS_RX_NO_ERRORS), {D(RX_FRAMES_OK)}},
            {P(GREEN_WRED_DROPPED_PACKETS), {D(AQM_GREEN_PACKET_DROP_COUNT)}},
            {P(YELLOW_WRED_DROPPED_PACKETS), {D(AQM_YELLOW_PACKET_DROP_COUNT)}},
            {P(RED_WRED_DROPPED_PACKETS), {D(AQM_RED_PACKET_DROP_COUNT)}},
            {P(WRED_DROPPED_PACKETS), {D(AQM_GREEN_PACKET_DROP_COUNT), D(AQM_YELLOW_PACKET_DROP_COUNT), D(AQM_RED_PACKET_DROP_COUNT)}},
            {P(PAUSE_RX_PKTS), {D(RX_FRAMES_PAUSE)}}, {P(PAUSE_TX_PKTS), {D(TX_FRAMES_PAUSE)}},
            {P(IF_IN_FEC_CORRECTABLE_FRAMES), {D(RX_KRPFEC_CORRECTED_CW_COUNT)}},
            {P(IF_IN_FEC_NOT_CORRECTABLE_FRAMES), {D(RX_KRPFEC_UNCORRECTED_CW_COUNT)}},
            {P(IF_IN_FEC_SYMBOL_ERRORS), {D(RX_KRPFEC_CW_SYMBOL_ERR)}},
        };
        // frame size buckets: ETHER_STATS_* = rx + tx, ETHER_IN_* rx, ETHER_OUT_* tx;
        // 64 B includes the runts (LEN_LT_64) for IN / OUT, as isai
        struct B { sai_stat_id_t all, in, out; std::vector<int> off; };
        const B b[] = {
            {P(ETHER_STATS_PKTS_64_OCTETS), P(ETHER_IN_PKTS_64_OCTETS), P(ETHER_OUT_PKTS_64_OCTETS), {1}},
            {P(ETHER_STATS_PKTS_65_TO_127_OCTETS), P(ETHER_IN_PKTS_65_TO_127_OCTETS), P(ETHER_OUT_PKTS_65_TO_127_OCTETS), {2}},
            {P(ETHER_STATS_PKTS_128_TO_255_OCTETS), P(ETHER_IN_PKTS_128_TO_255_OCTETS), P(ETHER_OUT_PKTS_128_TO_255_OCTETS), {3}},
            {P(ETHER_STATS_PKTS_256_TO_511_OCTETS), P(ETHER_IN_PKTS_256_TO_511_OCTETS), P(ETHER_OUT_PKTS_256_TO_511_OCTETS), {4}},
            {P(ETHER_STATS_PKTS_512_TO_1023_OCTETS), P(ETHER_IN_PKTS_512_TO_1023_OCTETS), P(ETHER_OUT_PKTS_512_TO_1023_OCTETS), {5}},
            {P(ETHER_STATS_PKTS_1024_TO_1518_OCTETS), P(ETHER_IN_PKTS_1024_TO_1518_OCTETS), P(ETHER_OUT_PKTS_1024_TO_1518_OCTETS), {6}},
            {P(ETHER_STATS_PKTS_1519_TO_2047_OCTETS), P(ETHER_IN_PKTS_1519_TO_2047_OCTETS), P(ETHER_OUT_PKTS_1519_TO_2047_OCTETS), {7}},
            {P(ETHER_STATS_PKTS_2048_TO_4095_OCTETS), P(ETHER_IN_PKTS_2048_TO_4095_OCTETS), P(ETHER_OUT_PKTS_2048_TO_4095_OCTETS), {8}},
            {P(ETHER_STATS_PKTS_4096_TO_9216_OCTETS), P(ETHER_IN_PKTS_4096_TO_9216_OCTETS), P(ETHER_OUT_PKTS_4096_TO_9216_OCTETS), {9, 10}},
            {P(ETHER_STATS_PKTS_9217_TO_16383_OCTETS), P(ETHER_IN_PKTS_9217_TO_16383_OCTETS), P(ETHER_OUT_PKTS_9217_TO_16383_OCTETS), {11}},
        };
        for (auto& x : b) {
            StatMap all{x.all, {}}, in{x.in, {}}, out{x.out, {}};
            for (int o : x.off) {
                all.ifcs.push_back(D(RX_FRAMES_LEN_LT_64) + o);
                all.ifcs.push_back(D(TX_FRAMES_LEN_LT_64) + o);
                in.ifcs.push_back(D(RX_FRAMES_LEN_LT_64) + o);
                out.ifcs.push_back(D(TX_FRAMES_LEN_LT_64) + o);
            }
            if (x.off[0] == 1) {   // 64 B: + runts
                in.ifcs.push_back(D(RX_FRAMES_LEN_LT_64));
                out.ifcs.push_back(D(TX_FRAMES_LEN_LT_64));
            }
            v.push_back(all);
            v.push_back(in);
            v.push_back(out);
        }
        for (int p = 0; p < 8; p++) {   // PFC frames and pause time per priority
            v.push_back({(sai_stat_id_t)(P(PFC_0_RX_PKTS) + 2 * p), {D(RX_FRAMES_PFC0) + p}});
            v.push_back({(sai_stat_id_t)(P(PFC_0_TX_PKTS) + 2 * p), {D(TX_FRAMES_PFC0) + p}});
            v.push_back({(sai_stat_id_t)(P(PFC_0_RX_PAUSE_DURATION) + 2 * p), {D(RX_TIME_PFC0) + p}});
            v.push_back({(sai_stat_id_t)(P(PFC_0_TX_PAUSE_DURATION) + 2 * p), {D(TX_TIME_PFC0) + p}});
        }
        return v;
    }();
    return m;
}
#undef D
#undef P

const StatMap* find_stat(sai_stat_id_t id)
{
    for (auto& s : stat_map())
        if (s.sai == id) return &s;
    return nullptr;
}
} // namespace

bool port_stat_mapped(sai_stat_id_t id) { return find_stat(id) != nullptr; }

std::vector<sai_stat_id_t> port_stat_ids()
{
    std::vector<sai_stat_id_t> v;
    for (auto& s : stat_map()) v.push_back(s.sai);
    return v;
}

sai_status_t port_stats_by_id(Core& c, int hw, uint32_t n, const sai_stat_id_t* ids, uint64_t* out)
{
    Guard g(c.mu);
    if (!c.front(hw)) return SAI_STATUS_INVALID_PORT_NUMBER;
    std::vector<int> want;
    std::vector<const StatMap*> maps(n);
    for (uint32_t i = 0; i < n; i++) {
        maps[i] = find_stat(ids[i]);
        if (!maps[i]) return SAI_STATUS_NOT_SUPPORTED;
        for (int x : maps[i]->ifcs)
            if (std::find(want.begin(), want.end(), x) == want.end()) want.push_back(x);
    }
    std::vector<uint64_t> vals(want.size(), 0);
    if (!want.empty()) {
        uint32_t got = 0;
        int r = IFCS_STATUS_REASON(c.L.ifcs_devport_stats_get(NODE, (ifcs_devport_t)hw, (uint32_t)want.size(), want.data(),
                                                              vals.data(), &got));
        if (r != IFCS_SUCCESS) return c.fail(r, "devport stats get (by id)");
    }
    for (uint32_t i = 0; i < n; i++) {
        uint64_t s = 0;
        for (int x : maps[i]->ifcs) s += vals[(size_t)(std::find(want.begin(), want.end(), x) - want.begin())];
        out[i] = s;
    }
    return SAI_STATUS_SUCCESS;
}

std::string counters_report(Core& c, int hw)
{
    std::string out;
    char b[160];
    std::vector<sai_stat_id_t> ids = port_stat_ids();
    std::vector<uint64_t> v(ids.size(), 0);
    sai_status_t st = port_stats_by_id(c, hw, (uint32_t)ids.size(), ids.data(), v.data());
    if (st != SAI_STATUS_SUCCESS) {
        std::snprintf(b, sizeof(b), "devport %d: counters unreadable (%d)\n", hw, st);
        return b;
    }
    std::snprintf(b, sizeof(b), "devport %d: %zu SAI port counters (SAI_PORT_STAT_* id = value)\n", hw, ids.size());
    out += b;
    for (size_t i = 0; i < ids.size(); i++) {
        std::snprintf(b, sizeof(b), "%d=%llu%s", (int)ids[i], (unsigned long long)v[i], (i % 8 == 7 || i + 1 == ids.size()) ? "\n" : " ");
        out += b;
    }
    for (uint32_t q = 0; q < 8; q++) {
        uint64_t pk = 0, by = 0, dr = 0;
        if (c.hostif->cpu_queue_stats(q, pk, by, dr) != SAI_STATUS_SUCCESS) continue;
        std::snprintf(b, sizeof(b), "cpu queue %u: tx %llu bytes %llu drops %llu\n", q, (unsigned long long)pk,
                      (unsigned long long)by, (unsigned long long)dr);
        out += b;
    }
    return out;
}

} // namespace tlb
} // namespace ob

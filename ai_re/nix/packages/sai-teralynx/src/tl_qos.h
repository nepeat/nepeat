// SPDX-License-Identifier: Apache-2.0
// QoS and buffers on IFCS (Backend QoS calls, backend.h), the way isai
// 6.2.0 programs this chip (oracle, devices/cs8210-32x/sai-oracle.md "QoS"):
//  - DSCP / DOT1P -> TC: one IFCS qos_map per SAI map (complete tables,
//    tc_dp values, dp 0), a qos_map_group per (DSCP map, DOT1P map) pair
//    shared by the ports with that pair, the sysport's
//    QOS_DSCP_TO_TC_DP_MAP / QOS_DOT1P_CFI_TO_TC_DP_MAP = the group;
//  - TC -> queue: the chip's fixed identity (isai programs nothing; a
//    non-identity map is NOT_SUPPORTED); TC -> PG, PFC priority -> queue /
//    PG: accepted, nothing programmed (isai: the same); colour and remark
//    maps (TC_AND_COLOR_TO_*): NOT_SUPPORTED;
//  - schedulers: queue SCHEDULING_MODE (strict / WDRR) + WDRR_WEIGHT +
//    guaranteed / max rate; the port shaper: devport MAX_RATE (+ burst);
//  - WRED: an aqm_curve_group per profile (thresholds in cells), queue
//    AQM_CURVE_GROUP + WRED_ENABLE + ECN_EXTENSION_ENABLE;
//  - buffer profiles on queues: RESERVED_SIZE (cells, port flapped around a
//    change while it is up, as isai), threshold mode and factors (dynamic
//    th n: unicast factor n + 5, non-unicast n + 1, isai's mapping); ingress
//    PG profiles: kept, not programmed (no lossless partition in this SDK
//    init config: isai finds none either);
//  - counters: queue_stats_get, bam_queue_stats_get (PGs), devport PFC frames.
#pragma once

#include "tl_core.h"

#include <array>

namespace ob {
namespace tlb {

class QosArea {
public:
    explicit QosArea(Core& c) : c_(c) {}

    sai_status_t map_create(QosMapKind k, const std::vector<QosMapEntry>& e, int& id);
    sai_status_t map_update(int id, const std::vector<QosMapEntry>& e);
    sai_status_t map_destroy(int id);
    sai_status_t port_map_set(int hw, QosMapKind k, int id);
    sai_status_t default_tc_set(int hw, uint8_t tc);
    sai_status_t pfc_set(int hw, bool separate, uint8_t tx, uint8_t rx);
    sai_status_t queue_sched_set(int hw, int q, const SchedCfg* s);
    sai_status_t port_sched_set(int hw, const SchedCfg* s);
    sai_status_t wred_create(const WredCfg& w, int& id);
    sai_status_t wred_update(int id, const WredCfg& w);
    sai_status_t wred_destroy(int id);
    sai_status_t queue_wred_set(int hw, int q, int id);
    sai_status_t queue_buffer_set(int hw, int q, const BufferCfg* b);
    sai_status_t pg_buffer_set(int hw, int pg, const BufferCfg* b);
    sai_status_t queue_stats_get(int hw, int q, uint32_t n, const sai_stat_id_t* ids, uint64_t* out);
    sai_status_t queue_stats_clear(int hw, int q, uint32_t n, const sai_stat_id_t* ids);
    sai_status_t pg_stats_get(int hw, int pg, uint32_t n, const sai_stat_id_t* ids, uint64_t* out);
    sai_status_t pg_stats_clear(int hw, int pg, uint32_t n, const sai_stat_id_t* ids);
    sai_status_t pfc_stat_get(int hw, int prio, bool tx, uint64_t& v);
    uint32_t cell_bytes();                       // DTM cell size (node DTM_CELL_SIZE, else 224)
    std::string show();                          // diag "qos"

private:
    struct Map { QosMapKind kind; std::vector<QosMapEntry> e; ifcs_handle_t h = IFCS_NULL_HANDLE; };
    struct Group { ifcs_handle_t h = IFCS_NULL_HANDLE; int refs = 0; };
    struct Wred { WredCfg cfg; ifcs_handle_t h = IFCS_NULL_HANDLE; };
    struct QueueDefaults { bool have = false; uint32_t reserved = 0, mode = 0, uc = 0, nonuc = 0, uc_static = 0, nonuc_static = 0; };
    using Key = std::pair<int, int>;             // (DSCP map id, DOT1P map id), -1 = none

    ifcs_queue_t queue_key(int hw, int q);
    sai_status_t queue_set(int hw, int q, std::vector<ifcs_attr_t> a, const char* what);
    sai_status_t map_info(const Map& m, ifcs_attr_t& a, std::vector<ifcs_qos_map_t>& store);
    sai_status_t group_get(const Key& k, ifcs_handle_t& h);
    void group_put(const Key& k);
    std::vector<ifcs_attr_t> curve_attrs(const WredCfg& w);
    sai_status_t wred_flags(int hw, int q, const WredCfg* w);
    sai_status_t queue_defaults(int hw, int q, QueueDefaults& d);

    Core& c_;
    int next_ = 1;
    uint32_t cell_ = 0;
    std::map<int, Map> maps_;
    std::map<Key, Group> groups_;
    std::map<int, Key> port_key_;                // hw -> the group key it uses
    std::map<int, Wred> wreds_;
    std::map<Key, int> queue_wred_;              // (hw, q) -> wred id
    std::map<Key, QueueDefaults> queue_defaults_;
    std::map<Key, uint32_t> queue_reserved_;     // what we programmed (cells)
    std::set<int> pg_logged_;
    std::map<int, uint32_t> pfc_mode_;           // hw -> devport PFC_MODE as programmed
    std::set<Key> queue_ecn_;                    // queues with ECN_EXTENSION_ENABLE on
};

} // namespace tlb
} // namespace ob

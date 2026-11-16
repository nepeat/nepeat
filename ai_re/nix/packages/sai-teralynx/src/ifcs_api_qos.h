/* SPDX-License-Identifier: Apache-2.0
 *
 * IFCS 0.12.41 declarations for the QoS / buffers and mirror / sampling
 * areas of the sai-teralynx backend (src/tl_qos.cpp, src/tl_mirror.cpp):
 * QoS maps and map groups, queues (scheduling, shaping, AQM / WRED, buffer
 * thresholds, counters), BAM queues (ingress priority-group counters), AQM
 * curve groups, duplicate-copy destination ports, collectors and collector
 * sets, the sysport traffic-monitor policies (mirror, sFlow). Included by
 * ifcs_api.h (its IFCS_SYMBOLS lists IFCS_SYMBOLS_QOS); same rules: names,
 * numeric values and layouts only, no vendor code.
 *
 * Values: pyctypes/ifcs_ctypes.py (sha256 ba980c0b..., see ifcs_api.h);
 * attribute value types from the SDK's CLI python (ifcs_cmds/queue.py,
 * devport.py, sysport.py, collector.py, aqm_curve_group.py getters).
 * Layouts: ifcs_traffic_monitor_policy_s from innovium.user DWARF (0.12.41:
 * 20 bytes, no actual_sampling_rate member); the QoS map structs and the
 * queue keys from libifcs 7.0.1 DWARF (pahole), identical field lists in
 * the 0.12.41 pyctypes. Every function below is exported by the target
 * libifcs.so (nm -D, sha256 388031e7...). Call forms: what isai 6.2.0 does on
 * this chip (oracle traces, devices/cs8210-32x/sai-oracle.md "QoS", "Mirror").
 */
#ifndef SAI_TERALYNX_IFCS_API_QOS_H
#define SAI_TERALYNX_IFCS_API_QOS_H

/* ----------------------------------------------------------- QoS maps */
enum { IFCS_QOS_MAP_ATTR_QOS_MAP_INFO = 0 };
enum {                                 /* ifcs_qos_map_type_e */
    IFCS_QOS_MAP_TYPE_DOT1P_CFI_TO_TC_DP = 0, IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP = 1,
    IFCS_QOS_MAP_TYPE_TC_DP_TO_DOT1P_CFI = 3, IFCS_QOS_MAP_TYPE_TC_DP_TO_DSCP = 4,
};
enum {                                 /* ifcs_qos_map_group_attr_id_e */
    IFCS_QOS_MAP_GROUP_ATTR_ACTION_TYPE = 0, IFCS_QOS_MAP_GROUP_ATTR_DOT1P_CFI_TO_TC_DP = 1,
    IFCS_QOS_MAP_GROUP_ATTR_DSCP_TO_TC_DP = 2, IFCS_QOS_MAP_GROUP_ATTR_TC_DP_TO_DOT1P_CFI = 4,
    IFCS_QOS_MAP_GROUP_ATTR_TC_DP_TO_DSCP = 5,
};
enum { IFCS_QOS_ACTION_TYPE_MAP = 0, IFCS_QOS_ACTION_TYPE_MARK = 1 };
typedef struct { uint32_t tc; uint32_t dp; uint8_t dirty; } ifcs_qos_tc_dp_t;
typedef struct { uint32_t dot1p; uint8_t cfi; uint8_t dirty; } ifcs_qos_dot1p_cfi_t;
typedef union { ifcs_qos_tc_dp_t tc_dp; uint32_t dscp; ifcs_qos_dot1p_cfi_t dot1p_cfi; uint32_t exp; uint32_t queue; } ifcs_qos_map_fields_t;
typedef struct { ifcs_qos_map_fields_t map_src; ifcs_qos_map_fields_t map_value; uint8_t dirty; } ifcs_qos_map_t;
typedef struct { uint32_t count; ifcs_qos_map_t* arr; } ifcs_qos_map_list_t;
typedef struct { int type; ifcs_qos_map_list_t list; uint8_t dirty; } ifcs_qos_map_info_t;

/* ---------------------------------------------- sysport / devport attributes */
enum {                                 /* ifcs_sysport_attr_id_e (subset) */
    IFCS_SYSPORT_ATTR_QOS_DOT1P_CFI_TO_TC_DP_MAP = 0, IFCS_SYSPORT_ATTR_QOS_DSCP_TO_TC_DP_MAP = 1,
    IFCS_SYSPORT_ATTR_QOS_TC_DP_TO_DOT1P_CFI_MAP = 3, IFCS_SYSPORT_ATTR_QOS_TC_DP_TO_DSCP_MAP = 4,
    IFCS_SYSPORT_ATTR_MIRROR_INGRESS = 22, IFCS_SYSPORT_ATTR_MIRROR_EGRESS = 24,
    IFCS_SYSPORT_ATTR_SFLOW_INGRESS = 25, IFCS_SYSPORT_ATTR_SFLOW_EGRESS = 26, IFCS_SYSPORT_ATTR_DEFAULT_TC = 49,
};
enum {                                 /* ifcs_devport_attr_id_e (subset); values u32 unless noted */
    IFCS_DEVPORT_ATTR_PFC_MODE = 78, IFCS_DEVPORT_ATTR_PFC0_ENABLE = 79, IFCS_DEVPORT_ATTR_PFC0_TX_ENABLE = 87,
    IFCS_DEVPORT_ATTR_PFC0_RX_ENABLE = 95, IFCS_DEVPORT_ATTR_MAX_RATE_ENABLE = 104,
    IFCS_DEVPORT_ATTR_MAX_RATE = 105 /* u64 */, IFCS_DEVPORT_ATTR_MAX_BURST_SIZE = 106,
    IFCS_DEVPORT_ATTR_ACTUAL_MAX_RATE = 107 /* u64 */, IFCS_DEVPORT_ATTR_ACTUAL_MAX_BURST_SIZE = 108,
};
enum { IFCS_DEVPORT_PFC_MODE_COMBINED = 0, IFCS_DEVPORT_PFC_MODE_SEPARATE = 1 };
enum {                                 /* ifcs_node_attr_id_e (subset) */
    IFCS_NODE_ATTR_EGRESS_SFLOW_ENABLE = 8, IFCS_NODE_ATTR_CPU_SYSPORT = 22 /* handle */,
    IFCS_NODE_ATTR_DTM_CELL_SIZE = 23 /* u16, bytes (224 on this chip, isai's read) */,
};

/* --------------------------------------------------------------- queues */
typedef struct { uint32_t _base_key; ifcs_devport_t devport; uint32_t queue_id; uint8_t dirty; } ifcs_queue_t;
typedef ifcs_queue_t ifcs_bam_queue_t;  /* same fields (devport, queue_id) */
enum {                                 /* ifcs_queue_attr_id_e; u32 unless noted */
    IFCS_QUEUE_ATTR_RESERVED_SIZE = 2, IFCS_QUEUE_ATTR_THRESHOLD_MODE = 3,
    IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT = 4, IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT = 5,
    IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR = 6, IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR = 9,
    IFCS_QUEUE_ATTR_SCHEDULING_MODE = 13, IFCS_QUEUE_ATTR_WDRR_WEIGHT = 14,
    IFCS_QUEUE_ATTR_GUARANTEED_RATE_ENABLE = 16, IFCS_QUEUE_ATTR_GUARANTEED_RATE = 17 /* u64 */,
    IFCS_QUEUE_ATTR_GUARANTEED_RATE_BURST_SIZE = 18, IFCS_QUEUE_ATTR_MAX_RATE_ENABLE = 21,
    IFCS_QUEUE_ATTR_MAX_RATE = 22 /* u64 */, IFCS_QUEUE_ATTR_MAX_RATE_BURST_SIZE = 23,
    IFCS_QUEUE_ATTR_AQM_CURVE_GROUP = 26 /* handle */, IFCS_QUEUE_ATTR_WRED_ENABLE = 27,
    IFCS_QUEUE_ATTR_ECN_EXTENSION_ENABLE = 28, IFCS_QUEUE_ATTR_AQM_QSIZE_AVERAGING_WEIGHT = 32,
    IFCS_QUEUE_ATTR_FORCE_BYPASS_FLOW_CONTROL = 34, IFCS_QUEUE_ATTR_FORCE_DO_NOT_ADMIT = 35,
};
enum { IFCS_QUEUE_SCHEDULING_MODE_STRICT = 0, IFCS_QUEUE_SCHEDULING_MODE_WDRR = 1 };
enum { IFCS_QUEUE_THRESHOLD_MODE_STATIC = 0, IFCS_QUEUE_THRESHOLD_MODE_DYNAMIC = 1 };
typedef int ifcs_queue_stats_id_t;
enum {
    IFCS_QUEUE_STATS_ID_TOTAL_USE_COUNT = 0, IFCS_QUEUE_STATS_ID_TOTAL_USE_WATERMARK = 1,
    IFCS_QUEUE_STATS_ID_PACKET_DROP_COUNT = 3, IFCS_QUEUE_STATS_ID_CELL_DROP_COUNT = 4,
    IFCS_QUEUE_STATS_ID_PACKET_TX_COUNT = 5, IFCS_QUEUE_STATS_ID_BYTE_TX_COUNT = 6,
    IFCS_QUEUE_STATS_ID_PACKET_ECN_MARKED_COUNT = 8, IFCS_QUEUE_STATS_ID_BYTE_ECN_MARKED_COUNT = 9,
};
typedef int ifcs_bam_queue_stats_id_t;
enum {
    IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_COUNT = 0, IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_WATERMARK = 1,
    IFCS_BAM_QUEUE_STATS_ID_PACKET_DROP_COUNT = 2, IFCS_BAM_QUEUE_STATS_ID_BYTE_DROP_COUNT = 3,
};
enum {                                 /* ifcs_aqm_curve_group_attr_id_e; u32 (cells, percent) */
    IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MIN_THRESHOLD = 0, IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAX_THRESHOLD = 1,
    IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAXP_PERCENTAGE = 2, IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MIN_THRESHOLD = 6,
    IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAX_THRESHOLD = 7, IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAXP_PERCENTAGE = 8,
    IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MIN_THRESHOLD = 12, IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAX_THRESHOLD = 13,
    IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAXP_PERCENTAGE = 14,
};

/* --------------------------------------------------- mirror / sampling */
enum { IFCS_DUP_COPY_DEST_PORT_ATTR_SYSPORT = 0 };
enum {                                 /* ifcs_collector_attr_id_e; values per the CLI's collector.py */
    IFCS_COLLECTOR_ATTR_COLLECTOR_TYPE = 0 /* u32 */, IFCS_COLLECTOR_ATTR_DIRECTION = 1 /* u32 */,
    IFCS_COLLECTOR_ATTR_DCDP = 2 /* handle */, IFCS_COLLECTOR_ATTR_DEST_MAC = 9, IFCS_COLLECTOR_ATTR_SRC_MAC = 10,
    IFCS_COLLECTOR_ATTR_SRC_IP = 11 /* ip_addr */, IFCS_COLLECTOR_ATTR_DEST_IP = 12, IFCS_COLLECTOR_ATTR_DSCP = 13 /* u8 */,
    IFCS_COLLECTOR_ATTR_TTL = 14 /* u8 */, IFCS_COLLECTOR_ATTR_DOT1Q_VLAN = 16 /* u16 */,
    IFCS_COLLECTOR_ATTR_SESSION_ID = 17 /* u16 */, IFCS_COLLECTOR_ATTR_TRUNCATE = 18 /* u32 */,
};
enum {
    IFCS_COLLECTOR_TYPE_SPAN = 0, IFCS_COLLECTOR_TYPE_RSPAN = 1, IFCS_COLLECTOR_TYPE_ERSPAN_TYPE_II = 3,
    IFCS_COLLECTOR_TYPE_SFLOW = 5,
};
enum { IFCS_COLLECTOR_SET_ATTR_COLLECTOR_SET_TYPE = 0, IFCS_COLLECTOR_SET_ATTR_DIRECTION = 1 };
enum {
    IFCS_TRAFFIC_MONITOR_ACTION_ENABLE = 0, IFCS_TRAFFIC_MONITOR_ACTION_DISABLE = 1,
    IFCS_TRAFFIC_MONITOR_CONTAINER_0 = 0, IFCS_TRAFFIC_MONITOR_CONTAINER_1 = 1,
    IFCS_TRAFFIC_MONITOR_DIRECTION_INGRESS = 0, IFCS_TRAFFIC_MONITOR_DIRECTION_EGRESS = 1,
    IFCS_TRAFFIC_MONITOR_TRUNCATE_NO = 0, IFCS_TRAFFIC_MONITOR_TRUNCATE_YES = 1,
};
typedef struct {                       /* DWARF (innovium.user): 20 bytes */
    int traffic_monitor_action;
    ifcs_handle_t collector_set;
    int traffic_monitor_container;
    uint32_t sampling_rate;
    uint8_t dirty;
} ifcs_traffic_monitor_policy_t;

/* ------------------------------------------------------- function pointers */
#define IFCS_FN(ret, name, args) typedef ret(*name##_fn) args;
IFCS_FN(ifcs_status_t, ifcs_qos_map_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_qos_map_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_qos_map_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_qos_map_info_t_init, (ifcs_qos_map_info_t*))
IFCS_FN(ifcs_status_t, ifcs_qos_map_group_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_qos_map_group_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_qos_map_group_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_sysport_attr_get, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_queue_t_init, (ifcs_queue_t*))
IFCS_FN(ifcs_status_t, ifcs_queue_t_devport_set, (ifcs_queue_t*, ifcs_devport_t))
IFCS_FN(ifcs_status_t, ifcs_queue_t_queue_id_set, (ifcs_queue_t*, uint32_t))
IFCS_FN(ifcs_status_t, ifcs_queue_attr_set, (ifcs_node_id_t, ifcs_queue_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_queue_attr_get, (ifcs_node_id_t, ifcs_queue_t*, uint32_t, ifcs_attr_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_queue_stats_get, (ifcs_node_id_t, ifcs_queue_t*, uint32_t, ifcs_queue_stats_id_t*, uint64_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_queue_stats_clear, (ifcs_node_id_t, ifcs_queue_t*, uint32_t, ifcs_queue_stats_id_t*))
IFCS_FN(ifcs_status_t, ifcs_bam_queue_t_init, (ifcs_bam_queue_t*))
IFCS_FN(ifcs_status_t, ifcs_bam_queue_stats_get, (ifcs_node_id_t, ifcs_bam_queue_t*, uint32_t, ifcs_bam_queue_stats_id_t*, uint64_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_bam_queue_stats_clear, (ifcs_node_id_t, ifcs_bam_queue_t*, uint32_t, ifcs_bam_queue_stats_id_t*))
IFCS_FN(ifcs_status_t, ifcs_aqm_curve_group_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_aqm_curve_group_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_aqm_curve_group_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_dup_copy_dest_port_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_dup_copy_dest_port_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_collector_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_collector_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_collector_set_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_collector_set_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_collector_set_member_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_collector_set_member_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_traffic_monitor_policy_t_init, (ifcs_traffic_monitor_policy_t*))
#undef IFCS_FN

#define IFCS_SYMBOLS_QOS(X) \
    X(ifcs_qos_map_create) X(ifcs_qos_map_attr_set) X(ifcs_qos_map_delete) X(ifcs_qos_map_info_t_init) \
    X(ifcs_qos_map_group_create) X(ifcs_qos_map_group_attr_set) X(ifcs_qos_map_group_delete) X(ifcs_sysport_attr_get) \
    X(ifcs_queue_t_init) X(ifcs_queue_t_devport_set) X(ifcs_queue_t_queue_id_set) X(ifcs_queue_attr_set) \
    X(ifcs_queue_attr_get) X(ifcs_queue_stats_get) X(ifcs_queue_stats_clear) X(ifcs_bam_queue_t_init) \
    X(ifcs_bam_queue_stats_get) X(ifcs_bam_queue_stats_clear) X(ifcs_aqm_curve_group_create) \
    X(ifcs_aqm_curve_group_attr_set) X(ifcs_aqm_curve_group_delete) X(ifcs_dup_copy_dest_port_create) \
    X(ifcs_dup_copy_dest_port_delete) X(ifcs_collector_create) X(ifcs_collector_delete) X(ifcs_collector_set_create) \
    X(ifcs_collector_set_delete) X(ifcs_collector_set_member_add) X(ifcs_collector_set_member_remove) \
    X(ifcs_traffic_monitor_policy_t_init)

#ifdef __cplusplus
static_assert(sizeof(ifcs_traffic_monitor_policy_t) == 20, "ifcs_traffic_monitor_policy_s (DWARF 0.12.41)");
static_assert(sizeof(ifcs_qos_tc_dp_t) == 12 && sizeof(ifcs_qos_dot1p_cfi_t) == 8 && sizeof(ifcs_qos_map_fields_t) == 12,
              "qos map fields");
static_assert(sizeof(ifcs_qos_map_t) == 28 && offsetof(ifcs_qos_map_t, map_value) == 12, "ifcs_qos_map_s");
static_assert(sizeof(ifcs_qos_map_info_t) == 32 && offsetof(ifcs_qos_map_info_t, list) == 8, "ifcs_qos_map_info_s");
static_assert(sizeof(ifcs_queue_t) == 16 && offsetof(ifcs_queue_t, queue_id) == 8, "ifcs_queue_s");
#endif

#endif

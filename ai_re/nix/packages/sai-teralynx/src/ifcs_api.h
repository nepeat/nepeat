/* SPDX-License-Identifier: Apache-2.0
 *
 * Our own declarations of the subset of the Innovium IFCS API (Teralynx 7,
 * IFCS 0.12.41) that the sai-teralynx backend calls through dlsym. Names,
 * numeric values and layouts only; no vendor code or comments.
 *
 * Values were read from (vendor files, local only, never in this repo):
 *   pyctypes/ifcs_ctypes.py  sha256 ba980c0b63495575e5f2ad2ead221efcb83641cb8d22dc1128d3ef8b8f91ed46
 *     (ctypesgen output of every IFCS header of this version: enums, field
 *      lists, function argtypes)
 * Layouts cross-checked against DWARF in
 *   innovium.user            sha256 2b3eb2d24663d0affb7234ea138866a25c9da26af828c9bad8e08f96572fa244
 *     (ifcs_attr_s, ifcs_attr_value_u, ifcs_ip_address_s, ifcs_ip_prefix_s,
 *      ifcs_handle_list_s, ifcs_u32_list_s, ifcs_mac_list_s,
 *      ifcs_ctc_policy_s, ifcs_fwd_policy_s, ifcs_reason_e,
 *      ifcs_node_attr_id_e): the static_asserts marked DWARF.
 * The key/event structs below are not in that DWARF; their layout is
 * computed from the pyctypes field lists (marked CTYPES). The two key structs
 * have the same size in IFCS 7.0.1's libifcs DWARF (amd64, sha256 72afa220...),
 * whose ifcs_attr_t is larger (296 B): never mix the two versions.
 * Target library: libifcs.so sha256 388031e7eb2443d77d89da826084976859bf92db19f9733be19b1c0b603c0d17.
 * The ACL / hostif trap call forms follow SDK/scripts/sonic_l3snake_acl.py
 * (table, profiles, matches, actions, ACEs) and what isai 6.2.0 does on the
 * same chip (oracle traces, oracle/README.md); values from pyctypes.
 */
#ifndef SAI_TERALYNX_IFCS_API_H
#define SAI_TERALYNX_IFCS_API_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------ base types */
typedef int ifcs_status_t;             /* low 5 bits: ifcs_reason (IFCS_STATUS_REASON) */
typedef uint32_t ifcs_handle_t;
typedef uint32_t ifcs_node_id_t;
typedef uint32_t ifcs_devport_t;
typedef uint32_t ifcs_attr_id_t;
typedef int ifcs_bool_t;
typedef uint8_t ifcs_mac_t[6];

enum {                                 /* ifcs_reason_e (DWARF + pyctypes) */
    IFCS_SUCCESS = 0, IFCS_UNINIT = 1, IFCS_BUSY = 2, IFCS_CONFIG = 3, IFCS_MEMORY = 4,
    IFCS_PARAM = 5, IFCS_EXIST = 6, IFCS_MULTI_EXIST = 7, IFCS_ENTRY_REPLACED = 8,
    IFCS_LENGTH_MISMATCH = 9, IFCS_NOTFOUND = 10, IFCS_INVAL = 11, IFCS_UNSUPPORTED = 12,
    IFCS_RESOURCE_FULL = 13, IFCS_TIMEOUT = 14, IFCS_DEVICE_ACCESS_ERROR = 15, IFCS_DRIVER_ERROR = 16,
};
#define IFCS_STATUS_REASON(rc) ((int)((unsigned)(rc) & 31u))

enum { IFCS_BOOL_FALSE = 0, IFCS_BOOL_TRUE = 1 };
enum { IFCS_BOOT_COLD = 0, IFCS_BOOT_WARM = 1 };
enum { IFCS_SHUTDOWN_TYPE_COLD = 0 };
enum { IFCS_IP_ADDR_FAMILY_IPV4 = 0, IFCS_IP_ADDR_FAMILY_IPV6 = 1 };
enum { IFCS_LOG_LEVEL_OFF = 0, IFCS_LOG_LEVEL_FATAL = 1, IFCS_LOG_LEVEL_ERROR = 2, IFCS_LOG_LEVEL_WARN = 3,
       IFCS_LOG_LEVEL_INFO = 4, IFCS_LOG_LEVEL_DEBUG = 5 };

/* ----------------------------------------------------------------- handles
 * 31..25 module, 24..21 object class, 20..0 value. */
#define IFCS_NULL_HANDLE 0u
#define IFCS_HANDLE_MODULE_SHIFT 25
#define IFCS_HANDLE_OBJECT_SHIFT 21
#define IFCS_HANDLE_VALUE_MAX 0x1fffffu
#define IFCS_HANDLE(mod, cls, val) \
    ((((uint32_t)(mod) & 127u) << IFCS_HANDLE_MODULE_SHIFT) | (((uint32_t)(cls) & 15u) << IFCS_HANDLE_OBJECT_SHIFT) | \
     ((uint32_t)(val) & IFCS_HANDLE_VALUE_MAX))
#define IFCS_HANDLE_VALUE(h) ((uint32_t)(h) & IFCS_HANDLE_VALUE_MAX)
enum { IFCS_MOD_HOSTIF = 12, IFCS_MOD_L2VNI = 18, IFCS_MOD_L3VNI = 19, IFCS_MOD_SYSPORT = 31 };
enum { IFCS_API_CLASS_HOSTIF_TRAP = 1, IFCS_API_CLASS_L2VNI = 1, IFCS_API_CLASS_L3VNI = 1, IFCS_API_CLASS_SYSPORT = 1 };
#define IFCS_HANDLE_L2VNI(v) IFCS_HANDLE(IFCS_MOD_L2VNI, IFCS_API_CLASS_L2VNI, (v))
#define IFCS_HANDLE_L3VNI(v) IFCS_HANDLE(IFCS_MOD_L3VNI, IFCS_API_CLASS_L3VNI, (v))
#define IFCS_HANDLE_SYSPORT(v) IFCS_HANDLE(IFCS_MOD_SYSPORT, IFCS_API_CLASS_SYSPORT, (v))

/* ------------------------------------------------------------------ lists */
typedef struct { uint32_t count; uint8_t* arr; } ifcs_u8_list_t;
typedef struct { uint32_t count; uint8_t* arr; } ifcs_string_t;     /* count: chars, no NUL */
typedef struct { uint32_t count; int8_t* arr; } ifcs_i8_list_t;
typedef struct { uint32_t count; int32_t* arr; } ifcs_i32_list_t;
typedef struct { uint32_t count; uint32_t* arr; } ifcs_u32_list_t;
typedef struct { uint32_t count; ifcs_handle_t* list; uint8_t dirty; } ifcs_handle_list_t;

/* ------------------------------------------------------------ IP and MAC */
typedef union { uint32_t ipv4; uint8_t ipv6[16]; } ifcs_ip_addr_t;   /* ipv4: host-order value */
typedef struct { int addr_family; ifcs_ip_addr_t addr; uint8_t dirty; } ifcs_ip_address_t;
typedef struct { int addr_family; ifcs_ip_addr_t addr; ifcs_ip_addr_t mask; uint8_t dirty; } ifcs_ip_prefix_t;

/* --------------------------------------------------------------- policies */
enum { IFCS_FWD_ACTION_FORWARD = 0, IFCS_FWD_ACTION_DROP = 1 };
enum { IFCS_COPY_TO_CPU_DISABLE = 0, IFCS_COPY_TO_CPU_ENABLE = 1 };
typedef struct { int fwd_action; ifcs_handle_t pp_drop_reason; uint8_t dirty; } ifcs_fwd_policy_t;
typedef struct { int ctc_action; ifcs_handle_t trap_handle; uint8_t dirty; } ifcs_ctc_policy_t;

/* --------------------------------------------------------------- attribute
 * The value union has 45 members in this version; only the ones we use are
 * named, the rest is padding to the verified size. The trailing bitmap says
 * which member is valid; the vendor CLI leaves it zero (it assigns the value
 * directly after zeroing the struct), and so do we. */
typedef union {
    uint64_t u64;
    uint32_t u32;
    int32_t int32;
    uint16_t u16;
    uint8_t u8;
    float flt;
    ifcs_u8_list_t u8_list;
    ifcs_string_t string;
    ifcs_i8_list_t i8_list;
    ifcs_i32_list_t i32_list;
    ifcs_u32_list_t u32_list;
    ifcs_devport_t devport;
    uint32_t cpu_queue;
    ifcs_bool_t data;
    ifcs_ip_address_t ip_addr;
    ifcs_ip_prefix_t ip_prefix;
    ifcs_mac_t mac;
    ifcs_handle_t handle;
    ifcs_handle_list_t handle_list;
    ifcs_ctc_policy_t ctc_policy;
    ifcs_fwd_policy_t fwd_policy;
    uint8_t _size[120];
} ifcs_attr_value_t;

typedef struct {
    ifcs_attr_id_t id;
    ifcs_attr_value_t value;
    uint64_t bitmap;
} ifcs_attr_t;

/* ------------------------------------------------------- key structs (CTYPES) */
enum { IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI = 0 };
typedef struct { ifcs_mac_t mac_addr; ifcs_handle_t l2vni; uint8_t dirty; } ifcs_l2_entry_key_mac_l2vni_t;
typedef union {
    ifcs_l2_entry_key_mac_l2vni_t mac_l2vni;
    uint8_t _size[88];                 /* largest member: ip_sg_l2vni (2 prefixes + handle) */
} ifcs_l2_entry_key_map_t;
typedef struct {
    uint32_t _base_key;
    int key_type;
    ifcs_l2_entry_key_map_t key;
    uint8_t dirty;
} ifcs_l2_entry_key_t;

enum { IFCS_ROUTE_ENTRY_KEY_TYPE_IP_DEST_L3VNI = 0 };
typedef struct { ifcs_ip_prefix_t ip_dest; ifcs_handle_t l3vni; uint8_t dirty; } ifcs_route_entry_key_ip_dest_l3vni_t;
typedef union {
    ifcs_route_entry_key_ip_dest_l3vni_t ip_dest_l3vni;
    uint8_t _size[48];
} ifcs_route_entry_key_map_t;
typedef struct {
    uint32_t _base_key;
    int key_type;
    ifcs_route_entry_key_map_t key;
    uint8_t dirty;
} ifcs_route_entry_key_t;

typedef struct { uint32_t max; uint32_t current; uint8_t dirty; } ifcs_usage_t;

/* ----------------------------------------------------------- events (CTYPES) */
enum { IFCS_DEVPORT_LINK_STATUS_DOWN = 0, IFCS_DEVPORT_LINK_STATUS_UP = 1 };
typedef struct {
    ifcs_node_id_t node_id;
    ifcs_devport_t devport_id;
    int port_state;                    /* IFCS_DEVPORT_LINK_STATUS_* */
    int fault_state;
    uint32_t rx_aligned_sts, rx_hi_ber_sts, rx_hi_ser_sts;
} ifcs_linkscan_event_info_t;

enum { IFCS_L2_EVENT_TYPE_LEARNED = 0, IFCS_L2_EVENT_TYPE_AGED = 1, IFCS_L2_EVENT_TYPE_MOVED = 2,
       IFCS_L2_EVENT_TYPE_FLUSHED = 3 };
typedef struct {
    int event_type;
    uint32_t count;
    ifcs_l2_entry_key_t l2_entry;
    uint32_t attr_count;
    ifcs_attr_t* attr_p;
} ifcs_l2_event_info_t;

typedef void (*ifcs_linkscan_event_notify_fn_t)(ifcs_node_id_t, void* user, ifcs_linkscan_event_info_t* info);
typedef void (*ifcs_l2_event_notify_fn_t)(ifcs_node_id_t, void* user, ifcs_l2_event_info_t* info);
typedef void (*ifcs_log_notify_t)(int level, void* user, const char* msg);
typedef void (*ifcs_l2_entry_user_cb_t)(ifcs_node_id_t, ifcs_l2_entry_key_t* key, uint32_t attr_count,
                                        ifcs_attr_t* attrs, void* user);

/* ------------------------------------------------------------ attribute ids */
enum {                                 /* ifcs_node_attr_id_e (DWARF + pyctypes) */
    IFCS_NODE_ATTR_ENABLE = 0,
    IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL = 55,          /* u32 */
    IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY = 58,       /* u32 */
    IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY = 59,       /* u32 */
    IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS = 62,            /* u32 */
    IFCS_NODE_ATTR_MASTER_LAST_TEMP = 74,                 /* flt, degrees C */
    IFCS_NODE_ATTR_REMOTE0_LAST_TEMP = 75, IFCS_NODE_ATTR_REMOTE1_LAST_TEMP = 76,
    IFCS_NODE_ATTR_REMOTE2_LAST_TEMP = 77, IFCS_NODE_ATTR_REMOTE3_LAST_TEMP = 78,
};

enum {                                 /* ifcs_devport_attr_id_e */
    IFCS_DEVPORT_ATTR_TYPE = 0, IFCS_DEVPORT_ATTR_SYSPORT = 7, IFCS_DEVPORT_ATTR_SPEED = 8,
    IFCS_DEVPORT_ATTR_LINK_STATUS = 9, IFCS_DEVPORT_ATTR_FEC_MODE = 14, IFCS_DEVPORT_ATTR_LOOPBACK = 15,
    IFCS_DEVPORT_ATTR_AUTO_NEG = 16, IFCS_DEVPORT_ATTR_LINK_TRAINING = 21, IFCS_DEVPORT_ATTR_ADMIN_STATE = 23,
    IFCS_DEVPORT_ATTR_TX_EQ_PRE1 = 43, IFCS_DEVPORT_ATTR_TX_EQ_PRE2 = 44, IFCS_DEVPORT_ATTR_TX_EQ_PRE3 = 45,
    IFCS_DEVPORT_ATTR_TX_EQ_ATTN = 46, IFCS_DEVPORT_ATTR_TX_EQ_POST = 47, IFCS_DEVPORT_ATTR_RX_EQ_FINE_TUNE = 49,
    IFCS_DEVPORT_ATTR_RX_CTLE_LF = 50, IFCS_DEVPORT_ATTR_RX_CTLE_HF = 51, IFCS_DEVPORT_ATTR_RX_CTLE_BW = 52,
    IFCS_DEVPORT_ATTR_RX_GAINSHAPE1 = 53, IFCS_DEVPORT_ATTR_RX_GAINSHAPE2 = 54, IFCS_DEVPORT_ATTR_RX_AGC_TARGET = 55,
    IFCS_DEVPORT_ATTR_RX_EYE_DISQUALIFY_THRESHOLD_25G = 56, IFCS_DEVPORT_ATTR_MAX_COUNT = 121,
};
enum { IFCS_DEVPORT_SPEED_10G = 1, IFCS_DEVPORT_SPEED_25G = 2, IFCS_DEVPORT_SPEED_40G = 3, IFCS_DEVPORT_SPEED_50G = 4,
       IFCS_DEVPORT_SPEED_100G = 5, IFCS_DEVPORT_SPEED_200G = 6, IFCS_DEVPORT_SPEED_400G = 7 };
enum { IFCS_DEVPORT_FEC_MODE_NONE = 0, IFCS_DEVPORT_FEC_MODE_KR = 1, IFCS_DEVPORT_FEC_MODE_KP = 2,
       IFCS_DEVPORT_FEC_MODE_FC = 3 };
enum { IFCS_DEVPORT_LOOPBACK_NONE = 0, IFCS_DEVPORT_LOOPBACK_PCS = 1, IFCS_DEVPORT_LOOPBACK_PMA = 2 };

enum {                                 /* ifcs_devport_stats_id_e (subset) */
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP = 2, IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST = 3,
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST = 4, IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST = 5,
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY = 37, IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL = 47,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP = 65, IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST = 66,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST = 67, IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST = 68,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY = 100, IFCS_DEVPORT_STATS_ID_TX_BYTES_ALL = 106,
    IFCS_DEVPORT_STATS_ID_MAX_COUNT = 111, IFCS_DEVPORT_STATS_ID_ALL = 112,
};

enum {                                 /* ifcs_sysport_attr_id_e (subset) */
    IFCS_SYSPORT_ATTR_LEARN_ENABLE = 28,                  /* data */
    IFCS_SYSPORT_ATTR_DEFAULT_CVID = 32,                  /* u16 */
    IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE = 42,/* data */
    IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID = 43,       /* u16 */
    IFCS_SYSPORT_ATTR_MTU = 51,                           /* u16, L2 MTU */
};

enum { IFCS_STP_PORT_ATTR_STP_STATE = 0 };
enum { IFCS_STP_PORT_STATE_BLOCKING = 0, IFCS_STP_PORT_STATE_FORWARDING = 3 };
enum { IFCS_MC_DEST_ATTR_LOCAL_DESTINATION = 1 };
enum { IFCS_L2VNI_ATTR_STP_INSTANCE = 0, IFCS_L2VNI_ATTR_FLOOD_GROUP = 6 };
enum { IFCS_L2_ENTRY_ATTR_ENTRY_TYPE = 0, IFCS_L2_ENTRY_ATTR_ENTRY_DEST = 1, IFCS_L2_ENTRY_ATTR_ENTRY_FWD_POLICY = 2 };
enum { IFCS_L2_ENTRY_TYPE_STATIC = 0, IFCS_L2_ENTRY_TYPE_DYNAMIC = 1 };

enum { IFCS_NETDEV_ATTR_INTF_NAME = 0, IFCS_NETDEV_ATTR_DEVPORTNUM = 1, IFCS_NETDEV_ATTR_MACADDR = 2,
       IFCS_NETDEV_ATTR_VLAN_TAG = 4 };
enum { IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_ORIGINAL = 0, IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_STRIP = 1,
       IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_KEEP = 2 };

enum { IFCS_INTF_ATTR_TYPE = 0, IFCS_INTF_ATTR_FORWARDING_INSTANCE = 3, IFCS_INTF_ATTR_TRANSPORT_INSTANCE = 4,
       IFCS_INTF_ATTR_TRANSPORT_MAC = 5, IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE = 18, IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE = 20,
       IFCS_INTF_ATTR_MTU = 24 };
enum { IFCS_INTF_TYPE_L3PORT = 0, IFCS_INTF_TYPE_SVI = 1 };
enum { IFCS_NEXTHOP_ATTR_INTF = 2, IFCS_NEXTHOP_ATTR_DEST_MAC = 3, IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION = 5 };
enum { IFCS_ROUTE_ENTRY_ATTR_NEXTHOP = 0, IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY = 1, IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY = 2 };
enum { IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM = 0, IFCS_HOSTIF_TRAP_ATTR_ENABLE = 1 };   /* QUEUE_NUM u32, ENABLE data */
enum { IFCS_HOSTIF_TRAP_STATS_ID_RX_PACKET_COUNT = 0, IFCS_HOSTIF_TRAP_STATS_ID_RX_BYTE_COUNT = 1 };
/* ifcs_hostif_trap_e: system (drop/exception) traps; handle = IFCS_HANDLE(HOSTIF, HOSTIF_TRAP, reason) */
enum { IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL = 17, IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL = 23 };
#define IFCS_HANDLE_HOSTIF_TRAP(v) IFCS_HANDLE(IFCS_MOD_HOSTIF, IFCS_API_CLASS_HOSTIF_TRAP, (v))

/* ------------------------------------------------------------------- ACL
 * One ingress table (match + action profile), one ACL per inno block
 * (scope PER_IB), entries (ACE) = a match set + an action set. */
enum {                                 /* ifcs_acl_match_type_e (subset) */
    IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS = 17, IFCS_ACL_MATCH_TYPE_ETHER_TYPE = 18,
    IFCS_ACL_MATCH_TYPE_L4_PROTOCOL = 56, IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT = 59,
    IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT = 60, IFCS_ACL_MATCH_TYPE_L4_BYTE1 = 61, IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP = 98,
};
enum {                                 /* ifcs_acl_action_type_e (subset) */
    IFCS_ACL_ACTION_TYPE_METER = 0, IFCS_ACL_ACTION_TYPE_ACL_COUNTER = 1, IFCS_ACL_ACTION_TYPE_DROP = 2,
    IFCS_ACL_ACTION_TYPE_COPY_TO_CPU = 4, IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE = 5,
};
enum { IFCS_ACL_DIRECTION_INGRESS = 0 };
enum { IFCS_ACL_SCOPE_NODE = 0, IFCS_ACL_SCOPE_PER_IB = 1 };
enum { IFCS_ACL_TABLE_TYPE_CONTROL = 1 };
enum { IFCS_ACL_ACTION_DISABLE = 0, IFCS_ACL_ACTION_ENABLE = 1 };   /* ifcs_acl_action_t: per colour */
enum { IFCS_ACL_METER_TYPE_BYTE = 0 };
enum { IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2697 = 1, IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2698 = 2 };
enum { IFCS_ACL_COUNTER_MODE_COLOR_AWARE = 1 };
enum { IFCS_ACL_COUNTER_STATS_ID_PACKETS = 0, IFCS_ACL_COUNTER_STATS_ID_BYTES = 1, IFCS_ACL_COUNTER_STATS_ID_GREEN_PACKETS = 2,
       IFCS_ACL_COUNTER_STATS_ID_YELLOW_PACKETS = 4, IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS = 6 };
enum { IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES = 0, IFCS_ACL_MATCH_PROFILE_ATTR_DIRECTION = 9,
       IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH = 11 };
enum { IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES = 0, IFCS_ACL_ACTION_PROFILE_ATTR_DIRECTION = 2 };
enum { IFCS_ACL_TABLE_ATTR_MATCH_PROFILE = 0, IFCS_ACL_TABLE_ATTR_ACTION_PROFILE = 1, IFCS_ACL_TABLE_ATTR_DIRECTION = 4,
       IFCS_ACL_TABLE_ATTR_SCOPE = 5, IFCS_ACL_TABLE_ATTR_PRIORITY = 6, IFCS_ACL_TABLE_ATTR_SIZE = 7,
       IFCS_ACL_TABLE_ATTR_MAX_METERS = 12, IFCS_ACL_TABLE_ATTR_MAX_COUNTERS = 14, IFCS_ACL_TABLE_ATTR_DEFAULT_STATS = 16,
       IFCS_ACL_TABLE_ATTR_TYPE = 17 };
enum { IFCS_ACL_ATTR_SCOPE = 0, IFCS_ACL_ATTR_IB = 1 };
enum { IFCS_ACL_MATCH_ATTR_MATCH_PROFILE = 0, IFCS_ACL_MATCH_ATTR_TYPE = 1, IFCS_ACL_MATCH_ATTR_VALUE = 2,
       IFCS_ACL_MATCH_ATTR_MASK = 3, IFCS_ACL_MATCH_ATTR_OBJECT = 4 };
enum { IFCS_ACL_ACTION_ATTR_ACTION_PROFILE = 0, IFCS_ACL_ACTION_ATTR_TYPE = 1, IFCS_ACL_ACTION_ATTR_VALUE = 2,
       IFCS_ACL_ACTION_ATTR_OBJECT = 3 };
enum { IFCS_ACL_METER_ATTR_ACL_TABLE = 0, IFCS_ACL_METER_ATTR_SCOPE = 1, IFCS_ACL_METER_ATTR_IB = 2,
       IFCS_ACL_METER_ATTR_TYPE = 3, IFCS_ACL_METER_ATTR_MODE = 4, IFCS_ACL_METER_ATTR_COLOR_AWARE = 5,
       IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE = 6, IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE = 7,
       IFCS_ACL_METER_ATTR_EXCESS_BURST_SIZE = 8, IFCS_ACL_METER_ATTR_EXCESS_INFORMATION_RATE = 9 };
enum { IFCS_ACL_COUNTER_ATTR_ACL_TABLE = 0, IFCS_ACL_COUNTER_ATTR_SCOPE = 1, IFCS_ACL_COUNTER_ATTR_IB = 2,
       IFCS_ACL_COUNTER_ATTR_MODE = 3 };
enum { IFCS_ACE_ATTR_SCOPE = 0, IFCS_ACE_ATTR_IB = 1, IFCS_ACE_ATTR_PRIORITY = 2, IFCS_ACE_ATTR_ENABLE = 3,
       IFCS_ACE_ATTR_MATCH_SET = 4, IFCS_ACE_ATTR_ACTION_SET = 5 };
/* Members of ifcs_acl_action_value_u (CTYPES; identical in 0.16.2): the
 * action VALUE attribute holds one of these at value offset 0. Colour
 * fields are ifcs_acl_action_t; the trailing byte holds the dirty bits. */
typedef struct { uint32_t hostif, green, yellow, red; uint8_t dirty; } ifcs_acl_ctc_action_t;          /* dirty: 1 hostif, 2 green, 4 yellow, 8 red */
typedef struct { uint32_t green, yellow, red; uint8_t dirty; } ifcs_acl_ctc_override_action_t;         /* dirty: 1 green, 2 yellow, 4 red */
typedef struct { uint32_t green, yellow, red; ifcs_handle_t pp_drop_reason; uint8_t dirty; } ifcs_acl_drop_action_t;   /* 1 2 4 8 */
typedef int ifcs_acl_counter_stats_id_t;
typedef int ifcs_hostif_trap_stats_id_t;
enum { IFCS_ECMP_MEMBER_ATTR_WEIGHT = 0 };

/* ------------------------------------------------------- function pointers */
#define IFCS_FN(ret, name, args) typedef ret(*name##_fn) args;
IFCS_FN(ifcs_status_t, ifcs_init, (const char* cfg, const char* log_name, int boot, uint32_t* num_nodes))
IFCS_FN(ifcs_status_t, ifcs_deinit, (int shutdown_type))
IFCS_FN(ifcs_status_t, ifcs_node_create, (ifcs_node_id_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_node_attr_set, (ifcs_node_id_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_node_attr_get, (ifcs_node_id_t, uint32_t, ifcs_attr_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_attr_t_init, (ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_attr_t_id_set, (ifcs_attr_t*, uint32_t))
IFCS_FN(ifcs_status_t, ifcs_attr_t_value_data_set, (ifcs_attr_t*, ifcs_bool_t))
IFCS_FN(const char*, ifcs_status_to_string, (ifcs_status_t))
IFCS_FN(ifcs_status_t, ifcs_log_function_register, (ifcs_log_notify_t, void*))
IFCS_FN(ifcs_status_t, ifcs_devport_attr_set, (ifcs_node_id_t, ifcs_devport_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_devport_attr_get, (ifcs_node_id_t, ifcs_devport_t, uint32_t, ifcs_attr_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_devport_stats_get, (ifcs_node_id_t, ifcs_devport_t, uint32_t, int*, uint64_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_devport_stats_clear, (ifcs_node_id_t, ifcs_devport_t, uint32_t, int*))
IFCS_FN(ifcs_status_t, ifcs_linkscan_event_register_notify, (ifcs_node_id_t, void*, ifcs_linkscan_event_notify_fn_t))
IFCS_FN(ifcs_status_t, ifcs_linkscan_event_unregister_notify, (ifcs_node_id_t))
IFCS_FN(ifcs_status_t, ifcs_sysport_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_stp_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_stp_port_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_mc_dest_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_mc_dest_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_mdg_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_mdg_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_mdg_mc_dest_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_mdg_mc_dest_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_l2vni_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_l2vni_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_l2vni_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_l2vni_member_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_l2vni_member_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_key_t_init, (ifcs_l2_entry_key_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_key_t_key_type_set, (ifcs_l2_entry_key_t*, int))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_key_t_mac_l2vni_set, (ifcs_l2_entry_key_t*, ifcs_l2_entry_key_mac_l2vni_t))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set, (ifcs_l2_entry_key_mac_l2vni_t*, ifcs_mac_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_key_mac_l2vni_t_l2vni_set, (ifcs_l2_entry_key_mac_l2vni_t*, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_create, (ifcs_node_id_t, ifcs_l2_entry_key_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_delete, (ifcs_node_id_t, ifcs_l2_entry_key_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_get_all, (ifcs_node_id_t, ifcs_l2_entry_key_t*, uint32_t, ifcs_attr_t*,
                                               ifcs_l2_entry_user_cb_t, void*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_attr_get, (ifcs_node_id_t, ifcs_l2_entry_key_t*, uint32_t, ifcs_attr_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_entry_usage_get, (ifcs_node_id_t, ifcs_l2_entry_key_t*, uint32_t, ifcs_attr_t*, ifcs_usage_t*))
IFCS_FN(ifcs_status_t, ifcs_l2_event_register_notify, (ifcs_node_id_t, void*, ifcs_l2_event_notify_fn_t))
IFCS_FN(ifcs_status_t, ifcs_l2_event_unregister_notify, (ifcs_node_id_t))
IFCS_FN(ifcs_status_t, ifcs_netdev_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_netdev_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_netdev_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_l3vni_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_intf_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_intf_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_intf_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_nexthop_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_nexthop_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_nexthop_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_nexthop_usage_get, (ifcs_node_id_t, uint32_t, ifcs_attr_t*, ifcs_usage_t*))
IFCS_FN(ifcs_status_t, ifcs_route_entry_key_t_init, (ifcs_route_entry_key_t*))
IFCS_FN(ifcs_status_t, ifcs_route_entry_key_t_key_type_set, (ifcs_route_entry_key_t*, int))
IFCS_FN(ifcs_status_t, ifcs_route_entry_key_t_ip_dest_l3vni_set, (ifcs_route_entry_key_t*, ifcs_route_entry_key_ip_dest_l3vni_t))
IFCS_FN(ifcs_status_t, ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set, (ifcs_route_entry_key_ip_dest_l3vni_t*, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set, (ifcs_route_entry_key_ip_dest_l3vni_t*, ifcs_ip_prefix_t*))
IFCS_FN(ifcs_status_t, ifcs_ip_prefix_t_init, (ifcs_ip_prefix_t*))
IFCS_FN(ifcs_status_t, ifcs_ip_prefix_t_addr_family_set, (ifcs_ip_prefix_t*, int))
IFCS_FN(ifcs_status_t, ifcs_ip_prefix_t_addr_set, (ifcs_ip_prefix_t*, ifcs_ip_addr_t*))
IFCS_FN(ifcs_status_t, ifcs_ip_prefix_t_mask_set, (ifcs_ip_prefix_t*, ifcs_ip_addr_t*))
IFCS_FN(ifcs_status_t, ifcs_route_entry_create, (ifcs_node_id_t, ifcs_route_entry_key_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_route_entry_delete, (ifcs_node_id_t, ifcs_route_entry_key_t*))
IFCS_FN(ifcs_status_t, ifcs_route_entry_attr_set, (ifcs_node_id_t, ifcs_route_entry_key_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_hostif_trap_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_hostif_trap_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_hostif_trap_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_hostif_trap_stats_get, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_hostif_trap_stats_id_t*, uint64_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_match_profile_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_match_profile_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_action_profile_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_action_profile_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_table_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_table_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_table_acl_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_table_acl_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_match_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_match_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_action_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_action_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_meter_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_meter_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_meter_attr_set, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_counter_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_counter_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_counter_stats_get, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_acl_counter_stats_id_t*, uint64_t*, uint32_t*))
IFCS_FN(ifcs_status_t, ifcs_ace_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_ace_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_acl_ace_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_acl_ace_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_ecmp_create, (ifcs_node_id_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_ecmp_delete, (ifcs_node_id_t, ifcs_handle_t))
IFCS_FN(ifcs_status_t, ifcs_ecmp_member_add, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*, uint32_t, ifcs_attr_t*))
IFCS_FN(ifcs_status_t, ifcs_ecmp_member_remove, (ifcs_node_id_t, ifcs_handle_t, uint32_t, ifcs_handle_t*))
IFCS_FN(ifcs_status_t, ifcs_ecmp_usage_get, (ifcs_node_id_t, uint32_t, ifcs_attr_t*, ifcs_usage_t*))
#undef IFCS_FN

/* Every entry point above, in one X-macro list (dlsym table, mock, tests). */
#define IFCS_SYMBOLS(X) \
    X(ifcs_init) X(ifcs_deinit) X(ifcs_node_create) X(ifcs_node_attr_set) X(ifcs_node_attr_get) \
    X(ifcs_attr_t_init) X(ifcs_attr_t_id_set) X(ifcs_attr_t_value_data_set) X(ifcs_status_to_string) \
    X(ifcs_log_function_register) X(ifcs_devport_attr_set) X(ifcs_devport_attr_get) X(ifcs_devport_stats_get) \
    X(ifcs_devport_stats_clear) X(ifcs_linkscan_event_register_notify) X(ifcs_linkscan_event_unregister_notify) \
    X(ifcs_sysport_attr_set) X(ifcs_stp_create) X(ifcs_stp_port_add) X(ifcs_mc_dest_create) X(ifcs_mc_dest_delete) \
    X(ifcs_mdg_create) X(ifcs_mdg_delete) X(ifcs_mdg_mc_dest_add) X(ifcs_mdg_mc_dest_remove) X(ifcs_l2vni_create) \
    X(ifcs_l2vni_delete) X(ifcs_l2vni_attr_set) X(ifcs_l2vni_member_add) X(ifcs_l2vni_member_remove) \
    X(ifcs_l2_entry_key_t_init) X(ifcs_l2_entry_key_t_key_type_set) X(ifcs_l2_entry_key_t_mac_l2vni_set) \
    X(ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set) X(ifcs_l2_entry_key_mac_l2vni_t_l2vni_set) X(ifcs_l2_entry_create) \
    X(ifcs_l2_entry_delete) X(ifcs_l2_entry_get_all) X(ifcs_l2_entry_attr_get) X(ifcs_l2_entry_usage_get) \
    X(ifcs_l2_event_register_notify) X(ifcs_l2_event_unregister_notify) X(ifcs_netdev_create) X(ifcs_netdev_delete) \
    X(ifcs_netdev_attr_set) X(ifcs_l3vni_create) X(ifcs_intf_create) X(ifcs_intf_delete) X(ifcs_intf_attr_set) \
    X(ifcs_nexthop_create) X(ifcs_nexthop_delete) X(ifcs_nexthop_attr_set) X(ifcs_nexthop_usage_get) \
    X(ifcs_route_entry_key_t_init) X(ifcs_route_entry_key_t_key_type_set) X(ifcs_route_entry_key_t_ip_dest_l3vni_set) \
    X(ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set) X(ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set) \
    X(ifcs_ip_prefix_t_init) X(ifcs_ip_prefix_t_addr_family_set) X(ifcs_ip_prefix_t_addr_set) X(ifcs_ip_prefix_t_mask_set) \
    X(ifcs_route_entry_create) X(ifcs_route_entry_delete) X(ifcs_route_entry_attr_set) X(ifcs_hostif_trap_create) \
    X(ifcs_hostif_trap_delete) X(ifcs_hostif_trap_attr_set) X(ifcs_hostif_trap_stats_get) \
    X(ifcs_acl_match_profile_create) X(ifcs_acl_match_profile_delete) X(ifcs_acl_action_profile_create) \
    X(ifcs_acl_action_profile_delete) X(ifcs_acl_table_create) X(ifcs_acl_table_delete) X(ifcs_acl_table_acl_add) \
    X(ifcs_acl_table_acl_remove) X(ifcs_acl_create) X(ifcs_acl_delete) X(ifcs_acl_match_create) X(ifcs_acl_match_delete) \
    X(ifcs_acl_action_create) X(ifcs_acl_action_delete) X(ifcs_acl_meter_create) X(ifcs_acl_meter_delete) \
    X(ifcs_acl_meter_attr_set) X(ifcs_acl_counter_create) X(ifcs_acl_counter_delete) X(ifcs_acl_counter_stats_get) \
    X(ifcs_ace_create) X(ifcs_ace_delete) X(ifcs_acl_ace_add) X(ifcs_acl_ace_remove) \
    X(ifcs_ecmp_create) X(ifcs_ecmp_delete) X(ifcs_ecmp_member_add) X(ifcs_ecmp_member_remove) X(ifcs_ecmp_usage_get)

#ifdef __cplusplus
}

/* -------------------------------------------------------- layout checks */
/* DWARF (innovium.user): */
static_assert(sizeof(ifcs_attr_value_t) == 120, "ifcs_attr_value_u");
static_assert(sizeof(ifcs_attr_t) == 136, "ifcs_attr_s");
static_assert(offsetof(ifcs_attr_t, value) == 8, "ifcs_attr_s.value");
static_assert(offsetof(ifcs_attr_t, bitmap) == 128, "ifcs_attr_s.bitmap");
static_assert(sizeof(ifcs_ip_address_t) == 24 && offsetof(ifcs_ip_address_t, addr) == 4, "ifcs_ip_address_s");
static_assert(sizeof(ifcs_ip_prefix_t) == 40 && offsetof(ifcs_ip_prefix_t, mask) == 20 &&
              offsetof(ifcs_ip_prefix_t, dirty) == 36, "ifcs_ip_prefix_s");
static_assert(sizeof(ifcs_handle_list_t) == 24 && offsetof(ifcs_handle_list_t, list) == 8, "ifcs_handle_list_s");
static_assert(sizeof(ifcs_u32_list_t) == 16 && offsetof(ifcs_u32_list_t, arr) == 8, "ifcs_u32_list_s");
static_assert(sizeof(ifcs_ctc_policy_t) == 12 && sizeof(ifcs_fwd_policy_t) == 12, "policies");
/* CTYPES (computed from pyctypes field lists; same sizes as 7.0.1 DWARF): */
static_assert(sizeof(ifcs_l2_entry_key_mac_l2vni_t) == 16 && offsetof(ifcs_l2_entry_key_mac_l2vni_t, l2vni) == 8, "mac_l2vni");
static_assert(sizeof(ifcs_l2_entry_key_t) == 100 && offsetof(ifcs_l2_entry_key_t, key) == 8, "ifcs_l2_entry_key_s");
static_assert(sizeof(ifcs_route_entry_key_ip_dest_l3vni_t) == 48 && offsetof(ifcs_route_entry_key_ip_dest_l3vni_t, l3vni) == 40,
              "ip_dest_l3vni");
static_assert(sizeof(ifcs_route_entry_key_t) == 60 && offsetof(ifcs_route_entry_key_t, key) == 8, "ifcs_route_entry_key_s");
static_assert(sizeof(ifcs_linkscan_event_info_t) == 28, "ifcs_linkscan_event_info_s (0.12.41: 7 fields)");
static_assert(sizeof(ifcs_l2_event_info_t) == 120 && offsetof(ifcs_l2_event_info_t, attr_count) == 108 &&
              offsetof(ifcs_l2_event_info_t, attr_p) == 112, "ifcs_l2_event_info_s");
static_assert(sizeof(ifcs_usage_t) == 12, "ifcs_usage_s");
static_assert(sizeof(ifcs_acl_ctc_action_t) == 20 && sizeof(ifcs_acl_ctc_override_action_t) == 16 &&
              sizeof(ifcs_acl_drop_action_t) == 20, "acl action values (CTYPES)");
#endif

#endif

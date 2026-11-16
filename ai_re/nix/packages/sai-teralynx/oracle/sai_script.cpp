// SPDX-License-Identifier: Apache-2.0
// sai-script: run a JSON list of SAI calls against any libsai (generic SAI,
// one source for our SAI 1.19 build and isai's 1.16 headers; only
// attributes and values both have are named). For the oracle (README.md):
// before and after every step it appends a marker line to IFCS_TRACE_FILE,
// so the libifcs trace can be cut per SAI call.
//   sai-script PROFILE SCRIPT.json
// PROFILE: key=value lines (the SAI profile). SCRIPT: see oracle/README.md.
extern "C" {
#include <sai.h>
}

#include <nlohmann/json.hpp>

#include <arpa/inet.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <string>
#include <thread>
#include <vector>

using json = nlohmann::json;

static std::map<std::string, std::string> g_profile;
static std::map<std::string, std::string>::iterator g_it;
static const char* prof_get(sai_switch_profile_id_t, const char* k)
{
    auto it = g_profile.find(k);
    return it == g_profile.end() ? nullptr : it->second.c_str();
}
static int prof_next(sai_switch_profile_id_t, const char** k, const char** v)
{
    if (!k || !v) { g_it = g_profile.begin(); return 0; }
    if (g_it == g_profile.end()) return -1;
    *k = g_it->first.c_str();
    *v = g_it->second.c_str();
    ++g_it;
    return 0;
}
static void on_port_state(uint32_t n, const sai_port_oper_status_notification_t* d)
{
    for (uint32_t i = 0; i < n; i++)
        printf("event port 0x%lx oper %d\n", (unsigned long)d[i].port_id, (int)d[i].port_state);
    fflush(stdout);
}
static void on_shutdown(sai_object_id_t) { printf("event shutdown request\n"); fflush(stdout); }
static void on_fdb(uint32_t n, const sai_fdb_event_notification_data_t* d)
{
    for (uint32_t i = 0; i < n; i++) {
        sai_object_id_t bp = SAI_NULL_OBJECT_ID;
        for (uint32_t j = 0; j < d[i].attr_count; j++)
            if (d[i].attr[j].id == SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID) bp = d[i].attr[j].value.oid;
        const uint8_t* m = d[i].fdb_entry.mac_address;
        printf("event fdb %d mac %02x:%02x:%02x:%02x:%02x:%02x bv 0x%llx bp 0x%llx\n", (int)d[i].event_type, m[0], m[1],
               m[2], m[3], m[4], m[5], (unsigned long long)d[i].fdb_entry.bv_id, (unsigned long long)bp);
    }
    fflush(stdout);
}

static int g_mark = -1;
static void mark(const std::string& s)
{
    if (g_mark < 0) return;
    std::string l = "M " + s + "\n";
    (void)!write(g_mark, l.data(), l.size());
}
static double now_ms()
{
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

// ------------------------------------------------------------ name tables
struct Name { const char* n; int64_t v; };
#define N(prefix, x) {#x, prefix##x}
static const Name k_types[] = {
    N(SAI_OBJECT_TYPE_, SWITCH), N(SAI_OBJECT_TYPE_, PORT), N(SAI_OBJECT_TYPE_, HOSTIF), N(SAI_OBJECT_TYPE_, HOSTIF_TRAP_GROUP),
    N(SAI_OBJECT_TYPE_, HOSTIF_TRAP), N(SAI_OBJECT_TYPE_, HOSTIF_TABLE_ENTRY), N(SAI_OBJECT_TYPE_, POLICER),
    N(SAI_OBJECT_TYPE_, VLAN), N(SAI_OBJECT_TYPE_, VLAN_MEMBER), N(SAI_OBJECT_TYPE_, ROUTER_INTERFACE),
    N(SAI_OBJECT_TYPE_, ACL_TABLE), N(SAI_OBJECT_TYPE_, ACL_ENTRY), N(SAI_OBJECT_TYPE_, ACL_COUNTER),
    N(SAI_OBJECT_TYPE_, ACL_RANGE), N(SAI_OBJECT_TYPE_, ACL_TABLE_GROUP), N(SAI_OBJECT_TYPE_, ACL_TABLE_GROUP_MEMBER),
    N(SAI_OBJECT_TYPE_, NEXT_HOP), N(SAI_OBJECT_TYPE_, VIRTUAL_ROUTER), N(SAI_OBJECT_TYPE_, BRIDGE_PORT),
    N(SAI_OBJECT_TYPE_, NEXT_HOP_GROUP), N(SAI_OBJECT_TYPE_, NEXT_HOP_GROUP_MEMBER),
    N(SAI_OBJECT_TYPE_, BRIDGE), N(SAI_OBJECT_TYPE_, LAG), N(SAI_OBJECT_TYPE_, LAG_MEMBER), N(SAI_OBJECT_TYPE_, HASH),
};
static const Name k_attrs_switch[] = {
    N(SAI_SWITCH_ATTR_, INIT_SWITCH), N(SAI_SWITCH_ATTR_, PORT_LIST), N(SAI_SWITCH_ATTR_, PORT_NUMBER),
    N(SAI_SWITCH_ATTR_, CPU_PORT), N(SAI_SWITCH_ATTR_, DEFAULT_VIRTUAL_ROUTER_ID), N(SAI_SWITCH_ATTR_, DEFAULT_1Q_BRIDGE_ID),
    N(SAI_SWITCH_ATTR_, DEFAULT_TRAP_GROUP), N(SAI_SWITCH_ATTR_, SRC_MAC_ADDRESS), N(SAI_SWITCH_ATTR_, FDB_AGING_TIME),
    N(SAI_SWITCH_ATTR_, DEFAULT_VLAN_ID), N(SAI_SWITCH_ATTR_, MAX_TEMP), N(SAI_SWITCH_ATTR_, PORT_STATE_CHANGE_NOTIFY),
    N(SAI_SWITCH_ATTR_, SWITCH_SHUTDOWN_REQUEST_NOTIFY), N(SAI_SWITCH_ATTR_, FDB_EVENT_NOTIFY),
    N(SAI_SWITCH_ATTR_, AVAILABLE_IPV4_ROUTE_ENTRY), N(SAI_SWITCH_ATTR_, AVAILABLE_IPV6_ROUTE_ENTRY),
    N(SAI_SWITCH_ATTR_, AVAILABLE_IPV4_NEXTHOP_ENTRY), N(SAI_SWITCH_ATTR_, AVAILABLE_IPV4_NEIGHBOR_ENTRY),
    N(SAI_SWITCH_ATTR_, AVAILABLE_IPV6_NEIGHBOR_ENTRY), N(SAI_SWITCH_ATTR_, AVAILABLE_NEXT_HOP_GROUP_ENTRY),
    N(SAI_SWITCH_ATTR_, LAG_HASH), N(SAI_SWITCH_ATTR_, ECMP_HASH), N(SAI_SWITCH_ATTR_, LAG_DEFAULT_HASH_SEED),
    N(SAI_SWITCH_ATTR_, ECMP_DEFAULT_HASH_SEED), N(SAI_SWITCH_ATTR_, LAG_DEFAULT_HASH_ALGORITHM),
    N(SAI_SWITCH_ATTR_, ECMP_DEFAULT_HASH_ALGORITHM),
};
static const Name k_attrs_port[] = {
    N(SAI_PORT_ATTR_, ADMIN_STATE), N(SAI_PORT_ATTR_, OPER_STATUS), N(SAI_PORT_ATTR_, SPEED), N(SAI_PORT_ATTR_, MTU),
    N(SAI_PORT_ATTR_, FEC_MODE), N(SAI_PORT_ATTR_, HW_LANE_LIST), N(SAI_PORT_ATTR_, LINK_TRAINING_ENABLE),
    N(SAI_PORT_ATTR_, INGRESS_ACL), N(SAI_PORT_ATTR_, EGRESS_ACL),
    N(SAI_PORT_ATTR_, PORT_VLAN_ID), N(SAI_PORT_ATTR_, AUTO_NEG_MODE),
};
static const Name k_attrs_hostif[] = {
    N(SAI_HOSTIF_ATTR_, TYPE), N(SAI_HOSTIF_ATTR_, OBJ_ID), N(SAI_HOSTIF_ATTR_, NAME), N(SAI_HOSTIF_ATTR_, OPER_STATUS),
    N(SAI_HOSTIF_ATTR_, QUEUE), N(SAI_HOSTIF_ATTR_, VLAN_TAG), N(SAI_HOSTIF_ATTR_, GENETLINK_MCGRP_NAME),
};
static const Name k_attrs_trap_group[] = {
    N(SAI_HOSTIF_TRAP_GROUP_ATTR_, ADMIN_STATE), N(SAI_HOSTIF_TRAP_GROUP_ATTR_, QUEUE), N(SAI_HOSTIF_TRAP_GROUP_ATTR_, POLICER),
};
static const Name k_attrs_trap[] = {
    N(SAI_HOSTIF_TRAP_ATTR_, TRAP_TYPE), N(SAI_HOSTIF_TRAP_ATTR_, PACKET_ACTION), N(SAI_HOSTIF_TRAP_ATTR_, TRAP_PRIORITY),
    N(SAI_HOSTIF_TRAP_ATTR_, TRAP_GROUP),
};
static const Name k_attrs_table_entry[] = {
    N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, TYPE), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, OBJ_ID), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, TRAP_ID),
    N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, CHANNEL_TYPE), N(SAI_HOSTIF_TABLE_ENTRY_ATTR_, HOST_IF),
};
static const Name k_attrs_policer[] = {
    N(SAI_POLICER_ATTR_, METER_TYPE), N(SAI_POLICER_ATTR_, MODE), N(SAI_POLICER_ATTR_, COLOR_SOURCE), N(SAI_POLICER_ATTR_, CBS),
    N(SAI_POLICER_ATTR_, CIR), N(SAI_POLICER_ATTR_, PBS), N(SAI_POLICER_ATTR_, PIR), N(SAI_POLICER_ATTR_, GREEN_PACKET_ACTION),
    N(SAI_POLICER_ATTR_, YELLOW_PACKET_ACTION), N(SAI_POLICER_ATTR_, RED_PACKET_ACTION),
};
static const Name k_attrs_vlan[] = {N(SAI_VLAN_ATTR_, VLAN_ID), N(SAI_VLAN_ATTR_, MEMBER_LIST)};
static const Name k_attrs_vlan_member[] = {
    N(SAI_VLAN_MEMBER_ATTR_, VLAN_ID), N(SAI_VLAN_MEMBER_ATTR_, BRIDGE_PORT_ID), N(SAI_VLAN_MEMBER_ATTR_, VLAN_TAGGING_MODE),
};
static const Name k_attrs_rif[] = {
    N(SAI_ROUTER_INTERFACE_ATTR_, VIRTUAL_ROUTER_ID), N(SAI_ROUTER_INTERFACE_ATTR_, TYPE), N(SAI_ROUTER_INTERFACE_ATTR_, PORT_ID),
    N(SAI_ROUTER_INTERFACE_ATTR_, VLAN_ID), N(SAI_ROUTER_INTERFACE_ATTR_, SRC_MAC_ADDRESS), N(SAI_ROUTER_INTERFACE_ATTR_, MTU),
    N(SAI_ROUTER_INTERFACE_ATTR_, ADMIN_V4_STATE), N(SAI_ROUTER_INTERFACE_ATTR_, ADMIN_V6_STATE),
    N(SAI_ROUTER_INTERFACE_ATTR_, NEIGHBOR_MISS_PACKET_ACTION),
};
static const Name k_attrs_route[] = {N(SAI_ROUTE_ENTRY_ATTR_, PACKET_ACTION), N(SAI_ROUTE_ENTRY_ATTR_, NEXT_HOP_ID)};
static const Name k_attrs_neighbor[] = {N(SAI_NEIGHBOR_ENTRY_ATTR_, DST_MAC_ADDRESS), N(SAI_NEIGHBOR_ENTRY_ATTR_, PACKET_ACTION),
                                        N(SAI_NEIGHBOR_ENTRY_ATTR_, NO_HOST_ROUTE)};
static const Name k_attrs_vr[] = {
    N(SAI_VIRTUAL_ROUTER_ATTR_, SRC_MAC_ADDRESS), N(SAI_VIRTUAL_ROUTER_ATTR_, ADMIN_V4_STATE),
    N(SAI_VIRTUAL_ROUTER_ATTR_, ADMIN_V6_STATE), N(SAI_VIRTUAL_ROUTER_ATTR_, VIOLATION_TTL1_PACKET_ACTION),
};
static const Name k_attrs_nhg[] = {
    N(SAI_NEXT_HOP_GROUP_ATTR_, TYPE), N(SAI_NEXT_HOP_GROUP_ATTR_, NEXT_HOP_COUNT), N(SAI_NEXT_HOP_GROUP_ATTR_, NEXT_HOP_MEMBER_LIST),
};
static const Name k_attrs_nhgm[] = {
    N(SAI_NEXT_HOP_GROUP_MEMBER_ATTR_, NEXT_HOP_GROUP_ID), N(SAI_NEXT_HOP_GROUP_MEMBER_ATTR_, NEXT_HOP_ID),
    N(SAI_NEXT_HOP_GROUP_MEMBER_ATTR_, WEIGHT),
};
static const Name k_rif_stats[] = {
    N(SAI_ROUTER_INTERFACE_STAT_, IN_OCTETS), N(SAI_ROUTER_INTERFACE_STAT_, IN_PACKETS), N(SAI_ROUTER_INTERFACE_STAT_, OUT_OCTETS),
    N(SAI_ROUTER_INTERFACE_STAT_, OUT_PACKETS), N(SAI_ROUTER_INTERFACE_STAT_, IN_ERROR_OCTETS),
    N(SAI_ROUTER_INTERFACE_STAT_, IN_ERROR_PACKETS), N(SAI_ROUTER_INTERFACE_STAT_, OUT_ERROR_OCTETS),
    N(SAI_ROUTER_INTERFACE_STAT_, OUT_ERROR_PACKETS),
};
static const Name k_attrs_nexthop[] = {
    N(SAI_NEXT_HOP_ATTR_, TYPE), N(SAI_NEXT_HOP_ATTR_, IP), N(SAI_NEXT_HOP_ATTR_, ROUTER_INTERFACE_ID),
};
static const Name k_attrs_bridge_port[] = {
    N(SAI_BRIDGE_PORT_ATTR_, PORT_ID), N(SAI_BRIDGE_PORT_ATTR_, TYPE), N(SAI_BRIDGE_PORT_ATTR_, ADMIN_STATE),
    N(SAI_BRIDGE_PORT_ATTR_, FDB_LEARNING_MODE), N(SAI_BRIDGE_PORT_ATTR_, BRIDGE_ID),
};
static const Name k_attrs_bridge[] = {N(SAI_BRIDGE_ATTR_, PORT_LIST), N(SAI_BRIDGE_ATTR_, TYPE)};
static const Name k_attrs_lag[] = {N(SAI_LAG_ATTR_, PORT_LIST), N(SAI_LAG_ATTR_, PORT_VLAN_ID)};
static const Name k_attrs_lag_member[] = {
    N(SAI_LAG_MEMBER_ATTR_, LAG_ID), N(SAI_LAG_MEMBER_ATTR_, PORT_ID), N(SAI_LAG_MEMBER_ATTR_, EGRESS_DISABLE),
    N(SAI_LAG_MEMBER_ATTR_, INGRESS_DISABLE),
};
static const Name k_attrs_hash[] = {N(SAI_HASH_ATTR_, NATIVE_HASH_FIELD_LIST)};
static const Name k_attrs_fdb[] = {
    N(SAI_FDB_ENTRY_ATTR_, TYPE), N(SAI_FDB_ENTRY_ATTR_, PACKET_ACTION), N(SAI_FDB_ENTRY_ATTR_, BRIDGE_PORT_ID),
};
static const Name k_attrs_fdb_flush[] = {
    N(SAI_FDB_FLUSH_ATTR_, BRIDGE_PORT_ID), N(SAI_FDB_FLUSH_ATTR_, BV_ID), N(SAI_FDB_FLUSH_ATTR_, ENTRY_TYPE),
};
// ACL objects (match fields and actions as SONiC's aclorch uses them)
#define ACL_FIELDS(P) \
    N(P, FIELD_SRC_IP), N(P, FIELD_DST_IP), N(P, FIELD_SRC_IPV6), N(P, FIELD_DST_IPV6), N(P, FIELD_L4_SRC_PORT), \
    N(P, FIELD_L4_DST_PORT), N(P, FIELD_IP_PROTOCOL), N(P, FIELD_IPV6_NEXT_HEADER), N(P, FIELD_ETHER_TYPE), \
    N(P, FIELD_TCP_FLAGS), N(P, FIELD_DSCP), N(P, FIELD_ECN), N(P, FIELD_TOS), N(P, FIELD_TTL), N(P, FIELD_ICMP_TYPE), \
    N(P, FIELD_ICMP_CODE), N(P, FIELD_ICMPV6_TYPE), N(P, FIELD_ICMPV6_CODE), N(P, FIELD_ACL_IP_TYPE), \
    N(P, FIELD_OUTER_VLAN_ID), N(P, FIELD_IN_PORTS), N(P, FIELD_IN_PORT), N(P, FIELD_OUT_PORTS), N(P, FIELD_OUT_PORT), \
    N(P, FIELD_SRC_MAC), N(P, FIELD_DST_MAC), N(P, FIELD_ACL_RANGE_TYPE)
static const Name k_attrs_acl_table[] = {
    N(SAI_ACL_TABLE_ATTR_, ACL_STAGE), N(SAI_ACL_TABLE_ATTR_, ACL_BIND_POINT_TYPE_LIST), N(SAI_ACL_TABLE_ATTR_, SIZE),
    N(SAI_ACL_TABLE_ATTR_, ACL_ACTION_TYPE_LIST), N(SAI_ACL_TABLE_ATTR_, ENTRY_LIST), ACL_FIELDS(SAI_ACL_TABLE_ATTR_),
};
static const Name k_attrs_acl_entry[] = {
    N(SAI_ACL_ENTRY_ATTR_, TABLE_ID), N(SAI_ACL_ENTRY_ATTR_, PRIORITY), N(SAI_ACL_ENTRY_ATTR_, ADMIN_STATE),
    N(SAI_ACL_ENTRY_ATTR_, ACTION_PACKET_ACTION), N(SAI_ACL_ENTRY_ATTR_, ACTION_COUNTER),
    N(SAI_ACL_ENTRY_ATTR_, ACTION_REDIRECT), N(SAI_ACL_ENTRY_ATTR_, ACTION_SET_TC), N(SAI_ACL_ENTRY_ATTR_, ACTION_SET_DSCP),
    N(SAI_ACL_ENTRY_ATTR_, ACTION_SET_POLICER), N(SAI_ACL_ENTRY_ATTR_, ACTION_MIRROR_INGRESS),
    N(SAI_ACL_ENTRY_ATTR_, ACTION_MIRROR_EGRESS), N(SAI_ACL_ENTRY_ATTR_, ACTION_SET_PACKET_COLOR),
    ACL_FIELDS(SAI_ACL_ENTRY_ATTR_),
};
#undef ACL_FIELDS
static const Name k_attrs_acl_counter[] = {
    N(SAI_ACL_COUNTER_ATTR_, TABLE_ID), N(SAI_ACL_COUNTER_ATTR_, ENABLE_PACKET_COUNT), N(SAI_ACL_COUNTER_ATTR_, ENABLE_BYTE_COUNT),
    N(SAI_ACL_COUNTER_ATTR_, PACKETS), N(SAI_ACL_COUNTER_ATTR_, BYTES),
};
static const Name k_attrs_acl_range[] = {N(SAI_ACL_RANGE_ATTR_, TYPE), N(SAI_ACL_RANGE_ATTR_, LIMIT)};
static const Name k_attrs_acl_group[] = {
    N(SAI_ACL_TABLE_GROUP_ATTR_, ACL_STAGE), N(SAI_ACL_TABLE_GROUP_ATTR_, ACL_BIND_POINT_TYPE_LIST),
    N(SAI_ACL_TABLE_GROUP_ATTR_, TYPE), N(SAI_ACL_TABLE_GROUP_ATTR_, MEMBER_LIST),
};
static const Name k_attrs_acl_group_member[] = {
    N(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_, ACL_TABLE_GROUP_ID), N(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_, ACL_TABLE_ID),
    N(SAI_ACL_TABLE_GROUP_MEMBER_ATTR_, PRIORITY),
};
static const Name k_values[] = {
    // hostif trap types
    N(SAI_HOSTIF_TRAP_TYPE_, STP), N(SAI_HOSTIF_TRAP_TYPE_, LACP), N(SAI_HOSTIF_TRAP_TYPE_, EAPOL), N(SAI_HOSTIF_TRAP_TYPE_, LLDP),
    N(SAI_HOSTIF_TRAP_TYPE_, PVRST), N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_QUERY), N(SAI_HOSTIF_TRAP_TYPE_, UDLD),
    N(SAI_HOSTIF_TRAP_TYPE_, SAMPLEPACKET), N(SAI_HOSTIF_TRAP_TYPE_, ARP_REQUEST), N(SAI_HOSTIF_TRAP_TYPE_, ARP_RESPONSE),
    N(SAI_HOSTIF_TRAP_TYPE_, DHCP), N(SAI_HOSTIF_TRAP_TYPE_, IPV6_NEIGHBOR_DISCOVERY), N(SAI_HOSTIF_TRAP_TYPE_, DHCPV6),
    N(SAI_HOSTIF_TRAP_TYPE_, IP2ME), N(SAI_HOSTIF_TRAP_TYPE_, SSH), N(SAI_HOSTIF_TRAP_TYPE_, SNMP),
    N(SAI_HOSTIF_TRAP_TYPE_, BGP), N(SAI_HOSTIF_TRAP_TYPE_, BGPV6), N(SAI_HOSTIF_TRAP_TYPE_, BFD), N(SAI_HOSTIF_TRAP_TYPE_, BFDV6),
    N(SAI_HOSTIF_TRAP_TYPE_, L3_MTU_ERROR), N(SAI_HOSTIF_TRAP_TYPE_, TTL_ERROR), N(SAI_HOSTIF_TRAP_TYPE_, NEIGHBOR_MISS),
    // packet actions
    N(SAI_PACKET_ACTION_, DROP), N(SAI_PACKET_ACTION_, FORWARD), N(SAI_PACKET_ACTION_, COPY), N(SAI_PACKET_ACTION_, TRAP),
    N(SAI_PACKET_ACTION_, LOG),
    // hostif
    N(SAI_HOSTIF_TYPE_, NETDEV), N(SAI_HOSTIF_VLAN_TAG_, STRIP), N(SAI_HOSTIF_VLAN_TAG_, KEEP), N(SAI_HOSTIF_VLAN_TAG_, ORIGINAL),
    N(SAI_HOSTIF_TABLE_ENTRY_TYPE_, WILDCARD), N(SAI_HOSTIF_TABLE_ENTRY_TYPE_, TRAP_ID),
    N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, NETDEV_PHYSICAL_PORT), N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, NETDEV_L3),
    N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, CB), N(SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_, GENETLINK),
    // policer
    N(SAI_METER_TYPE_, PACKETS), N(SAI_METER_TYPE_, BYTES), N(SAI_POLICER_MODE_, SR_TCM), N(SAI_POLICER_MODE_, TR_TCM),
    N(SAI_POLICER_MODE_, STORM_CONTROL), N(SAI_POLICER_COLOR_SOURCE_, BLIND), N(SAI_POLICER_COLOR_SOURCE_, AWARE),
    // port, L2, L3
    N(SAI_PORT_FEC_MODE_, NONE), N(SAI_PORT_FEC_MODE_, RS), N(SAI_PORT_FEC_MODE_, FC),
    N(SAI_VLAN_TAGGING_MODE_, UNTAGGED), N(SAI_VLAN_TAGGING_MODE_, TAGGED),
    N(SAI_ROUTER_INTERFACE_TYPE_, PORT), N(SAI_ROUTER_INTERFACE_TYPE_, VLAN), N(SAI_ROUTER_INTERFACE_TYPE_, LOOPBACK),
    N(SAI_NEXT_HOP_TYPE_, IP),
    // more trap types, hostif types
    N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_LEAVE), N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_V1_REPORT),
    N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_V2_REPORT), N(SAI_HOSTIF_TRAP_TYPE_, IGMP_TYPE_V3_REPORT),
    N(SAI_HOSTIF_TRAP_TYPE_, IPV6_MLD_V1_V2), N(SAI_HOSTIF_TRAP_TYPE_, IPV6_MLD_V1_REPORT),
    N(SAI_HOSTIF_TRAP_TYPE_, IPV6_MLD_V1_DONE), N(SAI_HOSTIF_TRAP_TYPE_, MLD_V2_REPORT),
    N(SAI_HOSTIF_TYPE_, GENETLINK), N(SAI_HOSTIF_TYPE_, FD),
    // ACL
    N(SAI_ACL_STAGE_, INGRESS), N(SAI_ACL_STAGE_, EGRESS), N(SAI_ACL_BIND_POINT_TYPE_, PORT), N(SAI_ACL_BIND_POINT_TYPE_, LAG),
    N(SAI_ACL_BIND_POINT_TYPE_, VLAN), N(SAI_ACL_BIND_POINT_TYPE_, ROUTER_INTERFACE), N(SAI_ACL_BIND_POINT_TYPE_, SWITCH),
    N(SAI_ACL_TABLE_GROUP_TYPE_, SEQUENTIAL), N(SAI_ACL_TABLE_GROUP_TYPE_, PARALLEL),
    N(SAI_ACL_ACTION_TYPE_, PACKET_ACTION), N(SAI_ACL_ACTION_TYPE_, COUNTER), N(SAI_ACL_ACTION_TYPE_, REDIRECT),
    N(SAI_ACL_ACTION_TYPE_, SET_TC), N(SAI_ACL_ACTION_TYPE_, SET_DSCP), N(SAI_ACL_ACTION_TYPE_, SET_POLICER),
    N(SAI_ACL_ACTION_TYPE_, MIRROR_INGRESS), N(SAI_ACL_ACTION_TYPE_, MIRROR_EGRESS),
    N(SAI_ACL_IP_TYPE_, ANY), N(SAI_ACL_IP_TYPE_, IP), N(SAI_ACL_IP_TYPE_, NON_IP), N(SAI_ACL_IP_TYPE_, IPV4ANY),
    N(SAI_ACL_IP_TYPE_, NON_IPV4), N(SAI_ACL_IP_TYPE_, IPV6ANY), N(SAI_ACL_IP_TYPE_, NON_IPV6), N(SAI_ACL_IP_TYPE_, ARP),
    N(SAI_ACL_IP_TYPE_, ARP_REQUEST), N(SAI_ACL_IP_TYPE_, ARP_REPLY),
    N(SAI_ACL_RANGE_TYPE_, L4_SRC_PORT_RANGE), N(SAI_ACL_RANGE_TYPE_, L4_DST_PORT_RANGE),
    N(SAI_NEXT_HOP_GROUP_TYPE_, ECMP),
    // bridge, FDB, hash
    N(SAI_BRIDGE_PORT_TYPE_, PORT), N(SAI_BRIDGE_PORT_FDB_LEARNING_MODE_, DISABLE), N(SAI_BRIDGE_PORT_FDB_LEARNING_MODE_, HW),
    N(SAI_FDB_ENTRY_TYPE_, STATIC), N(SAI_FDB_ENTRY_TYPE_, DYNAMIC), N(SAI_FDB_FLUSH_ENTRY_TYPE_, STATIC),
    N(SAI_FDB_FLUSH_ENTRY_TYPE_, DYNAMIC), N(SAI_FDB_FLUSH_ENTRY_TYPE_, ALL),
    N(SAI_NATIVE_HASH_FIELD_, SRC_IP), N(SAI_NATIVE_HASH_FIELD_, DST_IP), N(SAI_NATIVE_HASH_FIELD_, VLAN_ID),
    N(SAI_NATIVE_HASH_FIELD_, IP_PROTOCOL), N(SAI_NATIVE_HASH_FIELD_, ETHERTYPE), N(SAI_NATIVE_HASH_FIELD_, L4_SRC_PORT),
    N(SAI_NATIVE_HASH_FIELD_, L4_DST_PORT), N(SAI_NATIVE_HASH_FIELD_, SRC_MAC), N(SAI_NATIVE_HASH_FIELD_, DST_MAC),
    N(SAI_NATIVE_HASH_FIELD_, IN_PORT), N(SAI_HASH_ALGORITHM_, CRC), N(SAI_HASH_ALGORITHM_, XOR),
    N(SAI_HASH_ALGORITHM_, RANDOM), N(SAI_HASH_ALGORITHM_, CRC_32LO), N(SAI_HASH_ALGORITHM_, CRC_32HI),
};
static const Name k_port_stats[] = {
    N(SAI_PORT_STAT_, IF_IN_OCTETS), N(SAI_PORT_STAT_, IF_IN_UCAST_PKTS), N(SAI_PORT_STAT_, IF_IN_NON_UCAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_DISCARDS), N(SAI_PORT_STAT_, IF_IN_ERRORS), N(SAI_PORT_STAT_, IF_IN_BROADCAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_MULTICAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_OCTETS), N(SAI_PORT_STAT_, IF_OUT_UCAST_PKTS),
    N(SAI_PORT_STAT_, IF_OUT_NON_UCAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_DISCARDS), N(SAI_PORT_STAT_, IF_OUT_ERRORS),
    N(SAI_PORT_STAT_, IF_OUT_BROADCAST_PKTS), N(SAI_PORT_STAT_, IF_OUT_MULTICAST_PKTS),
    N(SAI_PORT_STAT_, IF_IN_FEC_CORRECTABLE_FRAMES), N(SAI_PORT_STAT_, IF_IN_FEC_NOT_CORRECTABLE_FRAMES),
};
// QoS / buffers / mirror / sampling (object types, port attributes, values)
static const Name k_types_qos[] = {
    N(SAI_OBJECT_TYPE_, QOS_MAP), N(SAI_OBJECT_TYPE_, SCHEDULER), N(SAI_OBJECT_TYPE_, SCHEDULER_GROUP),
    N(SAI_OBJECT_TYPE_, WRED), N(SAI_OBJECT_TYPE_, BUFFER_POOL), N(SAI_OBJECT_TYPE_, BUFFER_PROFILE),
    N(SAI_OBJECT_TYPE_, QUEUE), N(SAI_OBJECT_TYPE_, INGRESS_PRIORITY_GROUP), N(SAI_OBJECT_TYPE_, MIRROR_SESSION),
    N(SAI_OBJECT_TYPE_, SAMPLEPACKET),
};
static const Name k_attrs_port_qos[] = {
    N(SAI_PORT_ATTR_, QOS_NUMBER_OF_QUEUES), N(SAI_PORT_ATTR_, QOS_QUEUE_LIST), N(SAI_PORT_ATTR_, QOS_NUMBER_OF_SCHEDULER_GROUPS),
    N(SAI_PORT_ATTR_, QOS_SCHEDULER_GROUP_LIST), N(SAI_PORT_ATTR_, NUMBER_OF_INGRESS_PRIORITY_GROUPS),
    N(SAI_PORT_ATTR_, INGRESS_PRIORITY_GROUP_LIST), N(SAI_PORT_ATTR_, QOS_DEFAULT_TC), N(SAI_PORT_ATTR_, QOS_DOT1P_TO_TC_MAP),
    N(SAI_PORT_ATTR_, QOS_DOT1P_TO_COLOR_MAP), N(SAI_PORT_ATTR_, QOS_DSCP_TO_TC_MAP), N(SAI_PORT_ATTR_, QOS_DSCP_TO_COLOR_MAP),
    N(SAI_PORT_ATTR_, QOS_TC_TO_QUEUE_MAP), N(SAI_PORT_ATTR_, QOS_TC_AND_COLOR_TO_DOT1P_MAP),
    N(SAI_PORT_ATTR_, QOS_TC_AND_COLOR_TO_DSCP_MAP), N(SAI_PORT_ATTR_, QOS_TC_TO_PRIORITY_GROUP_MAP),
    N(SAI_PORT_ATTR_, QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP), N(SAI_PORT_ATTR_, QOS_PFC_PRIORITY_TO_QUEUE_MAP),
    N(SAI_PORT_ATTR_, QOS_SCHEDULER_PROFILE_ID), N(SAI_PORT_ATTR_, QOS_INGRESS_BUFFER_PROFILE_LIST),
    N(SAI_PORT_ATTR_, QOS_EGRESS_BUFFER_PROFILE_LIST), N(SAI_PORT_ATTR_, PRIORITY_FLOW_CONTROL_MODE),
    N(SAI_PORT_ATTR_, PRIORITY_FLOW_CONTROL), N(SAI_PORT_ATTR_, PRIORITY_FLOW_CONTROL_RX), N(SAI_PORT_ATTR_, PRIORITY_FLOW_CONTROL_TX),
    N(SAI_PORT_ATTR_, INGRESS_MIRROR_SESSION), N(SAI_PORT_ATTR_, EGRESS_MIRROR_SESSION),
    N(SAI_PORT_ATTR_, INGRESS_SAMPLEPACKET_ENABLE), N(SAI_PORT_ATTR_, EGRESS_SAMPLEPACKET_ENABLE),
    N(SAI_HOSTIF_ATTR_, GENETLINK_MCGRP_NAME),
};
static const Name k_attrs_qos_map[] = {N(SAI_QOS_MAP_ATTR_, TYPE), N(SAI_QOS_MAP_ATTR_, MAP_TO_VALUE_LIST)};
static const Name k_attrs_scheduler[] = {
    N(SAI_SCHEDULER_ATTR_, SCHEDULING_TYPE), N(SAI_SCHEDULER_ATTR_, SCHEDULING_WEIGHT), N(SAI_SCHEDULER_ATTR_, METER_TYPE),
    N(SAI_SCHEDULER_ATTR_, MIN_BANDWIDTH_RATE), N(SAI_SCHEDULER_ATTR_, MIN_BANDWIDTH_BURST_RATE),
    N(SAI_SCHEDULER_ATTR_, MAX_BANDWIDTH_RATE), N(SAI_SCHEDULER_ATTR_, MAX_BANDWIDTH_BURST_RATE),
};
static const Name k_attrs_scheduler_group[] = {
    N(SAI_SCHEDULER_GROUP_ATTR_, CHILD_COUNT), N(SAI_SCHEDULER_GROUP_ATTR_, CHILD_LIST), N(SAI_SCHEDULER_GROUP_ATTR_, PORT_ID),
    N(SAI_SCHEDULER_GROUP_ATTR_, LEVEL), N(SAI_SCHEDULER_GROUP_ATTR_, MAX_CHILDS), N(SAI_SCHEDULER_GROUP_ATTR_, SCHEDULER_PROFILE_ID),
    N(SAI_SCHEDULER_GROUP_ATTR_, PARENT_NODE),
};
static const Name k_attrs_wred[] = {
    N(SAI_WRED_ATTR_, GREEN_ENABLE), N(SAI_WRED_ATTR_, GREEN_MIN_THRESHOLD), N(SAI_WRED_ATTR_, GREEN_MAX_THRESHOLD),
    N(SAI_WRED_ATTR_, GREEN_DROP_PROBABILITY), N(SAI_WRED_ATTR_, YELLOW_ENABLE), N(SAI_WRED_ATTR_, YELLOW_MIN_THRESHOLD),
    N(SAI_WRED_ATTR_, YELLOW_MAX_THRESHOLD), N(SAI_WRED_ATTR_, YELLOW_DROP_PROBABILITY), N(SAI_WRED_ATTR_, RED_ENABLE),
    N(SAI_WRED_ATTR_, RED_MIN_THRESHOLD), N(SAI_WRED_ATTR_, RED_MAX_THRESHOLD), N(SAI_WRED_ATTR_, RED_DROP_PROBABILITY),
    N(SAI_WRED_ATTR_, WEIGHT), N(SAI_WRED_ATTR_, ECN_MARK_MODE),
};
static const Name k_attrs_buffer_pool[] = {
    N(SAI_BUFFER_POOL_ATTR_, SHARED_SIZE), N(SAI_BUFFER_POOL_ATTR_, TYPE), N(SAI_BUFFER_POOL_ATTR_, SIZE),
    N(SAI_BUFFER_POOL_ATTR_, THRESHOLD_MODE), N(SAI_BUFFER_POOL_ATTR_, XOFF_SIZE),
};
static const Name k_attrs_buffer_profile[] = {
    N(SAI_BUFFER_PROFILE_ATTR_, POOL_ID), N(SAI_BUFFER_PROFILE_ATTR_, RESERVED_BUFFER_SIZE), N(SAI_BUFFER_PROFILE_ATTR_, THRESHOLD_MODE),
    N(SAI_BUFFER_PROFILE_ATTR_, SHARED_DYNAMIC_TH), N(SAI_BUFFER_PROFILE_ATTR_, SHARED_STATIC_TH), N(SAI_BUFFER_PROFILE_ATTR_, XOFF_TH),
    N(SAI_BUFFER_PROFILE_ATTR_, XON_TH), N(SAI_BUFFER_PROFILE_ATTR_, XON_OFFSET_TH),
};
static const Name k_attrs_queue[] = {
    N(SAI_QUEUE_ATTR_, TYPE), N(SAI_QUEUE_ATTR_, PORT), N(SAI_QUEUE_ATTR_, INDEX), N(SAI_QUEUE_ATTR_, PARENT_SCHEDULER_NODE),
    N(SAI_QUEUE_ATTR_, WRED_PROFILE_ID), N(SAI_QUEUE_ATTR_, BUFFER_PROFILE_ID), N(SAI_QUEUE_ATTR_, SCHEDULER_PROFILE_ID),
    N(SAI_QUEUE_ATTR_, PFC_DLR_INIT),
};
static const Name k_attrs_ipg[] = {
    N(SAI_INGRESS_PRIORITY_GROUP_ATTR_, BUFFER_PROFILE), N(SAI_INGRESS_PRIORITY_GROUP_ATTR_, PORT),
    N(SAI_INGRESS_PRIORITY_GROUP_ATTR_, INDEX),
};
static const Name k_attrs_mirror[] = {
    N(SAI_MIRROR_SESSION_ATTR_, TYPE), N(SAI_MIRROR_SESSION_ATTR_, MONITOR_PORT), N(SAI_MIRROR_SESSION_ATTR_, TRUNCATE_SIZE),
    N(SAI_MIRROR_SESSION_ATTR_, SAMPLE_RATE), N(SAI_MIRROR_SESSION_ATTR_, TC), N(SAI_MIRROR_SESSION_ATTR_, VLAN_TPID),
    N(SAI_MIRROR_SESSION_ATTR_, VLAN_ID), N(SAI_MIRROR_SESSION_ATTR_, VLAN_PRI), N(SAI_MIRROR_SESSION_ATTR_, VLAN_CFI),
    N(SAI_MIRROR_SESSION_ATTR_, VLAN_HEADER_VALID), N(SAI_MIRROR_SESSION_ATTR_, ERSPAN_ENCAPSULATION_TYPE),
    N(SAI_MIRROR_SESSION_ATTR_, IPHDR_VERSION), N(SAI_MIRROR_SESSION_ATTR_, TOS), N(SAI_MIRROR_SESSION_ATTR_, TTL),
    N(SAI_MIRROR_SESSION_ATTR_, SRC_IP_ADDRESS), N(SAI_MIRROR_SESSION_ATTR_, DST_IP_ADDRESS),
    N(SAI_MIRROR_SESSION_ATTR_, SRC_MAC_ADDRESS), N(SAI_MIRROR_SESSION_ATTR_, DST_MAC_ADDRESS),
    N(SAI_MIRROR_SESSION_ATTR_, GRE_PROTOCOL_TYPE), N(SAI_MIRROR_SESSION_ATTR_, POLICER),
};
static const Name k_attrs_samplepacket[] = {
    N(SAI_SAMPLEPACKET_ATTR_, SAMPLE_RATE), N(SAI_SAMPLEPACKET_ATTR_, TYPE), N(SAI_SAMPLEPACKET_ATTR_, MODE),
};
static const Name k_values_qos[] = {
    N(SAI_HOSTIF_TYPE_, GENETLINK),
    N(SAI_QOS_MAP_TYPE_, DOT1P_TO_TC), N(SAI_QOS_MAP_TYPE_, DOT1P_TO_COLOR), N(SAI_QOS_MAP_TYPE_, DSCP_TO_TC),
    N(SAI_QOS_MAP_TYPE_, DSCP_TO_COLOR), N(SAI_QOS_MAP_TYPE_, TC_TO_QUEUE), N(SAI_QOS_MAP_TYPE_, TC_AND_COLOR_TO_DSCP),
    N(SAI_QOS_MAP_TYPE_, TC_AND_COLOR_TO_DOT1P), N(SAI_QOS_MAP_TYPE_, TC_TO_PRIORITY_GROUP),
    N(SAI_QOS_MAP_TYPE_, PFC_PRIORITY_TO_PRIORITY_GROUP), N(SAI_QOS_MAP_TYPE_, PFC_PRIORITY_TO_QUEUE),
    N(SAI_SCHEDULING_TYPE_, STRICT), N(SAI_SCHEDULING_TYPE_, WRR), N(SAI_SCHEDULING_TYPE_, DWRR),
    N(SAI_BUFFER_POOL_TYPE_, INGRESS), N(SAI_BUFFER_POOL_TYPE_, EGRESS),
    N(SAI_BUFFER_POOL_THRESHOLD_MODE_, STATIC), N(SAI_BUFFER_POOL_THRESHOLD_MODE_, DYNAMIC),
    N(SAI_BUFFER_PROFILE_THRESHOLD_MODE_, STATIC), N(SAI_BUFFER_PROFILE_THRESHOLD_MODE_, DYNAMIC),
    N(SAI_ECN_MARK_MODE_, NONE), N(SAI_ECN_MARK_MODE_, GREEN), N(SAI_ECN_MARK_MODE_, ALL),
    N(SAI_MIRROR_SESSION_TYPE_, LOCAL), N(SAI_MIRROR_SESSION_TYPE_, REMOTE), N(SAI_MIRROR_SESSION_TYPE_, ENHANCED_REMOTE),
    N(SAI_ERSPAN_ENCAPSULATION_TYPE_, MIRROR_L3_GRE_TUNNEL),
    N(SAI_SAMPLEPACKET_TYPE_, SLOW_PATH), N(SAI_SAMPLEPACKET_MODE_, EXCLUSIVE), N(SAI_SAMPLEPACKET_MODE_, SHARED),
    N(SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_, COMBINED), N(SAI_PORT_PRIORITY_FLOW_CONTROL_MODE_, SEPARATE),
    N(SAI_METER_TYPE_, PACKETS), N(SAI_METER_TYPE_, BYTES),
};
static const Name k_queue_stats[] = {
    N(SAI_QUEUE_STAT_, PACKETS), N(SAI_QUEUE_STAT_, BYTES), N(SAI_QUEUE_STAT_, DROPPED_PACKETS), N(SAI_QUEUE_STAT_, DROPPED_BYTES),
    N(SAI_QUEUE_STAT_, CURR_OCCUPANCY_BYTES), N(SAI_QUEUE_STAT_, WATERMARK_BYTES), N(SAI_QUEUE_STAT_, SHARED_WATERMARK_BYTES),
    N(SAI_QUEUE_STAT_, WRED_ECN_MARKED_PACKETS), N(SAI_QUEUE_STAT_, WRED_ECN_MARKED_BYTES),
};
static const Name k_ipg_stats[] = {
    N(SAI_INGRESS_PRIORITY_GROUP_STAT_, PACKETS), N(SAI_INGRESS_PRIORITY_GROUP_STAT_, BYTES),
    N(SAI_INGRESS_PRIORITY_GROUP_STAT_, CURR_OCCUPANCY_BYTES), N(SAI_INGRESS_PRIORITY_GROUP_STAT_, WATERMARK_BYTES),
    N(SAI_INGRESS_PRIORITY_GROUP_STAT_, SHARED_WATERMARK_BYTES), N(SAI_INGRESS_PRIORITY_GROUP_STAT_, XOFF_ROOM_WATERMARK_BYTES),
    N(SAI_INGRESS_PRIORITY_GROUP_STAT_, DROPPED_PACKETS),
};
static const Name k_pool_stats[] = {
    N(SAI_BUFFER_POOL_STAT_, CURR_OCCUPANCY_BYTES), N(SAI_BUFFER_POOL_STAT_, WATERMARK_BYTES),
    N(SAI_BUFFER_POOL_STAT_, XOFF_ROOM_WATERMARK_BYTES), N(SAI_BUFFER_POOL_STAT_, DROPPED_PACKETS),
};
#undef N
template <size_t K> static bool lookup(const Name (&t)[K], const std::string& s, int64_t& v)
{
    for (auto& n : t)
        if (s == n.n) { v = n.v; return true; }
    return false;
}

struct Fail { std::string what; };
static int64_t num(const json& j, const Name* t = nullptr, size_t k = 0)
{
    if (j.is_number_integer()) return j.get<int64_t>();
    if (j.is_boolean()) return j.get<bool>() ? 1 : 0;
    std::string s = j.get<std::string>();
    for (size_t i = 0; t && i < k; i++)
        if (s == t[i].n) return t[i].v;
    int64_t v;
    if (lookup(k_values, s, v)) return v;
    if (lookup(k_values_qos, s, v)) return v;
    try { return std::stoll(s, nullptr, 0); } catch (...) {}
    throw Fail{"unknown value " + s};
}

static std::map<std::string, sai_object_id_t> g_obj;
static std::map<std::string, std::vector<sai_object_id_t>> g_lists;
static sai_object_id_t g_switch = SAI_NULL_OBJECT_ID;

static sai_object_id_t oid(const json& j)
{
    if (j.is_number_integer()) return j.get<uint64_t>();
    std::string s = j.get<std::string>();
    if (s == "switch") return g_switch;
    if (s == "null") return SAI_NULL_OBJECT_ID;
    if (!s.empty() && s[0] == '$') s = s.substr(1);
    auto b = s.find('[');
    if (b != std::string::npos) {
        std::string n = s.substr(0, b);
        size_t i = std::stoul(s.substr(b + 1));
        auto it = g_lists.find(n);
        if (it == g_lists.end() || i >= it->second.size()) throw Fail{"no list element " + s};
        return it->second[i];
    }
    auto it = g_obj.find(s);
    if (it == g_obj.end()) throw Fail{"unknown object " + s};
    return it->second;
}

static void parse_mac(const std::string& s, sai_mac_t m)
{
    unsigned b[6] = {0};
    if (sscanf(s.c_str(), "%x:%x:%x:%x:%x:%x", &b[0], &b[1], &b[2], &b[3], &b[4], &b[5]) != 6) throw Fail{"bad mac " + s};
    for (int i = 0; i < 6; i++) m[i] = (uint8_t)b[i];
}
static sai_ip_address_t parse_ip(const std::string& s)
{
    sai_ip_address_t a;
    std::memset(&a, 0, sizeof(a));
    if (s.find(':') != std::string::npos) {
        a.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
        if (inet_pton(AF_INET6, s.c_str(), a.addr.ip6) != 1) throw Fail{"bad ip " + s};
    } else {
        a.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
        if (inet_pton(AF_INET, s.c_str(), &a.addr.ip4) != 1) throw Fail{"bad ip " + s};
    }
    return a;
}
static sai_ip_prefix_t parse_prefix(const std::string& s)
{
    sai_ip_prefix_t p;
    std::memset(&p, 0, sizeof(p));
    auto sl = s.find('/');
    sai_ip_address_t a = parse_ip(s.substr(0, sl));
    int len = sl == std::string::npos ? (a.addr_family == SAI_IP_ADDR_FAMILY_IPV4 ? 32 : 128) : std::stoi(s.substr(sl + 1));
    p.addr_family = a.addr_family;
    if (a.addr_family == SAI_IP_ADDR_FAMILY_IPV4) {
        p.addr.ip4 = a.addr.ip4;
        p.mask.ip4 = htonl(len == 0 ? 0 : 0xffffffffu << (32 - len));
    } else {
        std::memcpy(p.addr.ip6, a.addr.ip6, 16);
        for (int i = 0; i < 16; i++) {
            int bits = len - i * 8;
            p.mask.ip6[i] = bits >= 8 ? 0xff : bits <= 0 ? 0 : (uint8_t)(0xff << (8 - bits));
        }
    }
    return p;
}

struct Storage {
    std::vector<std::vector<sai_object_id_t>> oids;
    std::vector<std::vector<uint32_t>> u32s;
    std::vector<std::vector<int32_t>> s32s;
    std::vector<std::vector<sai_qos_map_t>> maps;
};

// ACL field data / mask: {"u8"|"u16"|"u32"|"s32": v, "mask": m} (u8/u16/u32 mask defaults to all ones),
// {"ip4"|"ip6"|"mac": "...", "mask": "..."}, {"oid": "$x"}, {"oids": ["$a", ...]}; "enable": false disables.
static void acl_field(const json& v, sai_acl_field_data_t& f, Storage& st)
{
    f.enable = v.value("enable", true);
    if (v.contains("u8")) { f.data.u8 = (uint8_t)num(v["u8"]); f.mask.u8 = (uint8_t)(v.contains("mask") ? num(v["mask"]) : 0xff); }
    else if (v.contains("u16")) { f.data.u16 = (uint16_t)num(v["u16"]); f.mask.u16 = (uint16_t)(v.contains("mask") ? num(v["mask"]) : 0xffff); }
    else if (v.contains("u32")) { f.data.u32 = (uint32_t)num(v["u32"]); f.mask.u32 = (uint32_t)(v.contains("mask") ? num(v["mask"]) : 0xffffffff); }
    else if (v.contains("s32")) f.data.s32 = (int32_t)num(v["s32"]);
    else if (v.contains("ip4")) {
        if (inet_pton(AF_INET, v["ip4"].get<std::string>().c_str(), &f.data.ip4) != 1) throw Fail{"bad ip4"};
        if (inet_pton(AF_INET, v.value("mask", std::string("255.255.255.255")).c_str(), &f.mask.ip4) != 1) throw Fail{"bad mask"};
    } else if (v.contains("ip6")) {
        if (inet_pton(AF_INET6, v["ip6"].get<std::string>().c_str(), f.data.ip6) != 1) throw Fail{"bad ip6"};
        std::string m = v.value("mask", std::string("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"));
        if (inet_pton(AF_INET6, m.c_str(), f.mask.ip6) != 1) throw Fail{"bad mask6"};
    } else if (v.contains("mac")) {
        parse_mac(v["mac"].get<std::string>(), f.data.mac);
        parse_mac(v.value("mask", std::string("ff:ff:ff:ff:ff:ff")), f.mask.mac);
    } else if (v.contains("oid")) f.data.oid = oid(v["oid"]);
    else if (v.contains("oids")) {
        st.oids.emplace_back();
        for (auto& o : v["oids"]) st.oids.back().push_back(oid(o));
        f.data.objlist.count = (uint32_t)st.oids.back().size();
        f.data.objlist.list = st.oids.back().data();
    }
}
// ACL action parameter: {"s32": v} / {"u8"|"u32": v} / {"oid": "$x"} / {} (enable only); "enable": false.
static void acl_action(const json& v, sai_acl_action_data_t& a)
{
    a.enable = v.value("enable", true);
    if (v.contains("s32")) a.parameter.s32 = (int32_t)num(v["s32"]);
    else if (v.contains("u8")) a.parameter.u8 = (uint8_t)num(v["u8"]);
    else if (v.contains("u32")) a.parameter.u32 = (uint32_t)num(v["u32"]);
    else if (v.contains("oid")) a.parameter.oid = oid(v["oid"]);
}

// qosmap entry ["keyfield", n, "valuefield", n]; fields tc dscp dot1p prio pg queue color.
static void qos_field(sai_qos_map_params_t& p, const std::string& f, uint32_t v)
{
    if (f == "tc") p.tc = (sai_cos_t)v;
    else if (f == "dscp") p.dscp = (uint8_t)v;
    else if (f == "dot1p") p.dot1p = (uint8_t)v;
    else if (f == "prio") p.prio = (uint8_t)v;
    else if (f == "pg") p.pg = (uint8_t)v;
    else if (f == "queue") p.queue_index = (sai_queue_index_t)v;
    else if (f == "color") p.color = (sai_packet_color_t)v;
    else throw Fail{"unknown qos map field " + f};
}

static const Name* attr_table(sai_object_type_t t, size_t& k)
{
#define T(x) k = sizeof(x) / sizeof(x[0]); return x
    switch (t) {
    case SAI_OBJECT_TYPE_SWITCH: T(k_attrs_switch);
    case SAI_OBJECT_TYPE_PORT: T(k_attrs_port);
    case SAI_OBJECT_TYPE_HOSTIF: T(k_attrs_hostif);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: T(k_attrs_trap_group);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: T(k_attrs_trap);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: T(k_attrs_table_entry);
    case SAI_OBJECT_TYPE_POLICER: T(k_attrs_policer);
    case SAI_OBJECT_TYPE_VLAN: T(k_attrs_vlan);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: T(k_attrs_vlan_member);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: T(k_attrs_rif);
    case SAI_OBJECT_TYPE_ROUTE_ENTRY: T(k_attrs_route);
    case SAI_OBJECT_TYPE_NEIGHBOR_ENTRY: T(k_attrs_neighbor);
    case SAI_OBJECT_TYPE_NEXT_HOP: T(k_attrs_nexthop);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: T(k_attrs_bridge_port);
    case SAI_OBJECT_TYPE_ACL_TABLE: T(k_attrs_acl_table);
    case SAI_OBJECT_TYPE_ACL_ENTRY: T(k_attrs_acl_entry);
    case SAI_OBJECT_TYPE_ACL_COUNTER: T(k_attrs_acl_counter);
    case SAI_OBJECT_TYPE_ACL_RANGE: T(k_attrs_acl_range);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP: T(k_attrs_acl_group);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER: T(k_attrs_acl_group_member);
    case SAI_OBJECT_TYPE_QOS_MAP: T(k_attrs_qos_map);
    case SAI_OBJECT_TYPE_SCHEDULER: T(k_attrs_scheduler);
    case SAI_OBJECT_TYPE_SCHEDULER_GROUP: T(k_attrs_scheduler_group);
    case SAI_OBJECT_TYPE_WRED: T(k_attrs_wred);
    case SAI_OBJECT_TYPE_BUFFER_POOL: T(k_attrs_buffer_pool);
    case SAI_OBJECT_TYPE_BUFFER_PROFILE: T(k_attrs_buffer_profile);
    case SAI_OBJECT_TYPE_QUEUE: T(k_attrs_queue);
    case SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP: T(k_attrs_ipg);
    case SAI_OBJECT_TYPE_MIRROR_SESSION: T(k_attrs_mirror);
    case SAI_OBJECT_TYPE_SAMPLEPACKET: T(k_attrs_samplepacket);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: T(k_attrs_vr);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP: T(k_attrs_nhg);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER: T(k_attrs_nhgm);
    case SAI_OBJECT_TYPE_BRIDGE: T(k_attrs_bridge);
    case SAI_OBJECT_TYPE_LAG: T(k_attrs_lag);
    case SAI_OBJECT_TYPE_LAG_MEMBER: T(k_attrs_lag_member);
    case SAI_OBJECT_TYPE_HASH: T(k_attrs_hash);
    case SAI_OBJECT_TYPE_FDB_ENTRY: T(k_attrs_fdb);
    case SAI_OBJECT_TYPE_FDB_FLUSH: T(k_attrs_fdb_flush);
    default: k = 0; return nullptr;
    }
#undef T
}

// One attribute ["NAME", "type", value]. Types: bool u8 u16 u32 s32 u64 oid
// mac ip chardata oidlist(n) u32list([..]) ptr(port_state|fdb|shutdown)
// oids([objects]: an object list with values) qosmap([[kfield,k,vfield,v],..]).
static sai_attribute_t make_attr(sai_object_type_t t, const json& a, Storage& st)
{
    sai_attribute_t x;
    std::memset(&x, 0, sizeof(x));
    size_t k = 0;
    const Name* tab = attr_table(t, k);
    int64_t id = 0;
    std::string an = a[0].is_string() ? a[0].get<std::string>() : "";
    bool found = false;
    for (size_t i = 0; tab && i < k; i++)
        if (an == tab[i].n) { id = tab[i].v; found = true; }
    if (!found && an.size() && !lookup(k_attrs_port_qos, an, id)) id = num(a[0]);
    else if (!found && an.empty()) id = num(a[0]);
    x.id = (sai_attr_id_t)id;
    std::string ty = a[1].get<std::string>();
    const json v = a.size() > 2 ? a[2] : json();
    if (v.is_null() && ty != "oidlist" && ty != "u32list" && ty != "qosmap") return x;   // a get: no value
    if (ty == "bool") x.value.booldata = v.get<bool>();
    else if (ty == "u8") x.value.u8 = (uint8_t)num(v);
    else if (ty == "s8") x.value.s8 = (int8_t)num(v);
    else if (ty == "u16") x.value.u16 = (uint16_t)num(v);
    else if (ty == "u32") x.value.u32 = (uint32_t)num(v);
    else if (ty == "s32") x.value.s32 = (int32_t)num(v);
    else if (ty == "u64") x.value.u64 = (uint64_t)num(v);
    else if (ty == "oid") x.value.oid = oid(v);
    else if (ty == "mac") parse_mac(v.get<std::string>(), x.value.mac);
    else if (ty == "ip") x.value.ipaddr = parse_ip(v.get<std::string>());
    else if (ty == "chardata") std::snprintf(x.value.chardata, sizeof(x.value.chardata), "%s", v.get<std::string>().c_str());
    else if (ty == "oidlist") {
        st.oids.emplace_back(v.is_number() ? v.get<size_t>() : 256);
        x.value.objlist.count = (uint32_t)st.oids.back().size();
        x.value.objlist.list = st.oids.back().data();
    } else if (ty == "u32list") {
        st.u32s.emplace_back();
        if (v.is_array()) for (auto& e : v) st.u32s.back().push_back((uint32_t)num(e));
        else st.u32s.back().resize(v.is_number() ? v.get<size_t>() : 64);
        x.value.u32list.count = (uint32_t)st.u32s.back().size();
        x.value.u32list.list = st.u32s.back().data();
    } else if (ty == "oids") {
        st.oids.emplace_back();
        for (auto& e : v) st.oids.back().push_back(oid(e));
        x.value.objlist.count = (uint32_t)st.oids.back().size();
        x.value.objlist.list = st.oids.back().data();
    } else if (ty == "qosmap") {
        st.maps.emplace_back();
        if (v.is_array())
            for (auto& e : v) {
                sai_qos_map_t m;
                std::memset(&m, 0, sizeof(m));
                qos_field(m.key, e.at(0).get<std::string>(), (uint32_t)num(e.at(1)));
                qos_field(m.value, e.at(2).get<std::string>(), (uint32_t)num(e.at(3)));
                st.maps.back().push_back(m);
            }
        else st.maps.back().resize(v.is_number() ? v.get<size_t>() : 128);
        x.value.qosmap.count = (uint32_t)st.maps.back().size();
        x.value.qosmap.list = st.maps.back().data();
    } else if (ty == "s32list") {
        st.s32s.emplace_back();
        if (v.is_array()) for (auto& e : v) st.s32s.back().push_back((int32_t)num(e));
        else st.s32s.back().resize(v.is_number() ? v.get<size_t>() : 64);
        x.value.s32list.count = (uint32_t)st.s32s.back().size();
        x.value.s32list.list = st.s32s.back().data();
    } else if (ty == "aclfield") {
        acl_field(v, x.value.aclfield, st);
    } else if (ty == "aclaction") {
        acl_action(v, x.value.aclaction);
    } else if (ty == "u32range") {
        x.value.u32range.min = (uint32_t)num(v.at(0));
        x.value.u32range.max = (uint32_t)num(v.at(1));
    } else if (ty == "ptr") {
        std::string p = v.get<std::string>();
        x.value.ptr = p == "port_state" ? (sai_pointer_t)on_port_state : p == "fdb" ? (sai_pointer_t)on_fdb
                    : p == "shutdown" ? (sai_pointer_t)on_shutdown : nullptr;
    } else throw Fail{"unknown attr type " + ty};
    return x;
}

static std::string show(sai_object_type_t, const sai_attribute_t& x, const std::string& ty)
{
    char b[256];
    if (ty == "bool") return x.value.booldata ? "true" : "false";
    if (ty == "s32") { snprintf(b, sizeof(b), "%d", x.value.s32); return b; }
    if (ty == "u32") { snprintf(b, sizeof(b), "%u", x.value.u32); return b; }
    if (ty == "u64") { snprintf(b, sizeof(b), "%llu", (unsigned long long)x.value.u64); return b; }
    if (ty == "u8") { snprintf(b, sizeof(b), "%u", (unsigned)x.value.u8); return b; }
    if (ty == "s8") { snprintf(b, sizeof(b), "%d", (int)x.value.s8); return b; }
    if (ty == "u16") { snprintf(b, sizeof(b), "%u", (unsigned)x.value.u16); return b; }
    if (ty == "qosmap") {
        std::string s = std::to_string(x.value.qosmap.count) + " entries";
        for (uint32_t i = 0; i < x.value.qosmap.count && i < 8; i++)
            s += " " + std::to_string(x.value.qosmap.list[i].key.dscp) + "/" + std::to_string(x.value.qosmap.list[i].key.tc) +
                 "->" + std::to_string(x.value.qosmap.list[i].value.tc) + "/" +
                 std::to_string(x.value.qosmap.list[i].value.queue_index);
        return s;
    }
    if (ty == "oid") { snprintf(b, sizeof(b), "0x%llx", (unsigned long long)x.value.oid); return b; }
    if (ty == "oidlist" || ty == "oids") {
        std::string s = std::to_string(x.value.objlist.count) + " oids";
        for (uint32_t i = 0; i < x.value.objlist.count && i < 16; i++) {
            snprintf(b, sizeof(b), " 0x%llx", (unsigned long long)x.value.objlist.list[i]);
            s += b;
        }
        return s;
    }
    if (ty == "u32list") {
        std::string s = "[";
        for (uint32_t i = 0; i < x.value.u32list.count; i++) s += (i ? "," : "") + std::to_string(x.value.u32list.list[i]);
        return s + "]";
    }
    if (ty == "mac") {
        snprintf(b, sizeof(b), "%02x:%02x:%02x:%02x:%02x:%02x", x.value.mac[0], x.value.mac[1], x.value.mac[2],
                 x.value.mac[3], x.value.mac[4], x.value.mac[5]);
        return b;
    }
    return "?";
}

struct Apis {
    sai_switch_api_t* sw; sai_port_api_t* port; sai_hostif_api_t* hostif; sai_policer_api_t* policer;
    sai_vlan_api_t* vlan; sai_router_interface_api_t* rif; sai_route_api_t* route; sai_neighbor_api_t* neighbor;
    sai_next_hop_api_t* nexthop; sai_bridge_api_t* bridge; sai_virtual_router_api_t* vr;
    sai_acl_api_t* acl;
    sai_next_hop_group_api_t* nhg;
    sai_lag_api_t* lag; sai_fdb_api_t* fdb; sai_hash_api_t* hash;
};
static Apis A;
static struct {
    sai_qos_map_api_t* qos_map; sai_scheduler_api_t* scheduler; sai_scheduler_group_api_t* sched_group;
    sai_wred_api_t* wred; sai_buffer_api_t* buffer; sai_queue_api_t* queue; sai_mirror_api_t* mirror;
    sai_samplepacket_api_t* samplepacket;
} Q;   // QoS / buffers / mirror / sampling

static sai_object_type_t type_of(const std::string& s)
{
    int64_t v;
    if (!lookup(k_types, s, v) && !lookup(k_types_qos, s, v)) throw Fail{"unknown object type " + s};
    return (sai_object_type_t)v;
}

static sai_status_t do_create(sai_object_type_t t, sai_object_id_t* o, uint32_t n, const sai_attribute_t* a)
{
    switch (t) {
    case SAI_OBJECT_TYPE_QOS_MAP: return Q.qos_map->create_qos_map(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_SCHEDULER: return Q.scheduler->create_scheduler(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_SCHEDULER_GROUP: return Q.sched_group->create_scheduler_group(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_WRED: return Q.wred->create_wred(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_BUFFER_POOL: return Q.buffer->create_buffer_pool(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_BUFFER_PROFILE: return Q.buffer->create_buffer_profile(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_MIRROR_SESSION: return Q.mirror->create_mirror_session(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_SAMPLEPACKET: return Q.samplepacket->create_samplepacket(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->create_hostif(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->create_hostif_trap_group(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->create_hostif_trap(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: return A.hostif->create_hostif_table_entry(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->create_policer(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->create_vlan(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: return A.vlan->create_vlan_member(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->create_router_interface(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP: return A.nexthop->create_next_hop(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->create_virtual_router(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_TABLE: return A.acl->create_acl_table(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_ENTRY: return A.acl->create_acl_entry(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_COUNTER: return A.acl->create_acl_counter(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_RANGE: return A.acl->create_acl_range(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP: return A.acl->create_acl_table_group(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER: return A.acl->create_acl_table_group_member(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP: return A.nhg->create_next_hop_group(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER: return A.nhg->create_next_hop_group_member(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: return A.bridge->create_bridge_port(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_LAG: return A.lag->create_lag(o, g_switch, n, a);
    case SAI_OBJECT_TYPE_LAG_MEMBER: return A.lag->create_lag_member(o, g_switch, n, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_remove(sai_object_id_t o)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_QOS_MAP: return Q.qos_map->remove_qos_map(o);
    case SAI_OBJECT_TYPE_SCHEDULER: return Q.scheduler->remove_scheduler(o);
    case SAI_OBJECT_TYPE_SCHEDULER_GROUP: return Q.sched_group->remove_scheduler_group(o);
    case SAI_OBJECT_TYPE_WRED: return Q.wred->remove_wred(o);
    case SAI_OBJECT_TYPE_BUFFER_POOL: return Q.buffer->remove_buffer_pool(o);
    case SAI_OBJECT_TYPE_BUFFER_PROFILE: return Q.buffer->remove_buffer_profile(o);
    case SAI_OBJECT_TYPE_MIRROR_SESSION: return Q.mirror->remove_mirror_session(o);
    case SAI_OBJECT_TYPE_SAMPLEPACKET: return Q.samplepacket->remove_samplepacket(o);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->remove_hostif(o);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->remove_hostif_trap_group(o);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->remove_hostif_trap(o);
    case SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY: return A.hostif->remove_hostif_table_entry(o);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->remove_policer(o);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->remove_vlan(o);
    case SAI_OBJECT_TYPE_VLAN_MEMBER: return A.vlan->remove_vlan_member(o);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->remove_router_interface(o);
    case SAI_OBJECT_TYPE_NEXT_HOP: return A.nexthop->remove_next_hop(o);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->remove_virtual_router(o);
    case SAI_OBJECT_TYPE_ACL_TABLE: return A.acl->remove_acl_table(o);
    case SAI_OBJECT_TYPE_ACL_ENTRY: return A.acl->remove_acl_entry(o);
    case SAI_OBJECT_TYPE_ACL_COUNTER: return A.acl->remove_acl_counter(o);
    case SAI_OBJECT_TYPE_ACL_RANGE: return A.acl->remove_acl_range(o);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP: return A.acl->remove_acl_table_group(o);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER: return A.acl->remove_acl_table_group_member(o);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP: return A.nhg->remove_next_hop_group(o);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER: return A.nhg->remove_next_hop_group_member(o);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: return A.bridge->remove_bridge_port(o);
    case SAI_OBJECT_TYPE_LAG: return A.lag->remove_lag(o);
    case SAI_OBJECT_TYPE_LAG_MEMBER: return A.lag->remove_lag_member(o);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_set(sai_object_id_t o, const sai_attribute_t* a)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_QOS_MAP: return Q.qos_map->set_qos_map_attribute(o, a);
    case SAI_OBJECT_TYPE_SCHEDULER: return Q.scheduler->set_scheduler_attribute(o, a);
    case SAI_OBJECT_TYPE_SCHEDULER_GROUP: return Q.sched_group->set_scheduler_group_attribute(o, a);
    case SAI_OBJECT_TYPE_WRED: return Q.wred->set_wred_attribute(o, a);
    case SAI_OBJECT_TYPE_BUFFER_POOL: return Q.buffer->set_buffer_pool_attribute(o, a);
    case SAI_OBJECT_TYPE_BUFFER_PROFILE: return Q.buffer->set_buffer_profile_attribute(o, a);
    case SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP: return Q.buffer->set_ingress_priority_group_attribute(o, a);
    case SAI_OBJECT_TYPE_QUEUE: return Q.queue->set_queue_attribute(o, a);
    case SAI_OBJECT_TYPE_MIRROR_SESSION: return Q.mirror->set_mirror_session_attribute(o, a);
    case SAI_OBJECT_TYPE_SAMPLEPACKET: return Q.samplepacket->set_samplepacket_attribute(o, a);
    case SAI_OBJECT_TYPE_SWITCH: return A.sw->set_switch_attribute(o, a);
    case SAI_OBJECT_TYPE_PORT: return A.port->set_port_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->set_hostif_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->set_hostif_trap_group_attribute(o, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->set_hostif_trap_attribute(o, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->set_policer_attribute(o, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->set_router_interface_attribute(o, a);
    case SAI_OBJECT_TYPE_ACL_ENTRY: return A.acl->set_acl_entry_attribute(o, a);
    case SAI_OBJECT_TYPE_ACL_COUNTER: return A.acl->set_acl_counter_attribute(o, a);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->set_virtual_router_attribute(o, a);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER: return A.nhg->set_next_hop_group_member_attribute(o, a);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: return A.bridge->set_bridge_port_attribute(o, a);
    case SAI_OBJECT_TYPE_LAG: return A.lag->set_lag_attribute(o, a);
    case SAI_OBJECT_TYPE_LAG_MEMBER: return A.lag->set_lag_member_attribute(o, a);
    case SAI_OBJECT_TYPE_HASH: return A.hash->set_hash_attribute(o, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}
static sai_status_t do_get(sai_object_id_t o, uint32_t n, sai_attribute_t* a)
{
    switch (sai_object_type_query(o)) {
    case SAI_OBJECT_TYPE_QOS_MAP: return Q.qos_map->get_qos_map_attribute(o, n, a);
    case SAI_OBJECT_TYPE_SCHEDULER: return Q.scheduler->get_scheduler_attribute(o, n, a);
    case SAI_OBJECT_TYPE_SCHEDULER_GROUP: return Q.sched_group->get_scheduler_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_WRED: return Q.wred->get_wred_attribute(o, n, a);
    case SAI_OBJECT_TYPE_BUFFER_POOL: return Q.buffer->get_buffer_pool_attribute(o, n, a);
    case SAI_OBJECT_TYPE_BUFFER_PROFILE: return Q.buffer->get_buffer_profile_attribute(o, n, a);
    case SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP: return Q.buffer->get_ingress_priority_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_QUEUE: return Q.queue->get_queue_attribute(o, n, a);
    case SAI_OBJECT_TYPE_MIRROR_SESSION: return Q.mirror->get_mirror_session_attribute(o, n, a);
    case SAI_OBJECT_TYPE_SAMPLEPACKET: return Q.samplepacket->get_samplepacket_attribute(o, n, a);
    case SAI_OBJECT_TYPE_SWITCH: return A.sw->get_switch_attribute(o, n, a);
    case SAI_OBJECT_TYPE_PORT: return A.port->get_port_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF: return A.hostif->get_hostif_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP: return A.hostif->get_hostif_trap_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HOSTIF_TRAP: return A.hostif->get_hostif_trap_attribute(o, n, a);
    case SAI_OBJECT_TYPE_POLICER: return A.policer->get_policer_attribute(o, n, a);
    case SAI_OBJECT_TYPE_BRIDGE_PORT: return A.bridge->get_bridge_port_attribute(o, n, a);
    case SAI_OBJECT_TYPE_VLAN: return A.vlan->get_vlan_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ROUTER_INTERFACE: return A.rif->get_router_interface_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ACL_TABLE: return A.acl->get_acl_table_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ACL_ENTRY: return A.acl->get_acl_entry_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ACL_COUNTER: return A.acl->get_acl_counter_attribute(o, n, a);
    case SAI_OBJECT_TYPE_ACL_TABLE_GROUP: return A.acl->get_acl_table_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_VIRTUAL_ROUTER: return A.vr->get_virtual_router_attribute(o, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP: return A.nhg->get_next_hop_group_attribute(o, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER: return A.nhg->get_next_hop_group_member_attribute(o, n, a);
    case SAI_OBJECT_TYPE_NEXT_HOP: return A.nexthop->get_next_hop_attribute(o, n, a);
    case SAI_OBJECT_TYPE_BRIDGE: return A.bridge->get_bridge_attribute(o, n, a);
    case SAI_OBJECT_TYPE_LAG: return A.lag->get_lag_attribute(o, n, a);
    case SAI_OBJECT_TYPE_LAG_MEMBER: return A.lag->get_lag_member_attribute(o, n, a);
    case SAI_OBJECT_TYPE_HASH: return A.hash->get_hash_attribute(o, n, a);
    default: return SAI_STATUS_NOT_SUPPORTED;
    }
}

static int g_failed = 0;
static void run(const json& steps, int depth);

static void step(const json& s, size_t idx, int depth)
{
    std::string op = s.at("op").get<std::string>();
    std::string desc = s.value("desc", std::string());
    std::string tag = std::to_string(depth) + "." + std::to_string(idx) + " " + op + (desc.empty() ? "" : " " + desc);
    if (op == "repeat") {
        int n = s.at("n").get<int>();
        for (int i = 0; i < n; i++) run(s.at("steps"), depth + 1);
        return;
    }
    if (op == "foreach") {   // {"op":"foreach","list":"ports","as":"p","steps":[...]}: $p = each element
        auto it = g_lists.find(s.at("list").get<std::string>());
        if (it == g_lists.end()) throw Fail{"no list " + s.at("list").get<std::string>()};
        size_t n = s.value("max", it->second.size());
        for (size_t i = 0; i < it->second.size() && i < n; i++) {
            g_obj[s.at("as").get<std::string>()] = it->second[i];
            run(s.at("steps"), depth + 1);
        }
        return;
    }
    if (op == "sleep") {
        std::this_thread::sleep_for(std::chrono::milliseconds(s.at("ms").get<int>()));
        return;
    }
    if (op == "print") { printf("%s\n", s.at("text").get<std::string>().c_str()); return; }
    if (op == "exec") {   // a shell command while the switch is up (frame tests); its output goes to our stdout
        std::string cmd = s.at("cmd").get<std::string>();
        mark("exec " + cmd);
        fflush(stdout);
        int rc = std::system(cmd.c_str());
        printf("EXEC rc=%d %s\n", rc, cmd.c_str());
        fflush(stdout);
        if (rc != 0 && !s.value("may_fail", false)) g_failed++;
        return;
    }
    if (op == "diag") {   // one command on the SAI's diag socket (SAI_OPENBCM_DIAG_SOCKET)
        auto it = g_profile.find("SAI_OPENBCM_DIAG_SOCKET");
        if (it == g_profile.end()) throw Fail{"diag: no SAI_OPENBCM_DIAG_SOCKET in the profile"};
        int fd = socket(AF_UNIX, SOCK_STREAM, 0);
        sockaddr_un sa;
        std::memset(&sa, 0, sizeof(sa));
        sa.sun_family = AF_UNIX;
        std::snprintf(sa.sun_path, sizeof(sa.sun_path), "%s", it->second.c_str());
        if (fd < 0 || connect(fd, (sockaddr*)&sa, sizeof(sa)) != 0) throw Fail{"diag: cannot connect " + it->second};
        std::string cmd = s.at("cmd").get<std::string>() + "\n";
        (void)!write(fd, cmd.data(), cmd.size());
        shutdown(fd, SHUT_WR);
        char b[4096];
        ssize_t n;
        printf("DIAG %s", cmd.c_str());
        while ((n = read(fd, b, sizeof(b))) > 0) fwrite(b, 1, (size_t)n, stdout);
        close(fd);
        fflush(stdout);
        return;
    }
    Storage st;
    std::vector<sai_attribute_t> attrs;
    sai_object_type_t t = SAI_OBJECT_TYPE_NULL;
    sai_object_id_t target = SAI_NULL_OBJECT_ID;
    if (s.contains("type")) t = type_of(s["type"].get<std::string>());
    if (s.contains("obj")) {
        try {
            target = oid(s["obj"]);
        } catch (Fail& f) {   // an object an earlier may_fail step did not produce
            if (!s.value("may_fail", false)) throw;
            printf("STEP %-40s skipped: %s\n", tag.c_str(), f.what.c_str());
            return;
        }
        t = sai_object_type_query(target);
        if (t == SAI_OBJECT_TYPE_NULL && s["obj"] == "switch") t = SAI_OBJECT_TYPE_SWITCH;
    }
    if (op == "create_switch") t = SAI_OBJECT_TYPE_SWITCH;
    if (op == "create_route" || op == "remove_route" || op == "set_route") t = SAI_OBJECT_TYPE_ROUTE_ENTRY;
    if (op == "create_neighbor" || op == "remove_neighbor" || op == "set_neighbor") t = SAI_OBJECT_TYPE_NEIGHBOR_ENTRY;
    if (op == "route_fill") t = SAI_OBJECT_TYPE_ROUTE_ENTRY;
    if (op == "create_fdb" || op == "remove_fdb" || op == "set_fdb") t = SAI_OBJECT_TYPE_FDB_ENTRY;
    if (op == "flush_fdb") t = SAI_OBJECT_TYPE_FDB_FLUSH;
    for (auto& a : s.value("attrs", json::array())) attrs.push_back(make_attr(t, a, st));
    if (s.contains("attr")) attrs.push_back(make_attr(t, s["attr"], st));

    sai_status_t r = SAI_STATUS_SUCCESS;
    std::string out;
    mark("step " + tag);
    double t0 = now_ms();
    if (op == "create_switch") {
        r = A.sw->create_switch(&g_switch, (uint32_t)attrs.size(), attrs.data());
    } else if (op == "remove_switch") {
        r = A.sw->remove_switch(g_switch);
    } else if (op == "create") {
        sai_object_id_t o = SAI_NULL_OBJECT_ID;
        r = do_create(t, &o, (uint32_t)attrs.size(), attrs.data());
        if (s.contains("as")) g_obj[s["as"].get<std::string>()] = o;
        char b[64];
        snprintf(b, sizeof(b), "0x%llx", (unsigned long long)o);
        out = b;
    } else if (op == "remove") {
        r = do_remove(target);
    } else if (op == "set") {
        r = do_set(target, attrs.data());
    } else if (op == "get") {
        r = do_get(target, (uint32_t)attrs.size(), attrs.data());
        auto list = s.value("attrs", json::array());
        if (s.contains("attr")) list.push_back(s["attr"]);
        for (size_t i = 0; r == SAI_STATUS_SUCCESS && i < attrs.size(); i++) {
            out += (i ? " " : "") + show(t, attrs[i], list[i][1].get<std::string>());
            if ((list[i][1] == "oidlist" || list[i][1] == "oids") && s.contains("as"))
                g_lists[s["as"].get<std::string>()].assign(attrs[i].value.objlist.list,
                                                           attrs[i].value.objlist.list + attrs[i].value.objlist.count);
            if (list[i][1] == "oid" && s.contains("as")) g_obj[s["as"].get<std::string>()] = attrs[i].value.oid;
        }
    } else if (op == "qstats") {   // queue / ingress priority group / buffer pool counters
        std::vector<sai_stat_id_t> ids;
        for (auto& x : s.at("ids")) {
            int64_t v;
            bool named = x.is_string() &&
                         (t == SAI_OBJECT_TYPE_QUEUE ? lookup(k_queue_stats, x.get<std::string>(), v)
                          : t == SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP ? lookup(k_ipg_stats, x.get<std::string>(), v)
                          : lookup(k_pool_stats, x.get<std::string>(), v));
            if (!named) v = num(x);
            ids.push_back((sai_stat_id_t)v);
        }
        std::vector<uint64_t> vals(ids.size());
        if (t == SAI_OBJECT_TYPE_QUEUE)
            r = Q.queue->get_queue_stats(target, (uint32_t)ids.size(), ids.data(), vals.data());
        else if (t == SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP)
            r = Q.buffer->get_ingress_priority_group_stats(target, (uint32_t)ids.size(), ids.data(), vals.data());
        else
            r = Q.buffer->get_buffer_pool_stats(target, (uint32_t)ids.size(), ids.data(), vals.data());
        for (size_t i = 0; r == SAI_STATUS_SUCCESS && i < vals.size(); i++) out += (i ? " " : "") + std::to_string(vals[i]);
    } else if (op == "stats") {   // port or router interface counters
        bool rif = t == SAI_OBJECT_TYPE_ROUTER_INTERFACE;
        std::vector<sai_stat_id_t> ids;
        for (auto& x : s.at("ids")) {
            int64_t v;
            if (!x.is_string() || !(rif ? lookup(k_rif_stats, x.get<std::string>(), v) : lookup(k_port_stats, x.get<std::string>(), v)))
                v = num(x);
            ids.push_back((sai_stat_id_t)v);
        }
        std::vector<uint64_t> vals(ids.size());
        r = rif ? A.rif->get_router_interface_stats(target, (uint32_t)ids.size(), ids.data(), vals.data())
                : A.port->get_port_stats(target, (uint32_t)ids.size(), ids.data(), vals.data());
        for (size_t i = 0; r == SAI_STATUS_SUCCESS && i < vals.size(); i++) out += (i ? " " : "") + std::to_string(vals[i]);
    } else if (op == "stats_cap") {   // {"op":"stats_cap","type":"PORT"}: stat ids (id:modes) the SAI reports
        std::vector<sai_stat_capability_t> caps(512);
        sai_stat_capability_list_t l;
        l.count = (uint32_t)caps.size();
        l.list = caps.data();
        r = sai_query_stats_capability(g_switch, t, &l);
        for (uint32_t i = 0; r == SAI_STATUS_SUCCESS && i < l.count; i++)
            out += (i ? " " : "") + std::to_string(caps[i].stat_enum) + ":" + std::to_string(caps[i].stat_modes);
    } else if (op == "create_route" || op == "remove_route" || op == "set_route") {
        sai_route_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.vr_id = oid(s.at("vr"));
        e.destination = parse_prefix(s.at("prefix").get<std::string>());
        if (op == "create_route") r = A.route->create_route_entry(&e, (uint32_t)attrs.size(), attrs.data());
        else if (op == "set_route") r = A.route->set_route_entry_attribute(&e, attrs.data());
        else r = A.route->remove_route_entry(&e);
    } else if (op == "create_neighbor" || op == "remove_neighbor" || op == "set_neighbor") {
        sai_neighbor_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.rif_id = oid(s.at("rif"));
        e.ip_address = parse_ip(s.at("ip").get<std::string>());
        if (op == "create_neighbor") r = A.neighbor->create_neighbor_entry(&e, (uint32_t)attrs.size(), attrs.data());
        else if (op == "set_neighbor") r = A.neighbor->set_neighbor_entry_attribute(&e, attrs.data());
        else r = A.neighbor->remove_neighbor_entry(&e);
    } else if (op == "route_fill") {
        // {"op":"route_fill","vr":..,"prefix":"20.0.0.0/24","count":N,"attrs":[..],"remove":false,"report":1000}
        // N routes from prefix upwards (each the next prefix of that length); create (or remove) them one
        // by one, a FILL line per report block (time per block), stop at the first failure (its status is
        // the step's). Only begin/end markers go into the trace.
        sai_route_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.vr_id = oid(s.at("vr"));
        e.destination = parse_prefix(s.at("prefix").get<std::string>());
        size_t count = s.at("count").get<size_t>(), report = s.value("report", (size_t)1000), done = 0;
        bool rm = s.value("remove", false), v6 = e.destination.addr_family == SAI_IP_ADDR_FAMILY_IPV6;
        int len = 0;
        for (int i = 0; i < (v6 ? 16 : 4); i++)
            for (int b = 7; b >= 0; b--) len += ((v6 ? e.destination.mask.ip6[i] : ((uint8_t*)&e.destination.mask.ip4)[i]) >> b) & 1;
        double tb = now_ms();
        for (; done < count; done++) {
            r = rm ? A.route->remove_route_entry(&e) : A.route->create_route_entry(&e, (uint32_t)attrs.size(), attrs.data());
            if (r != SAI_STATUS_SUCCESS) break;
            if ((done + 1) % report == 0) {
                double t = now_ms();
                printf("FILL %s n=%zu block_ms=%.1f us_per_route=%.1f\n", rm ? "remove" : "create", done + 1, t - tb,
                       (t - tb) * 1000.0 / (double)report);
                fflush(stdout);
                tb = t;
            }
            // next prefix: add 1 at bit (len - 1), counted from the top
            uint8_t* a = v6 ? e.destination.addr.ip6 : (uint8_t*)&e.destination.addr.ip4;
            int bit = len - 1, carry = 1;
            for (int byte = bit / 8; byte >= 0 && carry; byte--) {
                int sh = byte == bit / 8 ? 7 - bit % 8 : 0;
                unsigned v = (unsigned)a[byte] + (1u << sh);
                a[byte] = (uint8_t)v;
                carry = v > 0xff;
            }
        }
        char fb[160];
        snprintf(fb, sizeof(fb), "%s %zu of %zu (stopped: status %d)", rm ? "removed" : "created", done, count, (int)r);
        out = fb;
        printf("FILL done %s\n", fb);
    } else if (op == "create_fdb" || op == "remove_fdb" || op == "set_fdb") {
        // {"op":"create_fdb","vlan":"$vlan","mac":"02:..","attrs":[...]}
        sai_fdb_entry_t e;
        std::memset(&e, 0, sizeof(e));
        e.switch_id = g_switch;
        e.bv_id = oid(s.at("vlan"));
        parse_mac(s.at("mac").get<std::string>(), e.mac_address);
        if (op == "create_fdb") r = A.fdb->create_fdb_entry(&e, (uint32_t)attrs.size(), attrs.data());
        else if (op == "set_fdb") r = A.fdb->set_fdb_entry_attribute(&e, attrs.data());
        else r = A.fdb->remove_fdb_entry(&e);
    } else if (op == "flush_fdb") {
        r = A.fdb->flush_fdb_entries(g_switch, (uint32_t)attrs.size(), attrs.data());
    } else {
        throw Fail{"unknown op " + op};
    }
    double dt = now_ms() - t0;
    char b[64];
    snprintf(b, sizeof(b), " status=%d ms=%.3f", (int)r, dt);
    mark("end " + tag + b);
    printf("STEP %-40s %d %10.3f ms %s\n", tag.c_str(), (int)r, dt, out.c_str());
    fflush(stdout);
    bool expect_fail = s.value("may_fail", false);
    if (r != SAI_STATUS_SUCCESS && !expect_fail) g_failed++;
}

static void run(const json& steps, int depth)
{
    for (size_t i = 0; i < steps.size(); i++) step(steps[i], i, depth);
}

int main(int argc, char** argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: %s PROFILE SCRIPT.json\n", argv[0]);
        return 2;
    }
    std::ifstream pf(argv[1]);
    std::string line;
    while (std::getline(pf, line)) {
        auto eq = line.find('=');
        if (line.empty() || line[0] == '#' || eq == std::string::npos) continue;
        g_profile[line.substr(0, eq)] = line.substr(eq + 1);
    }
    json script;
    try {
        std::ifstream sf(argv[2]);
        sf >> script;
    } catch (std::exception& e) {
        fprintf(stderr, "%s: %s\n", argv[2], e.what());
        return 2;
    }
    if (const char* f = getenv("IFCS_TRACE_FILE")) g_mark = open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
    mark("script " + script.value("name", std::string(argv[2])));
    static sai_service_method_table_t svc = {prof_get, prof_next};
    sai_status_t r = sai_api_initialize(0, &svc);
    if (r != SAI_STATUS_SUCCESS) { fprintf(stderr, "sai_api_initialize %d\n", (int)r); return 1; }
    sai_api_query(SAI_API_SWITCH, (void**)&A.sw);
    sai_api_query(SAI_API_PORT, (void**)&A.port);
    sai_api_query(SAI_API_HOSTIF, (void**)&A.hostif);
    sai_api_query(SAI_API_POLICER, (void**)&A.policer);
    sai_api_query(SAI_API_VLAN, (void**)&A.vlan);
    sai_api_query(SAI_API_ROUTER_INTERFACE, (void**)&A.rif);
    sai_api_query(SAI_API_ROUTE, (void**)&A.route);
    sai_api_query(SAI_API_NEIGHBOR, (void**)&A.neighbor);
    sai_api_query(SAI_API_NEXT_HOP, (void**)&A.nexthop);
    sai_api_query(SAI_API_BRIDGE, (void**)&A.bridge);
    sai_api_query(SAI_API_VIRTUAL_ROUTER, (void**)&A.vr);
    sai_api_query(SAI_API_ACL, (void**)&A.acl);
    sai_api_query(SAI_API_QOS_MAP, (void**)&Q.qos_map);
    sai_api_query(SAI_API_SCHEDULER, (void**)&Q.scheduler);
    sai_api_query(SAI_API_SCHEDULER_GROUP, (void**)&Q.sched_group);
    sai_api_query(SAI_API_WRED, (void**)&Q.wred);
    sai_api_query(SAI_API_BUFFER, (void**)&Q.buffer);
    sai_api_query(SAI_API_QUEUE, (void**)&Q.queue);
    sai_api_query(SAI_API_MIRROR, (void**)&Q.mirror);
    sai_api_query(SAI_API_SAMPLEPACKET, (void**)&Q.samplepacket);
    sai_api_query(SAI_API_NEXT_HOP_GROUP, (void**)&A.nhg);
    sai_api_query(SAI_API_LAG, (void**)&A.lag);
    sai_api_query(SAI_API_FDB, (void**)&A.fdb);
    sai_api_query(SAI_API_HASH, (void**)&A.hash);
    int rc = 0;
    try {
        run(script.at("steps"), 0);
    } catch (Fail& f) {
        fprintf(stderr, "script error: %s\n", f.what.c_str());
        rc = 2;
    } catch (std::exception& e) {
        fprintf(stderr, "script error: %s\n", e.what());
        rc = 2;
    }
    mark("done failed=" + std::to_string(g_failed));
    sai_api_uninitialize();
    printf("sai-script: %d failed steps\n", g_failed);
    return rc ? rc : g_failed ? 1 : 0;
}

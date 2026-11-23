// SPDX-License-Identifier: Apache-2.0
// Attribute capability table for sai_query_attribute_capability(). Lists
// what the object files implement with hardware effect or as real state;
// "stubbed" attributes (stored, not programmed) are not advertised here.
// tests/test_sai.cpp checks every get-capable entry against a live object.
#include "sai_internal.h"

namespace ob {

#define C(ot, a, cr, se, ge) {SAI_OBJECT_TYPE_##ot, ot##_ATTR(a), cr, se, ge}
#define SWITCH_ATTR(a) SAI_SWITCH_ATTR_##a
#define PORT_ATTR(a) SAI_PORT_ATTR_##a
#define VLAN_ATTR(a) SAI_VLAN_ATTR_##a
#define VLAN_MEMBER_ATTR(a) SAI_VLAN_MEMBER_ATTR_##a
#define BRIDGE_ATTR(a) SAI_BRIDGE_ATTR_##a
#define BRIDGE_PORT_ATTR(a) SAI_BRIDGE_PORT_ATTR_##a
#define FDB_ENTRY_ATTR(a) SAI_FDB_ENTRY_ATTR_##a
#define HOSTIF_ATTR(a) SAI_HOSTIF_ATTR_##a
#define HOSTIF_TRAP_ATTR(a) SAI_HOSTIF_TRAP_ATTR_##a
#define HOSTIF_TRAP_GROUP_ATTR(a) SAI_HOSTIF_TRAP_GROUP_ATTR_##a
#define ROUTER_INTERFACE_ATTR(a) SAI_ROUTER_INTERFACE_ATTR_##a
#define NEIGHBOR_ENTRY_ATTR(a) SAI_NEIGHBOR_ENTRY_ATTR_##a
#define NEXT_HOP_ATTR(a) SAI_NEXT_HOP_ATTR_##a
#define ROUTE_ENTRY_ATTR(a) SAI_ROUTE_ENTRY_ATTR_##a
#define VIRTUAL_ROUTER_ATTR(a) SAI_VIRTUAL_ROUTER_ATTR_##a
#define NEXT_HOP_GROUP_ATTR(a) SAI_NEXT_HOP_GROUP_ATTR_##a
#define NEXT_HOP_GROUP_MEMBER_ATTR(a) SAI_NEXT_HOP_GROUP_MEMBER_ATTR_##a
#define QUEUE_ATTR(a) SAI_QUEUE_ATTR_##a
#define STP_ATTR(a) SAI_STP_ATTR_##a
#define LAG_ATTR(a) SAI_LAG_ATTR_##a
#define LAG_MEMBER_ATTR(a) SAI_LAG_MEMBER_ATTR_##a
#define POLICER_ATTR(a) SAI_POLICER_ATTR_##a
#define ACL_TABLE_ATTR(a) SAI_ACL_TABLE_ATTR_##a
#define ACL_ENTRY_ATTR(a) SAI_ACL_ENTRY_ATTR_##a
#define ACL_COUNTER_ATTR(a) SAI_ACL_COUNTER_ATTR_##a
#define ACL_TABLE_GROUP_ATTR(a) SAI_ACL_TABLE_GROUP_ATTR_##a
#define ACL_TABLE_GROUP_MEMBER_ATTR(a) SAI_ACL_TABLE_GROUP_MEMBER_ATTR_##a
// ACL match fields (sai_acl.cpp field_qual): table create/get, entry create/set/get.
#define ACL_FIELD(f) C(ACL_TABLE, FIELD_##f, true, false, true), C(ACL_ENTRY, FIELD_##f, true, true, true)

const std::vector<AttrCap>& attr_caps()
{
    static const std::vector<AttrCap> caps = {
        C(SWITCH, INIT_SWITCH, true, false, true),
        C(SWITCH, TYPE, true, false, true),
        C(SWITCH, SRC_MAC_ADDRESS, true, true, true),
        C(SWITCH, PORT_NUMBER, false, false, true),
        C(SWITCH, PORT_LIST, false, false, true),
        C(SWITCH, CPU_PORT, false, false, true),
        C(SWITCH, DEFAULT_VIRTUAL_ROUTER_ID, false, false, true),
        C(SWITCH, DEFAULT_VLAN_ID, false, false, true),
        C(SWITCH, DEFAULT_1Q_BRIDGE_ID, false, false, true),
        C(SWITCH, DEFAULT_STP_INST_ID, false, false, true),
        C(SWITCH, DEFAULT_TRAP_GROUP, false, false, true),
        C(SWITCH, PORT_MAX_MTU, false, false, true),
        C(SWITCH, OPER_STATUS, false, false, true),
        C(SWITCH, MAX_NUMBER_OF_SUPPORTED_PORTS, false, false, true),
        C(SWITCH, QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES, false, false, true),
        C(SWITCH, NUMBER_OF_QUEUES, false, false, true),
        C(SWITCH, NUMBER_OF_UNICAST_QUEUES, false, false, true),
        C(SWITCH, NUMBER_OF_MULTICAST_QUEUES, false, false, true),
        C(SWITCH, FDB_AGING_TIME, true, true, true),
        C(SWITCH, PORT_STATE_CHANGE_NOTIFY, true, true, true),
        C(SWITCH, FDB_EVENT_NOTIFY, true, true, true),
        C(SWITCH, SHUTDOWN_REQUEST_NOTIFY, true, true, true),
        C(SWITCH, SWITCH_STATE_CHANGE_NOTIFY, true, true, true),
        C(SWITCH, MAX_NUMBER_OF_TEMP_SENSORS, false, false, true),
        C(SWITCH, TEMP_LIST, false, false, true),
        C(SWITCH, MAX_TEMP, false, false, true),
        C(SWITCH, AVERAGE_TEMP, false, false, true),
        C(SWITCH, ACL_ENTRY_MINIMUM_PRIORITY, false, false, true),
        C(SWITCH, ACL_ENTRY_MAXIMUM_PRIORITY, false, false, true),
        C(SWITCH, MAX_ACL_ACTION_COUNT, false, false, true),
        C(SWITCH, ACL_STAGE_INGRESS, false, false, true),
        C(SWITCH, ACL_STAGE_EGRESS, false, false, true),
        C(SWITCH, NUMBER_OF_ECMP_GROUPS, false, false, true),
        C(SWITCH, ECMP_MEMBERS, false, false, true),
        C(SWITCH, MAX_ECMP_MEMBER_COUNT, false, false, true),
        C(SWITCH, TOTAL_BUFFER_SIZE, false, false, true),
        C(SWITCH, AVAILABLE_IPV4_ROUTE_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_IPV6_ROUTE_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_IPV4_NEXTHOP_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_IPV6_NEXTHOP_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_IPV4_NEIGHBOR_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_IPV6_NEIGHBOR_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_NEXT_HOP_GROUP_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY, false, false, true),
        C(SWITCH, AVAILABLE_FDB_ENTRY, false, false, true),

        C(PORT, TYPE, false, false, true),
        C(PORT, HW_LANE_LIST, false, false, true),
        C(PORT, ADMIN_STATE, false, true, true),
        C(PORT, OPER_STATUS, false, false, true),
        C(PORT, SPEED, false, true, true),
        C(PORT, OPER_SPEED, false, false, true),
        C(PORT, SUPPORTED_SPEED, false, false, true),
        C(PORT, MTU, false, true, true),
        C(PORT, FEC_MODE, false, true, true),
        C(PORT, SUPPORTED_FEC_MODE, false, false, true),
        C(PORT, AUTO_NEG_MODE, false, true, true),
        C(PORT, SUPPORTED_AUTO_NEG_MODE, false, false, true),
        C(PORT, PORT_VLAN_ID, false, true, true),
        C(PORT, LOOPBACK_MODE, false, true, true),
        C(PORT, QOS_NUMBER_OF_QUEUES, false, false, true),
        C(PORT, QOS_QUEUE_LIST, false, false, true),
        C(PORT, NUMBER_OF_INGRESS_PRIORITY_GROUPS, false, false, true),
        C(PORT, INGRESS_PRIORITY_GROUP_LIST, false, false, true),

        C(LAG, PORT_LIST, false, false, true),
        C(LAG, PORT_VLAN_ID, true, true, true),
        C(LAG_MEMBER, LAG_ID, true, false, true),
        C(LAG_MEMBER, PORT_ID, true, false, true),
        C(LAG_MEMBER, EGRESS_DISABLE, true, true, true),
        C(LAG_MEMBER, INGRESS_DISABLE, true, true, true),

        C(VLAN, VLAN_ID, true, false, true),
        C(VLAN, MEMBER_LIST, false, false, true),
        C(VLAN_MEMBER, VLAN_ID, true, false, true),
        C(VLAN_MEMBER, BRIDGE_PORT_ID, true, false, true),
        C(VLAN_MEMBER, VLAN_TAGGING_MODE, true, true, true),
        C(BRIDGE, TYPE, false, false, true),
        C(BRIDGE, PORT_LIST, false, false, true),
        C(BRIDGE_PORT, TYPE, true, false, true),
        C(BRIDGE_PORT, PORT_ID, true, false, true),
        C(BRIDGE_PORT, BRIDGE_ID, true, false, true),
        C(BRIDGE_PORT, FDB_LEARNING_MODE, true, true, true),
        C(FDB_ENTRY, TYPE, true, true, true),
        C(FDB_ENTRY, BRIDGE_PORT_ID, true, true, true),
        C(FDB_ENTRY, PACKET_ACTION, true, true, true),
        C(STP, VLAN_LIST, false, false, true),
        C(QUEUE, TYPE, false, false, true),
        C(QUEUE, SCHEDULER_PROFILE_ID, false, true, true),
        C(QUEUE, WRED_PROFILE_ID, false, true, true),
        C(QUEUE, BUFFER_PROFILE_ID, false, true, true),
        // QoS / mirror (sai_qos.cpp, sai_mirror.cpp; programmed when the backend has them)
        C(SWITCH, QOS_DSCP_TO_TC_MAP, false, true, true),
        C(SWITCH, QOS_DOT1P_TO_TC_MAP, false, true, true),
        C(SWITCH, QOS_TC_TO_QUEUE_MAP, false, true, true),
        C(PORT, QOS_DSCP_TO_TC_MAP, false, true, true),
        C(PORT, QOS_DOT1P_TO_TC_MAP, false, true, true),
        C(PORT, QOS_TC_TO_QUEUE_MAP, false, true, true),
        C(PORT, QOS_TC_TO_PRIORITY_GROUP_MAP, false, true, true),
        C(PORT, QOS_PFC_PRIORITY_TO_QUEUE_MAP, false, true, true),
        C(PORT, QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP, false, true, true),
        C(PORT, QOS_DEFAULT_TC, false, true, true),
        C(PORT, PRIORITY_FLOW_CONTROL, false, true, true),
        C(PORT, PRIORITY_FLOW_CONTROL_MODE, false, true, true),
        C(PORT, QOS_SCHEDULER_PROFILE_ID, false, true, true),
        C(QUEUE, INDEX, false, false, true),
        C(QUEUE, PORT, false, false, true),

        C(HOSTIF, TYPE, true, false, true),
        C(HOSTIF, OBJ_ID, true, false, true),
        C(HOSTIF, NAME, true, false, true),
        C(HOSTIF, OPER_STATUS, true, true, true),
        C(HOSTIF, VLAN_TAG, true, true, true),
        C(HOSTIF_TRAP, TRAP_TYPE, true, false, true),
        C(HOSTIF_TRAP, PACKET_ACTION, true, true, true),
        C(HOSTIF_TRAP_GROUP, QUEUE, true, true, true),
        C(HOSTIF_TRAP_GROUP, POLICER, true, true, true),
        C(PORT, INGRESS_ACL, false, true, true),
        C(LAG, INGRESS_ACL, true, true, true),

        C(POLICER, METER_TYPE, true, false, true),
        C(POLICER, MODE, true, false, true),
        C(POLICER, COLOR_SOURCE, true, false, true),
        C(POLICER, CBS, true, true, true),
        C(POLICER, CIR, true, true, true),
        C(POLICER, PBS, true, true, true),
        C(POLICER, PIR, true, true, true),
        C(POLICER, GREEN_PACKET_ACTION, true, true, true),
        C(POLICER, YELLOW_PACKET_ACTION, true, true, true),
        C(POLICER, RED_PACKET_ACTION, true, true, true),

        C(ACL_TABLE, ACL_STAGE, true, false, true),
        C(ACL_TABLE, ACL_BIND_POINT_TYPE_LIST, true, false, true),
        C(ACL_TABLE, ACL_ACTION_TYPE_LIST, true, false, true),
        C(ACL_TABLE, SIZE, true, false, true),
        C(ACL_TABLE, ENTRY_LIST, false, false, true),
        ACL_FIELD(SRC_IP), ACL_FIELD(DST_IP), ACL_FIELD(SRC_IPV6), ACL_FIELD(DST_IPV6),
        ACL_FIELD(L4_SRC_PORT), ACL_FIELD(L4_DST_PORT), ACL_FIELD(IP_PROTOCOL), ACL_FIELD(IPV6_NEXT_HEADER),
        ACL_FIELD(ETHER_TYPE), ACL_FIELD(TCP_FLAGS), ACL_FIELD(ICMP_TYPE), ACL_FIELD(ICMP_CODE),
        ACL_FIELD(ICMPV6_TYPE), ACL_FIELD(ICMPV6_CODE), ACL_FIELD(DSCP), ACL_FIELD(ECN), ACL_FIELD(TOS),
        ACL_FIELD(ACL_IP_TYPE), ACL_FIELD(SRC_MAC), ACL_FIELD(DST_MAC), ACL_FIELD(OUTER_VLAN_ID),
        ACL_FIELD(TTL), ACL_FIELD(IN_PORTS), ACL_FIELD(IN_PORT),
        C(ACL_TABLE, FIELD_ACL_RANGE_TYPE, true, false, true),   // kept; ACL_RANGE objects are not supported
        C(ACL_ENTRY, TABLE_ID, true, false, true),
        C(ACL_ENTRY, PRIORITY, true, true, true),
        C(ACL_ENTRY, ADMIN_STATE, true, true, true),
        C(ACL_ENTRY, ACTION_PACKET_ACTION, true, true, true),
        C(ACL_ENTRY, ACTION_COUNTER, true, true, true),
        C(ACL_ENTRY, ACTION_SET_POLICER, true, true, true),
        C(ACL_COUNTER, TABLE_ID, true, false, true),
        C(ACL_COUNTER, ENABLE_PACKET_COUNT, true, false, true),
        C(ACL_COUNTER, ENABLE_BYTE_COUNT, true, false, true),
        C(ACL_COUNTER, PACKETS, true, true, true),
        C(ACL_COUNTER, BYTES, true, true, true),
        C(ACL_TABLE_GROUP, ACL_STAGE, true, false, true),
        C(ACL_TABLE_GROUP, ACL_BIND_POINT_TYPE_LIST, true, false, true),
        C(ACL_TABLE_GROUP, TYPE, true, false, true),
        C(ACL_TABLE_GROUP, MEMBER_LIST, false, false, true),
        C(ACL_TABLE_GROUP_MEMBER, ACL_TABLE_GROUP_ID, true, false, true),
        C(ACL_TABLE_GROUP_MEMBER, ACL_TABLE_ID, true, false, true),
        C(ACL_TABLE_GROUP_MEMBER, PRIORITY, true, false, true),

        C(VIRTUAL_ROUTER, ADMIN_V4_STATE, true, true, true),
        C(VIRTUAL_ROUTER, ADMIN_V6_STATE, true, true, true),
        C(VIRTUAL_ROUTER, SRC_MAC_ADDRESS, true, true, true),
        C(ROUTER_INTERFACE, VIRTUAL_ROUTER_ID, true, false, true),
        C(ROUTER_INTERFACE, TYPE, true, false, true),
        C(ROUTER_INTERFACE, PORT_ID, true, false, true),
        C(ROUTER_INTERFACE, VLAN_ID, true, false, true),
        C(ROUTER_INTERFACE, SRC_MAC_ADDRESS, true, true, true),
        C(ROUTER_INTERFACE, MTU, true, true, true),
        C(NEIGHBOR_ENTRY, DST_MAC_ADDRESS, true, true, true),
        C(NEIGHBOR_ENTRY, NO_HOST_ROUTE, true, false, true),
        C(NEXT_HOP, TYPE, true, false, true),
        C(NEXT_HOP, IP, true, false, true),
        C(NEXT_HOP, ROUTER_INTERFACE_ID, true, false, true),
        C(ROUTE_ENTRY, PACKET_ACTION, true, true, true),
        C(ROUTE_ENTRY, NEXT_HOP_ID, true, true, true),
        C(NEXT_HOP_GROUP, TYPE, true, false, true),
        C(NEXT_HOP_GROUP, NEXT_HOP_COUNT, false, false, true),
        C(NEXT_HOP_GROUP, NEXT_HOP_MEMBER_LIST, false, false, true),
        C(NEXT_HOP_GROUP_MEMBER, NEXT_HOP_GROUP_ID, true, false, true),
        C(NEXT_HOP_GROUP_MEMBER, NEXT_HOP_ID, true, true, true),
        C(NEXT_HOP_GROUP_MEMBER, WEIGHT, true, true, true),
    };
    return caps;
}

} // namespace ob

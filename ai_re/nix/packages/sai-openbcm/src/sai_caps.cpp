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
#define QUEUE_ATTR(a) SAI_QUEUE_ATTR_##a
#define STP_ATTR(a) SAI_STP_ATTR_##a
#define LAG_ATTR(a) SAI_LAG_ATTR_##a
#define LAG_MEMBER_ATTR(a) SAI_LAG_MEMBER_ATTR_##a

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
        C(SWITCH, FDB_AGING_TIME, true, true, true),
        C(SWITCH, PORT_STATE_CHANGE_NOTIFY, true, true, true),
        C(SWITCH, FDB_EVENT_NOTIFY, true, true, true),
        C(SWITCH, SHUTDOWN_REQUEST_NOTIFY, true, true, true),
        C(SWITCH, SWITCH_STATE_CHANGE_NOTIFY, true, true, true),
        C(SWITCH, MAX_NUMBER_OF_TEMP_SENSORS, false, false, true),
        C(SWITCH, TEMP_LIST, false, false, true),
        C(SWITCH, MAX_TEMP, false, false, true),
        C(SWITCH, AVERAGE_TEMP, false, false, true),

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
    };
    return caps;
}

} // namespace ob

//! What the capability queries answer (`sai_query_attribute_capability`,
//! `sai_query_attribute_enum_values_capability`): the tables of the C++ sai-openbcm
//! (`sai_caps.cpp`, `sai_api.cpp`), cut to the object types this library implements. An
//! attribute that is kept without effect is not advertised, as in the C++.
//!
//! `tests/slice.rs` checks every get-capable row of the default objects against a live switch.
//!
//! The table says what the SAI layer can do; [`attribute_for`] and [`enum_values_for`] cut it
//! to what the backend can do: object types whose objects only the backend can program
//! ([`Backend::programs`]) are not advertised for a backend that refuses them.
use crate::backend::Backend;
use crate::error::{Error, Result};
use crate::oid::{EntryType, ObjectType};
use sai_sys as sys;

/// Whether an attribute can be used in create, set and get.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AttrCapability {
    /// May be passed to create, with effect.
    pub create: bool,
    /// May be set, with effect.
    pub set: bool,
    /// Can be read.
    pub get: bool,
}

type Row = (sys::sai_object_type_t, sys::sai_attr_id_t, bool, bool, bool);

/// Object type, attribute, create, set, get.
static ATTRS: [Row; 140] = [
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_INIT_SWITCH,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_PORT_NUMBER,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_PORT_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_CPU_PORT,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_PORT_MAX_MTU,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_OPER_STATUS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_SUPPORTED_PORTS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_NUMBER_OF_QUEUES,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_NUMBER_OF_UNICAST_QUEUES,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_NUMBER_OF_MULTICAST_QUEUES,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_FDB_AGING_TIME,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_SHUTDOWN_REQUEST_NOTIFY,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_SWITCH_STATE_CHANGE_NOTIFY,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_ACL_STAGE_EGRESS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_ECMP_MEMBERS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_TYPE, false, false, true),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_HW_LANE_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_ADMIN_STATE,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_OPER_STATUS,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_SPEED, false, true, true),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_OPER_SPEED,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_SUPPORTED_SPEED,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_MTU, false, true, true),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_FEC_MODE,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_SUPPORTED_FEC_MODE,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_AUTO_NEG_MODE,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_SUPPORTED_AUTO_NEG_MODE,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_PORT_VLAN_ID,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_LOOPBACK_MODE,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_QUEUE_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_NUMBER_OF_INGRESS_PRIORITY_GROUPS,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_VLAN, sys::SAI_VLAN_ATTR_VLAN_ID, true, false, true),
    (
        sys::SAI_OBJECT_TYPE_VLAN,
        sys::SAI_VLAN_ATTR_MEMBER_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
        sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
        sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE,
        sys::SAI_BRIDGE_ATTR_TYPE,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE,
        sys::SAI_BRIDGE_ATTR_PORT_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
        sys::SAI_BRIDGE_PORT_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
        sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
        sys::SAI_BRIDGE_PORT_ATTR_BRIDGE_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
        sys::SAI_BRIDGE_PORT_ATTR_FDB_LEARNING_MODE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_FDB_ENTRY,
        sys::SAI_FDB_ENTRY_ATTR_TYPE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_FDB_ENTRY,
        sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_FDB_ENTRY,
        sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_STP,
        sys::SAI_STP_ATTR_VLAN_LIST,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_QUEUE, sys::SAI_QUEUE_ATTR_TYPE, false, false, true),
    (
        sys::SAI_OBJECT_TYPE_QUEUE,
        sys::SAI_QUEUE_ATTR_SCHEDULER_PROFILE_ID,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_QUEUE,
        sys::SAI_QUEUE_ATTR_WRED_PROFILE_ID,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_QUEUE,
        sys::SAI_QUEUE_ATTR_BUFFER_PROFILE_ID,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_QOS_DOT1P_TO_TC_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_SWITCH,
        sys::SAI_SWITCH_ATTR_QOS_TC_TO_QUEUE_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_QUEUE_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_DEFAULT_TC,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_PORT,
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID,
        false,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_QUEUE,
        sys::SAI_QUEUE_ATTR_INDEX,
        false,
        false,
        true,
    ),
    (sys::SAI_OBJECT_TYPE_QUEUE, sys::SAI_QUEUE_ATTR_PORT, false, false, true),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF,
        sys::SAI_HOSTIF_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF,
        sys::SAI_HOSTIF_ATTR_OBJ_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF,
        sys::SAI_HOSTIF_ATTR_NAME,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF,
        sys::SAI_HOSTIF_ATTR_OPER_STATUS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF,
        sys::SAI_HOSTIF_ATTR_VLAN_TAG,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
        sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
        sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_METER_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_MODE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_COLOR_SOURCE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_CBS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_CIR,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_PBS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_PIR,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_GREEN_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_YELLOW_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_POLICER,
        sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
        sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V4_STATE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
        sys::SAI_VIRTUAL_ROUTER_ATTR_ADMIN_V6_STATE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
        sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
        sys::SAI_ROUTER_INTERFACE_ATTR_MTU,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY,
        sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY,
        sys::SAI_NEIGHBOR_ENTRY_ATTR_NO_HOST_ROUTE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP,
        sys::SAI_NEXT_HOP_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP,
        sys::SAI_NEXT_HOP_ATTR_IP,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP,
        sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTE_ENTRY,
        sys::SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_ROUTE_ENTRY,
        sys::SAI_ROUTE_ENTRY_ATTR_NEXT_HOP_ID,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
        sys::SAI_NEXT_HOP_GROUP_ATTR_TYPE,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
        sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
        sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_ID,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
        sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_WEIGHT,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG,
        sys::SAI_LAG_ATTR_PORT_LIST,
        false,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG,
        sys::SAI_LAG_ATTR_PORT_VLAN_ID,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG_MEMBER,
        sys::SAI_LAG_MEMBER_ATTR_LAG_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG_MEMBER,
        sys::SAI_LAG_MEMBER_ATTR_PORT_ID,
        true,
        false,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG_MEMBER,
        sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE,
        true,
        true,
        true,
    ),
    (
        sys::SAI_OBJECT_TYPE_LAG_MEMBER,
        sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE,
        true,
        true,
        true,
    ),
];

/// Object types the SAI layer implements with its own operations on the backend (ports,
/// VLANs, bridging): their rows do not depend on [`Backend::programs`], except the port
/// attributes that go to [`Backend::object_set`].
fn own(object_type: sys::sai_object_type_t) -> bool {
    matches!(
        ObjectType::from_sai(object_type),
        Some(
            ObjectType::Switch
                | ObjectType::Port
                | ObjectType::Vlan
                | ObjectType::VlanMember
                | ObjectType::Bridge
                | ObjectType::BridgePort
        )
    )
}

/// Object types of which the switch has objects from the start (`crate::sai`'s
/// `build_switch`): readable whatever the backend programs.
fn built_in(object_type: sys::sai_object_type_t) -> bool {
    matches!(
        ObjectType::from_sai(object_type),
        Some(
            ObjectType::VirtualRouter
                | ObjectType::Stp
                | ObjectType::HostifTrapGroup
                | ObjectType::Hash
                | ObjectType::Queue
                | ObjectType::IngressPriorityGroup
                | ObjectType::SchedulerGroup
        )
    )
}

/// Whether an attribute's create / set needs [`Backend::programs`] for its object type.
fn needs_backend(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> bool {
    !own(object_type) || (object_type == sys::SAI_OBJECT_TYPE_PORT && crate::sai::port_attr_programmed(attr))
}

/// [`attribute`] for a library over `backend`: an object type the backend does not program is
/// neither creatable nor settable, and readable only if the switch comes with such objects
/// (ports keep these attributes, so they stay readable).
pub fn attribute_for<B: Backend + ?Sized>(
    backend: &B,
    object_type: sys::sai_object_type_t,
    attr: sys::sai_attr_id_t,
) -> AttrCapability {
    let capability = attribute(object_type, attr);
    if !needs_backend(object_type, attr) || backend.programs(object_type) {
        return capability;
    }
    AttrCapability {
        create: false,
        set: false,
        get: capability.get && (built_in(object_type) || object_type == sys::SAI_OBJECT_TYPE_PORT),
    }
}

/// [`enum_values`] for a library over `backend`: `NotSupported` where the backend does not
/// program the object type.
pub fn enum_values_for<B: Backend + ?Sized>(
    backend: &B,
    object_type: sys::sai_object_type_t,
    attr: sys::sai_attr_id_t,
) -> Result<Vec<i32>> {
    if needs_backend(object_type, attr) && !backend.programs(object_type) {
        return Err(Error::NotSupported);
    }
    enum_values(object_type, attr)
}

/// Whether this library implements the object type at all.
pub fn implemented(object_type: sys::sai_object_type_t) -> bool {
    ObjectType::from_sai(object_type).is_some() || EntryType::from_sai(object_type).is_some()
}

/// `sai_query_attribute_capability`: all false for an attribute that is not in the table
/// (the C++ answers the same way, with success).
pub fn attribute(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> AttrCapability {
    ATTRS
        .iter()
        .find(|row| row.0 == object_type && row.1 == attr)
        .map_or_else(AttrCapability::default, |row| AttrCapability {
            create: row.2,
            set: row.3,
            get: row.4,
        })
}

/// Every row of the table (for tests).
pub fn attributes() -> impl Iterator<Item = (sys::sai_object_type_t, sys::sai_attr_id_t, AttrCapability)> {
    ATTRS.iter().map(|row| {
        (
            row.0,
            row.1,
            AttrCapability {
                create: row.2,
                set: row.3,
                get: row.4,
            },
        )
    })
}

/// `sai_query_attribute_enum_values_capability`: the enumerators of an attribute that are
/// implemented; `NotSupported` for attributes the table does not list.
pub fn enum_values(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> Result<Vec<i32>> {
    let values: &[u32] = match (object_type, attr) {
        (sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_FEC_MODE) => &[
            sys::SAI_PORT_FEC_MODE_NONE,
            sys::SAI_PORT_FEC_MODE_RS,
            sys::SAI_PORT_FEC_MODE_FC,
        ],
        (sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_LOOPBACK_MODE) => &[
            sys::SAI_PORT_LOOPBACK_MODE_NONE,
            sys::SAI_PORT_LOOPBACK_MODE_MAC,
            sys::SAI_PORT_LOOPBACK_MODE_PHY,
        ],
        (sys::SAI_OBJECT_TYPE_HOSTIF, sys::SAI_HOSTIF_ATTR_TYPE) => &[sys::SAI_HOSTIF_TYPE_NETDEV],
        (sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE, sys::SAI_ROUTER_INTERFACE_ATTR_TYPE) => &[
            sys::SAI_ROUTER_INTERFACE_TYPE_PORT,
            sys::SAI_ROUTER_INTERFACE_TYPE_VLAN,
            sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK,
        ],
        (sys::SAI_OBJECT_TYPE_POLICER, sys::SAI_POLICER_ATTR_MODE) => {
            &[sys::SAI_POLICER_MODE_SR_TCM, sys::SAI_POLICER_MODE_TR_TCM]
        }
        (sys::SAI_OBJECT_TYPE_POLICER, sys::SAI_POLICER_ATTR_METER_TYPE) => {
            &[sys::SAI_METER_TYPE_PACKETS, sys::SAI_METER_TYPE_BYTES]
        }
        _ => return Err(Error::NotSupported),
    };
    Ok(values.iter().map(|v| v.cast_signed()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_name_attributes_the_headers_have_and_follow_their_flags() {
        for (object_type, attr, cap) in attributes() {
            let meta = sai_meta::attribute(object_type, attr).unwrap();
            assert!(implemented(object_type), "{}", meta.name);
            // Nothing is advertised that SAI itself forbids.
            assert!(!cap.create || meta.flags.creatable(), "create {}", meta.name);
            assert!(!cap.set || meta.flags.settable(), "set {}", meta.name);
            assert!(cap.get, "{}", meta.name);
        }
        assert_eq!(
            attribute(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_SPEED),
            AttrCapability {
                create: false,
                set: true,
                get: true
            }
        );
        // kept without effect: not advertised
        assert_eq!(
            attribute(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_MEDIA_TYPE),
            AttrCapability::default()
        );
        // an object type this library does not have
        assert_eq!(
            attribute(sys::SAI_OBJECT_TYPE_ACL_TABLE, sys::SAI_ACL_TABLE_ATTR_ACL_STAGE),
            AttrCapability::default()
        );
        assert_eq!(
            enum_values(sys::SAI_OBJECT_TYPE_HOSTIF, sys::SAI_HOSTIF_ATTR_TYPE),
            Ok(vec![sys::SAI_HOSTIF_TYPE_NETDEV.cast_signed()])
        );
        assert_eq!(
            enum_values(sys::SAI_OBJECT_TYPE_PORT, sys::SAI_PORT_ATTR_SPEED),
            Err(Error::NotSupported)
        );
    }
}

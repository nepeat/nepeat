//! The object types a script names (C++ `kTypes`): lower-case name, SAI object type, and what
//! the C++ driver may do with it.
use sai_sys as sys;

/// What a script may do with objects of a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// create / remove / set / get (C++ `OBJ`).
    Object,
    /// create / remove / set / get and counters (C++ `OBJS`).
    Counted,
    /// set / get only.
    Switch,
    /// An entry with a struct key (`<key>@<name>`).
    Entry,
    /// The attributes of `flush` (not a type a script names).
    Flush,
}

/// One object type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjType {
    /// As scripts write it.
    pub name: &'static str,
    /// The SAI object type.
    pub ot: sys::sai_object_type_t,
    /// What may be done.
    pub shape: Shape,
}

impl ObjType {
    /// Has create and remove.
    pub const fn has_create(self) -> bool {
        matches!(self.shape, Shape::Object | Shape::Counted)
    }
    /// Has counters.
    pub const fn has_stats(self) -> bool {
        matches!(self.shape, Shape::Counted)
    }
    /// The upper-case name (`VLAN_MEMBER`), the attribute prefix's middle.
    pub fn upper(self) -> String {
        self.name.to_ascii_uppercase()
    }
}

macro_rules! types {
    ($($name:ident: $ot:ident $shape:ident,)*) => {
        /// Every type, in the C++ driver's order (an unknown id is named after the first type with
        /// its object type).
        pub const TYPES: &[ObjType] = &[
            $(ObjType { name: stringify!($name), ot: sys::$ot, shape: Shape::$shape },)*
        ];
    };
}

types! {
    port: SAI_OBJECT_TYPE_PORT Counted,
    vlan: SAI_OBJECT_TYPE_VLAN Counted,
    vlan_member: SAI_OBJECT_TYPE_VLAN_MEMBER Object,
    bridge: SAI_OBJECT_TYPE_BRIDGE Object,
    bridge_port: SAI_OBJECT_TYPE_BRIDGE_PORT Object,
    stp: SAI_OBJECT_TYPE_STP Object,
    lag: SAI_OBJECT_TYPE_LAG Object,
    lag_member: SAI_OBJECT_TYPE_LAG_MEMBER Object,
    hostif: SAI_OBJECT_TYPE_HOSTIF Object,
    hostif_trap_group: SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP Object,
    hostif_trap: SAI_OBJECT_TYPE_HOSTIF_TRAP Object,
    hostif_table_entry: SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY Object,
    virtual_router: SAI_OBJECT_TYPE_VIRTUAL_ROUTER Object,
    router_interface: SAI_OBJECT_TYPE_ROUTER_INTERFACE Counted,
    next_hop: SAI_OBJECT_TYPE_NEXT_HOP Object,
    next_hop_group: SAI_OBJECT_TYPE_NEXT_HOP_GROUP Object,
    next_hop_group_member: SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER Object,
    policer: SAI_OBJECT_TYPE_POLICER Counted,
    acl_table: SAI_OBJECT_TYPE_ACL_TABLE Object,
    acl_entry: SAI_OBJECT_TYPE_ACL_ENTRY Object,
    acl_counter: SAI_OBJECT_TYPE_ACL_COUNTER Object,
    acl_range: SAI_OBJECT_TYPE_ACL_RANGE Object,
    acl_table_group: SAI_OBJECT_TYPE_ACL_TABLE_GROUP Object,
    acl_table_group_member: SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER Object,
    qos_map: SAI_OBJECT_TYPE_QOS_MAP Object,
    scheduler: SAI_OBJECT_TYPE_SCHEDULER Object,
    scheduler_group: SAI_OBJECT_TYPE_SCHEDULER_GROUP Object,
    wred: SAI_OBJECT_TYPE_WRED Object,
    buffer_pool: SAI_OBJECT_TYPE_BUFFER_POOL Counted,
    buffer_profile: SAI_OBJECT_TYPE_BUFFER_PROFILE Object,
    ingress_priority_group: SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP Counted,
    queue: SAI_OBJECT_TYPE_QUEUE Counted,
    mirror_session: SAI_OBJECT_TYPE_MIRROR_SESSION Object,
    samplepacket: SAI_OBJECT_TYPE_SAMPLEPACKET Object,
    hash: SAI_OBJECT_TYPE_HASH Object,
    macsec: SAI_OBJECT_TYPE_MACSEC Object,
    macsec_port: SAI_OBJECT_TYPE_MACSEC_PORT Object,
    macsec_flow: SAI_OBJECT_TYPE_MACSEC_FLOW Counted,
    macsec_sc: SAI_OBJECT_TYPE_MACSEC_SC Object,
    macsec_sa: SAI_OBJECT_TYPE_MACSEC_SA Counted,
    switch: SAI_OBJECT_TYPE_SWITCH Switch,
    fdb_entry: SAI_OBJECT_TYPE_FDB_ENTRY Entry,
    neighbor_entry: SAI_OBJECT_TYPE_NEIGHBOR_ENTRY Entry,
    route_entry: SAI_OBJECT_TYPE_ROUTE_ENTRY Entry,
}

/// The pseudo-type of `flush`'s attributes (`SAI_FDB_FLUSH_ATTR_*`).
pub const FDB_FLUSH: ObjType = ObjType {
    name: "fdb_flush",
    ot: sys::SAI_OBJECT_TYPE_FDB_FLUSH,
    shape: Shape::Flush,
};

/// The type a script names.
pub fn named(name: &str) -> Option<ObjType> {
    TYPES.iter().copied().find(|t| t.name == name)
}

/// The first type with that SAI object type.
pub fn of(ot: sys::sai_object_type_t) -> Option<ObjType> {
    TYPES.iter().copied().find(|t| t.ot == ot)
}

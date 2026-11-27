//! Object type -> the method table entry, as `sai_script.cpp`'s `do_create` / `do_remove` / `do_set` /
//! `do_get` and the stats calls of both C programs dispatch (the same object types, nothing more).
#![forbid(unsafe_code)]

use crate::ffi::{self, ClearStatsFn, CreateFn, GetFn, Key, Lib, RemoveFn, SetFn, StatsExtFn, StatsFn};
use crate::value::Encoded;
use crate::{Api, Entry, EntryOp, NOT_IMPLEMENTED, Status, sys};

/// A dispatch answer.
pub(crate) enum Slot<F> {
    /// the object type has no such call (the C programs' `default: NOT_SUPPORTED`)
    Missing,
    /// the API was not queried, or the library's table entry is null
    Null,
    /// the function
    Fn(F),
}

fn slot<F>(f: Option<F>) -> Slot<F> {
    f.map_or(Slot::Null, Slot::Fn)
}

/// The queried method tables.
#[derive(Default)]
#[allow(missing_docs, reason = "one per API")]
pub(crate) struct Tables {
    pub(crate) switch: Option<sys::sai_switch_api_t>,
    port: Option<sys::sai_port_api_t>,
    hostif: Option<sys::sai_hostif_api_t>,
    policer: Option<sys::sai_policer_api_t>,
    vlan: Option<sys::sai_vlan_api_t>,
    rif: Option<sys::sai_router_interface_api_t>,
    route: Option<sys::sai_route_api_t>,
    neighbor: Option<sys::sai_neighbor_api_t>,
    nexthop: Option<sys::sai_next_hop_api_t>,
    bridge: Option<sys::sai_bridge_api_t>,
    vr: Option<sys::sai_virtual_router_api_t>,
    acl: Option<sys::sai_acl_api_t>,
    qos_map: Option<sys::sai_qos_map_api_t>,
    scheduler: Option<sys::sai_scheduler_api_t>,
    sched_group: Option<sys::sai_scheduler_group_api_t>,
    wred: Option<sys::sai_wred_api_t>,
    buffer: Option<sys::sai_buffer_api_t>,
    queue: Option<sys::sai_queue_api_t>,
    mirror: Option<sys::sai_mirror_api_t>,
    samplepacket: Option<sys::sai_samplepacket_api_t>,
    nhg: Option<sys::sai_next_hop_group_api_t>,
    lag: Option<sys::sai_lag_api_t>,
    pub(crate) fdb: Option<sys::sai_fdb_api_t>,
    hash: Option<sys::sai_hash_api_t>,
}

macro_rules! q {
    ($self:ident, $lib:ident, $field:ident, $api:ident) => {{
        let (st, t) = $lib.table(sys::$api);
        $self.$field = t;
        st
    }};
}

impl Tables {
    pub(crate) fn query(&mut self, lib: &Lib, api: Api) -> Status {
        match api {
            Api::Switch => q!(self, lib, switch, SAI_API_SWITCH),
            Api::Port => q!(self, lib, port, SAI_API_PORT),
            Api::Hostif => q!(self, lib, hostif, SAI_API_HOSTIF),
            Api::Policer => q!(self, lib, policer, SAI_API_POLICER),
            Api::Vlan => q!(self, lib, vlan, SAI_API_VLAN),
            Api::RouterInterface => q!(self, lib, rif, SAI_API_ROUTER_INTERFACE),
            Api::Route => q!(self, lib, route, SAI_API_ROUTE),
            Api::Neighbor => q!(self, lib, neighbor, SAI_API_NEIGHBOR),
            Api::NextHop => q!(self, lib, nexthop, SAI_API_NEXT_HOP),
            Api::Bridge => q!(self, lib, bridge, SAI_API_BRIDGE),
            Api::VirtualRouter => q!(self, lib, vr, SAI_API_VIRTUAL_ROUTER),
            Api::Acl => q!(self, lib, acl, SAI_API_ACL),
            Api::QosMap => q!(self, lib, qos_map, SAI_API_QOS_MAP),
            Api::Scheduler => q!(self, lib, scheduler, SAI_API_SCHEDULER),
            Api::SchedulerGroup => q!(self, lib, sched_group, SAI_API_SCHEDULER_GROUP),
            Api::Wred => q!(self, lib, wred, SAI_API_WRED),
            Api::Buffer => q!(self, lib, buffer, SAI_API_BUFFER),
            Api::Queue => q!(self, lib, queue, SAI_API_QUEUE),
            Api::Mirror => q!(self, lib, mirror, SAI_API_MIRROR),
            Api::Samplepacket => q!(self, lib, samplepacket, SAI_API_SAMPLEPACKET),
            Api::NextHopGroup => q!(self, lib, nhg, SAI_API_NEXT_HOP_GROUP),
            Api::Lag => q!(self, lib, lag, SAI_API_LAG),
            Api::Fdb => q!(self, lib, fdb, SAI_API_FDB),
            Api::Hash => q!(self, lib, hash, SAI_API_HASH),
        }
    }

    pub(crate) fn create(&self, t: sys::sai_object_type_t) -> Slot<CreateFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_QOS_MAP => self.qos_map.and_then(|a| a.create_qos_map),
            sys::SAI_OBJECT_TYPE_SCHEDULER => self.scheduler.and_then(|a| a.create_scheduler),
            sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP => self.sched_group.and_then(|a| a.create_scheduler_group),
            sys::SAI_OBJECT_TYPE_WRED => self.wred.and_then(|a| a.create_wred),
            sys::SAI_OBJECT_TYPE_BUFFER_POOL => self.buffer.and_then(|a| a.create_buffer_pool),
            sys::SAI_OBJECT_TYPE_BUFFER_PROFILE => self.buffer.and_then(|a| a.create_buffer_profile),
            sys::SAI_OBJECT_TYPE_MIRROR_SESSION => self.mirror.and_then(|a| a.create_mirror_session),
            sys::SAI_OBJECT_TYPE_SAMPLEPACKET => self.samplepacket.and_then(|a| a.create_samplepacket),
            sys::SAI_OBJECT_TYPE_HOSTIF => self.hostif.and_then(|a| a.create_hostif),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP => self.hostif.and_then(|a| a.create_hostif_trap_group),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP => self.hostif.and_then(|a| a.create_hostif_trap),
            sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY => self.hostif.and_then(|a| a.create_hostif_table_entry),
            sys::SAI_OBJECT_TYPE_POLICER => self.policer.and_then(|a| a.create_policer),
            sys::SAI_OBJECT_TYPE_VLAN => self.vlan.and_then(|a| a.create_vlan),
            sys::SAI_OBJECT_TYPE_VLAN_MEMBER => self.vlan.and_then(|a| a.create_vlan_member),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.create_router_interface),
            sys::SAI_OBJECT_TYPE_NEXT_HOP => self.nexthop.and_then(|a| a.create_next_hop),
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => self.vr.and_then(|a| a.create_virtual_router),
            sys::SAI_OBJECT_TYPE_ACL_TABLE => self.acl.and_then(|a| a.create_acl_table),
            sys::SAI_OBJECT_TYPE_ACL_ENTRY => self.acl.and_then(|a| a.create_acl_entry),
            sys::SAI_OBJECT_TYPE_ACL_COUNTER => self.acl.and_then(|a| a.create_acl_counter),
            sys::SAI_OBJECT_TYPE_ACL_RANGE => self.acl.and_then(|a| a.create_acl_range),
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP => self.acl.and_then(|a| a.create_acl_table_group),
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER => self.acl.and_then(|a| a.create_acl_table_group_member),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP => self.nhg.and_then(|a| a.create_next_hop_group),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER => self.nhg.and_then(|a| a.create_next_hop_group_member),
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT => self.bridge.and_then(|a| a.create_bridge_port),
            sys::SAI_OBJECT_TYPE_LAG => self.lag.and_then(|a| a.create_lag),
            sys::SAI_OBJECT_TYPE_LAG_MEMBER => self.lag.and_then(|a| a.create_lag_member),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn remove(&self, t: sys::sai_object_type_t) -> Slot<RemoveFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_QOS_MAP => self.qos_map.and_then(|a| a.remove_qos_map),
            sys::SAI_OBJECT_TYPE_SCHEDULER => self.scheduler.and_then(|a| a.remove_scheduler),
            sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP => self.sched_group.and_then(|a| a.remove_scheduler_group),
            sys::SAI_OBJECT_TYPE_WRED => self.wred.and_then(|a| a.remove_wred),
            sys::SAI_OBJECT_TYPE_BUFFER_POOL => self.buffer.and_then(|a| a.remove_buffer_pool),
            sys::SAI_OBJECT_TYPE_BUFFER_PROFILE => self.buffer.and_then(|a| a.remove_buffer_profile),
            sys::SAI_OBJECT_TYPE_MIRROR_SESSION => self.mirror.and_then(|a| a.remove_mirror_session),
            sys::SAI_OBJECT_TYPE_SAMPLEPACKET => self.samplepacket.and_then(|a| a.remove_samplepacket),
            sys::SAI_OBJECT_TYPE_HOSTIF => self.hostif.and_then(|a| a.remove_hostif),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP => self.hostif.and_then(|a| a.remove_hostif_trap_group),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP => self.hostif.and_then(|a| a.remove_hostif_trap),
            sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY => self.hostif.and_then(|a| a.remove_hostif_table_entry),
            sys::SAI_OBJECT_TYPE_POLICER => self.policer.and_then(|a| a.remove_policer),
            sys::SAI_OBJECT_TYPE_VLAN => self.vlan.and_then(|a| a.remove_vlan),
            sys::SAI_OBJECT_TYPE_VLAN_MEMBER => self.vlan.and_then(|a| a.remove_vlan_member),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.remove_router_interface),
            sys::SAI_OBJECT_TYPE_NEXT_HOP => self.nexthop.and_then(|a| a.remove_next_hop),
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => self.vr.and_then(|a| a.remove_virtual_router),
            sys::SAI_OBJECT_TYPE_ACL_TABLE => self.acl.and_then(|a| a.remove_acl_table),
            sys::SAI_OBJECT_TYPE_ACL_ENTRY => self.acl.and_then(|a| a.remove_acl_entry),
            sys::SAI_OBJECT_TYPE_ACL_COUNTER => self.acl.and_then(|a| a.remove_acl_counter),
            sys::SAI_OBJECT_TYPE_ACL_RANGE => self.acl.and_then(|a| a.remove_acl_range),
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP => self.acl.and_then(|a| a.remove_acl_table_group),
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER => self.acl.and_then(|a| a.remove_acl_table_group_member),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP => self.nhg.and_then(|a| a.remove_next_hop_group),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER => self.nhg.and_then(|a| a.remove_next_hop_group_member),
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT => self.bridge.and_then(|a| a.remove_bridge_port),
            sys::SAI_OBJECT_TYPE_LAG => self.lag.and_then(|a| a.remove_lag),
            sys::SAI_OBJECT_TYPE_LAG_MEMBER => self.lag.and_then(|a| a.remove_lag_member),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn set(&self, t: sys::sai_object_type_t) -> Slot<SetFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_QOS_MAP => self.qos_map.and_then(|a| a.set_qos_map_attribute),
            sys::SAI_OBJECT_TYPE_SCHEDULER => self.scheduler.and_then(|a| a.set_scheduler_attribute),
            sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP => self.sched_group.and_then(|a| a.set_scheduler_group_attribute),
            sys::SAI_OBJECT_TYPE_WRED => self.wred.and_then(|a| a.set_wred_attribute),
            sys::SAI_OBJECT_TYPE_BUFFER_POOL => self.buffer.and_then(|a| a.set_buffer_pool_attribute),
            sys::SAI_OBJECT_TYPE_BUFFER_PROFILE => self.buffer.and_then(|a| a.set_buffer_profile_attribute),
            sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => {
                self.buffer.and_then(|a| a.set_ingress_priority_group_attribute)
            }
            sys::SAI_OBJECT_TYPE_QUEUE => self.queue.and_then(|a| a.set_queue_attribute),
            sys::SAI_OBJECT_TYPE_MIRROR_SESSION => self.mirror.and_then(|a| a.set_mirror_session_attribute),
            sys::SAI_OBJECT_TYPE_SAMPLEPACKET => self.samplepacket.and_then(|a| a.set_samplepacket_attribute),
            sys::SAI_OBJECT_TYPE_SWITCH => self.switch.and_then(|a| a.set_switch_attribute),
            sys::SAI_OBJECT_TYPE_PORT => self.port.and_then(|a| a.set_port_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF => self.hostif.and_then(|a| a.set_hostif_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP => self.hostif.and_then(|a| a.set_hostif_trap_group_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP => self.hostif.and_then(|a| a.set_hostif_trap_attribute),
            sys::SAI_OBJECT_TYPE_POLICER => self.policer.and_then(|a| a.set_policer_attribute),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.set_router_interface_attribute),
            sys::SAI_OBJECT_TYPE_ACL_ENTRY => self.acl.and_then(|a| a.set_acl_entry_attribute),
            sys::SAI_OBJECT_TYPE_ACL_COUNTER => self.acl.and_then(|a| a.set_acl_counter_attribute),
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => self.vr.and_then(|a| a.set_virtual_router_attribute),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER => self.nhg.and_then(|a| a.set_next_hop_group_member_attribute),
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT => self.bridge.and_then(|a| a.set_bridge_port_attribute),
            sys::SAI_OBJECT_TYPE_LAG => self.lag.and_then(|a| a.set_lag_attribute),
            sys::SAI_OBJECT_TYPE_LAG_MEMBER => self.lag.and_then(|a| a.set_lag_member_attribute),
            sys::SAI_OBJECT_TYPE_HASH => self.hash.and_then(|a| a.set_hash_attribute),
            sys::SAI_OBJECT_TYPE_VLAN => self.vlan.and_then(|a| a.set_vlan_attribute),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn get(&self, t: sys::sai_object_type_t) -> Slot<GetFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_QOS_MAP => self.qos_map.and_then(|a| a.get_qos_map_attribute),
            sys::SAI_OBJECT_TYPE_SCHEDULER => self.scheduler.and_then(|a| a.get_scheduler_attribute),
            sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP => self.sched_group.and_then(|a| a.get_scheduler_group_attribute),
            sys::SAI_OBJECT_TYPE_WRED => self.wred.and_then(|a| a.get_wred_attribute),
            sys::SAI_OBJECT_TYPE_BUFFER_POOL => self.buffer.and_then(|a| a.get_buffer_pool_attribute),
            sys::SAI_OBJECT_TYPE_BUFFER_PROFILE => self.buffer.and_then(|a| a.get_buffer_profile_attribute),
            sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => {
                self.buffer.and_then(|a| a.get_ingress_priority_group_attribute)
            }
            sys::SAI_OBJECT_TYPE_QUEUE => self.queue.and_then(|a| a.get_queue_attribute),
            sys::SAI_OBJECT_TYPE_MIRROR_SESSION => self.mirror.and_then(|a| a.get_mirror_session_attribute),
            sys::SAI_OBJECT_TYPE_SAMPLEPACKET => self.samplepacket.and_then(|a| a.get_samplepacket_attribute),
            sys::SAI_OBJECT_TYPE_SWITCH => self.switch.and_then(|a| a.get_switch_attribute),
            sys::SAI_OBJECT_TYPE_PORT => self.port.and_then(|a| a.get_port_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF => self.hostif.and_then(|a| a.get_hostif_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP => self.hostif.and_then(|a| a.get_hostif_trap_group_attribute),
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP => self.hostif.and_then(|a| a.get_hostif_trap_attribute),
            sys::SAI_OBJECT_TYPE_POLICER => self.policer.and_then(|a| a.get_policer_attribute),
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT => self.bridge.and_then(|a| a.get_bridge_port_attribute),
            sys::SAI_OBJECT_TYPE_VLAN => self.vlan.and_then(|a| a.get_vlan_attribute),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.get_router_interface_attribute),
            sys::SAI_OBJECT_TYPE_ACL_TABLE => self.acl.and_then(|a| a.get_acl_table_attribute),
            sys::SAI_OBJECT_TYPE_ACL_ENTRY => self.acl.and_then(|a| a.get_acl_entry_attribute),
            sys::SAI_OBJECT_TYPE_ACL_COUNTER => self.acl.and_then(|a| a.get_acl_counter_attribute),
            sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP => self.acl.and_then(|a| a.get_acl_table_group_attribute),
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => self.vr.and_then(|a| a.get_virtual_router_attribute),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP => self.nhg.and_then(|a| a.get_next_hop_group_attribute),
            sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER => self.nhg.and_then(|a| a.get_next_hop_group_member_attribute),
            sys::SAI_OBJECT_TYPE_NEXT_HOP => self.nexthop.and_then(|a| a.get_next_hop_attribute),
            sys::SAI_OBJECT_TYPE_BRIDGE => self.bridge.and_then(|a| a.get_bridge_attribute),
            sys::SAI_OBJECT_TYPE_LAG => self.lag.and_then(|a| a.get_lag_attribute),
            sys::SAI_OBJECT_TYPE_LAG_MEMBER => self.lag.and_then(|a| a.get_lag_member_attribute),
            sys::SAI_OBJECT_TYPE_HASH => self.hash.and_then(|a| a.get_hash_attribute),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn stats(&self, t: sys::sai_object_type_t) -> Slot<StatsFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_PORT => self.port.and_then(|a| a.get_port_stats),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.get_router_interface_stats),
            sys::SAI_OBJECT_TYPE_QUEUE => self.queue.and_then(|a| a.get_queue_stats),
            sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => self.buffer.and_then(|a| a.get_ingress_priority_group_stats),
            sys::SAI_OBJECT_TYPE_BUFFER_POOL => self.buffer.and_then(|a| a.get_buffer_pool_stats),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn stats_ext(&self, t: sys::sai_object_type_t) -> Slot<StatsExtFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_PORT => self.port.and_then(|a| a.get_port_stats_ext),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.get_router_interface_stats_ext),
            sys::SAI_OBJECT_TYPE_QUEUE => self.queue.and_then(|a| a.get_queue_stats_ext),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn clear_stats(&self, t: sys::sai_object_type_t) -> Slot<ClearStatsFn> {
        slot(match t {
            sys::SAI_OBJECT_TYPE_PORT => self.port.and_then(|a| a.clear_port_stats),
            sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => self.rif.and_then(|a| a.clear_router_interface_stats),
            sys::SAI_OBJECT_TYPE_QUEUE => self.queue.and_then(|a| a.clear_queue_stats),
            _ => return Slot::Missing,
        })
    }

    pub(crate) fn clear_port_all_stats(&self) -> Slot<RemoveFn> {
        slot(self.port.and_then(|a| a.clear_port_all_stats))
    }

    /// `get_<entry>_attribute` of a route / neighbour / FDB entry.
    pub(crate) fn entry_get(&self, switch: sys::sai_object_id_t, k: &Entry, e: &mut Encoded) -> Status {
        match ffi::key(switch, k) {
            Key::Route(key) => self
                .route
                .and_then(|t| t.get_route_entry_attribute)
                .map_or(NOT_IMPLEMENTED, |f| ffi::entry_get(f, &key, e)),
            Key::Neighbor(key) => self
                .neighbor
                .and_then(|t| t.get_neighbor_entry_attribute)
                .map_or(NOT_IMPLEMENTED, |f| ffi::entry_get(f, &key, e)),
            Key::Fdb(key) => self
                .fdb
                .and_then(|t| t.get_fdb_entry_attribute)
                .map_or(NOT_IMPLEMENTED, |f| ffi::entry_get(f, &key, e)),
        }
    }

    pub(crate) fn entry(&self, op: EntryOp, switch: sys::sai_object_id_t, k: &Entry, e: &Encoded) -> Status {
        macro_rules! go {
            ($t:expr, $key:expr, $c:ident, $r:ident, $s:ident) => {
                match (op, $t) {
                    (EntryOp::Create, Some(t)) => t.$c.map_or(NOT_IMPLEMENTED, |f| ffi::entry_create(f, $key, e)),
                    (EntryOp::Remove, Some(t)) => t.$r.map_or(NOT_IMPLEMENTED, |f| ffi::entry_remove(f, $key)),
                    (EntryOp::Set, Some(t)) => t.$s.map_or(NOT_IMPLEMENTED, |f| ffi::entry_set(f, $key, e)),
                    (_, None) => NOT_IMPLEMENTED,
                }
            };
        }
        match ffi::key(switch, k) {
            Key::Route(key) => go!(
                self.route,
                &key,
                create_route_entry,
                remove_route_entry,
                set_route_entry_attribute
            ),
            Key::Neighbor(key) => go!(
                self.neighbor,
                &key,
                create_neighbor_entry,
                remove_neighbor_entry,
                set_neighbor_entry_attribute
            ),
            Key::Fdb(key) => go!(
                self.fdb,
                &key,
                create_fdb_entry,
                remove_fdb_entry,
                set_fdb_entry_attribute
            ),
        }
    }
}

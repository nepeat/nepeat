//! The method tables `sai_api_query` hands out and the trampolines in them. A trampoline
//! names its object type and forwards to [`crate::generic`] inside [`guard`]; nothing else.
//! The tables are immutable statics and list every field of the header's struct, so a SAI
//! header that grows an entry stops compiling here until someone decides about it.
//!
//! No entry is null. What this library does not implement (bulk operations, packet I/O,
//! MDIO, object types it does not have) is [`NotImplemented::NOT_IMPLEMENTED`]: a function
//! of the entry's exact signature that answers `SAI_STATUS_NOT_IMPLEMENTED` and looks at
//! none of its arguments. syncd calls some entries without checking them for null (bulk
//! create and remove, `flush_fdb_entries`, the MDIO functions), and takes
//! `NOT_IMPLEMENTED` from a bulk entry as "do them one by one".
use crate::Status;
use crate::generic;
use crate::guard::guard;
use sai_sys as sys;

/// A method table entry that answers `SAI_STATUS_NOT_IMPLEMENTED`.
pub(crate) trait NotImplemented {
    /// The entry.
    const NOT_IMPLEMENTED: Self;
}

/// One implementation per number of arguments: the stub is generic over the argument types,
/// so each table entry gets a function of exactly its own C signature. Safe code: the
/// arguments are received and dropped unread (all are integers and raw pointers).
macro_rules! not_implemented_arity {
    ($($arg:ident),*) => {
        impl<$($arg),*> NotImplemented for Option<unsafe extern "C" fn($($arg),*) -> Status> {
            const NOT_IMPLEMENTED: Self = {
                extern "C" fn stub<$($arg),*>($(_: $arg),*) -> Status {
                    sys::SAI_STATUS_NOT_IMPLEMENTED
                }
                Some(stub::<$($arg),*> as unsafe extern "C" fn($($arg),*) -> Status)
            };
        }
    };
}
not_implemented_arity!(A);
not_implemented_arity!(A, B);
not_implemented_arity!(A, B, C);
not_implemented_arity!(A, B, C, D);
not_implemented_arity!(A, B, C, D, E);
not_implemented_arity!(A, B, C, D, E, F);
not_implemented_arity!(A, B, C, D, E, F, G);
not_implemented_arity!(A, B, C, D, E, F, G, H);

/// Remove, set and get of one object type.
macro_rules! access_fns {
    ($object_type:expr, $api:expr) => {
        pub(crate) extern "C" fn remove(oid: sys::sai_object_id_t) -> Status {
            guard($api, || generic::remove($object_type, oid))
        }

        /// # Safety
        /// The contract of the `sai_set_*_attribute_fn` types in the SAI headers.
        pub(crate) unsafe extern "C" fn set(oid: sys::sai_object_id_t, attr: *const sys::sai_attribute_t) -> Status {
            // SAFETY: this function's contract is `generic::set`'s.
            guard($api, || unsafe { generic::set($object_type, oid, attr) })
        }

        /// # Safety
        /// The contract of the `sai_get_*_attribute_fn` types in the SAI headers.
        pub(crate) unsafe extern "C" fn get(
            oid: sys::sai_object_id_t,
            attr_count: u32,
            attr_list: *mut sys::sai_attribute_t,
        ) -> Status {
            // SAFETY: this function's contract is `generic::get`'s.
            guard($api, || unsafe {
                generic::get($object_type, oid, attr_count, attr_list)
            })
        }
    };
}

/// The counter entry points of one object type.
macro_rules! stats_fns {
    ($object_type:expr, $api:expr) => {
        /// # Safety
        /// The contract of the `sai_get_*_stats_fn` types in the SAI headers.
        pub(crate) unsafe extern "C" fn get_stats(
            oid: sys::sai_object_id_t,
            number_of_counters: u32,
            counter_ids: *const sys::sai_stat_id_t,
            counters: *mut u64,
        ) -> Status {
            // SAFETY: this function's contract is `generic::get_stats`'s.
            guard($api, || unsafe {
                generic::get_stats(
                    $object_type,
                    oid,
                    number_of_counters,
                    counter_ids,
                    sys::SAI_STATS_MODE_READ,
                    counters,
                )
            })
        }

        /// # Safety
        /// The contract of the `sai_get_*_stats_ext_fn` types in the SAI headers.
        pub(crate) unsafe extern "C" fn get_stats_ext(
            oid: sys::sai_object_id_t,
            number_of_counters: u32,
            counter_ids: *const sys::sai_stat_id_t,
            mode: sys::sai_stats_mode_t,
            counters: *mut u64,
        ) -> Status {
            // SAFETY: this function's contract is `generic::get_stats`'s.
            guard($api, || unsafe {
                generic::get_stats($object_type, oid, number_of_counters, counter_ids, mode, counters)
            })
        }

        /// # Safety
        /// The contract of the `sai_clear_*_stats_fn` types in the SAI headers.
        pub(crate) unsafe extern "C" fn clear_stats(
            oid: sys::sai_object_id_t,
            number_of_counters: u32,
            counter_ids: *const sys::sai_stat_id_t,
        ) -> Status {
            // SAFETY: this function's contract is `generic::clear_stats`'s.
            guard($api, || unsafe {
                generic::clear_stats($object_type, oid, number_of_counters, counter_ids)
            })
        }
    };
}

/// The entry points of one object type that has an object id.
macro_rules! object_fns {
    ($module:ident, $object_type:expr, $api:expr $(, $stats:ident)?) => {
        pub(crate) mod $module {
            use super::{Status, generic, guard, sys};

            /// # Safety
            /// The contract of the `sai_create_*_fn` types in the SAI headers.
            pub(crate) unsafe extern "C" fn create(
                oid: *mut sys::sai_object_id_t,
                switch_id: sys::sai_object_id_t,
                attr_count: u32,
                attr_list: *const sys::sai_attribute_t,
            ) -> Status {
                // SAFETY: this function's contract is `generic::create`'s.
                guard($api, || unsafe {
                    generic::create($object_type, oid, switch_id, attr_count, attr_list)
                })
            }

            access_fns!($object_type, $api);
            $(object_fns!(@ $stats $object_type, $api);)?
        }
    };
    (@ stats $object_type:expr, $api:expr) => {
        stats_fns!($object_type, $api);
    };
}

/// The entry points of one object type whose key is a struct (`sai_fdb_entry_t`, ...).
macro_rules! entry_fns {
    ($module:ident, $entry:ty, $api:expr) => {
        pub(crate) mod $module {
            use super::{Status, generic, guard, sys};

            /// # Safety
            /// The contract of the `sai_create_*_entry_fn` types in the SAI headers.
            pub(crate) unsafe extern "C" fn create(
                entry: *const $entry,
                attr_count: u32,
                attr_list: *const sys::sai_attribute_t,
            ) -> Status {
                // SAFETY: this function's contract is `generic::entry_create`'s.
                guard($api, || unsafe {
                    generic::entry_create(entry, attr_count, attr_list)
                })
            }

            /// # Safety
            /// The contract of the `sai_remove_*_entry_fn` types in the SAI headers.
            pub(crate) unsafe extern "C" fn remove(entry: *const $entry) -> Status {
                // SAFETY: this function's contract is `generic::entry_remove`'s.
                guard($api, || unsafe { generic::entry_remove(entry) })
            }

            /// # Safety
            /// The contract of the `sai_set_*_entry_attribute_fn` types in the SAI headers.
            pub(crate) unsafe extern "C" fn set(entry: *const $entry, attr: *const sys::sai_attribute_t) -> Status {
                // SAFETY: this function's contract is `generic::entry_set`'s.
                guard($api, || unsafe { generic::entry_set(entry, attr) })
            }

            /// # Safety
            /// The contract of the `sai_get_*_entry_attribute_fn` types in the SAI headers.
            pub(crate) unsafe extern "C" fn get(
                entry: *const $entry,
                attr_count: u32,
                attr_list: *mut sys::sai_attribute_t,
            ) -> Status {
                // SAFETY: this function's contract is `generic::entry_get`'s.
                guard($api, || unsafe {
                    generic::entry_get(entry, attr_count, attr_list)
                })
            }
        }
    };
}

object_fns!(port, sys::SAI_OBJECT_TYPE_PORT, sys::SAI_API_PORT, stats);
object_fns!(vlan, sys::SAI_OBJECT_TYPE_VLAN, sys::SAI_API_VLAN, stats);
object_fns!(vlan_member, sys::SAI_OBJECT_TYPE_VLAN_MEMBER, sys::SAI_API_VLAN);
object_fns!(bridge, sys::SAI_OBJECT_TYPE_BRIDGE, sys::SAI_API_BRIDGE, stats);
object_fns!(
    bridge_port,
    sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
    sys::SAI_API_BRIDGE,
    stats
);
object_fns!(stp, sys::SAI_OBJECT_TYPE_STP, sys::SAI_API_STP);
object_fns!(hostif, sys::SAI_OBJECT_TYPE_HOSTIF, sys::SAI_API_HOSTIF);
object_fns!(
    hostif_table_entry,
    sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY,
    sys::SAI_API_HOSTIF
);
object_fns!(
    hostif_trap_group,
    sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
    sys::SAI_API_HOSTIF
);
object_fns!(hostif_trap, sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, sys::SAI_API_HOSTIF);
object_fns!(
    hostif_user_defined_trap,
    sys::SAI_OBJECT_TYPE_HOSTIF_USER_DEFINED_TRAP,
    sys::SAI_API_HOSTIF
);
object_fns!(queue, sys::SAI_OBJECT_TYPE_QUEUE, sys::SAI_API_QUEUE, stats);
object_fns!(
    buffer_pool,
    sys::SAI_OBJECT_TYPE_BUFFER_POOL,
    sys::SAI_API_BUFFER,
    stats
);
object_fns!(
    ingress_priority_group,
    sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP,
    sys::SAI_API_BUFFER,
    stats
);
object_fns!(buffer_profile, sys::SAI_OBJECT_TYPE_BUFFER_PROFILE, sys::SAI_API_BUFFER);
object_fns!(
    virtual_router,
    sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
    sys::SAI_API_VIRTUAL_ROUTER
);
object_fns!(
    router_interface,
    sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE,
    sys::SAI_API_ROUTER_INTERFACE,
    stats
);
object_fns!(next_hop, sys::SAI_OBJECT_TYPE_NEXT_HOP, sys::SAI_API_NEXT_HOP);
object_fns!(
    next_hop_group,
    sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP,
    sys::SAI_API_NEXT_HOP_GROUP
);
object_fns!(
    next_hop_group_member,
    sys::SAI_OBJECT_TYPE_NEXT_HOP_GROUP_MEMBER,
    sys::SAI_API_NEXT_HOP_GROUP
);
object_fns!(policer, sys::SAI_OBJECT_TYPE_POLICER, sys::SAI_API_POLICER, stats);
object_fns!(qos_map, sys::SAI_OBJECT_TYPE_QOS_MAP, sys::SAI_API_QOS_MAP);
object_fns!(scheduler, sys::SAI_OBJECT_TYPE_SCHEDULER, sys::SAI_API_SCHEDULER);
object_fns!(
    scheduler_group,
    sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP,
    sys::SAI_API_SCHEDULER_GROUP
);
object_fns!(wred, sys::SAI_OBJECT_TYPE_WRED, sys::SAI_API_WRED);
object_fns!(hash, sys::SAI_OBJECT_TYPE_HASH, sys::SAI_API_HASH);
object_fns!(lag, sys::SAI_OBJECT_TYPE_LAG, sys::SAI_API_LAG);
object_fns!(lag_member, sys::SAI_OBJECT_TYPE_LAG_MEMBER, sys::SAI_API_LAG);
entry_fns!(fdb_entry, sys::sai_fdb_entry_t, sys::SAI_API_FDB);
entry_fns!(neighbor_entry, sys::sai_neighbor_entry_t, sys::SAI_API_NEIGHBOR);
entry_fns!(route_entry, sys::sai_route_entry_t, sys::SAI_API_ROUTE);

/// The switch: create has no switch id argument.
pub(crate) mod switch {
    use super::{Status, generic, guard, sys};

    /// # Safety
    /// The contract of `sai_create_switch_fn` in the SAI headers.
    pub(crate) unsafe extern "C" fn create(
        switch_id: *mut sys::sai_object_id_t,
        attr_count: u32,
        attr_list: *const sys::sai_attribute_t,
    ) -> Status {
        // SAFETY: this function's contract is `generic::create_switch`'s.
        guard(sys::SAI_API_SWITCH, || unsafe {
            generic::create_switch(switch_id, attr_count, attr_list)
        })
    }

    access_fns!(sys::SAI_OBJECT_TYPE_SWITCH, sys::SAI_API_SWITCH);
    stats_fns!(sys::SAI_OBJECT_TYPE_SWITCH, sys::SAI_API_SWITCH);
}

/// `flush_fdb_entries`.
///
/// # Safety
/// The contract of `sai_flush_fdb_entries_fn` in the SAI headers.
unsafe extern "C" fn fdb_flush(
    switch_id: sys::sai_object_id_t,
    attr_count: u32,
    attr_list: *const sys::sai_attribute_t,
) -> Status {
    // SAFETY: this function's contract is `generic::flush_fdb`'s.
    guard(sys::SAI_API_FDB, || unsafe {
        generic::flush_fdb(switch_id, attr_count, attr_list)
    })
}

/// The method table of an API, when this library has one.
pub(crate) fn of(api: sys::sai_api_t) -> Option<*const core::ffi::c_void> {
    Some(match api {
        sys::SAI_API_SWITCH => (&raw const SWITCH_API).cast(),
        sys::SAI_API_PORT => (&raw const PORT_API).cast(),
        sys::SAI_API_VLAN => (&raw const VLAN_API).cast(),
        sys::SAI_API_BRIDGE => (&raw const BRIDGE_API).cast(),
        sys::SAI_API_FDB => (&raw const FDB_API).cast(),
        sys::SAI_API_STP => (&raw const STP_API).cast(),
        sys::SAI_API_HOSTIF => (&raw const HOSTIF_API).cast(),
        sys::SAI_API_QUEUE => (&raw const QUEUE_API).cast(),
        sys::SAI_API_BUFFER => (&raw const BUFFER_API).cast(),
        sys::SAI_API_VIRTUAL_ROUTER => (&raw const VIRTUAL_ROUTER_API).cast(),
        sys::SAI_API_ROUTER_INTERFACE => (&raw const ROUTER_INTERFACE_API).cast(),
        sys::SAI_API_NEIGHBOR => (&raw const NEIGHBOR_API).cast(),
        sys::SAI_API_NEXT_HOP => (&raw const NEXT_HOP_API).cast(),
        sys::SAI_API_NEXT_HOP_GROUP => (&raw const NEXT_HOP_GROUP_API).cast(),
        sys::SAI_API_ROUTE => (&raw const ROUTE_API).cast(),
        sys::SAI_API_LAG => (&raw const LAG_API).cast(),
        sys::SAI_API_POLICER => (&raw const POLICER_API).cast(),
        sys::SAI_API_QOS_MAP => (&raw const QOS_MAP_API).cast(),
        sys::SAI_API_SCHEDULER => (&raw const SCHEDULER_API).cast(),
        sys::SAI_API_SCHEDULER_GROUP => (&raw const SCHEDULER_GROUP_API).cast(),
        sys::SAI_API_WRED => (&raw const WRED_API).cast(),
        sys::SAI_API_HASH => (&raw const HASH_API).cast(),
        _ => return None,
    })
}

/// The `sai_switch_api_t` method table.
pub(crate) static SWITCH_API: sys::sai_switch_api_t = sys::sai_switch_api_t {
    create_switch: Some(switch::create),
    remove_switch: Some(switch::remove),
    set_switch_attribute: Some(switch::set),
    get_switch_attribute: Some(switch::get),
    get_switch_stats: Some(switch::get_stats),
    get_switch_stats_ext: Some(switch::get_stats_ext),
    clear_switch_stats: Some(switch::clear_stats),
    switch_mdio_read: NotImplemented::NOT_IMPLEMENTED,
    switch_mdio_write: NotImplemented::NOT_IMPLEMENTED,
    create_switch_tunnel: NotImplemented::NOT_IMPLEMENTED,
    remove_switch_tunnel: NotImplemented::NOT_IMPLEMENTED,
    set_switch_tunnel_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_switch_tunnel_attribute: NotImplemented::NOT_IMPLEMENTED,
    switch_mdio_cl22_read: NotImplemented::NOT_IMPLEMENTED,
    switch_mdio_cl22_write: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_port_api_t` method table.
pub(crate) static PORT_API: sys::sai_port_api_t = sys::sai_port_api_t {
    create_port: Some(port::create),
    remove_port: Some(port::remove),
    set_port_attribute: Some(port::set),
    get_port_attribute: Some(port::get),
    get_port_stats: Some(port::get_stats),
    get_port_stats_ext: Some(port::get_stats_ext),
    clear_port_stats: Some(port::clear_stats),
    clear_port_all_stats: NotImplemented::NOT_IMPLEMENTED,
    create_port_pool: NotImplemented::NOT_IMPLEMENTED,
    remove_port_pool: NotImplemented::NOT_IMPLEMENTED,
    set_port_pool_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_pool_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_pool_stats: NotImplemented::NOT_IMPLEMENTED,
    get_port_pool_stats_ext: NotImplemented::NOT_IMPLEMENTED,
    clear_port_pool_stats: NotImplemented::NOT_IMPLEMENTED,
    create_port_connector: NotImplemented::NOT_IMPLEMENTED,
    remove_port_connector: NotImplemented::NOT_IMPLEMENTED,
    set_port_connector_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_connector_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_port_serdes: NotImplemented::NOT_IMPLEMENTED,
    remove_port_serdes: NotImplemented::NOT_IMPLEMENTED,
    set_port_serdes_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_serdes_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_ports: NotImplemented::NOT_IMPLEMENTED,
    remove_ports: NotImplemented::NOT_IMPLEMENTED,
    set_ports_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_ports_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_port_serdess: NotImplemented::NOT_IMPLEMENTED,
    remove_port_serdess: NotImplemented::NOT_IMPLEMENTED,
    set_port_serdess_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_serdess_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_port_llr_profile: NotImplemented::NOT_IMPLEMENTED,
    remove_port_llr_profile: NotImplemented::NOT_IMPLEMENTED,
    set_port_llr_profile_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_port_llr_profile_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_vlan_api_t` method table.
pub(crate) static VLAN_API: sys::sai_vlan_api_t = sys::sai_vlan_api_t {
    create_vlan: Some(vlan::create),
    remove_vlan: Some(vlan::remove),
    set_vlan_attribute: Some(vlan::set),
    get_vlan_attribute: Some(vlan::get),
    create_vlan_member: Some(vlan_member::create),
    remove_vlan_member: Some(vlan_member::remove),
    set_vlan_member_attribute: Some(vlan_member::set),
    get_vlan_member_attribute: Some(vlan_member::get),
    create_vlan_members: NotImplemented::NOT_IMPLEMENTED,
    remove_vlan_members: NotImplemented::NOT_IMPLEMENTED,
    get_vlan_stats: Some(vlan::get_stats),
    get_vlan_stats_ext: Some(vlan::get_stats_ext),
    clear_vlan_stats: Some(vlan::clear_stats),
};

/// The `sai_bridge_api_t` method table.
pub(crate) static BRIDGE_API: sys::sai_bridge_api_t = sys::sai_bridge_api_t {
    create_bridge: Some(bridge::create),
    remove_bridge: Some(bridge::remove),
    set_bridge_attribute: Some(bridge::set),
    get_bridge_attribute: Some(bridge::get),
    get_bridge_stats: Some(bridge::get_stats),
    get_bridge_stats_ext: Some(bridge::get_stats_ext),
    clear_bridge_stats: Some(bridge::clear_stats),
    create_bridge_port: Some(bridge_port::create),
    remove_bridge_port: Some(bridge_port::remove),
    set_bridge_port_attribute: Some(bridge_port::set),
    get_bridge_port_attribute: Some(bridge_port::get),
    get_bridge_port_stats: Some(bridge_port::get_stats),
    get_bridge_port_stats_ext: Some(bridge_port::get_stats_ext),
    clear_bridge_port_stats: Some(bridge_port::clear_stats),
};

/// The `sai_fdb_api_t` method table.
pub(crate) static FDB_API: sys::sai_fdb_api_t = sys::sai_fdb_api_t {
    create_fdb_entry: Some(fdb_entry::create),
    remove_fdb_entry: Some(fdb_entry::remove),
    set_fdb_entry_attribute: Some(fdb_entry::set),
    get_fdb_entry_attribute: Some(fdb_entry::get),
    flush_fdb_entries: Some(fdb_flush),
    create_fdb_entries: NotImplemented::NOT_IMPLEMENTED,
    remove_fdb_entries: NotImplemented::NOT_IMPLEMENTED,
    set_fdb_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_fdb_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_stp_api_t` method table.
pub(crate) static STP_API: sys::sai_stp_api_t = sys::sai_stp_api_t {
    create_stp: Some(stp::create),
    remove_stp: Some(stp::remove),
    set_stp_attribute: Some(stp::set),
    get_stp_attribute: Some(stp::get),
    create_stp_port: NotImplemented::NOT_IMPLEMENTED,
    remove_stp_port: NotImplemented::NOT_IMPLEMENTED,
    set_stp_port_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_stp_port_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_stp_ports: NotImplemented::NOT_IMPLEMENTED,
    remove_stp_ports: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_hostif_api_t` method table.
pub(crate) static HOSTIF_API: sys::sai_hostif_api_t = sys::sai_hostif_api_t {
    create_hostif: Some(hostif::create),
    remove_hostif: Some(hostif::remove),
    set_hostif_attribute: Some(hostif::set),
    get_hostif_attribute: Some(hostif::get),
    create_hostif_table_entry: Some(hostif_table_entry::create),
    remove_hostif_table_entry: Some(hostif_table_entry::remove),
    set_hostif_table_entry_attribute: Some(hostif_table_entry::set),
    get_hostif_table_entry_attribute: Some(hostif_table_entry::get),
    create_hostif_trap_group: Some(hostif_trap_group::create),
    remove_hostif_trap_group: Some(hostif_trap_group::remove),
    set_hostif_trap_group_attribute: Some(hostif_trap_group::set),
    get_hostif_trap_group_attribute: Some(hostif_trap_group::get),
    create_hostif_trap: Some(hostif_trap::create),
    remove_hostif_trap: Some(hostif_trap::remove),
    set_hostif_trap_attribute: Some(hostif_trap::set),
    get_hostif_trap_attribute: Some(hostif_trap::get),
    create_hostif_user_defined_trap: Some(hostif_user_defined_trap::create),
    remove_hostif_user_defined_trap: Some(hostif_user_defined_trap::remove),
    set_hostif_user_defined_trap_attribute: Some(hostif_user_defined_trap::set),
    get_hostif_user_defined_trap_attribute: Some(hostif_user_defined_trap::get),
    recv_hostif_packet: NotImplemented::NOT_IMPLEMENTED,
    send_hostif_packet: NotImplemented::NOT_IMPLEMENTED,
    allocate_hostif_packet: NotImplemented::NOT_IMPLEMENTED,
    free_hostif_packet: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_queue_api_t` method table.
pub(crate) static QUEUE_API: sys::sai_queue_api_t = sys::sai_queue_api_t {
    create_queue: Some(queue::create),
    remove_queue: Some(queue::remove),
    set_queue_attribute: Some(queue::set),
    get_queue_attribute: Some(queue::get),
    get_queue_stats: Some(queue::get_stats),
    get_queue_stats_ext: Some(queue::get_stats_ext),
    clear_queue_stats: Some(queue::clear_stats),
    set_queues_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_queues_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_buffer_api_t` method table.
pub(crate) static BUFFER_API: sys::sai_buffer_api_t = sys::sai_buffer_api_t {
    create_buffer_pool: Some(buffer_pool::create),
    remove_buffer_pool: Some(buffer_pool::remove),
    set_buffer_pool_attribute: Some(buffer_pool::set),
    get_buffer_pool_attribute: Some(buffer_pool::get),
    get_buffer_pool_stats: Some(buffer_pool::get_stats),
    get_buffer_pool_stats_ext: Some(buffer_pool::get_stats_ext),
    clear_buffer_pool_stats: Some(buffer_pool::clear_stats),
    create_ingress_priority_group: Some(ingress_priority_group::create),
    remove_ingress_priority_group: Some(ingress_priority_group::remove),
    set_ingress_priority_group_attribute: Some(ingress_priority_group::set),
    get_ingress_priority_group_attribute: Some(ingress_priority_group::get),
    get_ingress_priority_group_stats: Some(ingress_priority_group::get_stats),
    get_ingress_priority_group_stats_ext: Some(ingress_priority_group::get_stats_ext),
    clear_ingress_priority_group_stats: Some(ingress_priority_group::clear_stats),
    create_buffer_profile: Some(buffer_profile::create),
    remove_buffer_profile: Some(buffer_profile::remove),
    set_buffer_profile_attribute: Some(buffer_profile::set),
    get_buffer_profile_attribute: Some(buffer_profile::get),
    set_ingress_priority_groups_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_ingress_priority_groups_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_virtual_router_api_t` method table.
pub(crate) static VIRTUAL_ROUTER_API: sys::sai_virtual_router_api_t = sys::sai_virtual_router_api_t {
    create_virtual_router: Some(virtual_router::create),
    remove_virtual_router: Some(virtual_router::remove),
    set_virtual_router_attribute: Some(virtual_router::set),
    get_virtual_router_attribute: Some(virtual_router::get),
};

/// The `sai_router_interface_api_t` method table.
pub(crate) static ROUTER_INTERFACE_API: sys::sai_router_interface_api_t = sys::sai_router_interface_api_t {
    create_router_interface: Some(router_interface::create),
    remove_router_interface: Some(router_interface::remove),
    set_router_interface_attribute: Some(router_interface::set),
    get_router_interface_attribute: Some(router_interface::get),
    get_router_interface_stats: Some(router_interface::get_stats),
    get_router_interface_stats_ext: Some(router_interface::get_stats_ext),
    clear_router_interface_stats: Some(router_interface::clear_stats),
    create_router_interfaces: NotImplemented::NOT_IMPLEMENTED,
    remove_router_interfaces: NotImplemented::NOT_IMPLEMENTED,
    set_router_interfaces_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_router_interfaces_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_neighbor_api_t` method table.
pub(crate) static NEIGHBOR_API: sys::sai_neighbor_api_t = sys::sai_neighbor_api_t {
    create_neighbor_entry: Some(neighbor_entry::create),
    remove_neighbor_entry: Some(neighbor_entry::remove),
    set_neighbor_entry_attribute: Some(neighbor_entry::set),
    get_neighbor_entry_attribute: Some(neighbor_entry::get),
    remove_all_neighbor_entries: NotImplemented::NOT_IMPLEMENTED,
    create_neighbor_entries: NotImplemented::NOT_IMPLEMENTED,
    remove_neighbor_entries: NotImplemented::NOT_IMPLEMENTED,
    set_neighbor_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_neighbor_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_next_hop_api_t` method table.
pub(crate) static NEXT_HOP_API: sys::sai_next_hop_api_t = sys::sai_next_hop_api_t {
    create_next_hop: Some(next_hop::create),
    remove_next_hop: Some(next_hop::remove),
    set_next_hop_attribute: Some(next_hop::set),
    get_next_hop_attribute: Some(next_hop::get),
    create_next_hops: NotImplemented::NOT_IMPLEMENTED,
    remove_next_hops: NotImplemented::NOT_IMPLEMENTED,
    set_next_hops_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_next_hops_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_next_hop_group_api_t` method table.
pub(crate) static NEXT_HOP_GROUP_API: sys::sai_next_hop_group_api_t = sys::sai_next_hop_group_api_t {
    create_next_hop_group: Some(next_hop_group::create),
    remove_next_hop_group: Some(next_hop_group::remove),
    set_next_hop_group_attribute: Some(next_hop_group::set),
    get_next_hop_group_attribute: Some(next_hop_group::get),
    create_next_hop_group_member: Some(next_hop_group_member::create),
    remove_next_hop_group_member: Some(next_hop_group_member::remove),
    set_next_hop_group_member_attribute: Some(next_hop_group_member::set),
    get_next_hop_group_member_attribute: Some(next_hop_group_member::get),
    create_next_hop_group_members: NotImplemented::NOT_IMPLEMENTED,
    remove_next_hop_group_members: NotImplemented::NOT_IMPLEMENTED,
    create_next_hop_group_map: NotImplemented::NOT_IMPLEMENTED,
    remove_next_hop_group_map: NotImplemented::NOT_IMPLEMENTED,
    set_next_hop_group_map_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_next_hop_group_map_attribute: NotImplemented::NOT_IMPLEMENTED,
    set_next_hop_group_members_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_next_hop_group_members_attribute: NotImplemented::NOT_IMPLEMENTED,
    create_next_hop_groups: NotImplemented::NOT_IMPLEMENTED,
    remove_next_hop_groups: NotImplemented::NOT_IMPLEMENTED,
    set_next_hop_groups_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_next_hop_groups_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_route_api_t` method table.
pub(crate) static ROUTE_API: sys::sai_route_api_t = sys::sai_route_api_t {
    create_route_entry: Some(route_entry::create),
    remove_route_entry: Some(route_entry::remove),
    set_route_entry_attribute: Some(route_entry::set),
    get_route_entry_attribute: Some(route_entry::get),
    create_route_entries: NotImplemented::NOT_IMPLEMENTED,
    remove_route_entries: NotImplemented::NOT_IMPLEMENTED,
    set_route_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_route_entries_attribute: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_lag_api_t` method table.
pub(crate) static LAG_API: sys::sai_lag_api_t = sys::sai_lag_api_t {
    create_lag: Some(lag::create),
    remove_lag: Some(lag::remove),
    set_lag_attribute: Some(lag::set),
    get_lag_attribute: Some(lag::get),
    create_lag_member: Some(lag_member::create),
    remove_lag_member: Some(lag_member::remove),
    set_lag_member_attribute: Some(lag_member::set),
    get_lag_member_attribute: Some(lag_member::get),
    create_lag_members: NotImplemented::NOT_IMPLEMENTED,
    remove_lag_members: NotImplemented::NOT_IMPLEMENTED,
};

/// The `sai_policer_api_t` method table.
pub(crate) static POLICER_API: sys::sai_policer_api_t = sys::sai_policer_api_t {
    create_policer: Some(policer::create),
    remove_policer: Some(policer::remove),
    set_policer_attribute: Some(policer::set),
    get_policer_attribute: Some(policer::get),
    get_policer_stats: Some(policer::get_stats),
    get_policer_stats_ext: Some(policer::get_stats_ext),
    clear_policer_stats: Some(policer::clear_stats),
};

/// The `sai_qos_map_api_t` method table.
pub(crate) static QOS_MAP_API: sys::sai_qos_map_api_t = sys::sai_qos_map_api_t {
    create_qos_map: Some(qos_map::create),
    remove_qos_map: Some(qos_map::remove),
    set_qos_map_attribute: Some(qos_map::set),
    get_qos_map_attribute: Some(qos_map::get),
};

/// The `sai_scheduler_api_t` method table.
pub(crate) static SCHEDULER_API: sys::sai_scheduler_api_t = sys::sai_scheduler_api_t {
    create_scheduler: Some(scheduler::create),
    remove_scheduler: Some(scheduler::remove),
    set_scheduler_attribute: Some(scheduler::set),
    get_scheduler_attribute: Some(scheduler::get),
};

/// The `sai_scheduler_group_api_t` method table.
pub(crate) static SCHEDULER_GROUP_API: sys::sai_scheduler_group_api_t = sys::sai_scheduler_group_api_t {
    create_scheduler_group: Some(scheduler_group::create),
    remove_scheduler_group: Some(scheduler_group::remove),
    set_scheduler_group_attribute: Some(scheduler_group::set),
    get_scheduler_group_attribute: Some(scheduler_group::get),
};

/// The `sai_wred_api_t` method table.
pub(crate) static WRED_API: sys::sai_wred_api_t = sys::sai_wred_api_t {
    create_wred: Some(wred::create),
    remove_wred: Some(wred::remove),
    set_wred_attribute: Some(wred::set),
    get_wred_attribute: Some(wred::get),
};

/// The `sai_hash_api_t` method table.
pub(crate) static HASH_API: sys::sai_hash_api_t = sys::sai_hash_api_t {
    create_hash: Some(hash::create),
    remove_hash: Some(hash::remove),
    set_hash_attribute: Some(hash::set),
    get_hash_attribute: Some(hash::get),
    create_fine_grained_hash_field: NotImplemented::NOT_IMPLEMENTED,
    remove_fine_grained_hash_field: NotImplemented::NOT_IMPLEMENTED,
    set_fine_grained_hash_field_attribute: NotImplemented::NOT_IMPLEMENTED,
    get_fine_grained_hash_field_attribute: NotImplemented::NOT_IMPLEMENTED,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stub_has_its_entry_s_signature_and_answers_not_implemented() {
        let flush: sys::sai_switch_mdio_read_fn = NotImplemented::NOT_IMPLEMENTED;
        let mut value = 7u32;
        // SAFETY: the stub reads none of its arguments.
        let status = unsafe { flush.unwrap()(1, 2, 3, 4, &raw mut value) };
        assert_eq!((status, value), (sys::SAI_STATUS_NOT_IMPLEMENTED, 7));
        let bulk: sys::sai_bulk_object_create_fn = NotImplemented::NOT_IMPLEMENTED;
        // SAFETY: the stub reads none of its arguments; null pointers are never followed.
        let status = unsafe {
            bulk.unwrap()(
                1,
                4,
                core::ptr::null(),
                core::ptr::null_mut(),
                sys::SAI_BULK_OP_ERROR_MODE_STOP_ON_ERROR,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(status, sys::SAI_STATUS_NOT_IMPLEMENTED);
    }

    #[test]
    fn every_api_this_library_answers_has_a_table() {
        for api in 0..sys::SAI_API_MAX {
            let has = of(api).is_some();
            let expected = matches!(
                api,
                sys::SAI_API_SWITCH
                    | sys::SAI_API_PORT
                    | sys::SAI_API_VLAN
                    | sys::SAI_API_BRIDGE
                    | sys::SAI_API_FDB
                    | sys::SAI_API_STP
                    | sys::SAI_API_HOSTIF
                    | sys::SAI_API_QUEUE
                    | sys::SAI_API_BUFFER
                    | sys::SAI_API_VIRTUAL_ROUTER
                    | sys::SAI_API_ROUTER_INTERFACE
                    | sys::SAI_API_NEIGHBOR
                    | sys::SAI_API_NEXT_HOP
                    | sys::SAI_API_NEXT_HOP_GROUP
                    | sys::SAI_API_ROUTE
                    | sys::SAI_API_LAG
                    | sys::SAI_API_POLICER
                    | sys::SAI_API_QOS_MAP
                    | sys::SAI_API_SCHEDULER
                    | sys::SAI_API_SCHEDULER_GROUP
                    | sys::SAI_API_WRED
                    | sys::SAI_API_HASH
            );
            assert_eq!(has, expected, "api {api}");
        }
    }
}

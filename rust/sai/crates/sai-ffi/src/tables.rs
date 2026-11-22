//! The method tables `sai_api_query` hands out and the trampolines in them. A trampoline
//! names its object type and forwards to [`crate::generic`] inside [`guard`]; nothing else.
//! The tables are immutable statics and list every field of the header's struct, so a SAI
//! header that grows an entry stops compiling here until someone decides about it.
use crate::Status;
use crate::generic;
use crate::guard::guard;
use sai_sys as sys;

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

/// The four entry points of one object type.
macro_rules! object_fns {
    ($module:ident, $object_type:expr, $api:expr) => {
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
        }
    };
}

object_fns!(port, sys::SAI_OBJECT_TYPE_PORT, sys::SAI_API_PORT);
object_fns!(vlan, sys::SAI_OBJECT_TYPE_VLAN, sys::SAI_API_VLAN);
object_fns!(vlan_member, sys::SAI_OBJECT_TYPE_VLAN_MEMBER, sys::SAI_API_VLAN);
object_fns!(bridge, sys::SAI_OBJECT_TYPE_BRIDGE, sys::SAI_API_BRIDGE);
object_fns!(bridge_port, sys::SAI_OBJECT_TYPE_BRIDGE_PORT, sys::SAI_API_BRIDGE);

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
}

/// The `sai_switch_api_t` method table. Entries this slice does not implement are null (README, "What
/// syncd needs next").
pub(crate) static SWITCH_API: sys::sai_switch_api_t = sys::sai_switch_api_t {
    create_switch: Some(switch::create),
    remove_switch: Some(switch::remove),
    set_switch_attribute: Some(switch::set),
    get_switch_attribute: Some(switch::get),
    get_switch_stats: None,
    get_switch_stats_ext: None,
    clear_switch_stats: None,
    switch_mdio_read: None,
    switch_mdio_write: None,
    create_switch_tunnel: None,
    remove_switch_tunnel: None,
    set_switch_tunnel_attribute: None,
    get_switch_tunnel_attribute: None,
    switch_mdio_cl22_read: None,
    switch_mdio_cl22_write: None,
};

/// The `sai_port_api_t` method table. Entries this slice does not implement are null (README, "What
/// syncd needs next").
pub(crate) static PORT_API: sys::sai_port_api_t = sys::sai_port_api_t {
    create_port: Some(port::create),
    remove_port: Some(port::remove),
    set_port_attribute: Some(port::set),
    get_port_attribute: Some(port::get),
    get_port_stats: None,
    get_port_stats_ext: None,
    clear_port_stats: None,
    clear_port_all_stats: None,
    create_port_pool: None,
    remove_port_pool: None,
    set_port_pool_attribute: None,
    get_port_pool_attribute: None,
    get_port_pool_stats: None,
    get_port_pool_stats_ext: None,
    clear_port_pool_stats: None,
    create_port_connector: None,
    remove_port_connector: None,
    set_port_connector_attribute: None,
    get_port_connector_attribute: None,
    create_port_serdes: None,
    remove_port_serdes: None,
    set_port_serdes_attribute: None,
    get_port_serdes_attribute: None,
    create_ports: None,
    remove_ports: None,
    set_ports_attribute: None,
    get_ports_attribute: None,
    create_port_serdess: None,
    remove_port_serdess: None,
    set_port_serdess_attribute: None,
    get_port_serdess_attribute: None,
    create_port_llr_profile: None,
    remove_port_llr_profile: None,
    set_port_llr_profile_attribute: None,
    get_port_llr_profile_attribute: None,
};

/// The `sai_vlan_api_t` method table. Entries this slice does not implement are null (README, "What
/// syncd needs next").
pub(crate) static VLAN_API: sys::sai_vlan_api_t = sys::sai_vlan_api_t {
    create_vlan: Some(vlan::create),
    remove_vlan: Some(vlan::remove),
    set_vlan_attribute: Some(vlan::set),
    get_vlan_attribute: Some(vlan::get),
    create_vlan_member: Some(vlan_member::create),
    remove_vlan_member: Some(vlan_member::remove),
    set_vlan_member_attribute: Some(vlan_member::set),
    get_vlan_member_attribute: Some(vlan_member::get),
    create_vlan_members: None,
    remove_vlan_members: None,
    get_vlan_stats: None,
    get_vlan_stats_ext: None,
    clear_vlan_stats: None,
};

/// The `sai_bridge_api_t` method table. Entries this slice does not implement are null (README, "What
/// syncd needs next").
pub(crate) static BRIDGE_API: sys::sai_bridge_api_t = sys::sai_bridge_api_t {
    create_bridge: Some(bridge::create),
    remove_bridge: Some(bridge::remove),
    set_bridge_attribute: Some(bridge::set),
    get_bridge_attribute: Some(bridge::get),
    get_bridge_stats: None,
    get_bridge_stats_ext: None,
    clear_bridge_stats: None,
    create_bridge_port: Some(bridge_port::create),
    remove_bridge_port: Some(bridge_port::remove),
    set_bridge_port_attribute: Some(bridge_port::set),
    get_bridge_port_attribute: Some(bridge_port::get),
    get_bridge_port_stats: None,
    get_bridge_port_stats_ext: None,
    clear_bridge_port_stats: None,
};

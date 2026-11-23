//! The slice driven through the exported C functions and method tables, from Rust, using
//! the declarations `sai-sys` generated from the SAI headers (so the prototypes are the
//! headers'). One test function: the library's state is global. Runs under Miri.
#![allow(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    clippy::missing_safety_doc
)]
// Every unsafe block here is a C caller keeping the SAI contract (or deliberately passing
// null where the library must answer with an error); spelling that out per call adds nothing.

// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
use core::ffi::{c_char, c_void};
use sai as _; // link the library: it defines the sai_* symbols declared in sai-sys
use sai_sys::*;
use std::ffi::CStr;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static OFFER_VIRTUAL: AtomicBool = AtomicBool::new(true);
static PORT_EVENTS: Mutex<Vec<(sai_object_id_t, sai_port_oper_status_t)>> = Mutex::new(Vec::new());

unsafe extern "C" fn profile_get_value(_profile: sai_switch_profile_id_t, variable: *const c_char) -> *const c_char {
    let key = unsafe { CStr::from_ptr(variable) };
    match key.to_bytes() {
        b"SAI_RS_BACKEND" if OFFER_VIRTUAL.load(Ordering::SeqCst) => c"virtual".as_ptr(),
        b"SAI_RS_VIRTUAL_PORTS" => c"4".as_ptr(),
        _ => ptr::null(),
    }
}

unsafe extern "C" fn on_port_state(count: u32, data: *const sai_port_oper_status_notification_t) {
    let data = unsafe { std::slice::from_raw_parts(data, count as usize) };
    PORT_EVENTS
        .lock()
        .unwrap()
        .extend(data.iter().map(|d| (d.port_id, d.port_state)));
}

fn attr(id: sai_attr_id_t) -> sai_attribute_t {
    sai_attribute_t {
        id,
        value: sai_attribute_value_t { u64_: 0 },
    }
}

fn wait_for_event(expected: (sai_object_id_t, sai_port_oper_status_t)) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !PORT_EVENTS.lock().unwrap().contains(&expected) {
        assert!(
            Instant::now() < deadline,
            "no notification {expected:?}; got {:?}",
            PORT_EVENTS.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// ACL tables, entries (fields and actions in `aclfield` / `aclaction`), counters and binding,
/// through the ACL method table as `AclOrch` calls it.
fn acl_through_the_c_abi(switch: sai_object_id_t, ports: &[sai_object_id_t; 4]) {
    let acl: &sai_acl_api_t = unsafe { table(SAI_API_ACL) };
    let port_api: &sai_port_api_t = unsafe { table(SAI_API_PORT) };
    let mut binds = [SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed()];
    let mut t = attr(SAI_ACL_TABLE_ATTR_ACL_STAGE);
    t.value.s32 = SAI_ACL_STAGE_INGRESS.cast_signed();
    let mut b = attr(SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST);
    b.value.s32list = sai_s32_list_t {
        count: 1,
        list: binds.as_mut_ptr(),
    };
    let mut f = attr(SAI_ACL_TABLE_ATTR_FIELD_DST_IP);
    f.value.booldata = true;
    let mut table_id = 0;
    let tattrs = [t, b, f];
    assert_eq!(
        unsafe { acl.create_acl_table.unwrap()(&raw mut table_id, switch, 3, tattrs.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    // an entry: the table, DST_IP 10.0.0.1/32 (enable stored as 2: any non-zero C bool), DROP
    let mut e0 = attr(SAI_ACL_ENTRY_ATTR_TABLE_ID);
    e0.value.oid = table_id;
    let mut e1 = garbage(SAI_ACL_ENTRY_ATTR_FIELD_DST_IP);
    e1.value.aclfield.data.ip4 = u32::from_ne_bytes([10, 0, 0, 1]);
    e1.value.aclfield.mask.ip4 = u32::MAX;
    unsafe { (&raw mut e1.value.aclfield.enable).cast::<u8>().write(2) };
    let mut e2 = garbage(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION);
    e2.value.aclaction.enable = true;
    e2.value.aclaction.parameter.s32 = SAI_PACKET_ACTION_DROP.cast_signed();
    // a disabled field whose data is garbage, a null list included: never read
    let mut e3 = garbage(SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS);
    e3.value.aclfield.enable = false;
    e3.value.aclfield.data.objlist = sai_object_list_t {
        count: 9,
        list: ptr::null_mut(),
    };
    let eattrs = [e0, e1, e2, e3];
    let mut entry = 0;
    assert_eq!(
        unsafe { acl.create_acl_entry.unwrap()(&raw mut entry, switch, 4, eattrs.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { sai_object_type_query(entry) }, SAI_OBJECT_TYPE_ACL_ENTRY);
    let mut got = [
        garbage(SAI_ACL_ENTRY_ATTR_FIELD_DST_IP),
        garbage(SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION),
        garbage(SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP),
    ];
    assert_eq!(
        unsafe { acl.get_acl_entry_attribute.unwrap()(entry, 3, got.as_mut_ptr()) },
        SAI_STATUS_SUCCESS
    );
    unsafe {
        assert!(got[0].value.aclfield.enable);
        assert_eq!(got[0].value.aclfield.data.ip4.to_ne_bytes(), [10, 0, 0, 1]);
        assert_eq!(got[0].value.aclfield.mask.ip4, u32::MAX);
        assert!(got[1].value.aclaction.enable);
        assert_eq!(
            got[1].value.aclaction.parameter.s32,
            SAI_PACKET_ACTION_DROP.cast_signed()
        );
        assert!(!got[2].value.aclfield.enable);
    }
    // bound to a port: installed; a get of the counterless entry's in-ports answers disabled
    let mut bind = attr(SAI_PORT_ATTR_INGRESS_ACL);
    bind.value.oid = table_id;
    assert_eq!(
        unsafe { port_api.set_port_attribute.unwrap()(ports[1], &raw const bind) },
        SAI_STATUS_SUCCESS
    );
    let mut q = garbage(SAI_PORT_ATTR_INGRESS_ACL);
    assert_eq!(
        unsafe { port_api.get_port_attribute.unwrap()(ports[1], 1, &raw mut q) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { q.value.oid }, table_id);
    assert_eq!(
        unsafe { acl.remove_acl_table.unwrap()(table_id) },
        SAI_STATUS_OBJECT_IN_USE
    );
    bind.value.oid = 0;
    assert_eq!(
        unsafe { port_api.set_port_attribute.unwrap()(ports[1], &raw const bind) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { acl.remove_acl_entry.unwrap()(entry) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { acl.remove_acl_table.unwrap()(table_id) }, SAI_STATUS_SUCCESS);
    // chain groups: not objects here (C++: create NOT_SUPPORTED, the rest INVALID_OBJECT_ID)
    let mut chain = 0x55;
    assert_eq!(
        unsafe { acl.create_acl_table_chain_group.unwrap()(&raw mut chain, switch, 0, ptr::null()) },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(chain, 0x55);
    assert_eq!(
        unsafe { acl.remove_acl_table_chain_group.unwrap()(1) },
        SAI_STATUS_INVALID_OBJECT_ID
    );
}

unsafe fn table<T>(api: sai_api_t) -> &'static T {
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(unsafe { sai_api_query(api, &raw mut p) }, SAI_STATUS_SUCCESS);
    unsafe { p.cast::<T>().as_ref() }.unwrap()
}

/// A value slot as a C caller leaves it on its stack: every byte garbage until a member is set.
fn garbage(id: sai_attr_id_t) -> sai_attribute_t {
    let mut a = attr(id);
    a.value.u64_ = 0xa5a5_a5a5_a5a5_a5a5;
    a
}

/// The second slice: default objects, attribute-bag objects, entries (their own function
/// shapes), counters, capability queries and the entries that answer "not implemented".
fn objects_entries_and_counters(switch: sai_object_id_t, ports: &[sai_object_id_t; 4]) {
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    let port_api: &sai_port_api_t = unsafe { table(SAI_API_PORT) };
    let rif_api: &sai_router_interface_api_t = unsafe { table(SAI_API_ROUTER_INTERFACE) };
    let fdb_api: &sai_fdb_api_t = unsafe { table(SAI_API_FDB) };
    let route_api: &sai_route_api_t = unsafe { table(SAI_API_ROUTE) };
    let neighbor_api: &sai_neighbor_api_t = unsafe { table(SAI_API_NEIGHBOR) };
    let bridge_api: &sai_bridge_api_t = unsafe { table(SAI_API_BRIDGE) };
    let queue_api: &sai_queue_api_t = unsafe { table(SAI_API_QUEUE) };
    let get_switch = switch_api.get_switch_attribute.unwrap();
    let get_port = port_api.get_port_attribute.unwrap();

    // default objects
    let mut a = garbage(SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let vr = unsafe { a.value.oid };
    assert_eq!(unsafe { sai_object_type_query(vr) }, SAI_OBJECT_TYPE_VIRTUAL_ROUTER);
    let mut a = garbage(SAI_SWITCH_ATTR_DEFAULT_VLAN_ID);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let default_vlan = unsafe { a.value.oid };
    // a u8 answer changes one byte of the caller's slot and no other
    let mut a = garbage(SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.u8_ }, 8);
    let mut expected = [0xa5u8; 8];
    expected[0] = 8;
    assert_eq!(unsafe { a.value.u64_ }.to_ne_bytes(), expected);
    // queues: a list of 8, each a QUEUE with a u8 index
    let mut queues = [0u64; 8];
    let mut a = attr(SAI_PORT_ATTR_QOS_QUEUE_LIST);
    a.value.objlist = sai_object_list_t {
        count: 8,
        list: queues.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_port(ports[0], 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.objlist.count }, 8);
    let mut a = garbage(SAI_QUEUE_ATTR_INDEX);
    let get_queue = queue_api.get_queue_attribute.unwrap();
    assert_eq!(unsafe { get_queue(queues[5], 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.u8_ }, 5);

    // an attribute-bag object: a router interface on a port
    let mut rif_attrs = [
        garbage(SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID),
        garbage(SAI_ROUTER_INTERFACE_ATTR_TYPE),
        garbage(SAI_ROUTER_INTERFACE_ATTR_PORT_ID),
    ];
    rif_attrs[0].value.oid = vr;
    rif_attrs[1].value.s32 = SAI_ROUTER_INTERFACE_TYPE_PORT.cast_signed();
    rif_attrs[2].value.oid = ports[0];
    let create_rif = rif_api.create_router_interface.unwrap();
    let mut rif = 0;
    assert_eq!(
        unsafe { create_rif(&raw mut rif, switch, 2, rif_attrs.as_ptr()) },
        SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING
    );
    assert_eq!(
        unsafe { create_rif(&raw mut rif, switch, 3, rif_attrs.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { sai_object_type_query(rif) }, SAI_OBJECT_TYPE_ROUTER_INTERFACE);
    let mut a = garbage(SAI_ROUTER_INTERFACE_ATTR_MTU);
    let get_rif = rif_api.get_router_interface_attribute.unwrap();
    assert_eq!(unsafe { get_rif(rif, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.u32_ }, 1514);

    // entries: the key is a struct, IP addresses are bytes in network order
    let mut neighbor: sai_neighbor_entry_t = unsafe { core::mem::zeroed() };
    neighbor.switch_id = switch;
    neighbor.rif_id = rif;
    neighbor.ip_address.addr_family = SAI_IP_ADDR_FAMILY_IPV4;
    neighbor.ip_address.addr.ip4 = u32::from_ne_bytes([10, 0, 0, 2]);
    let mut mac = garbage(SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS);
    mac.value.mac = [2, 0, 0, 0, 0, 7];
    let create_neighbor = neighbor_api.create_neighbor_entry.unwrap();
    assert_eq!(
        unsafe { create_neighbor(ptr::null(), 1, &raw const mac) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { create_neighbor(&raw const neighbor, 1, &raw const mac) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe { create_neighbor(&raw const neighbor, 1, &raw const mac) },
        SAI_STATUS_ITEM_ALREADY_EXISTS
    );
    let mut a = garbage(SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS);
    let get_neighbor = neighbor_api.get_neighbor_entry_attribute.unwrap();
    assert_eq!(
        unsafe { get_neighbor(&raw const neighbor, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { a.value.mac }, [2, 0, 0, 0, 0, 7]);
    assert_eq!(unsafe { a.value.u64_ }.to_ne_bytes()[6..], [0xa5, 0xa5]);
    // the interface is held by its neighbor
    let remove_rif = rif_api.remove_router_interface.unwrap();
    assert_eq!(unsafe { remove_rif(rif) }, SAI_STATUS_OBJECT_IN_USE);

    let mut route: sai_route_entry_t = unsafe { core::mem::zeroed() };
    route.switch_id = switch;
    route.vr_id = vr;
    route.destination.addr_family = SAI_IP_ADDR_FAMILY_IPV6;
    route.destination.addr.ip6 = [0xfc, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    route.destination.mask.ip6 = [0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let create_route = route_api.create_route_entry.unwrap();
    assert_eq!(
        unsafe { create_route(&raw const route, 0, ptr::null()) },
        SAI_STATUS_SUCCESS
    );
    let mut a = garbage(SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION);
    let get_route = route_api.get_route_entry_attribute.unwrap();
    assert_eq!(
        unsafe { get_route(&raw const route, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { a.value.s32 }, SAI_PACKET_ACTION_FORWARD.cast_signed());
    let mut drop_it = garbage(SAI_ROUTE_ENTRY_ATTR_PACKET_ACTION);
    drop_it.value.s32 = SAI_PACKET_ACTION_DROP.cast_signed();
    let set_route = route_api.set_route_entry_attribute.unwrap();
    assert_eq!(
        unsafe { set_route(&raw const route, &raw const drop_it) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe { get_route(&raw const route, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { a.value.s32 }, SAI_PACKET_ACTION_DROP.cast_signed());
    // a key of another switch, a family that is none
    route.switch_id = ports[0];
    assert_eq!(
        unsafe { get_route(&raw const route, 1, &raw mut a) },
        SAI_STATUS_INVALID_OBJECT_ID
    );
    route.switch_id = switch;
    let remove_route = route_api.remove_route_entry.unwrap();
    assert_eq!(unsafe { remove_route(&raw const route) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { remove_route(&raw const route) }, SAI_STATUS_ITEM_NOT_FOUND);
    route.destination.addr_family = 5;
    assert_eq!(unsafe { remove_route(&raw const route) }, SAI_STATUS_INVALID_PARAMETER);
    let remove_neighbor = neighbor_api.remove_neighbor_entry.unwrap();
    assert_eq!(unsafe { remove_neighbor(&raw const neighbor) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { remove_rif(rif) }, SAI_STATUS_SUCCESS);

    // FDB entries and their flush
    let mut bridge_ports = [0u64; 4];
    let mut a = attr(SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let bridge = unsafe { a.value.oid };
    let mut a = attr(SAI_BRIDGE_ATTR_PORT_LIST);
    a.value.objlist = sai_object_list_t {
        count: 4,
        list: bridge_ports.as_mut_ptr(),
    };
    let get_bridge = bridge_api.get_bridge_attribute.unwrap();
    assert_eq!(unsafe { get_bridge(bridge, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let fdb = sai_fdb_entry_t {
        switch_id: switch,
        mac_address: [2, 0, 0, 0, 0, 9],
        bv_id: default_vlan,
    };
    let mut fdb_attrs = [
        garbage(SAI_FDB_ENTRY_ATTR_TYPE),
        garbage(SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID),
    ];
    fdb_attrs[0].value.s32 = SAI_FDB_ENTRY_TYPE_DYNAMIC.cast_signed();
    fdb_attrs[1].value.oid = bridge_ports[1];
    let create_fdb = fdb_api.create_fdb_entry.unwrap();
    assert_eq!(
        unsafe { create_fdb(&raw const fdb, 2, fdb_attrs.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    let mut a = garbage(SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID);
    let get_fdb = fdb_api.get_fdb_entry_attribute.unwrap();
    assert_eq!(unsafe { get_fdb(&raw const fdb, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.oid }, bridge_ports[1]);
    let flush = fdb_api.flush_fdb_entries.unwrap();
    assert_eq!(unsafe { flush(switch, 1, ptr::null()) }, SAI_STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { flush(switch, 0, ptr::null()) }, SAI_STATUS_SUCCESS);
    assert_eq!(
        unsafe { get_fdb(&raw const fdb, 1, &raw mut a) },
        SAI_STATUS_ITEM_NOT_FOUND
    );

    // counters: read, read with a mode, clear; pointers are checked before anything is read
    let ids = [SAI_PORT_STAT_IF_IN_OCTETS, SAI_PORT_STAT_IF_OUT_OCTETS];
    let mut counters = [7u64; 2];
    let get_stats = port_api.get_port_stats.unwrap();
    let get_stats_ext = port_api.get_port_stats_ext.unwrap();
    let clear_stats = port_api.clear_port_stats.unwrap();
    assert_eq!(
        unsafe { get_stats(ports[0], 2, ids.as_ptr(), counters.as_mut_ptr()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(counters, [0, 0]);
    assert_eq!(
        unsafe { get_stats(ports[0], 2, ids.as_ptr(), ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { get_stats(ports[0], 2, ptr::null(), counters.as_mut_ptr()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { get_stats(ports[0], 0, ptr::null(), ptr::null_mut()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe {
            get_stats_ext(
                ports[0],
                2,
                ids.as_ptr(),
                SAI_STATS_MODE_READ_AND_CLEAR,
                counters.as_mut_ptr(),
            )
        },
        SAI_STATUS_NOT_SUPPORTED // C++: no per-counter clear in the SDK API
    );
    assert_eq!(
        unsafe { get_stats_ext(ports[0], 2, ids.as_ptr(), 77, counters.as_mut_ptr()) },
        SAI_STATUS_NOT_SUPPORTED // C++ get_port_stats_ext: any mode but READ
    );
    assert_eq!(
        unsafe { clear_stats(ports[0], 2, ids.as_ptr()) },
        SAI_STATUS_NOT_SUPPORTED
    );
    let jabbers = [SAI_PORT_STAT_ETHER_STATS_JABBERS];
    counters = [7; 2];
    assert_eq!(
        unsafe { get_stats(ports[0], 1, jabbers.as_ptr(), counters.as_mut_ptr()) },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(counters, [7, 7], "a failed read wrote counters");
    let switch_stats = switch_api.get_switch_stats.unwrap();
    assert_eq!(
        unsafe { switch_stats(switch, 0, ptr::null(), ptr::null_mut()) },
        SAI_STATUS_NOT_SUPPORTED
    );

    // no entry of a table is null; what is not implemented says so and touches nothing
    let create_ports = port_api.create_ports.unwrap();
    let mut created = [0x55u64; 2];
    assert_eq!(
        unsafe {
            create_ports(
                switch,
                2,
                ptr::null(),
                ptr::null_mut(),
                SAI_BULK_OP_ERROR_MODE_STOP_ON_ERROR,
                created.as_mut_ptr(),
                ptr::null_mut(),
            )
        },
        SAI_STATUS_NOT_IMPLEMENTED
    );
    assert_eq!(created, [0x55; 2]);
    let mut tunnel = 0x55;
    assert_eq!(
        unsafe { switch_api.create_switch_tunnel.unwrap()(&raw mut tunnel, switch, 0, ptr::null()) },
        SAI_STATUS_NOT_IMPLEMENTED
    );
    assert_eq!(tunnel, 0x55);
    let mdio = switch_api.switch_mdio_read.unwrap();
    assert_eq!(
        unsafe { mdio(switch, 1, 2, 1, ptr::null_mut()) },
        SAI_STATUS_NOT_IMPLEMENTED
    );

    // capability queries
    let mut capability = sai_attr_capability_t {
        create_implemented: true,
        set_implemented: false,
        get_implemented: false,
    };
    assert_eq!(
        unsafe {
            sai_query_attribute_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, &raw mut capability)
        },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        (
            capability.create_implemented,
            capability.set_implemented,
            capability.get_implemented
        ),
        (false, true, true)
    );
    assert_eq!(
        unsafe { sai_query_attribute_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    let mut values = [0i32; 4];
    let mut list = sai_s32_list_t {
        count: 0,
        list: ptr::null_mut(),
    };
    assert_eq!(
        unsafe {
            sai_query_attribute_enum_values_capability(
                switch,
                SAI_OBJECT_TYPE_PORT,
                SAI_PORT_ATTR_FEC_MODE,
                &raw mut list,
            )
        },
        SAI_STATUS_BUFFER_OVERFLOW
    );
    assert_eq!(list.count, 3);
    list = sai_s32_list_t {
        count: 4,
        list: values.as_mut_ptr(),
    };
    assert_eq!(
        unsafe {
            sai_query_attribute_enum_values_capability(
                switch,
                SAI_OBJECT_TYPE_PORT,
                SAI_PORT_ATTR_FEC_MODE,
                &raw mut list,
            )
        },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(list.count, 3);
    assert_eq!(
        values[..3],
        [SAI_PORT_FEC_MODE_NONE, SAI_PORT_FEC_MODE_RS, SAI_PORT_FEC_MODE_FC].map(u32::cast_signed)
    );
    assert_eq!(
        unsafe {
            sai_query_attribute_enum_values_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_SPEED, &raw mut list)
        },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(
        unsafe {
            sai_query_attribute_enum_values_capability(
                switch,
                SAI_OBJECT_TYPE_PORT,
                SAI_PORT_ATTR_FEC_MODE,
                ptr::null_mut(),
            )
        },
        SAI_STATUS_INVALID_PARAMETER
    );
    let mut stats = [sai_stat_capability_t {
        stat_enum: 0,
        stat_modes: 0,
    }; 32];
    let mut stat_list = sai_stat_capability_list_t {
        count: 32,
        list: stats.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { sai_query_stats_capability(switch, SAI_OBJECT_TYPE_PORT, &raw mut stat_list) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(stat_list.count, 15);
    assert!(
        stats[..15]
            .iter()
            .any(|s| s.stat_enum == SAI_PORT_STAT_IF_IN_OCTETS && s.stat_modes & SAI_STATS_MODE_READ != 0)
    );
    assert_eq!(
        unsafe { sai_query_stats_capability(switch, SAI_OBJECT_TYPE_VLAN, &raw mut stat_list) },
        SAI_STATUS_NOT_SUPPORTED
    );
    let mut count = 5u32;
    assert_eq!(
        unsafe { sai_get_object_count(switch, SAI_OBJECT_TYPE_VLAN, &raw mut count) },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(count, 5);
}

#[test]
fn the_slice_through_the_c_abi() {
    let services = sai_service_method_table_t {
        profile_get_value: Some(profile_get_value),
        profile_get_next_value: None,
    };
    let mut p: *mut c_void = ptr::null_mut();

    // --- stage 0: nothing initialised
    assert_eq!(
        unsafe { sai_api_query(SAI_API_SWITCH, &raw mut p) },
        SAI_STATUS_UNINITIALIZED
    );
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_UNINITIALIZED);
    let mut version = 0;
    assert_eq!(unsafe { sai_query_api_version(&raw mut version) }, SAI_STATUS_SUCCESS);
    assert_eq!(version, 11902);
    assert_eq!(
        unsafe { sai_query_api_version(ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_log_set(SAI_API_SWITCH, SAI_LOG_LEVEL_ERROR) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe { sai_log_set(SAI_API_MAX + 1, SAI_LOG_LEVEL_ERROR) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(unsafe { sai_log_set(SAI_API_SWITCH, 77) }, SAI_STATUS_INVALID_PARAMETER);

    // --- stage 1: API initialisation
    assert_eq!(
        unsafe { sai_api_initialize(0, ptr::null()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_initialize(1, &raw const services) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const services) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const services) },
        SAI_STATUS_FAILURE
    );
    assert_eq!(
        unsafe { sai_api_query(SAI_API_SWITCH, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_query(SAI_API_TUNNEL, &raw mut p) },
        SAI_STATUS_NOT_IMPLEMENTED
    );
    assert_eq!(
        unsafe { sai_api_query(SAI_API_UNSPECIFIED, &raw mut p) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(unsafe { sai_api_query(9999, &raw mut p) }, SAI_STATUS_INVALID_PARAMETER);
    assert!(p.is_null(), "a failed query wrote its output");
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    let port_api: &sai_port_api_t = unsafe { table(SAI_API_PORT) };
    let vlan_api: &sai_vlan_api_t = unsafe { table(SAI_API_VLAN) };
    let bridge_api: &sai_bridge_api_t = unsafe { table(SAI_API_BRIDGE) };
    let create_switch = switch_api.create_switch.unwrap();
    let get_switch = switch_api.get_switch_attribute.unwrap();
    let get_port = port_api.get_port_attribute.unwrap();
    let set_port = port_api.set_port_attribute.unwrap();
    let create_vlan = vlan_api.create_vlan.unwrap();
    let remove_vlan = vlan_api.remove_vlan.unwrap();
    let get_vlan = vlan_api.get_vlan_attribute.unwrap();
    let create_member = vlan_api.create_vlan_member.unwrap();
    let remove_member = vlan_api.remove_vlan_member.unwrap();
    let get_bridge = bridge_api.get_bridge_attribute.unwrap();

    // objects before a switch exists
    let mut oid = 0;
    let mut vlan_id = attr(SAI_VLAN_ATTR_VLAN_ID);
    vlan_id.value.u16_ = 100;
    assert_eq!(
        unsafe { create_vlan(&raw mut oid, 0x21 << 56, 1, &raw const vlan_id) },
        SAI_STATUS_UNINITIALIZED
    );

    // --- stage 2: the switch
    let mut init = [
        attr(SAI_SWITCH_ATTR_INIT_SWITCH),
        attr(SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY),
    ];
    init[0].value.booldata = true;
    // One cast, kept: a function's address is only guaranteed equal to itself through one pointer value.
    let callback = on_port_state as *mut c_void;
    init[1].value.ptr = callback;
    let mut switch = 0;
    assert_eq!(
        unsafe { create_switch(ptr::null_mut(), 2, init.as_ptr()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 2, ptr::null()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 0, ptr::null()) },
        SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING
    );
    let unknown = [init[0], attr(0x0fff_fff0)];
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 2, unknown.as_ptr()) },
        SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 1 // C++ create_switch
    );
    // the virtual backend must be asked for
    OFFER_VIRTUAL.store(false, Ordering::SeqCst);
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 2, init.as_ptr()) },
        SAI_STATUS_FAILURE
    );
    OFFER_VIRTUAL.store(true, Ordering::SeqCst);
    assert_eq!(switch, 0, "a failed create wrote its output");
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 2, init.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { sai_object_type_query(switch) }, SAI_OBJECT_TYPE_SWITCH);
    assert_eq!(unsafe { sai_switch_id_query(switch) }, switch);
    assert_eq!(unsafe { sai_object_type_query(0) }, SAI_OBJECT_TYPE_NULL);
    assert_eq!(
        unsafe { create_switch(&raw mut oid, 2, init.as_ptr()) },
        SAI_STATUS_INSUFFICIENT_RESOURCES // C++: "only one switch is supported"
    );

    // scalar get; a get with no attributes; null list with a count
    let mut a = attr(SAI_SWITCH_ATTR_PORT_NUMBER);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.u32_ }, 4);
    assert_eq!(unsafe { get_switch(switch, 0, ptr::null_mut()) }, SAI_STATUS_SUCCESS);
    assert_eq!(
        unsafe { get_switch(switch, 1, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    // the callback comes back as the pointer that went in
    let mut a = attr(SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.ptr }, callback);

    // list get: size query (zero count, null buffer), too small, exact
    let mut a = attr(SAI_SWITCH_ATTR_PORT_LIST);
    a.value.objlist = sai_object_list_t {
        count: 0,
        list: ptr::null_mut(),
    };
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_BUFFER_OVERFLOW);
    assert_eq!(unsafe { a.value.objlist.count }, 4);
    let mut ports = [0xdead_u64; 4];
    a.value.objlist = sai_object_list_t {
        count: 3,
        list: ports.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_BUFFER_OVERFLOW);
    assert_eq!((unsafe { a.value.objlist.count }, ports), (4, [0xdead; 4]));
    a.value.objlist = sai_object_list_t {
        count: 4,
        list: ports.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.objlist.count }, 4);
    for port in ports {
        assert_eq!(unsafe { sai_object_type_query(port) }, SAI_OBJECT_TYPE_PORT);
        assert_eq!(unsafe { sai_switch_id_query(port) }, switch);
    }
    // several attributes, one list too small: the scalar is still answered, the count is set
    let mut two = [attr(SAI_SWITCH_ATTR_PORT_LIST), attr(SAI_SWITCH_ATTR_PORT_NUMBER)];
    two[0].value.objlist = sai_object_list_t {
        count: 1,
        list: ports.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { get_switch(switch, 2, two.as_mut_ptr()) },
        SAI_STATUS_BUFFER_OVERFLOW
    );
    assert_eq!(
        (unsafe { two[0].value.objlist.count }, unsafe { two[1].value.u32_ }),
        (4, 4)
    );
    // errors name the attribute's position
    let mut two = [
        attr(SAI_SWITCH_ATTR_PORT_NUMBER),
        attr(SAI_SWITCH_ATTR_MAX_STP_INSTANCE),
    ];
    assert_eq!(
        unsafe { get_switch(switch, 2, two.as_mut_ptr()) },
        SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 1
    );
    // a switch function given a port
    assert_eq!(
        unsafe { get_switch(ports[0], 1, &raw mut a) },
        SAI_STATUS_INVALID_OBJECT_ID // C++: the switch code looks the id up as a switch
    );

    // ports: lanes, admin state with its notification
    let mut lanes = [0u32; 8];
    let mut a = attr(SAI_PORT_ATTR_HW_LANE_LIST);
    a.value.u32list = sai_u32_list_t {
        count: 8,
        list: lanes.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_port(ports[0], 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!((unsafe { a.value.u32list.count }, &lanes[..4]), (4, &[1, 2, 3, 4][..]));
    let mut a = attr(SAI_PORT_ATTR_OPER_STATUS);
    assert_eq!(unsafe { get_port(ports[1], 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.s32 }, SAI_PORT_OPER_STATUS_DOWN.cast_signed());
    assert_eq!(unsafe { set_port(ports[1], ptr::null()) }, SAI_STATUS_INVALID_PARAMETER);
    let mut admin = attr(SAI_PORT_ATTR_ADMIN_STATE);
    admin.value.booldata = true;
    assert_eq!(unsafe { set_port(ports[1], &raw const admin) }, SAI_STATUS_SUCCESS);
    wait_for_event((ports[1], SAI_PORT_OPER_STATUS_UP));
    assert_eq!(unsafe { get_port(ports[1], 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { a.value.s32 }, SAI_PORT_OPER_STATUS_UP.cast_signed());
    admin.value.booldata = false;
    assert_eq!(unsafe { set_port(ports[1], &raw const admin) }, SAI_STATUS_SUCCESS);
    wait_for_event((ports[1], SAI_PORT_OPER_STATUS_DOWN));
    let lanes_ro = attr(SAI_PORT_ATTR_HW_LANE_LIST);
    assert_eq!(
        unsafe { set_port(ports[1], &raw const lanes_ro) },
        SAI_STATUS_ATTR_NOT_SUPPORTED_0 // C++ port_set_one's default
    );

    // VLAN and member
    assert_eq!(
        unsafe { create_vlan(ptr::null_mut(), switch, 1, &raw const vlan_id) },
        SAI_STATUS_INVALID_PARAMETER
    );
    let mut vlan = 0;
    assert_eq!(
        unsafe { create_vlan(&raw mut vlan, switch, 1, &raw const vlan_id) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { sai_object_type_query(vlan) }, SAI_OBJECT_TYPE_VLAN);
    assert_eq!(
        unsafe { create_vlan(&raw mut oid, switch, 1, &raw const vlan_id) },
        SAI_STATUS_ITEM_ALREADY_EXISTS
    );
    let mut a = attr(SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let bridge = unsafe { a.value.oid };
    let mut bridge_ports = [0u64; 4];
    let mut a = attr(SAI_BRIDGE_ATTR_PORT_LIST);
    a.value.objlist = sai_object_list_t {
        count: 4,
        list: bridge_ports.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_bridge(bridge, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    let mut member_attrs = [
        attr(SAI_VLAN_MEMBER_ATTR_VLAN_ID),
        attr(SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID),
        attr(SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE),
    ];
    member_attrs[0].value.oid = vlan;
    member_attrs[1].value.oid = ports[0]; // a port where a bridge port belongs
    member_attrs[2].value.s32 = SAI_VLAN_TAGGING_MODE_TAGGED.cast_signed();
    let mut member = 0;
    assert_eq!(
        unsafe { create_member(&raw mut member, switch, 3, member_attrs.as_ptr()) },
        SAI_STATUS_INVALID_ATTR_VALUE_0 - 1
    );
    member_attrs[1].value.oid = bridge_ports[0];
    assert_eq!(
        unsafe { create_member(&raw mut member, switch, 3, member_attrs.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    let mut a = attr(SAI_VLAN_ATTR_MEMBER_LIST);
    let mut members = [0u64; 2];
    a.value.objlist = sai_object_list_t {
        count: 2,
        list: members.as_mut_ptr(),
    };
    assert_eq!(unsafe { get_vlan(vlan, 1, &raw mut a) }, SAI_STATUS_SUCCESS);
    assert_eq!((unsafe { a.value.objlist.count }, members[0]), (1, member));
    // lifecycle errors
    assert_eq!(unsafe { remove_vlan(vlan) }, SAI_STATUS_OBJECT_IN_USE);
    assert_eq!(unsafe { remove_vlan(member) }, SAI_STATUS_INVALID_OBJECT_ID); // C++: no such VLAN
    assert_eq!(unsafe { remove_vlan(0) }, SAI_STATUS_INVALID_OBJECT_ID);
    assert_eq!(unsafe { remove_member(member) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { remove_member(member) }, SAI_STATUS_INVALID_OBJECT_ID);
    assert_eq!(unsafe { remove_vlan(vlan) }, SAI_STATUS_SUCCESS);

    objects_entries_and_counters(switch, &ports);
    acl_through_the_c_abi(switch, &ports);

    // --- teardown: remove, then everything answers "uninitialized"; the API can start over
    let remove_switch = switch_api.remove_switch.unwrap();
    assert_eq!(unsafe { remove_switch(vlan) }, SAI_STATUS_INVALID_OBJECT_ID); // C++
    assert_eq!(unsafe { remove_switch(switch) }, SAI_STATUS_SUCCESS);
    let mut a = attr(SAI_SWITCH_ATTR_PORT_NUMBER);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_UNINITIALIZED);
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { get_switch(switch, 1, &raw mut a) }, SAI_STATUS_UNINITIALIZED);

    // uninitialize with a switch still up: the switch and its notifier thread go with it
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const services) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe { create_switch(&raw mut switch, 2, init.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    admin.value.booldata = true;
    PORT_EVENTS.lock().unwrap().clear();
    assert_eq!(unsafe { set_port(ports[2], &raw const admin) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_SUCCESS);
    let delivered = PORT_EVENTS.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(
        PORT_EVENTS.lock().unwrap().len(),
        delivered,
        "a callback ran after uninitialize returned"
    );
}

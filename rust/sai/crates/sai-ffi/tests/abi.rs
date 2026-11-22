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

unsafe fn table<T>(api: sai_api_t) -> &'static T {
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(unsafe { sai_api_query(api, &raw mut p) }, SAI_STATUS_SUCCESS);
    unsafe { p.cast::<T>().as_ref() }.unwrap()
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
        unsafe { sai_api_query(SAI_API_ROUTE, &raw mut p) },
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
        SAI_STATUS_UNKNOWN_ATTRIBUTE_0 - 1
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
        SAI_STATUS_ITEM_ALREADY_EXISTS
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
        attr(SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID),
    ];
    assert_eq!(
        unsafe { get_switch(switch, 2, two.as_mut_ptr()) },
        SAI_STATUS_ATTR_NOT_SUPPORTED_0 - 1
    );
    // a switch function given a port
    assert_eq!(
        unsafe { get_switch(ports[0], 1, &raw mut a) },
        SAI_STATUS_INVALID_OBJECT_TYPE
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
        SAI_STATUS_INVALID_ATTRIBUTE_0
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
    assert_eq!(unsafe { remove_vlan(member) }, SAI_STATUS_INVALID_OBJECT_TYPE);
    assert_eq!(unsafe { remove_vlan(0) }, SAI_STATUS_INVALID_OBJECT_ID);
    assert_eq!(unsafe { remove_member(member) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { remove_member(member) }, SAI_STATUS_INVALID_OBJECT_ID);
    assert_eq!(unsafe { remove_vlan(vlan) }, SAI_STATUS_SUCCESS);

    // --- teardown: remove, then everything answers "uninitialized"; the API can start over
    let remove_switch = switch_api.remove_switch.unwrap();
    assert_eq!(unsafe { remove_switch(vlan) }, SAI_STATUS_INVALID_OBJECT_TYPE);
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

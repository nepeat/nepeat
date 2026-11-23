//! The global entry points as the C++ sai-openbcm answers them (`sai_api.cpp`, `create_switch`
//! and `pick_backend` of `sai_switch.cpp`; `tests/test_sai.cpp` `test_oid_encoding`,
//! `test_init_and_backend_selection`, the dump of `test_dump_and_teardown`, the capability
//! queries of `test_capabilities`), through the exported C functions. One test function: the
//! library's state is global.
#![allow(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    clippy::missing_safety_doc
)]
// Every unsafe block here is a C caller keeping the SAI contract (or deliberately passing
// null where the library must answer with an error).

// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
use core::ffi::{c_char, c_int, c_void};
use sai as _; // link the library: it defines the sai_* symbols declared in sai-sys
use sai_sys::*;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The SAI user's profile: `BOTH` answers `profile_get_value` and the enumeration,
/// `NEXT_ONLY` only the enumeration (pairs in this order, after `BOTH`'s).
struct Profile {
    both: BTreeMap<String, CString>,
    next_only: Vec<(CString, CString)>,
    /// The enumeration: the pairs and the position, set by the restart call.
    cursor: Vec<(CString, CString)>,
    at: usize,
}

static PROFILE: Mutex<Profile> = Mutex::new(Profile {
    both: BTreeMap::new(),
    next_only: Vec::new(),
    cursor: Vec::new(),
    at: 0,
});
/// An enumeration that never ends (answers 0 with nothing): the library must stop by itself.
static ENDLESS: AtomicBool = AtomicBool::new(false);
static NEXT_CALLS: AtomicUsize = AtomicUsize::new(0);

fn set(key: &str, value: &str) {
    PROFILE
        .lock()
        .unwrap()
        .both
        .insert(key.to_owned(), CString::new(value).unwrap());
}

fn unset(key: &str) {
    PROFILE.lock().unwrap().both.remove(key);
}

fn set_next_only(pairs: &[(&str, &str)]) {
    PROFILE.lock().unwrap().next_only = pairs
        .iter()
        .map(|(k, v)| (CString::new(*k).unwrap(), CString::new(*v).unwrap()))
        .collect();
}

unsafe extern "C" fn get_value(_profile: sai_switch_profile_id_t, variable: *const c_char) -> *const c_char {
    let key = unsafe { CStr::from_ptr(variable) }.to_str().unwrap();
    // The CString's buffer outlives the call: the map is changed only between SAI calls.
    PROFILE
        .lock()
        .unwrap()
        .both
        .get(key)
        .map_or(ptr::null(), |v| v.as_ptr())
}

unsafe extern "C" fn get_next_value(
    _profile: sai_switch_profile_id_t,
    variable: *mut *const c_char,
    value: *mut *const c_char,
) -> c_int {
    NEXT_CALLS.fetch_add(1, Ordering::SeqCst);
    let mut p = PROFILE.lock().unwrap();
    if variable.is_null() || value.is_null() {
        let mut pairs: Vec<(CString, CString)> = p
            .both
            .iter()
            .map(|(k, v)| (CString::new(k.as_str()).unwrap(), v.clone()))
            .collect();
        pairs.extend(p.next_only.iter().cloned());
        p.cursor = pairs;
        p.at = 0;
        return 0;
    }
    if ENDLESS.load(Ordering::SeqCst) {
        return 0;
    }
    let at = p.at;
    let Some((k, v)) = p.cursor.get(at) else {
        return -1;
    };
    unsafe {
        *variable = k.as_ptr();
        *value = v.as_ptr();
    }
    p.at += 1;
    0
}

static SERVICES: sai_service_method_table_t = sai_service_method_table_t {
    profile_get_value: Some(get_value),
    profile_get_next_value: Some(get_next_value),
};

fn attr(id: sai_attr_id_t) -> sai_attribute_t {
    sai_attribute_t {
        id,
        value: sai_attribute_value_t { u64_: 0 },
    }
}

fn init_switch(on: bool) -> sai_attribute_t {
    let mut a = attr(SAI_SWITCH_ATTR_INIT_SWITCH);
    a.value.booldata = on;
    a
}

fn type_of(oid: sai_object_id_t) -> sai_object_type_t {
    unsafe { sai_object_type_query(oid) }
}

fn switch_of(oid: sai_object_id_t) -> sai_object_id_t {
    unsafe { sai_switch_id_query(oid) }
}

fn log_set(api: sai_api_t, level: sai_log_level_t) -> sai_status_t {
    unsafe { sai_log_set(api, level) }
}

/// The C++ test port's `make_oid`: type in bits 63..56, switch in 55..48, index below.
fn make_oid(object_type: sai_object_type_t, index: u64, switch: u64) -> sai_object_id_t {
    (u64::from(object_type) << 56) | (switch << 48) | index
}

unsafe fn table<T>(api: sai_api_t) -> &'static T {
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(unsafe { sai_api_query(api, &raw mut p) }, SAI_STATUS_SUCCESS);
    unsafe { p.cast::<T>().as_ref() }.unwrap()
}

fn dump_path(name: &str) -> CString {
    CString::new(format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"))).unwrap()
}

/// The dump's lines, through `sai_dbg_generate_dump`.
fn dump() -> Vec<String> {
    let path = dump_path("global-a1-dump.txt");
    assert_eq!(unsafe { sai_dbg_generate_dump(path.as_ptr()) }, SAI_STATUS_SUCCESS);
    let text = std::fs::read_to_string(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(path.to_str().unwrap()).unwrap();
    text.lines().map(str::to_owned).collect()
}

fn create_switch(attrs: &[sai_attribute_t]) -> (sai_status_t, sai_object_id_t) {
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    let mut id = 0;
    let count = u32::try_from(attrs.len()).unwrap();
    let status = unsafe { switch_api.create_switch.unwrap()(&raw mut id, count, attrs.as_ptr()) };
    (status, id)
}

fn remove_switch(id: sai_object_id_t) {
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    assert_eq!(unsafe { switch_api.remove_switch.unwrap()(id) }, SAI_STATUS_SUCCESS);
}

/// `test_oid_encoding`: the queries decode ids only.
fn oid_queries() {
    let o = make_oid(SAI_OBJECT_TYPE_PORT, 50, 3);
    assert_eq!(type_of(o), SAI_OBJECT_TYPE_PORT);
    assert_eq!(switch_of(o), make_oid(SAI_OBJECT_TYPE_SWITCH, 1, 3));
    assert_eq!(type_of(0), SAI_OBJECT_TYPE_NULL); // SAI_NULL_OBJECT_ID
    assert_eq!(switch_of(0), 0);
    assert_eq!(type_of(0xff00_0000_0000_0001), SAI_OBJECT_TYPE_NULL); // type out of range
    assert_eq!(switch_of(0xff00_0000_0000_0001), 0);
    let big = make_oid(SAI_OBJECT_TYPE_FDB_FLUSH, (1 << 48) - 1, 255);
    assert_eq!(type_of(big), SAI_OBJECT_TYPE_FDB_FLUSH);
    assert_eq!(switch_of(big), make_oid(SAI_OBJECT_TYPE_SWITCH, 1, 255));
}

/// `test_capabilities` (last lines) and the capability queries before a switch exists.
fn capability_queries(switch: sai_object_id_t) {
    let mut cap = sai_attr_capability_t {
        create_implemented: true,
        set_implemented: true,
        get_implemented: true,
    };
    assert_eq!(
        unsafe {
            sai_query_attribute_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_ADMIN_STATE, &raw mut cap)
        },
        SAI_STATUS_SUCCESS
    );
    assert!(cap.set_implemented && cap.get_implemented && !cap.create_implemented);
    assert_eq!(
        unsafe {
            sai_query_attribute_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_PRBS_CONFIG, &raw mut cap)
        },
        SAI_STATUS_SUCCESS
    );
    assert!(!cap.set_implemented && !cap.get_implemented && !cap.create_implemented);
    assert_eq!(
        unsafe {
            sai_query_attribute_capability(switch, SAI_OBJECT_TYPE_PORT, SAI_PORT_ATTR_ADMIN_STATE, ptr::null_mut())
        },
        SAI_STATUS_INVALID_PARAMETER
    );

    let mut values = [0i32; 8];
    let mut list = sai_s32_list_t {
        count: 8,
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
    let fec = [SAI_PORT_FEC_MODE_NONE, SAI_PORT_FEC_MODE_RS, SAI_PORT_FEC_MODE_FC].map(u32::cast_signed);
    assert_eq!(values[..3], fec);
    list.count = 2;
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
    let mut count = 7u64;
    assert_eq!(
        unsafe {
            sai_object_type_get_availability(switch, SAI_OBJECT_TYPE_ROUTE_ENTRY, 0, ptr::null(), &raw mut count)
        },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(count, 7);
}

/// Port counters as `sai_query_stats_capability` lists them; checks the modes are `READ` only.
fn port_stats(switch: sai_object_id_t) -> Vec<sai_stat_id_t> {
    let mut stats = vec![
        sai_stat_capability_t {
            stat_enum: 0,
            stat_modes: 0
        };
        512
    ];
    let mut list = sai_stat_capability_list_t {
        count: 512,
        list: stats.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { sai_query_stats_capability(switch, SAI_OBJECT_TYPE_PORT, &raw mut list) },
        SAI_STATUS_SUCCESS
    );
    stats.truncate(usize::try_from(list.count).unwrap());
    assert!(
        stats.iter().all(|s| s.stat_modes == SAI_STATS_MODE_READ),
        "{:?}",
        stats.len()
    );
    stats.iter().map(|s| s.stat_enum).collect()
}

fn stats_status(object_type: sai_object_type_t) -> sai_status_t {
    let mut stats = [sai_stat_capability_t {
        stat_enum: 0,
        stat_modes: 0,
    }; 64];
    let mut list = sai_stat_capability_list_t {
        count: 64,
        list: stats.as_mut_ptr(),
    };
    unsafe { sai_query_stats_capability(0, object_type, &raw mut list) }
}

#[test]
fn the_global_entry_points_answer_as_the_cpp() {
    oid_queries();

    // --- before sai_api_initialize
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(
        unsafe { sai_api_query(SAI_API_SWITCH, &raw mut p) },
        SAI_STATUS_UNINITIALIZED
    );
    let path = dump_path("never-written.txt");
    assert_eq!(
        unsafe { sai_dbg_generate_dump(path.as_ptr()) },
        SAI_STATUS_UNINITIALIZED
    );
    assert!(!std::path::Path::new(path.to_str().unwrap()).exists());
    assert_eq!(
        unsafe { sai_dbg_generate_dump(ptr::null()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    // the capability tables need no initialisation; port counters: the 15 IF_* ones
    capability_queries(0);
    let before = port_stats(0);
    assert_eq!(before.len(), 15);
    assert!(before.contains(&SAI_PORT_STAT_IF_IN_OCTETS));
    assert_eq!(stats_status(SAI_OBJECT_TYPE_QUEUE), SAI_STATUS_UNINITIALIZED);
    assert_eq!(stats_status(SAI_OBJECT_TYPE_ROUTER_INTERFACE), SAI_STATUS_SUCCESS);
    assert_eq!(stats_status(SAI_OBJECT_TYPE_VLAN), SAI_STATUS_NOT_SUPPORTED);
    assert_eq!(
        unsafe { sai_query_stats_capability(0, SAI_OBJECT_TYPE_PORT, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );

    // --- log levels: api in UNSPECIFIED..=MAX, level in DEBUG..=CRITICAL
    assert_eq!(log_set(SAI_API_PORT, SAI_LOG_LEVEL_DEBUG), SAI_STATUS_SUCCESS);
    assert_eq!(log_set(9999, SAI_LOG_LEVEL_DEBUG), SAI_STATUS_INVALID_PARAMETER);
    assert_eq!(
        log_set(SAI_API_MAX + 1, SAI_LOG_LEVEL_DEBUG),
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(log_set(SAI_API_MAX, SAI_LOG_LEVEL_DEBUG), SAI_STATUS_SUCCESS);
    assert_eq!(log_set(SAI_API_UNSPECIFIED, SAI_LOG_LEVEL_CRITICAL), SAI_STATUS_SUCCESS);
    assert_eq!(
        log_set(SAI_API_PORT, SAI_LOG_LEVEL_CRITICAL + 1),
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(log_set(SAI_API_PORT, SAI_LOG_LEVEL_WARN), SAI_STATUS_SUCCESS);
    assert_eq!(log_set(SAI_API_MAX, SAI_LOG_LEVEL_WARN), SAI_STATUS_SUCCESS);
    assert_eq!(log_set(SAI_API_UNSPECIFIED, SAI_LOG_LEVEL_WARN), SAI_STATUS_SUCCESS);

    // --- sai_api_initialize, in the C++ order
    assert_eq!(
        unsafe { sai_api_initialize(0, ptr::null()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    let no_get_value = sai_service_method_table_t {
        profile_get_value: None,
        profile_get_next_value: Some(get_next_value),
    };
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const no_get_value) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_initialize(1, &raw const SERVICES) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const SERVICES) },
        SAI_STATUS_SUCCESS
    );
    // already initialised comes first
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const SERVICES) },
        SAI_STATUS_FAILURE
    );
    assert_eq!(unsafe { sai_api_initialize(1, ptr::null()) }, SAI_STATUS_FAILURE);
    assert_eq!(
        unsafe { sai_api_query(SAI_API_SWITCH, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { sai_api_query(SAI_API_TUNNEL, &raw mut p) },
        SAI_STATUS_NOT_IMPLEMENTED
    );
    let mut version = 0;
    assert_eq!(unsafe { sai_query_api_version(&raw mut version) }, SAI_STATUS_SUCCESS);
    assert!(version >= 10900);

    // --- no switch yet: the dump says so; a port get is UNINITIALIZED
    assert_eq!(
        dump()[..3],
        ["sai-openbcm dump", "backend none", "switch 0 mac 02:00:00:00:00:01"]
    );
    assert_eq!(stats_status(SAI_OBJECT_TYPE_QUEUE), SAI_STATUS_NOT_SUPPORTED);
    assert_eq!(port_stats(0).len(), 15);
    let port_api: &sai_port_api_t = unsafe { table(SAI_API_PORT) };
    let mut q = attr(SAI_PORT_ATTR_SPEED);
    assert_eq!(
        unsafe { port_api.get_port_attribute.unwrap()(make_oid(SAI_OBJECT_TYPE_PORT, 1, 0), 1, &raw mut q) },
        SAI_STATUS_UNINITIALIZED
    );

    // --- backend selection (no key: this library has no openbcm backend)
    let on = [init_switch(true)];
    assert_eq!(create_switch(&on).0, SAI_STATUS_FAILURE);
    set("SAI_OPENBCM_BACKEND", "openbcm");
    assert_eq!(create_switch(&on).0, SAI_STATUS_FAILURE);
    set("SAI_OPENBCM_BACKEND", "bogus");
    assert_eq!(create_switch(&on).0, SAI_STATUS_INVALID_PARAMETER);
    // the Rust key wins over the C++ one
    set("SAI_OPENBCM_BACKEND", "fake");
    set("SAI_RS_BACKEND", "bogus");
    assert_eq!(create_switch(&on).0, SAI_STATUS_INVALID_PARAMETER);
    unset("SAI_RS_BACKEND");
    // a bad attribute or board key wins over a bad backend name (C++ order)
    set("SAI_OPENBCM_BACKEND", "bogus");
    assert_eq!(create_switch(&[init_switch(false)]).0, SAI_STATUS_NOT_SUPPORTED);
    unset("SAI_OPENBCM_BACKEND");
    set("SAI_OPENBCM_PORT_FEC", "bogus");
    assert_eq!(create_switch(&on).0, SAI_STATUS_INVALID_PARAMETER); // not FAILURE (no backend)
    unset("SAI_OPENBCM_PORT_FEC");
    // the pointers first
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    let create = switch_api.create_switch.unwrap();
    let mut id = 0;
    assert_eq!(
        unsafe { create(ptr::null_mut(), 1, on.as_ptr()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { create(&raw mut id, 1, ptr::null()) },
        SAI_STATUS_INVALID_PARAMETER
    );

    set("SAI_OPENBCM_BACKEND", "fake");
    // the fake's netdev / wire keys: bad values refuse the switch
    for (key, value) in [
        ("SAI_OPENBCM_FAKE_NETDEV", "bogus"),
        ("SAI_OPENBCM_FAKE_WIRE", "Ethernet0:eth1"), // a wire needs NETDEV=tap
        ("SAI_OPENBCM_FAKE_WIRE", "Ethernet0"),
        ("SAI_OPENBCM_FAKE_WIRE", "Ethernet0:eth1,Ethernet4:eth1"),
    ] {
        set(key, value);
        assert_eq!(create_switch(&on).0, SAI_STATUS_INVALID_PARAMETER, "{key}={value}");
        assert_eq!(dump()[1], "backend none", "{key}={value}");
        unset(key);
    }
    assert_eq!(create_switch(&[init_switch(false)]).0, SAI_STATUS_NOT_SUPPORTED);
    assert_eq!(
        unsafe { create(&raw mut id, 0, ptr::null()) },
        SAI_STATUS_MANDATORY_ATTRIBUTE_MISSING
    );
    assert_eq!(dump()[1], "backend none");
    assert_eq!(id, 0, "a failed create wrote its output");

    // --- the profile: enumerated pairs reach the backend; an explicit key's value wins
    set_next_only(&[("SAI_OPENBCM_FAKE_PORTS", "4"), ("SAI_OPENBCM_FAKE_LANES", "2")]);
    set("SAI_OPENBCM_FAKE_LANES", "1");
    let calls = NEXT_CALLS.load(Ordering::SeqCst);
    let (status, switch) = create_switch(&on);
    assert_eq!(status, SAI_STATUS_SUCCESS);
    // one restart, one call per pair, one that ends it
    let pairs = PROFILE.lock().unwrap().cursor.len();
    assert_eq!(NEXT_CALLS.load(Ordering::SeqCst) - calls, pairs + 2);
    let mut a = attr(SAI_SWITCH_ATTR_PORT_NUMBER);
    assert_eq!(
        unsafe { switch_api.get_switch_attribute.unwrap()(switch, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { a.value.u32_ }, 4);
    assert_eq!(create_switch(&on).0, SAI_STATUS_INSUFFICIENT_RESOURCES); // C++: one switch

    // --- the dump of a switch
    let lines = dump();
    assert_eq!(lines[0], "sai-openbcm dump");
    assert_eq!(lines[1], "backend fake");
    assert!(lines[2].starts_with(&format!("switch {switch:x} mac ")), "{}", lines[2]);
    let ports: Vec<&String> = lines.iter().filter(|l| l.starts_with("port ")).collect();
    assert_eq!(ports.len(), 4);
    assert!(
        ports[0].ends_with(" hw 1 xe0 lanes 1 admin 0 oper 2 pvid 1 mtu 1514"),
        "{}",
        ports[0]
    );
    assert!(
        ports[3].ends_with(" hw 4 xe3 lanes 4 admin 0 oper 2 pvid 1 mtu 1514"),
        "{}",
        ports[3]
    );
    assert_eq!(lines[7], "vlan 1 members 4");
    assert!(
        lines[8].starts_with("bridge_ports 4 fdb 0 hostifs 0 traps 0 "),
        "{}",
        lines[8]
    );
    assert!(lines[9].starts_with("policers 0 acl_tables 0 "), "{}", lines[9]);
    assert!(lines[9].ends_with(" acl_bindings 0 copp_entries 0"), "{}", lines[9]);
    // a file that cannot be written
    let bad = CString::new("/nonexistent-directory/dump.txt").unwrap();
    assert_eq!(unsafe { sai_dbg_generate_dump(bad.as_ptr()) }, SAI_STATUS_FAILURE);

    // --- capability queries with a switch
    capability_queries(switch);
    let with_switch = port_stats(switch);
    assert!(with_switch.contains(&SAI_PORT_STAT_IF_IN_OCTETS));
    // clear_port_all_stats: a port, then an id that is not one
    let mut list = [0u64; 4];
    let mut a = attr(SAI_SWITCH_ATTR_PORT_LIST);
    a.value.objlist = sai_object_list_t {
        count: 4,
        list: list.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { switch_api.get_switch_attribute.unwrap()(switch, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    let clear_all = port_api.clear_port_all_stats.unwrap();
    assert_eq!(unsafe { clear_all(list[0]) }, SAI_STATUS_SUCCESS);
    assert_ne!(unsafe { clear_all(switch) }, SAI_STATUS_SUCCESS);
    remove_switch(switch);
    assert_eq!(unsafe { clear_all(list[0]) }, SAI_STATUS_UNINITIALIZED);

    // --- an enumeration that never ends stops after 4096 calls
    ENDLESS.store(true, Ordering::SeqCst);
    let calls = NEXT_CALLS.load(Ordering::SeqCst);
    let (status, switch) = create_switch(&on);
    assert_eq!(status, SAI_STATUS_SUCCESS);
    assert_eq!(NEXT_CALLS.load(Ordering::SeqCst) - calls, 1 + 4096);
    ENDLESS.store(false, Ordering::SeqCst);
    // the explicit keys still arrive: 32 ports by default, LANES=1 asked one by one
    let mut a = attr(SAI_SWITCH_ATTR_PORT_NUMBER);
    assert_eq!(
        unsafe { switch_api.get_switch_attribute.unwrap()(switch, 1, &raw mut a) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { a.value.u32_ }, 32);

    // --- uninitialize removes the switch
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_UNINITIALIZED);
    assert_eq!(
        unsafe { sai_dbg_generate_dump(path.as_ptr()) },
        SAI_STATUS_UNINITIALIZED
    );
}

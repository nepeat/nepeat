//! The MACSEC API through the exported C functions and its method table, from Rust: a switch
//! with an external `MACsec` table (`SAI_OPENBCM_EXT_MACSEC_FILE`), the five object types, the
//! secret members (`SAK`, `AUTH_KEY`) read from the caller's union, the write-only answer, the C++
//! glue's pointer and mode rules. One test function: the library's state is global.
#![allow(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    clippy::missing_safety_doc,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    missing_docs
)]
// Every unsafe block here is a C caller keeping the SAI contract (or deliberately passing null
// where the library must answer with an error).
use core::ffi::{c_char, c_void};
use sai as _; // link the library
use sai_sys::*;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::OnceLock;

static TABLE: OnceLock<CString> = OnceLock::new();

/// A table with one port's egress engine and every sequence an egress SA needs.
const TABLE_TEXT: &str = "ext-macsec 1\nifg-bits 384\nport 1 10\n\
    seq reset.egress\nW p 1 110 1\nseq enable.egress\nW p 1 100 0\nseq disable.egress\nW p 1 100 1\n\
    seq gate-open.egress\nW p 1 120 1\nseq gate-close.egress\nW p 1 120 0\n\
    seq sa-add-first.egress.0.gcm-aes-128\nW p 1 200 100\nW p 1 201 key[1].key[0]\nW p 1 203 hkey[15].hkey[0]\n\
    seq sa-del-last.egress.0\nW p 1 300 8001\n\
    seq stats-flow.egress\nX p 1 4f0 control_pkts[0]\n";

unsafe extern "C" fn profile_get_value(_profile: sai_switch_profile_id_t, variable: *const c_char) -> *const c_char {
    let key = unsafe { CStr::from_ptr(variable) };
    match key.to_bytes() {
        b"SAI_RS_BACKEND" => c"virtual".as_ptr(),
        b"SAI_RS_VIRTUAL_PORTS" => c"4".as_ptr(),
        b"SAI_OPENBCM_EXT_MACSEC_FILE" => TABLE.get().map_or(ptr::null(), |p| p.as_ptr()),
        _ => ptr::null(),
    }
}

fn attr(id: sai_attr_id_t) -> sai_attribute_t {
    sai_attribute_t {
        id,
        value: sai_attribute_value_t { u64_: 0 },
    }
}

fn s32(id: sai_attr_id_t, v: u32) -> sai_attribute_t {
    let mut a = attr(id);
    a.value.s32 = v.cast_signed();
    a
}

fn oid(id: sai_attr_id_t, v: sai_object_id_t) -> sai_attribute_t {
    let mut a = attr(id);
    a.value.oid = v;
    a
}

unsafe fn table<T>(api: sai_api_t) -> &'static T {
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(unsafe { sai_api_query(api, &raw mut p) }, SAI_STATUS_SUCCESS);
    unsafe { p.cast::<T>().as_ref() }.unwrap()
}

#[test]
fn the_macsec_api_through_the_c_abi() {
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, Into::into);
    let path = dir.join(format!("sai-ffi-macsec-{}.tbl", std::process::id()));
    std::fs::write(&path, TABLE_TEXT).unwrap();
    TABLE.set(CString::new(path.to_str().unwrap()).unwrap()).unwrap();
    let services = sai_service_method_table_t {
        profile_get_value: Some(profile_get_value),
        profile_get_next_value: None,
    };
    assert_eq!(
        unsafe { sai_api_initialize(0, &raw const services) },
        SAI_STATUS_SUCCESS
    );
    let switch_api: &sai_switch_api_t = unsafe { table(SAI_API_SWITCH) };
    let ms: &sai_macsec_api_t = unsafe { table(SAI_API_MACSEC) };
    let mut init = attr(SAI_SWITCH_ATTR_INIT_SWITCH);
    init.value.booldata = true;
    let mut switch = 0;
    assert_eq!(
        unsafe { switch_api.create_switch.unwrap()(&raw mut switch, 1, &raw const init) },
        SAI_STATUS_SUCCESS
    );
    let mut ports = [0; 8];
    let mut list = attr(SAI_SWITCH_ATTR_PORT_LIST);
    list.value.objlist = sai_object_list_t {
        count: 8,
        list: ports.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { switch_api.get_switch_attribute.unwrap()(switch, 1, &raw mut list) },
        SAI_STATUS_SUCCESS
    );
    let mut id = 0;
    let dir = s32(SAI_MACSEC_ATTR_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS);
    assert_eq!(
        unsafe { ms.create_macsec.unwrap()(&raw mut id, switch, 1, &raw const dir) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(unsafe { sai_object_type_query(id) }, SAI_OBJECT_TYPE_MACSEC);
    // the model's statuses, not the generic metadata checks': a second egress MACSEC
    assert_eq!(
        unsafe { ms.create_macsec.unwrap()(&raw mut id, switch, 1, &raw const dir) },
        SAI_STATUS_ITEM_ALREADY_EXISTS
    );
    let mp = [
        s32(SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS),
        oid(SAI_MACSEC_PORT_ATTR_PORT_ID, ports[0]),
    ];
    let mut mport = 0;
    assert_eq!(
        unsafe { ms.create_macsec_port.unwrap()(&raw mut mport, switch, 2, mp.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    let fa = s32(SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS);
    let mut flow = 0;
    assert_eq!(
        unsafe { ms.create_macsec_flow.unwrap()(&raw mut flow, switch, 1, &raw const fa) },
        SAI_STATUS_SUCCESS
    );
    let mut sci = attr(SAI_MACSEC_SC_ATTR_MACSEC_SCI);
    sci.value.u64_ = 1;
    let sca = [
        s32(SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS),
        oid(SAI_MACSEC_SC_ATTR_FLOW_ID, flow),
        sci,
        s32(
            SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
            SAI_MACSEC_CIPHER_SUITE_GCM_AES_128,
        ),
    ];
    let mut sc = 0;
    assert_eq!(
        unsafe { ms.create_macsec_sc.unwrap()(&raw mut sc, switch, 4, sca.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    // an SA: the SAK and AUTH_KEY travel in their union members
    let mut sak = attr(SAI_MACSEC_SA_ATTR_SAK);
    sak.value.macsecsak = [0xc0; 32];
    let mut auth = attr(SAI_MACSEC_SA_ATTR_AUTH_KEY);
    auth.value.macsecauthkey = [0x80; 16];
    let mut an = attr(SAI_MACSEC_SA_ATTR_AN);
    an.value.u8_ = 0;
    let mut pn = attr(SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN);
    pn.value.u64_ = 7;
    let saa = [
        s32(SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, SAI_MACSEC_DIRECTION_EGRESS),
        oid(SAI_MACSEC_SA_ATTR_SC_ID, sc),
        an,
        pn,
        sak,
        auth,
    ];
    let mut sa = 0;
    assert_eq!(
        unsafe { ms.create_macsec_sa.unwrap()(&raw mut sa, switch, 6, saa.as_ptr()) },
        SAI_STATUS_SUCCESS
    );
    // write-only key, the configured PN while it waits for its flow
    let mut got = [attr(SAI_MACSEC_SA_ATTR_SAK)];
    assert_eq!(
        unsafe { ms.get_macsec_sa_attribute.unwrap()(sa, 1, got.as_mut_ptr()) },
        SAI_STATUS_ATTR_NOT_SUPPORTED_0
    );
    let mut got = [attr(SAI_MACSEC_SA_ATTR_CURRENT_XPN), attr(SAI_MACSEC_SA_ATTR_AN)];
    assert_eq!(
        unsafe { ms.get_macsec_sa_attribute.unwrap()(sa, 2, got.as_mut_ptr()) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!((unsafe { got[0].value.u64_ }, unsafe { got[1].value.u8_ }), (7, 0));
    // pointer rules of the C++ glue: a null list looks the object up first
    assert_eq!(
        unsafe { ms.get_macsec_sa_attribute.unwrap()(sa, 1, ptr::null_mut()) },
        SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { ms.get_macsec_sa_attribute.unwrap()(sa ^ 0x7777, 1, ptr::null_mut()) },
        SAI_STATUS_INVALID_OBJECT_ID
    );
    assert_eq!(
        unsafe { ms.create_macsec_flow.unwrap()(ptr::null_mut(), switch, 1, &raw const fa) },
        SAI_STATUS_INVALID_PARAMETER
    );
    // counters: flow and SA only in READ mode; port and SC have none
    let stat = SAI_MACSEC_FLOW_STAT_CONTROL_PKTS;
    let mut value = 0u64;
    assert_eq!(
        unsafe { ms.get_macsec_flow_stats.unwrap()(flow, 1, &raw const stat, &raw mut value) },
        SAI_STATUS_SUCCESS
    );
    assert_eq!(
        unsafe {
            ms.get_macsec_flow_stats_ext.unwrap()(
                flow,
                1,
                &raw const stat,
                SAI_STATS_MODE_READ_AND_CLEAR,
                &raw mut value,
            )
        },
        SAI_STATUS_NOT_SUPPORTED
    );
    assert_eq!(
        unsafe { ms.clear_macsec_port_stats.unwrap()(mport, 0, ptr::null()) },
        SAI_STATUS_NOT_SUPPORTED
    );
    // teardown in MACsecOrch's order
    assert_eq!(unsafe { ms.remove_macsec_sa.unwrap()(sa) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { ms.remove_macsec_sc.unwrap()(sc) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { ms.remove_macsec_flow.unwrap()(flow) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { ms.remove_macsec_port.unwrap()(mport) }, SAI_STATUS_SUCCESS);
    assert_eq!(
        unsafe { ms.remove_macsec_sa.unwrap()(sa) },
        SAI_STATUS_INVALID_OBJECT_ID
    );
    assert_eq!(unsafe { switch_api.remove_switch.unwrap()(switch) }, SAI_STATUS_SUCCESS);
    assert_eq!(unsafe { sai_api_uninitialize() }, SAI_STATUS_SUCCESS);
    std::fs::remove_file(path).unwrap();
}

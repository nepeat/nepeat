//! The exported functions through their C ABI, as the C++ calls them: SAI attributes in
//! unions, callbacks as C function pointers (implemented here), the list conventions, a
//! callback calling back in, key material that reaches the run callback and nowhere else.
//! One test function: the library has one global state.

#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    reason = "test code: it plays the C caller, whose side of the contract is unsafe by nature"
)]

use sai_macsec_capi::sys::{
    ob_rs_callbacks, ob_rs_chip_port, ob_rs_engine, ob_rs_param, ob_rs_put_fn, ob_rs_seq, ob_rs_switch_port,
};
use sai_macsec_capi::*;
use sai_sys as sai;
use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// (chip port, sequence, parameters by name)
type Run = (u32, String, Vec<(String, Vec<u8>)>);
static RUNS: Mutex<Vec<Run>> = Mutex::new(Vec::new());
static IFG: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(0);
static NEST: AtomicBool = AtomicBool::new(false);
static NESTED: Mutex<Option<i32>> = Mutex::new(None);

unsafe fn text(p: *const c_char, n: usize) -> String {
    String::from_utf8(unsafe { std::slice::from_raw_parts(p.cast::<u8>(), n) }.to_vec()).unwrap()
}

unsafe extern "C" fn log(_api: i32, _level: i32, t: *const c_char, n: usize) {
    LOG.lock().unwrap().push(unsafe { text(t, n) });
}

unsafe extern "C" fn run(
    hw: u32,
    name: *const c_char,
    name_len: usize,
    params: *const ob_rs_param,
    n: usize,
    put: ob_rs_put_fn,
    ctx: *mut c_void,
    _why: *mut c_char,
    _cap: usize,
) -> i32 {
    let name = unsafe { text(name, name_len) };
    let params = if n == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(params, n) }
    };
    let params = params
        .iter()
        .map(|p| {
            (
                unsafe { text(p.name, p.name_len) },
                unsafe { std::slice::from_raw_parts(p.bytes, p.len) }.to_vec(),
            )
        })
        .collect();
    RUNS.lock().unwrap().push((hw, name.clone(), params));
    if NEST.load(Ordering::Relaxed) {
        // a callback calling back in: refused, not a deadlock
        let mut id = 0;
        let st = unsafe { ob_rs_macsec_create(sai::SAI_OBJECT_TYPE_MACSEC_FLOW, &raw mut id, 0, std::ptr::null()) };
        *NESTED.lock().unwrap() = Some(st);
    }
    let put = put.unwrap();
    if name.starts_with("pn.") {
        unsafe { put(ctx, c"pn".as_ptr(), 2, 0, 0x2222) };
        unsafe { put(ctx, c"pn".as_ptr(), 2, 1, 0x1111) };
    }
    if name.starts_with("stats-sa.") {
        unsafe { put(ctx, c"out_pkts_encrypted".as_ptr(), 18, 0, 5) };
    }
    0
}

unsafe extern "C" fn set_ifg(hw: u32, bits: u32) -> i32 {
    IFG.lock().unwrap().push((hw, bits));
    0
}

unsafe extern "C" fn alloc(t: i32) -> u64 {
    (u64::from(t.cast_unsigned()) << 56) | (NEXT.fetch_add(1, Ordering::Relaxed) + 1)
}

unsafe extern "C" fn collect(ctx: *mut c_void, t: *const c_char, n: usize) {
    unsafe { (*ctx.cast::<String>()).push_str(&text(t, n)) };
}

fn attr(id: u32) -> sai::sai_attribute_t {
    let mut a: sai::sai_attribute_t = unsafe { std::mem::zeroed() };
    a.id = id;
    a
}
fn s32(id: u32, v: u32) -> sai::sai_attribute_t {
    let mut a = attr(id);
    a.value.s32 = v as i32;
    a
}
fn oid(id: u32, v: u64) -> sai::sai_attribute_t {
    let mut a = attr(id);
    a.value.oid = v;
    a
}

fn create(t: u32, attrs: &[sai::sai_attribute_t]) -> Result<u64, i32> {
    let mut id = 0;
    let st = unsafe { ob_rs_macsec_create(t, &raw mut id, attrs.len() as u32, attrs.as_ptr()) };
    if st == 0 { Ok(id) } else { Err(st) }
}

const EGR: u32 = sai::SAI_MACSEC_DIRECTION_EGRESS;
const PORT: u64 = (sai::SAI_OBJECT_TYPE_PORT as u64) << 56 | 1;

#[test]
fn the_c_abi_end_to_end() {
    assert_eq!(ob_rs_macsec_abi(), sai_macsec_capi::sys::OB_RS_MACSEC_ABI);
    // before any attach: a switch without a table
    assert_eq!(
        create(sai::SAI_OBJECT_TYPE_MACSEC, &[s32(sai::SAI_MACSEC_ATTR_DIRECTION, EGR)]),
        Err(sai::SAI_STATUS_NOT_SUPPORTED)
    );

    let cb = ob_rs_callbacks {
        log: Some(log),
        run: Some(run),
        set_ifg: Some(set_ifg),
        alloc: Some(alloc),
    };
    let engines = [ob_rs_engine { lane: 1, phy: 0x10 }];
    let names: Vec<(&str, &str)> = vec![
        ("enable.egress", ""),
        ("disable.egress", ""),
        ("gate-open.egress", ""),
        ("gate-close.egress", ""),
        ("pn.egress.0", "pn\npn\n"),
        ("sa-add.egress.0.gcm-aes-128", ""),
        ("sa-del.egress.0", ""),
        ("stats-sa.egress.0", "out_pkts_encrypted\n"),
    ];
    let seqs: Vec<ob_rs_seq> = names
        .iter()
        .map(|(n, r)| ob_rs_seq {
            name: n.as_ptr().cast(),
            name_len: n.len(),
            results: r.as_ptr().cast(),
            results_len: r.len(),
        })
        .collect();
    let ports = [ob_rs_chip_port {
        hw: 7,
        has_lane: 1,
        first_lane: 1,
        name: c"ce0".as_ptr(),
        name_len: 3,
    }];
    let st = unsafe {
        ob_rs_macsec_attach(
            &raw const cb,
            1,
            1,
            384,
            engines.as_ptr(),
            1,
            seqs.as_ptr(),
            seqs.len(),
            ports.as_ptr(),
            1,
        )
    };
    assert_eq!(st, 0);
    assert!(
        LOG.lock().unwrap().iter().any(|l| l.contains("1 ports, 8 sequences")),
        "{:?}",
        LOG.lock().unwrap()
    );
    let sw = [ob_rs_switch_port { oid: PORT, hw: 7 }];
    unsafe { ob_rs_macsec_ports(sw.as_ptr(), 1) };

    // pointer checks first, as the C++
    let mut scratch = 0u64;
    assert_eq!(
        unsafe { ob_rs_macsec_create(sai::SAI_OBJECT_TYPE_MACSEC, std::ptr::null_mut(), 0, std::ptr::null()) },
        sai::SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe { ob_rs_macsec_create(sai::SAI_OBJECT_TYPE_PORT, &raw mut scratch, 0, std::ptr::null()) },
        sai::SAI_STATUS_INVALID_OBJECT_TYPE
    );

    let m = create(sai::SAI_OBJECT_TYPE_MACSEC, &[s32(sai::SAI_MACSEC_ATTR_DIRECTION, EGR)]).unwrap();
    assert_eq!(m >> 56, u64::from(sai::SAI_OBJECT_TYPE_MACSEC)); // the caller's allocator
    let mp = create(
        sai::SAI_OBJECT_TYPE_MACSEC_PORT,
        &[
            s32(sai::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
            oid(sai::SAI_MACSEC_PORT_ATTR_PORT_ID, PORT),
        ],
    )
    .unwrap();
    assert_eq!(*IFG.lock().unwrap(), [(7, 384)]);
    assert_eq!(ob_rs_macsec_port_ifg(7), 384);
    let flow = create(
        sai::SAI_OBJECT_TYPE_MACSEC_FLOW,
        &[s32(sai::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)],
    )
    .unwrap();
    let table = create(
        sai::SAI_OBJECT_TYPE_ACL_TABLE,
        &[s32(sai::SAI_ACL_TABLE_ATTR_ACL_STAGE, sai::SAI_ACL_STAGE_EGRESS_MACSEC)],
    )
    .unwrap();
    assert_eq!(ob_rs_macsec_owns(sai::SAI_OBJECT_TYPE_ACL_TABLE, table), 1);
    assert_eq!(ob_rs_macsec_owns(sai::SAI_OBJECT_TYPE_ACL_ENTRY, table), 0);
    assert_eq!(ob_rs_macsec_port_acl_set(PORT, 0, table, 3), 0);
    assert_eq!(ob_rs_macsec_port_acl(PORT, 0), table);
    // a table of the other direction: invalid value at the caller's index
    assert_eq!(
        ob_rs_macsec_port_acl_set(PORT, 1, table, 3),
        sai::SAI_STATUS_INVALID_ATTR_VALUE_0 - 3
    );
    let mut fa = attr(sai::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW);
    fa.value.aclaction.enable = true;
    fa.value.aclaction.parameter.oid = flow;
    let mut et = attr(sai::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE);
    et.value.aclfield.enable = true;
    et.value.aclfield.data.u16_ = 0x888e;
    et.value.aclfield.mask.u16_ = 0xffff;
    let entry = create(
        sai::SAI_OBJECT_TYPE_ACL_ENTRY,
        &[oid(sai::SAI_ACL_ENTRY_ATTR_TABLE_ID, table), et, fa],
    )
    .unwrap();
    // the entry's fields and actions come back in their unions
    let mut back = [
        attr(sai::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE),
        attr(sai::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW),
        attr(sai::SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC),
    ];
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_ACL_ENTRY, entry, 3, back.as_mut_ptr()) },
        0
    );
    unsafe {
        assert!(
            back[0].value.aclfield.enable
                && back[0].value.aclfield.data.u16_ == 0x888e
                && back[0].value.aclfield.mask.u16_ == 0xffff
        );
        assert!(back[1].value.aclaction.enable && back[1].value.aclaction.parameter.oid == flow);
        assert!(!back[2].value.aclfield.enable);
    }
    // the SCI: the u64's bytes in memory order are the wire bytes
    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0, 1]);
    let mut sci_a = attr(sai::SAI_MACSEC_SC_ATTR_MACSEC_SCI);
    sci_a.value.u64_ = sci;
    let sc = create(
        sai::SAI_OBJECT_TYPE_MACSEC_SC,
        &[
            s32(sai::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, EGR),
            oid(sai::SAI_MACSEC_SC_ATTR_FLOW_ID, flow),
            sci_a,
        ],
    )
    .unwrap();
    // an SA with a bool that is neither 0 nor 1 nowhere near it, a SAK whose 128-bit key is its last 16 bytes
    let mut sak = attr(sai::SAI_MACSEC_SA_ATTR_SAK);
    sak.value.macsecsak = core::array::from_fn(|i| if i < 16 { 0 } else { 0xc0 + i as u8 - 16 });
    let mut hk = attr(sai::SAI_MACSEC_SA_ATTR_AUTH_KEY);
    hk.value.macsecauthkey = [0x80; 16];
    let mut an = attr(sai::SAI_MACSEC_SA_ATTR_AN);
    an.value.u8_ = 0;
    let mut pn = attr(sai::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN);
    pn.value.u64_ = 7;
    RUNS.lock().unwrap().clear();
    let sa = create(
        sai::SAI_OBJECT_TYPE_MACSEC_SA,
        &[
            s32(sai::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, EGR),
            oid(sai::SAI_MACSEC_SA_ATTR_SC_ID, sc),
            an,
            sak,
            hk,
            pn,
        ],
    )
    .unwrap();
    {
        let runs = RUNS.lock().unwrap();
        let (hw, name, params) = &runs[0];
        assert_eq!((*hw, name.as_str()), (7, "sa-add.egress.0.gcm-aes-128"));
        let get = |n: &str| params.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("key"), (0..16).map(|i| 0xc0 + i).collect::<Vec<u8>>());
        assert_eq!(get("hkey"), [0x80; 16]);
        assert_eq!(get("sci"), [0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0, 1]);
        assert_eq!(get("pn"), 7u64.to_le_bytes());
        assert_eq!(runs[1].1, "gate-open.egress");
    }
    // read back: the PN from the PHY (32 bits for a non-XPN suite), the key write-only
    let mut x = [attr(sai::SAI_MACSEC_SA_ATTR_CURRENT_XPN)];
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa, 1, x.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { x[0].value.u64_ }, 0x1111_2222);
    let mut k = [attr(sai::SAI_MACSEC_SA_ATTR_SAK)];
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa, 1, k.as_mut_ptr()) },
        sai::SAI_STATUS_ATTR_NOT_SUPPORTED_0
    );
    let mut sc_back = [
        attr(sai::SAI_MACSEC_SC_ATTR_MACSEC_SCI),
        attr(sai::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID),
    ];
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_SC, sc, 2, sc_back.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { (sc_back[0].value.u64_, sc_back[1].value.oid) }, (sci, sa));
    // counters
    let ids = [
        sai::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED,
        sai::SAI_MACSEC_SA_STAT_IN_PKTS_OK,
    ];
    let mut out = [9u64; 2];
    assert_eq!(
        unsafe { ob_rs_macsec_stats(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa, 2, ids.as_ptr(), out.as_mut_ptr()) },
        0
    );
    assert_eq!(out, [5, 0]);
    assert_eq!(
        unsafe {
            ob_rs_macsec_stats(
                sai::SAI_OBJECT_TYPE_MACSEC_SA,
                sa,
                2,
                std::ptr::null(),
                out.as_mut_ptr(),
            )
        },
        sai::SAI_STATUS_INVALID_PARAMETER
    );
    assert_eq!(
        unsafe {
            ob_rs_macsec_stats(
                sai::SAI_OBJECT_TYPE_MACSEC_SA,
                sa + 99,
                2,
                std::ptr::null(),
                out.as_mut_ptr(),
            )
        },
        sai::SAI_STATUS_INVALID_OBJECT_ID
    );
    assert_eq!(
        unsafe {
            ob_rs_macsec_stats(
                sai::SAI_OBJECT_TYPE_MACSEC_PORT,
                mp,
                2,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        },
        sai::SAI_STATUS_NOT_SUPPORTED
    );
    // lists: too small a buffer says how many
    let mut l = [0u64; 1];
    let mut lists = [attr(sai::SAI_MACSEC_FLOW_ATTR_SC_LIST)];
    lists[0].value.objlist = sai::sai_object_list_t {
        count: 0,
        list: l.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_FLOW, flow, 1, lists.as_mut_ptr()) },
        sai::SAI_STATUS_BUFFER_OVERFLOW
    );
    assert_eq!(unsafe { lists[0].value.objlist.count }, 1);
    lists[0].value.objlist.count = 1;
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_FLOW, flow, 1, lists.as_mut_ptr()) },
        0
    );
    assert_eq!(l, [sc]);
    let mut pl = sai::sai_object_list_t {
        count: 1,
        list: l.as_mut_ptr(),
    };
    assert_eq!(unsafe { ob_rs_macsec_port_list(PORT, &raw mut pl) }, 0);
    assert_eq!((pl.count, l), (1, [mp]));
    let mut caps = [sai::sai_stat_capability_t {
        stat_enum: 0,
        stat_modes: 0,
    }; 4];
    let mut cl = sai::sai_stat_capability_list_t {
        count: 4,
        list: caps.as_mut_ptr(),
    };
    assert_eq!(
        unsafe { ob_rs_macsec_stats_capability(sai::SAI_OBJECT_TYPE_MACSEC_SA, &raw mut cl) },
        0
    );
    assert_eq!(
        (cl.count, caps[0].stat_enum),
        (1, sai::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED)
    );
    // the status text and the log hold no key byte
    let mut status = String::new();
    unsafe { ob_rs_macsec_status(Some(collect), (&raw mut status).cast()) };
    assert!(status.contains("ce0") && status.contains("open"), "{status}");
    let all = LOG.lock().unwrap().join("\n") + &status;
    for needle in ["c0c1", "c1c0", "8080", "c0 c1", "0xc0", "192"] {
        assert!(!all.contains(needle), "{needle} in {all}");
    }
    // a callback that calls back in is refused (no deadlock), the outer call goes on
    NEST.store(true, Ordering::Relaxed);
    let mut x = [attr(sai::SAI_MACSEC_SA_ATTR_CURRENT_XPN)];
    assert_eq!(
        unsafe { ob_rs_macsec_get(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa, 1, x.as_mut_ptr()) },
        0
    );
    NEST.store(false, Ordering::Relaxed);
    assert_eq!(*NESTED.lock().unwrap(), Some(sai::SAI_STATUS_FAILURE));
    // removal, then detach: a switch without a table again
    assert_eq!(
        ob_rs_macsec_remove(sai::SAI_OBJECT_TYPE_MACSEC_SC, sc),
        sai::SAI_STATUS_OBJECT_IN_USE
    );
    assert_eq!(ob_rs_macsec_remove(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa), 0);
    assert_eq!(
        ob_rs_macsec_remove(sai::SAI_OBJECT_TYPE_MACSEC_SA, sa),
        sai::SAI_STATUS_INVALID_OBJECT_ID
    );
    assert_eq!(ob_rs_macsec_remove(sai::SAI_OBJECT_TYPE_MACSEC_PORT, mp), 0);
    assert_eq!(IFG.lock().unwrap().last(), Some(&(7, 0)));
    let _ = m;
    ob_rs_macsec_detach();
    assert_eq!(ob_rs_macsec_owns(sai::SAI_OBJECT_TYPE_ACL_TABLE, table), 0);
    assert_eq!(
        create(sai::SAI_OBJECT_TYPE_MACSEC, &[s32(sai::SAI_MACSEC_ATTR_DIRECTION, EGR)]),
        Err(sai::SAI_STATUS_NOT_SUPPORTED)
    );
}

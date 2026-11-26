//! Every byte the ifcs crate hands libifcs is defined, and a struct passed by value as its byte twin
//! arrives as the struct (`ifcs-sys` docs, "By-value structs"): through the crate's own call sites
//! (the `Ifcs` methods, their wrappers, the table's function pointers) into C callees with the REAL
//! signatures (`tests/c/abi_fixture.c`), which compare what they received with the image C makes of
//! the same fields (memset 0, then the fields) and record it; the records come back through
//! `ifcs_node_attr_get`. Run natively and, from the cross build's test bundle, on aarch64 (binfmt):
//! the fixture is found at `IFCS_ABI_FIXTURE` (run time) or where build.rs built it.
//!
//! Before each sequence the fixture fills 64 KiB of stack with 0xa5 (a stack that held other data, as
//! on the switch), so a byte the Rust side left undefined would not be zero by luck. The negative
//! controls pass twins with non-zero padding and must be seen.
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::too_many_lines)]

use core::mem::{offset_of, size_of};
use ifcs::{Attrs, GuardOptions, Handle, Ifcs, IpPrefix, L2Key, QueueKey, RouteKey, Status, sys};
use sys::CBytes;

const NODE: u32 = 0;
const DIRTY_STACK: u32 = 0xabc0;
const SUCCESS: Status = Status(sys::IFCS_SUCCESS.cast_signed());
const PARAM: Status = Status(sys::IFCS_PARAM.cast_signed());

fn fixture() -> Option<String> {
    let p = std::env::var("IFCS_ABI_FIXTURE")
        .ok()
        .or_else(|| option_env!("IFCS_ABI_FIXTURE").map(str::to_owned));
    if p.is_none() {
        eprintln!("ifcs: no C compiler at build time: the ABI fixture tests are SKIPPED");
    }
    p
}

fn dirty_stack(lib: &Ifcs) {
    let rc = lib.ifcs_node_attr_set(NODE, &mut Attrs::new().u32(DIRTY_STACK, 0));
    assert_eq!(rc, SUCCESS);
}

/// The bytes the fixture recorded in `slot` (its first `n`).
fn slot(lib: &Ifcs, slot: u32, n: usize) -> Vec<u8> {
    let mut a = Attrs::new().of(slot);
    let (rc, got) = lib.ifcs_node_attr_get(NODE, &mut a);
    assert_eq!((rc, got), (SUCCESS, 1));
    a.raw()[0].value()[..n].to_vec()
}

/// `l2_key` of sai-backend-teralynx, step for step.
fn l2_key(lib: &Ifcs, mac: [u8; 6], l2vni: Handle) -> (L2Key, Status) {
    let mut k = L2Key::zeroed();
    assert_eq!(lib.ifcs_l2_entry_key_t_init(&mut k), SUCCESS);
    let t = sys::IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI.cast_signed();
    assert_eq!(lib.ifcs_l2_entry_key_t_key_type_set(&mut k, t), SUCCESS);
    let member = k.mac_l2vni();
    let rc = lib.ifcs_l2_entry_key_t_mac_l2vni_set(&mut k, member);
    assert_eq!(lib.ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&mut k, mac), SUCCESS);
    assert_eq!(lib.ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&mut k, l2vni), SUCCESS);
    (k, rc)
}

/// `L3Area::route_key` of sai-backend-teralynx, step for step (IPv4 or IPv6 by `v6`).
fn route_key(lib: &Ifcs, vni: Handle, v6: bool, addr: [u8; 16], mask: [u8; 16]) -> (RouteKey, Status) {
    let family = if v6 {
        sys::IFCS_IP_ADDR_FAMILY_IPV6
    } else {
        sys::IFCS_IP_ADDR_FAMILY_IPV4
    };
    let mut pf = IpPrefix::zeroed();
    assert_eq!(lib.ifcs_ip_prefix_t_init(&mut pf), SUCCESS);
    assert_eq!(
        lib.ifcs_ip_prefix_t_addr_family_set(&mut pf, family.cast_signed()),
        SUCCESS
    );
    assert_eq!(lib.ifcs_ip_prefix_t_addr_set(&mut pf, addr), SUCCESS);
    assert_eq!(lib.ifcs_ip_prefix_t_mask_set(&mut pf, mask), SUCCESS);
    let mut k = RouteKey::zeroed();
    assert_eq!(lib.ifcs_route_entry_key_t_init(&mut k), SUCCESS);
    let t = sys::IFCS_ROUTE_ENTRY_KEY_TYPE_IP_DEST_L3VNI.cast_signed();
    assert_eq!(lib.ifcs_route_entry_key_t_key_type_set(&mut k, t), SUCCESS);
    let member = k.ip_dest_l3vni();
    let rc = lib.ifcs_route_entry_key_t_ip_dest_l3vni_set(&mut k, member);
    assert_eq!(lib.ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&mut k, vni), SUCCESS);
    assert_eq!(
        lib.ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&mut k, &mut pf),
        SUCCESS
    );
    (k, rc)
}

fn pattern<T: CBytes>(seed: u8) -> T {
    let mut t = T::ZERO;
    for (i, b) in t.as_bytes_mut().iter_mut().enumerate() {
        *b = seed.wrapping_add(u8::try_from(i % 251).unwrap());
    }
    t
}

/// One test: the fixture's records are process-wide.
#[test]
fn every_byte_reaches_c_as_the_struct() {
    let Some(path) = fixture() else { return };
    let off = GuardOptions {
        enable: false,
        ..GuardOptions::default()
    };
    let lib = ifcs::load(&path, "", &off).unwrap();

    // 1. ABI: a by-value twin arrives byte for byte, every position (16 B: registers; 48 B: memory
    //    on SysV / a pointer to a copy on AAPCS64), for every by-value parameter the header has.
    let names: Vec<&str> = sys::BY_VALUE_ARGS.iter().map(|(f, _, _)| *f).collect();
    assert_eq!(
        names,
        [
            "ifcs_l2_entry_key_t_mac_l2vni_set",
            "ifcs_route_entry_key_t_ip_dest_l3vni_set"
        ],
        "a by-value parameter this test does not cover"
    );
    for seed in [1u8, 0x80, 0xfe] {
        dirty_stack(&lib);
        let v: sys::ifcs_l2_entry_key_mac_l2vni_t_bytes = pattern(seed);
        let mut k = L2Key::zeroed();
        assert_eq!(lib.ifcs_l2_entry_key_t_mac_l2vni_set(&mut k, v), PARAM); // its padding is not zero
        assert_eq!(slot(&lib, 1, 16), v.as_bytes(), "L2 member, seed {seed}");
        // libifcs copies it into the key: the key's union starts with it
        let u = offset_of!(sys::ifcs_l2_entry_key_t, key);
        assert_eq!(&k.bytes()[u..u + 16], v.as_bytes());

        dirty_stack(&lib);
        let v: sys::ifcs_route_entry_key_ip_dest_l3vni_t_bytes = pattern(seed);
        let mut k = RouteKey::zeroed();
        assert_eq!(lib.ifcs_route_entry_key_t_ip_dest_l3vni_set(&mut k, v), PARAM);
        assert_eq!(slot(&lib, 2, 48), v.as_bytes(), "route member, seed {seed}");
        let u = offset_of!(sys::ifcs_route_entry_key_t, key);
        assert_eq!(&k.bytes()[u..u + 48], v.as_bytes());
    }
    assert_eq!(size_of::<sys::ifcs_l2_entry_key_mac_l2vni_t_bytes>(), 16);
    assert_eq!(size_of::<sys::ifcs_route_entry_key_ip_dest_l3vni_t_bytes>(), 48);

    // 2. The keys as the backend builds them: every byte the C image's (padding and union tail zero).
    for (mac, vni) in [([2, 0x11, 0x22, 0x33, 0x44, 0x55], 100), ([0xff; 6], 4094)] {
        dirty_stack(&lib);
        let (mut k, rc) = l2_key(&lib, mac, Handle::l2vni(vni));
        assert_eq!(rc, SUCCESS, "the L2 member reached C with non-zero padding");
        let member = slot(&lib, 1, 16);
        assert_eq!(member[6..8], [0; 2]);
        assert_eq!(member[13..16], [0; 3]);
        dirty_stack(&lib);
        assert_eq!(
            lib.ifcs_l2_entry_create(NODE, &mut k, &mut Attrs::new().u32(1, 7)),
            SUCCESS
        );
        assert_eq!(slot(&lib, 3, 100), k.bytes());
        assert_eq!(slot(&lib, 7, 8)[4..8], [0; 4], "attribute padding");
        // a copy (a walk's key) deletes with the same bytes
        let mut again = k;
        dirty_stack(&lib);
        assert_eq!(lib.ifcs_l2_entry_delete(NODE, &mut again), SUCCESS);
        assert_eq!(slot(&lib, 3, 100), k.bytes());
    }
    let v4 = |a: [u8; 4]| {
        let mut b = [0u8; 16];
        b[..4].copy_from_slice(&u32::from_be_bytes(a).to_ne_bytes());
        b
    };
    for (v6, addr, mask) in [
        (false, v4([10, 99, 0, 1]), v4([255; 4])),
        (
            true,
            [0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            [0xff; 16],
        ),
        (false, [0; 16], [0; 16]),
    ] {
        dirty_stack(&lib);
        let (mut k, rc) = route_key(&lib, Handle::l3vni(1), v6, addr, mask);
        assert_eq!(rc, SUCCESS, "the route member reached C with non-zero padding");
        assert_eq!(slot(&lib, 2, 48)[45..48], [0; 3]);
        assert_eq!(slot(&lib, 5, 40)[37..40], [0; 3], "prefix padding");
        dirty_stack(&lib);
        let mut attrs = Attrs::new().handle(1, Handle(0x1820_0041)).fwd(2, 0);
        assert_eq!(lib.ifcs_route_entry_create(NODE, &mut k, &mut attrs), SUCCESS);
        assert_eq!(slot(&lib, 4, 60), k.bytes());
        assert_eq!(
            slot(&lib, 7, 16),
            [[1u8, 0, 0, 0, 0, 0, 0, 0], [2, 0, 0, 0, 0, 0, 0, 0]].concat()
        );
        dirty_stack(&lib);
        let (mut again, _) = route_key(&lib, Handle::l3vni(1), v6, addr, mask);
        assert_eq!(again.bytes(), k.bytes(), "the same route builds the same bytes");
        assert_eq!(lib.ifcs_route_entry_delete(NODE, &mut again), SUCCESS);
    }

    // 3. The queue key and an attribute list (padding after each id).
    dirty_stack(&lib);
    let mut q = QueueKey::zeroed();
    assert_eq!(lib.ifcs_queue_t_init(&mut q), SUCCESS);
    assert_eq!(lib.ifcs_queue_t_devport_set(&mut q, 3), SUCCESS);
    assert_eq!(lib.ifcs_queue_t_queue_id_set(&mut q, 7), SUCCESS);
    let mut attrs = Attrs::new()
        .u32(5, 1)
        .u64(6, u64::MAX)
        .mac(7, [1; 6])
        .u32_list(8, vec![1, 2]);
    assert_eq!(lib.ifcs_queue_attr_set(NODE, &mut q, &mut attrs), SUCCESS);
    assert_eq!(slot(&lib, 6, 16), q.bytes());
    assert_eq!(slot(&lib, 6, 16)[13..], [0; 3]);
    assert!(slot(&lib, 7, 32).chunks(8).all(|a| a[4..8] == [0; 4]));

    // 4. Negative controls: what the bug was (non-zero padding in a by-value member) is seen, and it
    //    reaches the key and its create, as on the switch.
    dirty_stack(&lib);
    let mut k = L2Key::zeroed();
    let mut v = k.mac_l2vni();
    v.0[13] = 0xff; // padding after dirty
    assert_eq!(lib.ifcs_l2_entry_key_t_mac_l2vni_set(&mut k, v), PARAM);
    assert_eq!(slot(&lib, 1, 16)[13], 0xff);
    assert_eq!(lib.ifcs_l2_entry_create(NODE, &mut k, &mut Attrs::null()), PARAM);
    let mut k = RouteKey::zeroed();
    let mut v = k.ip_dest_l3vni();
    v.0[45] = 0xed; // padding after the member's dirty
    assert_eq!(lib.ifcs_route_entry_key_t_ip_dest_l3vni_set(&mut k, v), PARAM);
    assert_eq!(slot(&lib, 2, 48)[45], 0xed);
    assert_eq!(lib.ifcs_route_entry_create(NODE, &mut k, &mut Attrs::null()), PARAM);
    let mut v = RouteKey::zeroed().ip_dest_l3vni();
    v.0[38] = 0xbd; // padding of the prefix inside it
    assert_eq!(
        lib.ifcs_route_entry_key_t_ip_dest_l3vni_set(&mut RouteKey::zeroed(), v),
        PARAM
    );
}

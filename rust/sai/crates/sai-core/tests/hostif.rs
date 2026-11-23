//! Host interfaces, trap groups, traps and host interface table entries on the virtual backend:
//! the checks of the C++ `test_hostif` (`tests/test_sai.cpp`), the hostif parts of `test_lag`
//! and of the genetlink / `SAMPLEPACKET` section of `tests/test_qos.inc`, failure injection (a
//! failed backend call leaves model and chip as they were, except where the C++ ignores the
//! answer), and the backend call log of `tests/driver-scripts/05-hostif-traps.ops` line for line.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod common;

use common::*;
use sai_core::Error;
use sai_core::backend::types::{GenlId, NetifId, TrapKind};
use sai_core::backend::{Mode, Profile, Recording};
use sai_core::error::AttrError;
use sai_core::hostif::{hostif_backend_id, trap_kind, trap_programmed};
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::{KnetNetif, VirtualBackend};
use sai_sys as sys;
use std::fmt::Write as _;

const TRAP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_TRAP;
const GROUP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP;
const ENTRY: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY;
const USER_TRAP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_USER_DEFINED_TRAP;

fn netdev(name: &str, port: RawOid) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_NETDEV)),
        Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(port)),
        Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(name.into())),
    ]
}

fn genetlink(name: &str, mcgrp: &str) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_GENETLINK)),
        Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(name.into())),
        Attr::new(sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME, Value::CharData(mcgrp.into())),
    ]
}

fn trap(trap_type: u32, action: u32, group: Option<RawOid>) -> Vec<Attr> {
    let mut attrs = vec![
        Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE, enumerator(trap_type)),
        Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, enumerator(action)),
    ];
    if let Some(group) = group {
        attrs.push(Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(group)));
    }
    attrs
}

fn queue(q: u32) -> Attr {
    Attr::new(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, Value::U32(q))
}

fn action(a: u32) -> Attr {
    Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, enumerator(a))
}

fn oper(up: bool) -> Attr {
    Attr::new(sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(up))
}

fn vlan_tag(tag: u32) -> Attr {
    Attr::new(sys::SAI_HOSTIF_ATTR_VLAN_TAG, enumerator(tag))
}

fn wildcard_entry() -> Vec<Attr> {
    vec![
        Attr::new(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE,
            enumerator(sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD),
        ),
        Attr::new(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE,
            enumerator(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT),
        ),
    ]
}

fn ports_of(sai: &Sai<VirtualBackend>, switch: RawOid) -> Vec<RawOid> {
    oid_list(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap())
}

fn netif_of(sai: &Sai<VirtualBackend>, hostif: RawOid) -> Option<&KnetNetif> {
    let id = hostif_backend_id(sai.store()?, hostif)?;
    sai.backend().knet_netifs().get(&NetifId(id))
}

/// The C++ test port's `trap_to_cpu`: the chip steers the trap type's kind to the CPU.
fn trap_to_cpu(sai: &Sai<VirtualBackend>, trap_type: u32) -> bool {
    sai.backend().traps().get(&trap_kind(trap_type.cast_signed())) == Some(&true)
}

fn default_group(sai: &Sai<VirtualBackend>, switch: RawOid) -> RawOid {
    oid(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP).unwrap())
}

/// Everything this area keeps, in the model and on the chip, as text: equal before and after a
/// call that failed.
fn snapshot(sai: &Sai<VirtualBackend>) -> String {
    let store = sai.store().unwrap();
    let mut s = String::new();
    for (id, h) in store.iter::<sai_core::hostif::HostInterface>() {
        let _ = writeln!(s, "{id:?} {h:?}");
    }
    for (id, g) in store.iter::<sai_core::hostif::TrapGroup>() {
        let _ = writeln!(s, "{id:?} {g:?}");
    }
    for (id, t) in store.iter::<sai_core::hostif::Trap>() {
        let _ = writeln!(s, "{id:?} {t:?}");
    }
    for (id, e) in store.iter::<sai_core::hostif::TableEntry>() {
        let _ = writeln!(s, "{id:?} {e:?}");
    }
    let chip = sai.backend();
    let _ = write!(
        s,
        "{:?}\n{:?}\n{:?}\n",
        chip.knet_netifs(),
        chip.traps(),
        chip.mirror().genls
    );
    s
}

#[test]
fn host_interfaces_traps_and_table_entries_as_the_cpp_test_hostif() {
    let (mut sai, switch) = switch(4);
    let ports = ports_of(&sai, switch);
    let mac = [0x02, 0, 0, 0, 0, 0x42];
    sai.set(
        SWITCH,
        switch,
        &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)),
    )
    .unwrap();

    let h = sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])).unwrap();
    assert_eq!(sai.backend().knet_netifs().len(), 1);
    let n = netif_of(&sai, h).unwrap();
    assert_eq!(n.name, "Ethernet0");
    assert_eq!(n.hw.0, 1);
    assert!(!n.keep_tag && !n.carrier);
    assert_eq!(n.mac.0, mac, "the switch's MAC");
    assert_eq!(n.vid.get(), 1, "the port's PVID");
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])),
        Err(Error::ItemAlreadyExists)
    );
    let cpu = oid(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("cpu", cpu)),
        Err(Error::NotSupported)
    );
    // GENETLINK without the mirror model (C++: SAI_STATUS_IS_ATTR_NOT_SUPPORTED)
    let mut genl_on_cpu = netdev("cpu", cpu);
    genl_on_cpu[0] = Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_GENETLINK));
    assert_eq!(
        sai.create(HOSTIF, switch, &genl_on_cpu),
        Err(Error::attr(AttrError::NotSupported, 0))
    );

    sai.set(HOSTIF, h, &oper(true)).unwrap();
    assert!(netif_of(&sai, h).unwrap().carrier);
    let old_filter = netif_of(&sai, h).unwrap().filter;
    sai.set(HOSTIF, h, &vlan_tag(sys::SAI_HOSTIF_VLAN_TAG_KEEP)).unwrap();
    assert!(netif_of(&sai, h).unwrap().keep_tag);
    assert_ne!(netif_of(&sai, h).unwrap().filter, old_filter);
    assert_eq!(
        get1(&sai, HOSTIF, h, sys::SAI_HOSTIF_ATTR_NAME),
        Ok(Value::CharData("Ethernet0".into()))
    );
    assert_eq!(
        sai.get(
            HOSTIF,
            h,
            &[
                sys::SAI_HOSTIF_ATTR_TYPE,
                sys::SAI_HOSTIF_ATTR_OBJ_ID,
                sys::SAI_HOSTIF_ATTR_OPER_STATUS,
                sys::SAI_HOSTIF_ATTR_VLAN_TAG,
                sys::SAI_HOSTIF_ATTR_QUEUE,
                sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME,
            ]
        ),
        Ok(vec![
            enumerator(sys::SAI_HOSTIF_TYPE_NETDEV),
            Value::Oid(ports[0]),
            Value::Bool(true),
            enumerator(sys::SAI_HOSTIF_VLAN_TAG_KEEP),
            Value::U32(0),
            Value::CharData(String::new()),
        ])
    );

    // trap group + traps
    let grp = sai.create(GROUP, switch, &[queue(4)]).unwrap();
    let t1 = sai
        .create(
            TRAP,
            switch,
            &trap(
                sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST,
                sys::SAI_PACKET_ACTION_COPY,
                Some(grp),
            ),
        )
        .unwrap();
    assert!(trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST));
    assert!(trap_programmed(sai.store().unwrap(), t1));
    assert_eq!(
        sai.create(
            TRAP,
            switch,
            &trap(
                sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST,
                sys::SAI_PACKET_ACTION_COPY,
                Some(grp)
            ),
        ),
        Err(Error::ItemAlreadyExists)
    );
    // no SDK control: kept, not programmed
    let t2 = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_BGP, sys::SAI_PACKET_ACTION_COPY, Some(grp)),
        )
        .unwrap();
    assert!(!trap_programmed(sai.store().unwrap(), t2));
    assert_eq!(sai.remove(GROUP, grp), Err(Error::ObjectInUse));
    sai.set(TRAP, t1, &action(sys::SAI_PACKET_ACTION_DROP)).unwrap();
    assert!(!trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST));
    assert_eq!(
        sai.get(
            TRAP,
            t1,
            &[
                sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE,
                sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
                sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY,
                sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP,
            ]
        ),
        Ok(vec![
            enumerator(sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST),
            enumerator(sys::SAI_PACKET_ACTION_DROP),
            Value::U32(0),
            Value::Oid(grp),
        ])
    );
    sai.remove(TRAP, t1).unwrap();
    sai.remove(TRAP, t2).unwrap();
    sai.remove(GROUP, grp).unwrap();
    assert_eq!(sai.remove(GROUP, default_group(&sai, switch)), Err(Error::ObjectInUse));

    // the wildcard table entry orchagent creates at init
    let ent = sai.create(ENTRY, switch, &wildcard_entry()).unwrap();
    assert_eq!(
        get1(&sai, ENTRY, ent, sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE),
        Ok(enumerator(
            sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT
        ))
    );
    assert_eq!(
        get1(&sai, ENTRY, ent, sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF),
        Ok(Value::Oid(RawOid::NULL))
    );
    sai.remove(ENTRY, ent).unwrap();

    sai.remove(HOSTIF, h).unwrap();
    assert!(sai.backend().knet_netifs().is_empty());
    assert_eq!(sai.remove(HOSTIF, h), Err(Error::InvalidObjectId));
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn trap_groups_default_and_trap_group_moves() {
    let (mut sai, switch) = switch(2);
    let def = default_group(&sai, switch);
    assert_eq!(def.sai_object_type(), GROUP);
    // the C++ id of the default group: make_oid(HOSTIF_TRAP_GROUP, 1)
    assert_eq!(def, RawOid(u64::from(GROUP) << 56 | 1));
    assert_eq!(
        sai.get(
            GROUP,
            def,
            &[
                sys::SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE,
                sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE,
                sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER,
            ]
        ),
        Ok(vec![Value::Bool(true), Value::U32(0), Value::Oid(RawOid::NULL)])
    );
    // a trap without a group is in the default one
    let lldp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    assert_eq!(
        get1(&sai, TRAP, lldp, sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP),
        Ok(Value::Oid(def))
    );
    let grp = sai.create(GROUP, switch, &[queue(2)]).unwrap();
    let to = |g| Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(g));
    sai.set(TRAP, lldp, &to(grp)).unwrap();
    assert_eq!(sai.remove(GROUP, grp), Err(Error::ObjectInUse));
    // null (no @allownull in the headers) is the default group, as the C++ takes it
    // (sai_hostif.cpp:396)
    sai.set(TRAP, lldp, &to(RawOid::NULL)).unwrap();
    assert_eq!(
        get1(&sai, TRAP, lldp, sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP),
        Ok(Value::Oid(def))
    );
    sai.remove(GROUP, grp).unwrap();
    // a group that is not there: INVALID_ATTR_VALUE at the attribute's index
    assert_eq!(
        sai.set(TRAP, lldp, &to(grp)),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    let mut bad = trap(sys::SAI_HOSTIF_TRAP_TYPE_LACP, sys::SAI_PACKET_ACTION_TRAP, Some(grp));
    assert_eq!(
        sai.create(TRAP, switch, &bad),
        Err(Error::attr(AttrError::InvalidValue, 2))
    );
    bad.truncate(1);
    assert_eq!(sai.create(TRAP, switch, &bad), Err(Error::MandatoryAttributeMissing));
    // a policer that is not there
    let policer = RawOid(u64::from(sys::SAI_OBJECT_TYPE_POLICER) << 56 | 0x3e7);
    assert_eq!(
        sai.set(
            GROUP,
            def,
            &Attr::new(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, Value::Oid(policer))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    // priority and admin state are kept
    sai.set(
        TRAP,
        lldp,
        &Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY, Value::U32(7)),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, TRAP, lldp, sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY),
        Ok(Value::U32(7))
    );
    sai.set(
        GROUP,
        def,
        &Attr::new(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE, Value::Bool(false)),
    )
    .unwrap();
    assert_eq!(
        get1(&sai, GROUP, def, sys::SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE),
        Ok(Value::Bool(false))
    );
    sai.remove(TRAP, lldp).unwrap();
    assert!(!trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_LLDP));
    // user-defined traps: not supported (C++ create_user_trap)
    assert_eq!(sai.create(USER_TRAP, switch, &[]), Err(Error::NotSupported));
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn netdev_rules_lag_and_lacp_as_the_cpp_test_lag() {
    let (mut sai, switch) = switch(4);
    let ports = ports_of(&sai, switch);
    let lag = sai.create(sys::SAI_OBJECT_TYPE_LAG, switch, &[]).unwrap();
    // teamd makes the PortChannel's netdev
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("PortChannel1", lag)),
        Err(Error::NotSupported)
    );
    // name rules: 1..=15 bytes; NAME / OBJ_ID missing
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("", ports[0])),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("Ethernet01234567", ports[0])),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])[..2]),
        Err(Error::MandatoryAttributeMissing)
    );
    let mut no_port = netdev("Ethernet0", ports[0]);
    no_port.remove(1);
    assert_eq!(
        sai.create(HOSTIF, switch, &no_port),
        Err(Error::MandatoryAttributeMissing)
    );
    // a type the library does not have, at its index
    let mut fd = netdev("Ethernet0", ports[0]);
    fd.swap(0, 2);
    fd[2] = Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_FD));
    assert_eq!(
        sai.create(HOSTIF, switch, &fd),
        Err(Error::attr(AttrError::NotSupported, 2))
    );
    let h = sai.create(HOSTIF, switch, &netdev("Ethernet16", ports[3])).unwrap();
    // one netdev per port and per name
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("other", ports[3])),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("Ethernet16", ports[2])),
        Err(Error::ItemAlreadyExists)
    );
    assert!(
        sai.backend()
            .knet_netifs()
            .values()
            .any(|n| n.name == "Ethernet16" && n.hw.0 == 4)
    );
    let lt = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LACP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    assert!(trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_LACP));
    assert!(trap_programmed(sai.store().unwrap(), lt));
    sai.remove(TRAP, lt).unwrap();
    assert!(!trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_LACP));
    sai.remove(HOSTIF, h).unwrap();
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn genetlink_host_interfaces_and_their_trap_entries_as_the_cpp_test_qos() {
    let (mut sai, switch) = switch(4);
    // without the mirror model: the type is not supported (test_qos.inc:262)
    assert_eq!(
        sai.create(HOSTIF, switch, &genetlink("psample", "packets")),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    sai.backend_mut().toggles_mut().mirror_on = true;
    assert_eq!(
        sai.create(HOSTIF, switch, &genetlink("psample", "")),
        Err(Error::MandatoryAttributeMissing)
    );
    let gh = sai.create(HOSTIF, switch, &genetlink("psample", "packets")).unwrap();
    assert_eq!(
        sai.create(HOSTIF, switch, &genetlink("psample", "other")),
        Err(Error::ItemAlreadyExists)
    );
    let gid = GenlId(hostif_backend_id(sai.store().unwrap(), gh).unwrap());
    let genl = &sai.backend().mirror().genls[&gid];
    assert!(genl.name == "psample" && genl.mcgrp == "packets" && genl.traps.is_empty());
    assert_eq!(
        get1(&sai, HOSTIF, gh, sys::SAI_HOSTIF_ATTR_TYPE),
        Ok(enumerator(sys::SAI_HOSTIF_TYPE_GENETLINK))
    );
    assert_eq!(
        get1(&sai, HOSTIF, gh, sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME),
        Ok(Value::CharData("packets".into()))
    );
    // OPER_STATUS of a genetlink host interface is kept; VLAN_TAG has no netif: FAILURE
    sai.set(HOSTIF, gh, &oper(true)).unwrap();
    assert_eq!(
        get1(&sai, HOSTIF, gh, sys::SAI_HOSTIF_ATTR_OPER_STATUS),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        sai.set(HOSTIF, gh, &vlan_tag(sys::SAI_HOSTIF_VLAN_TAG_KEEP)),
        Err(Error::Failure)
    );

    let tg = default_group(&sai, switch);
    let samplepacket = sai
        .create(
            TRAP,
            switch,
            &trap(
                sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET,
                sys::SAI_PACKET_ACTION_TRAP,
                Some(tg),
            ),
        )
        .unwrap();
    let entry = |trap_id: RawOid, hostif: RawOid, entry_type: u32| {
        vec![
            Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE, enumerator(entry_type)),
            Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID, Value::Oid(trap_id)),
            Attr::new(
                sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE,
                enumerator(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK),
            ),
            Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF, Value::Oid(hostif)),
        ]
    };
    let trap_id = sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID;
    // a GENETLINK entry needs type TRAP_ID, a genetlink host interface and a trap
    assert_eq!(
        sai.create(
            ENTRY,
            switch,
            &entry(samplepacket, gh, sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD)
        ),
        Err(Error::InvalidParameter)
    );
    // a trap that is not there (a null TRAP_ID is refused by the metadata: INVALID_ATTR_VALUE_1;
    // the C++ says INVALID_PARAMETER)
    assert_eq!(
        sai.create(ENTRY, switch, &entry(RawOid::NULL, gh, trap_id)),
        Err(Error::attr(AttrError::InvalidValue, 1))
    );
    let gone = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    sai.remove(TRAP, gone).unwrap();
    assert_eq!(
        sai.create(ENTRY, switch, &entry(gone, gh, trap_id)),
        Err(Error::InvalidParameter)
    );
    let ports = ports_of(&sai, switch);
    let nd = sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])).unwrap();
    assert_eq!(
        sai.create(ENTRY, switch, &entry(samplepacket, nd, trap_id)),
        Err(Error::InvalidParameter)
    );
    let ten = sai.create(ENTRY, switch, &entry(samplepacket, gh, trap_id)).unwrap();
    assert!(
        sai.backend().mirror().genls[&gid]
            .traps
            .contains(&sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET.cast_signed())
    );
    assert_eq!(sai.remove(HOSTIF, gh), Err(Error::ObjectInUse));
    // every attribute of an entry is create-only
    assert_eq!(
        sai.set(
            ENTRY,
            ten,
            &Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF, Value::Oid(gh))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0)) // C++ set_hostif_table_entry_attribute
    );
    sai.remove(ENTRY, ten).unwrap();
    assert!(sai.backend().mirror().genls[&gid].traps.is_empty());
    sai.remove(HOSTIF, gh).unwrap();
    assert!(!sai.backend().mirror().genls.contains_key(&gid));
    sai.remove(TRAP, samplepacket).unwrap();
    sai.remove(HOSTIF, nd).unwrap();
    model_and_chip_agree(&sai).unwrap();
}

/// Calls the area makes, each failed once: the call fails with the backend's status and model
/// and chip are as before (the C++ statuses; `ITEM_NOT_FOUND` on removal counts as removed).
#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let (mut sai, switch) = switch(4);
    sai.backend_mut().toggles_mut().mirror_on = true;
    let ports = ports_of(&sai, switch);
    let grp = sai.create(GROUP, switch, &[queue(1)]).unwrap();

    let fail = |sai: &mut Sai<VirtualBackend>, op: &str| {
        sai.backend_mut().fail_nth(op, 1, Error::InsufficientResources).unwrap();
    };
    let refused = Err(Error::InsufficientResources);

    // creates
    let before = snapshot(&sai);
    let count = sai.store().unwrap().len();
    fail(&mut sai, "netif_create");
    assert_eq!(
        sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])).map(drop),
        refused
    );
    fail(&mut sai, "genetlink_create");
    assert_eq!(
        sai.create(HOSTIF, switch, &genetlink("psample", "packets")).map(drop),
        refused
    );
    fail(&mut sai, "trap_set");
    assert_eq!(
        sai.create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, Some(grp))
        )
        .map(drop),
        refused
    );
    // trap_program's NOT_SUPPORTED is the trap_set path; any other answer is final
    fail(&mut sai, "trap_program");
    assert_eq!(
        sai.create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, Some(grp))
        )
        .map(drop),
        refused
    );
    assert_eq!(snapshot(&sai), before);
    assert_eq!(sai.store().unwrap().len(), count);

    // then the objects, and their sets and removes
    let h = sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])).unwrap();
    let gh = sai.create(HOSTIF, switch, &genetlink("psample", "packets")).unwrap();
    let lldp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, Some(grp)),
        )
        .unwrap();
    let sample = sai
        .create(
            TRAP,
            switch,
            &trap(
                sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET,
                sys::SAI_PACKET_ACTION_TRAP,
                None,
            ),
        )
        .unwrap();
    let entry = [
        Attr::new(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE,
            enumerator(sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID),
        ),
        Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID, Value::Oid(sample)),
        Attr::new(
            sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE,
            enumerator(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK),
        ),
        Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF, Value::Oid(gh)),
    ];
    let before = snapshot(&sai);
    fail(&mut sai, "genetlink_trap_set");
    assert_eq!(sai.create(ENTRY, switch, &entry).map(drop), refused);
    assert_eq!(snapshot(&sai), before);
    let ten = sai.create(ENTRY, switch, &entry).unwrap();

    let before = snapshot(&sai);
    fail(&mut sai, "netif_carrier_set");
    assert_eq!(sai.set(HOSTIF, h, &oper(true)), refused);
    fail(&mut sai, "netif_set_keep_tag");
    assert_eq!(sai.set(HOSTIF, h, &vlan_tag(sys::SAI_HOSTIF_VLAN_TAG_KEEP)), refused);
    fail(&mut sai, "trap_set");
    assert_eq!(sai.set(TRAP, lldp, &action(sys::SAI_PACKET_ACTION_DROP)), refused);
    fail(&mut sai, "trap_program");
    assert_eq!(sai.set(TRAP, lldp, &action(sys::SAI_PACKET_ACTION_DROP)), refused);
    fail(&mut sai, "genetlink_trap_set");
    assert_eq!(sai.remove(ENTRY, ten), refused);
    fail(&mut sai, "netif_destroy");
    assert_eq!(sai.remove(HOSTIF, h), refused);
    assert_eq!(snapshot(&sai), before);
    assert_eq!(
        get1(&sai, HOSTIF, h, sys::SAI_HOSTIF_ATTR_OPER_STATUS),
        Ok(Value::Bool(false))
    );
    assert_eq!(
        get1(&sai, TRAP, lldp, sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION),
        Ok(enumerator(sys::SAI_PACKET_ACTION_TRAP))
    );
    sai.remove(ENTRY, ten).unwrap();
    let before = snapshot(&sai);
    fail(&mut sai, "genetlink_destroy");
    assert_eq!(sai.remove(HOSTIF, gh), refused);
    assert_eq!(snapshot(&sai), before);

    // ITEM_NOT_FOUND from the chip: the netif is gone already, the host interface goes too
    sai.backend_mut()
        .fail_nth("netif_destroy", 1, Error::ItemNotFound)
        .unwrap();
    sai.remove(HOSTIF, h).unwrap();
    assert_eq!(sai.remove(HOSTIF, h), Err(Error::InvalidObjectId));
    model_and_chip_agree(&sai).unwrap();
}

/// Where the C++ does not care about the backend's answer, neither does the port (both are
/// findings, kept): the carrier call at create, and everything at trap removal.
#[test]
fn where_the_cpp_ignores_the_backend_so_does_the_port() {
    let (mut sai, switch) = switch(2);
    let ports = ports_of(&sai, switch);
    sai.backend_mut()
        .fail_nth("netif_carrier_set", 1, Error::Failure)
        .unwrap();
    let mut attrs = netdev("Ethernet0", ports[0]);
    attrs.push(oper(true));
    let h = sai.create(HOSTIF, switch, &attrs).unwrap();
    // the model says up, the netdev's carrier is still off
    assert_eq!(
        get1(&sai, HOSTIF, h, sys::SAI_HOSTIF_ATTR_OPER_STATUS),
        Ok(Value::Bool(true))
    );
    assert!(!netif_of(&sai, h).unwrap().carrier);

    let lacp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LACP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    sai.backend_mut().fail_nth("trap_set", 1, Error::Failure).unwrap();
    sai.remove(TRAP, lacp).unwrap();
    // the trap is gone, the chip still sends LACP to the CPU
    assert!(trap_to_cpu(&sai, sys::SAI_HOSTIF_TRAP_TYPE_LACP));
    // a set of PACKET_ACTION whose trap_set is NOT_SUPPORTED is a success: kept only
    let bgp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_BGP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    sai.set(TRAP, bgp, &action(sys::SAI_PACKET_ACTION_DROP)).unwrap();
    assert!(!trap_programmed(sai.store().unwrap(), bgp));
    model_and_chip_agree(&sai).unwrap();
}

/// tests/driver-scripts/05-hostif-traps.ops through the recording decorator: from the first
/// host interface on, the same statuses and the same backend call lines as the C++ library on
/// its fake backend (`make driver-check`, build/driver-out/05-hostif-traps.1, 2026-11-23).
#[test]
fn the_call_log_of_the_hostif_driver_script_is_the_cpps() {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    let profile = Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "8"), ("SAI_RS_VIRTUAL_COMPAT", "cpp")]);
    let attrs = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac([2, 0, 0, 0, 0, 1])),
    ];
    let switch = sai.create_switch(&attrs, &profile).unwrap().raw();
    let log = sai.backend().log();
    let ports = oid_list(
        sai.get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
            .unwrap()
            .remove(0),
    );
    let cpu = oid(sai
        .get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_CPU_PORT])
        .unwrap()
        .remove(0));
    let trapgroup0 = oid(sai
        .get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP])
        .unwrap()
        .remove(0));
    log.take();

    let mut statuses = Vec::new();
    let mut st = |r: sai_core::Result<()>| statuses.push(r.err());
    let h0 = sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0]));
    st(h0.map(drop));
    let h0 = h0.unwrap();
    st(sai.create(HOSTIF, switch, &netdev("Ethernet0", ports[0])).map(drop));
    st(sai.create(HOSTIF, switch, &netdev("cpu", cpu)).map(drop));
    st(sai.set(HOSTIF, h0, &oper(true)));
    st(sai.set(HOSTIF, h0, &vlan_tag(sys::SAI_HOSTIF_VLAN_TAG_KEEP)));
    let grp = sai.create(GROUP, switch, &[queue(4)]).unwrap();
    let arp = trap(
        sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST,
        sys::SAI_PACKET_ACTION_COPY,
        Some(grp),
    );
    let t_arp = sai.create(TRAP, switch, &arp).unwrap();
    st(sai.create(TRAP, switch, &arp).map(drop));
    let t_bgp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_BGP, sys::SAI_PACKET_ACTION_TRAP, Some(grp)),
        )
        .unwrap();
    let t_lacp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LACP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    let t_lldp = sai
        .create(
            TRAP,
            switch,
            &trap(sys::SAI_HOSTIF_TRAP_TYPE_LLDP, sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    st(sai.remove(GROUP, grp));
    st(sai.set(TRAP, t_arp, &action(sys::SAI_PACKET_ACTION_DROP)));
    let te = sai.create(ENTRY, switch, &wildcard_entry()).unwrap();
    st(sai.remove(ENTRY, te));
    for t in [t_arp, t_bgp, t_lacp, t_lldp] {
        st(sai.remove(TRAP, t));
    }
    st(sai.remove(GROUP, grp));
    st(sai.remove(GROUP, trapgroup0));
    st(sai.remove(HOSTIF, h0));
    st(sai.remove(HOSTIF, h0));
    st(sai.remove(SWITCH, switch));
    assert_eq!(
        statuses,
        [
            None,
            Some(Error::ItemAlreadyExists),
            Some(Error::NotSupported),
            None,
            None,
            Some(Error::ItemAlreadyExists),
            Some(Error::ObjectInUse),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Error::ObjectInUse),
            None,
            Some(Error::InvalidObjectId),
            None,
        ]
    );
    let spec = |t: u32, a: u32, queue: u32| {
        format!(
            "trap_program(t={{type={t},action={a},priority=0,queue={queue},group_up=true,policer=-1,\
             yellow_drop=false,red_drop=false}}, remove=false) -> NOT_SUPPORTED"
        )
    };
    let (arp_t, bgp_t, lacp_t, lldp_t) = (
        sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST,
        sys::SAI_HOSTIF_TRAP_TYPE_BGP,
        sys::SAI_HOSTIF_TRAP_TYPE_LACP,
        sys::SAI_HOSTIF_TRAP_TYPE_LLDP,
    );
    let (copy, trap_a, drop_a) = (
        sys::SAI_PACKET_ACTION_COPY,
        sys::SAI_PACKET_ACTION_TRAP,
        sys::SAI_PACKET_ACTION_DROP,
    );
    // the C++ fake's lines (05-hostif-traps.1), from the first host interface on
    let expected = [
        "netif_create(ifname=\"Ethernet0\", hw=1, vid=1, mac=02:00:00:00:00:01, keep_tag=false) -> SUCCESS netif_id=1 filter_id=2".to_owned(),
        "netif_carrier_set(ifname=\"Ethernet0\", up=false) -> SUCCESS".to_owned(),
        "netif_carrier_set(ifname=\"Ethernet0\", up=true) -> SUCCESS".to_owned(),
        "netif_set_keep_tag(netif_id=1, hw=1, keep_tag=true, filter_id=-1) -> SUCCESS filter_id=3".to_owned(),
        spec(arp_t, copy, 4),
        "trap_set(kind=ArpRequest, to_cpu=true) -> SUCCESS".to_owned(),
        spec(bgp_t, trap_a, 4),
        "trap_set(kind=Other, to_cpu=true) -> NOT_SUPPORTED".to_owned(),
        spec(lacp_t, trap_a, 0),
        "trap_set(kind=Lacp, to_cpu=true) -> SUCCESS".to_owned(),
        spec(lldp_t, trap_a, 0),
        "trap_set(kind=Lldp, to_cpu=true) -> SUCCESS".to_owned(),
        spec(arp_t, drop_a, 4),
        "trap_set(kind=ArpRequest, to_cpu=false) -> SUCCESS".to_owned(),
        "trap_set(kind=ArpRequest, to_cpu=false) -> SUCCESS".to_owned(),
        "trap_set(kind=Lacp, to_cpu=false) -> SUCCESS".to_owned(),
        "trap_set(kind=Lldp, to_cpu=false) -> SUCCESS".to_owned(),
        "netif_destroy(netif_id=1, filter_id=3) -> SUCCESS".to_owned(),
        "shutdown() -> void".to_owned(),
    ];
    assert_eq!(log.take(), expected);
    assert_eq!(arp_t, 8192, "the C++ line's number");
}

#[test]
fn the_trap_kinds_the_test_port_reads() {
    for (t, kind) in [
        (sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST, TrapKind::ArpRequest),
        (sys::SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE, TrapKind::ArpResponse),
        (sys::SAI_HOSTIF_TRAP_TYPE_DHCP_L2, TrapKind::Dhcp),
        (sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6, TrapKind::Dhcpv6),
        (sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT, TrapKind::Igmp),
        (sys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT, TrapKind::Mld),
        (sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION, TrapKind::Nd),
        (sys::SAI_HOSTIF_TRAP_TYPE_LLDP, TrapKind::Lldp),
        (sys::SAI_HOSTIF_TRAP_TYPE_LACP, TrapKind::Lacp),
        (sys::SAI_HOSTIF_TRAP_TYPE_STP, TrapKind::Stp),
        (sys::SAI_HOSTIF_TRAP_TYPE_IP2ME, TrapKind::Other),
        (sys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET, TrapKind::Other),
    ] {
        assert_eq!(trap_kind(t.cast_signed()), kind, "{t}");
    }
}

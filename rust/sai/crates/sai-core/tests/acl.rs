//! ACL, policers and `CoPP` on the virtual backend: the checks of the C++ suite's
//! `tests/test_acl.inc` (`test_policer_copp`, `test_acl`) through the core's entry points, the
//! failure rule (a refused backend call leaves model and chip as they were) and the backend call
//! log of the C++ driver script `07-acl-copp.ops` (its ACL part, line for line).
#![allow(missing_docs, clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]
mod common;

use common::{PORT, SWITCH, enumerator, get1, oid, oid_list, switch};
use sai_core::acl::acl_backend_id;
use sai_core::backend::recording::{Mode, Recording};
use sai_core::backend::types::{FpEntryId, FpGroupId, FpQual, PolicerId};
use sai_core::error::{AttrError, Error};
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{AclAction, AclField, AclFieldData, AclKind, AclParam, Attr, FieldMatch, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::net::{Ipv4Addr, Ipv6Addr};

const POLICER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_POLICER;
const TABLE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_TABLE;
const ENTRY: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_ENTRY;
const COUNTER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_COUNTER;
const GROUP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP;
const MEMBER: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP_MEMBER;
const TRAP_GROUP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP;
const TRAP: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_HOSTIF_TRAP;

fn not_supported(i: usize) -> Error {
    Error::attr(AttrError::NotSupported, i)
}

fn invalid(i: usize) -> Error {
    Error::attr(AttrError::InvalidValue, i)
}

fn enabled(data: AclFieldData) -> Value {
    Value::AclField(AclField::Enabled(data))
}

fn act(param: AclParam) -> Value {
    Value::AclAction(AclAction::Enabled(param))
}

fn packet_action(a: u32) -> Value {
    act(AclParam::Enum(a.cast_signed()))
}

fn mk_policer<B: sai_core::backend::Backend>(sai: &mut Sai<B>, sw: RawOid, cir: u64, cbs: u64, red: u32) -> RawOid {
    sai.create(
        POLICER,
        sw,
        &[
            Attr::new(
                sys::SAI_POLICER_ATTR_METER_TYPE,
                enumerator(sys::SAI_METER_TYPE_PACKETS),
            ),
            Attr::new(sys::SAI_POLICER_ATTR_MODE, enumerator(sys::SAI_POLICER_MODE_SR_TCM)),
            Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(cir)),
            Attr::new(sys::SAI_POLICER_ATTR_CBS, Value::U64(cbs)),
            Attr::new(sys::SAI_POLICER_ATTR_RED_PACKET_ACTION, enumerator(red)),
        ],
    )
    .unwrap()
}

/// `AclOrch`'s L3 table: ingress, PORT + LAG, its match fields (C++ `mk_l3_table`).
fn l3_table_attrs(stage: u32, extra: Option<sys::sai_attr_id_t>) -> Vec<Attr> {
    let mut a = vec![Attr::new(
        sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
        Value::I32List(vec![
            sys::SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed(),
            sys::SAI_ACL_BIND_POINT_TYPE_LAG.cast_signed(),
        ]),
    )];
    for f in [
        sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IP,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT,
        sys::SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS,
    ]
    .into_iter()
    .chain(extra)
    {
        a.push(Attr::new(f, Value::Bool(true)));
    }
    a.push(Attr::new(
        sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE,
        Value::I32List(vec![
            sys::SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE.cast_signed(),
            sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE.cast_signed(),
        ]),
    ));
    a.push(Attr::new(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, enumerator(stage)));
    a
}

fn mk_counter<B: sai_core::backend::Backend>(sai: &mut Sai<B>, sw: RawOid, table: RawOid) -> RawOid {
    sai.create(
        COUNTER,
        sw,
        &[
            Attr::new(sys::SAI_ACL_COUNTER_ATTR_TABLE_ID, Value::Oid(table)),
            Attr::new(sys::SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT, Value::Bool(true)),
            Attr::new(sys::SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT, Value::Bool(true)),
        ],
    )
    .unwrap()
}

fn dst_ip(a: [u8; 4]) -> Value {
    enabled(AclFieldData::Ip4(FieldMatch {
        data: Ipv4Addr::from(a),
        mask: Ipv4Addr::BROADCAST,
    }))
}

/// `AclRule::createRule` (C++ `mk_rule`): `TABLE_ID`, `PRIORITY`, `ADMIN_STATE`, `COUNTER`, `DST_IP`, an
/// optional extra field (data 0x0806), the packet action.
fn rule_attrs(
    table: RawOid,
    prio: u32,
    counter: RawOid,
    ip: [u8; 4],
    action: u32,
    extra: Option<sys::sai_attr_id_t>,
) -> Vec<Attr> {
    let mut a = vec![
        Attr::new(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(table)),
        Attr::new(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, Value::U32(prio)),
        Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(true)),
    ];
    if !counter.is_null() {
        a.push(Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER,
            act(AclParam::Oid(counter)),
        ));
    }
    a.push(Attr::new(sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP, dst_ip(ip)));
    if let Some(f) = extra {
        a.push(Attr::new(
            f,
            enabled(AclFieldData::U16(FieldMatch {
                data: 0x0806,
                mask: 0xffff,
            })),
        ));
    }
    a.push(Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
        packet_action(action),
    ));
    a
}

fn ports(sai: &Sai<VirtualBackend>, sw: RawOid) -> Vec<RawOid> {
    oid_list(get1(sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap())
}

fn hw_of(port: RawOid) -> u16 {
    u16::try_from(port.0 & 0xffff_ffff_ffff).unwrap()
}

/// The backend entry an ACL entry is installed as.
fn fp_entry(sai: &Sai<VirtualBackend>, e: RawOid) -> Option<sai_core::virt::FpEntry> {
    let id = acl_backend_id(sai.store().unwrap(), e)?;
    sai.backend().fp().entries.get(&FpEntryId(id)).cloned()
}

fn counter_packets(sai: &Sai<VirtualBackend>, k: RawOid) -> u64 {
    match sai
        .get(
            COUNTER,
            k,
            &[sys::SAI_ACL_COUNTER_ATTR_PACKETS, sys::SAI_ACL_COUNTER_ATTR_BYTES],
        )
        .unwrap()
        .first()
    {
        Some(Value::U64(p)) => *p,
        other => panic!("{other:?}"),
    }
}

fn bind(sai: &mut Sai<VirtualBackend>, port: RawOid, attr: sys::sai_attr_id_t, acl: RawOid) -> sai_core::Result<()> {
    sai.set(PORT, port, &Attr::new(attr, Value::Oid(acl)))
}

/// The port leaves VLAN 1 and its bridge port goes (C++ `free_port`), so a LAG can take it.
fn free_port(sai: &mut Sai<VirtualBackend>, sw: RawOid, port: RawOid) {
    let bridge = oid(get1(sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap());
    let bridge_ports =
        oid_list(get1(sai, sys::SAI_OBJECT_TYPE_BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap());
    let Some(bp) = bridge_ports.into_iter().find(|bp| {
        get1(
            sai,
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
            *bp,
            sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
        ) == Ok(Value::Oid(port))
    }) else {
        return;
    };
    let vlan1 = oid(get1(sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap());
    for m in oid_list(get1(sai, sys::SAI_OBJECT_TYPE_VLAN, vlan1, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap()) {
        if get1(
            sai,
            sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
            m,
            sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID,
        ) == Ok(Value::Oid(bp))
        {
            sai.remove(sys::SAI_OBJECT_TYPE_VLAN_MEMBER, m).unwrap();
        }
    }
    sai.remove(sys::SAI_OBJECT_TYPE_BRIDGE_PORT, bp).unwrap();
}

/// A trap; a null group is left out (the default group: the C++ test passes null, which the
/// metadata check refuses here).
fn mk_trap(sai: &mut Sai<VirtualBackend>, sw: RawOid, trap_type: u32, action: u32, group: RawOid) -> RawOid {
    let mut attrs = vec![
        Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE, enumerator(trap_type)),
        Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, enumerator(action)),
    ];
    if !group.is_null() {
        attrs.push(Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(group)));
    }
    sai.create(TRAP, sw, &attrs).unwrap()
}

fn copp_entries(sai: &Sai<VirtualBackend>) -> Vec<sai_core::virt::FpEntry> {
    let Some(gid) = sai_core::copp::copp_group(sai.store().unwrap()) else {
        return Vec::new();
    };
    sai.backend()
        .fp()
        .entries
        .values()
        .filter(|e| e.gid == gid)
        .cloned()
        .collect()
}

fn pid_of(sai: &Sai<VirtualBackend>, p: RawOid) -> PolicerId {
    PolicerId(acl_backend_id(sai.store().unwrap(), p).unwrap())
}

#[test]
fn policers_and_copp() {
    let (mut sai, sw) = switch(32);
    // policer objects: CoppOrch's attributes
    let p = mk_policer(&mut sai, sw, 600, 600, sys::SAI_PACKET_ACTION_DROP);
    let pid = pid_of(&sai, p);
    let cfg = *sai.backend().fp().policers.get(&pid).unwrap();
    assert!(cfg.packets && !cfg.tr_tcm && cfg.cir == 600 && cfg.cbs == 600 && !cfg.color_blind);
    assert_eq!(
        sai.get(
            POLICER,
            p,
            &[
                sys::SAI_POLICER_ATTR_CIR,
                sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
                sys::SAI_POLICER_ATTR_COLOR_SOURCE
            ]
        ),
        Ok(vec![
            Value::U64(600),
            enumerator(sys::SAI_PACKET_ACTION_DROP),
            enumerator(sys::SAI_POLICER_COLOR_SOURCE_AWARE)
        ])
    );
    sai.set(POLICER, p, &Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(1000)))
        .unwrap();
    assert_eq!(sai.backend().fp().policers.get(&pid).unwrap().cir, 1000);
    // create-only: the C++ policer_attr's answer
    assert_eq!(
        sai.set(
            POLICER,
            p,
            &Attr::new(sys::SAI_POLICER_ATTR_MODE, enumerator(sys::SAI_POLICER_MODE_TR_TCM))
        ),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    let meter = Attr::new(
        sys::SAI_POLICER_ATTR_METER_TYPE,
        enumerator(sys::SAI_METER_TYPE_PACKETS),
    );
    assert_eq!(
        sai.create(
            POLICER,
            sw,
            &[meter.clone(), Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(5))]
        ),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        sai.create(
            POLICER,
            sw,
            &[
                meter.clone(),
                Attr::new(
                    sys::SAI_POLICER_ATTR_MODE,
                    enumerator(sys::SAI_POLICER_MODE_STORM_CONTROL)
                )
            ]
        ),
        Err(not_supported(1))
    );
    // PIR < CIR: the virtual chip (like RFC 2698) refuses
    assert!(
        sai.create(
            POLICER,
            sw,
            &[
                meter,
                Attr::new(sys::SAI_POLICER_ATTR_MODE, enumerator(sys::SAI_POLICER_MODE_TR_TCM)),
                Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(100)),
                Attr::new(sys::SAI_POLICER_ATTR_PIR, Value::U64(50)),
            ]
        )
        .is_err()
    );
    assert_eq!(
        sai.stats_get(POLICER, p, &[sys::SAI_POLICER_STAT_PACKETS], false),
        Err(Error::NotSupported)
    );

    // CoPP: a trap group with the policer, traps in it -> field entries
    let q = Attr::new(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, Value::U32(4));
    let grp = sai.create(TRAP_GROUP, sw, core::slice::from_ref(&q)).unwrap();
    let mut pa1 = Attr::new(sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, Value::Oid(p));
    sai.set(TRAP_GROUP, grp, &pa1).unwrap();
    assert_eq!(sai_core::copp::copp_group(sai.store().unwrap()), None);
    let arp = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST,
        sys::SAI_PACKET_ACTION_COPY,
        grp,
    );
    let v = copp_entries(&sai);
    assert_eq!(v.len(), 1);
    let r = &v.first().unwrap().rule;
    assert_eq!(r.m.ip_type, Some(sys::SAI_ACL_IP_TYPE_ARP_REQUEST.cast_signed()));
    assert!(r.policer == Some(pid) && r.red_drop && r.cpu_copy_only && !r.drop);
    assert!(r.m.in_ports.is_none()); // every port
    let gid: FpGroupId = sai_core::copp::copp_group(sai.store().unwrap()).unwrap();
    let group = *sai.backend().fp().groups.get(&gid).unwrap();
    assert!(group.qset & FpQual::DstCpu.bit() != 0 && group.priority > 999);
    let lacp = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_LACP,
        sys::SAI_PACKET_ACTION_TRAP,
        grp,
    );
    let bgp = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_BGP,
        sys::SAI_PACKET_ACTION_TRAP,
        grp,
    );
    let ip2me = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_IP2ME,
        sys::SAI_PACKET_ACTION_TRAP,
        grp,
    );
    // no match: unpoliced
    let ttl = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR,
        sys::SAI_PACKET_ACTION_TRAP,
        grp,
    );
    let v = copp_entries(&sai);
    assert_eq!(v.len(), 1 + 1 + 2 + 1);
    let (mut n_lacp, mut n_bgp, mut n_ip2me) = (0, 0, 0);
    for e in &v {
        let r = &e.rule;
        if r.m.ether_type.is_some_and(|m| m.value == 0x8809) {
            n_lacp += 1;
            assert!(r.red_drop && !r.cpu_copy_only && r.priority == 200);
        }
        if r.m.l4_dst_port.is_some_and(|m| m.value == 179) {
            n_bgp += 1;
            assert!(r.m.dst_cpu && r.priority == 150);
        }
        if r.m.l4_src_port.is_some_and(|m| m.value == 179) {
            n_bgp += 1;
        }
        if r.m.present() == FpQual::DstCpu.bit() {
            n_ip2me += 1;
            assert!(r.priority == 100 && r.red_drop);
        }
        // the policer is shared (one meter for the group)
        assert_eq!(r.policer, Some(pid));
    }
    assert_eq!((n_lacp, n_bgp, n_ip2me), (1, 2, 1));
    assert_eq!(sai.remove(POLICER, p), Err(Error::ObjectInUse));
    // red action FORWARD: entries re-installed without the colour drop
    let mut red = Attr::new(
        sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
        enumerator(sys::SAI_PACKET_ACTION_FORWARD),
    );
    sai.set(POLICER, p, &red).unwrap();
    let v = copp_entries(&sai);
    assert_eq!(v.len(), 5);
    assert!(v.iter().all(|e| !e.rule.red_drop));
    red.value = enumerator(sys::SAI_PACKET_ACTION_DROP);
    sai.set(POLICER, p, &red).unwrap();
    // a trap that stops going to the CPU loses its rule; another group without a policer: none
    sai.set(
        TRAP,
        lacp,
        &Attr::new(
            sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION,
            enumerator(sys::SAI_PACKET_ACTION_DROP),
        ),
    )
    .unwrap();
    assert_eq!(copp_entries(&sai).len(), 4);
    let grp2 = sai.create(TRAP_GROUP, sw, &[q]).unwrap();
    sai.set(
        TRAP,
        bgp,
        &Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(grp2)),
    )
    .unwrap();
    assert_eq!(copp_entries(&sai).len(), 2);
    // policer off the group: no CoPP entries, group gone
    pa1.value = Value::Oid(RawOid::NULL);
    sai.set(TRAP_GROUP, grp, &pa1).unwrap();
    assert_eq!(sai_core::copp::copp_rule_count(sai.store().unwrap()), 0);
    assert_eq!(sai_core::copp::copp_group(sai.store().unwrap()), None);
    pa1.value = Value::Oid(sai_core::oid::PolicerId::from_index(999).raw());
    assert_eq!(sai.set(TRAP_GROUP, grp, &pa1), Err(invalid(0)));
    // policer on the default trap group (CoppOrch's "default"): traps in it are policed
    let def = oid(get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP).unwrap());
    pa1.value = Value::Oid(p);
    sai.set(TRAP_GROUP, def, &pa1).unwrap();
    let lldp = mk_trap(
        &mut sai,
        sw,
        sys::SAI_HOSTIF_TRAP_TYPE_LLDP,
        sys::SAI_PACKET_ACTION_TRAP,
        RawOid::NULL,
    );
    assert_eq!(copp_entries(&sai).len(), 1);
    for t in [arp, lacp, bgp, ip2me, ttl, lldp] {
        sai.remove(TRAP, t).unwrap();
    }
    assert_eq!(sai_core::copp::copp_rule_count(sai.store().unwrap()), 0);
    pa1.value = Value::Oid(RawOid::NULL);
    sai.set(TRAP_GROUP, def, &pa1).unwrap();
    sai.remove(TRAP_GROUP, grp).unwrap();
    sai.remove(TRAP_GROUP, grp2).unwrap();
    sai.remove(POLICER, p).unwrap();
    let fp = sai.backend().fp();
    assert!(fp.policers.is_empty() && fp.entries.is_empty() && fp.groups.is_empty());
    sai.store().unwrap().check_invariants().unwrap();
}

#[test]
fn tables_entries_counters_groups_and_bindings() {
    let (mut sai, sw) = switch(32);
    // capabilities AclOrch reads
    assert_eq!(
        get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT),
        Ok(Value::U32(3))
    );
    match get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS) {
        Ok(Value::AclCapability {
            action_list_mandatory,
            actions,
            ..
        }) => {
            assert!(!action_list_mandatory);
            assert_eq!(actions.len(), 3);
            assert!(actions.contains(&sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION.cast_signed()));
        }
        other => panic!("{other:?}"),
    }
    match get1(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_ACL_STAGE_EGRESS) {
        Ok(Value::AclCapability { actions, .. }) => assert_eq!(actions, [0i32; 0]),
        other => panic!("{other:?}"),
    }

    // unsupported: egress, a match field we do not program, ranges
    assert_eq!(
        sai.create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_EGRESS, None)),
        Err(not_supported(12))
    );
    assert_eq!(
        sai.create(
            TABLE,
            sw,
            &l3_table_attrs(
                sys::SAI_ACL_STAGE_INGRESS,
                Some(sys::SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_PRI)
            )
        ),
        Err(not_supported(11))
    );
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_ACL_RANGE, sw, &[]),
        Err(Error::NotSupported)
    );

    // table -> SDK field group with the table's qualifiers + in-ports
    let t = sai
        .create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_INGRESS, None))
        .unwrap();
    let gid = FpGroupId(acl_backend_id(sai.store().unwrap(), t).unwrap());
    let qset = sai.backend().fp().groups.get(&gid).unwrap().qset;
    for (q, want) in [
        (FpQual::InPorts, true),
        (FpQual::DstIp, true),
        (FpQual::IcmpTypeCode, true),
        (FpQual::DstIp6, false),
    ] {
        assert_eq!(qset & q.bit() != 0, want, "{q:?}");
    }
    assert_eq!(
        get1(&sai, TABLE, t, sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        get1(&sai, TABLE, t, sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6),
        Ok(Value::Bool(false))
    );
    // CRM: unsupported, cleanly
    assert_eq!(
        get1(&sai, TABLE, t, sys::SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY),
        Err(not_supported(0))
    );

    // rule: drop to 10.0.0.1, with a counter; table not bound -> not installed
    let k = mk_counter(&mut sai, sw, t);
    let e = sai
        .create(
            ENTRY,
            sw,
            &rule_attrs(t, 9000, k, [10, 0, 0, 1], sys::SAI_PACKET_ACTION_DROP, None),
        )
        .unwrap();
    assert_eq!(acl_backend_id(sai.store().unwrap(), e), None);
    assert_eq!(
        sai.create(
            ENTRY,
            sw,
            &rule_attrs(
                t,
                10,
                RawOid::NULL,
                [10, 0, 0, 2],
                sys::SAI_PACKET_ACTION_DROP,
                Some(sys::SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID)
            )
        ),
        Err(invalid(4))
    );
    // counter taken
    assert_eq!(
        sai.create(
            ENTRY,
            sw,
            &rule_attrs(t, 11, k, [10, 0, 0, 3], sys::SAI_PACKET_ACTION_DROP, None)
        ),
        Err(Error::ObjectInUse)
    );

    // PortsOrch: table group, bind it to the port, member = the table
    let all = ports(&sai, sw);
    let (port_a, port_b) = (*all.get(20).unwrap(), *all.get(21).unwrap());
    let (hw_a, hw_b) = (hw_of(port_a), hw_of(port_b));
    let grp = sai
        .create(
            GROUP,
            sw,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
                    enumerator(sys::SAI_ACL_STAGE_INGRESS),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST,
                    Value::I32List(vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed()]),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE,
                    enumerator(sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL),
                ),
            ],
        )
        .unwrap();
    bind(&mut sai, port_a, sys::SAI_PORT_ATTR_INGRESS_ACL, grp).unwrap();
    // the table is not a member yet
    assert_eq!(acl_backend_id(sai.store().unwrap(), e), None);
    let mem = sai
        .create(
            MEMBER,
            sw,
            &[
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, Value::Oid(grp)),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, Value::Oid(t)),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, Value::U32(100)),
            ],
        )
        .unwrap();
    let fe = fp_entry(&sai, e).unwrap();
    assert!(fe.rule.drop && fe.rule.counter && fe.rule.priority == 9000);
    assert_eq!(
        fe.rule.m.in_ports.as_deref(),
        Some(&[sai_core::backend::HwPort(hw_a)][..])
    );
    let dst = fe.rule.m.dst_ip.unwrap();
    assert_eq!((dst.value, dst.mask), (Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::BROADCAST));
    assert_eq!(
        get1(&sai, PORT, port_a, sys::SAI_PORT_ATTR_INGRESS_ACL),
        Ok(Value::Oid(grp))
    );
    assert_eq!(
        bind(&mut sai, port_b, sys::SAI_PORT_ATTR_EGRESS_ACL, grp),
        Err(not_supported(0))
    );

    // counter: hits on the SDK entry; kept when the entry is re-created
    let eid = FpEntryId(acl_backend_id(sai.store().unwrap(), e).unwrap());
    sai.backend_mut().fp_hit(eid, 5, 500);
    assert_eq!(counter_packets(&sai, k), 5);
    // second port in the same group
    bind(&mut sai, port_b, sys::SAI_PORT_ATTR_INGRESS_ACL, grp).unwrap();
    let fe = fp_entry(&sai, e).unwrap();
    let (lo, hi) = (hw_a.min(hw_b), hw_a.max(hw_b));
    assert_eq!(
        fe.rule.m.in_ports,
        Some(vec![sai_core::backend::HwPort(lo), sai_core::backend::HwPort(hi)])
    );
    assert_eq!(counter_packets(&sai, k), 5);
    let eid = FpEntryId(acl_backend_id(sai.store().unwrap(), e).unwrap());
    sai.backend_mut().fp_hit(eid, 2, 200);
    assert_eq!(counter_packets(&sai, k), 7);
    sai.set(COUNTER, k, &Attr::new(sys::SAI_ACL_COUNTER_ATTR_PACKETS, Value::U64(0)))
        .unwrap();
    assert_eq!(counter_packets(&sai, k), 0);

    // entry attributes: action, admin state, priority
    sai.set(
        ENTRY,
        e,
        &Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            packet_action(sys::SAI_PACKET_ACTION_FORWARD),
        ),
    )
    .unwrap();
    let fe = fp_entry(&sai, e).unwrap();
    assert!(!fe.rule.drop && fe.rule.drop_cancel);
    sai.set(
        ENTRY,
        e,
        &Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(false)),
    )
    .unwrap();
    assert_eq!(acl_backend_id(sai.store().unwrap(), e), None);
    sai.set(
        ENTRY,
        e,
        &Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    assert!(acl_backend_id(sai.store().unwrap(), e).is_some());
    // the same counter again: no conflict with itself
    sai.set(
        ENTRY,
        e,
        &Attr::new(sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, act(AclParam::Oid(k))),
    )
    .unwrap();
    let got = sai
        .get(
            ENTRY,
            e,
            &[sys::SAI_ACL_ENTRY_ATTR_PRIORITY, sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP],
        )
        .unwrap();
    assert_eq!(got.first(), Some(&Value::U32(9000)));
    assert_eq!(got.get(1), Some(&dst_ip([10, 0, 0, 1])));
    // unset: answered disabled, of the attribute's kind
    assert_eq!(
        get1(&sai, ENTRY, e, sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP),
        Ok(Value::AclField(AclField::Disabled(AclKind::Ip4)))
    );

    // LAG binding: the LAG's member ports, following membership
    free_port(&mut sai, sw, *all.get(22).unwrap());
    free_port(&mut sai, sw, *all.get(23).unwrap());
    let lag = sai.create(sys::SAI_OBJECT_TYPE_LAG, sw, &[]).unwrap();
    let member = |port: RawOid| {
        vec![
            Attr::new(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)),
            Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)),
        ]
    };
    let lm1 = sai
        .create(sys::SAI_OBJECT_TYPE_LAG_MEMBER, sw, &member(*all.get(22).unwrap()))
        .unwrap();
    let mut lb = Attr::new(sys::SAI_LAG_ATTR_INGRESS_ACL, Value::Oid(grp));
    sai.set(sys::SAI_OBJECT_TYPE_LAG, lag, &lb).unwrap();
    assert_eq!(fp_entry(&sai, e).unwrap().rule.m.in_ports.map(|p| p.len()), Some(3));
    let lm2 = sai
        .create(sys::SAI_OBJECT_TYPE_LAG_MEMBER, sw, &member(*all.get(23).unwrap()))
        .unwrap();
    assert_eq!(fp_entry(&sai, e).unwrap().rule.m.in_ports.map(|p| p.len()), Some(4));
    sai.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm2).unwrap();
    lb.value = Value::Oid(RawOid::NULL);
    sai.set(sys::SAI_OBJECT_TYPE_LAG, lag, &lb).unwrap();
    sai.remove(sys::SAI_OBJECT_TYPE_LAG_MEMBER, lm1).unwrap();
    sai.remove(sys::SAI_OBJECT_TYPE_LAG, lag).unwrap();
    assert_eq!(fp_entry(&sai, e).unwrap().rule.m.in_ports.map(|p| p.len()), Some(2));

    // in use: table with entries / members, group bound, counter on an entry
    assert_eq!(sai.remove(TABLE, t), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(GROUP, grp), Err(Error::ObjectInUse));
    assert_eq!(sai.remove(COUNTER, k), Err(Error::ObjectInUse));

    // AclOrch / PortsOrch teardown order
    sai.remove(ENTRY, e).unwrap();
    sai.remove(COUNTER, k).unwrap();
    sai.remove(MEMBER, mem).unwrap();
    bind(&mut sai, port_a, sys::SAI_PORT_ATTR_INGRESS_ACL, RawOid::NULL).unwrap();
    bind(&mut sai, port_b, sys::SAI_PORT_ATTR_INGRESS_ACL, RawOid::NULL).unwrap();
    sai.remove(GROUP, grp).unwrap();
    sai.remove(TABLE, t).unwrap();
    assert!(sai.backend().fp().entries.is_empty() && sai.backend().fp().groups.is_empty());
    let store = sai.store().unwrap();
    assert_eq!(store.iter::<sai_core::acl::AclTable>().count(), 0);
    assert_eq!(store.iter::<sai_core::acl::AclEntry>().count(), 0);
    assert_eq!(sai_core::acl::acl_binding_count(store), 0);

    // L3V6: an IPv6 match keeps all 16 bytes
    let tv6 = sai
        .create(
            TABLE,
            sw,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
                    enumerator(sys::SAI_ACL_STAGE_INGRESS),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                    Value::I32List(vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed()]),
                ),
                Attr::new(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6, Value::Bool(true)),
            ],
        )
        .unwrap();
    let bytes: [u8; 16] = core::array::from_fn(|b| 0x20 + u8::try_from(b).unwrap());
    let v6 = enabled(AclFieldData::Ip6(FieldMatch {
        data: Ipv6Addr::from(bytes),
        mask: Ipv6Addr::from([0xff; 16]),
    }));
    let ev6 = sai
        .create(
            ENTRY,
            sw,
            &[
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(tv6)),
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6, v6.clone()),
                Attr::new(
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                    packet_action(sys::SAI_PACKET_ACTION_DROP),
                ),
            ],
        )
        .unwrap();
    assert_eq!(get1(&sai, ENTRY, ev6, sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6), Ok(v6));
    bind(&mut sai, port_b, sys::SAI_PORT_ATTR_INGRESS_ACL, tv6).unwrap();
    let fe = fp_entry(&sai, ev6).unwrap();
    assert_eq!(fe.rule.m.dst_ip6.map(|m| m.value.octets()[15]), Some(0x2f));
    bind(&mut sai, port_b, sys::SAI_PORT_ATTR_INGRESS_ACL, RawOid::NULL).unwrap();
    sai.remove(ENTRY, ev6).unwrap();
    sai.remove(TABLE, tv6).unwrap();

    // a table bound directly (no group) works too
    let t = sai
        .create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_INGRESS, None))
        .unwrap();
    let e = sai
        .create(
            ENTRY,
            sw,
            &rule_attrs(t, 5, RawOid::NULL, [192, 0, 2, 1], sys::SAI_PACKET_ACTION_TRAP, None),
        )
        .unwrap();
    bind(&mut sai, port_a, sys::SAI_PORT_ATTR_INGRESS_ACL, t).unwrap();
    let fe = fp_entry(&sai, e).unwrap();
    assert!(fe.rule.drop && fe.rule.copy_cpu && !fe.rule.counter);
    bind(&mut sai, port_a, sys::SAI_PORT_ATTR_INGRESS_ACL, RawOid::NULL).unwrap();
    sai.remove(ENTRY, e).unwrap();
    sai.remove(TABLE, t).unwrap();
    sai.store().unwrap().check_invariants().unwrap();
}

#[test]
fn a_refused_backend_call_leaves_model_and_chip_as_they_were() {
    let (mut sai, sw) = switch(8);
    let objects = |sai: &Sai<VirtualBackend>| sai.store().unwrap().len();
    // policer create
    let before = objects(&sai);
    sai.backend_mut()
        .fail_nth("policer_create", 1, Error::TableFull)
        .unwrap();
    assert!(
        sai.create(
            POLICER,
            sw,
            &[
                Attr::new(
                    sys::SAI_POLICER_ATTR_METER_TYPE,
                    enumerator(sys::SAI_METER_TYPE_PACKETS)
                ),
                Attr::new(sys::SAI_POLICER_ATTR_MODE, enumerator(sys::SAI_POLICER_MODE_SR_TCM)),
                Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(10)),
            ]
        )
        .is_err()
    );
    assert_eq!(objects(&sai), before);
    assert!(sai.backend().fp().policers.is_empty());
    // policer set: the meter and the object keep the old rate
    let p = mk_policer(&mut sai, sw, 600, 600, sys::SAI_PACKET_ACTION_DROP);
    let pid = pid_of(&sai, p);
    sai.backend_mut().fail_nth("policer_set", 1, Error::Failure).unwrap();
    assert_eq!(
        sai.set(POLICER, p, &Attr::new(sys::SAI_POLICER_ATTR_CIR, Value::U64(9))),
        Err(Error::Failure)
    );
    assert_eq!(get1(&sai, POLICER, p, sys::SAI_POLICER_ATTR_CIR), Ok(Value::U64(600)));
    assert_eq!(sai.backend().fp().policers.get(&pid).unwrap().cir, 600);
    // policer remove
    sai.backend_mut()
        .fail_nth("policer_destroy", 1, Error::Failure)
        .unwrap();
    assert_eq!(sai.remove(POLICER, p), Err(Error::Failure));
    assert_eq!(get1(&sai, POLICER, p, sys::SAI_POLICER_ATTR_CIR), Ok(Value::U64(600)));
    sai.remove(POLICER, p).unwrap();
    // table create (the group priority is used up, as in the C++)
    let before = objects(&sai);
    sai.backend_mut()
        .fail_nth("fp_group_create", 1, Error::InsufficientResources)
        .unwrap();
    assert_eq!(
        sai.create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_INGRESS, None)),
        Err(Error::InsufficientResources)
    );
    assert_eq!(objects(&sai), before);
    assert!(sai.backend().fp().groups.is_empty());
    // entry create on a bound table
    let t = sai
        .create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_INGRESS, None))
        .unwrap();
    assert_eq!(
        sai.backend()
            .fp()
            .groups
            .values()
            .map(|g| g.priority)
            .collect::<Vec<_>>(),
        [2],
        "the failed create used priority 1"
    );
    let port = *ports(&sai, sw).first().unwrap();
    bind(&mut sai, port, sys::SAI_PORT_ATTR_INGRESS_ACL, t).unwrap();
    let before = objects(&sai);
    sai.backend_mut()
        .fail_nth("fp_entry_create", 1, Error::TableFull)
        .unwrap();
    assert_eq!(
        sai.create(
            ENTRY,
            sw,
            &rule_attrs(t, 5, RawOid::NULL, [10, 0, 0, 9], sys::SAI_PACKET_ACTION_DROP, None)
        ),
        Err(Error::TableFull)
    );
    assert_eq!(objects(&sai), before);
    assert!(sai.backend().fp().entries.is_empty());
    // table remove
    bind(&mut sai, port, sys::SAI_PORT_ATTR_INGRESS_ACL, RawOid::NULL).unwrap();
    sai.backend_mut()
        .fail_nth("fp_group_destroy", 1, Error::Failure)
        .unwrap();
    assert_eq!(sai.remove(TABLE, t), Err(Error::Failure));
    assert_eq!(sai.backend().fp().groups.len(), 1);
    assert_eq!(
        get1(&sai, TABLE, t, sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP),
        Ok(Value::Bool(true))
    );
    sai.remove(TABLE, t).unwrap();
    sai.store().unwrap().check_invariants().unwrap();
}

/// The ACL part of the C++ driver script `07-acl-copp.ops` (from `create acl_table` on, its
/// `port2` / `port3` are the third and fourth ports) and the backend calls the C++ library made
/// for it on its fake backend (`frontend_driver`, recording backend), line for line.
#[test]
fn the_backend_call_log_of_the_cpp_driver_script() {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    // the C++ driver's switch: 8 ports of 4 lanes, numbered as the C++ fake numbers them
    let profile = sai_core::backend::Profile::from_pairs([
        ("SAI_RS_VIRTUAL_PORTS", "8"),
        ("SAI_RS_VIRTUAL_LANES", "4"),
        ("SAI_RS_VIRTUAL_COMPAT", "cpp"),
    ]);
    let sw = sai.create_switch(&common::init_attrs(), &profile).unwrap().raw();
    let all = oid_list(
        sai.get(SWITCH, sw, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
            .unwrap()
            .remove(0),
    );
    let (port2, port3) = (*all.get(2).unwrap(), *all.get(3).unwrap());
    let _ = sai.backend().log().take();
    let step = |sai: &mut Sai<Recording<VirtualBackend>>, expected: &[&str]| {
        let lines = sai.backend().log().take();
        assert_eq!(lines, expected, "backend calls");
    };
    let t = sai
        .create(
            TABLE,
            sw,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
                    enumerator(sys::SAI_ACL_STAGE_INGRESS),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                    Value::I32List(vec![
                        sys::SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed(),
                        sys::SAI_ACL_BIND_POINT_TYPE_LAG.cast_signed(),
                    ]),
                ),
                Attr::new(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, Value::Bool(true)),
                Attr::new(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP, Value::Bool(true)),
                Attr::new(sys::SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT, Value::Bool(true)),
                Attr::new(
                    sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE,
                    Value::I32List(vec![
                        sys::SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE.cast_signed(),
                        sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE.cast_signed(),
                    ]),
                ),
            ],
        )
        .unwrap();
    step(
        &mut sai,
        &[
            "fp_group_create_ex(qset=[InPorts,EtherType,DstIp,L4DstPort], priority=1, x={egress=false,src_range=false,dst_range=false,out_ports=false,redirect=false,set_tc=false,set_dscp=false,mirror=false}) -> SUCCESS gid=1",
        ],
    );
    let k = mk_counter(&mut sai, sw, t);
    let e = sai
        .create(
            ENTRY,
            sw,
            &[
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(t)),
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, Value::U32(9000)),
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(true)),
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, act(AclParam::Oid(k))),
                Attr::new(sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP, dst_ip([10, 0, 0, 1])),
                Attr::new(
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                    packet_action(sys::SAI_PACKET_ACTION_DROP),
                ),
            ],
        )
        .unwrap();
    step(&mut sai, &[]);
    let g = sai
        .create(
            GROUP,
            sw,
            &[
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
                    enumerator(sys::SAI_ACL_STAGE_INGRESS),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST,
                    Value::I32List(vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT.cast_signed()]),
                ),
                Attr::new(
                    sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE,
                    enumerator(sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL),
                ),
            ],
        )
        .unwrap();
    sai.set(PORT, port2, &Attr::new(sys::SAI_PORT_ATTR_INGRESS_ACL, Value::Oid(g)))
        .unwrap();
    step(&mut sai, &[]);
    let gm = sai
        .create(
            MEMBER,
            sw,
            &[
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, Value::Oid(g)),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, Value::Oid(t)),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, Value::U32(100)),
            ],
        )
        .unwrap();
    let rule = "priority=9000,m={InPorts=[9],DstIp=10.0.0.1/255.255.255.255},drop=true,drop_cancel=false,copy_cpu=false,counter=true,policer=-1,green_drop=false,yellow_drop=false,red_drop=false,cpu_copy_only=false,mirror_ingress=[],mirror_egress=[],redirect=-1,redirect_egress=false,set_tc=-1,set_dscp=-1";
    step(
        &mut sai,
        &[format!("fp_entry_create(gid=1, r={{{rule}}}) -> SUCCESS eid=1").as_str()],
    );
    sai.set(PORT, port3, &Attr::new(sys::SAI_PORT_ATTR_INGRESS_ACL, Value::Oid(g)))
        .unwrap();
    let both = rule.replace("[9]", "[9,13]");
    step(
        &mut sai,
        &[
            "fp_entry_counter(eid=1) -> SUCCESS packets=0 bytes=0",
            "fp_entry_destroy(eid=1) -> SUCCESS",
            format!("fp_entry_create(gid=1, r={{{both}}}) -> SUCCESS eid=2").as_str(),
        ],
    );
    // one counter read per get call, as the C++ (sai_acl.cpp:1270)
    assert_eq!(
        sai.get(
            COUNTER,
            k,
            &[sys::SAI_ACL_COUNTER_ATTR_PACKETS, sys::SAI_ACL_COUNTER_ATTR_BYTES]
        ),
        Ok(vec![Value::U64(0), Value::U64(0)])
    );
    step(&mut sai, &["fp_entry_counter(eid=2) -> SUCCESS packets=0 bytes=0"]);
    sai.set(
        ENTRY,
        e,
        &Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            packet_action(sys::SAI_PACKET_ACTION_FORWARD),
        ),
    )
    .unwrap();
    let fwd = both.replace("drop=true,drop_cancel=false", "drop=false,drop_cancel=true");
    step(
        &mut sai,
        &[
            "fp_entry_counter(eid=2) -> SUCCESS packets=0 bytes=0",
            "fp_entry_destroy(eid=2) -> SUCCESS",
            format!("fp_entry_create(gid=1, r={{{fwd}}}) -> SUCCESS eid=3").as_str(),
        ],
    );
    sai.set(
        ENTRY,
        e,
        &Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(false)),
    )
    .unwrap();
    step(
        &mut sai,
        &[
            "fp_entry_counter(eid=3) -> SUCCESS packets=0 bytes=0",
            "fp_entry_destroy(eid=3) -> SUCCESS",
        ],
    );
    sai.set(
        ENTRY,
        e,
        &Attr::new(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    step(
        &mut sai,
        &[format!("fp_entry_create(gid=1, r={{{fwd}}}) -> SUCCESS eid=4").as_str()],
    );
    assert_eq!(
        sai.get(
            ENTRY,
            e,
            &[
                sys::SAI_ACL_ENTRY_ATTR_PRIORITY,
                sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP,
                sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION
            ]
        ),
        Ok(vec![
            Value::U32(9000),
            dst_ip([10, 0, 0, 1]),
            packet_action(sys::SAI_PACKET_ACTION_FORWARD)
        ])
    );
    assert_eq!(sai.remove(TABLE, t), Err(Error::ObjectInUse));
    sai.remove(ENTRY, e).unwrap();
    step(
        &mut sai,
        &[
            "fp_entry_counter(eid=4) -> SUCCESS packets=0 bytes=0",
            "fp_entry_destroy(eid=4) -> SUCCESS",
        ],
    );
    sai.remove(COUNTER, k).unwrap();
    sai.remove(MEMBER, gm).unwrap();
    sai.set(
        PORT,
        port2,
        &Attr::new(sys::SAI_PORT_ATTR_INGRESS_ACL, Value::Oid(RawOid::NULL)),
    )
    .unwrap();
    sai.set(
        PORT,
        port3,
        &Attr::new(sys::SAI_PORT_ATTR_INGRESS_ACL, Value::Oid(RawOid::NULL)),
    )
    .unwrap();
    sai.remove(GROUP, g).unwrap();
    step(&mut sai, &[]);
    sai.remove(TABLE, t).unwrap();
    step(&mut sai, &["fp_group_destroy(gid=1) -> SUCCESS"]);
}

/// Every attribute the capability table advertises as readable answers on a live ACL object or
/// policer (C++ `test_acl`: "every advertised get on the ACL / POLICER objects works").
#[test]
fn every_advertised_get_on_a_live_object_works() {
    let (mut sai, sw) = switch(8);
    let t = sai
        .create(TABLE, sw, &l3_table_attrs(sys::SAI_ACL_STAGE_INGRESS, None))
        .unwrap();
    let k = mk_counter(&mut sai, sw, t);
    let e = sai
        .create(
            ENTRY,
            sw,
            &rule_attrs(t, 9000, k, [10, 0, 0, 1], sys::SAI_PACKET_ACTION_DROP, None),
        )
        .unwrap();
    let grp = sai
        .create(
            GROUP,
            sw,
            &[Attr::new(
                sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE,
                enumerator(sys::SAI_ACL_STAGE_INGRESS),
            )],
        )
        .unwrap();
    let mem = sai
        .create(
            MEMBER,
            sw,
            &[
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, Value::Oid(grp)),
                Attr::new(sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, Value::Oid(t)),
            ],
        )
        .unwrap();
    let pol = mk_policer(&mut sai, sw, 100, 100, sys::SAI_PACKET_ACTION_DROP);
    let mut asked = 0;
    for (object_type, attr, cap) in sai_core::caps::attributes() {
        let oid = match object_type {
            TABLE => t,
            ENTRY => e,
            COUNTER => k,
            GROUP => grp,
            MEMBER => mem,
            POLICER => pol,
            _ => continue,
        };
        if cap.get {
            assert!(sai.get(object_type, oid, &[attr]).is_ok(), "get {object_type} {attr}");
            asked += 1;
        }
    }
    assert!(asked > 40, "{asked}");
}

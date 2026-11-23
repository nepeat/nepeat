//! The library's log lines (`sai_core::log`) for representative operations per area, as the C++
//! sai-openbcm logs them (`OB_NOTICE` / `OB_INFO` / `OB_WARN` / `OB_ERR` of `sai_*.cpp` at
//! commit 50a6aa7: API, level and text): through [`log::capture`], which takes the lines
//! instead of syslog. Every test of this file captures (captures take turns), so the lines each
//! sees are its own.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod common;

use common::*;
use sai_core::backend::HwPort;
use sai_core::hostif::trap_kind;
use sai_core::log::{self, Line};
use sai_core::model::{EntryKey, Port};
use sai_core::oid::RawOid;
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_core::{Error, backend::types::TrapKind};
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};
use sys::{
    SAI_API_BRIDGE as BRIDGE_API, SAI_API_FDB as FDB, SAI_API_HASH as HASH, SAI_API_HOSTIF as HIF, SAI_API_LAG as LAG,
    SAI_API_MIRROR as MIRROR, SAI_API_NEIGHBOR as NEIGHBOR, SAI_API_NEXT_HOP as NH, SAI_API_PORT as PORT_API,
    SAI_API_ROUTER_INTERFACE as RIF_API, SAI_API_SCHEDULER_GROUP as SG, SAI_API_SWITCH as SW,
    SAI_API_VIRTUAL_ROUTER as VR_API, SAI_API_VLAN as VLAN_API,
};

/// Every API at `level`.
fn all_at(level: sys::sai_log_level_t) {
    for api in 0..=sys::SAI_API_MAX {
        assert!(log::set_level(api, level));
    }
}

/// The lines as `(api, level, message)`.
fn messages(lines: &[Line]) -> Vec<(sys::sai_api_t, sys::sai_log_level_t, String)> {
    lines.iter().map(|l| (l.api, l.level, l.message().to_owned())).collect()
}

/// `want` is among the lines (API, level and text).
#[track_caller]
fn has(lines: &[Line], api: sys::sai_api_t, level: sys::sai_log_level_t, want: &str) {
    assert!(
        lines
            .iter()
            .any(|l| l.api == api && l.level == level && l.message() == want),
        "no line (api {api}, level {level}) {want:?} in:\n{}",
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    );
}

/// A line of `api` / `level` that starts with `prefix`; its message.
#[track_caller]
fn starting(lines: &[Line], api: sys::sai_api_t, level: sys::sai_log_level_t, prefix: &str) -> String {
    lines
        .iter()
        .find(|l| l.api == api && l.level == level && l.message().starts_with(prefix))
        .map_or_else(
            || {
                panic!(
                    "no line (api {api}, level {level}) starting {prefix:?} in:\n{}",
                    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
                )
            },
            |l| l.message().to_owned(),
        )
}

fn ports(sai: &Sai<VirtualBackend>, switch: RawOid) -> Vec<RawOid> {
    oid_list(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap())
}

/// The default bridge port of `port`.
fn default_bridge_port(sai: &Sai<VirtualBackend>, switch: RawOid, port: RawOid) -> RawOid {
    let bridge = oid(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID).unwrap());
    oid_list(get1(sai, BRIDGE, bridge, sys::SAI_BRIDGE_ATTR_PORT_LIST).unwrap())
        .into_iter()
        .find(|bp| get1(sai, BRIDGE_PORT, *bp, sys::SAI_BRIDGE_PORT_ATTR_PORT_ID) == Ok(Value::Oid(port)))
        .unwrap()
}

/// Takes `port` out of the default bridge (its VLAN 1 membership and bridge port), as orchagent
/// does before it makes the port a LAG member.
fn detach(sai: &mut Sai<VirtualBackend>, switch: RawOid, port: RawOid) {
    let bp = default_bridge_port(sai, switch, port);
    let vlan1 = oid(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID).unwrap());
    for m in oid_list(get1(sai, VLAN, vlan1, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap()) {
        if get1(sai, VLAN_MEMBER, m, sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID) == Ok(Value::Oid(bp)) {
            sai.remove(VLAN_MEMBER, m).unwrap();
        }
    }
    sai.remove(BRIDGE_PORT, bp).unwrap();
}

/// The SDK name and chip port of a port object.
fn port_of(sai: &Sai<VirtualBackend>, port: RawOid) -> (String, HwPort) {
    let store = sai.store().unwrap();
    let p = store
        .get::<Port>(sai_core::oid::PortId::try_from(port).unwrap())
        .unwrap();
    (p.name.clone(), p.hw)
}

const NOTICE: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_NOTICE;
const INFO: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_INFO;
const WARN: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_WARN;
const ERROR: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_ERROR;

fn e(v: u32) -> Value {
    Value::Enum(v.cast_signed())
}

#[test]
fn the_areas_log_what_the_cpp_logs() {
    let (names, lines) = log::capture(|| {
        all_at(INFO);
        let (mut sai, sw) = switch(8);
        let ports = ports(&sai, sw);
        let (p0, _) = port_of(&sai, ports[0]);
        let (p1, _) = port_of(&sai, ports[1]);
        let (p2, hw2) = port_of(&sai, ports[2]);
        let events = sai.take_events().unwrap();

        // host interfaces (sai_hostif.cpp)
        let netdev = |name: &str, obj: RawOid| {
            vec![
                Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, e(sys::SAI_HOSTIF_TYPE_NETDEV)),
                Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(obj)),
                Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(name.into())),
            ]
        };
        sai.create(HOSTIF, sw, &netdev("Ethernet0", ports[0])).unwrap();
        let mut bad_type = netdev("Ethernet9", ports[6]);
        bad_type[0] = Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, e(sys::SAI_HOSTIF_TYPE_FD));
        assert!(sai.create(HOSTIF, sw, &bad_type).is_err());
        sai.backend_mut().fail_nth("netif_create", 1, Error::TableFull).unwrap();
        assert_eq!(
            sai.create(HOSTIF, sw, &netdev("Ethernet4", ports[1])),
            Err(Error::TableFull)
        );
        let genl = vec![
            Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, e(sys::SAI_HOSTIF_TYPE_GENETLINK)),
            Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData("psample".into())),
            Attr::new(
                sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME,
                Value::CharData("packets".into()),
            ),
        ];
        assert!(sai.create(HOSTIF, sw, &genl).is_err()); // the virtual chip mirrors nothing
        let ttl = sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR;
        assert_eq!(trap_kind(ttl.cast_signed()), TrapKind::Other);
        sai.create(
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP,
            sw,
            &[
                Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE, e(ttl)),
                Attr::new(sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, e(sys::SAI_PACKET_ACTION_TRAP)),
            ],
        )
        .unwrap();
        let entry = |entry_type: u32| {
            vec![
                Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE, e(entry_type)),
                Attr::new(sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_OBJ_ID, Value::Oid(ports[0])),
                Attr::new(
                    sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE,
                    e(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_CB),
                ),
            ]
        };
        sai.create(
            sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY,
            sw,
            &entry(sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_PORT),
        )
        .unwrap();

        // L2 (sai_l2.cpp)
        let vlan = sai
            .create(VLAN, sw, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(100))])
            .unwrap();
        assert_eq!(sai.create(BRIDGE, sw, &[]), Err(Error::NotSupported));
        let bport = |on: RawOid| {
            vec![
                Attr::new(sys::SAI_BRIDGE_PORT_ATTR_TYPE, e(sys::SAI_BRIDGE_PORT_TYPE_PORT)),
                Attr::new(sys::SAI_BRIDGE_PORT_ATTR_PORT_ID, Value::Oid(on)),
            ]
        };
        let mut sub = bport(ports[3]);
        sub[0] = Attr::new(sys::SAI_BRIDGE_PORT_ATTR_TYPE, e(sys::SAI_BRIDGE_PORT_TYPE_SUB_PORT));
        assert!(sai.create(BRIDGE_PORT, sw, &sub).is_err());
        let bp3 = default_bridge_port(&sai, sw, ports[3]);
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let fdb = EntryKey::Fdb { bv_id: vlan, mac };
        sai.create_entry(
            sw,
            fdb,
            &[
                Attr::new(sys::SAI_FDB_ENTRY_ATTR_TYPE, e(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC)),
                Attr::new(sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, Value::Oid(bp3)),
            ],
        )
        .unwrap();
        assert_eq!(sai.flush_fdb(sw, &[]).map(|k| k.len()), Ok(1));

        // L3 (sai_l3.cpp)
        let vr = sai.create(VR, sw, &[]).unwrap();
        sai.create(RIF, sw, &port_rif(vr, ports[1])).unwrap();
        let lo = vec![
            Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(vr)),
            Attr::new(
                sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                e(sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK),
            ),
        ];
        sai.create(RIF, sw, &lo).unwrap();
        let mut sub_port = lo.clone();
        sub_port[1] = Attr::new(
            sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
            e(sys::SAI_ROUTER_INTERFACE_TYPE_SUB_PORT),
        );
        assert_eq!(sai.create(RIF, sw, &sub_port), Err(Error::NotSupported));
        let vlan_rif = sai
            .create(
                RIF,
                sw,
                &[
                    Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID, Value::Oid(vr)),
                    Attr::new(
                        sys::SAI_ROUTER_INTERFACE_ATTR_TYPE,
                        e(sys::SAI_ROUTER_INTERFACE_TYPE_VLAN),
                    ),
                    Attr::new(sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID, Value::Oid(vlan)),
                ],
            )
            .unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        sai.create(
            NEXT_HOP,
            sw,
            &[
                Attr::new(sys::SAI_NEXT_HOP_ATTR_TYPE, e(sys::SAI_NEXT_HOP_TYPE_IP)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_IP, Value::IpAddress(ip)),
                Attr::new(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID, Value::Oid(vlan_rif)),
            ],
        )
        .unwrap();
        let nmac = [0x02, 0, 0, 0, 0, 0x02];
        sai.create_entry(
            sw,
            EntryKey::Neighbor { rif: vlan_rif, ip },
            &[Attr::new(
                sys::SAI_NEIGHBOR_ENTRY_ATTR_DST_MAC_ADDRESS,
                Value::Mac(nmac),
            )],
        )
        .unwrap();

        // LAGs (sai_lag.cpp)
        detach(&mut sai, sw, ports[2]);
        let lag = sai.create(sys::SAI_OBJECT_TYPE_LAG, sw, &[]).unwrap();
        let member = |port: RawOid| {
            vec![
                Attr::new(sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)),
                Attr::new(sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)),
            ]
        };
        sai.create(sys::SAI_OBJECT_TYPE_LAG_MEMBER, sw, &member(ports[2]))
            .unwrap();
        assert_eq!(
            sai.create(sys::SAI_OBJECT_TYPE_LAG_MEMBER, sw, &member(ports[1])),
            Err(Error::ObjectInUse)
        );
        assert_eq!(
            sai.create(BRIDGE_PORT, sw, &bport(ports[2])),
            Err(Error::InvalidParameter)
        );

        // ports and the switch (sai_port.cpp, sai_switch.cpp)
        sai.set(
            PORT,
            ports[2],
            &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
        )
        .unwrap();
        sai.backend_mut().inject_link(hw2, true).unwrap();
        let notes: Vec<_> = events.try_iter().flat_map(|ev| sai.handle_event(ev)).collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(sai.create(PORT, sw, &[]), Err(Error::NotSupported));
        assert_eq!(sai.remove(PORT, ports[7]), Err(Error::NotSupported));

        // hash, QoS, mirror (sai_hash.cpp, sai_qos.cpp, sai_mirror.cpp)
        assert_eq!(sai.create(sys::SAI_OBJECT_TYPE_HASH, sw, &[]), Err(Error::NotSupported));
        assert_eq!(
            sai.create(sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP, sw, &[]),
            Err(Error::NotSupported)
        );
        sai.create(
            sys::SAI_OBJECT_TYPE_MIRROR_SESSION,
            sw,
            &[
                Attr::new(sys::SAI_MIRROR_SESSION_ATTR_TYPE, e(sys::SAI_MIRROR_SESSION_TYPE_LOCAL)),
                Attr::new(sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(ports[5])),
            ],
        )
        .unwrap();
        sai.remove(SWITCH, sw).unwrap();
        all_at(WARN);
        (p0, p1, p2, hw2)
    });
    let (p0, p1, p2, hw2) = names;
    has(&lines, SW, NOTICE, "initializing backend fake");
    has(&lines, SW, NOTICE, "switch created: 8 front-panel ports, backend fake");
    let hostif = starting(&lines, HIF, NOTICE, &format!("hostif Ethernet0 on {p0} (netif "));
    assert!(hostif.ends_with(')') && hostif.contains(", filter "), "{hostif}");
    has(
        &lines,
        HIF,
        ERROR,
        &format!(
            "hostif type {} not supported (NETDEV, GENETLINK)",
            sys::SAI_HOSTIF_TYPE_FD
        ),
    );
    has(
        &lines,
        HIF,
        ERROR,
        &format!("hostif Ethernet4: netif create failed ({})", Error::TableFull.status()),
    );
    has(
        &lines,
        HIF,
        ERROR,
        "hostif psample: GENETLINK not supported by the backend",
    );
    has(
        &lines,
        HIF,
        NOTICE,
        &format!(
            "trap type {} kept but not programmed (no SDK control for it)",
            sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR
        ),
    );
    has(
        &lines,
        HIF,
        NOTICE,
        &format!(
            "hostif table entry type {} channel {} kept but not programmed",
            sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_PORT,
            sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_CB
        ),
    );
    has(&lines, VLAN_API, INFO, "VLAN 100 created");
    has(
        &lines,
        BRIDGE_API,
        ERROR,
        "only the default .1Q bridge exists (.1D bridges not supported)",
    );
    has(
        &lines,
        BRIDGE_API,
        ERROR,
        &format!(
            "bridge port type {} not supported (PORT only)",
            sys::SAI_BRIDGE_PORT_TYPE_SUB_PORT
        ),
    );
    has(&lines, FDB, INFO, "fdb 00:11:22:33:44:55 vlan 100 added");
    has(&lines, FDB, INFO, "flush: 1 entries removed");
    has(&lines, VR_API, NOTICE, "virtual router: SDK VRF 1");
    starting(
        &lines,
        RIF_API,
        NOTICE,
        &format!("rif type {} vid ", sys::SAI_ROUTER_INTERFACE_TYPE_PORT),
    );
    has(
        &lines,
        RIF_API,
        NOTICE,
        &format!(
            "rif type {} vid 0 intf -1 vrf 1",
            sys::SAI_ROUTER_INTERFACE_TYPE_LOOPBACK
        ),
    );
    starting(
        &lines,
        RIF_API,
        NOTICE,
        &format!("rif type {} vid 100 intf ", sys::SAI_ROUTER_INTERFACE_TYPE_VLAN),
    );
    has(
        &lines,
        RIF_API,
        ERROR,
        &format!(
            "router interface type {} not supported",
            sys::SAI_ROUTER_INTERFACE_TYPE_SUB_PORT
        ),
    );
    has(
        &lines,
        NH,
        INFO,
        "next hop without neighbor: points at the CPU (glean) until it resolves",
    );
    has(
        &lines,
        NEIGHBOR,
        WARN,
        "neighbor 02:00:00:00:00:02: egress port unknown (no FDB entry yet); sending to CPU",
    );
    let created = starting(&lines, LAG, NOTICE, "LAG created (trunk ");
    let tid = created.trim_start_matches("LAG created (trunk ").trim_end_matches(')');
    has(
        &lines,
        LAG,
        NOTICE,
        &format!("LAG trunk {tid}: member {p2} (egress on, ingress on)"),
    );
    has(
        &lines,
        LAG,
        ERROR,
        &format!("LAG member: port {p1} still has a bridge port or router interface"),
    );
    has(
        &lines,
        BRIDGE_API,
        ERROR,
        &format!("bridge port: port {p2} is a LAG member"),
    );
    has(&lines, PORT_API, NOTICE, &format!("port {p2} admin up"));
    has(&lines, PORT_API, NOTICE, &format!("port {p2} (hw {}) oper up", hw2.0));
    has(
        &lines,
        PORT_API,
        ERROR,
        "dynamic port creation (breakout) is not supported; ports come from the SDK config",
    );
    has(&lines, PORT_API, ERROR, "port removal is not supported");
    has(
        &lines,
        HASH,
        WARN,
        "hash objects are the switch's LAG_HASH / ECMP_HASH; create refused",
    );
    has(
        &lines,
        SG,
        WARN,
        "scheduler groups are fixed (one per queue); create refused",
    );
    let mirror = starting(&lines, MIRROR, INFO, "mirror session 0x");
    assert!(
        mirror.contains(" SPAN monitor ") && mirror.ends_with(", backend id -1"),
        "{mirror}"
    );
    has(&lines, SW, NOTICE, "switch removed");
    // nothing else wrote to stderr or syslog meanwhile, and every line is the C++ shape
    for l in &lines {
        assert!(l.text.starts_with("sai-openbcm "), "{}", l.text);
    }
}

#[test]
fn levels_gate_lines_per_api() {
    let ((), lines) = log::capture(|| {
        all_at(WARN);
        let (mut sai, sw) = switch(2);
        // WARN: the switch's NOTICE lines are not written, the hash refusal (WARN) is
        assert_eq!(sai.create(sys::SAI_OBJECT_TYPE_HASH, sw, &[]), Err(Error::NotSupported));
        assert!(log::set_level(sys::SAI_API_VLAN, INFO));
        sai.create(VLAN, sw, &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(7))])
            .unwrap();
        // the LAG API stays at WARN: its NOTICE is not written
        sai.create(sys::SAI_OBJECT_TYPE_LAG, sw, &[]).unwrap();
        all_at(WARN);
    });
    assert_eq!(
        messages(&lines),
        vec![
            (
                sys::SAI_API_HASH,
                WARN,
                "hash objects are the switch's LAG_HASH / ECMP_HASH; create refused".to_owned()
            ),
            (sys::SAI_API_VLAN, INFO, "VLAN 7 created".to_owned()),
        ]
    );
}

#[test]
fn lines_of_other_threads_are_captured_and_the_diag_server_logs_at_notice() {
    let ((), lines) = log::capture(|| {
        all_at(WARN);
        std::thread::spawn(|| {
            sai_core::diag::log(sai_diag::Level::Notice, "diag socket: ps ce0");
        })
        .join()
        .unwrap();
        assert!(log::set_level(sys::SAI_API_SWITCH, NOTICE));
        std::thread::spawn(|| {
            sai_core::diag::log(sai_diag::Level::Notice, "diag socket: show c ce0");
            sai_core::diag::log(
                sai_diag::Level::Error,
                "diag socket: mkdir /x: No such file or directory",
            );
        })
        .join()
        .unwrap();
        all_at(WARN);
    });
    assert_eq!(
        messages(&lines),
        vec![
            (sys::SAI_API_SWITCH, NOTICE, "diag socket: show c ce0".to_owned()),
            (
                sys::SAI_API_SWITCH,
                ERROR,
                "diag socket: mkdir /x: No such file or directory".to_owned()
            ),
        ]
    );
}

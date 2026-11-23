//! The backend against sdk-glue's Rust fake, every chip its own (the tests run in parallel):
//! the bring-up and its glue calls, every operation's success (the chip's state afterwards) and a
//! scripted fault per glue call mapped through the status table, the policer-clone fallback, the
//! egress refusals, the carrier file, link and L2 events, link gates, temperatures, MDIO, and the
//! failures of init. Runs on any host and, from the test bundle, on big-endian ppc64.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code"
)]

mod common;

use common::{Rig, own_profile, read, rig, trace, unique};
use sai_backend_openbcm::status::sai_error;
use sai_backend_openbcm::{CONFIG_KEY, KNET_LINK, OpenbcmBackend, PROFILE_KEYS, RC_KEY, UNIT_KEY};
use sai_core::Error;
use sai_core::backend::types::{
    Dest, EcmpId, EgressId, FdbChange, FdbEvent, Fec, FilterId, FpCap, FpEntryId, FpGroupId, FpMatch, FpQual, FpRule,
    GateVerdict, IntfId, IpPrefix, LagMember, Learn, LinkGate, Loopback, MacAddr, Masked, MdioDev, NetifId, PhyAddr,
    PolicerCfg, PolicerId, PortInterface, PortStat, RouteTarget, StationId, TrapKind, TrapSpec, TrunkId, Vrf,
};
use sai_core::backend::{
    Backend, BackendEvent, Base, EventSink, Field, Hash, Hostif, HwPort, L2, L3, Lag, Mdio, Mirror, Policer, PortInfo,
    PortOps, Profile, Qos, Shell, Temp, Vid,
};
use sdk_glue::rust_fake::{Chip, OPS};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

fn vid(v: u16) -> Vid {
    Vid::new(v).unwrap()
}

const MAC: MacAddr = MacAddr([0x02, 0, 0, 0, 0, 0x11]);

/// Every glue code (and one without a name): what a scripted fault returns.
const CODES: [i32; 20] = [
    -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14, -15, -16, -17, -18, -100, -57,
];

fn mapped(code: i32) -> Error {
    sai_error(sdk_glue::Error::from_raw(code).unwrap())
}

fn v4(a: [u8; 4]) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(a))
}

fn prefix(a: [u8; 4], m: [u8; 4]) -> IpPrefix {
    IpPrefix::new(v4(a), v4(m)).unwrap()
}

/// Init must fail with `error` on a fresh chip: nothing left running, the sink dropped.
fn init_fails(profile: &Profile, error: Error) -> Chip {
    let chip = Chip::new();
    let mut backend = OpenbcmBackend::on_chip(chip.clone());
    let (sink, events) = EventSink::channel();
    assert_eq!(backend.init(profile, sink), Err(error));
    assert!(!chip.bcm_layer_up(), "the BCM layer is still up after a failed init");
    assert!(!chip.link_callback_registered() && !chip.l2_callback_registered());
    assert_eq!(
        events.try_recv(),
        Err(TryRecvError::Disconnected),
        "the event sink is still held"
    );
    assert_eq!(backend.ports(), vec![]);
    assert_eq!(backend.hardware_info(), None);
    assert_eq!(backend.port_link_get(HwPort(1)), Err(Error::Uninitialized));
    // the same backend comes up afterwards
    let (sink, _events) = EventSink::channel();
    backend.init(&own_profile("ports 1 4 100000\n", &[]), sink).unwrap();
    chip
}

#[test]
fn init_makes_the_cpp_calls_and_nothing_else() {
    let (t, path) = trace();
    let rc = unique("led.soc");
    std::fs::write(&rc, "# led program\n").unwrap();
    let rc_text = rc.display().to_string();
    let mut r = rig(
        &format!("name BCM56960_B1\nports 2 4 100000\nport 66 129 1 10000 xe0\n{t}"),
        &[(RC_KEY, &rc_text), (UNIT_KEY, "0")],
    );
    let cpu = r.backend.l3_egress_cpu().unwrap();
    let drop = r.backend.l3_egress_drop().unwrap();
    assert_ne!(cpu, drop);
    // ob_sdk_init, ob_sdk_register_callbacks, ob_sdk_l3_egress_special(0), (1), ob_sdk_chip_limits
    // (not traced by the fake): no port is touched
    assert_eq!(
        read(&path),
        format!(
            "init-all unit 0 rc {rc_text}\nregister unit 0\negress special cpu -> {}\negress special drop -> {}\n",
            cpu.0, drop.0
        )
    );
    assert_eq!(r.chip.register_calls(), 1);
    assert!(r.chip.link_callback_registered() && r.chip.l2_callback_registered());
    for port in [1, 2, 66] {
        assert_eq!(r.chip.port_enabled(port), Some(true), "as \"init all\" left it");
        assert_eq!(r.chip.port_settings(port), Some((-1, -1, -1, false)));
    }
    assert_eq!(r.chip.counts().egress, 2);
    assert_eq!(r.backend.name(), "openbcm");
    assert!(!r.backend.is_fake());
    assert_eq!(r.backend.hardware_info(), Some("BCM56960_B1".to_owned()));
    assert_eq!(Base::hardware_info(&r.backend), "BCM56960_B1");
    assert_eq!(r.backend.cpu_port(), HwPort(0));
    assert_eq!(
        r.backend.ports(),
        vec![
            PortInfo {
                hw: HwPort(1),
                lanes: vec![1, 2, 3, 4],
                speed: 100_000,
                name: "ce0".into(),
                autoneg: false,
            },
            PortInfo {
                hw: HwPort(2),
                lanes: vec![5, 6, 7, 8],
                speed: 100_000,
                name: "ce1".into(),
                autoneg: false,
            },
            PortInfo {
                hw: HwPort(66),
                lanes: vec![129],
                speed: 10_000,
                name: "xe0".into(),
                autoneg: false,
            },
        ]
    );
    assert!(r.events.try_recv().is_err());
    assert_eq!(PROFILE_KEYS, [CONFIG_KEY, RC_KEY, UNIT_KEY]);
    assert_eq!(
        PROFILE_KEYS,
        ["SAI_INIT_CONFIG_FILE", "SAI_OPENBCM_RC_FILE", "SAI_OPENBCM_UNIT"]
    );
    for key in PROFILE_KEYS {
        assert!(sai_core::keys::EXPLICIT.contains(&key));
    }
    assert_eq!(KNET_LINK, "/proc/bcm/knet/link");
    assert_eq!(sai_backend_openbcm::GLUE, sdk_glue::LINKED);
}

#[test]
fn ports_cpu_port_and_chip_name_are_asked_on_every_call() {
    let mut r = rig("name X\nports 3 4 100000\ncpu 7\n", &[]);
    assert_eq!(r.backend.cpu_port(), HwPort(7));
    // the autoneg comes from ob_sdk_port_autoneg_get
    r.backend.port_autoneg_set(HwPort(2), true).unwrap();
    let an: Vec<bool> = r.backend.ports().iter().map(|p| p.autoneg).collect();
    assert_eq!(an, [false, true, false]);
    // a failed enumeration: no ports (logged), the next call has them again
    assert!(r.chip.fail("ports", 1, -11));
    assert_eq!(r.backend.ports(), vec![]);
    assert_eq!(r.backend.ports().len(), 3);
    // a failed autoneg read: that port's autoneg is false
    assert!(r.chip.fail("autoneg-get", 2, -11));
    let an: Vec<bool> = r.backend.ports().iter().map(|p| p.autoneg).collect();
    assert_eq!(an, [false, false, false]);
    // ob_sdk_hw_info's failure is an empty name
    assert!(r.chip.fail("hw-info", 1, -11));
    assert_eq!(Base::hardware_info(&r.backend), "");
    assert_eq!(Base::hardware_info(&r.backend), "X");
}

#[test]
fn a_refused_profile_touches_no_chip() {
    let chip = Chip::new();
    let mut backend = OpenbcmBackend::on_chip(chip.clone());
    let (sink, _events) = EventSink::channel();
    assert_eq!(backend.init(&Profile::default(), sink), Err(Error::Failure));
    assert_eq!(chip.counts(), sdk_glue::rust_fake::Counts::default());
    assert_eq!(chip.register_calls(), 0);
    assert!(!chip.bcm_layer_up());
    init_fails(&Profile::default(), Error::Failure);
    init_fails(&Profile::from_pairs([(CONFIG_KEY, "")]), Error::Failure);
    let missing = unique("missing.cfg");
    init_fails(
        &Profile::from_pairs([(CONFIG_KEY, missing.display().to_string())]),
        Error::Failure,
    );
    let dir = unique("a-directory");
    std::fs::create_dir_all(&dir).unwrap();
    init_fails(
        &Profile::from_pairs([(CONFIG_KEY, dir.display().to_string())]),
        Error::Failure,
    );
    for unit in ["x", "-1", "256", "0x0"] {
        init_fails(
            &own_profile("ports 1 4 100000\n", &[(UNIT_KEY, unit)]),
            Error::InvalidParameter,
        );
    }
    // an empty unit is the default; " 0 " too
    rig("ports 1 4 100000\n", &[(UNIT_KEY, "")]);
    rig("ports 1 4 100000\n", &[(UNIT_KEY, " 0 ")]);
}

#[test]
fn a_failed_bring_up_leaves_nothing_running() {
    // the glue's own refusals, mapped
    init_fails(
        &own_profile("ports 1 4 100000\n", &[(UNIT_KEY, "1")]),
        Error::InvalidParameter,
    ); // OB_E_UNIT
    init_fails(&own_profile("init all\n", &[]), Error::Failure); // OB_E_FAIL
    init_fails(&own_profile("init busy\n", &[]), Error::ObjectInUse); // OB_E_BUSY
    init_fails(&own_profile("not a directive\n", &[]), Error::InvalidParameter); // OB_E_CONFIG
    let rc = unique("missing.soc").display().to_string();
    init_fails(
        &own_profile("ports 1 4 100000\n", &[(RC_KEY, &rc)]),
        Error::InvalidParameter,
    );
    // after ob_sdk_init: the callbacks, the special egress objects
    let chip = init_fails(
        &own_profile("ports 1 4 100000\nfail register 1 -14\n", &[]),
        Error::InsufficientResources,
    );
    assert_eq!(chip.register_calls(), 1);
    init_fails(
        &own_profile("ports 1 4 100000\nfail egress-special 1 -6\n", &[]),
        Error::TableFull,
    );
    init_fails(
        &own_profile("ports 1 4 100000\nfail egress-special 2 -2\n", &[]),
        Error::NoMemory,
    );
    // the chip limits are only logged
    let r = rig("ports 1 4 100000\nfail chip-limits 1 -16\n", &[]);
    assert!(r.chip.bcm_layer_up());
}

#[test]
fn a_second_init_is_refused_and_shutdown_then_init_works_on_the_same_chip() {
    let (t, path) = trace();
    let mut r = rig(&format!("ports 2 4 100000\n{t}"), &[]);
    let (sink, _e) = EventSink::channel();
    assert_eq!(
        r.backend.init(&own_profile("ports 2 4 100000\n", &[]), sink),
        Err(Error::Failure)
    );
    assert!(r.chip.bcm_layer_up(), "the refused second init did not touch the chip");
    let gate: LinkGate = Arc::new(|| GateVerdict::NoOpinion);
    r.backend.port_link_gate(HwPort(1), Some(gate)).unwrap();
    let before = read(&path);
    r.backend.shutdown();
    // a gate still registered off, then ob_sdk_shutdown (which unregisters the callbacks in the glue)
    assert_eq!(
        read(&path).strip_prefix(before.as_str()).unwrap(),
        "gate 1 off\nunregister unit 0\nshutdown unit 0\n"
    );
    assert!(!r.chip.bcm_layer_up());
    assert_eq!(r.events.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(r.backend.ports(), vec![]);
    assert_eq!(r.backend.vlan_create(vid(5)), Err(Error::Uninitialized));
    assert_eq!(r.backend.l3_egress_cpu(), Err(Error::Failure));
    r.backend.shutdown(); // again: nothing
    let (sink, events) = EventSink::channel();
    r.backend.init(&own_profile("ports 3 4 100000\n", &[]), sink).unwrap();
    assert_eq!(r.backend.ports().len(), 3);
    assert!(r.chip.bcm_layer_up());
    assert!(r.chip.link_event(2, true));
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(2),
            up: true
        })
    );
}

#[test]
fn before_init_every_operation_is_uninitialized() {
    let chip = Chip::new();
    let mut b = OpenbcmBackend::on_chip(chip.clone());
    assert_eq!(b.ports(), vec![]);
    assert_eq!(b.cpu_port(), HwPort(0));
    assert_eq!(b.hardware_info(), None);
    assert_eq!(Base::hardware_info(&b), "");
    assert!(b.mdio_bus().is_none());
    assert!(b.temperature_reader().is_none());
    assert_eq!(b.port_admin_set(HwPort(1), true), Err(Error::Uninitialized));
    assert_eq!(
        b.port_stat_get(HwPort(1), PortStat::InOctets),
        Err(Error::Uninitialized)
    );
    assert_eq!(b.table_usage(), Err(Error::Uninitialized));
    assert_eq!(b.asic_temperatures(), Err(Error::Uninitialized));
    assert_eq!(
        b.mdio_c45_read(PhyAddr(0), MdioDev::new(1).unwrap(), 0),
        Err(Error::Uninitialized)
    );
    assert_eq!(b.lag_create(), Err(Error::Uninitialized));
    assert_eq!(b.fp_entry_destroy(FpEntryId(1)), Err(Error::Uninitialized));
    assert_eq!(b.policer_destroy(PolicerId(1)), Err(Error::Uninitialized));
    assert_eq!(b.l3_egress_cpu(), Err(Error::Failure));
    assert_eq!(b.l3_egress_drop(), Err(Error::Failure));
    assert_eq!(b.run_shell("ps"), Err(Error::Uninitialized));
    assert_eq!(chip.counts(), sdk_glue::rust_fake::Counts::default());
}

/// One glue call per row: the fake's op name and the backend operation that makes it.
type Call = Box<dyn Fn(&mut OpenbcmBackend) -> Result<(), Error>>;

#[test]
fn a_fault_of_every_glue_call_maps_through_the_status_table() {
    let mut r = rig("ports 4 4 100000\n", &[]);
    let b = &mut r.backend;
    // objects the lookups before the glue need
    let gid = b.fp_group_create(&[FpQual::InPorts], 1).unwrap();
    let counted = FpRule {
        counter: true,
        ..FpRule::default()
    };
    let eid = b.fp_entry_create(gid, &counted).unwrap();
    let net = prefix([10, 0, 0, 0], [255, 0, 0, 0]);
    let cfg = PolicerCfg::default();
    let gate: LinkGate = Arc::new(|| GateVerdict::NoOpinion);
    let rows: Vec<(&str, Call)> = vec![
        ("enable", Box::new(|b| b.port_admin_set(HwPort(1), true))),
        ("link-get", Box::new(|b| b.port_link_get(HwPort(1)).map(drop))),
        ("speed-set", Box::new(|b| b.port_speed_set(HwPort(1), 40_000))),
        ("speed-get", Box::new(|b| b.port_speed_get(HwPort(1)).map(drop))),
        ("speeds", Box::new(|b| b.port_supported_speeds(HwPort(1)).map(drop))),
        ("fec", Box::new(|b| b.port_fec_set(HwPort(1), Fec::Rs))),
        ("autoneg", Box::new(|b| b.port_autoneg_set(HwPort(1), true))),
        (
            "interface",
            Box::new(|b| b.port_interface_set(HwPort(1), PortInterface::Caui)),
        ),
        ("ifg", Box::new(|b| b.port_ifg_set(HwPort(1), 96))),
        ("vlan-create", Box::new(|b| b.vlan_create(vid(10)))),
        ("vlan-destroy", Box::new(|b| b.vlan_destroy(vid(10)))),
        ("vlan-add", Box::new(|b| b.vlan_port_add(vid(1), HwPort(1), true))),
        ("vlan-remove", Box::new(|b| b.vlan_port_remove(vid(1), HwPort(1)))),
        ("vlan-members", Box::new(|b| b.vlan_members(vid(1)).map(drop))),
        ("frame-max", Box::new(|b| b.port_frame_max_set(HwPort(1), 9100))),
        ("loopback", Box::new(|b| b.port_loopback_set(HwPort(1), Loopback::Mac))),
        ("pvid", Box::new(|b| b.port_pvid_set(HwPort(1), vid(1)))),
        ("learn", Box::new(|b| b.port_learn_set(HwPort(1), Learn::Cpu))),
        (
            "stat-get",
            Box::new(|b| b.port_stat_get(HwPort(1), PortStat::InOctets).map(drop)),
        ),
        ("stat-clear", Box::new(|b| b.port_stat_clear(HwPort(1)))),
        (
            "l2-add",
            Box::new(|b| b.l2_add(MAC, vid(1), Dest::Port(HwPort(1)), false, false)),
        ),
        ("l2-delete", Box::new(|b| b.l2_delete(MAC, vid(1)))),
        ("l2-flush", Box::new(|b| b.l2_flush(None, None, false, true))),
        ("l2-age", Box::new(|b| b.l2_age_set(300))),
        (
            "netif-create",
            Box::new(|b| b.netif_create("Ethernet0", HwPort(1), vid(1), MAC, false).map(drop)),
        ),
        ("netif-destroy", Box::new(|b| b.netif_destroy(NetifId(1), FilterId(1)))),
        (
            "netif-filter",
            Box::new(|b| b.netif_set_keep_tag(NetifId(1), HwPort(1), true, None).map(drop)),
        ),
        ("trap", Box::new(|b| b.trap_set(TrapKind::Lldp, true))),
        ("trunk-create", Box::new(|b| b.lag_create().map(drop))),
        ("trunk-destroy", Box::new(|b| b.lag_destroy(TrunkId(1)))),
        ("trunk-set", Box::new(|b| b.lag_set_members(TrunkId(1), &[]))),
        (
            "intf-create",
            Box::new(|b| b.l3_intf_create(MAC, vid(1), 1500, Vrf(0)).map(drop)),
        ),
        (
            "intf-update",
            Box::new(|b| b.l3_intf_update(IntfId(1), MAC, vid(1), 1500, Vrf(0))),
        ),
        ("intf-destroy", Box::new(|b| b.l3_intf_destroy(IntfId(1)))),
        ("station-add", Box::new(|b| b.l3_station_add(MAC).map(drop))),
        ("station-delete", Box::new(|b| b.l3_station_delete(StationId(1)))),
        (
            "egress-create",
            Box::new(|b| {
                b.l3_egress_create(IntfId(1), MAC, Dest::Port(HwPort(1)), vid(1))
                    .map(drop)
            }),
        ),
        (
            "egress-update",
            Box::new(|b| b.l3_egress_update(EgressId(1), IntfId(1), MAC, Dest::Port(HwPort(1)), vid(1))),
        ),
        ("egress-destroy", Box::new(|b| b.l3_egress_destroy(EgressId(1)))),
        (
            "host-add",
            Box::new(|b| b.l3_host_add(Vrf(0), v4([10, 0, 0, 1]), EgressId(1))),
        ),
        ("host-delete", Box::new(|b| b.l3_host_delete(Vrf(0), v4([10, 0, 0, 1])))),
        (
            "route-add",
            Box::new(move |b| b.l3_route_add(Vrf(0), net, RouteTarget::Egress(EgressId(1)), false)),
        ),
        ("route-delete", Box::new(move |b| b.l3_route_delete(Vrf(0), net))),
        ("ecmp-set", Box::new(|b| b.l3_ecmp_create(&[EgressId(1)]).map(drop))),
        ("ecmp-set", Box::new(|b| b.l3_ecmp_update(EcmpId(1), &[EgressId(1)]))),
        ("ecmp-destroy", Box::new(|b| b.l3_ecmp_destroy(EcmpId(1)))),
        (
            "fp-group-create",
            Box::new(|b| b.fp_group_create(&[FpQual::InPorts], 2).map(drop)),
        ),
        ("fp-group-destroy", Box::new(|b| b.fp_group_destroy(FpGroupId(12_345)))),
        (
            "fp-entry-create",
            Box::new(move |b| b.fp_entry_create(gid, &FpRule::default()).map(drop)),
        ),
        (
            "fp-entry-destroy",
            // the C++ forgets the entry also when the SDK refuses: a new one each time
            Box::new(move |b| {
                let e = b.fp_entry_create(gid, &FpRule::default()).unwrap();
                b.fp_entry_destroy(e)
            }),
        ),
        ("fp-stat", Box::new(move |b| b.fp_entry_counter(eid).map(drop))),
        ("policer-create", Box::new(move |b| b.policer_create(&cfg).map(drop))),
        ("policer-set", Box::new(move |b| b.policer_set(PolicerId(1), &cfg))),
        ("policer-destroy", Box::new(|b| b.policer_destroy(PolicerId(12_345)))),
        ("table-usage", Box::new(|b| b.table_usage().map(drop))),
        ("chip-limits", Box::new(|b| b.chip_limits().map(drop))),
        ("shell", Box::new(|b| b.run_shell("ps"))),
        (
            "shell",
            Box::new(|b| {
                let out = std::fs::File::create(unique("shell.txt")).unwrap();
                b.run_shell_to("ps", std::os::fd::AsFd::as_fd(&out))
            }),
        ),
        (
            "gate",
            Box::new(move |b| b.port_link_gate(HwPort(1), Some(Arc::clone(&gate)))),
        ),
        ("gate", Box::new(|b| b.port_link_gate(HwPort(1), None))),
    ];
    let mut compared = 0;
    for (i, (op, call)) in rows.iter().enumerate() {
        assert!(OPS.contains(op), "{op}: not an op of the fake");
        let code = CODES[i % CODES.len()];
        assert!(r.chip.fail(op, 1, code));
        assert_eq!(call(&mut r.backend), Err(mapped(code)), "{op} failing with {code}");
        compared += 1;
    }
    // the ops of the fake this backend never calls outside init / ports() / hardware_info()
    let covered: Vec<&str> = rows.iter().map(|(op, _)| *op).collect();
    let elsewhere = [
        "register",
        "egress-special",
        "ports",
        "autoneg-get",
        "hw-info",
        "knet-cleanup",
        "overtemp",
    ];
    let missing: Vec<&&str> = OPS
        .iter()
        .filter(|op| !covered.contains(op) && !elsewhere.contains(op))
        .collect();
    assert!(missing.is_empty(), "ops of the fake no row covers: {missing:?}");
    common::note(&format!("{compared} scripted glue faults mapped"));
}

#[test]
fn port_operations_reach_the_chip() {
    let mut r = rig("ports 2 4 100000\n", &[]);
    let b = &mut r.backend;
    let p = HwPort(1);
    b.port_admin_set(p, false).unwrap();
    assert_eq!(r.chip.port_enabled(1), Some(false));
    b.port_frame_max_set(p, 9100).unwrap();
    b.port_fec_set(p, Fec::Rs).unwrap();
    b.port_autoneg_set(p, true).unwrap();
    b.port_interface_set(p, PortInterface::Caui).unwrap();
    b.port_ifg_set(p, 96).unwrap();
    b.port_loopback_set(p, Loopback::Phy).unwrap();
    b.port_pvid_set(p, vid(10)).unwrap();
    b.port_learn_set(p, Learn::Cpu).unwrap();
    let port = r.chip.port(1).unwrap();
    assert_eq!(
        (
            port.frame_max,
            port.fec,
            port.autoneg,
            port.ifg,
            port.loopback,
            port.pvid,
            port.learn
        ),
        (9100, 1, 1, 96, 2, 10, 3)
    );
    assert_eq!(port.interface.as_deref(), Some("caui"));
    for (fec, n) in [(Fec::None, 0), (Fec::Fc, 2), (Fec::Rs, 1)] {
        b.port_fec_set(p, fec).unwrap();
        assert_eq!(r.chip.port(1).unwrap().fec, n);
    }
    for (lb, n) in [(Loopback::None, 0), (Loopback::Mac, 1)] {
        b.port_loopback_set(p, lb).unwrap();
        assert_eq!(r.chip.port(1).unwrap().loopback, n);
    }
    for (mode, n) in [(Learn::Hw, 0), (Learn::Disable, 1), (Learn::Drop, 2)] {
        b.port_learn_set(p, mode).unwrap();
        assert_eq!(r.chip.port(1).unwrap().learn, n);
    }
    // counters by ob::PortStat index
    for (i, stat) in PortStat::ALL.into_iter().enumerate() {
        let glue = sdk_glue::PortStat::from_raw(i32::try_from(i).unwrap()).unwrap();
        assert!(r.chip.set_counter(1, glue, 1000 + i as u64));
        assert_eq!(b.port_stat_get(p, stat), Ok(1000 + i as u64), "{stat:?}");
    }
    b.port_stat_clear(p).unwrap();
    assert_eq!(b.port_stat_get(p, PortStat::OutMulticastPkts), Ok(0));
    b.port_speed_set(p, 40_000).unwrap();
    assert_eq!(b.port_speed_get(p), Ok(40_000));
    assert!(b.port_supported_speeds(p).unwrap().contains(&100_000));
    assert_eq!(b.port_link_get(p), Ok(false));
    // a port the chip does not have
    assert_eq!(b.port_frame_max_set(HwPort(9), 9100), Err(Error::InvalidPortNumber));
    assert_eq!(b.port_link_get(HwPort(9)), Err(Error::InvalidPortNumber));
    // the optional port operations stay at the trait defaults, as in the C++
    assert!(!b.port_stat_by_id(0));
    assert_eq!(b.port_stats_by_id(p, &[]), Err(Error::NotSupported));
    assert_eq!(b.port_pfc_set(p, false, 0, 0), Err(Error::NotSupported));
}

#[test]
fn vlans_l2_and_trunks_reach_the_chip() {
    let mut r = rig("ports 4 4 100000\n", &[]);
    let b = &mut r.backend;
    b.vlan_create(vid(10)).unwrap();
    b.vlan_port_add(vid(10), HwPort(1), true).unwrap();
    b.vlan_port_add(vid(10), HwPort(2), false).unwrap();
    assert_eq!(r.chip.vlan(10), Some(vec![(1, true), (2, false)]));
    assert_eq!(b.vlan_members(vid(10)), Ok(vec![(HwPort(1), true), (HwPort(2), false)]));
    b.vlan_port_remove(vid(10), HwPort(1)).unwrap();
    assert_eq!(r.chip.vlan(10), Some(vec![(2, false)]));
    assert_eq!(b.vlan_create(vid(10)), Err(Error::ItemAlreadyExists));
    b.vlan_destroy(vid(10)).unwrap();
    assert_eq!(r.chip.vlan(10), None);
    assert_eq!(b.vlan_members(vid(10)), Err(Error::ItemNotFound));

    // L2: a port and a LAG destination (OB_LAG_DEST_BIT | tid)
    let tid = b.lag_create().unwrap();
    let mac2 = MacAddr([0x02, 0, 0, 0, 0, 0x22]);
    b.l2_add(MAC, vid(1), Dest::Port(HwPort(3)), true, false).unwrap();
    b.l2_add(mac2, vid(1), Dest::Lag(tid), false, true).unwrap();
    let entries = r.chip.l2_entries();
    assert_eq!(entries.len(), 2);
    let find = |m: MacAddr| entries.iter().find(|(mac, _, _)| *mac == m.0).unwrap().2;
    assert_eq!(
        (find(MAC).dest, find(MAC).is_static, find(MAC).discard),
        (3, true, false)
    );
    assert_eq!(
        (find(mac2).dest, find(mac2).is_static, find(mac2).discard),
        (0x4000_0000 | i32::try_from(tid.0).unwrap(), false, true)
    );
    b.l2_delete(MAC, vid(1)).unwrap();
    assert_eq!(r.chip.l2_entries().len(), 1);
    b.l2_flush(Some(Dest::Lag(tid)), Some(vid(1)), true, true).unwrap();
    assert_eq!(r.chip.l2_entries().len(), 0);
    b.l2_age_set(300).unwrap();
    assert_eq!(r.chip.l2_age(), 300);

    // trunks: flags 1 egress disable, 2 ingress disable; none empties it
    let m = |p, e, i| LagMember {
        hw: HwPort(p),
        egress_disable: e,
        ingress_disable: i,
    };
    b.lag_set_members(
        tid,
        &[
            m(1, false, false),
            m(2, true, false),
            m(3, false, true),
            m(4, true, true),
        ],
    )
    .unwrap();
    assert_eq!(r.chip.trunk(tid.0), Some(vec![(1, 0), (2, 1), (3, 2), (4, 3)]));
    b.lag_set_members(tid, &[]).unwrap();
    assert_eq!(r.chip.trunk(tid.0), Some(vec![]));
    b.lag_destroy(tid).unwrap();
    assert_eq!(r.chip.trunk(tid.0), None);
    assert_eq!(b.lag_destroy(tid), Err(Error::ItemNotFound));
}

#[test]
fn host_interfaces_and_traps_reach_the_chip() {
    let mut r = rig("ports 2 4 100000\n", &[]);
    let b = &mut r.backend;
    let n = b.netif_create("Ethernet0", HwPort(1), vid(1), MAC, false).unwrap();
    assert_eq!((r.chip.counts().netifs, r.chip.counts().filters), (1, 1));
    let f = b
        .netif_set_keep_tag(n.netif_id, HwPort(1), true, Some(n.filter_id))
        .unwrap();
    assert_ne!(f, n.filter_id);
    assert_eq!(r.chip.counts().filters, 1, "the old filter went");
    b.netif_destroy(n.netif_id, f).unwrap();
    assert_eq!((r.chip.counts().netifs, r.chip.counts().filters), (0, 0));
    for kind in [TrapKind::Lldp, TrapKind::Lacp, TrapKind::ArpRequest, TrapKind::Nd] {
        b.trap_set(kind, true).unwrap();
    }
    b.trap_set(TrapKind::Lacp, false).unwrap();
    assert_eq!(r.chip.trap(sdk_glue::TrapKind::Lldp), Some(true));
    assert_eq!(r.chip.trap(sdk_glue::TrapKind::Lacp), Some(false));
    assert_eq!(r.chip.trap(sdk_glue::TrapKind::ArpRequest), Some(true));
    assert_eq!(r.chip.trap(sdk_glue::TrapKind::Nd), Some(true));
    // kinds without a control: OB_E_UNAVAIL
    for kind in [TrapKind::Dhcpv6, TrapKind::Stp, TrapKind::Other] {
        assert_eq!(b.trap_set(kind, true), Err(Error::NotSupported));
    }
    // the C++ backend has no trap_program, no generic netlink
    let spec = TrapSpec {
        trap_type: 0,
        action: 0,
        priority: 0,
        queue: 0,
        group_up: true,
        policer: None,
        yellow_drop: false,
        red_drop: false,
    };
    assert_eq!(b.trap_program(&spec, false), Err(Error::NotSupported));
    assert_eq!(
        b.genetlink_create("psample", "packets").map(drop),
        Err(Error::NotSupported)
    );
}

#[test]
fn the_carrier_goes_to_the_knet_procfs_file() {
    let path = unique("knet-link");
    // also without a chip, as the C++ (no glue call)
    let mut b = OpenbcmBackend::on_chip(Chip::new()).with_knet_link(&path);
    b.netif_carrier_set("Ethernet0", true).unwrap();
    assert_eq!(read(&path), "Ethernet0=up\n");
    b.netif_carrier_set("Ethernet4", false).unwrap();
    assert_eq!(read(&path), "Ethernet4=down\n", "one line per write, as fopen(\"w\")");
    let mut nowhere = OpenbcmBackend::new().with_knet_link(unique("no-such-dir").join("link"));
    assert_eq!(nowhere.netif_carrier_set("Ethernet0", true), Err(Error::Failure));
}

#[test]
fn l3_reaches_the_chip_and_the_shared_egress_objects_stay() {
    let mut r = rig("ports 2 4 100000\n", &[]);
    let b = &mut r.backend;
    let cpu = b.l3_egress_cpu().unwrap();
    let drop = b.l3_egress_drop().unwrap();
    assert_eq!(b.l3_egress_cpu(), Ok(cpu), "cached");
    assert_eq!(
        b.l3_egress_glean(),
        Ok(cpu),
        "glean is the CPU egress (trait default, as the C++)"
    );
    let intf = b.l3_intf_create(MAC, vid(1), 9100, Vrf(0)).unwrap();
    b.l3_intf_update(intf, MAC, vid(1), 1500, Vrf(3)).unwrap();
    let station = b.l3_station_add(MAC).unwrap();
    let e = b.l3_egress_create(intf, MAC, Dest::Port(HwPort(2)), vid(1)).unwrap();
    b.l3_egress_update(e, intf, MAC, Dest::Port(HwPort(1)), vid(1)).unwrap();
    let counts = r.chip.counts();
    assert_eq!((counts.intfs, counts.stations, counts.egress), (1, 1, 3));
    // the shared objects are refused without a glue call
    assert_eq!(b.l3_egress_destroy(cpu), Err(Error::InvalidParameter));
    assert_eq!(b.l3_egress_destroy(drop), Err(Error::InvalidParameter));
    assert!(r.chip.fail("egress-destroy", 1, -11));
    assert_eq!(b.l3_egress_destroy(cpu), Err(Error::InvalidParameter), "no glue call");
    assert_eq!(
        b.l3_egress_destroy(e),
        Err(Error::Failure),
        "the scripted fault was still pending"
    );
    assert_eq!(r.chip.counts().egress, 3);
    // hosts and routes, IPv4 and IPv6
    let v6 = IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1));
    b.l3_host_add(Vrf(0), v4([10, 0, 0, 2]), e).unwrap();
    b.l3_host_add(Vrf(0), v6, e).unwrap();
    assert_eq!(r.chip.counts().hosts, 2);
    b.l3_host_delete(Vrf(0), v6).unwrap();
    let net = prefix([10, 1, 0, 0], [255, 255, 0, 0]);
    let net6 = IpPrefix::new(v6, IpAddr::V6(Ipv6Addr::new(0xffff, 0xffff, 0, 0, 0, 0, 0, 0))).unwrap();
    b.l3_route_add(Vrf(0), net, RouteTarget::Egress(e), false).unwrap();
    assert_eq!(
        b.l3_route_add(Vrf(0), net, RouteTarget::Egress(e), false),
        Err(Error::ItemAlreadyExists)
    );
    b.l3_route_add(Vrf(0), net, RouteTarget::Egress(drop), true).unwrap();
    b.l3_route_add(Vrf(0), net6, RouteTarget::Egress(cpu), false).unwrap();
    let ecmp = b.l3_ecmp_create(&[e, e]).unwrap();
    b.l3_ecmp_update(ecmp, &[e]).unwrap();
    b.l3_route_add(Vrf(0), net, RouteTarget::Ecmp(ecmp), true).unwrap();
    assert_eq!((r.chip.counts().routes, r.chip.counts().ecmp), (2, 1));
    b.l3_route_delete(Vrf(0), net).unwrap();
    b.l3_route_delete(Vrf(0), net6).unwrap();
    b.l3_ecmp_destroy(ecmp).unwrap();
    b.l3_egress_destroy(e).unwrap();
    b.l3_station_delete(station).unwrap();
    b.l3_intf_destroy(intf).unwrap();
    let counts = r.chip.counts();
    assert_eq!(
        (counts.intfs, counts.stations, counts.egress, counts.routes, counts.ecmp),
        (0, 0, 2, 0, 0)
    );
    // the L3 extras of the C++ defaults
    assert!(!b.l3_intf_stat_supported(0));
    assert_eq!(b.l3_intf_admin_set(intf, true, true), Err(Error::NotSupported));
}

#[test]
fn field_groups_entries_and_counters() {
    let mut r = rig("ports 4 4 100000\n", &[]);
    let b = &mut r.backend;
    let qset = [FpQual::InPorts, FpQual::IpType, FpQual::DstCpu];
    let gid = b.fp_group_create(&qset, 7).unwrap();
    assert_eq!(r.chip.counts().fp_groups, 1);
    let rule = FpRule {
        priority: u32::MAX,
        m: FpMatch {
            in_ports: Some(vec![HwPort(2), HwPort(3)]),
            ip_type: Some(i32::try_from(sai_sys::SAI_ACL_IP_TYPE_ARP_REQUEST).unwrap()),
            dst_cpu: true,
            ..FpMatch::default()
        },
        counter: true,
        copy_cpu: true,
        ..FpRule::default()
    };
    let eid = b.fp_entry_create(gid, &rule).unwrap();
    let entry = r.chip.fp_entry(eid.0).unwrap();
    assert_eq!(entry.gid, i32::try_from(gid.0).unwrap());
    let bits = qset.iter().fold(0, |a, q| a | q.bit());
    assert_eq!(entry.present, bits);
    assert_eq!(entry.priority, 0x7fff_ffff, "clamped as the C++");
    assert_eq!(entry.ports, vec![2, 3]);
    assert_eq!(entry.policer, -1);
    assert!(entry.stat >= 0);
    assert!(r.chip.fp_hit(u32::try_from(entry.stat).unwrap(), 3, 300));
    let counter = b.fp_entry_counter(eid).unwrap();
    assert_eq!((counter.packets, counter.bytes), (3, 300));
    // no counter: NotSupported without a glue call; unknown: ItemNotFound
    let plain = b.fp_entry_create(gid, &FpRule::default()).unwrap();
    assert_eq!(b.fp_entry_counter(plain), Err(Error::NotSupported));
    assert_eq!(b.fp_entry_counter(FpEntryId(999_999)), Err(Error::ItemNotFound));
    assert_eq!(b.fp_entry_destroy(FpEntryId(999_999)), Err(Error::ItemNotFound));
    // a qualifier outside the group: the glue's PARAM
    let outside = FpRule {
        m: FpMatch {
            ttl: Some(Masked { value: 1, mask: 0xff }),
            ..FpMatch::default()
        },
        ..FpRule::default()
    };
    assert_eq!(b.fp_entry_create(gid, &outside), Err(Error::InvalidParameter));
    // an IP type the C++ maps to -1 (ANY): refused by the glue as the C++ would hand it over
    let any = FpRule {
        m: FpMatch {
            ip_type: Some(i32::try_from(sai_sys::SAI_ACL_IP_TYPE_ANY).unwrap()),
            ..FpMatch::default()
        },
        ..FpRule::default()
    };
    assert_eq!(b.fp_entry_create(gid, &any), Err(Error::InvalidParameter));
    b.fp_entry_destroy(eid).unwrap();
    b.fp_entry_destroy(plain).unwrap();
    assert_eq!(b.fp_entry_destroy(eid), Err(Error::ItemNotFound));
    assert_eq!((r.chip.counts().fp_entries, r.chip.counts().fp_stats), (0, 0));
    b.fp_group_destroy(gid).unwrap();
    assert_eq!(r.chip.counts().fp_groups, 0);
    // the optional field features stay at the defaults
    assert!(!b.fp_supported(FpCap::Ranges));
    assert_eq!(b.fp_entry_update(eid, &FpRule::default()), Err(Error::NotSupported));
    assert_eq!(b.fp_group_free(gid), Err(Error::NotSupported));
}

fn metered(policer: PolicerId) -> FpRule {
    FpRule {
        policer: Some(policer),
        red_drop: true,
        ..FpRule::default()
    }
}

#[test]
fn policers_and_the_cpp_clone_fallback() {
    for code in [-15, -4] {
        let mut r = rig("ports 4 4 100000\n", &[]);
        let b = &mut r.backend;
        let gid = b.fp_group_create(&[FpQual::InPorts], 1).unwrap();
        let cfg = PolicerCfg {
            cir: 1000,
            cbs: 100,
            ..PolicerCfg::default()
        };
        let pid = b.policer_create(&cfg).unwrap();
        assert_eq!(r.chip.policer(pid.0).unwrap().cir, 1000);
        // users == 0: a refusal is the answer, no copy
        assert!(r.chip.fail("fp-entry-create", 1, code));
        assert_eq!(b.fp_entry_create(gid, &metered(pid)), Err(mapped(code)));
        assert_eq!(r.chip.counts().policers, 1);
        let first = b.fp_entry_create(gid, &metered(pid)).unwrap();
        assert_eq!(r.chip.fp_entry(first.0).unwrap().policer, i32::try_from(pid.0).unwrap());
        assert_eq!(b.policer_destroy(pid), Err(Error::ObjectInUse));
        // users > 0: the SDK refuses to share it; the entry meters with a copy
        assert!(r.chip.fail("fp-entry-create", 1, code));
        let second = b.fp_entry_create(gid, &metered(pid)).unwrap();
        assert_eq!(r.chip.counts().policers, 2);
        let copy = r.chip.fp_entry(second.0).unwrap().policer;
        assert_ne!(copy, i32::try_from(pid.0).unwrap());
        let copy = u32::try_from(copy).unwrap();
        assert_eq!(r.chip.policer(copy).unwrap().cir, 1000);
        // a set changes the policer and every copy
        let faster = PolicerCfg { cir: 5000, ..cfg };
        b.policer_set(pid, &faster).unwrap();
        assert_eq!(r.chip.policer(pid.0).unwrap().cir, 5000);
        assert_eq!(r.chip.policer(copy).unwrap().cir, 5000);
        // a copy made later has the new rates
        assert!(r.chip.fail("fp-entry-create", 1, code));
        let third = b.fp_entry_create(gid, &metered(pid)).unwrap();
        let copy3 = u32::try_from(r.chip.fp_entry(third.0).unwrap().policer).unwrap();
        assert_eq!(r.chip.policer(copy3).unwrap().cir, 5000);
        // the retry fails too: the copy goes, the retry's answer is the answer
        assert!(r.chip.fail("fp-entry-create", 1, code));
        assert!(r.chip.fail("fp-entry-create", 2, -6));
        assert_eq!(b.fp_entry_create(gid, &metered(pid)), Err(Error::TableFull));
        assert_eq!(r.chip.counts().policers, 3);
        // no copy when making the copy fails: the first answer
        assert!(r.chip.fail("fp-entry-create", 1, code));
        assert!(r.chip.fail("policer-create", 1, -14));
        assert_eq!(b.fp_entry_create(gid, &metered(pid)), Err(mapped(code)));
        assert_eq!(r.chip.counts().policers, 3);
        // other codes are no reason for a copy
        assert!(r.chip.fail("fp-entry-create", 1, -6));
        assert_eq!(b.fp_entry_create(gid, &metered(pid)), Err(Error::TableFull));
        assert_eq!(r.chip.counts().policers, 3);
        // destroying an entry destroys its copy; the policer is free after the last one
        b.fp_entry_destroy(second).unwrap();
        assert!(r.chip.policer(copy).is_none());
        assert_eq!(r.chip.counts().policers, 2);
        b.fp_entry_destroy(third).unwrap();
        assert_eq!(b.policer_destroy(pid), Err(Error::ObjectInUse));
        // a refused destroy still forgets the entry and its use (C++ order)
        assert!(r.chip.fail("fp-entry-destroy", 1, -11));
        assert_eq!(b.fp_entry_destroy(first), Err(Error::Failure));
        assert_eq!(b.fp_entry_destroy(first), Err(Error::ItemNotFound));
        // ... while the chip still has it, metering with the policer: the SDK refuses (BUSY)
        assert!(r.chip.fp_entry(first.0).is_some());
        assert_eq!(b.policer_destroy(pid), Err(Error::ObjectInUse));
        assert!(r.chip.policer(pid.0).is_some());
    }
    // a policer without entries goes
    let mut r = rig("ports 1 4 100000\n", &[]);
    let pid = r.backend.policer_create(&PolicerCfg::default()).unwrap();
    r.backend.policer_destroy(pid).unwrap();
    assert!(r.chip.policer(pid.0).is_none());
    assert_eq!(r.backend.policer_destroy(pid), Err(Error::ItemNotFound));
}

#[test]
fn a_failing_copy_set_stops_the_policer_set() {
    let mut r = rig("ports 4 4 100000\n", &[]);
    let b = &mut r.backend;
    let gid = b.fp_group_create(&[FpQual::InPorts], 1).unwrap();
    let pid = b.policer_create(&PolicerCfg::default()).unwrap();
    b.fp_entry_create(gid, &metered(pid)).unwrap();
    assert!(r.chip.fail("fp-entry-create", 1, -15));
    let second = b.fp_entry_create(gid, &metered(pid)).unwrap();
    let copy = u32::try_from(r.chip.fp_entry(second.0).unwrap().policer).unwrap();
    assert!(r.chip.fail("policer-set", 2, -7));
    let cfg = PolicerCfg {
        cir: 77,
        ..PolicerCfg::default()
    };
    assert_eq!(b.policer_set(pid, &cfg), Err(Error::ItemNotFound));
    assert_eq!(r.chip.policer(pid.0).unwrap().cir, 77);
    assert_eq!(r.chip.policer(copy).unwrap().cir, 0);
}

#[test]
fn link_and_l2_events_reach_the_sink() {
    let r = rig("ports 4 4 100000\n", &[]);
    let Rig { chip, events, .. } = &r;
    assert!(chip.link_event(3, true));
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(3),
            up: true
        })
    );
    // from another thread, as the SDK's link scan
    let c = chip.clone();
    std::thread::spawn(move || assert!(c.link_event(3, false)))
        .join()
        .unwrap();
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(3),
            up: false
        })
    );
    let mac = [0x02, 1, 2, 3, 4, 5];
    assert!(chip.l2_event(Some(mac), 10, 2, 1));
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Fdb(FdbEvent {
            change: FdbChange::Learned,
            mac: MacAddr(mac),
            vid: vid(10),
            hw_port: Dest::Port(HwPort(2)),
        }))
    );
    assert!(chip.l2_event(Some(mac), 4094, 0x4000_0005, 0));
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Fdb(FdbEvent {
            change: FdbChange::Aged,
            mac: MacAddr(mac),
            vid: vid(4094),
            hw_port: Dest::Lag(TrunkId(5)),
        }))
    );
    // out of range: dropped by the trampoline (and a port above 16 bits by the backend)
    chip.l2_event(Some(mac), 0, 2, 1);
    chip.l2_event(None, 10, 2, 1);
    chip.l2_event(Some(mac), 10, 70_000, 1);
    assert_eq!(events.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn a_link_gate_is_asked_by_the_link_scan() {
    let mut r = rig("ports 4 4 100000\nlinkscan 4 hw\n", &[]);
    let verdict = Arc::new(Mutex::new(GateVerdict::Down));
    let v = Arc::clone(&verdict);
    let gate: LinkGate = Arc::new(move || *v.lock().unwrap());
    r.backend.port_link_gate(HwPort(1), Some(Arc::clone(&gate))).unwrap();
    assert_eq!(r.chip.gate(1), Some(0), "down");
    *verdict.lock().unwrap() = GateVerdict::NoOpinion;
    assert_eq!(r.chip.gate(1), Some(-1), "no opinion");
    // the cable comes up; the scan reads the ASIC's link while the gate has no opinion
    assert!(!r.chip.link_event(1, true), "a gated port's link is the scan's");
    assert_eq!(r.chip.linkscan(), 1);
    assert_eq!(
        r.events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(1),
            up: true
        })
    );
    *verdict.lock().unwrap() = GateVerdict::Down;
    assert_eq!(r.chip.linkscan(), 1);
    assert_eq!(
        r.events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(1),
            up: false
        })
    );
    r.backend.port_link_gate(HwPort(1), None).unwrap();
    assert_eq!(r.chip.gate(1), None);
    r.backend.port_link_gate(HwPort(2), None).unwrap(); // none there: the glue says fine
    // a port in hardware link scan cannot have a gate (OB_E_CONFIG)
    assert_eq!(
        r.backend.port_link_gate(HwPort(4), Some(gate)),
        Err(Error::InvalidParameter)
    );
    assert_eq!(r.chip.gate(4), None);
}

#[test]
fn temperatures_and_the_publishers_reader() {
    let mut r = rig("ports 1 4 100000\ntemp 450 471\ntemp-fail 4\n", &[]);
    let t = r.backend.asic_temperatures().unwrap();
    assert_eq!((t.cur, t.peak), (vec![450, 471], vec![465, 486]));
    let reader = r.backend.temperature_reader().unwrap();
    let t = std::thread::spawn(move || reader()).join().unwrap().unwrap();
    assert_eq!(t.cur, vec![450, 471]);
    let reader = r.backend.temperature_reader().unwrap();
    assert_eq!(reader().map(drop), Ok(()));
    // the 4th read fails (OB_E_TIMEOUT)
    assert_eq!(r.backend.asic_temperatures(), Err(Error::Failure));
    r.backend.shutdown();
    assert_eq!(reader().map(drop), Err(Error::Uninitialized), "a dead handle");
    // no monitors: OB_E_UNAVAIL
    let r = rig("ports 1 4 100000\n", &[]);
    assert_eq!(r.backend.asic_temperatures(), Err(Error::NotSupported));
}

#[test]
fn mdio_through_the_backend_and_its_bus_handle() {
    let mut r = rig("ports 1 4 100000\nreg 21 1 2 1234\ndead 22\n", &[]);
    let dev = MdioDev::new(1).unwrap();
    assert_eq!(r.backend.mdio_c45_read(PhyAddr(0x21), dev, 2), Ok(0x1234));
    r.backend.mdio_c45_write(PhyAddr(0x21), dev, 3, 0xbeef).unwrap();
    assert_eq!(r.chip.mdio_reg(0x21, 1, 3), Some(0xbeef));
    assert_eq!(r.backend.mdio_c45_read(PhyAddr(0x22), dev, 2), Err(Error::Failure)); // OB_E_TIMEOUT
    assert_eq!(
        r.backend.mdio_c45_read(PhyAddr(0x800), dev, 2),
        Err(Error::InvalidParameter)
    );
    let bus = r.backend.mdio_bus().unwrap();
    let on_thread = Arc::clone(&bus);
    std::thread::spawn(move || on_thread.write(PhyAddr(0x21), dev, 4, 7).unwrap())
        .join()
        .unwrap();
    assert_eq!(bus.read(PhyAddr(0x21), dev, 4), Ok(7));
    assert_eq!(r.chip.mdio_reg(0x21, 1, 4), Some(7));
    r.backend.shutdown();
    assert_eq!(bus.read(PhyAddr(0x21), dev, 4), Err(Error::Uninitialized));
    assert_eq!(bus.write(PhyAddr(0x21), dev, 4, 8), Err(Error::Uninitialized));
    assert!(r.backend.mdio_bus().is_none());
}

#[test]
fn shell_base_and_the_defaults() {
    let mut r = rig("ports 2 4 100000\n", &[]);
    let b = &mut r.backend;
    b.run_shell("ps").unwrap();
    let out_path = unique("shell-out.txt");
    let out = std::fs::File::create(&out_path).unwrap();
    b.run_shell_to("show c", std::os::fd::AsFd::as_fd(&out)).unwrap();
    drop(out);
    assert_eq!(r.chip.shell_log(), vec!["ps".to_owned(), "show c".to_owned()]);
    assert_eq!(read(&out_path), "fake: show c\n");
    assert_eq!(b.run_shell("bad\0cmd"), Err(Error::InvalidParameter));
    let usage = b.table_usage().unwrap();
    assert_eq!((usage.v4_routes_free, usage.fdb_free), (16384, 32768));
    let limits = b.chip_limits().unwrap();
    assert_eq!(
        (
            limits.buffer_bytes,
            limits.acl_prio_max,
            limits.vrf_max,
            limits.ecmp_paths
        ),
        (16_777_216, 0x7fff_ffff, 1023, 64)
    );
    // what openbcm_backend.cpp does not override
    assert!(!b.is_fake());
    assert!(!b.qos_supported());
    assert!(!b.mirror_supported());
    assert!(!b.hash_supported());
    assert_eq!(b.hash_set(true, &[], 0), Err(Error::NotSupported));
    assert_eq!(b.port_queues(), 0);
    assert!(!b.programs(sai_sys::SAI_OBJECT_TYPE_PORT));
}

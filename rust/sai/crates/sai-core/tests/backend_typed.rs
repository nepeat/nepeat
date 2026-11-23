//! The typed backend interface: the virtual chip's parity with the C++ fake backend
//! (`fake_backend.cpp`), the recording decorator's line format (`call-log-format.md` v1), fault
//! injection by operation name, and that `Box<dyn Backend>` forwards the typed operations.
#![allow(missing_docs, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod common;

use common::{PORT, SWITCH, init_attrs, oid_list};
use sai_core::backend::types::{
    Dest, EgressId, FdbChange, FdbEvent, FpCounter, FpEntryId, FpGroupExtra, FpMatch, FpQual, FpRule, GateVerdict,
    IpPrefix, LagMember, Loopback, MacAddr, Masked, MdioDev, MirrorCfg, PhyAddr, PolicerCfg, PortInterface, PortStat,
    RouteTarget, SchedCfg, StormKind, TrapKind, TrunkId, Vrf,
};
use sai_core::backend::{
    Backend, BackendEvent, Base, EventSink, Field, Hash, Hostif, HwPort, L2, L3, Lag, Mdio, Mirror, Mode, Policer,
    PortOps, Profile, Qos, Recording, Shell, Storm, Temp, Vid,
};
use sai_core::error::Error;
use sai_core::oid::TypedId;
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::{Compat, EgressKind, VirtualBackend};
use sai_sys as sys;
use std::io::Read as _;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

fn up(pairs: &[(&str, &str)]) -> (VirtualBackend, mpsc::Receiver<BackendEvent>) {
    let mut chip = VirtualBackend::new();
    let (sink, events) = EventSink::channel();
    chip.init(&Profile::from_pairs(pairs.iter().copied()), sink).unwrap();
    (chip, events)
}

fn vid(n: u16) -> Vid {
    Vid::new(n).unwrap()
}

fn mac(last: u8) -> MacAddr {
    MacAddr([0, 0x11, 0x22, 0x33, 0x44, last])
}

#[test]
fn the_cpp_profile_keys_and_numbering() {
    let (chip, _) = up(&[("SAI_OPENBCM_FAKE_PORTS", "3"), ("SAI_OPENBCM_FAKE_LANES", "2")]);
    let hw: Vec<u16> = chip.ports().iter().map(|p| p.hw.0).collect();
    assert_eq!(hw, [1, 2, 3], "C++ names, Rust numbering by default");
    let (chip, _) = up(&[
        ("SAI_OPENBCM_FAKE_PORTS", "3"),
        ("SAI_OPENBCM_FAKE_LANES", "4"),
        ("SAI_RS_VIRTUAL_COMPAT", "cpp"),
    ]);
    assert_eq!(chip.compat(), Compat::Cpp);
    let ports = chip.ports();
    assert_eq!(
        ports.iter().map(|p| p.hw.0).collect::<Vec<_>>(),
        [1, 5, 9],
        "the SDK's gaps"
    );
    assert_eq!(ports[1].lanes, [5, 6, 7, 8]);
    assert_eq!(chip.port_supported_speeds(HwPort(5)), Ok(vec![40_000, 100_000]));
    assert_eq!(chip.port_state(HwPort(9)).unwrap().name, "ce2");
    // the Rust name wins over the C++ one
    let (chip, _) = up(&[("SAI_OPENBCM_FAKE_PORTS", "3"), ("SAI_RS_VIRTUAL_PORTS", "2")]);
    assert_eq!(chip.ports().len(), 2);
    // strict as before: a bad value is refused under either name
    let mut chip = VirtualBackend::new();
    let (sink, _rx) = EventSink::channel();
    let bad = Profile::from_pairs([("SAI_OPENBCM_FAKE_LINK", "sometimes")]);
    assert_eq!(chip.init(&bad, sink), Err(Error::InvalidParameter));
}

#[test]
fn link_follows_admin_loopback_and_the_gate() {
    let (mut chip, events) = up(&[("SAI_RS_VIRTUAL_PORTS", "2"), ("SAI_RS_VIRTUAL_LINK", "none")]);
    let p = HwPort(1);
    chip.port_admin_set(p, true).unwrap();
    assert_eq!(chip.port_link_get(p), Ok(false), "LINK=none: no link");
    chip.port_loopback_set(p, Loopback::Mac).unwrap();
    assert_eq!(chip.port_link_get(p), Ok(true), "loopback brings it up");
    assert_eq!(events.try_recv(), Ok(BackendEvent::Link { hw: p, up: true }));
    // a gate that says down vetoes the link on the next scan
    let open = Arc::new(AtomicBool::new(false));
    let verdict = Arc::clone(&open);
    let gate = Arc::new(move || {
        if verdict.load(Ordering::Acquire) {
            GateVerdict::NoOpinion
        } else {
            GateVerdict::Down
        }
    });
    chip.port_link_gate(p, Some(gate)).unwrap();
    assert_eq!(chip.gated_ports(), [p]);
    chip.linkscan();
    assert_eq!(chip.port_link_get(p), Ok(false));
    assert_eq!(events.try_recv(), Ok(BackendEvent::Link { hw: p, up: false }));
    open.store(true, Ordering::Release);
    chip.linkscan();
    assert_eq!(chip.port_link_get(p), Ok(true), "no opinion: the ASIC decides");
    // an empty gate removes it
    chip.port_link_gate(p, None).unwrap();
    assert_eq!(chip.gated_ports(), []);
    assert_eq!(chip.port_link_gate(HwPort(99), None), Err(Error::InvalidParameter));
}

#[test]
fn port_settings_as_the_fake_keeps_them() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2"), ("SAI_RS_VIRTUAL_LANES", "2")]);
    let p = HwPort(2);
    assert_eq!(chip.port_frame_max_set(p, 63), Err(Error::InvalidParameter));
    chip.port_frame_max_set(p, 9416).unwrap();
    assert_eq!(
        chip.port_interface_set(p, PortInterface::Cr4),
        Err(Error::InvalidParameter),
        "a 4-lane interface needs 4 lanes"
    );
    chip.port_interface_set(p, PortInterface::Cr).unwrap();
    chip.port_ifg_set(p, 96).unwrap();
    chip.port_pvid_set(p, vid(10)).unwrap();
    let state = chip.port_state(p).unwrap();
    assert_eq!(
        (state.frame_max, state.iface, state.ifg, state.quirk_calls, state.pvid),
        (9416, Some(PortInterface::Cr), 96, 2, vid(10))
    );
    chip.set_stat(p, PortStat::InOctets, 1500);
    assert_eq!(chip.port_stat_get(p, PortStat::InOctets), Ok(1500));
    chip.port_stat_clear(p).unwrap();
    assert_eq!(chip.port_stat_get(p, PortStat::InOctets), Ok(0));
    assert!(
        !chip.port_stat_by_id(sys::SAI_PORT_STAT_IF_IN_OCTETS),
        "the fake leaves it at the default"
    );
}

#[test]
fn lags_l2_and_fdb_events() {
    let (mut chip, events) = up(&[("SAI_RS_VIRTUAL_PORTS", "4")]);
    let tid = chip.lag_create().unwrap();
    assert_eq!(tid, TrunkId(1));
    let m = |hw| LagMember {
        hw: HwPort(hw),
        egress_disable: false,
        ingress_disable: false,
    };
    chip.lag_set_members(tid, &[m(1), m(2)]).unwrap();
    let other = chip.lag_create().unwrap();
    assert_eq!(
        chip.lag_set_members(other, &[m(2)]),
        Err(Error::InvalidParameter),
        "one trunk per port"
    );
    assert_eq!(chip.lag_set_members(TrunkId(9), &[]), Err(Error::ItemNotFound));
    let lag = Dest::Lag(tid);
    chip.l2_add(mac(1), Vid::DEFAULT, lag, true, false).unwrap();
    chip.l2_add(mac(2), Vid::DEFAULT, Dest::Port(HwPort(3)), false, false)
        .unwrap();
    assert_eq!(
        chip.l2_add(mac(3), vid(7), lag, false, false),
        Err(Error::InvalidParameter),
        "no such VLAN"
    );
    chip.l2_flush(None, None, false, true).unwrap();
    assert_eq!(chip.l2_table().len(), 1, "the dynamic entry went, the static stayed");
    chip.l2_age_set(60).unwrap();
    assert_eq!(chip.age_seconds(), 60);
    let event = FdbEvent {
        change: FdbChange::Learned,
        mac: mac(9),
        vid: Vid::DEFAULT,
        hw_port: Dest::Port(HwPort(4)),
    };
    chip.inject_fdb(event);
    assert_eq!(events.try_recv(), Ok(BackendEvent::Fdb(event)));
    assert!(chip.l2_table().contains_key(&(mac(9), Vid::DEFAULT)));
    chip.inject_fdb(FdbEvent {
        change: FdbChange::Aged,
        ..event
    });
    assert!(!chip.l2_table().contains_key(&(mac(9), Vid::DEFAULT)));
    chip.lag_destroy(tid).unwrap();
}

fn prefix(a: [u8; 4], len: u32) -> IpPrefix {
    let mask = u32::MAX.checked_shl(32 - len).unwrap_or(0);
    IpPrefix::new(IpAddr::V4(Ipv4Addr::from(a)), IpAddr::V4(Ipv4Addr::from_bits(mask))).unwrap()
}

#[test]
fn l3_tables_ids_and_references() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    chip.vlan_create(vid(4094)).unwrap();
    let intf = chip.l3_intf_create(mac(0), vid(4094), 9100, Vrf::DEFAULT).unwrap();
    assert_eq!(
        chip.l3_intf_create(mac(0), vid(5), 9100, Vrf::DEFAULT),
        Err(Error::InvalidParameter)
    );
    let e1 = chip
        .l3_egress_create(intf, mac(1), Dest::Port(HwPort(1)), vid(4094))
        .unwrap();
    let e2 = chip
        .l3_egress_create(intf, mac(2), Dest::Port(HwPort(2)), vid(4094))
        .unwrap();
    assert_eq!(
        (e1, e2),
        (EgressId(100_002), EgressId(100_003)),
        "the SDK's egress id range"
    );
    let cpu = chip.l3_egress_cpu().unwrap();
    assert_eq!(chip.l3_egress_cpu(), Ok(cpu), "shared");
    assert_eq!(chip.l3_egress_glean(), Ok(cpu), "glean is the CPU egress by default");
    assert_eq!(chip.l3().egress.get(&cpu), Some(&EgressKind::Cpu));
    let ecmp = chip.l3_ecmp_create(&[e1, e2, e2]).unwrap();
    assert_eq!(ecmp.0, 200_000);
    assert_eq!(
        chip.l3_ecmp_create(&[cpu]),
        Err(Error::InvalidParameter),
        "only forwarding members"
    );
    let p = prefix([10, 1, 0, 0], 16);
    chip.l3_route_add(Vrf::DEFAULT, p, RouteTarget::Ecmp(ecmp), false)
        .unwrap();
    assert_eq!(
        chip.l3_route_add(Vrf::DEFAULT, p, RouteTarget::Egress(e1), false),
        Err(Error::ItemAlreadyExists)
    );
    chip.l3_route_add(Vrf::DEFAULT, p, RouteTarget::Egress(e1), true)
        .unwrap();
    chip.l3_host_add(Vrf(1), IpAddr::V4(Ipv4Addr::new(10, 1, 0, 2)), e2)
        .unwrap();
    assert_eq!(
        chip.l3_host_add(Vrf(1024), IpAddr::V4(Ipv4Addr::LOCALHOST), e2),
        Err(Error::InvalidParameter)
    );
    assert_eq!(chip.l3_egress_destroy(e1), Err(Error::ObjectInUse), "a route uses it");
    assert_eq!(chip.l3_egress_destroy(cpu), Err(Error::InvalidParameter));
    assert_eq!(chip.l3_intf_destroy(intf), Err(Error::ObjectInUse));
    let usage = chip.table_usage().unwrap();
    assert_eq!(
        (usage.v4_routes_free, usage.hosts_free, usage.ecmp_members_free),
        (16383, 16383, 16381)
    );
    assert_eq!(chip.chip_limits().unwrap().vrf_max, 1023);
}

#[test]
fn field_processor_policers_and_counters() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    let gid = chip.fp_group_create(&[FpQual::InPorts, FpQual::EtherType], 10).unwrap();
    let pid = chip
        .policer_create(&PolicerCfg {
            cir: 100,
            ..PolicerCfg::default()
        })
        .unwrap();
    assert_eq!(
        chip.policer_create(&PolicerCfg::default()),
        Err(Error::InvalidParameter),
        "passes nothing"
    );
    let rule = FpRule {
        m: FpMatch {
            in_ports: Some(vec![HwPort(1)]),
            ether_type: Some(Masked {
                value: 0x806,
                mask: 0xffff,
            }),
            ..FpMatch::default()
        },
        counter: true,
        policer: Some(pid),
        ..FpRule::default()
    };
    let eid = chip.fp_entry_create(gid, &rule).unwrap();
    let mut outside = rule.clone();
    outside.m.ttl = Some(Masked { value: 1, mask: 0xff });
    assert_eq!(
        chip.fp_entry_create(gid, &outside),
        Err(Error::NotSupported),
        "qualifier not in the group"
    );
    chip.fp_hit(eid, 3, 192);
    assert_eq!(chip.fp_entry_counter(eid), Ok(FpCounter { packets: 3, bytes: 192 }));
    assert_eq!(chip.policer_destroy(pid), Err(Error::ObjectInUse));
    assert_eq!(chip.fp_group_destroy(gid), Err(Error::ObjectInUse));
    assert_eq!(
        chip.fp_group_create_ex(
            &[FpQual::DstIp],
            1,
            FpGroupExtra {
                set_tc: true,
                ..FpGroupExtra::default()
            }
        ),
        Err(Error::NotSupported),
        "the default refuses extras"
    );
    chip.fp_entry_destroy(eid).unwrap();
    assert_eq!(chip.fp_entry_counter(FpEntryId(eid.0)), Err(Error::ItemNotFound));
}

#[test]
fn the_optional_models_follow_their_toggles() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    assert!(!chip.qos_supported() && !chip.mirror_supported() && !chip.hash_supported());
    assert_eq!(chip.hash_set(true, &[1, 2], 7), Err(Error::NotSupported));
    assert_eq!(chip.genetlink_create("psample", "packets"), Err(Error::NotSupported));
    chip.toggles_mut().l2x_on = true;
    chip.toggles_mut().mirror_on = true;
    chip.toggles_mut().qos_on = true;
    assert!(chip.qos_supported() && chip.mirror_supported() && chip.hash_supported());
    chip.hash_set(true, &[1, 2], 7).unwrap();
    assert_eq!(chip.l2x().hash.get(&true), Some(&(vec![1, 2], 7)));
    assert_eq!(
        chip.port_storm_set(
            HwPort(1),
            StormKind::Broadcast,
            Some(sai_core::backend::types::PolicerId(5))
        ),
        Err(Error::ItemNotFound)
    );
    let genl = chip.genetlink_create("psample", "packets").unwrap();
    let mirror = chip.mirror_create(&MirrorCfg::new(HwPort(2))).unwrap();
    assert_eq!(
        mirror.0,
        genl.0 + 1,
        "one id counter for QoS / mirror objects, as the C++"
    );
    chip.port_mirror_set(HwPort(1), true, &[mirror]).unwrap();
    assert_eq!(chip.mirror_destroy(mirror), Err(Error::ObjectInUse));
    chip.queue_scheduler_set(HwPort(1), 3, Some(&SchedCfg::default()))
        .unwrap();
    assert_eq!(
        chip.queue_scheduler_set(HwPort(1), 8, None),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        chip.queue_stats_get(HwPort(1), 3, &[sys::SAI_QUEUE_STAT_PACKETS]),
        Ok(vec![0])
    );
    assert_eq!(
        chip.queue_stats_get(HwPort(1), 3, &[sys::SAI_QUEUE_STAT_WATERMARK_BYTES]),
        Err(Error::NotSupported)
    );
    assert_eq!(
        chip.pool_stats_get(true, &[]),
        Err(Error::NotSupported),
        "the fake leaves it at the default"
    );
}

#[test]
fn hostif_traps_mdio_shell_temperatures() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    let made = chip
        .netif_create("Ethernet0", HwPort(1), Vid::DEFAULT, mac(0), false)
        .unwrap();
    assert_eq!((made.netif_id.0, made.filter_id.0), (1, 2), "the C++ fake's first ids");
    assert_eq!(
        chip.netif_create("Ethernet0", HwPort(2), Vid::DEFAULT, mac(0), false)
            .map(|_| ()),
        Err(Error::ItemAlreadyExists)
    );
    chip.netif_carrier_set("Ethernet0", true).unwrap();
    let filter = chip.netif_set_keep_tag(made.netif_id, HwPort(1), true).unwrap();
    assert_eq!(filter.0, 3);
    assert_eq!(
        chip.netif_set_keep_tag(made.netif_id, HwPort(2), true),
        Err(Error::ItemNotFound),
        "wrong port"
    );
    let netif = &chip.knet_netifs()[&made.netif_id];
    assert!(netif.carrier && netif.keep_tag && !netif.has_kernel_netdev());
    chip.trap_set(TrapKind::Lldp, true).unwrap();
    assert_eq!(chip.trap_set(TrapKind::Other, true), Err(Error::NotSupported));
    chip.netif_destroy(made.netif_id, filter).unwrap();

    let dev = MdioDev::new(1).unwrap();
    assert!(
        MdioDev::new(0).is_none() && MdioDev::new(32).is_none(),
        "clause 22 is not expressible"
    );
    chip.mdio_c45_write(PhyAddr(0x10), dev, 0x9000, 0xbeef).unwrap();
    assert_eq!(chip.mdio_c45_read(PhyAddr(0x10), dev, 0x9000), Ok(0xbeef));
    assert_eq!(
        chip.mdio_c45_read(PhyAddr(0x10), dev, 0x9001),
        Ok(0),
        "unset registers read 0"
    );
    chip.mdio().lock().unwrap().dead.insert(PhyAddr(0x11));
    assert_eq!(chip.mdio_c45_read(PhyAddr(0x11), dev, 0), Err(Error::Failure));
    assert_eq!(chip.mdio().lock().unwrap().log.len(), 3);

    chip.run_shell("ps").unwrap();
    let (mut ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
    chip.run_shell_to("l2 show", std::os::fd::AsFd::as_fd(&theirs)).unwrap();
    drop(theirs);
    let mut text = String::new();
    ours.read_to_string(&mut text).unwrap();
    assert_eq!(text, "fake: l2 show\n");
    assert_eq!(chip.shell_log(), ["ps", "l2 show"]);

    let temps = chip.asic_temperatures().unwrap();
    assert_eq!(temps.cur, [400, 415, 430, 445, 460, 475, 490, 505]);
    assert_eq!(temps.peak[0], 405);
    chip.set_temp_fail(true);
    assert_eq!(chip.asic_temperatures(), Err(Error::Failure));
    assert!(chip.is_fake());
}

#[test]
fn faults_by_operation_name() {
    let (mut chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    chip.fail_nth("lag_create", 2, Error::TableFull).unwrap();
    assert!(chip.lag_create().is_ok());
    assert_eq!(chip.lag_create(), Err(Error::TableFull), "the second call");
    assert!(chip.lag_create().is_ok(), "once");
    chip.fail_nth("port_link_get", 1, Error::Failure).unwrap();
    assert_eq!(
        chip.port_link_get(HwPort(1)),
        Err(Error::Failure),
        "read-only operations too"
    );
    assert_eq!(
        chip.fail_nth("no_such_op", 1, Error::Failure),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        chip.fail_nth("lag_create", 0, Error::Failure),
        Err(Error::InvalidParameter)
    );
}

#[test]
fn a_boxed_backend_forwards_the_typed_operations() {
    let (chip, _) = up(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    let mut boxed: Box<dyn Backend> = Box::new(chip);
    assert!(boxed.is_fake());
    assert_eq!(boxed.lag_create(), Ok(TrunkId(1)));
    assert_eq!(boxed.hardware_info(), "fake");
}

fn recorded(
    pairs: &[(&str, &str)],
) -> (
    Sai<Recording<VirtualBackend>>,
    sai_core::backend::CallLog,
    sai_core::oid::RawOid,
) {
    let rec = Recording::new(VirtualBackend::new(), Mode::Normal);
    let log = rec.log();
    let mut sai = Sai::new(rec);
    let switch = sai
        .create_switch(&init_attrs(), &Profile::from_pairs(pairs.iter().copied()))
        .unwrap()
        .raw();
    (sai, log, switch)
}

#[test]
fn the_call_log_of_switch_creation_and_a_port() {
    let (mut sai, log, switch) = recorded(&[("SAI_RS_VIRTUAL_PORTS", "2")]);
    assert_eq!(
        log.take(),
        [
            r#"init(profile={SAI_RS_VIRTUAL_PORTS="2"}) -> SUCCESS"#,
            "ports() -> [{hw=1,lanes=[1,2,3,4],speed=100000},{hw=2,lanes=[5,6,7,8],speed=100000}]",
            "port_admin_set(hw=1, up=false) -> SUCCESS",
            "port_admin_set(hw=2, up=false) -> SUCCESS",
            "vlan_members(vid=1) -> SUCCESS out=[(1,true),(2,true)]",
        ]
    );
    let ports = oid_list(
        sai.get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
            .unwrap()
            .remove(0),
    );
    sai.set(
        PORT,
        ports[0],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    assert_eq!(
        sai.get(PORT, ports[0], &[sys::SAI_PORT_ATTR_OPER_STATUS]).unwrap(),
        [Value::Enum(sys::SAI_PORT_OPER_STATUS_UP.cast_signed())]
    );
    assert_eq!(
        sai.set(PORT, ports[0], &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(12345))),
        Err(Error::attr(sai_core::error::AttrError::InvalidValue, 0))
    );
    assert_eq!(
        log.take(),
        [
            // the event is written where it happens: inside the call that caused it
            "event link(hw=1, up=true)",
            "port_admin_set(hw=1, up=true) -> SUCCESS",
            "port_link_get(hw=1) -> SUCCESS up=true",
            "port_supported_speeds(hw=1) -> SUCCESS out=[10000,25000,40000,50000,100000]",
        ]
    );
    sai.remove(SWITCH, switch).unwrap();
    assert_eq!(log.take(), ["shutdown() -> void"]);
}

#[test]
fn the_call_log_format_examples() {
    let (mut sai, log, _) = recorded(&[("SAI_RS_VIRTUAL_PORTS", "8")]);
    log.take();
    let chip = sai.backend_mut();
    chip.vlan_create(vid(10)).unwrap();
    chip.l2_add(
        MacAddr([0, 0x11, 0x22, 0x33, 0x44, 0x55]),
        vid(10),
        Dest::Port(HwPort(2)),
        true,
        false,
    )
    .unwrap();
    chip.vlan_create(vid(4094)).unwrap();
    let intf = chip
        .l3_intf_create(MacAddr([2, 0, 0, 0, 0, 1]), vid(4094), 9100, Vrf::DEFAULT)
        .unwrap();
    let e = chip
        .l3_egress_create(
            intf,
            MacAddr([0, 0x11, 0x22, 0x33, 0x44, 0x55]),
            Dest::Port(HwPort(2)),
            vid(4094),
        )
        .unwrap();
    let ecmp = chip.l3_ecmp_create(&[e]).unwrap();
    chip.l3_route_add(Vrf::DEFAULT, prefix([10, 1, 0, 0], 16), RouteTarget::Ecmp(ecmp), false)
        .unwrap();
    chip.port_link_get(HwPort(7)).unwrap();
    let gid = chip.fp_group_create(&[FpQual::InPorts], 1).unwrap();
    let eid = chip
        .fp_entry_create(
            gid,
            &FpRule {
                counter: true,
                ..FpRule::default()
            },
        )
        .unwrap();
    chip.fp_entry_counter(eid).unwrap();
    chip.lag_create().unwrap();
    assert!(chip.port_speed_set(HwPort(1), 12345).is_err());
    chip.queue_scheduler_set(HwPort(1), 3, None).unwrap();
    chip.l2_flush(None, Some(vid(10)), true, true).unwrap();
    chip.port_link_gate(HwPort(1), None).unwrap();
    chip.netif_create("Ethernet0", HwPort(1), Vid::DEFAULT, MacAddr([2, 0, 0, 0, 0, 1]), false)
        .unwrap();
    assert!(chip.is_fake(), "a query: not logged in normal mode");
    assert_eq!(
        log.take(),
        [
            "vlan_create(vid=10) -> SUCCESS",
            "l2_add(mac=00:11:22:33:44:55, vid=10, hw=2, is_static=true, discard=false) -> SUCCESS",
            "vlan_create(vid=4094) -> SUCCESS",
            "l3_intf_create(mac=02:00:00:00:00:01, vid=4094, mtu=9100, vrf=0) -> SUCCESS intf_id=1",
            "l3_egress_create(intf_id=1, dmac=00:11:22:33:44:55, hw=2, vid=4094) -> SUCCESS egress_id=100002",
            "l3_ecmp_create(egress=[100002]) -> SUCCESS ecmp_id=200000",
            "l3_route_add(vrf=0, p=10.1.0.0/16, target=200000, ecmp=true, replace=false) -> SUCCESS",
            "port_link_get(hw=7) -> SUCCESS up=false",
            "fp_group_create(qset=[InPorts], priority=1) -> SUCCESS gid=1",
            "fp_entry_create(gid=1, r={priority=0,m={},drop=false,drop_cancel=false,copy_cpu=false,counter=true,policer=-1,green_drop=false,yellow_drop=false,red_drop=false,cpu_copy_only=false,mirror_ingress=[],mirror_egress=[],redirect=-1,redirect_egress=false,set_tc=-1,set_dscp=-1}) -> SUCCESS eid=1",
            "fp_entry_counter(eid=1) -> SUCCESS packets=0 bytes=0",
            "lag_create() -> SUCCESS tid=1",
            "port_speed_set(hw=1, mbps=12345) -> INVALID_PARAMETER",
            "queue_scheduler_set(hw=1, queue=3, cfg=none) -> SUCCESS",
            "l2_flush(hw=-1, vid=10, statics=true, dynamics=true) -> SUCCESS",
            "port_link_gate(hw=1, gate=empty) -> SUCCESS",
            "netif_create(ifname=\"Ethernet0\", hw=1, vid=1, mac=02:00:00:00:00:01, keep_tag=false) -> SUCCESS netif_id=1 filter_id=2",
        ]
    );
}

#[test]
fn full_mode_logs_the_queries_and_events_from_test_hooks() {
    let mut rec = Recording::new(VirtualBackend::new(), Mode::Full);
    let log = rec.log();
    let (sink, _rx) = EventSink::channel();
    rec.init(&Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "1")]), sink)
        .unwrap();
    assert!(rec.is_fake());
    assert_eq!(rec.cpu_port(), HwPort(0));
    rec.inner_mut().inject_fdb(FdbEvent {
        change: FdbChange::Learned,
        mac: mac(5),
        vid: Vid::DEFAULT,
        hw_port: Dest::Lag(TrunkId(1)),
    });
    let (_ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
    rec.run_shell_to("x", std::os::fd::AsFd::as_fd(&theirs)).unwrap();
    assert_eq!(
        log.lines(),
        [
            r#"init(profile={SAI_RS_VIRTUAL_PORTS="1"}) -> SUCCESS"#,
            "is_fake() -> true",
            "cpu_port() -> 0",
            "event fdb(learned=true, mac=00:11:22:33:44:05, vid=1, hw_port=1073741825)",
            "run_shell_to(cmd=\"x\") -> SUCCESS",
        ]
    );
}

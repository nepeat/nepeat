//! The test port over a core on the virtual backend, set up as the C++ suite runs
//! (`SAI_OPENBCM_FAKE_PORTS`, `SAI_RS_VIRTUAL_COMPAT=cpp`): every request of the protocol, the
//! encodings, the `ABSENT` / `BAD_REQUEST` / `UNSUPPORTED` paths, and that the hooks and reads
//! leave the object model (and armed backend faults) alone.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "tests"
)]
use sai_core::backend::BackendEvent;
use sai_core::backend::types::{
    BufferCfg, Dest, FloodKind, FpMatch, FpQual, FpRule, LagMember, Learn, MacAddr, Masked, MdioDev, MirrorCfg,
    PhyAddr, PolicerCfg, PortInterface, QosMapEntry, QosMapKind, RouteTarget, SchedCfg, StormKind, TrapKind, Vrf,
    WredCfg,
};
use sai_core::backend::{
    Backend, Field as _, Hash as _, Hostif as _, HwPort, L3 as _, Lag as _, Mdio as _, Mirror as _, Mode, Policer as _,
    PortOps as _, Profile, Qos as _, Recording, Sample as _, Storm as _, Vid,
};
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::testing;
use sai_core::value::{Attr, IpPrefix, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use sai_test_port::{Code, Context, answer};
use std::net::IpAddr;

type Core = Sai<Box<dyn Backend>>;

const CX: Context = Context {
    has_hardware_backend: false,
    events_delivered: 7,
};

/// Ports 1, 5, 9, 13 (four lanes each, the C++ numbering).
fn profile() -> Profile {
    Profile::from_pairs([
        ("SAI_OPENBCM_FAKE_PORTS", "4"),
        ("SAI_OPENBCM_FAKE_LANES", "4"),
        ("SAI_RS_VIRTUAL_COMPAT", "cpp"),
        ("SAI_RS_VIRTUAL_ON_HARDWARE", "1"),
    ])
}

fn core_on(backend: Box<dyn Backend>) -> (Core, RawOid) {
    let mut sai: Core = Sai::new(backend);
    let switch = sai
        .create_switch(
            &[Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))],
            &profile(),
        )
        .unwrap()
        .raw();
    (sai, switch)
}

fn core() -> (Core, RawOid) {
    core_on(Box::new(VirtualBackend::new()))
}

fn ask(sai: &mut Core, request: &str) -> (Code, String) {
    let reply = answer(request, Some(sai), &CX);
    (reply.code, reply.text)
}

fn ok(sai: &mut Core, request: &str) -> String {
    let (code, text) = ask(sai, request);
    assert_eq!(code, Code::Ok, "{request}: {text}");
    text
}

fn val(sai: &mut Core, request: &str) -> String {
    let text = ok(sai, request);
    text.strip_prefix("value=")
        .and_then(|t| t.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("{request}: {text:?}"))
        .to_owned()
}

fn code(sai: &mut Core, request: &str) -> Code {
    ask(sai, request).0
}

fn chip(sai: &mut Core) -> &mut VirtualBackend {
    testing::virtual_backend_mut(sai).unwrap()
}

fn port_oid(hw: u64) -> u64 {
    testing::oid_layout::make_oid(sys::SAI_OBJECT_TYPE_PORT, hw, 0).0
}

const MAC: MacAddr = MacAddr([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);

#[test]
fn pure_requests_need_no_library() {
    let none: Option<&mut Core> = None;
    let r = answer("oid_index_mask", none, &CX);
    assert_eq!((r.code, r.text.as_str()), (Code::Ok, "value=281474976710655\n"));
    let pure = |request: &str| {
        let r = answer::<Box<dyn Backend>>(request, None, &CX);
        (r.code, r.text)
    };
    assert_eq!(pure("has_hardware_backend"), (Code::Ok, "value=0\n".into()));
    assert_eq!(pure("events_delivered"), (Code::Ok, "value=7\n".into()));
    assert_eq!(pure("switch_created"), (Code::Ok, "value=0\n".into()));
    let lag5 = (u64::from(sys::SAI_OBJECT_TYPE_LAG) << 56) | (3 << 48) | 5;
    assert_eq!(
        pure(&format!("make_oid {} 5 3", sys::SAI_OBJECT_TYPE_LAG)),
        (Code::Ok, format!("value={lag5}\n"))
    );
    assert_eq!(
        pure(&format!("oid_type {lag5}")),
        (Code::Ok, format!("value={}\n", sys::SAI_OBJECT_TYPE_LAG))
    );
    assert_eq!(pure(&format!("oid_index {lag5}")), (Code::Ok, "value=5\n".into()));
    assert_eq!(pure(&format!("oid_switch {lag5}")), (Code::Ok, "value=3\n".into()));
    assert_eq!(
        pure("oid_type 0"),
        (Code::Ok, format!("value={}\n", sys::SAI_OBJECT_TYPE_NULL))
    );
    // the capability table, row by row
    let (c, caps) = pure("attr_caps");
    assert_eq!(c, Code::Ok);
    assert_eq!(caps.lines().count(), sai_core::caps::attributes().count());
    let (ot, id, cap) = sai_core::caps::attributes().next().unwrap();
    assert_eq!(
        caps.lines().next().unwrap(),
        format!(
            "ot={ot} id={id} create={} set={} get={}",
            u8::from(cap.create),
            u8::from(cap.set),
            u8::from(cap.get)
        )
    );
    assert_eq!(pure("log_to_stderr 0"), (Code::Ok, String::new()));
    // what needs the library is unsupported before sai_api_initialize
    let (c, why) = pure("backend_id 1");
    assert_eq!(c, Code::Unsupported);
    assert_eq!(why, "backend_id: the API is not initialised");
    assert_eq!(pure("ports").0, Code::Unsupported);
}

#[test]
fn malformed_requests_are_refused_with_a_reason() {
    let pure = |request: &str| {
        let r = answer::<Box<dyn Backend>>(request, None, &CX);
        (r.code, r.text)
    };
    for (request, reason) in [
        ("", "empty request"),
        ("frobnicate 1", "frobnicate: unknown request"),
        ("oid_index", "oid_index: missing object id"),
        ("oid_index x", "oid_index: bad object id \"x\""),
        ("oid_index -1", "oid_index: bad object id \"-1\""),
        ("oid_index_mask 1", "oid_index_mask: unexpected argument \"1\""),
        ("oid_index  1", "oid_index: missing object id"),
        ("log_to_stderr 2", "log_to_stderr: bad on \"2\" (0 or 1)"),
        (
            "set_feature acl 1",
            "set_feature: unknown feature \"acl\" (qos, mirror, l2)",
        ),
        ("count T_NOPE", "count: unknown table \"T_NOPE\""),
        (
            "inject_fdb 1 00:11:22:33:44 1 1",
            "inject_fdb: bad MAC \"00:11:22:33:44\"",
        ),
        (
            "host_egress 0 10.0.0.1",
            "host_egress: bad IP \"10.0.0.1\" (4/<8 hex> or 6/<32 hex>)",
        ),
        ("set_temperatures 1,x", "set_temperatures: bad temperature \"x\""),
        ("diag_start a%2", "diag_start: bad string \"a%2\""),
    ] {
        assert_eq!(pure(request), (Code::BadRequest, reason.to_owned()), "{request}");
    }
}

#[test]
fn the_librarys_view() {
    let (mut sai, switch) = core();
    assert_eq!(val(&mut sai, "switch_created"), "1");
    // a port's backend id is its hw; an unknown id has none
    assert_eq!(val(&mut sai, &format!("backend_id {}", port_oid(5))), "5");
    assert_eq!(val(&mut sai, &format!("backend_id {}", port_oid(2))), "-1");
    assert_eq!(val(&mut sai, "backend_id 0"), "-1");
    // a LAG: its trunk; members' LAG
    let lag = sai.create(sys::SAI_OBJECT_TYPE_LAG, switch, &[]).unwrap();
    let tid = val(&mut sai, &format!("backend_id {}", lag.0));
    assert!(tid.parse::<i64>().unwrap() >= 0, "{tid}");
    assert_eq!(ok(&mut sai, &format!("lag_members {tid}")), "");
    assert_eq!(val(&mut sai, &format!("port_lag {}", port_oid(1))), "0");
    // the default virtual router: VRF 0; counted
    let vr = sai
        .get(
            sys::SAI_OBJECT_TYPE_SWITCH,
            switch,
            &[sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID],
        )
        .unwrap();
    let Value::Oid(vr) = vr[0] else { panic!("{vr:?}") };
    assert_eq!(val(&mut sai, &format!("backend_id {}", vr.0)), "0");
    assert_eq!(
        val(
            &mut sai,
            &format!("object_count {}", sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER)
        ),
        "1"
    );
    assert_eq!(
        val(&mut sai, &format!("object_count {}", sys::SAI_OBJECT_TYPE_ACL_TABLE)),
        "0"
    );
    assert_eq!(
        val(&mut sai, &format!("object_count {}", sys::SAI_OBJECT_TYPE_ACL_ENTRY)),
        "0"
    );
    let (c, why) = ask(&mut sai, &format!("object_count {}", sys::SAI_OBJECT_TYPE_PORT));
    assert_eq!(
        (c, why.as_str()),
        (
            Code::BadRequest,
            "object_count: answered for VIRTUAL_ROUTER, ACL_TABLE and ACL_ENTRY only"
        )
    );
    // unknown objects: the absent answers
    assert_eq!(ok(&mut sai, "rif 12345"), "intf=-1 vrf=-1 vid=0\n");
    assert_eq!(val(&mut sai, "trap_programmed 12345"), "0");
    assert_eq!(val(&mut sai, "acl_binding_count"), "0");
    assert_eq!(val(&mut sai, "copp_group"), "-1");
    assert_eq!(val(&mut sai, "copp_rule_count"), "0");
    assert_eq!(val(&mut sai, "neighbor_egress 12345 4/0a000001"), "-1");
    assert_eq!(val(&mut sai, "events_delivered"), "7");
    assert_eq!(val(&mut sai, "has_hardware_backend"), "0");
    // services
    assert_eq!(val(&mut sai, "ext_phy_active"), "0");
    assert_eq!(val(&mut sai, "ext_macsec_active"), "0");
    let status = ok(&mut sai, "ext_phy_status");
    assert!(
        status.starts_with("text=extphy:%20no%20external%20PHY%20table"),
        "{status}"
    );
    assert!(status.ends_with("%0A\n") && status.lines().count() == 1, "{status}");
    assert!(ok(&mut sai, "ext_macsec_status").starts_with("text=extmacsec:%20"));
    assert_eq!(ok(&mut sai, "diag_start"), "");
    assert_eq!(ok(&mut sai, "diag_start "), "");
    assert_eq!(code(&mut sai, "diag_start /tmp/x%20y.sock"), Code::Unsupported);
    assert_eq!(code(&mut sai, "diag_stop"), Code::Unsupported);
    assert_eq!(ok(&mut sai, "log_to_stderr 0"), "");
}

#[test]
fn hooks_change_the_chip() {
    let (mut sai, _) = core();
    // features
    assert_eq!(ok(&mut sai, "set_feature qos 1"), "");
    assert_eq!(ok(&mut sai, "set_feature mirror 1"), "");
    assert_eq!(ok(&mut sai, "set_feature l2 1"), "");
    assert!(chip(&mut sai).toggles().qos_on && chip(&mut sai).toggles().mirror_on && chip(&mut sai).toggles().l2x_on);
    ok(&mut sai, "set_feature l2 0");
    assert!(!chip(&mut sai).toggles().l2x_on);
    // link: queued as an event, the model untouched until it is applied
    let events = sai.take_events().unwrap();
    assert_eq!(ok(&mut sai, "inject_link 9 1"), "");
    assert!(ok(&mut sai, "port 9").contains(" link=1 "));
    assert_eq!(
        events.try_recv().unwrap(),
        BackendEvent::Link {
            hw: HwPort(9),
            up: true
        }
    );
    assert_eq!(code(&mut sai, "inject_link 2 1"), Code::BadRequest);
    assert_eq!(code(&mut sai, "inject_link -1 1"), Code::BadRequest);
    assert_eq!(ok(&mut sai, "linkscan"), "");
    // FDB: learned and aged, the table follows, the event is queued
    while events.try_recv().is_ok() {}
    ok(&mut sai, "inject_fdb 1 00:11:22:33:44:55 1 5");
    assert_eq!(
        ok(&mut sai, "l2_entries"),
        "mac=00:11:22:33:44:55 vid=1 dest=5 static=0 discard=0\n"
    );
    assert!(matches!(events.try_recv(), Ok(BackendEvent::Fdb(_))));
    ok(&mut sai, "inject_fdb 0 00:11:22:33:44:55 1 5");
    assert_eq!(ok(&mut sai, "l2_entries"), "");
    assert_eq!(code(&mut sai, "inject_fdb 1 00:11:22:33:44:55 0 5"), Code::BadRequest);
    assert_eq!(code(&mut sai, "inject_fdb 1 00:11:22:33:44:55 1 -2"), Code::BadRequest);
    drop(events);
    // L2 and host tables, directly
    let lag_dest = val(&mut sai, "lag_dest 3");
    assert_eq!(lag_dest, (0x4000_0000u32 | 3).to_string());
    ok(&mut sai, &format!("backend_l2_insert 00:11:22:33:44:55 20 {lag_dest}"));
    assert_eq!(
        ok(&mut sai, "l2_entries"),
        format!("mac=00:11:22:33:44:55 vid=20 dest={lag_dest} static=0 discard=0\n")
    );
    ok(&mut sai, "backend_l2_erase 00:11:22:33:44:55 20");
    assert_eq!(ok(&mut sai, "l2_entries"), "");
    assert_eq!(code(&mut sai, "lag_dest -1"), Code::BadRequest);
    ok(&mut sai, "backend_host_insert 0 4/0a000001 100002");
    ok(
        &mut sai,
        "backend_host_insert 2 6/fc000000000000000000000000000001 100003",
    );
    assert_eq!(
        ok(&mut sai, "hosts"),
        "vrf=0 ip=4/0a000001 egress=100002\nvrf=2 ip=6/fc000000000000000000000000000001 egress=100003\n"
    );
    assert_eq!(val(&mut sai, "host_egress 0 4/0a000001"), "100002");
    ok(&mut sai, "backend_host_erase 0 4/0a000001");
    assert_eq!(val(&mut sai, "host_egress 0 4/0a000001"), "-1");
    // drop egress: made once; the backend refuses to destroy it, and reports unknown ones
    let drop = val(&mut sai, "drop_egress");
    assert_eq!(val(&mut sai, "drop_egress"), drop);
    assert_eq!(
        ok(&mut sai, &format!("egress {drop}")),
        format!("id={drop} intf=0 mac=00:00:00:00:00:00 dest=0 vid=0 cpu=0 drop=1\n")
    );
    assert_eq!(
        ok(&mut sai, &format!("backend_egress_destroy {drop}")),
        format!("status={}\n", sys::SAI_STATUS_INVALID_PARAMETER)
    );
    assert_eq!(
        ok(&mut sai, "backend_egress_destroy 99"),
        format!("status={}\n", sys::SAI_STATUS_ITEM_NOT_FOUND)
    );
    // counters: what the SAI stats calls then read
    let port = RawOid(port_oid(1));
    ok(
        &mut sai,
        &format!("set_port_stat 1 {} 1234", sys::SAI_PORT_STAT_IF_IN_OCTETS),
    );
    assert_eq!(
        sai.stats_get(
            sys::SAI_OBJECT_TYPE_PORT,
            port,
            &[sys::SAI_PORT_STAT_IF_IN_OCTETS],
            false
        )
        .unwrap(),
        [1234]
    );
    let (c, why) = ask(&mut sai, "set_port_stat 1 999999 1");
    assert_eq!(c, Code::BadRequest, "{why}");
    // a number no port has: set all the same, as the C++ fake's map does
    assert_eq!(
        ok(
            &mut sai,
            &format!("set_port_stat 2 {} 1", sys::SAI_PORT_STAT_IF_IN_OCTETS)
        ),
        ""
    );
    assert_eq!(
        code(
            &mut sai,
            &format!("set_port_stat 70000 {} 1", sys::SAI_PORT_STAT_IF_IN_OCTETS)
        ),
        Code::BadRequest
    );
    ok(
        &mut sai,
        &format!("set_queue_stat 1 3 {} 77", sys::SAI_QUEUE_STAT_PACKETS),
    );
    assert_eq!(
        chip(&mut sai)
            .queue_stats_get(HwPort(1), 3, &[sys::SAI_QUEUE_STAT_PACKETS])
            .unwrap(),
        [77]
    );
    ok(
        &mut sai,
        &format!("set_pg_stat 5 2 {} 88", sys::SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES),
    );
    assert_eq!(
        chip(&mut sai)
            .pg_stats_get(HwPort(5), 2, &[sys::SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES])
            .unwrap(),
        [88]
    );
    assert_eq!(
        code(
            &mut sai,
            &format!("set_queue_stat 1 300 {} 1", sys::SAI_QUEUE_STAT_PACKETS)
        ),
        Code::BadRequest
    );
    // field entry counters (an unknown entry: nothing happens)
    assert_eq!(ok(&mut sai, "fp_hit 4242 1 64"), "");
    // temperatures
    ok(&mut sai, "set_temperatures 401,-5,0");
    assert_eq!(
        sai_core::backend::Temp::asic_temperatures(chip(&mut sai)).unwrap().cur,
        [401, -5, 0]
    );
    ok(&mut sai, "set_temperatures");
    ok(&mut sai, "set_temperatures ");
    assert_eq!(
        sai_core::backend::Temp::asic_temperatures(chip(&mut sai)).unwrap().cur,
        Vec::<i32>::new()
    );
    ok(&mut sai, "set_temperature_fail 1");
    assert!(sai_core::backend::Temp::asic_temperatures(chip(&mut sai)).is_err());
    ok(&mut sai, "set_temperature_fail 0");
    // MDIO: registers, log, dead PHYs
    ok(&mut sai, "mdio_set 4 1 49152 43981");
    let dev = MdioDev::new(1).unwrap();
    assert_eq!(chip(&mut sai).mdio_c45_read(PhyAddr(4), dev, 0xc000), Ok(0xabcd));
    assert_eq!(ok(&mut sai, "mdio_log 1"), "op=R phy=4 dev=1 reg=49152 val=43981\n");
    assert_eq!(ok(&mut sai, "mdio_log 0"), "");
    ok(&mut sai, "mdio_set_dead 4 1");
    assert!(chip(&mut sai).mdio_c45_read(PhyAddr(4), dev, 0xc000).is_err());
    ok(&mut sai, "mdio_set_dead 4 0");
    assert!(chip(&mut sai).mdio_c45_read(PhyAddr(4), dev, 0xc000).is_ok());
    assert_eq!(code(&mut sai, "mdio_set 4 0 1 1"), Code::BadRequest);
    assert_eq!(code(&mut sai, "mdio_set 4 1 65536 1"), Code::BadRequest);
    assert_eq!(code(&mut sai, "mdio_set 4 1 1 65536"), Code::BadRequest);
    // the shell log: commands percent-encoded
    assert_eq!(ok(&mut sai, "shell_log"), "");
    sai_core::backend::Shell::run_shell(chip(&mut sai), "ps xe0 = 1").unwrap();
    assert_eq!(ok(&mut sai, "shell_log"), "cmd=ps%20xe0%20%3D%201\n");
    // quirks forgotten, calls kept
    chip(&mut sai)
        .port_interface_set(HwPort(13), PortInterface::Caui4)
        .unwrap();
    assert!(ok(&mut sai, "port 13").contains(" iface=caui4 ifg=0 quirk_calls=1 "));
    ok(&mut sai, "port_forget_quirks 13");
    assert!(ok(&mut sai, "port 13").contains(" iface= ifg=0 quirk_calls=1 "));
    ok(&mut sai, "port_forget_quirks 2");
}

#[test]
fn the_chips_state_in_records() {
    let (mut sai, switch) = core();
    for table in [
        "T_STATIONS",
        "T_QOS_MAPS",
        "T_PORT_QOS_MAPS",
        "T_QUEUE_SCHEDULERS",
        "T_PORT_SCHEDULERS",
        "T_QUEUE_BUFFERS",
        "T_PG_BUFFERS",
        "T_MIRRORS",
        "T_PORT_MIRRORS",
        "T_SAMPLES",
        "T_PORT_SAMPLES",
        "T_STORM",
        "T_FLOOD",
        "T_FP_GROUPS",
        "T_FP_ENTRIES",
        "T_POLICERS",
    ] {
        let count: usize = val(&mut sai, &format!("count {table}")).parse().unwrap();
        assert!(count < 1000, "{table}");
    }
    // ports, by hw, as the C++ numbers them
    let ports = ok(&mut sai, "ports");
    let hws: Vec<&str> = ports.lines().map(|l| l.split(' ').next().unwrap()).collect();
    assert_eq!(hws, ["hw=1", "hw=5", "hw=9", "hw=13"]);
    let one = ok(&mut sai, "port 1");
    assert_eq!(ports.lines().next().unwrap(), one.trim_end());
    let keys: Vec<&str> = one
        .trim_end()
        .split(' ')
        .map(|f| f.split('=').next().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "hw",
            "admin",
            "link",
            "fec",
            "autoneg",
            "iface",
            "ifg",
            "quirk_calls",
            "loopback",
            "pvid",
            "frame_max",
            "learn"
        ]
    );
    assert!(one.starts_with("hw=1 admin=0 link=0 "), "{one}");
    assert_eq!(ask(&mut sai, "port 2"), (Code::Absent, String::new()));
    assert_eq!(ask(&mut sai, "port -1"), (Code::Absent, String::new()));
    chip(&mut sai).port_learn_set(HwPort(1), Learn::Cpu).unwrap();
    assert!(ok(&mut sai, "port 1").ends_with(" learn=3\n"));
    chip(&mut sai)
        .port_fec_set(HwPort(1), sai_core::backend::types::Fec::Rs)
        .unwrap();
    assert!(ok(&mut sai, "port 1").contains(&format!(" fec={} ", sys::SAI_PORT_FEC_MODE_RS)));
    chip(&mut sai)
        .port_loopback_set(HwPort(1), sai_core::backend::types::Loopback::Mac)
        .unwrap();
    assert!(ok(&mut sai, "port 1").contains(&format!(" loopback={} ", sys::SAI_PORT_LOOPBACK_MODE_MAC)));
    // L2 and VLANs
    let age: u32 = val(&mut sai, "l2_age_seconds").parse().unwrap();
    assert!(age > 0);
    assert_eq!(val(&mut sai, "vlan_exists 1"), "1");
    assert_eq!(val(&mut sai, "vlan_exists 77"), "0");
    assert_eq!(val(&mut sai, "vlan_exists 0"), "0");
    sai.create(
        sys::SAI_OBJECT_TYPE_VLAN,
        switch,
        &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(77))],
    )
    .unwrap();
    assert_eq!(val(&mut sai, "vlan_exists 77"), "1");
    chip(&mut sai)
        .vlan_port_add(Vid::new(77).unwrap(), HwPort(9), false)
        .unwrap();
    chip(&mut sai)
        .vlan_port_add(Vid::new(77).unwrap(), HwPort(5), true)
        .unwrap();
    let members = ok(&mut sai, "vlan_members 77");
    assert!(members.ends_with("hw=5 untagged=1\nhw=9 untagged=0\n"), "{members}");
    assert_eq!(ok(&mut sai, "vlan_members 4000"), "");
    // netifs and traps
    let created = chip(&mut sai)
        .netif_create("Eth 1", HwPort(5), Vid::new(1).unwrap(), MAC, true)
        .unwrap();
    assert_eq!(
        ok(&mut sai, "netifs"),
        format!(
            "id={} name=Eth%201 hw=5 vid=1 mac=00:11:22:33:44:55 keep_tag=1 carrier=0 filter={}\n",
            created.netif_id.get(),
            created.filter_id.get()
        )
    );
    let lldp = sys::SAI_HOSTIF_TRAP_TYPE_LLDP;
    assert_eq!(val(&mut sai, &format!("trap_to_cpu {lldp}")), "0");
    chip(&mut sai).trap_set(TrapKind::Lldp, true).unwrap();
    assert_eq!(val(&mut sai, &format!("trap_to_cpu {lldp}")), "1");
    assert_eq!(
        val(&mut sai, &format!("trap_to_cpu {}", sys::SAI_HOSTIF_TRAP_TYPE_BGP)),
        "0"
    );
    assert_eq!(val(&mut sai, "trap_to_cpu -3"), "0");
    // L3: interfaces, egress, routes (canonicalised by the mask), ECMP
    let vid = Vid::new(77).unwrap();
    let c = chip(&mut sai);
    let intf = c.l3_intf_create(MAC, vid, 9100, Vrf(2)).unwrap();
    let e1 = c
        .l3_egress_create(intf, MacAddr([2, 0, 0, 0, 0, 1]), Dest::Port(HwPort(9)), vid)
        .unwrap();
    let e2 = c
        .l3_egress_create(intf, MacAddr([2, 0, 0, 0, 0, 2]), Dest::Port(HwPort(13)), vid)
        .unwrap();
    let ecmp = c.l3_ecmp_create(&[e1, e2, e1]).unwrap();
    let v6: IpAddr = "2001:db8::".parse().unwrap();
    let mask: IpAddr = "ffff:ffff::".parse().unwrap();
    c.l3_route_add(Vrf(2), IpPrefix::new(v6, mask).unwrap(), RouteTarget::Ecmp(ecmp), false)
        .unwrap();
    assert_eq!(
        ok(&mut sai, "l3_intfs"),
        format!("id={} mac=00:11:22:33:44:55 vid=77 mtu=9100 vrf=2\n", intf.get())
    );
    let egress = ok(&mut sai, "egress_objects");
    assert!(
        egress.contains(&format!(
            "id={} intf={} mac=02:00:00:00:00:01 dest=9 vid=77 cpu=0 drop=0\n",
            e1.get(),
            intf.get()
        )),
        "{egress}"
    );
    assert_eq!(ask(&mut sai, "egress 1"), (Code::Absent, String::new()));
    assert_eq!(
        ok(&mut sai, "routes"),
        format!(
            "vrf=2 addr=6/20010db8000000000000000000000000 mask=6/ffffffff000000000000000000000000 target={}\n",
            ecmp.get()
        )
    );
    // an address with host bits set finds the same route
    let route = "route_target 2 6/20010db8000000000000000000000001 6/ffffffff000000000000000000000000";
    assert_eq!(val(&mut sai, route), ecmp.get().to_string());
    assert_eq!(val(&mut sai, "route_target 2 4/0a000000 4/ff000000"), "-1");
    assert_eq!(
        val(&mut sai, "route_target 2 4/0a000000 6/ffffffff000000000000000000000000"),
        "-1"
    );
    assert_eq!(
        ok(&mut sai, "ecmp_groups"),
        format!("id={} egress={},{},{}\n", ecmp.get(), e1.get(), e2.get(), e1.get())
    );
    // LAG members
    let c = chip(&mut sai);
    let tid = c.lag_create().unwrap();
    let members = [
        LagMember {
            hw: HwPort(9),
            egress_disable: false,
            ingress_disable: true,
        },
        LagMember {
            hw: HwPort(1),
            egress_disable: true,
            ingress_disable: false,
        },
    ];
    c.lag_set_members(tid, &members).unwrap();
    assert_eq!(
        ok(&mut sai, &format!("lag_members {}", tid.get())),
        "hw=9 egress_disable=0 ingress_disable=1\nhw=1 egress_disable=1 ingress_disable=0\n"
    );
    assert_eq!(ask(&mut sai, "lag_members 999"), (Code::Absent, String::new()));
    // field processor and meters
    let c = chip(&mut sai);
    let pid = c
        .policer_create(&PolicerCfg {
            tr_tcm: true,
            packets: false,
            color_blind: true,
            cir: 1,
            cbs: 2,
            pir: 3,
            pbs: 4,
        })
        .unwrap();
    let gid = c
        .fp_group_create(&[FpQual::InPorts, FpQual::DstIp6, FpQual::EtherType], 7)
        .unwrap();
    let rule = FpRule {
        priority: 12,
        m: FpMatch {
            in_ports: Some(vec![HwPort(1), HwPort(5)]),
            ether_type: Some(Masked {
                value: 0x86dd,
                mask: 0xffff,
            }),
            dst_ip6: Some(Masked {
                value: "fc00::1".parse().unwrap(),
                mask: "ffff::".parse().unwrap(),
            }),
            ..FpMatch::default()
        },
        drop: true,
        counter: true,
        policer: Some(pid),
        red_drop: true,
        ..FpRule::default()
    };
    let eid = c.fp_entry_create(gid, &rule).unwrap();
    assert_eq!(
        ok(&mut sai, &format!("fp_group {}", gid.get())),
        format!("qset={} priority=7\n", rule.m.present() | FpQual::DstIp6.bit())
    );
    assert_eq!(ask(&mut sai, "fp_group 999"), (Code::Absent, String::new()));
    let entry = format!(
        "eid={} gid={} priority=12 present={} in_ports=1,5 ether=34525 ip_type=0 l4_src=0 l4_dst=0 \
         dst=6/fc000000000000000000000000000001 dst_mask=6/ffff0000000000000000000000000000 drop=1 drop_cancel=0 \
         copy_cpu=0 counter=1 policer={} red_drop=1 cpu_copy_only=0 mirror_ingress= mirror_egress=\n",
        eid.get(),
        gid.get(),
        rule.m.present(),
        pid.get()
    );
    assert_eq!(ok(&mut sai, &format!("fp_entry {}", eid.get())), entry);
    assert_eq!(ok(&mut sai, "fp_entries"), entry);
    assert_eq!(ask(&mut sai, "fp_entry -1"), (Code::Absent, String::new()));
    ok(&mut sai, &format!("fp_hit {} 3 300", eid.get()));
    let counter = chip(&mut sai).fp_entry_counter(eid).unwrap();
    assert_eq!((counter.packets, counter.bytes), (3, 300));
    assert_eq!(
        ok(&mut sai, &format!("policer {}", pid.get())),
        "tr_tcm=1 packets=0 color_blind=1 cir=1 cbs=2 pir=3 pbs=4\n"
    );
    assert_eq!(ask(&mut sai, "policer 999"), (Code::Absent, String::new()));
    assert_eq!(val(&mut sai, "count T_FP_ENTRIES"), "1");
    assert_eq!(val(&mut sai, "count T_POLICERS"), "1");
}

#[test]
fn qos_mirror_and_l2_extras() {
    let (mut sai, _) = core();
    ok(&mut sai, "set_feature qos 1");
    ok(&mut sai, "set_feature mirror 1");
    ok(&mut sai, "set_feature l2 1");
    let c = chip(&mut sai);
    let entries = [
        QosMapEntry {
            key: 0,
            key_color: 0,
            value: 1,
            value_color: 0,
        },
        QosMapEntry {
            key: 1,
            key_color: 2,
            value: 3,
            value_color: 1,
        },
    ];
    let map = c.qos_map_create(QosMapKind::TcColorToDscp, &entries).unwrap();
    c.port_qos_map_set(HwPort(5), QosMapKind::TcColorToDscp, Some(map))
        .unwrap();
    c.port_default_tc_set(HwPort(5), 3).unwrap();
    c.port_pfc_set(HwPort(5), true, 0x18, 0x08).unwrap();
    let sched = SchedCfg {
        strict: true,
        weight: 4,
        packets: true,
        min_rate: 1,
        min_burst: 2,
        max_rate: 3,
        max_burst: 4,
    };
    c.queue_scheduler_set(HwPort(5), 2, Some(&sched)).unwrap();
    c.port_scheduler_set(HwPort(9), Some(&sched)).unwrap();
    let wred_cfg = WredCfg {
        enable: [true, false, true],
        min: [1, 2, 3],
        max: [4, 5, 6],
        prob: [7, 8, 9],
        weight: 2,
        ecn: 1,
    };
    let wred = c.wred_create(&wred_cfg).unwrap();
    c.queue_wred_set(HwPort(5), 2, Some(wred)).unwrap();
    let buffer = BufferCfg {
        ingress: true,
        dynamic: false,
        dynamic_th: -2,
        static_th: 9,
        reserved: 8,
        xoff: 7,
        xon: 6,
        xon_offset: 5,
        pool_size: 4,
        pool_xoff: 3,
    };
    c.queue_buffer_set(HwPort(5), 1, Some(&buffer)).unwrap();
    c.pg_buffer_set(HwPort(5), 0, Some(&buffer)).unwrap();
    let dscp = sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP;
    assert_eq!(
        ok(&mut sai, &format!("qos_map {}", map.get())),
        format!("type={dscp} entries=0.0.1.0,1.2.3.1\n")
    );
    assert_eq!(ask(&mut sai, "qos_map 999"), (Code::Absent, String::new()));
    assert_eq!(val(&mut sai, &format!("port_qos_map 5 {dscp}")), map.get().to_string());
    assert_eq!(val(&mut sai, &format!("port_qos_map 9 {dscp}")), "-1");
    assert_eq!(val(&mut sai, "port_qos_map 5 999"), "-1");
    assert_eq!(val(&mut sai, "port_default_tc 5"), "3");
    assert_eq!(val(&mut sai, "port_default_tc 9"), "-1");
    assert_eq!(ok(&mut sai, "port_pfc 5"), "separate=1 tx=24 rx=8\n");
    assert_eq!(ask(&mut sai, "port_pfc 9"), (Code::Absent, String::new()));
    let sched_text = "strict=1 weight=4 packets=1 min_rate=1 min_burst=2 max_rate=3 max_burst=4\n";
    assert_eq!(ok(&mut sai, "queue_scheduler 5 2"), sched_text);
    assert_eq!(ask(&mut sai, "queue_scheduler 5 3"), (Code::Absent, String::new()));
    assert_eq!(ok(&mut sai, "port_scheduler 9"), sched_text);
    assert_eq!(ask(&mut sai, "port_scheduler 5"), (Code::Absent, String::new()));
    assert_eq!(
        ok(&mut sai, &format!("wred {}", wred.get())),
        "enable=1,0,1 min=1,2,3 max=4,5,6 prob=7,8,9 weight=2 ecn=1\n"
    );
    assert_eq!(val(&mut sai, "queue_wred 5 2"), wred.get().to_string());
    assert_eq!(val(&mut sai, "queue_wred 5 300"), "-1");
    let buffer_text =
        "ingress=1 dynamic=0 dynamic_th=-2 static_th=9 reserved=8 xoff=7 xon=6 xon_offset=5 pool_size=4 pool_xoff=3\n";
    assert_eq!(ok(&mut sai, "queue_buffer 5 1"), buffer_text);
    assert_eq!(ok(&mut sai, "pg_buffer 5 0"), buffer_text);
    assert_eq!(ask(&mut sai, "pg_buffer 5 1"), (Code::Absent, String::new()));
    // mirror, sample, genetlink
    let c = chip(&mut sai);
    let mut cfg = MirrorCfg::new(HwPort(13));
    cfg.erspan = true;
    cfg.src = "10.1.2.3".parse().unwrap();
    cfg.dst = "fc00::2".parse().unwrap();
    cfg.dmac = MAC;
    cfg.vlan = true;
    cfg.vlan_id = 20;
    let mirror = c.mirror_create(&cfg).unwrap();
    c.port_mirror_set(HwPort(1), true, &[mirror]).unwrap();
    let sample = c.sample_create(4096).unwrap();
    c.port_sample_set(HwPort(1), false, Some(sample)).unwrap();
    let genl = c.genetlink_create("psample", "pkt grp").unwrap();
    c.genetlink_trap_set(genl, 9, true).unwrap();
    c.genetlink_trap_set(genl, 2, true).unwrap();
    assert_eq!(
        ok(&mut sai, &format!("mirror {}", mirror.get())),
        "erspan=1 monitor=13 truncate=0 sample_rate=0 tos=0 ttl=255 src=4/0a010203 dst=6/fc000000000000000000000000000002 \
         smac=00:00:00:00:00:00 dmac=00:11:22:33:44:55 gre_type=35006 vlan=1 vlan_tpid=33024 vlan_id=20 vlan_pri=0 vlan_cfi=0\n"
    );
    assert_eq!(ask(&mut sai, "mirror 999"), (Code::Absent, String::new()));
    assert_eq!(ok(&mut sai, "port_mirrors 1 1"), format!("value={}\n", mirror.get()));
    assert_eq!(ok(&mut sai, "port_mirrors 1 0"), "value=\n");
    assert_eq!(val(&mut sai, &format!("sample_rate {}", sample.get())), "4096");
    assert_eq!(val(&mut sai, "sample_rate 999"), "0");
    assert_eq!(val(&mut sai, "port_sample 1 0"), sample.get().to_string());
    assert_eq!(val(&mut sai, "port_sample 1 1"), "-1");
    assert_eq!(
        ok(&mut sai, &format!("genetlink {}", genl.get())),
        "name=psample mcgrp=pkt%20grp traps=2,9\n"
    );
    assert_eq!(ask(&mut sai, "genetlink 999"), (Code::Absent, String::new()));
    // hash, storm, flood
    let c = chip(&mut sai);
    c.hash_set(true, &[3, 1, 2], 77).unwrap();
    let pid = c
        .policer_create(&PolicerCfg {
            cir: 100,
            cbs: 10,
            ..PolicerCfg::default()
        })
        .unwrap();
    c.port_storm_set(HwPort(9), StormKind::Multicast, Some(pid)).unwrap();
    c.vlan_flood_set(Vid::DEFAULT, FloodKind::Broadcast, false).unwrap();
    assert_eq!(ok(&mut sai, "hash 1"), "fields=3,1,2 seed=77\n");
    assert_eq!(ask(&mut sai, "hash 0"), (Code::Absent, String::new()));
    let mcast = sys::SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID;
    assert_eq!(val(&mut sai, &format!("port_storm 9 {mcast}")), pid.get().to_string());
    assert_eq!(
        val(
            &mut sai,
            &format!("port_storm 9 {}", sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID)
        ),
        "-1"
    );
    assert_eq!(val(&mut sai, &format!("port_storm 9 {}", sys::SAI_PORT_ATTR_MTU)), "-1");
    let bcast = sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE;
    assert_eq!(val(&mut sai, &format!("vlan_flood 1 {bcast}")), "0");
    c_on(&mut sai);
    assert_eq!(val(&mut sai, &format!("vlan_flood 1 {bcast}")), "1");
    assert_eq!(
        val(
            &mut sai,
            &format!("vlan_flood 1 {}", sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE)
        ),
        "-1"
    );
    for (table, n) in [
        ("T_QOS_MAPS", 1),
        ("T_PORT_QOS_MAPS", 1),
        ("T_QUEUE_SCHEDULERS", 1),
        ("T_PORT_SCHEDULERS", 1),
        ("T_QUEUE_BUFFERS", 1),
        ("T_PG_BUFFERS", 1),
        ("T_MIRRORS", 1),
        ("T_PORT_MIRRORS", 1),
        ("T_SAMPLES", 1),
        ("T_PORT_SAMPLES", 1),
        ("T_STORM", 1),
        ("T_FLOOD", 1),
    ] {
        assert_eq!(val(&mut sai, &format!("count {table}")), n.to_string(), "{table}");
    }
}

fn c_on(sai: &mut Core) {
    chip(sai)
        .vlan_flood_set(Vid::DEFAULT, FloodKind::Broadcast, true)
        .unwrap();
}

#[test]
fn without_a_switch_or_a_virtual_chip_the_chip_is_unsupported() {
    // a core whose switch was never created, then removed
    let mut sai: Core = Sai::new(Box::new(VirtualBackend::new()));
    let (c, why) = ask(&mut sai, "ports");
    assert_eq!(
        (c, why.as_str()),
        (
            Code::Unsupported,
            "ports: no switch (create one with SAI_RS_BACKEND=virtual)"
        )
    );
    assert_eq!(val(&mut sai, "switch_created"), "0");
    assert_eq!(val(&mut sai, &format!("backend_id {}", port_oid(1))), "-1");
    assert_eq!(
        val(
            &mut sai,
            &format!("object_count {}", sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER)
        ),
        "0"
    );
    assert_eq!(code(&mut sai, "set_feature qos 1"), Code::Unsupported);
    let (mut sai, switch) = core();
    sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, switch).unwrap();
    assert_eq!(val(&mut sai, "switch_created"), "0");
    assert_eq!(code(&mut sai, "inject_link 1 1"), Code::Unsupported);
    // the recording decorator passes the chip through
    let (mut sai, _) = core_on(Box::new(Recording::new(VirtualBackend::new(), Mode::Normal)));
    assert_eq!(ok(&mut sai, "ports").lines().count(), 4);
    ok(&mut sai, "inject_link 1 1");
    assert!(ok(&mut sai, "port 1").contains(" link=1 "));
}

/// Every request of the protocol, each with arguments of the right shape.
const ALL: &[&str] = &[
    "has_hardware_backend",
    "switch_created",
    "make_oid 1 2 0",
    "oid_type 1",
    "oid_index 1",
    "oid_switch 1",
    "oid_index_mask",
    "attr_caps",
    "backend_id 1",
    "rif 1",
    "port_lag 1",
    "trap_programmed 1",
    "object_count 3",
    "acl_binding_count",
    "copp_group",
    "copp_rule_count",
    "events_delivered",
    "neighbor_egress 1 4/0a000001",
    "diag_start",
    "diag_stop",
    "log_to_stderr 0",
    "ext_phy_active",
    "ext_phy_status",
    "ext_macsec_active",
    "ext_macsec_status",
    "set_feature qos 0",
    "inject_link 1 0",
    "linkscan",
    "inject_fdb 0 00:00:00:00:00:01 1 1",
    "set_port_stat 1 0 0",
    "set_queue_stat 1 0 0 0",
    "set_pg_stat 1 0 0 0",
    "fp_hit 1 0 0",
    "set_temperatures 400",
    "set_temperature_fail 0",
    "mdio_set 1 1 1 1",
    "mdio_set_dead 1 0",
    "mdio_log 0",
    "shell_log",
    "backend_host_insert 0 4/0a000009 100002",
    "backend_host_erase 0 4/0a000009",
    "backend_l2_insert 00:00:00:00:00:02 1 1",
    "backend_l2_erase 00:00:00:00:00:02 1",
    "drop_egress",
    "backend_egress_destroy 7",
    "lag_dest 1",
    "count T_STATIONS",
    "ports",
    "port 1",
    "port_forget_quirks 1",
    "l2_age_seconds",
    "vlan_exists 1",
    "vlan_members 1",
    "l2_entries",
    "netifs",
    "trap_to_cpu 3",
    "l3_intfs",
    "egress_objects",
    "egress 1",
    "hosts",
    "host_egress 0 4/0a000001",
    "routes",
    "route_target 0 4/0a000000 4/ff000000",
    "ecmp_groups",
    "lag_members 1",
    "fp_group 1",
    "fp_entries",
    "fp_entry 1",
    "policer 1",
    "qos_map 1",
    "port_qos_map 1 2",
    "port_default_tc 1",
    "port_pfc 1",
    "queue_scheduler 1 0",
    "port_scheduler 1",
    "wred 1",
    "queue_wred 1 0",
    "queue_buffer 1 0",
    "pg_buffer 1 0",
    "mirror 1",
    "port_mirrors 1 1",
    "sample_rate 1",
    "port_sample 1 0",
    "genetlink 1",
    "hash 1",
    "port_storm 1 56",
    "vlan_flood 1 10",
];

#[test]
fn every_request_is_known_and_answered() {
    let (mut sai, _) = core();
    let names: std::collections::BTreeSet<&str> = ALL.iter().map(|r| r.split(' ').next().unwrap()).collect();
    assert_eq!(names.len(), ALL.len(), "one sample per request");
    assert_eq!(ALL.len(), 87, "the protocol's requests");
    for request in ALL {
        let (c, text) = ask(&mut sai, request);
        assert_ne!(c, Code::BadRequest, "{request}: {text}");
        assert_ne!(c, Code::Failure, "{request}");
        match c {
            Code::Ok => assert!(text.is_empty() || text.ends_with('\n'), "{request}: {text:?}"),
            Code::Absent => assert!(text.is_empty(), "{request}"),
            _ => assert!(!text.is_empty() && !text.contains('\n'), "{request}: {text:?}"),
        }
    }
}

#[test]
fn the_test_port_leaves_the_model_and_armed_faults_alone() {
    let (mut sai, switch) = core();
    let port = RawOid(port_oid(1));
    // a fault armed for the next admin change, and one for the next VLAN creation
    chip(&mut sai)
        .fail_nth("port_admin_set", 1, sai_core::Error::Failure)
        .unwrap();
    chip(&mut sai)
        .fail_nth("vlan_create", 1, sai_core::Error::TableFull)
        .unwrap();
    let before_len = sai.store().unwrap().len();
    let before_entries = sai.store().unwrap().entry_count();
    let before_admin = sai
        .get(sys::SAI_OBJECT_TYPE_PORT, port, &[sys::SAI_PORT_ATTR_ADMIN_STATE])
        .unwrap();
    // every request, the hooks included, and the chip-only table changes
    for request in ALL {
        let _ = ask(&mut sai, request);
    }
    for request in [
        "backend_host_insert 0 4/0a000001 100002",
        "backend_l2_insert 00:11:22:33:44:55 1 5",
        "inject_fdb 1 00:11:22:33:44:66 1 5",
        "inject_link 1 1",
        "port_forget_quirks 1",
    ] {
        ok(&mut sai, request);
    }
    let store = sai.store().unwrap();
    assert_eq!((store.len(), store.entry_count()), (before_len, before_entries));
    store.check_invariants().unwrap();
    assert_eq!(
        sai.get(sys::SAI_OBJECT_TYPE_PORT, port, &[sys::SAI_PORT_ATTR_ADMIN_STATE])
            .unwrap(),
        before_admin
    );
    // the faults are still armed: the test port's calls did not consume them
    let up = Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true));
    assert_eq!(
        sai.set(sys::SAI_OBJECT_TYPE_PORT, port, &up),
        Err(sai_core::Error::Failure)
    );
    assert_eq!(sai.set(sys::SAI_OBJECT_TYPE_PORT, port, &up), Ok(()));
    let vlan = [Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(300))];
    assert_eq!(
        sai.create(sys::SAI_OBJECT_TYPE_VLAN, switch, &vlan),
        Err(sai_core::Error::TableFull)
    );
    assert!(sai.create(sys::SAI_OBJECT_TYPE_VLAN, switch, &vlan).is_ok());
}

#[test]
fn object_ids_decode_as_the_library_does() {
    let (mut sai, switch) = core();
    assert_eq!(
        val(&mut sai, &format!("oid_type {}", switch.0)),
        sys::SAI_OBJECT_TYPE_SWITCH.to_string()
    );
    let port = port_oid(13);
    assert_eq!(RawOid(port).sai_object_type(), sys::SAI_OBJECT_TYPE_PORT);
    assert_eq!(val(&mut sai, &format!("oid_index {port}")), "13");
    assert_eq!(
        val(&mut sai, &format!("make_oid {} 13 0", sys::SAI_OBJECT_TYPE_PORT)),
        sai_core::oid::PortId::try_from(RawOid(port))
            .unwrap()
            .raw()
            .0
            .to_string()
    );
}

//! The switch and port areas against the C++ test suite's checks (`sai-openbcm/tests/test_sai.cpp`:
//! `test_switch_create`, `test_ports`, `test_port_quirks`, `test_capabilities` (switch and port
//! rows), `test_capacity`, `test_temperature`), over the virtual backend chosen as the C++ names it
//! (`SAI_OPENBCM_BACKEND=fake`: the C++ fake's port numbering); failure injection (a failed backend
//! call leaves model and chip as they were); the backend call log of the C++ oracle's
//! `tests/driver-scripts/01-switch.ops` / `02-ports.ops` (`frontend_driver`) for the same calls.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::similar_names,
    missing_docs
)]
use sai_core::Error;
use sai_core::backend::types::{Dest, Fec, Loopback, MacAddr, PortInterface, PortStat, RouteTarget, Vrf};
use sai_core::backend::{Backend, CallLog, HwPort, L2, L3, Mode, Profile, Recording, Vid};
use sai_core::caps;
use sai_core::error::AttrError;
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, IpPrefix, Pointer, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;

const SWITCH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SWITCH;
const PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_PORT;

fn profile(extra: &[(&str, &str)]) -> Profile {
    Profile::from_pairs(
        [
            ("SAI_OPENBCM_BACKEND", "fake"),
            ("SAI_OPENBCM_RC_FILE", "/nonexistent.soc"),
        ]
        .into_iter()
        .chain(extra.iter().copied()),
    )
}

fn init_attrs() -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
            Value::Pointer(Pointer::from_addr(0x1000)),
        ),
        Attr::new(
            sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY,
            Value::Pointer(Pointer::from_addr(0x2000)),
        ),
    ]
}

fn create<B: Backend>(backend: B, extra: &[(&str, &str)]) -> (Sai<B>, RawOid) {
    let mut sai = Sai::new(backend);
    let id = sai.create_switch(&init_attrs(), &profile(extra)).unwrap();
    (sai, id.raw())
}

fn get<B: Backend>(
    sai: &Sai<B>,
    ot: sys::sai_object_type_t,
    oid: RawOid,
    attr: sys::sai_attr_id_t,
) -> Result<Value, Error> {
    sai.get(ot, oid, &[attr]).map(|mut v| v.remove(0))
}

fn oid(v: Value) -> RawOid {
    match v {
        Value::Oid(o) => o,
        other => panic!("{other:?}"),
    }
}

fn u32_of(v: Value) -> u32 {
    match v {
        Value::U32(n) => n,
        other => panic!("{other:?}"),
    }
}

fn ports<B: Backend>(sai: &Sai<B>, sw: RawOid) -> Vec<RawOid> {
    match get(sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap() {
        Value::OidList(l) => l,
        other => panic!("{other:?}"),
    }
}

fn not_supported(i: usize) -> Error {
    Error::attr(AttrError::NotSupported, i)
}

fn invalid_value(i: usize) -> Error {
    Error::attr(AttrError::InvalidValue, i)
}

fn enumerator(v: u32) -> Value {
    Value::Enum(v.cast_signed())
}

/// Applies the events the backend raised; the notifications.
fn drain<B: Backend>(
    sai: &mut Sai<B>,
    rx: &std::sync::mpsc::Receiver<sai_core::backend::BackendEvent>,
) -> Vec<Notification> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.extend(sai.handle_event(e));
    }
    out
}

#[test]
fn switch_create_as_the_cpp_tests_it() {
    let (mut sai, sw) = create(VirtualBackend::new(), &[]);
    assert_eq!(
        sai.store()
            .unwrap()
            .get::<sai_core::model::Switch>(sai.switch_id().unwrap())
            .unwrap()
            .src_mac,
        [2, 0, 0, 0, 0, 1]
    );
    assert_eq!(RawOid(sw.0).switch_id(), sw); // the switch object's index is 1 (C++ make_oid(SWITCH, 1))
    assert_eq!(sw.0 & 0xffff_ffff_ffff, 1);
    // one switch only
    assert_eq!(
        sai.create_switch(&init_attrs(), &profile(&[])),
        Err(Error::InsufficientResources)
    );
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_NUMBER),
        Ok(Value::U32(32))
    );
    let list = ports(&sai, sw);
    assert_eq!(list.len(), 32);
    assert!(list.iter().all(|p| p.sai_object_type() == PORT));
    for (attr, ot) in [
        (sys::SAI_SWITCH_ATTR_CPU_PORT, PORT),
        (
            sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID,
            sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER,
        ),
        (sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID, sys::SAI_OBJECT_TYPE_VLAN),
        (sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID, sys::SAI_OBJECT_TYPE_BRIDGE),
        (sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, sys::SAI_OBJECT_TYPE_STP),
        (
            sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP,
            sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP,
        ),
    ] {
        assert_eq!(
            oid(get(&sai, SWITCH, sw, attr).unwrap()).sai_object_type(),
            ot,
            "{attr}"
        );
    }
    // multi-attribute get: the failing index is in the status
    assert_eq!(
        sai.get(
            SWITCH,
            sw,
            &[sys::SAI_SWITCH_ATTR_PORT_NUMBER, sys::SAI_SWITCH_ATTR_ECMP_HASH]
        ),
        Err(not_supported(1))
    );
    let mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
    sai.set(
        SWITCH,
        sw,
        &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)),
    )
    .unwrap();
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS),
        Ok(Value::Mac(mac))
    );
    sai.set(
        SWITCH,
        sw,
        &Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(600)),
    )
    .unwrap();
    assert_eq!(sai.backend().age_seconds(), 600);
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_FDB_AGING_TIME),
        Ok(Value::U32(600))
    );
    // stubbed: stored
    let seed = Attr::new(sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED, Value::U32(1234));
    assert_eq!(get(&sai, SWITCH, sw, seed.id), Ok(Value::U32(0)));
    sai.set(SWITCH, sw, &seed).unwrap();
    assert_eq!(get(&sai, SWITCH, sw, seed.id), Ok(Value::U32(1234)));
    // read-only: ATTR_NOT_SUPPORTED (as the C++); a create-only stub: INVALID_ATTR_VALUE
    let hash = Attr::new(sys::SAI_SWITCH_ATTR_ECMP_HASH, Value::Oid(RawOid::NULL));
    assert_eq!(sai.set(SWITCH, sw, &hash), Err(not_supported(0)));
    assert_eq!(
        sai.refused_set(SWITCH, sw, sys::SAI_SWITCH_ATTR_MAX_TEMP),
        Some(not_supported(0))
    );
    assert_eq!(
        sai.refused_set(SWITCH, sw, sys::SAI_SWITCH_ATTR_SWITCH_ID),
        Some(invalid_value(0))
    );
    // C++: INVALID_OBJECT_ID; here the id's type (none) is checked first
    assert_eq!(
        get(&sai, SWITCH, RawOid(0x1234), sys::SAI_SWITCH_ATTR_PORT_NUMBER),
        Err(Error::InvalidObjectId)
    );
    // the notification callbacks are kept, the others answer null
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY),
        Ok(Value::Pointer(Pointer::from_addr(0x1000)))
    );
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_BFD_SESSION_STATE_CHANGE_NOTIFY),
        Ok(Value::Pointer(Pointer::NULL))
    );
    sai.set(
        SWITCH,
        sw,
        &Attr::new(
            sys::SAI_SWITCH_ATTR_NAT_EVENT_NOTIFY,
            Value::Pointer(Pointer::from_addr(7)),
        ),
    )
    .unwrap();
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_NAT_EVENT_NOTIFY),
        Ok(Value::Pointer(Pointer::from_addr(7)))
    );
    assert_eq!(
        sai.switch_callback(sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY),
        Pointer::from_addr(0x1000)
    );
    // an attribute the switch does not have
    assert_eq!(get(&sai, SWITCH, sw, 0x7fff_ffff), Err(not_supported(0)));
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_SWITCH_HARDWARE_INFO),
        Ok(Value::I8List(Vec::new()))
    );
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_MAX_MTU),
        Ok(Value::U32(9408))
    );
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_QOS_MAX_NUMBER_OF_TRAFFIC_CLASSES),
        Ok(Value::U8(8))
    );
    // the fake ignores SAI_OPENBCM_RC_FILE (openbcm runs it)
    assert!(sai.backend().shell_log().is_empty());
    sai.remove(SWITCH, sw).unwrap();
    assert!(sai.switch_id().is_none());
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_PORT_NUMBER),
        Err(Error::Uninitialized)
    );
}

#[test]
fn create_refusals_follow_the_cpp_order() {
    let mut sai = Sai::new(VirtualBackend::new());
    let init = |on| Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(on));
    assert_eq!(
        sai.create_switch(&[], &profile(&[])),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        sai.create_switch(&[init(false)], &profile(&[])),
        Err(Error::NotSupported)
    );
    let read_only = Attr::new(sys::SAI_SWITCH_ATTR_PORT_NUMBER, Value::U32(4));
    assert_eq!(
        sai.create_switch(&[init(true), read_only], &profile(&[])),
        Err(not_supported(1))
    );
    // the attributes are checked before the quirk keys
    assert_eq!(
        sai.create_switch(&[init(false)], &profile(&[("SAI_OPENBCM_PORT_FEC", "on")])),
        Err(Error::NotSupported)
    );
    assert!(sai.switch_id().is_none());
    assert_eq!(sai_core::switch::precheck(&[init(true)], &profile(&[])), Ok(()));
    // FDB_AGING_TIME at create goes to the chip
    let aging = Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(300));
    sai.create_switch(&[init(true), aging], &profile(&[])).unwrap();
    assert_eq!(sai.backend().age_seconds(), 300);
}

#[test]
fn ports_as_the_cpp_tests_them() {
    let (mut sai, sw) = create(VirtualBackend::new(), &[]);
    let rx = sai.take_events().unwrap();
    let list = ports(&sai, sw);
    let p0 = list[0];
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_HW_LANE_LIST),
        Ok(Value::U32List(vec![1, 2, 3, 4]))
    );
    // Safety rule: ports start admin-down, in SAI and in the backend.
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_ADMIN_STATE),
        Ok(Value::Bool(false))
    );
    for (i, _) in list.iter().enumerate() {
        let hw = HwPort(u16::try_from(1 + 4 * i).unwrap());
        assert!(!sai.backend().port_state(hw).unwrap().admin);
    }
    let speed = |v| Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(v));
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_SPEED), Ok(Value::U32(100_000)));
    sai.set(PORT, p0, &speed(40_000)).unwrap();
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_SPEED), Ok(Value::U32(40_000)));
    assert_eq!(sai.set(PORT, p0, &speed(25_000)), Err(not_supported(0)));
    assert_eq!(sai.set(PORT, p0, &speed(0)), Err(invalid_value(0)));
    sai.set(PORT, p0, &speed(100_000)).unwrap();
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_SUPPORTED_SPEED),
        Ok(Value::U32List(vec![40_000, 100_000]))
    );
    let mtu = |v| Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(v));
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_MTU), Ok(Value::U32(1514)));
    sai.set(PORT, p0, &mtu(9100)).unwrap();
    assert_eq!(sai.backend().port_state(HwPort(1)).unwrap().frame_max, 9108);
    assert_eq!(sai.set(PORT, p0, &mtu(20_000)), Err(invalid_value(0)));
    assert_eq!(sai.set(PORT, p0, &mtu(67)), Err(invalid_value(0)));
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_MTU), Ok(Value::U32(9100)));
    sai.set(
        PORT,
        p0,
        &Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, enumerator(sys::SAI_PORT_FEC_MODE_RS)),
    )
    .unwrap();
    assert_eq!(sai.backend().port_state(HwPort(1)).unwrap().fec, Fec::Rs);
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_FEC_MODE),
        Ok(enumerator(sys::SAI_PORT_FEC_MODE_RS))
    );
    assert_eq!(
        sai.set(PORT, p0, &Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, Value::Enum(99))),
        Err(invalid_value(0))
    );
    sai.set(
        PORT,
        p0,
        &Attr::new(sys::SAI_PORT_ATTR_AUTO_NEG_MODE, Value::Bool(true)),
    )
    .unwrap();
    assert!(sai.backend().port_state(HwPort(1)).unwrap().autoneg);
    // Admin up: link up in the fake, one notification.
    let oper = |sai: &Sai<VirtualBackend>, p| get(sai, PORT, p, sys::SAI_PORT_ATTR_OPER_STATUS).unwrap();
    assert_eq!(oper(&sai, p0), enumerator(sys::SAI_PORT_OPER_STATUS_DOWN));
    let admin = |up| Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up));
    sai.set(PORT, p0, &admin(true)).unwrap();
    assert!(sai.backend().port_state(HwPort(1)).unwrap().admin);
    let p0id = sai_core::oid::PortId::try_from(p0).unwrap();
    assert_eq!(
        drain(&mut sai, &rx),
        [Notification::PortOperStatus { port: p0id, up: true }]
    );
    assert_eq!(oper(&sai, p0), enumerator(sys::SAI_PORT_OPER_STATUS_UP));
    sai.backend_mut().inject_link(HwPort(1), false).unwrap(); // cable pulled
    assert_eq!(
        drain(&mut sai, &rx),
        [Notification::PortOperStatus { port: p0id, up: false }]
    );
    sai.set(PORT, p0, &admin(false)).unwrap();
    drain(&mut sai, &rx);
    // Loopback on an admin-up port brings the link up.
    let lb = Attr::new(
        sys::SAI_PORT_ATTR_LOOPBACK_MODE,
        enumerator(sys::SAI_PORT_LOOPBACK_MODE_MAC),
    );
    sai.set(PORT, list[1], &lb).unwrap();
    assert_eq!(sai.backend().port_state(HwPort(5)).unwrap().lb, Loopback::Mac);
    assert_eq!(get(&sai, PORT, list[1], lb.id), Ok(lb.value.clone()));
    let pvid = |v| Attr::new(sys::SAI_PORT_ATTR_PORT_VLAN_ID, Value::U16(v));
    assert_eq!(sai.set(PORT, p0, &pvid(4095)), Err(invalid_value(0)));
    sai.set(PORT, p0, &pvid(10)).unwrap();
    assert_eq!(sai.backend().port_state(HwPort(1)).unwrap().pvid, Vid::new(10).unwrap());
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_PORT_VLAN_ID), Ok(Value::U16(10)));
    // CPU port
    let cpu = oid(get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    assert_eq!(
        get(&sai, PORT, cpu, sys::SAI_PORT_ATTR_TYPE),
        Ok(enumerator(sys::SAI_PORT_TYPE_CPU))
    );
    assert_eq!(sai.set(PORT, cpu, &admin(false)), Err(not_supported(0)));
    assert_eq!(get(&sai, PORT, cpu, sys::SAI_PORT_ATTR_SPEED), Err(not_supported(0)));
    assert_eq!(
        get(&sai, PORT, cpu, sys::SAI_PORT_ATTR_OPER_STATUS),
        Ok(enumerator(sys::SAI_PORT_OPER_STATUS_UP))
    );
    // queues: 8 per port
    let queues = match get(&sai, PORT, p0, sys::SAI_PORT_ATTR_QOS_QUEUE_LIST).unwrap() {
        Value::OidList(l) => l,
        other => panic!("{other:?}"),
    };
    assert_eq!(queues.len(), 8);
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES),
        Ok(Value::U32(8))
    );
    assert_eq!(
        sai.get(
            sys::SAI_OBJECT_TYPE_QUEUE,
            queues[3],
            &[sys::SAI_QUEUE_ATTR_PORT, sys::SAI_QUEUE_ATTR_INDEX]
        ),
        Ok(vec![Value::Oid(p0), Value::U8(3)])
    );
    // counters
    sai.backend_mut().set_stat(HwPort(1), PortStat::InOctets, 1000);
    sai.backend_mut().set_stat(HwPort(1), PortStat::OutUcastPkts, 7);
    let ids = [sys::SAI_PORT_STAT_IF_IN_OCTETS, sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS];
    assert_eq!(sai.stats_get(PORT, p0, &ids, false), Ok(vec![1000, 7]));
    assert_eq!(
        sai.stats_get(PORT, p0, &[sys::SAI_PORT_STAT_PFC_0_RX_PKTS], false),
        Err(Error::NotSupported)
    );
    assert_eq!(sai.stats_get(PORT, p0, &ids, true), Err(Error::NotSupported)); // no READ_AND_CLEAR
    assert_eq!(sai.stats_clear(PORT, p0, &ids), Err(Error::NotSupported)); // no per-counter clear
    assert_eq!(sai.stats_get(PORT, cpu, &ids, false), Err(Error::InvalidObjectId));
    sai.clear_port_all_stats(p0).unwrap();
    assert_eq!(sai.stats_get(PORT, p0, &ids, false), Ok(vec![0, 0]));
    assert_eq!(sai.clear_port_all_stats(cpu), Err(Error::InvalidObjectId));
    assert_eq!(sai.stats_capability(PORT).map(|c| c.len()), Ok(15));
    // bad input
    let p999 = sai_core::oid::PortId::from_index(999).raw();
    assert_eq!(
        get(&sai, PORT, p999, sys::SAI_PORT_ATTR_SPEED),
        Err(Error::InvalidObjectId)
    );
    assert_eq!(get(&sai, PORT, p0, 0x7fff_ffff), Err(not_supported(0)));
    assert_eq!(sai.create(PORT, sw, &[]), Err(Error::NotSupported));
    assert_eq!(sai.remove(PORT, p0), Err(Error::NotSupported));
    // kept-only attributes: their C++ defaults, then what was set
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_IPG), Ok(Value::U32(96)));
    assert_eq!(
        get(&sai, PORT, p0, sys::SAI_PORT_ATTR_FULL_DUPLEX_MODE),
        Ok(Value::Bool(true))
    );
    sai.set(PORT, p0, &Attr::new(sys::SAI_PORT_ATTR_IPG, Value::U32(128)))
        .unwrap();
    assert_eq!(get(&sai, PORT, p0, sys::SAI_PORT_ATTR_IPG), Ok(Value::U32(128)));
    // read-only through set: ATTR_NOT_SUPPORTED (C++ port_set_one's default)
    assert_eq!(
        sai.set(PORT, p0, &Attr::new(sys::SAI_PORT_ATTR_OPER_STATUS, enumerator(1))),
        Err(not_supported(0))
    );
    assert_eq!(
        sai.refused_set(PORT, p0, sys::SAI_PORT_ATTR_HW_LANE_LIST),
        Some(not_supported(0))
    );
}

#[test]
fn board_quirks_as_the_cpp_tests_them() {
    let four = [
        ("SAI_OPENBCM_FAKE_PORTS", "4"),
        ("SAI_OPENBCM_FAKE_LANES", "4"),
        ("SAI_OPENBCM_FAKE_SPEED", "100000"),
    ];
    for bad in [
        ("SAI_OPENBCM_PORT_INTERFACE", "bogus"),
        ("SAI_OPENBCM_PORT_FEC", "on"),
        ("SAI_OPENBCM_PORT_AUTONEG", "0"),
        ("SAI_OPENBCM_PORT_IFG", "100"),
        ("SAI_OPENBCM_PORT_IFG", "16"),
        ("SAI_OPENBCM_PORT_IFG", "128x"),
    ] {
        let mut sai = Sai::new(VirtualBackend::new());
        let mut p = four.to_vec();
        p.push(bad);
        assert_eq!(
            sai.create_switch(&init_attrs(), &profile(&p)),
            Err(Error::InvalidParameter),
            "{bad:?}"
        );
        assert!(sai.switch_id().is_none());
    }
    let quirks = [
        ("SAI_OPENBCM_PORT_INTERFACE", "caui"),
        ("SAI_OPENBCM_PORT_FEC", "none"),
        ("SAI_OPENBCM_PORT_AUTONEG", "off"),
        ("SAI_OPENBCM_PORT_IFG", "128"),
    ];
    let with = |extra: &[(&'static str, &'static str)]| [four.as_slice(), &quirks, extra].concat();
    let (mut sai, sw) = create(VirtualBackend::new(), &with(&[]));
    let list = ports(&sai, sw);
    assert_eq!(list.len(), 4);
    for hw in [1, 5, 9, 13] {
        let p = sai.backend().port_state(HwPort(hw)).unwrap();
        assert_eq!(p.iface, Some(PortInterface::Caui));
        assert_eq!(p.ifg, 128);
        assert_eq!(p.fec, Fec::None);
        assert!(!p.autoneg && !p.admin);
        assert_eq!(p.quirk_calls, 2);
    }
    // SONiC's FEC and autoneg: accepted, returned, not programmed.
    let fec = |m| Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, enumerator(m));
    sai.set(PORT, list[0], &fec(sys::SAI_PORT_FEC_MODE_RS)).unwrap();
    assert_eq!(
        get(&sai, PORT, list[0], sys::SAI_PORT_ATTR_FEC_MODE),
        Ok(enumerator(sys::SAI_PORT_FEC_MODE_RS))
    );
    assert_eq!(sai.backend().port_state(HwPort(1)).unwrap().fec, Fec::None);
    assert_eq!(sai.set(PORT, list[0], &fec(99)), Err(invalid_value(0)));
    sai.set(
        PORT,
        list[0],
        &Attr::new(sys::SAI_PORT_ATTR_AUTO_NEG_MODE, Value::Bool(true)),
    )
    .unwrap();
    assert_eq!(
        get(&sai, PORT, list[0], sys::SAI_PORT_ATTR_AUTO_NEG_MODE),
        Ok(Value::Bool(true))
    );
    assert!(!sai.backend().port_state(HwPort(1)).unwrap().autoneg);
    // A speed change applies the quirks again (the SDK re-does the MAC setup).
    sai.set(PORT, list[0], &Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(40_000)))
        .unwrap();
    let p1 = sai.backend().port_state(HwPort(1)).unwrap();
    assert_eq!(p1.quirk_calls, 4);
    assert_eq!((p1.iface, p1.ifg), (Some(PortInterface::Caui), 128));
    for hw in [5, 9, 13] {
        assert_eq!(sai.backend().port_state(HwPort(hw)).unwrap().quirk_calls, 2);
    }
    sai.remove(SWITCH, sw).unwrap();
    // A quirk the chip refuses (a 4-lane interface on 1-lane ports) refuses the switch.
    let one_lane = [
        ("SAI_OPENBCM_FAKE_PORTS", "4"),
        ("SAI_OPENBCM_FAKE_LANES", "1"),
        ("SAI_OPENBCM_FAKE_SPEED", "25000"),
    ];
    assert_eq!(
        sai.create_switch(&init_attrs(), &profile(&[one_lane.as_slice(), &quirks].concat())),
        Err(Error::Failure)
    );
    assert!(sai.switch_id().is_none());
    assert!(sai.backend().ports().is_empty()); // shut down
    // No keys: nothing is touched.
    let sw = sai.create_switch(&init_attrs(), &profile(&one_lane)).unwrap().raw();
    for hw in [1, 2, 3, 4] {
        assert_eq!(sai.backend().port_state(HwPort(hw)).unwrap().quirk_calls, 0);
    }
    let list = ports(&sai, sw);
    sai.set(PORT, list[0], &fec(sys::SAI_PORT_FEC_MODE_RS)).unwrap();
    assert_eq!(sai.backend().port_state(HwPort(1)).unwrap().fec, Fec::Rs);
}

#[test]
fn temperatures_as_the_cpp_tests_them() {
    let (mut sai, sw) = create(VirtualBackend::new(), &[]);
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS),
        Ok(Value::U8(8))
    );
    assert_eq!(
        get(&sai, SWITCH, sw, sys::SAI_SWITCH_ATTR_TEMP_LIST),
        Ok(Value::I32List(vec![40, 42, 43, 45, 46, 48, 49, 51]))
    );
    let max_avg = [sys::SAI_SWITCH_ATTR_MAX_TEMP, sys::SAI_SWITCH_ATTR_AVERAGE_TEMP];
    assert_eq!(sai.get(SWITCH, sw, &max_avg), Ok(vec![Value::I32(51), Value::I32(45)]));
    sai.backend_mut().set_temps(vec![-55, 1205]);
    assert_eq!(sai.get(SWITCH, sw, &max_avg), Ok(vec![Value::I32(121), Value::I32(58)]));
    sai.backend_mut().set_temp_fail(true);
    assert_eq!(
        sai.get(
            SWITCH,
            sw,
            &[sys::SAI_SWITCH_ATTR_PORT_NUMBER, sys::SAI_SWITCH_ATTR_MAX_TEMP]
        ),
        Err(Error::Failure)
    );
    let set = Attr::new(sys::SAI_SWITCH_ATTR_MAX_TEMP, Value::I32(1));
    assert!(sai.set(SWITCH, sw, &set).is_err()); // read-only
}

#[test]
fn capacity_as_the_cpp_tests_it() {
    let (mut sai, sw) = create(VirtualBackend::new(), &[]);
    let g = |sai: &Sai<VirtualBackend>, a| get(sai, SWITCH, sw, a).unwrap();
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_ACL_ENTRY_MINIMUM_PRIORITY), Value::U32(0));
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY),
        Value::U32(0x7fff_ffff)
    );
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_MAX_ACL_ACTION_COUNT), Value::U32(3));
    for (stage, n) in [
        (sys::SAI_SWITCH_ATTR_ACL_STAGE_INGRESS, 3),
        (sys::SAI_SWITCH_ATTR_ACL_STAGE_EGRESS, 0),
    ] {
        match g(&sai, stage) {
            Value::AclCapability {
                action_list_mandatory,
                actions,
                ..
            } => {
                assert!(!action_list_mandatory);
                assert_eq!(actions.len(), n);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_NUMBER_OF_ECMP_GROUPS), Value::U32(4096));
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_ECMP_MEMBERS), Value::U32(64));
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_MAX_ECMP_MEMBER_COUNT), Value::U32(16384));
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_ENTRY),
        Value::U32(4096)
    );
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_NEXT_HOP_GROUP_MEMBER_ENTRY),
        Value::U32(16384)
    );
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_TOTAL_BUFFER_SIZE),
        Value::U64(20165 * 4 * 208 / 1024)
    );
    for attr in [
        sys::SAI_SWITCH_ATTR_MAX_STP_INSTANCE,
        sys::SAI_SWITCH_ATTR_AVAILABLE_SNAT_ENTRY,
        sys::SAI_SWITCH_ATTR_AVAILABLE_DNAT_ENTRY,
        sys::SAI_SWITCH_ATTR_ECMP_HASH,
        sys::SAI_SWITCH_ATTR_LAG_HASH,
    ] {
        assert_eq!(get(&sai, SWITCH, sw, attr), Err(not_supported(0)), "{attr}");
    }
    let v4 = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY));
    let v6 = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY));
    let n4 = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY));
    let n6 = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY));
    let nh = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY));
    let fdb = u32_of(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY));
    assert_eq!((v4, v6, nh, fdb), (16384, 8192, 32768, 32768));
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEXTHOP_ENTRY),
        Value::U32(nh)
    );
    // Free entries follow the chip's tables.
    let drop = sai.backend_mut().l3_egress_drop().unwrap();
    let prefix = IpPrefix::new("198.51.100.0".parse().unwrap(), "255.255.255.0".parse().unwrap()).unwrap();
    sai.backend_mut()
        .l3_route_add(Vrf::DEFAULT, prefix, RouteTarget::Egress(drop), false)
        .unwrap();
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY),
        Value::U32(v4 - 1)
    );
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY), Value::U32(v6));
    let h6: std::net::IpAddr = "2000::9".parse().unwrap();
    sai.backend_mut().l3_host_add(Vrf::DEFAULT, h6, drop).unwrap(); // an IPv6 host takes 2 entries
    sai.backend_mut()
        .l2_add(
            MacAddr([2, 0, 0, 0, 0, 9]),
            Vid::DEFAULT,
            Dest::Port(HwPort(1)),
            true,
            false,
        )
        .unwrap();
    assert_eq!(g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY), Value::U32(fdb - 1));
    assert!(n4 < nh);
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEIGHBOR_ENTRY),
        Value::U32(n4 - 2)
    );
    assert_eq!(n6, n4 / 2);
    assert_eq!(
        g(&sai, sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_NEIGHBOR_ENTRY),
        Value::U32((n4 - 2) / 2)
    );
}

/// Rows other areas answer through their hooks (until they land: `crate::qos`, A6).
fn answered_elsewhere(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> bool {
    matches!(
        (object_type, attr),
        (
            SWITCH,
            sys::SAI_SWITCH_ATTR_QOS_DSCP_TO_TC_MAP
                | sys::SAI_SWITCH_ATTR_QOS_DOT1P_TO_TC_MAP
                | sys::SAI_SWITCH_ATTR_QOS_TC_TO_QUEUE_MAP
        ) | (
            PORT,
            sys::SAI_PORT_ATTR_QOS_DSCP_TO_TC_MAP
                | sys::SAI_PORT_ATTR_QOS_DOT1P_TO_TC_MAP
                | sys::SAI_PORT_ATTR_QOS_TC_TO_QUEUE_MAP
                | sys::SAI_PORT_ATTR_QOS_TC_TO_PRIORITY_GROUP_MAP
                | sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_QUEUE_MAP
                | sys::SAI_PORT_ATTR_QOS_PFC_PRIORITY_TO_PRIORITY_GROUP_MAP
                | sys::SAI_PORT_ATTR_QOS_DEFAULT_TC
                | sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL_MODE
                | sys::SAI_PORT_ATTR_QOS_SCHEDULER_PROFILE_ID
        )
    )
}

#[test]
fn every_advertised_switch_and_port_get_works() {
    let (sai, sw) = create(VirtualBackend::new(), &[]);
    let p0 = ports(&sai, sw)[0];
    let mut asked = 0;
    for (object_type, attr, cap) in caps::attributes() {
        if !cap.get || answered_elsewhere(object_type, attr) {
            continue;
        }
        let target = match object_type {
            SWITCH => sw,
            PORT => p0,
            _ => continue,
        };
        assert!(get(&sai, object_type, target, attr).is_ok(), "{object_type} {attr}");
        asked += 1;
    }
    assert_eq!(asked, 48 + 29 - 3 - 9);
    let cap = sai.attribute_capability(PORT, sys::SAI_PORT_ATTR_ADMIN_STATE);
    assert!(cap.set && cap.get && !cap.create);
    let cap = sai.attribute_capability(PORT, sys::SAI_PORT_ATTR_PRBS_CONFIG);
    assert!(!cap.set && !cap.get);
    assert_eq!(
        sai.enum_values_capability(PORT, sys::SAI_PORT_ATTR_FEC_MODE)
            .map(|v| v.len()),
        Ok(3)
    );
    assert_eq!(caps::ATTRS.len(), 218);
    // C++ stats capability: ports READ only; RIF from the L3 area; others not supported
    assert!(
        sai.stats_capability(PORT)
            .unwrap()
            .iter()
            .all(|(_, m)| *m == sys::SAI_STATS_MODE_READ)
    );
    assert_eq!(
        sai.stats_capability(sys::SAI_OBJECT_TYPE_VLAN),
        Err(Error::NotSupported)
    );
}

#[test]
fn a_failed_backend_call_leaves_model_and_chip_as_they_were() {
    let (mut sai, sw) = create(VirtualBackend::new(), &[]);
    let p0 = ports(&sai, sw)[0];
    let hw = HwPort(1);
    let cases: [(&str, Attr, sys::sai_attr_id_t); 6] = [
        (
            "port_frame_max_set",
            Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9000)),
            sys::SAI_PORT_ATTR_MTU,
        ),
        (
            "port_fec_set",
            Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, enumerator(sys::SAI_PORT_FEC_MODE_RS)),
            sys::SAI_PORT_ATTR_FEC_MODE,
        ),
        (
            "port_autoneg_set",
            Attr::new(sys::SAI_PORT_ATTR_AUTO_NEG_MODE, Value::Bool(true)),
            sys::SAI_PORT_ATTR_AUTO_NEG_MODE,
        ),
        (
            "port_pvid_set",
            Attr::new(sys::SAI_PORT_ATTR_PORT_VLAN_ID, Value::U16(7)),
            sys::SAI_PORT_ATTR_PORT_VLAN_ID,
        ),
        (
            "port_loopback_set",
            Attr::new(
                sys::SAI_PORT_ATTR_LOOPBACK_MODE,
                enumerator(sys::SAI_PORT_LOOPBACK_MODE_PHY),
            ),
            sys::SAI_PORT_ATTR_LOOPBACK_MODE,
        ),
        (
            "port_admin_set",
            Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
            sys::SAI_PORT_ATTR_ADMIN_STATE,
        ),
    ];
    for (op, attr, read) in cases {
        let before_model = get(&sai, PORT, p0, read).unwrap();
        let before_chip = format!("{:?}", sai.backend().port_state(hw).unwrap());
        sai.backend_mut().fail_nth(op, 1, Error::Failure).unwrap();
        assert_eq!(sai.set(PORT, p0, &attr), Err(Error::Failure), "{op}");
        assert_eq!(get(&sai, PORT, p0, read).unwrap(), before_model, "{op}");
        assert_eq!(
            format!("{:?}", sai.backend().port_state(hw).unwrap()),
            before_chip,
            "{op}"
        );
    }
    // a backend NOT_SUPPORTED is the attribute's (C++ attr_backend_status)
    sai.backend_mut()
        .fail_nth("port_frame_max_set", 1, Error::NotSupported)
        .unwrap();
    assert_eq!(
        sai.set(PORT, p0, &Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9000))),
        Err(not_supported(0))
    );
    // switch: FDB aging
    let age_before = sai.backend().age_seconds();
    sai.backend_mut().fail_nth("l2_age_set", 1, Error::Failure).unwrap();
    let aging = Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(77));
    assert_eq!(sai.set(SWITCH, sw, &aging), Err(Error::Failure));
    assert_eq!(get(&sai, SWITCH, sw, aging.id), Ok(Value::U32(0)));
    assert_eq!(sai.backend().age_seconds(), age_before);
    // counters: the backend's error, nothing returned
    sai.backend_mut().fail_nth("port_stat_get", 2, Error::Failure).unwrap();
    let ids = [sys::SAI_PORT_STAT_IF_IN_OCTETS, sys::SAI_PORT_STAT_IF_OUT_OCTETS];
    assert_eq!(sai.stats_get(PORT, p0, &ids, false), Err(Error::Failure));
    // capacity: the backend's error, re-indexed
    sai.backend_mut()
        .fail_nth("table_usage", 1, Error::NotSupported)
        .unwrap();
    assert_eq!(
        sai.get(
            SWITCH,
            sw,
            &[
                sys::SAI_SWITCH_ATTR_PORT_NUMBER,
                sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY
            ]
        ),
        Err(not_supported(1))
    );
    // backend init fails: no switch, nothing left running
    let mut fresh = Sai::new(VirtualBackend::new());
    fresh.backend_mut().fail_nth("init", 1, Error::Failure).unwrap();
    assert_eq!(fresh.create_switch(&init_attrs(), &profile(&[])), Err(Error::Failure));
    assert!(fresh.switch_id().is_none());
    // a disable that fails at creation is logged only (C++)
    let mut fresh = Sai::new(VirtualBackend::new());
    fresh
        .backend_mut()
        .fail_nth("port_admin_set", 1, Error::Failure)
        .unwrap();
    assert!(fresh.create_switch(&init_attrs(), &profile(&[])).is_ok());
}

/// The backend lines of the C++ oracle (`frontend_driver` on `01-switch.ops` and `02-ports.ops`,
/// 4 ports, recorded 2026-11-22) for the same calls; the profile of `init` differs (the driver
/// passes its own keys) and is checked separately.
#[test]
fn the_call_log_is_the_cpp_librarys() {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    let log: CallLog = sai.backend().log();
    let attrs = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
            Value::Mac([2, 0x11, 0x22, 0x33, 0x44, 0x55]),
        ),
        Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(300)),
    ];
    let sw = sai
        .create_switch(&attrs, &profile(&[("SAI_OPENBCM_FAKE_PORTS", "4")]))
        .unwrap()
        .raw();
    let rx = sai.take_events().unwrap();
    let mut lines = log.take();
    assert!(
        lines
            .remove(0)
            .starts_with("init(profile={SAI_OPENBCM_BACKEND=\"fake\",SAI_OPENBCM_FAKE_PORTS=\"4\",")
    );
    let port = |hw: u32| {
        format!(
            "{{hw={hw},lanes=[{},{},{},{}],speed=100000,name=\"ce{}\",autoneg=false}}",
            hw,
            hw + 1,
            hw + 2,
            hw + 3,
            hw / 4
        )
    };
    let all = format!("ports() -> [{}]", [1, 5, 9, 13].map(port).join(","));
    let mut expected = vec![all.clone(), all];
    for hw in [1, 5, 9, 13] {
        expected.push(format!("port_admin_set(hw={hw}, up=false) -> SUCCESS"));
        expected.push(format!("port_link_get(hw={hw}) -> SUCCESS up=false"));
    }
    expected.push("vlan_members(vid=1) -> SUCCESS out=[(1,true),(5,true),(9,true),(13,true)]".to_owned());
    expected.push("l2_age_set(seconds=300) -> SUCCESS".to_owned());
    assert_eq!(lines, expected);
    let usage = "table_usage() -> SUCCESS u={v4_routes_free=16384,v6_routes_free=8192,hosts_free=16384,egress_free=32768,fdb_free=32768,ecmp_groups_free=4096,ecmp_members_free=16384}";
    let step = |sai: &Sai<Recording<VirtualBackend>>, attrs: &[sys::sai_attr_id_t]| {
        sai.get(SWITCH, sw, attrs).unwrap();
        log.take()
    };
    assert!(
        step(
            &sai,
            &[
                sys::SAI_SWITCH_ATTR_PORT_NUMBER,
                sys::SAI_SWITCH_ATTR_CPU_PORT,
                sys::SAI_SWITCH_ATTR_PORT_LIST
            ]
        )
        .is_empty()
    );
    assert_eq!(
        step(
            &sai,
            &[
                sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY,
                sys::SAI_SWITCH_ATTR_AVAILABLE_IPV6_ROUTE_ENTRY,
                sys::SAI_SWITCH_ATTR_AVAILABLE_IPV4_NEXTHOP_ENTRY,
                sys::SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY
            ]
        ),
        vec![usage; 4]
    );
    let temps =
        "asic_temperatures() -> SUCCESS cur=[400,415,430,445,460,475,490,505] peak=[405,420,435,450,465,480,495,510]";
    assert_eq!(
        step(
            &sai,
            &[
                sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS,
                sys::SAI_SWITCH_ATTR_TEMP_LIST,
                sys::SAI_SWITCH_ATTR_MAX_TEMP,
                sys::SAI_SWITCH_ATTR_AVERAGE_TEMP
            ]
        ),
        vec![temps; 4]
    );
    sai.set(
        SWITCH,
        sw,
        &Attr::new(sys::SAI_SWITCH_ATTR_FDB_AGING_TIME, Value::U32(600)),
    )
    .unwrap();
    assert_eq!(log.take(), ["l2_age_set(seconds=600) -> SUCCESS"]);
    // 02-ports.ops
    let p0 = ports(&sai, sw)[0];
    sai.get(
        PORT,
        p0,
        &[
            sys::SAI_PORT_ATTR_HW_LANE_LIST,
            sys::SAI_PORT_ATTR_SPEED,
            sys::SAI_PORT_ATTR_SUPPORTED_SPEED,
            sys::SAI_PORT_ATTR_ADMIN_STATE,
            sys::SAI_PORT_ATTR_OPER_STATUS,
            sys::SAI_PORT_ATTR_MTU,
        ],
    )
    .unwrap();
    assert_eq!(
        log.take(),
        [
            "port_speed_get(hw=1) -> SUCCESS mbps=100000",
            "port_supported_speeds(hw=1) -> SUCCESS out=[40000,100000]",
            "port_link_get(hw=1) -> SUCCESS up=false",
        ]
    );
    let set = |sai: &mut Sai<Recording<VirtualBackend>>, attr: Attr| {
        let status = sai.set(PORT, p0, &attr);
        (status, log.take())
    };
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(25_000))),
        (
            Err(not_supported(0)),
            vec!["port_speed_set(hw=1, mbps=25000) -> NOT_SUPPORTED".to_owned()]
        )
    );
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9100))),
        (
            Ok(()),
            vec!["port_frame_max_set(hw=1, bytes=9108) -> SUCCESS".to_owned()]
        )
    );
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(20_000))),
        (Err(invalid_value(0)), vec![])
    );
    assert_eq!(
        set(
            &mut sai,
            Attr::new(sys::SAI_PORT_ATTR_FEC_MODE, enumerator(sys::SAI_PORT_FEC_MODE_RS))
        ),
        (Ok(()), vec!["port_fec_set(hw=1, fec=Rs) -> SUCCESS".to_owned()])
    );
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_AUTO_NEG_MODE, Value::Bool(true))),
        (Ok(()), vec!["port_autoneg_set(hw=1, on=true) -> SUCCESS".to_owned()])
    );
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true))),
        (
            Ok(()),
            vec![
                "event link(hw=1, up=true)".to_owned(),
                "port_admin_set(hw=1, up=true) -> SUCCESS".to_owned()
            ]
        )
    );
    drain(&mut sai, &rx);
    assert_eq!(
        set(&mut sai, Attr::new(sys::SAI_PORT_ATTR_PORT_VLAN_ID, Value::U16(4095))),
        (Err(invalid_value(0)), vec![])
    );
    sai.backend_mut()
        .inner_mut()
        .set_stat(HwPort(1), PortStat::InOctets, 1000);
    sai.backend_mut()
        .inner_mut()
        .set_stat(HwPort(1), PortStat::OutUcastPkts, 7);
    let ids = [sys::SAI_PORT_STAT_IF_IN_OCTETS, sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS];
    assert_eq!(sai.stats_get(PORT, p0, &ids, false), Ok(vec![1000, 7]));
    assert_eq!(
        log.take(),
        [
            "port_stat_get(hw=1, stat=InOctets) -> SUCCESS v=1000",
            "port_stat_get(hw=1, stat=OutUcastPkts) -> SUCCESS v=7"
        ]
    );
    assert_eq!(
        sai.stats_get(PORT, p0, &[sys::SAI_PORT_STAT_PFC_0_RX_PKTS], false),
        Err(Error::NotSupported)
    );
    assert!(log.take().is_empty());
    sai.remove(SWITCH, sw).unwrap();
    assert_eq!(log.take(), ["shutdown() -> void"]);
}

/// The C++ `test_dump_and_teardown`'s publisher part through switch creation and removal.
#[test]
fn the_switch_runs_the_temperature_publisher() {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let dir = base.join(format!("sai-core-switch-temp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (out, prom) = (dir.join("temperature"), dir.join("asic.prom"));
    let keys = [
        ("SAI_OPENBCM_FAKE_PORTS", "4"),
        ("SAI_OPENBCM_FAKE_LANES", "1"),
        ("SAI_OPENBCM_FAKE_SPEED", "25000"),
        ("SAI_OPENBCM_TEMP_FILE", out.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_PROM", prom.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", "1"),
    ];
    let (mut sai, sw) = create(VirtualBackend::new(), &keys);
    let poll = |f: &dyn Fn() -> bool| {
        (0..400).any(|_| f() || (std::thread::sleep(std::time::Duration::from_millis(10)), false).1)
    };
    assert!(poll(&|| out.exists() && prom.exists()));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "50.5\n");
    assert!(
        std::fs::read_to_string(&prom)
            .unwrap()
            .contains("switch_asic_temperature_max_celsius{unit=\"0\"} 50.5\n")
    );
    sai.backend_mut().set_temp_fail(true); // a failed read: both files go
    assert!(poll(&|| !out.exists() && !prom.exists()));
    sai.backend_mut().set_temp_fail(false);
    assert!(poll(&|| out.exists()));
    sai.remove(SWITCH, sw).unwrap(); // removes the files
    assert!(!out.exists() && !prom.exists());
    // no file keys: no publisher
    let (_sai, _) = create(VirtualBackend::new(), &keys[..3]);
    std::thread::sleep(std::time::Duration::from_millis(1200));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

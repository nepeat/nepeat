//! The backend against the fake glue: bring-up, fault injection, link events, and the slice
//! through sai-core. Runs on any host and, from the test bundle, on big-endian ppc64.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, reason = "test code")]

mod common;

use common::{profile, scenario, scratch, serial};
use sai_backend_openbcm::{OpenbcmBackend, PROFILE_KEYS};
use sai_core::Error;
use sai_core::backend::{Backend, BackendEvent, EventSink, HwPort, PortInfo, Profile, Vid};
use sai_core::oid::TypedId;
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_sys as sys;
use sdk_glue::fake;
use std::sync::mpsc;
use std::time::Duration;

fn vid(v: u16) -> Vid {
    Vid::new(v).unwrap()
}

/// A backend brought up on `text`, and the receiving end of its events.
fn up(name: &str, text: &str, extra: &[(&str, &str)]) -> (OpenbcmBackend, mpsc::Receiver<BackendEvent>) {
    let mut backend = OpenbcmBackend::new();
    let (sink, events) = EventSink::channel();
    backend.init(&profile(name, text, extra), sink).unwrap();
    (backend, events)
}

/// Init must fail with `error`; afterwards nothing runs and nobody holds the event sink.
fn init_fails(profile: &Profile, error: Error) {
    let mut backend = OpenbcmBackend::new();
    let (sink, events) = EventSink::channel();
    assert_eq!(backend.init(profile, sink), Err(error));
    assert!(!fake::bcm_layer_up(), "the BCM layer is still up after a failed init");
    assert!(
        !fake::link_callback_registered(),
        "a link callback is still registered after a failed init"
    );
    assert_eq!(
        events.try_recv(),
        Err(mpsc::TryRecvError::Disconnected),
        "the event sink is still held"
    );
    assert_eq!(backend.ports(), vec![]);
    assert_eq!(backend.hardware_info(), None);
    assert_eq!(backend.port_link_get(HwPort(1)), Err(Error::Uninitialized));
}

#[test]
fn init_brings_the_chip_up_disables_ports_and_applies_the_board_settings() {
    let _g = serial();
    let trace = scratch().join("init-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let rc = scenario("led.soc", "# led program\n");
    let rc_text = rc.display().to_string();
    let (backend, events) = up(
        "init.cfg",
        &format!(
            "name BCM56960_B1\nports 2 4 100000\nport 66 129 1 10000 xe0\ntrace {}\n",
            trace.display()
        ),
        &[
            ("SAI_OPENBCM_RC_FILE", &rc_text),
            ("SAI_OPENBCM_UNIT", "0"),
            ("SAI_OPENBCM_PORT_INTERFACE", "caui"),
            ("SAI_OPENBCM_PORT_AUTONEG", "off"),
            ("SAI_OPENBCM_PORT_FEC", "none"),
            ("SAI_OPENBCM_PORT_IFG", "128"),
        ],
    );
    assert_eq!(backend.name(), "openbcm");
    assert_eq!(backend.hardware_info(), Some("BCM56960_B1"));
    assert_eq!(backend.cpu_port(), HwPort(0));
    assert_eq!(
        backend.ports(),
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
    for port in [1, 2, 66] {
        assert_eq!(fake::port_enabled(port), Some(false));
        assert_eq!(fake::port_settings(port), Some((0, 0, 128, true)));
    }
    assert!(fake::link_callback_registered());
    assert!(events.try_recv().is_err()); // nothing was up, nothing went down
    // the order on the chip: init all (+ rc), callback, then per port: disable, autoneg,
    // interface, FEC, inter-frame gap (the C++ order)
    let per_port = |p: u32| {
        format!("port {p} enable 0\nport {p} autoneg 0\nport {p} interface caui\nport {p} fec 0\nport {p} ifg 128\n")
    };
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        format!(
            "init-all unit 0 rc {rc_text}\nregister unit 0\n{}{}{}",
            per_port(1),
            per_port(2),
            per_port(66)
        )
    );
    assert_eq!(
        PROFILE_KEYS,
        [
            "SAI_INIT_CONFIG_FILE",
            "SAI_OPENBCM_RC_FILE",
            "SAI_OPENBCM_UNIT",
            "SAI_OPENBCM_PORT_INTERFACE",
            "SAI_OPENBCM_PORT_FEC",
            "SAI_OPENBCM_PORT_AUTONEG",
            "SAI_OPENBCM_PORT_IFG"
        ]
    );
    assert_eq!(sai_backend_openbcm::GLUE, "fake");
}

#[test]
fn without_board_settings_ports_are_only_disabled() {
    let _g = serial();
    let (backend, _events) = up(
        "plain.cfg",
        "ports 2 4 100000\n",
        &[("SAI_OPENBCM_PORT_FEC", ""), ("SAI_OPENBCM_RC_FILE", "")],
    );
    assert_eq!(fake::port_enabled(1), Some(false));
    assert_eq!(fake::port_settings(1), Some((-1, -1, -1, false)));
    assert_eq!(backend.ports().len(), 2);
}

#[test]
fn a_refused_profile_touches_no_chip() {
    let _g = serial();
    let trace = scratch().join("refused-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let text = format!("ports 2 4 100000\ntrace {}\n", trace.display());
    // no config file key, an empty one, a missing file, a directory
    init_fails(&Profile::default(), Error::Failure);
    init_fails(&Profile::from_pairs([("SAI_INIT_CONFIG_FILE", "")]), Error::Failure);
    let missing = scratch().join("no-such.bcm");
    init_fails(
        &Profile::from_pairs([("SAI_INIT_CONFIG_FILE", missing.display().to_string())]),
        Error::Failure,
    );
    init_fails(
        &Profile::from_pairs([("SAI_INIT_CONFIG_FILE", scratch().display().to_string())]),
        Error::Failure,
    );
    // values that do not parse
    for (key, value) in [
        ("SAI_OPENBCM_UNIT", "zero"),
        ("SAI_OPENBCM_UNIT", "-1"),
        ("SAI_OPENBCM_UNIT", "256"),
        ("SAI_OPENBCM_PORT_INTERFACE", "CAUI"),
        ("SAI_OPENBCM_PORT_FEC", "auto"),
        ("SAI_OPENBCM_PORT_AUTONEG", "0"),
        ("SAI_OPENBCM_PORT_IFG", "100"),
        ("SAI_OPENBCM_PORT_IFG", "512"),
        ("SAI_OPENBCM_PORT_IFG", "56"),
        ("SAI_OPENBCM_PORT_IFG", "96 "),
    ] {
        init_fails(&profile("refused.cfg", &text, &[(key, value)]), Error::InvalidParameter);
    }
    // the fake glue was never called: it would have written its first trace line
    assert!(!trace.exists(), "the SDK was started for a profile that was refused");
}

#[test]
fn a_failed_bring_up_leaves_nothing_running() {
    let _g = serial();
    let ports = "ports 3 4 100000\n";
    let quirks = [("SAI_OPENBCM_PORT_AUTONEG", "off"), ("SAI_OPENBCM_PORT_IFG", "128")];
    let cases: [(&str, Error); 12] = [
        ("init busy\n", Error::ObjectInUse),
        ("init unit\n", Error::InvalidParameter),
        ("init all\n", Error::Failure),
        ("init rc\n", Error::Failure),
        ("bogus line\n", Error::InvalidParameter), // the glue's OB_E_CONFIG
        ("fail register 1 -14\n", Error::InsufficientResources),
        ("fail ports 1 -11\n", Error::Failure),
        ("fail enable 1 -9\n", Error::Failure),
        ("fail enable 3 -18\n", Error::InvalidParameter),
        ("fail autoneg 2 -16\n", Error::NotSupported),
        ("fail ifg 3 -4\n", Error::InvalidParameter),
        ("cpu 70000\n", Error::Failure),
    ];
    for (fault, error) in cases {
        let text = format!("{ports}{fault}");
        init_fails(&profile("fault.cfg", &text, &quirks), error);
    }
    // a chip without front-panel ports, and a port number the SAI cannot name
    init_fails(&profile("none.cfg", "name EMPTY\n", &[]), Error::Failure);
    init_fails(&profile("big.cfg", "port 70000 1 4 100000 ce0\n", &[]), Error::Failure);
    // a wrong unit
    init_fails(
        &profile("unit.cfg", ports, &[("SAI_OPENBCM_UNIT", "1")]),
        Error::InvalidParameter,
    );

    // where the SDK had come up, the bring-up was undone: callback unregistered, then shutdown
    let trace = scratch().join("undo-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let text = format!("{ports}fail enable 2 -11\ntrace {}\n", trace.display());
    init_fails(&profile("undo.cfg", &text, &[]), Error::Failure);
    assert_eq!(fake::shutdown_calls(), 1);
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        "init-all unit 0 rc -\nregister unit 0\nport 1 enable 0\nenable ERR -11\nunregister unit 0\nunregister unit 0\nshutdown unit 0\n"
    );

    // and the next bring-up works
    let (backend, _events) = up("after.cfg", ports, &[]);
    assert_eq!(backend.ports().len(), 3);
}

#[test]
fn init_twice_is_refused_and_shutdown_allows_it_again() {
    let _g = serial();
    let (mut backend, events) = up("twice.cfg", "ports 1 4 100000\n", &[]);
    let (sink, second) = EventSink::channel();
    assert_eq!(
        backend.init(&profile("twice.cfg", "ports 1 4 100000\n", &[]), sink),
        Err(Error::Failure)
    );
    assert_eq!(second.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    // the first bring-up is untouched
    assert!(fake::bcm_layer_up());
    assert!(fake::link_callback_registered());
    assert_eq!(backend.ports().len(), 1);
    backend.shutdown();
    assert_eq!(events.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    let (sink, _third) = EventSink::channel();
    backend
        .init(&profile("twice.cfg", "ports 2 4 100000\n", &[]), sink)
        .unwrap();
    assert_eq!(backend.ports().len(), 2);
}

#[test]
fn shutdown_unregisters_the_callback_and_closes_the_event_queue() {
    let _g = serial();
    let trace = scratch().join("shutdown-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let (mut backend, events) = up(
        "down.cfg",
        &format!("ports 2 4 100000\ntrace {}\n", trace.display()),
        &[],
    );
    backend.shutdown();
    assert!(!fake::link_callback_registered());
    assert!(!fake::bcm_layer_up());
    assert_eq!(fake::shutdown_calls(), 1);
    // the closure and its sink are gone: a notifier thread reading the queue ends
    assert_eq!(
        events.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
    assert!(!fake::link_event(1, true));
    let text = std::fs::read_to_string(&trace).unwrap();
    assert!(
        text.ends_with("unregister unit 0\nunregister unit 0\nshutdown unit 0\n"),
        "{text}"
    );
    // every operation answers "uninitialized" and reaches no chip
    assert_eq!(backend.ports(), vec![]);
    assert_eq!(backend.port_admin_set(HwPort(1), true), Err(Error::Uninitialized));
    assert_eq!(backend.port_link_get(HwPort(1)), Err(Error::Uninitialized));
    assert_eq!(backend.port_speed_set(HwPort(1), 40_000), Err(Error::Uninitialized));
    assert_eq!(backend.port_speed_get(HwPort(1)), Err(Error::Uninitialized));
    assert_eq!(backend.port_supported_speeds(HwPort(1)), Err(Error::Uninitialized));
    assert_eq!(backend.vlan_create(vid(2)), Err(Error::Uninitialized));
    assert_eq!(backend.vlan_destroy(vid(2)), Err(Error::Uninitialized));
    assert_eq!(
        backend.vlan_port_add(vid(1), HwPort(1), true),
        Err(Error::Uninitialized)
    );
    assert_eq!(backend.vlan_port_remove(vid(1), HwPort(1)), Err(Error::Uninitialized));
    assert_eq!(backend.vlan_members(vid(1)), Err(Error::Uninitialized));
    assert_eq!(std::fs::read_to_string(&trace).unwrap(), text);
    backend.shutdown(); // twice is harmless
    // dropping a backend that is up shuts it down too
    let (backend, events) = up("drop.cfg", "ports 1 4 100000\n", &[]);
    drop(backend);
    assert!(!fake::bcm_layer_up());
    assert_eq!(events.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn a_link_change_on_another_thread_becomes_an_event() {
    let _g = serial();
    let (mut backend, events) = up("link.cfg", "ports 2 4 100000\nport 70 9 1 10000 xe0\n", &[]);
    let cable = std::thread::spawn(|| {
        assert!(fake::link_event(2, true));
        assert!(fake::link_event(70, true));
        assert!(fake::link_event(2, false));
    });
    cable.join().unwrap();
    let wait = Duration::from_secs(10);
    assert_eq!(
        events.recv_timeout(wait),
        Ok(BackendEvent::Link {
            hw: HwPort(2),
            up: true
        })
    );
    assert_eq!(
        events.recv_timeout(wait),
        Ok(BackendEvent::Link {
            hw: HwPort(70),
            up: true
        })
    );
    assert_eq!(
        events.recv_timeout(wait),
        Ok(BackendEvent::Link {
            hw: HwPort(2),
            up: false
        })
    );
    assert_eq!(backend.port_link_get(HwPort(70)), Ok(true));
    assert_eq!(backend.port_link_get(HwPort(2)), Ok(false));
    // disabling a port whose link is up: the chip reports the loss, on the calling thread
    backend.port_admin_set(HwPort(70), false).unwrap();
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(70),
            up: false
        })
    );
    assert!(events.try_recv().is_err());
}

#[test]
fn operations_and_error_mapping() {
    let _g = serial();
    let (mut b, events) = up(
        "ops.cfg",
        "ports 3 4 100000\nlink admin\nfail enable 6 -9\nfail link-get 2 -1\nfail speed-set 2 -16\nfail speeds 2 -2\n\
         fail vlan-create 2 -6\nfail vlan-destroy 1 -10\nfail vlan-add 2 -14\nfail vlan-remove 1 -13\nfail vlan-members 2 -17\n",
        &[],
    );
    // ports: init used 3 enable calls
    b.port_admin_set(HwPort(1), true).unwrap();
    assert_eq!(
        events.try_recv(),
        Ok(BackendEvent::Link {
            hw: HwPort(1),
            up: true
        })
    );
    assert_eq!(b.port_link_get(HwPort(1)), Ok(true));
    assert_eq!(b.port_link_get(HwPort(1)), Err(Error::Failure)); // INTERNAL
    b.port_admin_set(HwPort(1), false).unwrap();
    assert_eq!(b.port_admin_set(HwPort(1), true), Err(Error::Failure)); // TIMEOUT, the 6th
    assert_eq!(fake::port_enabled(1), Some(false));
    assert_eq!(b.port_admin_set(HwPort(9), true), Err(Error::InvalidParameter)); // PORT
    assert_eq!(b.port_link_get(HwPort(9)), Err(Error::InvalidParameter));
    assert_eq!(b.port_speed_get(HwPort(2)), Ok(100_000));
    b.port_speed_set(HwPort(2), 40_000).unwrap();
    assert_eq!(b.port_speed_set(HwPort(2), 100_000), Err(Error::NotSupported)); // UNAVAIL
    assert_eq!(b.port_speed_get(HwPort(2)), Ok(40_000));
    assert_eq!(b.port_speed_set(HwPort(2), 25_000), Err(Error::InvalidParameter)); // PARAM
    assert_eq!(b.port_supported_speeds(HwPort(2)), Ok(vec![40_000, 100_000]));
    assert_eq!(b.port_supported_speeds(HwPort(2)), Err(Error::InsufficientResources)); // MEMORY
    assert_eq!(b.port_supported_speeds(HwPort(9)), Err(Error::InvalidParameter));

    // VLANs
    assert_eq!(
        b.vlan_members(vid(1)),
        Ok(vec![(HwPort(1), true), (HwPort(2), true), (HwPort(3), true)])
    );
    assert_eq!(b.vlan_members(vid(1)), Err(Error::Uninitialized)); // INIT
    b.vlan_create(vid(100)).unwrap();
    assert_eq!(b.vlan_create(vid(101)), Err(Error::TableFull)); // FULL
    assert_eq!(b.vlan_create(vid(100)), Err(Error::ItemAlreadyExists)); // EXISTS
    assert_eq!(b.vlan_members(vid(101)), Err(Error::ItemNotFound)); // NOT_FOUND
    b.vlan_port_add(vid(100), HwPort(2), false).unwrap();
    assert_eq!(
        b.vlan_port_add(vid(100), HwPort(3), true),
        Err(Error::InsufficientResources)
    ); // RESOURCE
    b.vlan_port_add(vid(100), HwPort(3), true).unwrap();
    b.vlan_port_add(vid(100), HwPort(2), true).unwrap(); // tagging changes
    assert_eq!(b.vlan_members(vid(100)), Ok(vec![(HwPort(2), true), (HwPort(3), true)]));
    assert_eq!(b.vlan_port_remove(vid(100), HwPort(2)), Err(Error::InvalidObjectId)); // BADID
    assert_eq!(b.vlan_members(vid(100)).unwrap().len(), 2);
    b.vlan_port_remove(vid(100), HwPort(2)).unwrap();
    assert_eq!(b.vlan_destroy(vid(100)), Err(Error::ObjectInUse)); // BUSY
    assert_eq!(b.vlan_members(vid(100)), Ok(vec![(HwPort(3), true)]));
    b.vlan_destroy(vid(100)).unwrap();
    assert_eq!(b.vlan_destroy(vid(100)), Err(Error::ItemNotFound));
    assert_eq!(b.vlan_destroy(vid(1)), Err(Error::InvalidObjectId)); // the SDK keeps its default VLAN
}

#[test]
fn the_slice_through_the_core_on_this_backend() {
    let _g = serial();
    let mut sai = Sai::new(OpenbcmBackend::new());
    let prof = profile(
        "core.cfg",
        "ports 4 4 100000\nlink admin\n",
        &[("SAI_OPENBCM_PORT_AUTONEG", "off")],
    );
    let init = [Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))];
    let switch = sai.create_switch(&init, &prof).unwrap().raw();
    let events = sai.take_events().unwrap();
    let get = |sai: &Sai<OpenbcmBackend>, ty, oid, attr| sai.get(ty, oid, &[attr]).map(|mut v| v.remove(0));
    assert_eq!(
        get(
            &sai,
            sys::SAI_OBJECT_TYPE_SWITCH,
            switch,
            sys::SAI_SWITCH_ATTR_PORT_NUMBER
        ),
        Ok(Value::U32(4))
    );
    let Value::OidList(ports) = get(
        &sai,
        sys::SAI_OBJECT_TYPE_SWITCH,
        switch,
        sys::SAI_SWITCH_ATTR_PORT_LIST,
    )
    .unwrap() else {
        panic!("not a list")
    };
    assert_eq!(ports.len(), 4);
    for port in 1..=4 {
        assert_eq!(fake::port_enabled(port), Some(false));
    }
    assert_eq!(
        get(
            &sai,
            sys::SAI_OBJECT_TYPE_PORT,
            ports[1],
            sys::SAI_PORT_ATTR_HW_LANE_LIST
        ),
        Ok(Value::U32List(vec![5, 6, 7, 8]))
    );
    // the chip's default VLAN became VLAN 1 with four members
    let Value::Oid(vlan1) = get(
        &sai,
        sys::SAI_OBJECT_TYPE_SWITCH,
        switch,
        sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID,
    )
    .unwrap() else {
        panic!("not an id")
    };
    let Value::OidList(members) = get(&sai, sys::SAI_OBJECT_TYPE_VLAN, vlan1, sys::SAI_VLAN_ATTR_MEMBER_LIST).unwrap()
    else {
        panic!("not a list")
    };
    assert_eq!(members.len(), 4);

    // admin up reaches the chip; the link event comes back and becomes a notification
    let admin_up = Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true));
    sai.set(sys::SAI_OBJECT_TYPE_PORT, ports[1], &admin_up).unwrap();
    assert_eq!(fake::port_enabled(2), Some(true));
    let event = events.try_recv().unwrap();
    assert_eq!(
        event,
        BackendEvent::Link {
            hw: HwPort(2),
            up: true
        }
    );
    assert_eq!(sai.handle_event(event).len(), 1);
    assert!(
        get(
            &sai,
            sys::SAI_OBJECT_TYPE_PORT,
            ports[1],
            sys::SAI_PORT_ATTR_OPER_STATUS
        )
        .is_ok()
    );

    // a VLAN on the chip
    let vlan = sai
        .create(
            sys::SAI_OBJECT_TYPE_VLAN,
            switch,
            &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(100))],
        )
        .unwrap();
    assert_eq!(sai.backend().vlan_members(vid(100)), Ok(vec![]));
    sai.remove(sys::SAI_OBJECT_TYPE_VLAN, vlan).unwrap();
    assert_eq!(sai.backend().vlan_members(vid(100)), Err(Error::ItemNotFound));

    // removing the switch shuts the chip down
    sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, switch).unwrap();
    assert!(!fake::bcm_layer_up());
    assert!(!fake::link_callback_registered());
    assert_eq!(events.try_recv(), Err(mpsc::TryRecvError::Disconnected));

    // VLAN 1's members are taken only when the chip answers (C++ switch_build_objects): a
    // refused read leaves VLAN 1 without members, the switch is created
    let prof = profile("core2.cfg", "ports 4 4 100000\nfail vlan-members 1 -11\n", &[]);
    let switch = sai.create_switch(&init, &prof).unwrap().raw();
    sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, switch).unwrap();
    assert!(!fake::bcm_layer_up());
    assert!(!fake::link_callback_registered());
}

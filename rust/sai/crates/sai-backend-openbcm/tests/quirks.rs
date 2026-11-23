//! The board quirks and the port disabling are the SAI layer's now (sai-core `switch::build_objects`,
//! as the C++ `switch_build_objects`): through the core on this backend each front-panel port is
//! disabled and gets each quirk exactly once, in the C++ order; a bad quirk value refuses the
//! switch before the chip is touched. And the slice of the SAI API through the core on this
//! backend (ports, VLAN 1, admin state, link events, a VLAN, switch removal).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, reason = "test code")]

mod common;

use common::{own_profile, read, trace};
use sai_backend_openbcm::OpenbcmBackend;
use sai_core::Error;
use sai_core::backend::{Backend, BackendEvent, HwPort, Vid};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_sys as sys;
use sdk_glue::rust_fake::Chip;
use std::sync::mpsc::TryRecvError;

fn init() -> [Attr; 1] {
    [Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))]
}

const QUIRKS: [(&str, &str); 4] = [
    ("SAI_OPENBCM_PORT_INTERFACE", "caui"),
    ("SAI_OPENBCM_PORT_AUTONEG", "off"),
    ("SAI_OPENBCM_PORT_FEC", "none"),
    ("SAI_OPENBCM_PORT_IFG", "128"),
];

#[test]
fn the_core_disables_every_port_and_applies_each_quirk_once() {
    let chip = Chip::new();
    let (t, path) = trace();
    let mut sai = Sai::new(OpenbcmBackend::on_chip(chip.clone()));
    sai.create_switch(
        &init(),
        &own_profile(&format!("ports 2 4 100000\nport 66 129 1 10000 xe0\n{t}"), &QUIRKS),
    )
    .unwrap();
    let text = read(&path);
    for port in [1, 2, 66] {
        assert_eq!(chip.port_enabled(port), Some(false));
        assert_eq!(chip.port_settings(port), Some((0, 0, 128, true)));
        // disable, then autoneg, interface, FEC, inter-frame gap: once each
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with(&format!("port {port} ")))
            .collect();
        assert_eq!(
            lines,
            [
                format!("port {port} enable 0"),
                format!("port {port} autoneg 0"),
                format!("port {port} interface caui"),
                format!("port {port} fec 0"),
                format!("port {port} ifg 128"),
            ],
            "port {port}: {text}"
        );
    }
    // without quirks the ports are only disabled
    let chip = Chip::new();
    let (t, path) = trace();
    let mut sai = Sai::new(OpenbcmBackend::on_chip(chip.clone()));
    sai.create_switch(
        &init(),
        &own_profile(&format!("ports 2 4 100000\n{t}"), &[("SAI_OPENBCM_PORT_FEC", "")]),
    )
    .unwrap();
    for port in [1, 2] {
        assert_eq!(chip.port_settings(port), Some((-1, -1, -1, false)));
        assert_eq!(chip.port_enabled(port), Some(false));
    }
    assert_eq!(read(&path).lines().filter(|l| l.starts_with("port ")).count(), 2);
}

#[test]
fn a_bad_quirk_refuses_the_switch_before_the_chip_and_a_refused_one_tears_down() {
    for (key, value) in [
        ("SAI_OPENBCM_PORT_FEC", "on"),
        ("SAI_OPENBCM_PORT_INTERFACE", "Caui"),
        ("SAI_OPENBCM_PORT_IFG", "65"),
        ("SAI_OPENBCM_PORT_AUTONEG", "true"),
    ] {
        let chip = Chip::new();
        let mut sai = Sai::new(OpenbcmBackend::on_chip(chip.clone()));
        assert_eq!(
            sai.create_switch(&init(), &own_profile("ports 2 4 100000\n", &[(key, value)])),
            Err(Error::InvalidParameter),
            "{key}={value}"
        );
        assert!(!chip.bcm_layer_up());
        assert_eq!(chip.register_calls(), 0, "{key}={value}: the chip was touched");
    }
    // the chip refuses a quirk: the switch is not created, the chip is shut down again
    let chip = Chip::new();
    let mut sai = Sai::new(OpenbcmBackend::on_chip(chip.clone()));
    assert_eq!(
        sai.create_switch(&init(), &own_profile("ports 2 4 100000\nfail ifg 2 -11\n", &QUIRKS)),
        Err(Error::Failure)
    );
    assert!(!chip.bcm_layer_up());
    assert!(!chip.link_callback_registered());
}

#[test]
fn the_slice_through_the_core_on_this_backend() {
    let chip = Chip::new();
    let mut sai = Sai::new(OpenbcmBackend::on_chip(chip.clone()));
    let prof = own_profile("ports 4 4 100000\nlink admin\n", &[("SAI_OPENBCM_PORT_AUTONEG", "off")]);
    let switch = sai.create_switch(&init(), &prof).unwrap();
    let switch = sai_core::oid::TypedId::raw(switch);
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
        assert_eq!(chip.port_enabled(port), Some(false));
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
    assert_eq!(chip.port_enabled(2), Some(true));
    let event = events.try_recv().unwrap();
    assert_eq!(
        event,
        BackendEvent::Link {
            hw: HwPort(2),
            up: true
        }
    );
    assert_eq!(sai.handle_event(event).len(), 1);
    // a VLAN on the chip
    let vlan = sai
        .create(
            sys::SAI_OBJECT_TYPE_VLAN,
            switch,
            &[Attr::new(sys::SAI_VLAN_ATTR_VLAN_ID, Value::U16(100))],
        )
        .unwrap();
    assert_eq!(chip.vlan(100), Some(vec![]));
    sai.remove(sys::SAI_OBJECT_TYPE_VLAN, vlan).unwrap();
    assert_eq!(chip.vlan(100), None);
    assert_eq!(
        sai.backend().vlan_members(Vid::new(100).unwrap()),
        Err(Error::ItemNotFound)
    );
    // removing the switch shuts the chip down
    sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, switch).unwrap();
    assert!(!chip.bcm_layer_up());
    assert!(!chip.link_callback_registered());
    assert_eq!(events.try_recv(), Err(TryRecvError::Disconnected));
    // VLAN 1's members are taken only when the chip answers (C++ switch_build_objects)
    let prof = own_profile("ports 4 4 100000\nfail vlan-members 1 -11\n", &[]);
    let switch = sai_core::oid::TypedId::raw(sai.create_switch(&init(), &prof).unwrap());
    sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, switch).unwrap();
    assert!(!chip.bcm_layer_up());
}

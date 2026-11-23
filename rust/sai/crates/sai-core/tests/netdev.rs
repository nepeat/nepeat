//! Host interfaces of type NETDEV on the virtual backend: the model side (always), and real
//! TAP netdevs (only with `SAI_RS_PRIVILEGED_TESTS=1`, which only the NixOS VM tests set; see
//! crates/tap-netdev/tests/privileged.rs for running it by hand in a network namespace).
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
use sai_core::backend::{Backend, EventSink, HwPort, Profile};
use sai_core::oid::RawOid;
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::{NetdevMode, VirtualBackend};
use sai_sys as sys;

fn hostif(name: &str, port: RawOid) -> Vec<Attr> {
    vec![
        Attr::new(sys::SAI_HOSTIF_ATTR_TYPE, enumerator(sys::SAI_HOSTIF_TYPE_NETDEV)),
        Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(port)),
        Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(name.into())),
    ]
}

fn oper(up: bool) -> Attr {
    Attr::new(sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(up))
}

fn admin(up: bool) -> Attr {
    Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up))
}

fn ports(sai: &Sai<VirtualBackend>, switch: RawOid) -> Vec<RawOid> {
    oid_list(get1(sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap())
}

#[test]
fn a_netdev_host_interface_is_a_netif_of_the_chip_whose_carrier_is_its_oper_status() {
    let (mut sai, switch) = switch(4);
    assert_eq!(
        sai.backend().netdev_mode(),
        NetdevMode::None,
        "off unless the profile asks"
    );
    let ports = ports(&sai, switch);
    let mut attrs = hostif("Ethernet0", ports[0]);
    attrs.push(oper(true));
    let e0 = sai.create(HOSTIF, switch, &attrs).unwrap();
    let e4 = sai.create(HOSTIF, switch, &hostif("Ethernet4", ports[1])).unwrap();
    let netif = |sai: &Sai<VirtualBackend>, id| {
        sai.backend()
            .netifs()
            .get(&id)
            .map(|n| (n.name.clone(), n.port, n.carrier))
    };
    assert_eq!(netif(&sai, e0), Some(("Ethernet0".into(), HwPort(1), true)));
    assert_eq!(netif(&sai, e4), Some(("Ethernet4".into(), HwPort(2), false)));
    assert!(
        sai.backend()
            .netifs()
            .values()
            .all(|n| !n.has_kernel_netdev() && !n.wired())
    );

    sai.set(HOSTIF, e4, &oper(true)).unwrap();
    assert_eq!(netif(&sai, e4).map(|n| n.2), Some(true));
    sai.set(HOSTIF, e0, &oper(false)).unwrap();
    assert_eq!(netif(&sai, e0).map(|n| n.2), Some(false));
    assert_eq!(
        get1(&sai, HOSTIF, e0, sys::SAI_HOSTIF_ATTR_OPER_STATUS),
        Ok(Value::Bool(false))
    );

    sai.remove(HOSTIF, e0).unwrap();
    assert_eq!(netif(&sai, e0), None);
    model_and_chip_agree(&sai).unwrap();
    // the switch going takes the rest
    sai.remove(SWITCH, switch).unwrap();
    assert!(sai.backend().netifs().is_empty());
}

#[test]
fn netdev_host_interfaces_follow_the_cpp_rules() {
    let (mut sai, switch) = switch(2);
    let ports = ports(&sai, switch);
    let cpu = oid(get1(&sai, SWITCH, switch, sys::SAI_SWITCH_ATTR_CPU_PORT).unwrap());
    let before = sai.store().unwrap().len();
    // a front-panel port only
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("cpu0", cpu)),
        Err(Error::NotSupported)
    );
    // a name a netdev can have
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("", ports[0])),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet0123456", ports[0]))
            .map(|_| ()),
        Ok(())
    );
    let after_one = sai.store().unwrap().len();
    assert_eq!(after_one, before + 1);
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet01234567", ports[1])),
        Err(Error::InvalidParameter)
    );
    // one netdev per port, one port per name
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("other", ports[0])),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet0123456", ports[1])),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(sai.store().unwrap().len(), after_one);
    assert_eq!(sai.backend().netifs().len(), 1);
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn a_failed_backend_call_leaves_no_netif() {
    let (mut sai, switch) = switch(2);
    let ports = ports(&sai, switch);
    sai.backend_mut()
        .fail_next(sai_core::virt::Op::ObjectCreate, Error::InsufficientResources);
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("Ethernet0", ports[0])),
        Err(Error::InsufficientResources)
    );
    assert!(sai.backend().netifs().is_empty());
    let e0 = sai.create(HOSTIF, switch, &hostif("Ethernet0", ports[0])).unwrap();
    sai.backend_mut()
        .fail_next(sai_core::virt::Op::ObjectSet, Error::Failure);
    assert_eq!(sai.set(HOSTIF, e0, &oper(true)), Err(Error::Failure));
    assert_eq!(sai.backend().netifs().get(&e0).map(|n| n.carrier), Some(false));
    assert_eq!(
        get1(&sai, HOSTIF, e0, sys::SAI_HOSTIF_ATTR_OPER_STATUS),
        Ok(Value::Bool(false))
    );
    sai.backend_mut()
        .fail_next(sai_core::virt::Op::ObjectRemove, Error::Failure);
    assert_eq!(sai.remove(HOSTIF, e0), Err(Error::Failure));
    assert_eq!(sai.backend().netifs().len(), 1);
    model_and_chip_agree(&sai).unwrap();
}

#[test]
fn the_wire_gate_is_admin_and_link() {
    let (mut sai, switch) = switch(2);
    let ports = ports(&sai, switch);
    let passes = |sai: &Sai<VirtualBackend>| sai.backend().port_passes(HwPort(1));
    assert_eq!(passes(&sai), Some(false), "ports start admin down");
    sai.set(PORT, ports[0], &admin(true)).unwrap();
    assert_eq!(passes(&sai), Some(true), "link follows admin");
    sai.backend_mut().inject_link(HwPort(1), false).unwrap();
    assert_eq!(passes(&sai), Some(false), "cable pulled");
    sai.backend_mut().inject_link(HwPort(1), true).unwrap();
    assert_eq!(passes(&sai), Some(true));
    sai.set(PORT, ports[0], &admin(false)).unwrap();
    assert_eq!(passes(&sai), Some(false));
}

fn init(pairs: &[(&str, &str)]) -> sai_core::Result<()> {
    let (events, _rx) = EventSink::channel();
    VirtualBackend::new().init(&Profile::from_pairs(pairs.iter().copied()), events)
}

#[test]
fn the_netdev_profile_keys_are_checked_before_anything_is_made() {
    // Only error paths here: a successful "tap" init on a privileged host would be allowed to
    // make netdevs later, and unit tests never do (the privileged test below does).
    assert_eq!(init(&[("SAI_RS_VIRTUAL_NETDEV", "none")]), Ok(()));
    assert_eq!(init(&[("SAI_RS_VIRTUAL_NETDEV", "knet")]), Err(Error::InvalidParameter));
    assert_eq!(init(&[("SAI_RS_VIRTUAL_NETDEV", "TAP")]), Err(Error::InvalidParameter));
    // a wire needs the TAP
    assert_eq!(
        init(&[("SAI_RS_VIRTUAL_WIRE", "Ethernet16:eth1")]),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        init(&[
            ("SAI_RS_VIRTUAL_NETDEV", "none"),
            ("SAI_RS_VIRTUAL_WIRE", "Ethernet16:eth1")
        ]),
        Err(Error::InvalidParameter)
    );
    // malformed lists, and names no netdev can have
    for wire in [
        "Ethernet16",
        "Ethernet16:eth1,Ethernet16:eth2",
        "a:b,c:b",
        "a/b:eth1",
        "a:b c",
    ] {
        assert_eq!(
            init(&[("SAI_RS_VIRTUAL_NETDEV", "tap"), ("SAI_RS_VIRTUAL_WIRE", wire)]),
            Err(Error::InvalidParameter),
            "{wire}"
        );
    }
    assert_eq!(init(&[("SAI_RS_VIRTUAL_WIRE", "")]), Ok(()));
}

// ---- privileged: real TAP netdevs through the SAI calls

fn privileged(test: &str) -> bool {
    let on = std::env::var_os("SAI_RS_PRIVILEGED_TESTS").is_some_and(|v| v == "1");
    if !on {
        eprintln!("{test}: skipped (creates netdevs; set SAI_RS_PRIVILEGED_TESTS=1 where that is wanted)");
    }
    on
}

fn sysfs(name: &str, attr: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{name}/{attr}"))
        .ok()
        .map(|s| s.trim().to_owned())
}

fn link_up(name: &str) {
    let status = std::process::Command::new("ip")
        .args(["link", "set", "dev", name, "up"])
        .status();
    assert!(
        status.as_ref().is_ok_and(std::process::ExitStatus::success),
        "ip link set {name} up: {status:?}"
    );
}

#[test]
fn with_netdev_tap_each_host_interface_is_a_kernel_netdev() {
    if !privileged("with_netdev_tap_each_host_interface_is_a_kernel_netdev") {
        return;
    }
    // The host interface the wire leads to (eth1 in the SONiC VM test): a TAP of our own.
    let host =
        tap_netdev::Tap::create(tap_netdev::IfName::new("sairs-host").unwrap(), [0x02, 0, 0, 0, 0, 0x99]).unwrap();
    let mut sai = Sai::new(VirtualBackend::new());
    let profile = Profile::from_pairs([
        ("SAI_RS_VIRTUAL_PORTS", "2"),
        ("SAI_RS_VIRTUAL_NETDEV", "tap"),
        ("SAI_RS_VIRTUAL_WIRE", "sairs-e1:sairs-host"),
    ]);
    let switch = sai.create_switch(&init_attrs(), &profile).unwrap();
    let switch = sai_core::oid::TypedId::raw(switch);
    let mac = [0x52, 0x54, 0x00, 0xaa, 0xbb, 0x01];
    sai.set(
        SWITCH,
        switch,
        &Attr::new(sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)),
    )
    .unwrap();
    let ports = ports(&sai, switch);

    let e0 = sai.create(HOSTIF, switch, &hostif("sairs-e0", ports[0])).unwrap();
    assert_eq!(
        sysfs("sairs-e0", "address").as_deref(),
        Some("52:54:00:aa:bb:01"),
        "the switch's MAC"
    );
    assert_eq!(sysfs("sairs-e0", "tun_flags").as_deref(), Some("0x1002"));
    link_up("sairs-e0");
    assert_eq!(
        sysfs("sairs-e0", "carrier").as_deref(),
        Some("0"),
        "carrier off until OPER_STATUS"
    );
    sai.set(HOSTIF, e0, &oper(true)).unwrap();
    assert_eq!(sysfs("sairs-e0", "carrier").as_deref(), Some("1"));
    sai.set(HOSTIF, e0, &oper(false)).unwrap();
    assert_eq!(sysfs("sairs-e0", "carrier").as_deref(), Some("0"));
    // a name the kernel already has: the TAP cannot be made, nothing is stored
    let before = sai.store().unwrap().len();
    assert_eq!(
        sai.create(HOSTIF, switch, &hostif("sairs-host", ports[1])),
        Err(Error::Failure)
    );
    assert_eq!(sai.store().unwrap().len(), before);

    let e1 = sai.create(HOSTIF, switch, &hostif("sairs-e1", ports[1])).unwrap();
    let netif = sai.backend().netifs().get(&e1).unwrap();
    assert!(
        netif.has_kernel_netdev() && netif.wired(),
        "sairs-e1 is wired to sairs-host"
    );
    assert!(!sai.backend().netifs().get(&e0).unwrap().wired());

    sai.remove(HOSTIF, e0).unwrap();
    assert!(
        sysfs("sairs-e0", "ifindex").is_none(),
        "removed with its host interface"
    );
    model_and_chip_agree(&sai).unwrap();
    sai.remove(SWITCH, switch).unwrap();
    assert!(sysfs("sairs-e1", "ifindex").is_none(), "removed with the switch");
    drop(host);
}

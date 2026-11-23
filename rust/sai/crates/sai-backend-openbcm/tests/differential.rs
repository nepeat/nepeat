//! This backend on the fake glue against sai-core's virtual backend: the same random
//! operations on both must give the same answers, the same events and the same chip state,
//! except where the two chips are known to differ (listed at `expected`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, reason = "test code")]

mod common;

use common::own_profile;
use proptest::prelude::*;
use sai_backend_openbcm::OpenbcmBackend;
use sai_core::Error;
use sai_core::backend::{Backend, BackendEvent, EventSink, HwPort, Profile, Vid};
use sai_core::virt::VirtualBackend;
use sdk_glue::rust_fake::Chip;
use std::collections::BTreeSet;

const PORTS: u16 = 4;
const VIDS: [u16; 5] = [1, 2, 3, 100, 4094];

#[derive(Clone, Copy, Debug)]
enum Op {
    Admin(u16, bool),
    Speed(u16, u32),
    Create(u16),
    Destroy(u16),
    Add(u16, u16, bool),
    Remove(u16, u16),
}

fn op() -> impl Strategy<Value = Op> {
    let port = 1..=PORTS + 1; // one port that does not exist
    let vid = proptest::sample::select(VIDS.to_vec());
    // 25000 is a speed only the virtual chip's port has: kept out. 12345 nobody has.
    let speed = proptest::sample::select(vec![40_000u32, 100_000, 12_345]);
    prop_oneof![
        (port.clone(), any::<bool>()).prop_map(|(p, up)| Op::Admin(p, up)),
        (port.clone(), speed).prop_map(|(p, s)| Op::Speed(p, s)),
        vid.clone().prop_map(Op::Create),
        vid.clone().prop_map(Op::Destroy),
        (vid.clone(), port.clone(), any::<bool>()).prop_map(|(v, p, u)| Op::Add(v, p, u)),
        (vid, port).prop_map(|(v, p)| Op::Remove(v, p)),
    ]
}

fn apply(backend: &mut dyn Backend, op: Op) -> Result<(), Error> {
    let vid = |v| Vid::new(v).unwrap();
    match op {
        Op::Admin(p, up) => backend.port_admin_set(HwPort(p), up),
        Op::Speed(p, s) => backend.port_speed_set(HwPort(p), s),
        Op::Create(v) => backend.vlan_create(vid(v)),
        Op::Destroy(v) => backend.vlan_destroy(vid(v)),
        Op::Add(v, p, untagged) => backend.vlan_port_add(vid(v), HwPort(p), untagged),
        Op::Remove(v, p) => backend.vlan_port_remove(vid(v), HwPort(p)),
    }
}

/// What the SDK-backed chip answers where the virtual chip answers `virt`. The differences:
/// - removing a port that is not a member: the SDK's port-bitmap removal succeeds, the
///   virtual chip says "not found" (the core never asks: it checks its store first);
/// - the SDK refuses to destroy its default VLAN (BADID), the virtual chip destroys it;
/// - an unknown port in a VLAN that does not exist: the virtual chip looks at the port
///   first (invalid parameter), the SDK at the VLAN (not found);
/// - an unknown port otherwise: the virtual chip says invalid parameter (as the C++ fake), the
///   SDK `OB_E_PORT`, which the C++ backend (and this one) map to invalid port number.
fn expected(op: Op, virt: Result<(), Error>, vlans: &BTreeSet<u16>) -> Result<(), Error> {
    let port = match op {
        Op::Admin(p, _) | Op::Speed(p, _) | Op::Add(_, p, _) | Op::Remove(_, p) => p,
        Op::Create(_) | Op::Destroy(_) => 0,
    };
    match op {
        Op::Remove(v, p) if vlans.contains(&v) && p <= PORTS && virt == Err(Error::ItemNotFound) => Ok(()),
        Op::Remove(v, p) if vlans.contains(&v) && p > PORTS => Err(Error::InvalidPortNumber),
        Op::Destroy(1) => Err(Error::InvalidObjectId),
        Op::Add(v, p, _) if !vlans.contains(&v) && p > PORTS => Err(Error::ItemNotFound),
        _ if port > PORTS && virt == Err(Error::InvalidParameter) => Err(Error::InvalidPortNumber),
        _ => virt,
    }
}

fn state(backend: &dyn Backend) -> Vec<String> {
    let mut lines = Vec::new();
    for p in 1..=PORTS {
        lines.push(format!(
            "port {p}: link {:?} speed {:?} speeds {}",
            backend.port_link_get(HwPort(p)),
            backend.port_speed_get(HwPort(p)),
            backend.port_supported_speeds(HwPort(p)).is_ok()
        ));
    }
    for v in VIDS {
        lines.push(format!("vlan {v}: {:?}", backend.vlan_members(Vid::new(v).unwrap())));
    }
    lines
}

fn run(ops: &[Op]) -> Result<(), TestCaseError> {
    let mut virt = VirtualBackend::new();
    let (sink, virt_events) = EventSink::channel();
    let virt_profile = Profile::from_pairs([
        ("SAI_RS_VIRTUAL_PORTS", PORTS.to_string()),
        ("SAI_RS_VIRTUAL_ON_HARDWARE", "1".to_owned()),
    ]);
    virt.init(&virt_profile, sink).unwrap();
    let mut real = OpenbcmBackend::on_chip(Chip::new());
    let (sink, real_events) = EventSink::channel();
    real.init(
        &own_profile(&format!("ports {PORTS} 4 100000\nlink admin\n"), &[]),
        sink,
    )
    .unwrap();
    // the core's first step on any backend (switch creation disables every front-panel port)
    for p in 1..=PORTS {
        virt.port_admin_set(HwPort(p), false).unwrap();
        real.port_admin_set(HwPort(p), false).unwrap();
    }
    prop_assert_eq!(real.ports(), virt.ports());
    prop_assert_eq!(real.cpu_port(), virt.cpu_port());
    prop_assert_eq!(state(&real), state(&virt));

    // VLAN 1 can only leave the virtual chip; from then on the two differ on it by design
    let mut vlans: BTreeSet<u16> = BTreeSet::from([1]);
    let mut vlan1_diverged = false;
    for &op in ops {
        if vlan1_diverged && matches!(op, Op::Create(1) | Op::Destroy(1) | Op::Add(1, ..) | Op::Remove(1, ..)) {
            continue;
        }
        let v = apply(&mut virt, op);
        let r = apply(&mut real, op);
        prop_assert_eq!(r, expected(op, v, &vlans), "{:?} (virtual: {:?})", op, v);
        match (op, r) {
            (Op::Create(vid), Ok(())) => {
                vlans.insert(vid);
            }
            (Op::Destroy(vid), Ok(())) => {
                vlans.remove(&vid);
            }
            (Op::Destroy(1), Err(_)) => vlan1_diverged = true,
            _ => {}
        }
    }
    let (mut real_state, mut virt_state) = (state(&real), state(&virt));
    if vlan1_diverged {
        real_state.retain(|l| !l.starts_with("vlan 1:"));
        virt_state.retain(|l| !l.starts_with("vlan 1:"));
    }
    prop_assert_eq!(real_state, virt_state);
    let real_events: Vec<BackendEvent> = real_events.try_iter().collect();
    let virt_events: Vec<BackendEvent> = virt_events.try_iter().collect();
    prop_assert_eq!(real_events, virt_events);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn same_answers_events_and_state_as_the_virtual_backend(ops in proptest::collection::vec(op(), 0..60)) {
        run(&ops)?;
    }
}

//! Property tests: random create / set / remove sequences on the model.
//!
//! - `sequences_run_in_a_valid_order`: with a PHY that works, every sequence the model runs is
//!   legal for the state the PHY is in (a shadow of the PHY kept from the runs themselves),
//!   and the model's picture of every engine equals that shadow after every operation.
//!   Flows may move between ports here.
//! - `failures_keep_the_model_sound`: with sequences failing at random, the invariants hold
//!   after every operation, a failed create leaves the object set as it was, a removal removes,
//!   installed SAs hold no key, key parameters only ever go to sa-add sequences, and
//!   clear-on-read counters never go down.

#![allow(
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::assert_is_empty,
    clippy::format_collect,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use common::{EGR, ING, Run, World, action_oid, boolean, en, ms_table, oid, port_raw, sa_attrs, u32v, u64v};
use proptest::prelude::*;
use sai_core::Error;
use sai_core::oid::RawOid;
use sai_macsec::{AclEntryId, AclTableId, Counts, Dir, FlowId, MacsecId, MacsecPortId, PortKey, SaId, ScId, SeqRunner};
use sai_sys as sys;
use std::collections::BTreeMap;

fn config(cases: u32) -> ProptestConfig {
    // No regression files: the source tree is read-only in the nix build.
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn e(v: u32) -> i32 {
    i32::try_from(v).unwrap()
}

/// One operation as numbers; [`Sim::apply`] gives them meaning (indices are taken modulo
/// the number of live objects, so most operations hit something).
type Op = (u8, u8, u8, u8, u16);

fn ops(max: usize) -> impl Strategy<Value = Vec<Op>> {
    // the first operations build a working port so that the random ones have something to do
    proptest::collection::vec((0..30u8, any::<u8>(), any::<u8>(), any::<u8>(), any::<u16>()), 1..max)
}

#[derive(Default)]
struct Live {
    macsec: Vec<MacsecId>,
    mports: Vec<MacsecPortId>,
    flows: Vec<FlowId>,
    tables: Vec<AclTableId>,
    entries: Vec<AclEntryId>,
    scs: Vec<ScId>,
    sas: Vec<SaId>,
}

fn pick<T: Copy>(v: &[T], i: u8) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[usize::from(i) % v.len()])
    }
}

struct Sim {
    w: World,
    live: Live,
    /// (SA, statistic) to the last value read, while the PHY clears on read
    last_stats: BTreeMap<(RawOid, u32), u64>,
}

/// What an operation was and how it ended.
enum Done {
    Create(Result<(), Error>),
    Remove,
    Other,
}

impl Sim {
    fn new(gates: bool) -> Self {
        let mut w = World::new(&ms_table(gates, true)).unwrap();
        for phy in [0x10, 0x11] {
            for r in [0x20f, 0x21f] {
                w.phy.reg(phy, r, 1);
            }
        }
        w.phy.log_take();
        w.phy.runs_take();
        Self {
            w,
            live: Live::default(),
            last_stats: BTreeMap::new(),
        }
    }

    fn forget<T: PartialEq>(v: &mut Vec<T>, id: T, r: Result<(), Error>) {
        if r != Err(Error::ObjectInUse) {
            v.retain(|x| *x != id);
        }
    }

    fn apply(&mut self, (kind, a, b, c, d): Op) -> Done {
        let (w, live) = (&mut self.w, &mut self.live);
        let dir = if a % 2 == 1 { ING } else { EGR };
        let rdir = if a % 2 == 1 { Dir::Ingress } else { Dir::Egress };
        match kind {
            0 => {
                let r = w.m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, dir)]);
                Done::Create(r.map(|id| live.macsec.push(id)))
            }
            1 | 2 => {
                let port = port_raw(w.port(usize::from(b % 4)));
                let attrs = [
                    en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir),
                    oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port),
                ];
                let r = w.m.create_macsec_port(&mut w.phy, &attrs);
                Done::Create(r.map(|id| live.mports.push(id)))
            }
            3 => {
                let r = w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, dir)]);
                Done::Create(r.map(|id| live.flows.push(id)))
            }
            4 => {
                let stage = if dir == ING {
                    sys::SAI_ACL_STAGE_INGRESS_MACSEC
                } else {
                    sys::SAI_ACL_STAGE_EGRESS_MACSEC
                };
                let r = w.m.create_acl_table(&[en(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage))]);
                Done::Create(r.map(|id| live.tables.push(id)))
            }
            5 | 6 => {
                let table = if c % 5 == 0 {
                    RawOid::NULL
                } else {
                    pick(&live.tables, c).map_or(RawOid::NULL, AclTableId::raw)
                };
                let port = w.port(usize::from(b % 3));
                let _ = w.m.set_port_acl(&mut w.phy, port, rdir, table);
                Done::Other
            }
            7 | 8 => {
                let Some(table) = pick(&live.tables, a) else {
                    return Done::Other;
                };
                let mut attrs = vec![
                    oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, table.raw()),
                    boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, b % 4 != 0),
                ];
                if let Some(flow) = pick(&live.flows, c).filter(|_| c % 3 != 0) {
                    attrs.push(action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, flow.raw()));
                }
                let r = w.m.create_acl_entry(&mut w.phy, &attrs);
                Done::Create(r.map(|id| live.entries.push(id)))
            }
            9 | 10 => {
                let Some(entry) = pick(&live.entries, a) else {
                    return Done::Other;
                };
                let flow = pick(&live.flows, b).filter(|_| c % 4 != 0);
                let attr = action_oid(
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                    flow.is_some(),
                    flow.map_or(RawOid::NULL, FlowId::raw),
                );
                let _ = w.m.set_acl_entry(&mut w.phy, entry, &attr);
                Done::Other
            }
            11 => {
                let Some(entry) = pick(&live.entries, a) else {
                    return Done::Other;
                };
                let _ = w.m.set_acl_entry(
                    &mut w.phy,
                    entry,
                    &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, b % 2 == 0),
                );
                Done::Other
            }
            12 | 13 => {
                let Some(flow) = pick(&live.flows, b) else {
                    return Done::Other;
                };
                let cipher = i32::from(c % 4);
                let r = w.m.create_sc(&[
                    en(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir),
                    oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, flow.raw()),
                    u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, u64::from(d)),
                    en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, cipher),
                    boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, d % 2 == 0),
                    u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, u32::from(d)),
                ]);
                Done::Create(r.map(|id| live.scs.push(id)))
            }
            14 => {
                let Some(sc) = pick(&live.scs, a) else {
                    return Done::Other;
                };
                let attr = match b % 3 {
                    0 => en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, i32::from(c % 4)),
                    1 => boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, c % 2 == 0),
                    _ => u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, u32::from(d)),
                };
                let _ = w.m.set_sc(sc, &attr);
                Done::Other
            }
            15..=18 => {
                let Some(sc) = pick(&live.scs, b) else {
                    return Done::Other;
                };
                let pn = if d % 7 == 0 { 0x1_0000_0002 } else { u64::from(d) };
                let attrs = sa_attrs(dir, sc.raw(), c % 5, d % 2 == 0, true, pn);
                let r = w.m.create_sa(&mut w.phy, &attrs);
                Done::Create(r.map(|id| live.sas.push(id)))
            }
            19 => {
                let Some(sa) = pick(&live.sas, a) else {
                    return Done::Other;
                };
                let _ =
                    w.m.set_sa(sa, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, u64::from(d)));
                let _ = w.m.get_sa(&mut w.phy, sa, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN]);
                Done::Other
            }
            20 => {
                let Some(sa) = pick(&live.sas, a) else {
                    return Done::Other;
                };
                let ids = [
                    sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED,
                    sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK,
                ];
                if b % 5 == 0 {
                    if w.m.clear_sa_stats(&mut w.phy, sa).is_ok() {
                        self.last_stats.retain(|(id, _), _| *id != sa.raw());
                    }
                } else if let Ok(values) = w.m.get_sa_stats(&mut w.phy, sa, &ids) {
                    for (id, value) in ids.iter().zip(values) {
                        let last = self.last_stats.insert((sa.raw(), *id), value).unwrap_or(0);
                        assert!(value >= last, "a clear-on-read counter went down: {last} -> {value}");
                    }
                }
                Done::Other
            }
            21 => {
                let Some(flow) = pick(&live.flows, a) else {
                    return Done::Other;
                };
                let ids = [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS];
                if b % 5 == 0 {
                    if w.m.clear_flow_stats(&mut w.phy, flow).is_ok() {
                        self.last_stats.retain(|(id, _), _| *id != flow.raw());
                    }
                } else if let Ok(values) = w.m.get_flow_stats(&mut w.phy, flow, &ids) {
                    let last = self.last_stats.insert((flow.raw(), ids[0]), values[0]).unwrap_or(0);
                    assert!(
                        values[0] >= last,
                        "a clear-on-read counter went down: {last} -> {}",
                        values[0]
                    );
                }
                Done::Other
            }
            22..=24 => {
                let Some(sa) = pick(&live.sas, a) else {
                    return Done::Other;
                };
                let r = w.m.remove_sa(&mut w.phy, sa);
                assert_eq!(
                    w.m.sa_installed(sa),
                    None,
                    "a removed SA is gone whatever the sequence did"
                );
                Self::forget(&mut live.sas, sa, r);
                Done::Remove
            }
            25 => {
                match b % 6 {
                    0 => pick(&live.macsec, a).map(|id| Self::forget(&mut live.macsec, id, w.m.remove_macsec(id))),
                    1 => pick(&live.mports, a)
                        .map(|id| Self::forget(&mut live.mports, id, w.m.remove_macsec_port(&mut w.phy, id))),
                    2 => pick(&live.flows, a).map(|id| Self::forget(&mut live.flows, id, w.m.remove_flow(id))),
                    3 => pick(&live.tables, a).map(|id| Self::forget(&mut live.tables, id, w.m.remove_acl_table(id))),
                    4 => pick(&live.entries, a)
                        .map(|id| Self::forget(&mut live.entries, id, w.m.remove_acl_entry(&mut w.phy, id))),
                    _ => pick(&live.scs, a).map(|id| Self::forget(&mut live.scs, id, w.m.remove_sc(id))),
                };
                Done::Remove
            }
            26 | 27 => {
                // counters and packet numbers of the PHYs
                let reg = [0x400, 0x402, 0x410, 0x412, 0x4f0, 0x500, 0x504, 0x510][usize::from(b % 8)];
                w.phy.reg(0x10 + u32::from(a % 2), reg, d);
                Done::Other
            }
            _ => Done::Other,
        }
    }

    fn live_counts(&self) -> Counts {
        Counts {
            macsec: self.live.macsec.len(),
            ports: self.live.mports.len(),
            flows: self.live.flows.len(),
            scs: self.live.scs.len(),
            sas: self.live.sas.len(),
            acl_tables: self.live.tables.len(),
            acl_entries: self.live.entries.len(),
        }
    }
}

/// MACsecOrch's order for port 0, both directions: gives the random operations a port
/// that works to act on.
const PREFIX: &[Op] = &[
    (1, 0, 0, 0, 0),
    (1, 1, 0, 0, 0),
    (3, 0, 0, 0, 0),
    (3, 1, 0, 0, 0),
    (4, 0, 0, 0, 0),
    (4, 1, 0, 0, 0),
    (5, 0, 0, 2, 0),
    (5, 1, 0, 1, 0),
    (7, 0, 1, 2, 0),
    (7, 1, 1, 1, 0),
    (12, 0, 0, 0, 1),
    (12, 1, 1, 3, 2),
];

/// The properties below are not vacuous: the prefix yields a port on which SAs install, and
/// the order checker refuses what it should.
#[test]
fn the_prefix_builds_a_working_port_and_the_checker_checks() {
    let mut sim = Sim::new(true);
    for op in PREFIX {
        sim.apply(*op);
    }
    assert_eq!(
        sim.w.m.counts(),
        Counts {
            macsec: 0,
            ports: 2,
            flows: 2,
            scs: 2,
            sas: 0,
            acl_tables: 2,
            acl_entries: 2
        }
    );
    sim.w.phy.runs_take();
    assert!(matches!(sim.apply((15, 0, 0, 0, 3)), Done::Create(Ok(())))); // egress, AN 0
    assert!(matches!(sim.apply((15, 1, 1, 2, 3)), Done::Create(Ok(())))); // ingress, AN 2
    let runs = sim.w.phy.runs_take();
    assert_eq!(
        runs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        [
            "sa-add-first.egress.0.gcm-aes-128",
            "gate-open.egress",
            "sa-add-first.ingress.2.gcm-aes-xpn-256",
            "gate-open.ingress"
        ]
    );
    let mut shadow = Shadow {
        enabled: true,
        ..Shadow::default()
    };
    step(&mut shadow, &runs[0], Dir::Egress).unwrap();
    step(&mut shadow, &runs[1], Dir::Egress).unwrap();
    assert!(
        step(&mut shadow, &runs[1], Dir::Egress)
            .unwrap_err()
            .contains("nothing to open the gate for")
    );
    assert!(
        step(&mut shadow, &runs[0], Dir::Egress)
            .unwrap_err()
            .contains("the AN is taken")
    );
    let del = Run {
        name: "sa-del-last.egress.0".to_owned(),
        params: BTreeMap::new(),
        ..runs[1].clone()
    };
    assert!(
        step(&mut shadow, &del, Dir::Egress)
            .unwrap_err()
            .contains("the gate is open")
    );
    let del = Run {
        name: "sa-del.egress.1".to_owned(),
        ..del
    };
    assert!(
        step(&mut shadow, &del, Dir::Egress)
            .unwrap_err()
            .contains("not installed")
    );
}

/// What a PHY holds, one direction, kept from the sequences that ran on it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Shadow {
    enabled: bool,
    gate: bool,
    sa: [bool; 4],
    /// egress: the AN installed last
    tx: Option<usize>,
    last: String,
}

/// Checks one successful run against the shadow and applies it. `gates`: the table has gate sequences.
fn step(shadow: &mut Shadow, run: &Run, dir: Dir) -> Result<(), String> {
    let parts: Vec<&str> = run.name.split('.').collect();
    let an = || parts[2].parse::<usize>().unwrap();
    let count = shadow.sa.iter().filter(|x| **x).count();
    let was = format!("{shadow:?}");
    let bad = |why: &str| Err(format!("{}: {why} (PHY: {was})", run.name));
    match parts[0] {
        "reset" => {}
        "enable" => {
            if shadow.enabled {
                return bad("the engine is on already");
            }
            if !shadow.last.starts_with("reset.") {
                return bad("not right after reset");
            }
            *shadow = Shadow {
                enabled: true,
                ..Shadow::default()
            };
        }
        "disable" => {
            if !shadow.enabled || count != 0 || shadow.gate {
                return bad("the engine is off, or still holds SAs, or its gate is open");
            }
            shadow.enabled = false;
        }
        "sa-add" | "sa-add-first" => {
            if !shadow.enabled || shadow.sa[an()] {
                return bad("the engine is off or the AN is taken");
            }
            if (parts[0] == "sa-add-first") != (count == 0) {
                return bad("-first is for an SC without an installed SA, and only for that");
            }
            for name in ["key", "hkey", "sci", "pn", "window"] {
                if !run.params.contains_key(name) {
                    return bad("a parameter is missing");
                }
            }
            shadow.sa[an()] = true;
            if dir == Dir::Egress {
                shadow.tx = Some(an());
            }
        }
        "sa-del" | "sa-del-last" => {
            if !shadow.sa[an()] {
                return bad("the AN is not installed");
            }
            if (parts[0] == "sa-del-last") != (count == 1) {
                return bad("-last is for the SC's last SA, and only for that");
            }
            if shadow.gate && (count == 1 || shadow.tx == Some(an())) {
                return bad("the gate is open while the last / the transmitting SA goes");
            }
            shadow.sa[an()] = false;
            if shadow.tx == Some(an()) {
                shadow.tx = None;
            }
        }
        "gate-open" => {
            if !shadow.enabled || shadow.gate || count == 0 || (dir == Dir::Egress && shadow.tx.is_none()) {
                return bad("nothing to open the gate for");
            }
            shadow.gate = true;
        }
        "gate-close" => {
            if !shadow.gate {
                return bad("the gate is closed already");
            }
            shadow.gate = false;
        }
        "pn" | "stats-sa" => {
            if !shadow.sa[an()] {
                return bad("the AN is not installed");
            }
        }
        "stats-flow" => {
            if !shadow.enabled {
                return bad("the engine is off");
            }
        }
        other => return Err(format!("unexpected sequence {other}")),
    }
    if !run.params.is_empty() && !parts[0].starts_with("sa-add") {
        return bad("parameters for a sequence that takes none");
    }
    shadow.last.clone_from(&run.name);
    Ok(())
}

fn dir_of(name: &str) -> Dir {
    if name.contains(".ingress") {
        Dir::Ingress
    } else {
        Dir::Egress
    }
}

proptest! {
    #![proptest_config(config(400))]

    #[test]
    fn sequences_run_in_a_valid_order(random in ops(120)) {
        let mut sim = Sim::new(true);
        let mut shadows: BTreeMap<(PortKey, Dir), Shadow> = BTreeMap::new();
        for op in PREFIX.iter().copied().chain(random) {
            sim.apply(op);
            for run in sim.w.phy.runs_take() {
                let dir = dir_of(&run.name);
                if !run.ok {
                    // The PHY works, so the only failure is the runner refusing an sa-add whose parameters
                    // do not cover the sequence, before it touches the bus: an SA that outlived its
                    // MACSEC_PORT (Q3), or whose SC changed cipher suite while it was waiting (Q10).
                    let why = sim.w.m.engine_state(run.port, dir).unwrap().last_fail;
                    prop_assert!(run.name.starts_with("sa-add") && why.contains("is not available"), "{}: {why}", run.name);
                    continue;
                }
                step(shadows.entry((run.port, dir)).or_default(), &run, dir).map_err(TestCaseError::fail)?;
            }
            prop_assert!(sim.w.m.check_invariants().is_ok(), "{:?}", sim.w.m.check_invariants());
            // the model's picture of every engine is the PHY's state
            for key in sim.w.m.engine_ports() {
                for dir in Dir::BOTH {
                    let model = sim.w.m.engine_state(key, dir).unwrap();
                    let shadow = shadows.get(&(key, dir)).cloned().unwrap_or_default();
                    prop_assert_eq!(model.enabled, shadow.enabled);
                    prop_assert_eq!(model.gate, shadow.gate);
                    prop_assert_eq!(model.sa.map(|s| s.is_some()), shadow.sa);
                    let tx = model.tx.and_then(|tx| model.sa.iter().position(|s| *s == Some(tx)));
                    prop_assert_eq!(tx, shadow.tx);
                    prop_assert_eq!(model.fails, sim.w.m.engine_state(key, dir).unwrap().fails);
                }
            }
            prop_assert_eq!(sim.w.m.counts(), sim.live_counts());
        }
    }

    #[test]
    fn failures_keep_the_model_sound(
        random in ops(120),
        fails in proptest::collection::vec(prop_oneof![4 => Just(false), 1 => Just(true)], 0..400),
        gates in any::<bool>(),
        ifg_fails in any::<bool>(),
    ) {
        let mut sim = Sim::new(gates);
        sim.w.phy.ifg_fails = ifg_fails;
        for (n, op) in PREFIX.iter().copied().chain(random).enumerate() {
            if n == PREFIX.len() {
                sim.w.phy.fail_plan.extend(fails.iter().copied()); // from here on sequences fail at random
            }
            let before = sim.w.m.counts();
            let done = sim.apply(op);
            let after = sim.w.m.counts();
            if let Done::Create(Err(_)) = done {
                // a failed create leaves the object set as it was
                prop_assert_eq!(before, after);
            }
            prop_assert_eq!(after, sim.live_counts());
            prop_assert!(sim.w.m.check_invariants().is_ok(), "{:?}", sim.w.m.check_invariants());
            for run in sim.w.phy.runs_take() {
                // key material only ever goes to an sa-add sequence
                prop_assert!(run.params.is_empty() || run.name.starts_with("sa-add"), "{}", run.name);
            }
            for sa in &sim.live.sas {
                // an SA in a PHY holds no key in the model
                if sim.w.m.sa_installed(*sa) == Some(true) {
                    prop_assert_eq!(sim.w.m.sa_holds_key(*sa), Some(false));
                }
            }
        }
        // the log never carries key bytes (the test SAK is c0 c1 ..., the auth key 80 81 ..., the salt 50 51 ...)
        let log = sim.w.log_text();
        for needle in ["c0c1", "c1c0", "c2c3", "8081", "8f80", "5051"] {
            prop_assert!(!log.contains(needle), "{needle} in the log");
        }
        let _ = sim.w.phy.set_ifg(PortKey(1), 0);
    }
}

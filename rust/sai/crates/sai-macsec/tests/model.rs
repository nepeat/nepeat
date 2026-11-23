//! Behaviour the C++ unit test does not reach: failure and rollback, the startup probe, the
//! auth-key policy, absolute counters, the replay variants, attribute rules, the diag texts,
//! and the questionable behaviours of the C++ that this port keeps (pinned here so that a
//! change is a decision, not an accident; "Qn" / "Dn" refer to the crate documentation).

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
    reason = "test code: a failed unwrap is a failed test"
)]

mod common;

use common::{
    EGR, FakePhy, FakeTable, ING, World, action_oid, action_s32, boolean, en, ether_type, invalid_value, ms_table,
    not_supported, oid, port_raw, sa_attrs, switch_ports, test_sak, u8v, u32v, u64v,
};
use sai_core::Error;
use sai_core::oid::RawOid;
use sai_macsec::{
    AclEntryId, AclField, AclParameter, AclTableId, Attr, AuthKeyPolicy, Dir, FlowId, Level, Macsec, MacsecPortId,
    Options, Params, PortKey, SaId, ScId, Secret, Value, hkey,
};
use sai_sys as sys;

fn e(v: u32) -> i32 {
    i32::try_from(v).unwrap()
}

fn dir_of(d: i32) -> Dir {
    if d == ING { Dir::Ingress } else { Dir::Egress }
}

struct Bound {
    mport: MacsecPortId,
    flow: FlowId,
    table: AclTableId,
    entry: AclEntryId,
    sc: ScId,
}

fn good() -> World {
    let mut w = World::new(&ms_table(true, true)).unwrap();
    for phy in [0x10, 0x11] {
        for r in [0x20f, 0x21f] {
            w.phy.reg(phy, r, 1);
        }
    }
    w.phy.log_take();
    w.phy.runs_take();
    w
}

fn mport_attrs(w: &World, port: usize, dir: i32) -> [Attr; 2] {
    [
        en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir),
        oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port_raw(w.port(port))),
    ]
}

fn mport(w: &mut World, port: usize, dir: i32) -> Result<MacsecPortId, Error> {
    let attrs = mport_attrs(w, port, dir);
    w.m.create_macsec_port(&mut w.phy, &attrs)
}

fn sc_attrs(dir: i32, flow: FlowId, cipher: u32, replay: Option<u32>) -> Vec<Attr> {
    let mut v = vec![
        en(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir),
        oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, flow.raw()),
        u64v(
            sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI,
            u64::from_ne_bytes([1, 2, 3, 4, 5, 6, 7, 8]),
        ),
        en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(cipher)),
    ];
    if let Some(window) = replay {
        v.push(boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, true));
        v.push(u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, window));
    }
    v
}

fn table(w: &mut World, dir: i32) -> AclTableId {
    let stage = if dir == ING {
        sys::SAI_ACL_STAGE_INGRESS_MACSEC
    } else {
        sys::SAI_ACL_STAGE_EGRESS_MACSEC
    };
    w.m.create_acl_table(&[en(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage))])
        .unwrap()
}

fn flow_entry(w: &mut World, table: AclTableId, flow: FlowId) -> AclEntryId {
    w.m.create_acl_entry(
        &mut w.phy,
        &[
            oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, table.raw()),
            action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, flow.raw()),
        ],
    )
    .unwrap()
}

/// MACSEC_PORT on, table bound, an entry sending data to a flow, an SC: SAs install at once.
fn bound(w: &mut World, port: usize, dir: i32, cipher: u32, replay: Option<u32>) -> Bound {
    let mport = mport(w, port, dir).unwrap();
    let flow =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, dir)])
            .unwrap();
    let table = table(w, dir);
    let port_id = w.port(port);
    w.m.set_port_acl(&mut w.phy, port_id, dir_of(dir), table.raw()).unwrap();
    let entry = flow_entry(w, table, flow);
    let sc = w.m.create_sc(&sc_attrs(dir, flow, cipher, replay)).unwrap();
    w.phy.log_take();
    w.phy.runs_take();
    Bound {
        mport,
        flow,
        table,
        entry,
        sc,
    }
}

fn sa(w: &mut World, dir: i32, sc: ScId, an: u8) -> Result<SaId, Error> {
    w.m.create_sa(&mut w.phy, &sa_attrs(dir, sc.raw(), an, false, false, 1))
}

const AES128: u32 = sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128;
const XPN256: u32 = sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256;

// ------------------------------------------------------------------ the hash key

#[test]
fn auth_key_policy_verify_refuses_a_hash_key_that_is_not_the_saks() {
    for (k256, cipher) in [(false, AES128), (true, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_256)] {
        let options = Options {
            auth_key: AuthKeyPolicy::Verify,
            log: None,
            alloc: None,
        };
        let mut w = World::with(&ms_table(true, true), options).unwrap();
        w.phy.reg(0x10, 0x20f, 1);
        let b = bound(&mut w, 0, EGR, cipher, None);
        // the C++ test's auth key (80 81 ...) is not AES(SAK, 0): refused at the attribute, nothing kept, nothing run
        let attrs = sa_attrs(EGR, b.sc.raw(), 0, k256, false, 1);
        assert_eq!(attrs[5].id, sys::SAI_MACSEC_SA_ATTR_AUTH_KEY);
        assert_eq!(w.m.create_sa(&mut w.phy, &attrs), Err(invalid_value(5)));
        assert_eq!(w.m.counts().sas, 0);
        assert!(w.phy.log_take().is_empty());
        // the right one: AES-ECB(SAK, 0^128) of the key the cipher suite uses (128 bit: the last 16 of the 32)
        let sak = test_sak(0, k256);
        let key = if k256 { &sak[..] } else { &sak[16..] };
        let mut attrs = attrs;
        attrs[5] = Attr::new(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::AuthKey(hkey::derive_hkey(key).unwrap()),
        );
        w.m.create_sa(&mut w.phy, &attrs).unwrap();
        let run = &w.phy.runs_take()[0];
        let expect: String = hkey::derive_hkey(key)
            .unwrap()
            .expose()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(run.param("hkey"), expect);
    }
}

// ------------------------------------------------------------------ counters

#[test]
fn absolute_counters_are_reported_as_read_and_cannot_be_cleared() {
    let text = ms_table(true, true).replace("counters clear-on-read", "counters absolute");
    let mut w = World::new(&text).unwrap();
    w.phy.reg(0x10, 0x20f, 1);
    let b = bound(&mut w, 0, EGR, AES128, None);
    let a = sa(&mut w, EGR, b.sc, 0).unwrap();
    let ids = [sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED];
    w.phy.reg(0x10, 0x400, 5);
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [5]);
    w.phy.reg(0x10, 0x400, 9);
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [9]);
    assert_eq!(w.m.clear_sa_stats(&mut w.phy, a), Err(Error::NotSupported));
    assert_eq!(w.m.clear_flow_stats(&mut w.phy, b.flow), Err(Error::NotSupported));
    w.phy.reg(0x10, 0x4f0, 4);
    let control = [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS];
    assert_eq!(w.m.get_flow_stats(&mut w.phy, b.flow, &control).unwrap(), [4]);
    assert_eq!(w.m.get_flow_stats(&mut w.phy, b.flow, &control).unwrap(), [4]);
}

#[test]
fn clear_on_read_counters_accumulate_and_clear() {
    let mut w = good();
    let b = bound(&mut w, 0, ING, AES128, None);
    let a = sa(&mut w, ING, b.sc, 1).unwrap();
    let ids = [sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK, sys::SAI_MACSEC_SA_STAT_IN_PKTS_LATE];
    w.phy.reg(0x10, 0x412, 7); // stats-sa.ingress.1: 1.412 / 1.413
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [7, 0]);
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [14, 0]);
    assert_eq!(
        w.m.get_sa_stats_ext(&mut w.phy, a, &ids, sys::SAI_STATS_MODE_READ)
            .unwrap(),
        [21, 0]
    );
    assert_eq!(
        w.m.get_sa_stats_ext(&mut w.phy, a, &ids, sys::SAI_STATS_MODE_READ_AND_CLEAR),
        Err(Error::NotSupported)
    );
    w.phy.log_take();
    w.m.clear_sa_stats(&mut w.phy, a).unwrap();
    assert_eq!(w.phy.log_take().len(), 2); // the PHY's counters were read once more, to empty them
    w.phy.reg(0x10, 0x412, 0);
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [0, 0]);
    // a failing stats sequence fails the read and keeps what was accumulated
    w.phy.reg(0x10, 0x412, 3);
    w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap();
    w.phy.fail_plan.push_back(true);
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids), Err(Error::Failure));
    assert_eq!(w.m.get_sa_stats(&mut w.phy, a, &ids).unwrap(), [6, 0]);
    // flow: the same, and the capability names what the table reads
    w.phy.reg(0x10, 0x4f0, 2);
    let control = [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS];
    assert_eq!(w.m.get_flow_stats(&mut w.phy, b.flow, &control).unwrap(), [2]);
    w.m.clear_flow_stats(&mut w.phy, b.flow).unwrap();
    w.phy.reg(0x10, 0x4f0, 0);
    assert_eq!(w.m.get_flow_stats(&mut w.phy, b.flow, &control).unwrap(), [0]);
    assert_eq!(
        w.m.get_flow_stats_ext(&mut w.phy, b.flow, &control, sys::SAI_STATS_MODE_READ_AND_CLEAR),
        Err(Error::NotSupported)
    );
    assert_eq!(
        w.m.stats_capability(&w.phy, sys::SAI_OBJECT_TYPE_MACSEC_FLOW).unwrap(),
        [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS]
    );
    assert_eq!(
        w.m.stats_capability(&w.phy, sys::SAI_OBJECT_TYPE_MACSEC_SC),
        Err(Error::NotSupported)
    );
    // there are no MACSEC_PORT / SC statistics
    assert_eq!(w.m.get_macsec_port_stats(b.mport, &[]).unwrap(), Vec::<u64>::new());
    assert_eq!(w.m.get_macsec_port_stats(b.mport, &[1]), Err(Error::NotSupported));
    assert_eq!(w.m.clear_macsec_port_stats(b.mport), Err(Error::NotSupported));
    assert_eq!(w.m.get_sc_stats(b.sc, &[]).unwrap(), Vec::<u64>::new());
    assert_eq!(w.m.get_sc_stats(b.sc, &[1]), Err(Error::NotSupported));
    assert_eq!(w.m.clear_sc_stats(b.sc), Err(Error::NotSupported));
}

#[test]
fn an_unbound_flow_reads_its_counters_from_the_port_its_sc_was_installed_on() {
    let mut w = good();
    let b = bound(&mut w, 1, EGR, AES128, None);
    sa(&mut w, EGR, b.sc, 0).unwrap();
    w.m.set_acl_entry(
        &mut w.phy,
        b.entry,
        &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false),
    )
    .unwrap();
    w.phy.reg(0x11, 0x4f0, 3);
    let control = [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS];
    assert_eq!(w.m.get_flow_stats(&mut w.phy, b.flow, &control).unwrap(), [3]);
    // a flow that never met a port has nothing to read
    let lone =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)])
            .unwrap();
    w.phy.log_take();
    assert_eq!(w.m.get_flow_stats(&mut w.phy, lone, &control).unwrap(), [0]);
    assert!(w.phy.log_take().is_empty());
}

// ------------------------------------------------------------------ which sequence

#[test]
fn ingress_replay_protection_picks_the_replay_variant_when_the_table_has_it() {
    let mut w = good();
    let b = bound(&mut w, 0, ING, AES128, Some(0x80));
    sa(&mut w, ING, b.sc, 0).unwrap();
    // the C++ table has exactly one .replay sequence: first, AN 0, gcm-aes-128
    assert_eq!(w.phy.writes(), "210=f00 217=80 121=1");
    let runs = w.phy.runs_take();
    assert_eq!(runs[0].name, "sa-add-first.ingress.0.gcm-aes-128.replay");
    assert_eq!(runs[0].param("window"), "80000000");
    // AN 1 is not the first and has no .replay variant: the plain one
    w.phy.reg(0x10, 0x310, 0);
    sa(&mut w, ING, b.sc, 1).unwrap();
    assert_eq!(w.phy.names_take(), ["sa-add.ingress.1.gcm-aes-128"]);
    // egress never uses a .replay variant, and its window parameter follows the SC all the same
    let be = bound(&mut w, 0, EGR, AES128, Some(0x80));
    sa(&mut w, EGR, be.sc, 0).unwrap();
    let runs = w.phy.runs_take();
    assert_eq!(runs[0].name, "sa-add-first.egress.0.gcm-aes-128");
    assert_eq!(runs[0].param("window"), "80000000");
}

#[test]
fn a_sequence_the_table_lacks() {
    // no sa-del at all: removal says NOT_SUPPORTED and the model forgets the SA (Q1)
    let text: String = {
        let mut keep = true;
        ms_table(true, true)
            .lines()
            .filter(|l| {
                if let Some(name) = l.strip_prefix("seq ") {
                    keep = !name.starts_with("sa-del") && !name.starts_with("enable.ingress");
                }
                keep
            })
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let mut w = World::new(&text).unwrap();
    w.phy.reg(0x10, 0x20f, 1);
    let b = bound(&mut w, 0, EGR, AES128, None);
    let a = sa(&mut w, EGR, b.sc, 0).unwrap();
    w.phy.log_take();
    assert_eq!(w.m.remove_sa(&mut w.phy, a), Err(Error::NotSupported));
    assert_eq!(w.phy.writes(), "120=0"); // the gate was closed first
    assert_eq!(w.m.counts().sas, 0);
    assert!(
        w.log_text()
            .contains("the external MACsec table has no sequence sa-del.egress.0")
    );
    // no enable.ingress: the MACSEC_PORT cannot be created (reset ran: it is optional and there)
    assert_eq!(mport(&mut w, 0, ING), Err(Error::NotSupported));
    assert_eq!(w.phy.writes(), "111=1");
    w.m.check_invariants().unwrap();
}

// ------------------------------------------------------------------ failure and rollback

#[test]
fn a_failed_create_leaves_the_model_as_it_was() {
    let mut w = good();
    let hw0 = w.key(0);
    // MACSEC_PORT: reset succeeds, enable fails
    w.phy.fail_plan.extend([false, true]);
    assert_eq!(mport(&mut w, 0, EGR), Err(Error::Failure));
    let state = w.m.engine_state(hw0, Dir::Egress).unwrap();
    assert!(!state.enabled);
    assert_eq!((state.runs, state.fails), (5, 1)); // 3 at attach (probe, reset, disable) + reset + enable
    assert_eq!(state.last_fail, "enable.egress: injected failure");
    assert_eq!(w.m.counts().ports, 0);
    assert_eq!(w.m.port_ifg(hw0), 0);
    assert_eq!(w.phy.ifg_calls, 0);
    // ... or reset fails: enable is not tried
    w.phy.runs_take();
    w.phy.fail_plan.push_back(true);
    assert_eq!(mport(&mut w, 0, EGR), Err(Error::Failure));
    assert_eq!(w.phy.names_take(), ["reset.egress"]);
    // SA: the install fails
    let b = bound(&mut w, 0, EGR, AES128, None);
    w.phy.fail_plan.push_back(true);
    assert_eq!(sa(&mut w, EGR, b.sc, 0), Err(Error::Failure));
    assert_eq!(w.m.counts().sas, 0);
    assert_eq!(w.m.engine_state(hw0, Dir::Egress).unwrap().sa, [None; 4]);
    assert!(!w.m.engine_state(hw0, Dir::Egress).unwrap().gate);
    // and the same SA can be created afterwards
    sa(&mut w, EGR, b.sc, 0).unwrap();
    w.m.check_invariants().unwrap();
}

/// Q1: a removal whose sequence fails still removes the object; the status is the sequence's.
#[test]
fn q1_a_failed_removal_removes_the_object_and_reports_the_failure() {
    let mut w = good();
    let hw0 = w.key(0);
    let b = bound(&mut w, 0, EGR, AES128, None);
    let a0 = sa(&mut w, EGR, b.sc, 0).unwrap();
    let a1 = sa(&mut w, EGR, b.sc, 1).unwrap();
    w.phy.runs_take();
    w.phy.fail_plan.push_back(true);
    assert_eq!(w.m.remove_sa(&mut w.phy, a0), Err(Error::Failure));
    assert_eq!(w.phy.names_take(), ["sa-del.egress.0"]);
    assert_eq!(w.m.sa_installed(a0), None);
    assert_eq!(
        w.m.engine_state(hw0, Dir::Egress).unwrap().sa,
        [None, Some(a1), None, None]
    );
    assert_eq!(w.m.remove_sa(&mut w.phy, a0), Err(Error::InvalidObjectId));
    // MACSEC_PORT: sa-del of the SA still installed fails (ignored), disable fails (reported): gone all the same
    w.phy.fail_plan.extend([false, true, true]); // gate-close, sa-del-last, disable
    assert_eq!(w.m.remove_macsec_port(&mut w.phy, b.mport), Err(Error::Failure));
    assert_eq!(
        w.phy.names_take(),
        ["gate-close.egress", "sa-del-last.egress.1", "disable.egress"]
    );
    assert_eq!(w.m.counts().ports, 0);
    assert!(!w.m.engine_state(hw0, Dir::Egress).unwrap().enabled);
    assert!(w.log_text().contains("MACsec egress off (the disable sequence FAILED)"));
    assert_eq!(w.m.port_ifg(hw0), 0);
    assert_eq!(w.phy.ifg[&hw0], 96);
    w.m.check_invariants().unwrap();
}

/// Q3: an SA that outlives its MACSEC_PORT is "pending" again, without its key (it was
/// wiped when it went into the PHY). The next reconcile tries to install it with the
/// parameters it still has; the runner refuses before touching the bus, every time.
#[test]
fn q3_an_sa_that_outlives_its_macsec_port_cannot_be_installed_again() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    let a = sa(&mut w, EGR, b.sc, 0).unwrap();
    w.m.remove_macsec_port(&mut w.phy, b.mport).unwrap();
    assert_eq!((w.m.sa_installed(a), w.m.sa_holds_key(a)), (Some(false), Some(false)));
    w.phy.log_take();
    w.phy.runs_take();
    mport(&mut w, 0, EGR).unwrap();
    assert_eq!(w.phy.writes(), "110=1 100=0"); // reset, enable; the install touched nothing
    let runs = w.phy.runs_take();
    assert_eq!(runs[2].name, "sa-add-first.egress.0.gcm-aes-128");
    assert_eq!(
        runs[2].params.keys().map(String::as_str).collect::<Vec<_>>(),
        ["sci", "window"]
    );
    let state = w.m.engine_state(w.key(0), Dir::Egress).unwrap();
    assert!(
        state.last_fail.contains("parameter key[1] is not available"),
        "{}",
        state.last_fail
    );
    assert!(
        w.m.status(&w.phy)
            .contains("SAs waiting for their flow to be bound to a port: 1")
    );
    w.m.check_invariants().unwrap();
}

/// Q4: an install that fails when the flow gets bound (nobody to report it to) is retried
/// by every later reconcile of that port, key kept until one succeeds.
#[test]
fn q4_a_failed_install_at_bind_time_is_kept_and_retried() {
    let mut w = good();
    let hw0 = w.key(0);
    let b = bound(&mut w, 0, ING, XPN256, None);
    w.m.set_acl_entry(
        &mut w.phy,
        b.entry,
        &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false),
    )
    .unwrap();
    let a =
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, b.sc.raw(), 0, true, true, 5))
            .unwrap();
    assert_eq!(w.m.sa_holds_key(a), Some(true));
    // MINIMUM_INGRESS_XPN of a pending SA changes what will be installed
    w.m.set_sa(a, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 0x1_0000_0009))
        .unwrap();
    w.phy.fail_plan.push_back(true);
    w.m.set_acl_entry(&mut w.phy, b.entry, &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true))
        .unwrap();
    assert_eq!((w.m.sa_installed(a), w.m.sa_holds_key(a)), (Some(false), Some(true)));
    assert!(!w.m.engine_state(hw0, Dir::Ingress).unwrap().gate);
    w.phy.runs_take();
    // any reconcile of the port: here an unrelated entry
    w.m.create_acl_entry(
        &mut w.phy,
        &[oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, b.table.raw()), ether_type(0x888e)],
    )
    .unwrap();
    let runs = w.phy.runs_take();
    assert_eq!(runs[0].name, "sa-add-first.ingress.0.gcm-aes-xpn-256");
    assert_eq!(runs[0].param("pn"), "0900000001000000");
    assert_eq!(runs[1].name, "gate-open.ingress");
    assert_eq!((w.m.sa_installed(a), w.m.sa_holds_key(a)), (Some(true), Some(false)));
    // installed: MINIMUM_INGRESS_XPN is accepted and changes nothing; the PN comes from the PHY
    w.m.set_sa(a, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 1))
        .unwrap();
    w.phy.reg(0x10, 0x510, 0x44);
    w.phy.reg(0x10, 0x512, 0x2);
    let pn =
        w.m.get_sa(&mut w.phy, a, &[sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN])
            .unwrap();
    assert!(matches!(pn[0], Value::U64(0x2_0000_0044)), "{pn:?}"); // 64 bits for an XPN suite
    // a failing pn sequence fails the get
    w.phy.fail_plan.push_back(true);
    assert_eq!(
        w.m.get_sa(&mut w.phy, a, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
            .unwrap_err(),
        Error::Failure
    );
    // egress SAs take no MINIMUM_INGRESS_XPN, and nothing else can be set
    assert_eq!(
        w.m.set_sa(a, &u64v(sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN, 1)),
        Err(not_supported(0))
    );
}

/// Q10: the cipher suite of an SC can change while SAs wait for their flow to be bound (only
/// installed SAs fix it). A waiting SA keeps the key bytes cut for the suite it was created
/// under.
#[test]
fn q10_a_cipher_suite_change_under_waiting_sas() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    let entry_admin = |on| boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, on);
    let cipher = |c: u32| en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(c));
    w.m.set_acl_entry(&mut w.phy, b.entry, &entry_admin(false)).unwrap();
    // created under gcm-aes-128: 16 key bytes kept. Then the SC becomes gcm-aes-256.
    let a =
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, b.sc.raw(), 0, false, false, 1))
            .unwrap();
    w.m.set_sc(b.sc, &cipher(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_256))
        .unwrap();
    w.m.set_acl_entry(&mut w.phy, b.entry, &entry_admin(true)).unwrap();
    assert!(w.phy.log_take().is_empty()); // the runner refused: the 256-bit sequence wants key[31]
    let state = w.m.engine_state(w.key(0), Dir::Egress).unwrap();
    assert!(
        state.last_fail.starts_with("sa-add-first.egress.0.gcm-aes-256: "),
        "{}",
        state.last_fail
    );
    assert_eq!(w.m.sa_installed(a), Some(false));
    w.m.remove_sa(&mut w.phy, a).unwrap();
    // created under gcm-aes-256 (32 key bytes kept), installed as gcm-aes-128: the sequence takes
    // key[0..16], the FIRST half, where SAI puts a 128-bit SAK in the last 16 bytes
    w.m.set_acl_entry(&mut w.phy, b.entry, &entry_admin(false)).unwrap();
    let a =
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, b.sc.raw(), 0, true, false, 1))
            .unwrap();
    w.m.set_sc(b.sc, &cipher(AES128)).unwrap();
    w.phy.runs_take();
    w.m.set_acl_entry(&mut w.phy, b.entry, &entry_admin(true)).unwrap();
    assert_eq!(w.m.sa_installed(a), Some(true));
    assert_eq!(
        w.phy.writes(),
        "200=100 201=c1c0 202=cfce 203=8f80 204=108 205=1 206=0 207=0 120=1"
    );
    assert_eq!(w.phy.runs_take()[0].param("key").len(), 64);
}

#[test]
fn a_failed_gate_sequence_is_logged_and_retried() {
    let mut w = good();
    let hw0 = w.key(0);
    let b = bound(&mut w, 0, EGR, AES128, None);
    w.phy.fail_plan.extend([false, true]); // sa-add ok, gate-open fails
    let a0 = sa(&mut w, EGR, b.sc, 0).unwrap(); // the SA is in: the call succeeds
    assert!(!w.m.engine_state(hw0, Dir::Egress).unwrap().gate);
    w.phy.runs_take();
    sa(&mut w, EGR, b.sc, 1).unwrap();
    assert_eq!(w.phy.names_take(), ["sa-add.egress.1.gcm-aes-128", "gate-open.egress"]);
    assert!(w.m.engine_state(hw0, Dir::Egress).unwrap().gate);
    // Q5: a gate that does not close does not stop the removal of the transmitting SA
    let a1 = w.m.engine_state(hw0, Dir::Egress).unwrap().tx.unwrap();
    w.phy.fail_plan.push_back(true);
    w.m.remove_sa(&mut w.phy, a1).unwrap();
    assert_eq!(
        w.phy.names_take(),
        ["gate-close.egress", "sa-del.egress.1", "gate-close.egress"]
    );
    let state = w.m.engine_state(hw0, Dir::Egress).unwrap();
    // AN 0 is still installed but no SA transmits: the second gate-close (reconcile) closed it
    assert_eq!((state.sa[0], state.tx, state.gate), (Some(a0), None, false));
}

#[test]
fn the_inter_frame_gap_that_cannot_be_programmed_does_not_fail_the_port() {
    let mut w = good();
    w.phy.ifg_fails = true;
    mport(&mut w, 0, EGR).unwrap();
    assert!(
        w.log_text()
            .contains("port ce0: MACsec is on without the wider inter-frame gap")
    );
    assert_eq!(w.m.port_ifg(w.key(0)), 384);
    assert_eq!(w.m.port_ifg(w.key(1)), 0);
    // a table without ifg-bits asks for nothing
    let mut plain = World::new(&ms_table(true, true).replace("ifg-bits 384\n", "")).unwrap();
    mport(&mut plain, 0, EGR).unwrap();
    assert_eq!((plain.m.port_ifg(plain.key(0)), plain.phy.ifg_calls), (0, 0));
}

// ------------------------------------------------------------------ startup

#[test]
fn startup_probe_resets_only_engines_that_are_not_in_bypass() {
    let ports = switch_ports(4);
    let table = FakeTable::parse(&ms_table(true, true));
    let mut phy = FakePhy::new(table.clone(), &ports);
    phy.reg(0x10, 0x100, 1); // port 0 egress: in bypass
    phy.reg(0x11, 0x101, 0x8000); // port 1 ingress: in bypass (any non-zero value)
    phy.fail_plan.extend([false, true]); // port 0 ingress: the probe itself fails
    let m = Macsec::attach(&table.model_table(), &ports, Options::default(), &mut phy).unwrap();
    assert_eq!(
        phy.names_take(),
        [
            "probe.egress",
            "probe.ingress",
            "probe.egress",
            "reset.egress",
            "disable.egress",
            "probe.ingress"
        ]
    );
    assert_eq!(phy.writes(), "110=1 100=1"); // of PHY 0x11 only
    // Q6: the engine whose probe failed was left alone, whatever it holds
    let state = m.engine_state(PortKey(1), Dir::Ingress).unwrap();
    assert_eq!((state.runs, state.fails), (1, 1));
    assert_eq!(m.engine_ports(), [PortKey(1), PortKey(5)]);
    m.check_invariants().unwrap();
}

#[test]
fn detach_forgets_everything_and_runs_nothing() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    sa(&mut w, EGR, b.sc, 0).unwrap();
    w.phy.log_take();
    w.m.detach();
    assert!(w.phy.log_take().is_empty()); // Q2: the engine stays on, with its key, until the next attach probes it
    assert!(!w.m.active());
    assert_eq!(w.m.counts(), sai_macsec::Counts::default());
    assert_eq!(
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)]),
        Err(Error::NotSupported)
    );
    assert_eq!(w.m.remove_sc(b.sc), Err(Error::InvalidObjectId));
}

// ------------------------------------------------------------------ one SC per port; flows that move

#[test]
fn an_association_number_held_by_another_sc_is_insufficient_resources() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    let a0 = sa(&mut w, EGR, b.sc, 0).unwrap();
    // the entry now sends data to another flow, with its own SC
    let flow2 =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)])
            .unwrap();
    let sc2 = w.m.create_sc(&sc_attrs(EGR, flow2, AES128, None)).unwrap();
    w.m.set_acl_entry(
        &mut w.phy,
        b.entry,
        &action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, flow2.raw()),
    )
    .unwrap();
    w.phy.log_take();
    assert_eq!(sa(&mut w, EGR, sc2, 0), Err(Error::InsufficientResources));
    assert!(w.phy.log_take().is_empty());
    assert_eq!(w.m.counts().sas, 1);
    // Q7: the first SC's SA is still in the PHY and the gate is still open for the new flow
    let state = w.m.engine_state(w.key(0), Dir::Egress).unwrap();
    assert_eq!((state.sa[0], state.gate), (Some(a0), true));
    // Q8: a free AN installs, and the first SC now reports the other SC's SA as its transmitting one
    let other = sa(&mut w, EGR, sc2, 1).unwrap();
    let active =
        w.m.get_sc(b.sc, &[sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID])
            .unwrap();
    assert!(matches!(active[0], Value::Oid(o) if o == other.raw()), "{active:?}");
    w.m.check_invariants().unwrap();
}

/// D1, without the C++ (tests/differential.rs shows the C++ side of it).
#[test]
fn d1_an_sa_is_removed_from_the_port_that_holds_it() {
    let mut w = good();
    let b0 = bound(&mut w, 0, EGR, AES128, None);
    let a0 = sa(&mut w, EGR, b0.sc, 0).unwrap();
    // the flow moves to port 1
    w.m.set_acl_entry(
        &mut w.phy,
        b0.entry,
        &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false),
    )
    .unwrap();
    mport(&mut w, 1, EGR).unwrap();
    let t1 = table(&mut w, EGR);
    let p1 = w.port(1);
    w.m.set_port_acl(&mut w.phy, p1, Dir::Egress, t1.raw()).unwrap();
    flow_entry(&mut w, t1, b0.flow);
    let a1 = sa(&mut w, EGR, b0.sc, 1).unwrap();
    assert_eq!(w.m.engine_state(w.key(0), Dir::Egress).unwrap().sa[0], Some(a0));
    assert_eq!(w.m.engine_state(w.key(1), Dir::Egress).unwrap().sa[1], Some(a1));
    w.phy.log_take();
    w.phy.runs_take();
    // counters and PN of a0 come from PHY 0x10, where it is
    w.phy.reg(0x10, 0x400, 4);
    assert_eq!(
        w.m.get_sa_stats(&mut w.phy, a0, &[sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED])
            .unwrap(),
        [4]
    );
    w.phy.reg(0x10, 0x500, 0x77);
    let pn =
        w.m.get_sa(&mut w.phy, a0, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
            .unwrap();
    assert!(matches!(pn[0], Value::U64(0x77)), "{pn:?}");
    assert!(w.phy.runs_take().iter().all(|r| r.port == w.key(0)));
    w.phy.log_take();
    w.m.remove_sa(&mut w.phy, a0).unwrap();
    assert_eq!(w.phy.writes(), "300=8001");
    assert_eq!(w.phy.runs_take()[0].port, w.key(0));
    assert_eq!(w.m.engine_state(w.key(0), Dir::Egress).unwrap().sa, [None; 4]);
    w.m.check_invariants().unwrap();
}

// ------------------------------------------------------------------ attribute rules

#[test]
fn macsec_attributes() {
    let mut w = good();
    assert_eq!(w.m.create_macsec(&[]), Err(Error::MandatoryAttributeMissing));
    assert_eq!(
        w.m.create_macsec(&[
            en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR),
            boolean(sys::SAI_MACSEC_ATTR_WARM_BOOT_SUPPORTED, true)
        ]),
        Err(not_supported(1))
    );
    // a value of another kind than the attribute's
    assert_eq!(
        w.m.create_macsec(&[boolean(sys::SAI_MACSEC_ATTR_DIRECTION, true)]),
        Err(invalid_value(0))
    );
    let m =
        w.m.create_macsec(&[
            en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR),
            boolean(sys::SAI_MACSEC_ATTR_ENABLE_POST, false),
            boolean(sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE, false),
            en(sys::SAI_MACSEC_ATTR_STATS_MODE, e(sys::SAI_STATS_MODE_READ)),
        ])
        .unwrap();
    w.m.set_macsec(m, &boolean(sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, false))
        .unwrap();
    w.m.set_macsec(m, &boolean(sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE, true))
        .unwrap();
    assert_eq!(
        w.m.set_macsec(m, &boolean(sys::SAI_MACSEC_ATTR_ENABLE_POST, false)),
        Err(not_supported(0))
    );
    let ids = [
        sys::SAI_MACSEC_ATTR_DIRECTION,
        sys::SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED,
        sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED,
        sys::SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED,
        sys::SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED,
        sys::SAI_MACSEC_ATTR_STATS_MODE_READ_SUPPORTED,
        sys::SAI_MACSEC_ATTR_STATS_MODE_READ_CLEAR_SUPPORTED,
        sys::SAI_MACSEC_ATTR_WARM_BOOT_SUPPORTED,
        sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE,
        sys::SAI_MACSEC_ATTR_ENABLE_POST,
        sys::SAI_MACSEC_ATTR_SWITCHING_MODE_CUT_THROUGH_SUPPORTED,
        sys::SAI_MACSEC_ATTR_SWITCHING_MODE_STORE_AND_FORWARD_SUPPORTED,
        sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE,
        sys::SAI_MACSEC_ATTR_STATS_MODE,
        sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_FLOW,
        sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SC,
        sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA,
    ];
    let got = format!("{:?}", w.m.get_macsec(&w.phy, m, &ids).unwrap());
    assert_eq!(
        got,
        "[Enum(0), Bool(true), Bool(true), Bool(true), Bool(true), Bool(true), Bool(false), Bool(false), Bool(false), \
         Bool(false), Bool(false), Bool(true), Bool(true), Enum(1), U32(2), U32(2), U32(8)]"
    );
    assert_eq!(
        w.m.get_macsec(
            &w.phy,
            m,
            &[sys::SAI_MACSEC_ATTR_DIRECTION, sys::SAI_MACSEC_ATTR_FLOW_LIST]
        )
        .unwrap_err(),
        not_supported(1)
    );
    // a table with only the gcm-aes-128 sequences: the capabilities follow the table
    let only128: String = {
        let mut keep = true;
        ms_table(true, true)
            .lines()
            .filter(|l| {
                if let Some(name) = l.strip_prefix("seq ") {
                    keep = !name.starts_with("sa-add") || name.ends_with(".gcm-aes-128");
                }
                keep
            })
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let mut small = World::new(&only128).unwrap();
    let m2 = small
        .m
        .create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, ING)])
        .unwrap();
    let got = format!("{:?}", small.m.get_macsec(&small.phy, m2, &ids[..5]).unwrap());
    assert_eq!(got, "[Enum(1), Bool(true), Bool(false), Bool(true), Bool(false)]");
    // removal is refused under a MACSEC_PORT of the direction only
    let p = mport(&mut w, 0, ING).unwrap();
    w.m.remove_macsec(m).unwrap();
    assert_eq!(w.m.remove_macsec(m), Err(Error::InvalidObjectId));
    assert_eq!(
        w.m.set_macsec(m, &boolean(sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE, true)),
        Err(Error::InvalidObjectId)
    );
    w.m.remove_macsec_port(&mut w.phy, p).unwrap();
}

#[test]
fn macsec_port_attributes() {
    let mut w = good();
    let dir = en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, ING);
    let port = oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port_raw(w.port(1)));
    assert_eq!(
        w.m.create_macsec_port(&mut w.phy, std::slice::from_ref(&dir)),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        w.m.create_macsec_port(&mut w.phy, std::slice::from_ref(&port)),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        w.m.create_macsec_port(
            &mut w.phy,
            &[
                dir.clone(),
                port.clone(),
                boolean(sys::SAI_MACSEC_PORT_ATTR_STAG_ENABLE, true)
            ]
        ),
        Err(not_supported(2)) // no VLAN tag in clear
    );
    // not a port of this switch: an id of another type, a port that does not exist
    let flow =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, ING)])
            .unwrap();
    for bad in [flow.raw(), RawOid(port_raw(w.port(1)).0 + 100)] {
        assert_eq!(
            w.m.create_macsec_port(&mut w.phy, &[dir.clone(), oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, bad)]),
            Err(invalid_value(1))
        );
    }
    assert!(w.phy.log_take().is_empty());
    let p =
        w.m.create_macsec_port(
            &mut w.phy,
            &[
                boolean(sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE, false),
                port,
                dir,
                en(sys::SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE, 0),
            ],
        )
        .unwrap();
    assert_eq!(w.phy.writes(), "111=1 101=0"); // on PHY 0x11
    assert_eq!(w.phy.runs_take()[0].port, w.key(1));
    w.m.set_macsec_port(p, &boolean(sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE, false))
        .unwrap();
    assert_eq!(
        w.m.set_macsec_port(p, &boolean(sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE, true)),
        Err(not_supported(0))
    );
    let ids = [
        sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION,
        sys::SAI_MACSEC_PORT_ATTR_PORT_ID,
        sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE,
        sys::SAI_MACSEC_PORT_ATTR_STAG_ENABLE,
        sys::SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE,
    ];
    let got = w.m.get_macsec_port(p, &ids).unwrap();
    assert!(matches!(got[1], Value::Oid(o) if o == port_raw(w.port(1))));
    assert!(matches!(got[4], Value::Enum(v) if v == e(sys::SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD)));
    assert_eq!(
        w.m.get_macsec_port(p, &[sys::SAI_MACSEC_PORT_ATTR_STATS_COUNT_MODE])
            .unwrap_err(),
        not_supported(0)
    );
    assert_eq!(w.m.port_macsec_ports(w.port(1)), [p]);
    assert!(w.m.port_macsec_ports(w.port(0)).is_empty());
    w.m.remove_macsec_port(&mut w.phy, p).unwrap();
    assert_eq!(w.m.remove_macsec_port(&mut w.phy, p), Err(Error::InvalidObjectId));
}

#[test]
fn flow_and_sc_attributes() {
    let mut w = good();
    assert_eq!(w.m.create_flow(&[]), Err(Error::MandatoryAttributeMissing));
    assert_eq!(
        w.m.create_flow(&[
            en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR),
            Attr::new(sys::SAI_MACSEC_FLOW_ATTR_SC_LIST, Value::OidList(vec![]))
        ]),
        Err(not_supported(1))
    );
    let b = bound(&mut w, 0, ING, XPN256, Some(7));
    assert_eq!(w.m.set_flow(b.flow, &boolean(0, true)), Err(not_supported(0)));
    let lists =
        w.m.get_flow(
            b.flow,
            &[
                sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION,
                sys::SAI_MACSEC_FLOW_ATTR_SC_LIST,
                sys::SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST,
            ],
        )
        .unwrap();
    assert!(matches!(&lists[1], Value::OidList(l) if *l == [b.sc.raw()]));
    assert!(matches!(&lists[2], Value::OidList(l) if *l == [b.entry.raw()]));
    assert_eq!(
        w.m.get_flow(b.flow, &[sys::SAI_MACSEC_FLOW_ATTR_STATS_COUNT_MODE])
            .unwrap_err(),
        not_supported(0)
    );
    // SC: what the PHY sequences are recorded for is fixed
    let base = || sc_attrs(ING, b.flow, XPN256, None);
    let with = |a: Attr| {
        let mut v = base();
        v.push(a);
        v
    };
    for (attr, ok) in [
        (
            boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE, false),
            false,
        ),
        (boolean(sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, false), false),
        (u8v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET, 4), false),
        (u8v(sys::SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET, 30), false),
        (boolean(sys::SAI_MACSEC_SC_ATTR_USE_ES, true), false),
        (boolean(sys::SAI_MACSEC_SC_ATTR_USE_SCB, true), false),
        (u8v(sys::SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET, 0), true),
        (boolean(sys::SAI_MACSEC_SC_ATTR_USE_ES, false), true),
    ] {
        // (the flow has its SC already: an acceptable value gets as far as that)
        let want = if ok {
            Error::InsufficientResources
        } else {
            not_supported(4)
        };
        assert_eq!(w.m.create_sc(&with(attr.clone())), Err(want));
        assert_eq!(w.m.set_sc(b.sc, &attr), if ok { Ok(()) } else { Err(not_supported(0)) });
    }
    assert_eq!(
        w.m.create_sc(&with(en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, 99))),
        Err(invalid_value(4))
    );
    assert_eq!(
        w.m.create_sc(&with(Attr::new(
            sys::SAI_MACSEC_SC_ATTR_SA_LIST,
            Value::OidList(vec![])
        ))),
        Err(not_supported(4))
    );
    assert_eq!(w.m.create_sc(&base()[..2]), Err(Error::MandatoryAttributeMissing));
    let mut no_flow = base();
    no_flow[1] = oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, b.sc.raw()); // not a flow
    assert_eq!(w.m.create_sc(&no_flow), Err(invalid_value(1)));
    // the SCI is returned as it was passed; the fixed attributes read their fixed values
    let ids = [
        sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION,
        sys::SAI_MACSEC_SC_ATTR_FLOW_ID,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE,
        sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE,
        sys::SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET,
        sys::SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET,
        sys::SAI_MACSEC_SC_ATTR_USE_ES,
        sys::SAI_MACSEC_SC_ATTR_USE_SCB,
        sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID,
        sys::SAI_MACSEC_SC_ATTR_SA_LIST,
    ];
    let got = w.m.get_sc(b.sc, &ids).unwrap();
    assert!(matches!(got[2], Value::U64(v) if v == u64::from_ne_bytes([1, 2, 3, 4, 5, 6, 7, 8])));
    assert_eq!(
        format!("{:?}", &got[3..12]),
        "[Enum(3), Bool(true), U32(7), Bool(true), Bool(true), U8(0), U8(0), Bool(false), Bool(false)]"
    );
    assert!(matches!(got[12], Value::Oid(RawOid::NULL))); // an ingress SC has no transmitting SA
    assert_eq!(
        w.m.get_sc(b.sc, &[sys::SAI_MACSEC_SC_ATTR_MACSEC_PORT_ID]).unwrap_err(),
        not_supported(0)
    );
    // cipher suite, replay protection and window: free while no SA is installed, then fixed
    w.m.set_sc(b.sc, &en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(AES128)))
        .unwrap();
    w.m.set_sc(b.sc, &en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(XPN256)))
        .unwrap();
    assert_eq!(
        w.m.set_sc(b.sc, &en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, 99)),
        Err(invalid_value(0))
    );
    let a =
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, b.sc.raw(), 0, true, true, 1))
            .unwrap();
    let replay = |on| boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, on);
    let window = |v| u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, v);
    w.m.set_sc(b.sc, &en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(XPN256)))
        .unwrap(); // unchanged: fine
    w.m.set_sc(b.sc, &replay(true)).unwrap();
    w.m.set_sc(b.sc, &window(7)).unwrap();
    assert_eq!(w.m.set_sc(b.sc, &replay(false)), Err(Error::ObjectInUse));
    assert_eq!(w.m.set_sc(b.sc, &window(8)), Err(Error::ObjectInUse));
    assert_eq!(w.m.set_sc(b.sc, &window(8)), Err(Error::ObjectInUse));
    assert_eq!(w.m.remove_sc(b.sc), Err(Error::ObjectInUse));
    w.m.remove_sa(&mut w.phy, a).unwrap();
    w.m.set_sc(b.sc, &replay(false)).unwrap();
    w.m.set_sc(b.sc, &window(8)).unwrap();
    w.m.remove_sc(b.sc).unwrap();
    assert_eq!(w.m.get_sc(b.sc, &ids).unwrap_err(), Error::InvalidObjectId);
    assert_eq!(w.m.set_sc(b.sc, &window(8)), Err(Error::InvalidObjectId));
}

#[test]
fn sa_attributes() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    let bx = bound(&mut w, 0, ING, XPN256, None);
    let attrs = || sa_attrs(EGR, b.sc.raw(), 0, false, false, 1);
    // every mandatory attribute, one at a time (direction, SC, AN, SAK, auth key)
    for missing in [0, 1, 2, 4, 5] {
        let mut v = attrs();
        v.remove(missing);
        assert_eq!(w.m.create_sa(&mut w.phy, &v), Err(Error::MandatoryAttributeMissing));
    }
    let mut v = attrs();
    v[2] = u8v(sys::SAI_MACSEC_SA_ATTR_AN, 4);
    assert_eq!(w.m.create_sa(&mut w.phy, &v), Err(invalid_value(2)));
    let mut v = attrs();
    v.push(u64v(sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN, 1)); // read-only
    assert_eq!(w.m.create_sa(&mut w.phy, &v), Err(not_supported(6)));
    let mut v = attrs();
    v[1] = oid(sys::SAI_MACSEC_SA_ATTR_SC_ID, b.flow.raw()); // not an SC
    assert_eq!(w.m.create_sa(&mut w.phy, &v), Err(Error::InvalidParameter));
    // an ingress SA on an egress SC
    assert_eq!(
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, b.sc.raw(), 0, false, false, 1)),
        Err(Error::InvalidParameter)
    );
    // a packet number beyond 32 bits needs an XPN suite
    assert_eq!(
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, b.sc.raw(), 0, false, false, 0x1_0000_0000)),
        Err(Error::InvalidParameter)
    );
    w.m.create_sa(&mut w.phy, &sa_attrs(EGR, b.sc.raw(), 0, false, false, 0xffff_ffff))
        .unwrap();
    // an XPN suite needs salt and SSCI
    assert_eq!(
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, bx.sc.raw(), 0, true, false, 1)),
        Err(Error::MandatoryAttributeMissing)
    );
    // a value of another kind in a key attribute
    let mut v = attrs();
    v[4] = Attr::new(
        sys::SAI_MACSEC_SA_ATTR_SAK,
        Value::AuthKey(Secret::from_slice(&[0; 16]).unwrap()),
    );
    assert_eq!(w.m.create_sa(&mut w.phy, &v), Err(invalid_value(4)));
    // nothing of the refused calls reached the model or the bus beyond the one good SA
    assert_eq!(w.m.counts().sas, 1);
    // the default packet number is 1; salt and SSCI are ignored for a non-XPN suite
    w.phy.runs_take();
    let mut v = sa_attrs(EGR, b.sc.raw(), 1, false, true, 0);
    v.remove(3);
    let a = w.m.create_sa(&mut w.phy, &v).unwrap();
    let run = &w.phy.runs_take()[0];
    assert_eq!(run.param("pn"), "0100000000000000");
    assert_eq!(
        run.params.keys().map(String::as_str).collect::<Vec<_>>(),
        ["hkey", "key", "pn", "sci", "window"]
    );
    let got =
        w.m.get_sa(
            &mut w.phy,
            a,
            &[
                sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION,
                sys::SAI_MACSEC_SA_ATTR_SC_ID,
                sys::SAI_MACSEC_SA_ATTR_AN,
            ],
        )
        .unwrap();
    assert!(matches!(got[..], [Value::Enum(0), Value::Oid(sc), Value::U8(1)] if sc == b.sc.raw()));
    for write_only in [
        sys::SAI_MACSEC_SA_ATTR_SAK,
        sys::SAI_MACSEC_SA_ATTR_SALT,
        sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
        sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI,
    ] {
        assert_eq!(
            w.m.get_sa(&mut w.phy, a, &[sys::SAI_MACSEC_SA_ATTR_AN, write_only])
                .unwrap_err(),
            not_supported(1)
        );
    }
    // Q9: CONFIGURED_EGRESS_XPN reads the live packet number, like CURRENT_XPN
    w.phy.reg(0x10, 0x504, 0x99);
    let pn =
        w.m.get_sa(&mut w.phy, a, &[sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN])
            .unwrap();
    assert!(matches!(pn[0], Value::U64(0x99)), "{pn:?}");
    w.m.remove_sa(&mut w.phy, a).unwrap();
    assert_eq!(w.m.get_sa(&mut w.phy, a, &[]).unwrap_err(), Error::InvalidObjectId);
    assert_eq!(
        w.m.get_sa_stats(&mut w.phy, a, &[]).unwrap_err(),
        Error::InvalidObjectId
    );
    assert_eq!(w.m.clear_sa_stats(&mut w.phy, a), Err(Error::InvalidObjectId));
    assert_eq!(
        w.m.set_sa(a, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 1)),
        Err(Error::InvalidObjectId)
    );
}

#[test]
fn acl_tables_entries_and_the_port_binding() {
    let mut w = good();
    let stage = |s: u32| en(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(s));
    assert!(!Macsec::is_macsec_stage(&[stage(sys::SAI_ACL_STAGE_INGRESS)]));
    assert!(!Macsec::is_macsec_stage(&[]));
    let ingress = stage(sys::SAI_ACL_STAGE_INGRESS_MACSEC);
    assert!(Macsec::is_macsec_stage(std::slice::from_ref(&ingress)));
    // bind points other than the port, fields the PHY's classification cannot stand for
    let lag = Attr::new(
        sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
        Value::I32List(vec![
            e(sys::SAI_ACL_BIND_POINT_TYPE_PORT),
            e(sys::SAI_ACL_BIND_POINT_TYPE_LAG),
        ]),
    );
    assert_eq!(w.m.create_acl_table(&[ingress.clone(), lag]), Err(not_supported(1)));
    assert_eq!(
        w.m.create_acl_table(&[ingress.clone(), boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IP, true)]),
        Err(not_supported(1))
    );
    assert_eq!(
        w.m.create_acl_table(&[ingress.clone(), u32v(sys::SAI_ACL_TABLE_ATTR_ENTRY_LIST, 0)]),
        Err(not_supported(1))
    );
    let t =
        w.m.create_acl_table(&[
            ingress,
            boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IP, false),
            boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_MACSEC_SCI, true),
            boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC, false),
            u32v(sys::SAI_ACL_TABLE_ATTR_SIZE, 4),
        ])
        .unwrap();
    assert_eq!(w.m.acl_table(t.raw()), Some(t));
    assert_eq!(w.m.acl_table(RawOid(t.raw().0 + 1)), None);
    let flow =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, ING)])
            .unwrap();
    let egress_flow =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)])
            .unwrap();
    let table_id = oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, t.raw());
    let flow_action = |f: FlowId| action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, f.raw());
    assert_eq!(
        w.m.create_acl_entry(&mut w.phy, &[]),
        Err(Error::MandatoryAttributeMissing)
    );
    assert_eq!(
        w.m.create_acl_entry(&mut w.phy, &[oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, flow.raw())]),
        Err(invalid_value(0))
    );
    // a flow of the other direction, something that is not a flow, an action that is neither drop nor forward
    for bad in [egress_flow.raw(), t.raw(), RawOid::NULL] {
        assert_eq!(
            w.m.create_acl_entry(
                &mut w.phy,
                &[
                    table_id.clone(),
                    action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, bad)
                ]
            ),
            Err(invalid_value(1))
        );
    }
    let trap = action_s32(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
        true,
        e(sys::SAI_PACKET_ACTION_TRAP),
    );
    assert_eq!(
        w.m.create_acl_entry(&mut w.phy, &[table_id.clone(), trap]),
        Err(not_supported(1))
    );
    assert_eq!(
        w.m.create_acl_entry(
            &mut w.phy,
            &[table_id.clone(), boolean(sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP, true)]
        ),
        Err(not_supported(1))
    );
    let sci_field = Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI,
        Value::AclField(Some(AclField::U64 {
            data: 5,
            mask: u64::MAX,
        })),
    );
    let en1 =
        w.m.create_acl_entry(
            &mut w.phy,
            &[
                table_id.clone(),
                u32v(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 9),
                sci_field,
                flow_action(flow),
                // a data entry that drops an EtherType / forwards in clear: accepted, logged, not programmed
                ether_type(0x888e),
                action_s32(
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                    true,
                    e(sys::SAI_PACKET_ACTION_DROP),
                ),
            ],
        )
        .unwrap();
    assert!(
        w.log_text()
            .contains("an entry that drops an EtherType is not programmed")
    );
    let clear = action_s32(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
        true,
        e(sys::SAI_PACKET_ACTION_FORWARD),
    );
    let en2 = w.m.create_acl_entry(&mut w.phy, &[table_id.clone(), clear]).unwrap();
    assert!(
        w.log
            .lock()
            .unwrap()
            .iter()
            .any(|(l, t)| *l == Level::Warn && t.contains("forwards in clear"))
    );
    assert_eq!(w.m.acl_entry(en1.raw()), Some(en1));
    assert_eq!(w.m.acl_entry(t.raw()), None);
    // get: table
    let got =
        w.m.get_acl_table(
            t,
            &[
                sys::SAI_ACL_TABLE_ATTR_ACL_STAGE,
                sys::SAI_ACL_TABLE_ATTR_FIELD_MACSEC_SCI,
                sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC,
                sys::SAI_ACL_TABLE_ATTR_ENTRY_LIST,
                sys::SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY,
            ],
        )
        .unwrap();
    assert!(matches!(got[0], Value::Enum(v) if v == e(sys::SAI_ACL_STAGE_INGRESS_MACSEC)));
    assert!(matches!(got[1..3], [Value::Bool(true), Value::Bool(false)]));
    assert!(matches!(&got[3], Value::OidList(l) if *l == [en1.raw(), en2.raw()]));
    assert!(matches!(got[4], Value::U32(16)));
    assert_eq!(
        w.m.get_acl_table(t, &[sys::SAI_ACL_TABLE_ATTR_SIZE]).unwrap_err(),
        not_supported(0)
    );
    // get: entry
    let got =
        w.m.get_acl_entry(
            en1,
            &[
                sys::SAI_ACL_ENTRY_ATTR_TABLE_ID,
                sys::SAI_ACL_ENTRY_ATTR_PRIORITY,
                sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE,
                sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI,
                sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC,
            ],
        )
        .unwrap();
    assert!(matches!(got[0], Value::Oid(o) if o == t.raw()));
    assert!(matches!(got[1..3], [Value::U32(9), Value::Bool(true)]));
    assert!(
        matches!(got[3], Value::AclAction { enable: true, parameter: AclParameter::S32(a) } if a == e(sys::SAI_PACKET_ACTION_DROP))
    );
    assert!(matches!(got[4], Value::AclAction { enable: true, parameter: AclParameter::Oid(o) } if o == flow.raw()));
    assert!(matches!(got[5], Value::AclField(Some(AclField::U64 { data: 5, .. }))));
    assert!(matches!(got[6], Value::AclField(None)));
    assert_eq!(
        w.m.get_acl_entry(en1, &[sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER])
            .unwrap_err(),
        not_supported(0)
    );
    // set: a failed set changes nothing; a field can be switched off; the table cannot be changed
    assert_eq!(
        w.m.set_acl_entry(&mut w.phy, en1, &flow_action(egress_flow)),
        Err(invalid_value(0))
    );
    assert_eq!(w.m.set_acl_entry(&mut w.phy, en1, &table_id), Err(invalid_value(0)));
    let off = Attr::new(sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI, Value::AclField(None));
    w.m.set_acl_entry(&mut w.phy, en1, &off).unwrap();
    let got =
        w.m.get_acl_entry(
            en1,
            &[
                sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI,
                sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
            ],
        )
        .unwrap();
    assert!(matches!(got[0], Value::AclField(None)));
    assert!(matches!(got[1], Value::AclAction { enable: true, .. }));
    // the port binding
    let (p0, p1, p2) = (w.port(0), w.port(1), w.port(2));
    assert_eq!(w.m.port_acl(p0, Dir::Ingress), RawOid::NULL);
    assert_eq!(
        w.m.set_port_acl(&mut w.phy, p0, Dir::Ingress, flow.raw()),
        Err(invalid_value(0))
    ); // not a table
    assert_eq!(
        w.m.set_port_acl(&mut w.phy, p2, Dir::Ingress, t.raw()),
        Err(not_supported(0))
    ); // no MACsec PHY
    w.m.set_port_acl(&mut w.phy, p2, Dir::Ingress, RawOid::NULL).unwrap(); // nothing to unbind: fine
    w.m.set_port_acl(&mut w.phy, p0, Dir::Ingress, t.raw()).unwrap();
    w.m.set_port_acl(&mut w.phy, p0, Dir::Ingress, t.raw()).unwrap(); // again: fine
    assert_eq!(
        w.m.set_port_acl(&mut w.phy, p1, Dir::Ingress, t.raw()),
        Err(invalid_value(0))
    ); // bound to another port
    let t2 = table(&mut w, ING);
    w.m.set_port_acl(&mut w.phy, p0, Dir::Ingress, t2.raw()).unwrap(); // replaces the binding
    assert_eq!(w.m.port_acl(p0, Dir::Ingress), t2.raw());
    w.m.set_port_acl(&mut w.phy, p1, Dir::Ingress, t.raw()).unwrap(); // the first table is free again
    // removal
    assert_eq!(w.m.remove_flow(flow), Err(Error::ObjectInUse)); // an entry's flow action names it
    assert_eq!(w.m.remove_acl_table(t), Err(Error::ObjectInUse)); // bound, and has entries
    w.m.set_port_acl(&mut w.phy, p1, Dir::Ingress, RawOid::NULL).unwrap();
    assert_eq!(w.m.remove_acl_table(t), Err(Error::ObjectInUse)); // has entries
    w.m.remove_acl_entry(&mut w.phy, en1).unwrap();
    w.m.remove_acl_entry(&mut w.phy, en2).unwrap();
    assert_eq!(w.m.remove_acl_entry(&mut w.phy, en2), Err(Error::InvalidObjectId));
    w.m.remove_acl_table(t).unwrap();
    assert_eq!(w.m.remove_acl_table(t), Err(Error::InvalidObjectId));
    w.m.remove_flow(flow).unwrap();
    assert!(w.phy.log_take().is_empty()); // no MACSEC_PORT anywhere: nothing ever ran
    w.m.check_invariants().unwrap();
}

// ------------------------------------------------------------------ diag texts

#[test]
fn status_text_format() {
    let mut w = good();
    let b = bound(&mut w, 0, EGR, AES128, None);
    sa(&mut w, EGR, b.sc, 0).unwrap();
    sa(&mut w, EGR, b.sc, 2).unwrap();
    let bi = bound(&mut w, 1, ING, AES128, None);
    sa(&mut w, ING, bi.sc, 3).unwrap();
    w.phy.fail_plan.push_back(true);
    sa(&mut w, ING, bi.sc, 1).unwrap_err();
    let text = w.m.status(&w.phy);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "extmacsec: 2 ports, 111 sequences; objects: 0 macsec, 2 ports, 2 flows, 2 SCs, 3 SAs, 2 ACL tables, 2 ACL entries"
    );
    assert_eq!(
        lines[1],
        "port      phy  dir     engine gate   AN0 AN1 AN2 AN3 tx   runs fails  max_us last failure"
    );
    // the longest run time (7 characters before the last column) is whatever it was
    let fixed = |l: &str| format!("{}{}", &l[..70], &l[77..]);
    assert_eq!(
        fixed(lines[2]),
        "ce0        10  egress  on     open   sa  -   sa  -   2       8     0  "
    );
    assert_eq!(
        fixed(lines[3]),
        "ce1        11  ingress on     open   -   -   -   sa          8     1  sa-add.ingress.1.gcm-aes-128: injected failure"
    );
    assert_eq!(lines[4], "engines in bypass: 2 of 4");
    assert_eq!(lines.len(), 5);
}

#[test]
fn diag_run_by_hand() {
    let mut w = good();
    let none = Macsec::without_table(Options::default()).diag_run(&mut w.phy, "ce0", "probe.egress", &Params::new());
    assert_eq!(none, "extmacsec: no external MACsec table\n");
    assert_eq!(
        w.m.diag_run(&mut w.phy, "ce9", "probe.egress", &Params::new()),
        "extmacsec: no such port in the table: ce9\n"
    );
    w.phy.reg(0x11, 0x101, 3);
    let text = w.m.diag_run(&mut w.phy, "ce1", "probe.ingress", &Params::new());
    let (head, tail) = text.split_once(", ").unwrap();
    assert_eq!(head, "extmacsec: ce1 probe.ingress: ok");
    assert!(tail.ends_with(" us\n  bypass 3\n"), "{text}");
    assert_eq!(w.phy.runs_take()[0].port, w.key(1));
    // a sequence with parameters: they reach the runner, a missing one is refused by it
    let mut par = Params::new();
    par.set("window", &[0x34, 0x12]);
    let text =
        w.m.diag_run(&mut w.phy, "ce0", "sa-add-first.ingress.0.gcm-aes-128.replay", &par);
    assert!(
        text.starts_with("extmacsec: ce0 sa-add-first.ingress.0.gcm-aes-128.replay: ok, "),
        "{text}"
    );
    assert_eq!(w.phy.writes(), "210=f00 217=1234");
    let text = w.m.diag_run(&mut w.phy, "ce0", "sa-add.egress.0.gcm-aes-128", &par);
    assert!(
        text.starts_with(&format!(
            "extmacsec: ce0 sa-add.egress.0.gcm-aes-128: FAILED ({}): sa-add.egress.0.gcm-aes-128: line ",
            Error::InvalidParameter.status()
        )),
        "{text}"
    );
    assert!(text.contains("parameter key[1] is not available"), "{text}");
    assert!(w.phy.log_take().is_empty());
    // a sequence the table does not have
    let text = w.m.diag_run(&mut w.phy, "ce0", "no-such", &Params::new());
    assert!(
        text.starts_with(&format!(
            "extmacsec: ce0 no-such: FAILED ({}): ",
            Error::NotSupported.status()
        )),
        "{text}"
    );
}

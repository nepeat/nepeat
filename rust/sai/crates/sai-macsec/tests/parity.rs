//! The object-model half of the C++ `tests/test_macsec.inc` (`test_macsec`), case by case
//! in its order, on the same made-up PHY and the same table text. The bus-write strings are
//! the C++ test's `MS_BUS` strings verbatim; next to them the exact sequence names and
//! parameter bytes are asserted. Section comments quote the C++ test's.
//!
//! Not here (not the model's): the table parser cases, the executor's golden rendering
//! (`phy-seq`), and the switch-creation cases "missing file" and "bad table" (the loader).

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
    reason = "test code: a failed unwrap is a failed test; the big test follows the C++ test's order"
)]

mod common;

use common::{
    EGR, ING, World, action_oid, action_s32, boolean, en, ether_type, invalid_value, ms_table, not_supported, oid,
    port_raw, sa_attrs, u32v, u64v,
};
use sai_core::Error;
use sai_core::oid::RawOid;
use sai_macsec::{Attr, Dir, Macsec, Options, Value};
use sai_sys as sys;

fn e(v: u32) -> i32 {
    i32::try_from(v).unwrap()
}

fn get1(values: Vec<Value>) -> Value {
    values.into_iter().next().unwrap()
}

macro_rules! assert_value {
    ($got:expr, $pat:pat $(if $guard:expr)?) => {{
        let got = $got;
        assert!(matches!(got, $pat $(if $guard)?), "got {got:?}");
    }};
}

/// "---- no table: the API is there and says so"
#[test]
fn no_table_the_api_is_there_and_says_so() {
    let mut w = World::new(&ms_table(true, true)).unwrap(); // only for a runner to pass
    let mut m = Macsec::without_table(Options::default());
    assert!(!m.active());
    assert_eq!(
        m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)]),
        Err(Error::NotSupported)
    );
    // the other creates take the same exit (need_table)
    assert_eq!(
        m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)]),
        Err(Error::NotSupported)
    );
    assert_eq!(m.create_macsec_port(&mut w.phy, &[]).unwrap_err(), Error::NotSupported);
    assert_eq!(m.create_sc(&[]).unwrap_err(), Error::NotSupported);
    assert_eq!(m.create_sa(&mut w.phy, &[]).unwrap_err(), Error::NotSupported);
    assert_eq!(m.create_acl_table(&[]).unwrap_err(), Error::NotSupported);
    assert_eq!(
        m.status(&w.phy),
        "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n"
    );
    assert_eq!(m.port_ifg(w.key(0)), 0);
}

/// "---- switch creation: ... unknown lane" (`f_lane`: lane 3 is inside port 0, not its first)
#[test]
fn switch_creation_unknown_lane() {
    assert_eq!(
        World::new("ext-macsec 1\nport 3 10\n").map(|_| ()).unwrap_err(),
        Error::InvalidParameter
    );
}

/// "---- the good table" to "teardown in MACsecOrch's order".
#[test]
fn the_good_table_as_macsecorch_drives_it() {
    // ---- the good table. Port 0 = lane 1 = PHY 0x10, port 1 = lane 5 = PHY 0x11.
    let mut w = World::new(&ms_table(true, true)).unwrap();
    assert!(w.m.active());
    // every engine read 0 = "not in bypass" (the fake's registers start at 0): all four were reset and disabled
    assert_eq!(w.phy.writes(), "110=1 100=1 111=1 101=1 110=1 100=1 111=1 101=1");
    assert_eq!(
        w.phy.names_take(),
        [
            "probe.egress",
            "reset.egress",
            "disable.egress",
            "probe.ingress",
            "reset.ingress",
            "disable.ingress",
            "probe.egress",
            "reset.egress",
            "disable.egress",
            "probe.ingress",
            "reset.ingress",
            "disable.ingress",
        ]
    );
    assert!(w.log_text().contains("4 engines were left on by a previous process"));
    let (p0, p2, hw0) = (w.port(0), w.port(2), w.key(0));
    // SAI_OPENBCM_PORT_IFG = 128: the port code programmed the quirk's gap at switch creation
    w.phy.quirk_ifg = 128;
    w.phy.ifg.insert(hw0, 128);
    for phy in [0x10, 0x11] {
        // the SA sequences poll 1.2df for 1
        for r in [0x20f, 0x21f] {
            w.phy.reg(phy, r, 1);
        }
    }

    // MACSEC objects and what MACsecOrch asks them
    let m_e =
        w.m.create_macsec(&[
            en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR),
            boolean(sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true),
        ])
        .unwrap();
    let m_i =
        w.m.create_macsec(&[
            en(sys::SAI_MACSEC_ATTR_DIRECTION, ING),
            boolean(sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true),
        ])
        .unwrap();
    assert_eq!(
        w.m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, ING)]),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        w.m.create_macsec(&[
            en(sys::SAI_MACSEC_ATTR_DIRECTION, ING),
            boolean(sys::SAI_MACSEC_ATTR_ENABLE_POST, true)
        ]),
        Err(not_supported(1))
    );
    assert_value!(
        get1(
            w.m.get_macsec(&w.phy, m_i, &[sys::SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL])
                .unwrap()
        ),
        Value::Bool(false)
    );
    assert_value!(
        get1(w.m.get_macsec(&w.phy, m_e, &[sys::SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC]).unwrap()),
        Value::Enum(v) if v == e(sys::SAI_MACSEC_MAX_SECURE_ASSOCIATIONS_PER_SC_FOUR)
    );
    assert_value!(
        get1(
            w.m.get_macsec(&w.phy, m_e, &[sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED])
                .unwrap()
        ),
        Value::Bool(true)
    );

    // MACSEC_PORT: the engine comes out of bypass (after a reset), the inter-frame gap widens
    let mport = |dir, port| {
        [
            en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port_raw(port)),
        ]
    };
    assert_eq!(
        w.m.create_macsec_port(&mut w.phy, &mport(EGR, p2)),
        Err(Error::NotSupported) // not in the table
    );
    let qc0 = w.phy.ifg_calls;
    let mp_e = w.m.create_macsec_port(&mut w.phy, &mport(EGR, p0)).unwrap();
    assert_eq!(w.phy.writes(), "110=1 100=0");
    assert_eq!(w.phy.names_take(), ["reset.egress", "enable.egress"]);
    assert_eq!(w.phy.ifg[&hw0], 384);
    // the gap alone: no interface / autoneg re-set (a link flap on a live port)
    assert_eq!(w.phy.ifg_calls, qc0 + 1);
    let mp_i = w.m.create_macsec_port(&mut w.phy, &mport(ING, p0)).unwrap();
    assert_eq!(w.phy.writes(), "111=1 101=0");
    assert_eq!(
        w.m.create_macsec_port(&mut w.phy, &mport(ING, p0)),
        Err(Error::ItemAlreadyExists)
    );
    // the gap survives what re-applies the port quirks (a speed set): the port code asks port_ifg
    let bits = w.m.port_ifg(hw0);
    sai_macsec::SeqRunner::set_ifg(&mut w.phy, hw0, bits).unwrap();
    assert_eq!(w.phy.ifg[&hw0], 384);
    w.phy.log_take();
    w.phy.runs_take();
    w.m.check_invariants().unwrap();

    // flows, the MACsec ACL tables with MACsecOrch's entries, bound to the port: nothing reaches the ASIC or the PHY
    let f_e =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)])
            .unwrap();
    let f_i =
        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, ING)])
            .unwrap();
    let dirs = [Dir::Egress, Dir::Ingress];
    let (mut tbl, mut eapol, mut data) = (Vec::new(), Vec::new(), Vec::new());
    for (d, dir) in dirs.into_iter().enumerate() {
        let stage = if d == 1 {
            sys::SAI_ACL_STAGE_INGRESS_MACSEC
        } else {
            sys::SAI_ACL_STAGE_EGRESS_MACSEC
        };
        let attrs = [
            en(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage)),
            Attr::new(
                sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                Value::I32List(vec![e(sys::SAI_ACL_BIND_POINT_TYPE_PORT)]),
            ),
            boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC, true),
            boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, true),
        ];
        assert!(Macsec::is_macsec_stage(&attrs));
        tbl.push(w.m.create_acl_table(&attrs).unwrap());
        eapol.push(
            w.m.create_acl_entry(
                &mut w.phy,
                &[
                    oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, tbl[d].raw()),
                    u32v(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 1000),
                    boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true),
                    ether_type(0x888e),
                    action_s32(
                        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                        true,
                        e(sys::SAI_PACKET_ACTION_FORWARD),
                    ),
                ],
            )
            .unwrap(),
        );
        w.m.set_port_acl(&mut w.phy, p0, dir, tbl[d].raw()).unwrap();
        assert_eq!(w.m.port_acl(p0, dir), tbl[d].raw());
    }
    assert!(w.phy.log_take().is_empty());
    assert_eq!(
        w.m.set_port_acl(&mut w.phy, p0, Dir::Egress, tbl[1].raw()),
        Err(invalid_value(0)) // an ingress table on the egress side
    );

    // SCs (the SCI as SONiC passes it: bytes in memory order), each with MACsecOrch's data entry (drop)
    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, 0x01]);
    let sc_attrs = |dir, flow: RawOid, cipher: u32| {
        vec![
            en(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir),
            oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, flow),
            u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, sci),
            boolean(sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, true),
            boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE, true),
            en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(cipher)),
        ]
    };
    {
        let mut v = sc_attrs(EGR, f_e.raw(), sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128);
        v[3] = boolean(sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, false); // integrity only: not what the sequences do
        assert_eq!(w.m.create_sc(&v), Err(not_supported(3)));
        assert_eq!(
            w.m.create_sc(&sc_attrs(EGR, f_i.raw(), sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)),
            Err(Error::InvalidParameter) // an ingress flow
        );
    }
    let sc_e =
        w.m.create_sc(&sc_attrs(EGR, f_e.raw(), sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128))
            .unwrap();
    assert_eq!(
        w.m.create_sc(&sc_attrs(EGR, f_e.raw(), sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)),
        Err(Error::InsufficientResources) // one SC per flow
    );
    let mut vi = sc_attrs(ING, f_i.raw(), sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256);
    vi.push(boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, true));
    vi.push(u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, 0x1234));
    let sc_i = w.m.create_sc(&vi).unwrap();
    for table in &tbl {
        data.push(
            w.m.create_acl_entry(
                &mut w.phy,
                &[
                    oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, table.raw()),
                    u32v(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 1),
                    boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true),
                    action_s32(
                        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                        true,
                        e(sys::SAI_PACKET_ACTION_DROP),
                    ),
                ],
            )
            .unwrap(),
        );
    }
    assert!(w.phy.log_take().is_empty());
    w.m.check_invariants().unwrap();

    // An SA before its flow is bound to the port (MACsecOrch with the port not enabled yet): kept, nothing on the bus.
    // Everything logged from here to the end of the key tests is searched for key bytes.
    w.log.lock().unwrap().clear();
    let sa_e0 =
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, sc_e.raw(), 0, false, false, 7))
            .unwrap();
    assert!(w.phy.log_take().is_empty());
    assert!(w.m.status(&w.phy).contains("waiting for their flow"));
    assert_eq!(w.m.sa_holds_key(sa_e0), Some(true));
    assert_value!(
        get1(
            w.m.get_sa(&mut w.phy, sa_e0, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
                .unwrap()
        ),
        Value::U64(7) // as configured: not in the PHY yet
    );
    // The flow becomes active (setMACsecFlowActive): the SA goes in as the SC's first, then the gate opens.
    w.m.set_acl_entry(
        &mut w.phy,
        data[0],
        &action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, f_e.raw()),
    )
    .unwrap();
    w.m.set_acl_entry(
        &mut w.phy,
        data[0],
        &action_s32(
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
            false,
            e(sys::SAI_PACKET_ACTION_DROP),
        ),
    )
    .unwrap();
    //         marker: first, cipher 0, AN 0 | key[1].key[0] | key[15].key[14] | hkey[15].hkey[0] | sci[0].sci[7] | pn | pn hi | window
    assert_eq!(
        w.phy.writes(),
        "200=100 201=c1c0 202=cfce 203=8f80 204=1c01 205=7 206=0 207=0 120=1"
    );
    let runs = w.phy.runs_take();
    assert_eq!(
        runs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["sa-add-first.egress.0.gcm-aes-128", "gate-open.egress"]
    );
    assert_eq!(runs[0].port, hw0);
    assert_eq!(
        runs[0].params.keys().map(String::as_str).collect::<Vec<_>>(),
        ["hkey", "key", "pn", "sci", "window"]
    );
    assert_eq!(runs[0].param("key"), "c0c1c2c3c4c5c6c7c8c9cacbcccdcecf"); // the last 16 of SAI's 32
    assert_eq!(runs[0].param("hkey"), "808182838485868788898a8b8c8d8e8f");
    assert_eq!(runs[0].param("sci"), "1cea0b382bd50001"); // as on the wire
    assert_eq!(runs[0].param("pn"), "0700000000000000"); // least significant byte first
    assert_eq!(runs[0].param("window"), "00000000"); // no replay protection
    assert!(runs[1].params.is_empty());
    assert!(!w.m.status(&w.phy).contains("waiting"));
    assert_eq!(w.m.sa_holds_key(sa_e0), Some(false)); // the PHY has the key now
    // rekey: AN 1 created when it becomes the encoding AN; it transmits; then the old one goes (no bus access needed to switch back)
    let sa_e1 =
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, sc_e.raw(), 1, false, false, 1))
            .unwrap();
    assert_eq!(
        w.phy.writes(),
        "200=1 201=c2c1 202=d0cf 203=8f80 204=1c01 205=1 206=0 207=0"
    );
    assert_eq!(w.phy.names_take(), ["sa-add.egress.1.gcm-aes-128"]);
    assert_value!(
        get1(w.m.get_sc(sc_e, &[sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID]).unwrap()),
        Value::Oid(o) if o == sa_e1.raw()
    );
    assert_eq!(
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, sc_e.raw(), 1, false, false, 1)),
        Err(Error::ItemAlreadyExists)
    );
    w.m.remove_sa(&mut w.phy, sa_e0).unwrap();
    assert_eq!(w.phy.writes(), "300=1");
    assert_eq!(w.phy.names_take(), ["sa-del.egress.0"]);
    w.m.check_invariants().unwrap();
    // counters: the PHY clears on read, the SAI value accumulates; PN from the PHY (32 bit for a non-XPN suite)
    let sa_stat = |w: &mut World, id, stat| w.m.get_sa_stats(&mut w.phy, id, &[stat]).unwrap()[0];
    w.phy.reg(0x10, 0x402, 5);
    assert_eq!(sa_stat(&mut w, sa_e1, sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED), 5);
    w.phy.reg(0x10, 0x402, 3);
    w.phy.reg(0x10, 0x403, 1);
    assert_eq!(
        sa_stat(&mut w, sa_e1, sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED),
        5 + 3 + 0x10000
    );
    assert_eq!(sa_stat(&mut w, sa_e1, sys::SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED), 0); // not in this table
    w.phy.reg(0x10, 0x4f0, 9);
    let control = [sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS];
    assert_eq!(w.m.get_flow_stats(&mut w.phy, f_e, &control).unwrap(), [9]);
    assert_eq!(w.m.get_flow_stats(&mut w.phy, f_e, &control).unwrap(), [18]);
    w.phy.reg(0x10, 0x504, 0x2222);
    w.phy.reg(0x10, 0x505, 0x1111);
    w.phy.reg(0x10, 0x506, 0xffff); // beyond 32 bit: ignored for gcm-aes-128
    assert_value!(
        get1(
            w.m.get_sa(&mut w.phy, sa_e1, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
                .unwrap()
        ),
        Value::U64(0x1111_2222)
    );
    assert_eq!(
        w.m.get_sa(&mut w.phy, sa_e1, &[sys::SAI_MACSEC_SA_ATTR_SAK])
            .unwrap_err(),
        not_supported(0) // write-only
    );
    // out_pkts_encrypted, in_pkts_ok: what this table reads
    assert_eq!(
        w.m.stats_capability(&w.phy, sys::SAI_OBJECT_TYPE_MACSEC_SA).unwrap(),
        [
            sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED,
            sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK
        ]
    );
    w.phy.log_take();
    w.phy.runs_take();

    // ingress, flow active first (the port enabled): XPN-256 with replay protection, the SA goes in at once
    w.m.set_acl_entry(
        &mut w.phy,
        data[1],
        &action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, f_i.raw()),
    )
    .unwrap();
    assert!(w.phy.log_take().is_empty()); // no SA yet: the gate stays closed
    w.phy.reg(0x10, 0x310, 0xab00);
    let sa_i2 =
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, sc_i.raw(), 2, true, true, 0x1_0000_0002))
            .unwrap();
    //      first, cipher 3, AN 2 | key | key[31].key[30] | hkey | sci | pn lo | pn hi | window | iv[0].iv[3] (salt ^ ssci) | iv[4].iv[11] | ssci[0].salt[0] | kept & ff00 | 3
    assert_eq!(
        w.phy.writes(),
        "210=132 211=c3c2 212=e1e0 213=8f80 214=1c01 215=2 216=1 217=1234 218=5157 219=545b 21a=150 310=ab03 121=1"
    );
    let runs = w.phy.runs_take();
    assert_eq!(
        runs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["sa-add-first.ingress.2.gcm-aes-xpn-256", "gate-open.ingress"] // this table has no .replay variant of it
    );
    assert_eq!(
        runs[0].params.keys().map(String::as_str).collect::<Vec<_>>(),
        ["hkey", "iv", "key", "pn", "salt", "sci", "ssci", "window"]
    );
    assert_eq!(
        runs[0].param("key"),
        "c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1"
    );
    assert_eq!(runs[0].param("salt"), "505152535455565758595a5b");
    assert_eq!(runs[0].param("ssci"), "01020304"); // most significant byte first
    assert_eq!(runs[0].param("iv"), "515351575455565758595a5b"); // salt with the SSCI XOR-ed into bytes 0..3
    assert_eq!(runs[0].param("pn"), "0200000001000000");
    assert_eq!(runs[0].param("window"), "34120000");
    let sa_i3 =
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, sc_i.raw(), 3, true, true, 1))
            .unwrap();
    assert_eq!(
        w.phy.writes(),
        "210=33 211=c4c3 212=e2e1 213=8f80 214=1c01 215=1 216=0 217=1234 218=5157 219=545b 21a=150 310=ab04"
    );
    assert_eq!(
        w.m.set_sc(
            sc_i,
            &en(
                sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                e(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)
            )
        ),
        Err(Error::ObjectInUse) // not under installed SAs
    );
    w.m.set_sa(sa_i3, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 99))
        .unwrap(); // accepted, not written
    assert!(w.phy.log_take().is_empty());
    // a sequence that fails (the PHY does not acknowledge): the SA is not created, nothing of it stays
    w.phy.reg(0x10, 0x21f, 0);
    let before = w.m.counts();
    assert_eq!(
        w.m.create_sa(&mut w.phy, &sa_attrs(ING, sc_i.raw(), 0, true, true, 1)),
        Err(Error::Failure)
    );
    assert_eq!(w.m.counts(), before);
    assert!(w.m.status(&w.phy).contains("sa-add.ingress.0.gcm-aes-xpn-256")); // the last failure, by name
    w.phy.reg(0x10, 0x21f, 1);
    w.phy.log_take();
    w.phy.runs_take();
    w.m.check_invariants().unwrap();

    // the key reached the bus (above) and nothing else: not the log, not the status text
    {
        let (text, st) = (w.log_text(), w.m.status(&w.phy));
        assert!(text.contains("SA AN 2 installed")); // the log was captured
        for k in [
            "c0c1", "c1c0", "c2c3", "c3c2", "8081", "8f80", "5051", "5157", "c0 c1", "c0:c1", "0xc0",
        ] {
            assert!(!text.contains(k), "{k} in the log:\n{text}");
            assert!(!st.contains(k), "{k} in the status text:\n{st}");
        }
        let debug = format!("{:?}", w.m);
        assert!(!debug.contains("c0") && !debug.contains("192"), "{debug}");
    }

    // removal: the transmitting egress SA is also the last: gate closed first, then the "last" sequence
    w.m.remove_sa(&mut w.phy, sa_e1).unwrap();
    assert_eq!(w.phy.writes(), "120=0 301=8001");
    assert_eq!(w.phy.names_take(), ["gate-close.egress", "sa-del-last.egress.1"]);
    w.m.remove_sa(&mut w.phy, sa_i2).unwrap();
    assert_eq!(w.phy.writes(), "312=1");
    w.m.remove_sa(&mut w.phy, sa_i3).unwrap();
    assert_eq!(w.phy.writes(), "121=0 313=8001");
    // a new first SA after that: installed as first again, gate open again (the flow is still active)
    let sa_e2 =
        w.m.create_sa(&mut w.phy, &sa_attrs(EGR, sc_e.raw(), 2, false, false, 1))
            .unwrap();
    assert_eq!(
        w.phy.writes(),
        "200=102 201=c3c2 202=d1d0 203=8f80 204=1c01 205=1 206=0 207=0 120=1"
    );
    // the flow goes inactive (MACsecOrch: port disabled): the gate closes, the SA stays
    w.m.set_acl_entry(
        &mut w.phy,
        data[0],
        &action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, false, RawOid::NULL),
    )
    .unwrap();
    assert_eq!(w.phy.writes(), "120=0");
    assert_eq!(w.m.remove_sc(sc_e), Err(Error::ObjectInUse));
    assert_eq!(w.m.remove_flow(f_e), Err(Error::ObjectInUse));
    assert_eq!(w.m.remove_acl_table(tbl[0]), Err(Error::ObjectInUse));
    w.m.remove_sa(&mut w.phy, sa_e2).unwrap();
    assert_eq!(w.phy.writes(), "302=8001");
    w.m.check_invariants().unwrap();

    // teardown in MACsecOrch's order; the port's engines go back to bypass and the gap to the quirk's value
    for (d, dir) in dirs.into_iter().enumerate() {
        w.m.remove_acl_entry(&mut w.phy, data[d]).unwrap();
        w.m.remove_acl_entry(&mut w.phy, eapol[d]).unwrap();
        w.m.set_port_acl(&mut w.phy, p0, dir, RawOid::NULL).unwrap();
        w.m.remove_acl_table(tbl[d]).unwrap();
    }
    w.m.remove_sc(sc_e).unwrap();
    w.m.remove_sc(sc_i).unwrap();
    w.m.remove_flow(f_e).unwrap();
    w.m.remove_flow(f_i).unwrap();
    assert_eq!(w.m.remove_macsec(m_e), Err(Error::ObjectInUse));
    w.m.remove_macsec_port(&mut w.phy, mp_e).unwrap();
    assert_eq!(w.phy.writes(), "100=1");
    assert_eq!(w.phy.ifg[&hw0], 384); // the ingress MACsec port is still there
    w.m.remove_macsec_port(&mut w.phy, mp_i).unwrap();
    assert_eq!(w.phy.writes(), "101=1");
    assert_eq!(w.phy.ifg[&hw0], 128);
    w.m.remove_macsec(m_e).unwrap();
    w.m.remove_macsec(m_i).unwrap();
    assert!(
        w.m.status(&w.phy)
            .contains("0 macsec, 0 ports, 0 flows, 0 SCs, 0 SAs, 0 ACL tables, 0 ACL entries")
    );
    w.m.check_invariants().unwrap();
}

/// "---- a table without gate sequences and without probe: nothing at creation, SAs still work"
#[test]
fn a_table_without_gate_sequences_and_without_probe() {
    let mut w = World::new(&ms_table(false, false)).unwrap();
    assert!(w.phy.log_take().is_empty());
    let (p0, hw0) = (w.port(0), w.key(0));
    w.phy.reg(0x10, 0x20f, 1);
    let m_e = w.m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)]).unwrap();
    let mp_e =
        w.m.create_macsec_port(
            &mut w.phy,
            &[
                en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
                oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port_raw(p0)),
            ],
        )
        .unwrap();
    assert_eq!(w.phy.ifg[&hw0], 384);
    w.m.remove_macsec_port(&mut w.phy, mp_e).unwrap();
    assert_eq!(w.phy.ifg[&hw0], 96); // no quirk: the Ethernet default
    w.m.remove_macsec(m_e).unwrap();
}

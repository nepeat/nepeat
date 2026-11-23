//! The runner on its own: ports, parameters, results, errors, the gap; property tests for the
//! parameter path (every key byte reaches the bus as the table places it, whatever the bytes).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    reason = "test code: a failed unwrap is a failed test"
)]

use phy_seq::{Bus, Dev, MacsecTable};
use proptest::prelude::*;
use sai_core::Error;
use sai_core::oid::{PortId, RawOid};
use sai_macsec::{CounterMode, EngineAddr, Params, PortKey, SeqError, SeqRunner, SwitchPort, param};
use sai_macsec_phy::{BusStatus, NoIfg, PhySeqRunner, model_table, phy_params, status_error};
use sai_sys as sys;
use std::collections::BTreeSet;

#[derive(Default)]
struct Rec {
    reads: u16,
    writes: Vec<(u16, u8, u16, u16)>,
    delays: Vec<u32>,
    dead: BTreeSet<u16>,
}

impl Bus for Rec {
    type Error = BusStatus;
    fn read(&mut self, phy: u16, _dev: Dev, _reg: u16) -> Result<u16, BusStatus> {
        if self.dead.contains(&phy) {
            return Err(BusStatus(-1));
        }
        Ok(self.reads)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        if self.dead.contains(&phy) {
            return Err(BusStatus(sys::SAI_STATUS_TABLE_FULL));
        }
        self.writes.push((phy, dev.get(), reg, value));
        Ok(())
    }
    fn delay_us(&mut self, us: u32) {
        self.delays.push(us);
    }
}

fn port(i: u32, first_lane: u32) -> SwitchPort {
    SwitchPort {
        id: PortId::try_from(RawOid((u64::from(sys::SAI_OBJECT_TYPE_PORT) << 56) | u64::from(i + 1))).unwrap(),
        key: PortKey(100 + i),
        name: format!("ce{i}"),
        lanes: (first_lane..first_lane + 4).collect(),
    }
}

const TABLE: &str = "ext-macsec 1\ncounters absolute\nifg-bits 384\nport 1 1e0\nport 5 1e1\nport 9 1e2\n\
seq sa-add.egress.0.gcm-aes-256\n\
W p 1 a000 key[1].key[0]\nW p 1 a001 key[31].key[30]\nW p 1 a002 hkey[15].sci[7]\nW p 1 a003 pn[7].pn[0]\n\
W p 1 a004 salt[11].ssci[0]\nW p 1 a005 iv[0].window[3]\nD 5\nW 7 1f ffff 12.34\n\
seq stats-sa.egress.0\nX p 1 10 in_pkts_ok[0]\nX p 1 11 in_pkts_ok[1]\nX p 1 12 $3\nX p 1 13 octets[2]\n\
seq check\nE p 1 20 5 f\n";

fn runner() -> PhySeqRunner<Rec, NoIfg> {
    let table = MacsecTable::parse(TABLE.as_bytes()).unwrap();
    // two switch ports start at lane 5: the last one is the table's (ext_macsec_attach)
    let ports = [port(0, 1), port(1, 5), port(2, 5)];
    PhySeqRunner::new(table, &ports, Rec::default(), NoIfg)
}

fn full_params(key: &[u8; 32], hkey: &[u8; 16]) -> Params {
    let mut p = Params::new();
    p.set(param::KEY, key);
    p.set(param::HKEY, hkey);
    p.set(param::SCI, &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]);
    p.set(param::PN, &0x0123_4567_89ab_cdefu64.to_le_bytes());
    p.set(param::SALT, &[0x50; 12]);
    p.set(param::SSCI, &[1, 2, 3, 4]);
    p.set(param::IV, &[0x51; 12]);
    p.set(param::WINDOW, &64u32.to_le_bytes());
    p
}

#[test]
fn ports_map_by_first_lane_and_the_last_match_wins() {
    let r = runner();
    assert_eq!(r.phy_of(PortKey(100)), Some(0x1e0));
    assert_eq!(r.phy_of(PortKey(101)), None);
    assert_eq!(r.phy_of(PortKey(102)), Some(0x1e1));
    // lane 9: no switch port starts there
    assert_eq!(r.phy_of(PortKey(103)), None);
}

#[test]
fn the_model_table_is_the_parsed_one() {
    let t = model_table(runner().table());
    assert_eq!(t.counters, CounterMode::Absolute);
    assert_eq!(t.ifg_bits, 384);
    assert_eq!(
        t.engines,
        [
            EngineAddr { lane: 1, phy: 0x1e0 },
            EngineAddr { lane: 5, phy: 0x1e1 },
            EngineAddr { lane: 9, phy: 0x1e2 }
        ]
    );
}

#[test]
fn names_and_result_names() {
    let r = runner();
    assert_eq!(
        r.sequence_names(),
        ["check", "sa-add.egress.0.gcm-aes-256", "stats-sa.egress.0"]
    );
    assert!(r.has("check") && !r.has("probe.egress"));
    assert_eq!(
        r.result_names("stats-sa.egress.0"),
        ["in_pkts_ok", "in_pkts_ok", "octets"]
    );
    assert!(r.result_names("nope").is_empty());
}

#[test]
fn key_bytes_reach_the_bus_as_the_table_places_them() {
    let mut r = runner();
    let key: [u8; 32] = core::array::from_fn(|i| 0xc0 + i as u8);
    let hkey: [u8; 16] = core::array::from_fn(|i| 0x80 + i as u8);
    let out = r
        .run(PortKey(100), "sa-add.egress.0.gcm-aes-256", &full_params(&key, &hkey))
        .unwrap();
    assert!(out.names().next().is_none());
    assert_eq!(
        r.bus().writes,
        [
            (0x1e0, 1, 0xa000, 0xc1c0),
            (0x1e0, 1, 0xa001, 0xdfde),
            (0x1e0, 1, 0xa002, 0x8f17),
            (0x1e0, 1, 0xa003, 0x01ef),
            (0x1e0, 1, 0xa004, 0x5001),
            (0x1e0, 1, 0xa005, 0x5100),
            (7, 0x1f, 0xffff, 0x1234),
        ]
    );
    assert_eq!(r.bus().delays, [5]);
}

#[test]
fn results_are_words_least_significant_first() {
    let mut r = runner();
    r.bus_mut().reads = 0xbeef;
    let out = r.run(PortKey(102), "stats-sa.egress.0", &Params::new()).unwrap();
    assert_eq!(out.u64("in_pkts_ok"), 0xbeef_beef);
    assert_eq!(out.u64("octets"), 0xbeef_0000_0000); // words 0 and 1 never read: 0
    assert_eq!(out.names().collect::<Vec<_>>(), ["in_pkts_ok", "octets"]); // $3 is no result
}

#[test]
fn errors_carry_the_cpp_status_and_message() {
    let mut r = runner();
    // a missing parameter: nothing accessed
    let mut p = Params::new();
    p.set(param::KEY, &[0; 16]); // key[31] is not there
    let e = r.run(PortKey(100), "sa-add.egress.0.gcm-aes-256", &p).unwrap_err();
    assert_eq!(
        e,
        SeqError {
            status: Error::InvalidParameter,
            why: "line 9: parameter key[31] is not available".into()
        }
    );
    assert!(r.bus().writes.is_empty());
    // a check that does not hold
    let e = r.run(PortKey(100), "check", &Params::new()).unwrap_err();
    assert_eq!(e.status, Error::Failure);
    assert_eq!(
        e.why,
        "line 22: E phy 0x1e0 1.0020 read 0x0000, expected 0x0005 (mask 0x000f, 1 reads)"
    );
    // a bus error: its own status, the number in the message
    r.bus_mut().dead.insert(0x1e0);
    let e = r.run(PortKey(100), "check", &Params::new()).unwrap_err();
    assert_eq!(
        e,
        SeqError {
            status: Error::Failure,
            why: "line 22: E phy 0x1e0 1.0020: MDIO error -1".into()
        }
    );
    let key = [0u8; 32];
    let e = r
        .run(
            PortKey(100),
            "sa-add.egress.0.gcm-aes-256",
            &full_params(&key, &[0; 16]),
        )
        .unwrap_err();
    assert_eq!(e.status, Error::TableFull);
    assert_eq!(
        e.why,
        format!("line 8: W phy 0x1e0 1.a000: MDIO error {}", sys::SAI_STATUS_TABLE_FULL)
    );
    // what the model never asks for
    assert_eq!(
        r.run(PortKey(101), "check", &Params::new()).unwrap_err().status,
        Error::NotSupported
    );
    assert_eq!(
        r.run(PortKey(102), "nope", &Params::new()).unwrap_err().status,
        Error::NotSupported
    );
}

#[test]
fn statuses_map_to_errors() {
    for e in [
        Error::Failure,
        Error::NotSupported,
        Error::InvalidParameter,
        Error::TableFull,
        Error::ObjectInUse,
        Error::InvalidObjectId,
        Error::ItemNotFound,
        Error::Uninitialized,
    ] {
        assert_eq!(status_error(e.status()), e);
    }
    // what sai_core::Error cannot say (BCM_E_MEMORY, BCM_E_PORT through map_rv): FAILURE
    assert_eq!(status_error(sys::SAI_STATUS_NO_MEMORY), Error::Failure);
    assert_eq!(status_error(sys::SAI_STATUS_INVALID_PORT_NUMBER), Error::Failure);
    assert_eq!(BusStatus(-7).to_string(), "-7");
}

#[test]
fn parameters_unknown_names_are_dropped_and_long_ones_cut() {
    let mut p = Params::new();
    p.set("bogus", &[1, 2, 3]);
    p.set(param::SSCI, &[1, 2, 3, 4, 5, 6]);
    let out = phy_params(&p);
    assert_eq!(out.len(phy_seq::ParamName::Ssci), 4);
    assert_eq!(format!("{out:?}"), "{\"ssci\": <4 bytes>}");
}

#[test]
fn debug_shows_no_parameter_and_no_table_bytes() {
    let r = runner();
    let text = format!("{r:?}");
    assert!(text.contains("sequences: 3") && !text.contains("key"), "{text}");
}

#[test]
fn the_gap_goes_to_the_sink() {
    let table = MacsecTable::parse(TABLE.as_bytes()).unwrap();
    let mut seen = Vec::new();
    let mut r = PhySeqRunner::new(table, &[port(0, 1)], Rec::default(), |p: PortKey, bits| {
        seen.push((p, bits));
        Ok(())
    });
    r.set_ifg(PortKey(100), 384).unwrap();
    r.set_ifg(PortKey(100), 0).unwrap();
    drop(r);
    assert_eq!(seen, [(PortKey(100), 384), (PortKey(100), 0)]);
}

proptest! {
    /// Whatever the bytes and lengths: a word is (hi << 8) | lo of the named bytes, or the run
    /// stops before the first access when a named byte is missing.
    #[test]
    fn every_parameter_byte_lands_where_the_table_says(
        key in proptest::collection::vec(any::<u8>(), 0..40),
        hkey in proptest::collection::vec(any::<u8>(), 0..20),
        rest in proptest::collection::vec(any::<u8>(), 12..13),
    ) {
        let mut r = runner();
        let mut p = Params::new();
        p.set(param::KEY, &key);
        p.set(param::HKEY, &hkey);
        p.set(param::SCI, &rest[..8]);
        p.set(param::PN, &rest[..8]);
        p.set(param::SALT, &rest);
        p.set(param::SSCI, &rest[..4]);
        p.set(param::IV, &rest);
        p.set(param::WINDOW, &rest[..4]);
        let out = r.run(PortKey(100), "sa-add.egress.0.gcm-aes-256", &p);
        if key.len() < 32 || hkey.len() < 16 {
            prop_assert_eq!(out.unwrap_err().status, Error::InvalidParameter);
            prop_assert!(r.bus().writes.is_empty());
        } else {
            prop_assert!(out.is_ok());
            let w = |hi: u8, lo: u8| (u16::from(hi) << 8) | u16::from(lo);
            let words: Vec<u16> = r.bus().writes.iter().map(|x| x.3).collect();
            prop_assert_eq!(
                words,
                vec![
                    w(key[1], key[0]),
                    w(key[31], key[30]),
                    w(hkey[15], rest[7]),
                    w(rest[7], rest[0]),
                    w(rest[11], rest[0]),
                    w(rest[0], rest[3]),
                    0x1234,
                ]
            );
        }
    }
}

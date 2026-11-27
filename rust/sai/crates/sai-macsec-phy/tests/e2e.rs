//! End to end against the golden of the C++: the whole Rust path (the `sai-macsec` model, this
//! crate's runner, the `phy-seq` executor on a recording fake bus) runs scripts of SAI object
//! operations over a table, and per operation the SAI status, the values read (packet numbers,
//! counters, lists, capabilities, the inter-frame gap), the diag status text (failure messages
//! included), and every bus event in order must be the same text the C++ printed: each read with the
//! value read, each write with the value written (key words included: this is where a wrong byte
//! order of a key or a packet number would show), each delay, each access the bus refused.
//!
//! The C++ side (`sai_macsec.cpp` + `sai_extmacsec.cpp` on the fake backend, through the SAI API:
//! `sai-macsec/tests/cpp/driver.cpp`, built with `-DSAI_MACSEC_DRIVER_CHIP`) was deleted in H5 on
//! 2026-11-27; it was last in the tree at commit 767d9b0, and its outputs are the goldens in
//! `golden/e2e-*.txt` (captured 2026-11-26; frozen, never regenerated). Tables: a synthetic one made
//! here (always), and the board's real one when `SAI_MACSEC_E2E_TABLE` names it (the AS7772 table as
//! generated for the system, `sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_MACSEC_FILE`;
//! read where it is, never copied; the golden pins its length and FNV-1a, and its cases are skipped,
//! said on stderr, where it is not named: Nix generates that table, the golden cannot carry it).
//!
//! Scripts never move a flow between ports (variables of one parity only meet the port of that
//! parity): there the two differ on purpose (sai-macsec's D1). For sai-macsec's D9 and D10 (the
//! owner's fixes of the C++) the Rust output must equal, to the end of every script, the golden's
//! "fixed" output (the C++ with exactly those edits, `listed_fixes.rs` at commit 767d9b0): the
//! scripts where that differs from the unmodified C++ ("plain") are the ones that reach D9 / D10.
//!
//! The random scripts are deterministic per seed (sai-macsec's `tests/common/seeded.rs`): the
//! golden's are seeds 20261126 and up (synthetic) and 20261226 and up (AS7772).

#![allow(
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::format_collect,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    reason = "test code: a failed unwrap is a failed test"
)]

use phy_seq::{Bus, Dev, MacsecTable};
use proptest::prelude::*;
use sai_core::oid::{PortId, RawOid, TypedId};
use sai_macsec::{
    AclEntryId, AclField, AclParameter, AclTableId, Attr, Dir, FlowId, Macsec, MacsecId, MacsecPortId, Options,
    PortKey, SaId, ScId, Secret, SeqError, SwitchPort, Value,
};
use sai_macsec_phy::{BusStatus, IfgSink, PhySeqRunner, model_table};
use sai_sys as sys;
#[path = "../../sai-macsec/tests/common/golden.rs"]
mod golden;
#[path = "../../sai-macsec/tests/common/seeded.rs"]
mod seeded;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

/// A line for whoever runs the suite: on stderr, and appended to the file `$SAI_RS_TEST_SUMMARY`
/// names (the target-test bundle collects these; captured stderr of a passing test is not shown).
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}
/// The fake switch of the driver: 32 ports of 4 lanes (the AS7772 table names 32).
const PORTS: u32 = 32;
/// SAI_STATUS_FAILURE: what the C++ fake bus returns for a PHY marked dead.
const DEAD: i32 = -1;

// ------------------------------------------------------------------ the C++ side

// ------------------------------------------------------------------ the Rust side

/// The C++ fake backend's MDIO bus (the driver's "chip" variant): a register file (a write is
/// read back, a register never written reads 0), registers whose reads see a set value and not
/// the writes (a FIFO), PHYs that can be marked dead (every access fails with FAILURE and is
/// not logged by the fake), plus the log in the driver's format.
#[derive(Default)]
struct RecBus {
    regs: BTreeMap<(u16, u8, u16), u16>,
    rd_only: BTreeMap<(u16, u8, u16), u16>,
    dead: BTreeSet<u16>,
    log: String,
}

impl Bus for RecBus {
    type Error = BusStatus;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        if self.dead.contains(&phy) {
            write!(self.log, " !R{phy:x}.{:x}.{reg:x}", dev.get()).unwrap();
            return Err(BusStatus(DEAD));
        }
        let key = (phy, dev.get(), reg);
        let v = self
            .rd_only
            .get(&key)
            .or_else(|| self.regs.get(&key))
            .copied()
            .unwrap_or(0);
        write!(self.log, " R{phy:x}.{:x}.{reg:x}={v:x}", dev.get()).unwrap();
        Ok(v)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        if self.dead.contains(&phy) {
            write!(self.log, " !W{phy:x}.{:x}.{reg:x}", dev.get()).unwrap();
            return Err(BusStatus(DEAD));
        }
        self.regs.insert((phy, dev.get(), reg), value);
        write!(self.log, " W{phy:x}.{:x}.{reg:x}={value:x}", dev.get()).unwrap();
        Ok(())
    }
    fn delay_us(&mut self, us: u32) {
        write!(self.log, " D{us}").unwrap();
    }
}

/// The C++ `port_ifg_apply` on the fake backend without a board gap quirk: the MACsec gap,
/// else 96 bit times if the port had the MACsec gap, else nothing.
#[derive(Default)]
struct FakeIfg {
    ifg: BTreeMap<PortKey, u32>,
    macsec: BTreeMap<PortKey, bool>,
}

impl IfgSink for FakeIfg {
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError> {
        let had = self.macsec.insert(port, macsec_bits != 0).unwrap_or(false);
        let want = if macsec_bits != 0 {
            macsec_bits
        } else if had {
            96
        } else {
            0
        };
        if want != 0 {
            self.ifg.insert(port, want);
        }
        Ok(())
    }
}

fn switch_ports() -> Vec<SwitchPort> {
    (0..PORTS)
        .map(|i| SwitchPort {
            id: PortId::try_from(RawOid((u64::from(sys::SAI_OBJECT_TYPE_PORT) << 56) | u64::from(i + 1))).unwrap(),
            key: PortKey(i + 1),
            name: format!("ce{i}"),
            lanes: (0..4).map(|l| i * 4 + l + 1).collect(),
        })
        .collect()
}

struct World {
    m: Macsec,
    r: PhySeqRunner<RecBus, FakeIfg>,
    ports: Vec<SwitchPort>,
}

fn take(w: &mut World) -> String {
    std::mem::take(&mut w.r.bus_mut().log)
}

fn num(s: &str) -> u64 {
    u64::from_str_radix(s, 16).unwrap()
}
fn dir_of(s: &str) -> i32 {
    (if s == "1" {
        sys::SAI_MACSEC_DIRECTION_INGRESS
    } else {
        sys::SAI_MACSEC_DIRECTION_EGRESS
    }) as i32
}
fn a(id: sys::sai_attr_id_t, value: Value) -> Attr {
    Attr::new(id, value)
}
fn e(v: u32) -> Value {
    Value::Enum(v as i32)
}

struct Vars(BTreeMap<String, RawOid>);

impl Vars {
    fn get<T: TryFrom<RawOid>>(&self, name: &str) -> Option<T> {
        self.0
            .get(name)
            .map(|raw| T::try_from(*raw).ok().unwrap_or_else(|| panic!("{name}: another type")))
    }
    fn raw(&self, name: &str) -> Option<RawOid> {
        self.0.get(name).copied()
    }
    fn name_of(&self, raw: RawOid) -> String {
        if raw.is_null() {
            return "-".to_owned();
        }
        self.0
            .iter()
            .find(|(_, v)| **v == raw)
            .map_or_else(|| "?".to_owned(), |(k, _)| k.clone())
    }
    fn names(&self, v: &Value) -> String {
        let Value::OidList(list) = v else {
            panic!("not a list: {v:?}")
        };
        if list.is_empty() {
            return "none".to_owned();
        }
        list.iter().map(|o| self.name_of(*o)).collect::<Vec<_>>().join(",")
    }
}

/// The driver's key material for seed `seed` (driver.cpp, op "sa").
fn sa_attrs(dir: i32, sc: RawOid, an: u8, pn: u64, seed: u8) -> Vec<Attr> {
    let bytes = |base: u8, n: usize| -> Vec<u8> { (0..n).map(|i| base.wrapping_add(i as u8)).collect() };
    let pn_attr = if dir == sys::SAI_MACSEC_DIRECTION_EGRESS as i32 {
        sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN
    } else {
        sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
    };
    vec![
        a(sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, Value::Enum(dir)),
        a(sys::SAI_MACSEC_SA_ATTR_SC_ID, Value::Oid(sc)),
        a(sys::SAI_MACSEC_SA_ATTR_AN, Value::U8(an)),
        a(pn_attr, Value::U64(pn)),
        a(
            sys::SAI_MACSEC_SA_ATTR_SAK,
            Value::Sak(Secret::from_slice(&bytes(seed.wrapping_mul(37), 32)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::AuthKey(Secret::from_slice(&bytes(0x80u8.wrapping_add(seed), 16)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_SALT,
            Value::Salt(Secret::from_slice(&bytes(0x50u8.wrapping_add(seed), 12)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI,
            Value::U32(0x0102_0304 + u32::from(seed)),
        ),
    ]
}

fn action(id: sys::sai_attr_id_t, enable: bool, parameter: AclParameter) -> Attr {
    a(id, Value::AclAction { enable, parameter })
}

/// What the driver prints for the same table and script, from the Rust path.
fn rust_output(table_text: &[u8], script: &str) -> String {
    let table = MacsecTable::parse(table_text).unwrap();
    let ports = switch_ports();
    let mut r = PhySeqRunner::new(table.clone(), &ports, RecBus::default(), FakeIfg::default());
    let m = match Macsec::attach(&model_table(&table), &ports, Options::default(), &mut r) {
        Ok(m) => m,
        Err(err) => return format!("attach => {}\n", err.status()),
    };
    let mut w = World { m, r, ports };
    let mut out = String::new();
    writeln!(out, "attach => 0{}", take(&mut w)).unwrap();
    let mut vars = Vars(BTreeMap::new());
    for line in script.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let Some(op) = t.first().copied() else { continue };
        let mut extra = String::new();
        let status: Option<sai_core::Result<()>> = (|| {
            let creates = matches!(op, "macsec" | "mport" | "flow" | "table" | "entry" | "sc" | "sa");
            if creates && vars.0.contains_key(t[1]) {
                return None;
            }
            let created = |vars: &mut Vars, r: sai_core::Result<RawOid>| {
                r.map(|raw| {
                    vars.0.insert(t[1].to_owned(), raw);
                })
            };
            let port = |w: &World, i: &str| w.ports[usize::try_from(num(i)).unwrap()].clone();
            Some(match op {
                "macsec" => {
                    let r =
                        w.m.create_macsec(&[a(sys::SAI_MACSEC_ATTR_DIRECTION, Value::Enum(dir_of(t[2])))]);
                    created(&mut vars, r.map(Into::into))
                }
                "mport" => {
                    let p = port(&w, t[3]);
                    let attrs = [
                        a(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2]))),
                        a(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, Value::Oid(p.id.raw())),
                    ];
                    let r = w.m.create_macsec_port(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "flow" => {
                    let r =
                        w.m.create_flow(&[a(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2])))]);
                    created(&mut vars, r.map(Into::into))
                }
                "table" => {
                    let stage = if t[2] == "1" {
                        sys::SAI_ACL_STAGE_INGRESS_MACSEC
                    } else {
                        sys::SAI_ACL_STAGE_EGRESS_MACSEC
                    };
                    let r = w.m.create_acl_table(&[
                        a(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage)),
                        a(
                            sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                            Value::I32List(vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT as i32]),
                        ),
                        a(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, Value::Bool(true)),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "bind" => {
                    let table = if t[3] == "-" { RawOid::NULL } else { vars.raw(t[3])? };
                    let p = port(&w, t[1]);
                    let dir = if t[2] == "1" { Dir::Ingress } else { Dir::Egress };
                    w.m.set_port_acl(&mut w.r, p.id, dir, table)
                }
                "entry" => {
                    let table = vars.raw(t[2])?;
                    let flow = if t[4] == "-" { None } else { Some(vars.raw(t[4])?) };
                    let mut attrs = vec![
                        a(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(table)),
                        a(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, Value::U32(1)),
                        a(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(t[3] == "1")),
                    ];
                    if t[6] == "1" {
                        attrs.push(a(
                            sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE,
                            Value::AclField(Some(AclField::U16 {
                                data: 0x888e,
                                mask: 0xffff,
                            })),
                        ));
                    }
                    if t[5] != "0" {
                        let act = if t[5] == "1" {
                            sys::SAI_PACKET_ACTION_DROP
                        } else {
                            sys::SAI_PACKET_ACTION_FORWARD
                        };
                        attrs.push(action(
                            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                            true,
                            AclParameter::S32(act as i32),
                        ));
                    }
                    if let Some(flow) = flow {
                        attrs.push(action(
                            sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                            true,
                            AclParameter::Oid(flow),
                        ));
                    }
                    let r = w.m.create_acl_entry(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "eflow" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    let flow = if t[2] == "-" { RawOid::NULL } else { vars.raw(t[2])? };
                    let x = action(
                        sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                        t[2] != "-",
                        AclParameter::Oid(flow),
                    );
                    w.m.set_acl_entry(&mut w.r, entry, &x)
                }
                "eadmin" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    w.m.set_acl_entry(
                        &mut w.r,
                        entry,
                        &a(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(t[2] == "1")),
                    )
                }
                "sc" => {
                    let flow = vars.raw(t[3])?;
                    // the driver's SCI: these eight bytes in memory order, as a u64
                    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, num(t[7]) as u8]);
                    let r = w.m.create_sc(&[
                        a(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2]))),
                        a(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, Value::Oid(flow)),
                        a(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, Value::U64(sci)),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                            Value::Enum(num(t[4]) as i32),
                        ),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE,
                            Value::Bool(t[5] == "1"),
                        ),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            Value::U32(num(t[6]) as u32),
                        ),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "scset" => {
                    let sc = vars.get::<ScId>(t[1])?;
                    let x = match t[2] {
                        "cipher" => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                            Value::Enum(num(t[3]) as i32),
                        ),
                        "replay" => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE,
                            Value::Bool(t[3] == "1"),
                        ),
                        _ => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            Value::U32(num(t[3]) as u32),
                        ),
                    };
                    w.m.set_sc(sc, &x)
                }
                "sa" => {
                    let sc = vars.raw(t[3])?;
                    let attrs = sa_attrs(dir_of(t[2]), sc, num(t[4]) as u8, num(t[5]), num(t[6]) as u8);
                    let r = w.m.create_sa(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "saminpn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.set_sa(
                        sa,
                        &a(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, Value::U64(num(t[2]))),
                    )
                }
                "sapn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.get_sa(&mut w.r, sa, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
                        .map(|v| {
                            let Value::U64(pn) = v[0] else { panic!() };
                            write!(extra, " pn={pn:x}").unwrap();
                        })
                }
                "sastat" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED,
                        sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK,
                    ];
                    w.m.get_sa_stats(&mut w.r, sa, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "flowstat" => {
                    let flow = vars.get::<FlowId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS,
                        sys::SAI_MACSEC_FLOW_STAT_OTHER_ERR,
                    ];
                    w.m.get_flow_stats(&mut w.r, flow, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "saclr" => w.m.clear_sa_stats(&mut w.r, vars.get::<SaId>(t[1])?),
                "flowclr" => w.m.clear_flow_stats(&mut w.r, vars.get::<FlowId>(t[1])?),
                "active" => {
                    let sc = vars.get::<ScId>(t[1])?;
                    w.m.get_sc(sc, &[sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID]).map(|v| {
                        let Value::Oid(sa) = v[0] else { panic!() };
                        write!(extra, " active={}", vars.name_of(sa)).unwrap();
                    })
                }
                "lists" => {
                    let flow = vars.get::<FlowId>(t[1])?;
                    w.m.get_flow(
                        flow,
                        &[
                            sys::SAI_MACSEC_FLOW_ATTR_SC_LIST,
                            sys::SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST,
                        ],
                    )
                    .map(|v| write!(extra, " scs={} entries={}", vars.names(&v[0]), vars.names(&v[1])).unwrap())
                }
                "caps" => {
                    let m = vars.get::<MacsecId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA,
                    ];
                    let sa =
                        w.m.stats_capability(&w.r, sys::SAI_OBJECT_TYPE_MACSEC_SA)
                            .unwrap()
                            .len();
                    let flow =
                        w.m.stats_capability(&w.r, sys::SAI_OBJECT_TYPE_MACSEC_FLOW)
                            .unwrap()
                            .len();
                    w.m.get_macsec(&w.r, m, &ids).map(|v| {
                        extra.push_str(" caps=");
                        for b in &v[..4] {
                            let Value::Bool(b) = b else { panic!() };
                            extra.push(if *b { '1' } else { '0' });
                        }
                        let Value::U32(n) = v[4] else { panic!() };
                        write!(extra, " sas={n} stats={sa},{flow}").unwrap();
                    })
                }
                "rm" => {
                    let r = match t[1] {
                        "macsec" => w.m.remove_macsec(vars.get::<MacsecId>(t[2])?),
                        "mport" => w.m.remove_macsec_port(&mut w.r, vars.get::<MacsecPortId>(t[2])?),
                        "flow" => w.m.remove_flow(vars.get::<FlowId>(t[2])?),
                        "sc" => w.m.remove_sc(vars.get::<ScId>(t[2])?),
                        "sa" => w.m.remove_sa(&mut w.r, vars.get::<SaId>(t[2])?),
                        "table" => w.m.remove_acl_table(vars.get::<AclTableId>(t[2])?),
                        _ => w.m.remove_acl_entry(&mut w.r, vars.get::<AclEntryId>(t[2])?),
                    };
                    if r != Err(sai_core::Error::ObjectInUse) {
                        vars.0.remove(t[2]);
                    }
                    r
                }
                "reg" => {
                    let key = (num(t[1]) as u16, 1u8, num(t[2]) as u16);
                    w.r.bus_mut().regs.insert(key, num(t[3]) as u16);
                    Ok(())
                }
                "rdreg" => {
                    let key = (num(t[1]) as u16, 1u8, num(t[2]) as u16);
                    if t[3] == "-" {
                        w.r.bus_mut().rd_only.remove(&key);
                    } else {
                        w.r.bus_mut().rd_only.insert(key, num(t[3]) as u16);
                    }
                    Ok(())
                }
                "dead" => {
                    let phy = num(t[1]) as u16;
                    if t[2] == "1" {
                        w.r.bus_mut().dead.insert(phy);
                    } else {
                        w.r.bus_mut().dead.remove(&phy);
                    }
                    Ok(())
                }
                "ifg" => {
                    let key = port(&w, t[1]).key;
                    write!(extra, " ifg={}", w.r.ifg().ifg.get(&key).copied().unwrap_or(0)).unwrap();
                    Ok(())
                }
                "status" => {
                    for l in w.m.status(&w.r).lines() {
                        write!(extra, "\n| {l}").unwrap();
                    }
                    Ok(())
                }
                other => panic!("unknown operation {other}"),
            })
        })();
        match status {
            None => writeln!(out, "{line} => skip").unwrap(),
            Some(st) => {
                let code = st.err().map_or(0, sai_core::Error::status);
                let extra = if code == 0 || op == "status" {
                    extra
                } else {
                    String::new()
                };
                writeln!(out, "{line} => {code}{extra}{}", take(&mut w)).unwrap();
            }
        }
        w.m.check_invariants()
            .unwrap_or_else(|why| panic!("after '{line}': {why}"));
    }
    out
}

/// The status text carries each engine's longest run time: wall clock, not comparable. It is
/// the 7 characters before column 76 of a port line (`%-8s %4x  %-7s %-6s %-6s %-3s x5 %5llu %5llu %7llu`).
fn normalise(text: &str) -> String {
    text.lines()
        .map(|l| {
            let body = l.strip_prefix("| ").unwrap_or("");
            let port_line =
                body.len() >= 76 && (body.get(15..22) == Some("egress ") || body.get(15..22) == Some("ingress"));
            if port_line && body.is_char_boundary(69) && body.is_char_boundary(76) {
                format!("| {}      ?{}\n", &body[..69], &body[76..])
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

// ------------------------------------------------------------------ tables and scripts

/// A synthetic table (no relation to any chip): every sequence the model runs, reads that
/// keep results and scratch values, polls, checks, delays, every parameter byte. Register
/// 1.20f / 1.21f must read 1 for an install to complete, 1.1fN = 0 for the other sequences.
fn synthetic_table() -> String {
    let mut t = String::from("ext-macsec 1\n# synthetic\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n");
    let ciphers = ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"];
    for (d, dn) in ["egress", "ingress"].iter().enumerate() {
        writeln!(t, "seq probe.{dn}\nX p 1 10{d} bypass[0]").unwrap();
        writeln!(t, "seq reset.{dn}\nE p 1 1f6 0\nW p 1 11{d} 1").unwrap();
        writeln!(t, "seq enable.{dn}\nE p 1 1f4 0\nW p 1 10{d} 0\nD 10").unwrap();
        writeln!(t, "seq disable.{dn}\nE p 1 1f1 0\nW p 1 10{d} 1").unwrap();
        writeln!(t, "seq gate-open.{dn}\nE p 1 1f3 0\nW p 1 12{d} 1").unwrap();
        writeln!(t, "seq gate-close.{dn}\nW p 1 12{d} 0").unwrap();
        writeln!(t, "seq stats-flow.{dn}\nE p 1 1f5 0\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 other_err[3]").unwrap();
        for an in 0..4 {
            for (c, cipher) in ciphers.iter().enumerate() {
                let (xpn, k256) = (c >= 2, c & 1 == 1);
                for first in ["", "-first"] {
                    writeln!(
                        t,
                        "seq sa-add{first}.{dn}.{an}.{cipher}\nR p 1 2{d}0\nW p 1 2{d}0 {:x}",
                        (c << 4) | an
                    )
                    .unwrap();
                    let klen = if k256 { 32 } else { 16 };
                    for i in (0..klen).step_by(2) {
                        writeln!(t, "W p 1 6{d}{:02x} key[{}].key[{i}]", i / 2, i + 1).unwrap();
                    }
                    for i in (0..16).step_by(2) {
                        writeln!(t, "W p 1 7{d}{:02x} hkey[{i}].hkey[{}]", i / 2, i + 1).unwrap();
                    }
                    writeln!(t, "W p 1 2{d}4 sci[0].sci[1]\nW p 1 2{d}5 sci[6].sci[7]\nW p 1 2{d}6 pn[1].pn[0]\nW p 1 2{d}7 pn[3].pn[2]").unwrap();
                    writeln!(t, "W p 1 2{d}8 window[1].window[0]\nW p 1 2{d}9 window[3].window[2]").unwrap();
                    if xpn {
                        writeln!(t, "W p 1 2{d}a pn[5].pn[4]\nW p 1 2{d}b pn[7].pn[6]").unwrap();
                        for i in (0..12).step_by(2) {
                            writeln!(t, "W p 1 8{d}{:02x} iv[{i}].salt[{}]", i / 2, i + 1).unwrap();
                        }
                        writeln!(t, "W p 1 2{d}c ssci[0].ssci[1]\nW p 1 2{d}d ssci[2].ssci[3]").unwrap();
                    }
                    writeln!(t, "X p 1 310 $0\nW p 1 310 $0&ff00|{:x}\nP p 1 2{d}f 1 1 3 7", an + 1).unwrap();
                }
            }
            writeln!(t, "seq sa-add-first.{dn}.{an}.gcm-aes-128.replay\nW p 1 2{d}0 f00\nW p 1 2{d}8 window[1].window[0]\nP p 1 2{d}f 1 1 2 0").unwrap();
            writeln!(
                t,
                "seq sa-del.{dn}.{an}\nE p 1 1f2 0\nW p 1 3{d}{an} 1\nseq sa-del-last.{dn}.{an}\nW p 1 3{d}{an} 8001"
            )
            .unwrap();
            writeln!(t, "seq pn.{dn}.{an}\nE p 1 1f5 0").unwrap();
            for k in 0..4 {
                writeln!(t, "X p 1 5{d}{} pn[{k}]", an * 4 + k).unwrap();
            }
            let stat = if d == 1 { "in_pkts_ok" } else { "out_pkts_encrypted" };
            writeln!(
                t,
                "seq stats-sa.{dn}.{an}\nX p 1 4{d}{} {stat}[0]\nX p 1 4{d}{} {stat}[1]",
                an * 2,
                an * 2 + 1
            )
            .unwrap();
        }
    }
    t
}

/// Board-specific knowledge the scripts need: the PHY address words of ports 0 and 1, and the
/// registers (and values) whose preset decides whether a sequence completes.
struct Board {
    phys: [&'static str; 2],
    head: String,
    regs: Vec<&'static str>,
    values: Vec<u16>,
    /// Groups of script lines a random operation may be (the read-back words of an install).
    groups: Vec<String>,
}

fn synthetic_board() -> Board {
    let head = "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\n".to_owned();
    Board {
        phys: ["10", "11"],
        head,
        regs: vec![
            "20f", "21f", "1f1", "1f2", "1f3", "1f4", "1f5", "1f6", "310", "400", "410", "4f0", "4f1", "4f2", "500",
            "501", "504", "510",
        ],
        values: vec![0, 1, 5, 0xab00, 0xffff],
        groups: vec!["status".to_owned()],
    }
}

/// The AS7772 table: ports 0 and 1 are cages with PHYs 0x17c / 0x17d (first lanes 1 and 5).
/// The LMI status (1.900a) and the message acknowledge (1.8221) are preset so that most
/// sequences complete, and the LMI data FIFO (1.9005) reads 0, not the last command word; the
/// scripts change these and the read-back words (1.a101, 1.a102) so that sequences also fail
/// at their checks.
fn as7772_board(table: &str) -> Board {
    let mut head = String::new();
    for phy in ["17c", "17d"] {
        writeln!(
            head,
            "reg {phy} 900a 7\nreg {phy} 8221 12e3\nreg {phy} a101 e066\nrdreg {phy} 9005 0"
        )
        .unwrap();
    }
    head.push_str("macsec m0 0\nmacsec m1 1\n");
    Board {
        phys: ["17c", "17d"],
        head,
        regs: vec![
            "900a", "8221", "a101", "a102", "a101", "a102", "9005", "a103", "a104", "a000",
        ],
        values: readback_values(table),
        groups: (0..2usize)
            .flat_map(|q| {
                (0..2usize).flat_map(move |d| (0..4usize).flat_map(move |an| (0..4usize).map(move |c| (q, d, an, c))))
            })
            .map(|(q, d, an, c)| readback(table, ["17c", "17d"][q], d, an, c).trim_end().to_owned())
            .collect(),
    }
}

/// The values the table's sequences check read-back words against (`E p 1 a101 / a102 ...`: the
/// PHY's answer to a message), plus a few others.
fn readback_values(table: &str) -> Vec<u16> {
    let mut v: BTreeSet<u16> = [0, 1, 7, 0x12e3, 0xffff].into_iter().collect();
    for l in table.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        if let ["E", "p", "1", "a101" | "a102", val, ..] = w.as_slice() {
            v.insert(u16::from_str_radix(val, 16).unwrap());
        }
    }
    v.into_iter().collect()
}

/// `reg` lines that make the read-back checks of the install sequence of (dir, an, cipher) hold
/// on PHY `phy` (what the chip would answer).
fn readback(table: &str, phy: &str, d: usize, an: usize, cipher: usize) -> String {
    let name = format!(
        "seq sa-add.{}.{an}.{}",
        ["egress", "ingress"][d],
        ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"][cipher]
    );
    let mut out = String::new();
    for l in table
        .lines()
        .skip_while(|l| *l != name)
        .skip(1)
        .take_while(|l| !l.starts_with("seq "))
    {
        let w: Vec<&str> = l.split_whitespace().collect();
        if let ["E", "p", "1", reg @ ("a101" | "a102"), val, ..] = w.as_slice() {
            writeln!(out, "reg {phy} {reg} {val}").unwrap();
        }
    }
    out
}

/// The backbones with the read-back words of each install set just before it.
fn as7772_backbones(table: &str, board: &Board) -> Vec<String> {
    let mut out = Vec::new();
    for cipher in 0..4 {
        let mut lines = Vec::new();
        for (q, d) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let x = q + 2 * d;
            for l in skeleton(q, d, cipher, d == 1) {
                let an = if l.starts_with(&format!("eflow e{x} ")) {
                    Some(0)
                } else if l.starts_with(&format!("sa a{} ", x + 4)) {
                    Some(1)
                } else if l.starts_with(&format!("sa a{} ", x + 8)) {
                    Some(2)
                } else {
                    None
                };
                if let Some(an) = an {
                    lines.push(readback(table, board.phys[q], d, an, cipher).trim_end().to_owned());
                }
                lines.push(l);
            }
        }
        lines.extend(teardown());
        out.push(format!("{}{}\n", board.head, lines.join("\n")));
    }
    out
}

/// MACsecOrch's order for port `q` (0, 1), direction `d`.
fn skeleton(q: usize, d: usize, cipher: usize, replay: bool) -> Vec<String> {
    let x = q + 2 * d;
    vec![
        format!("mport p{x} {d} {q}"),
        format!("flow f{x} {d}"),
        format!("table t{x} {d}"),
        format!("bind {q} {d} t{x}"),
        format!("entry e{x} t{x} 1 - 1 0"),
        format!("sc c{x} {d} f{x} {cipher} {} 1234 {x}", u8::from(replay)),
        format!("sa a{x} {d} c{x} 0 7 {x}"),
        format!("eflow e{x} f{x}"),
        format!("sa a{} {d} c{x} 1 1 {}", x + 4, x + 4),
        format!("sapn a{x}"),
        format!("sastat a{}", x + 4),
        format!("flowstat f{x}"),
        "status".to_owned(),
        format!("rm sa a{x}"),
        format!("sa a{} {d} c{x} 2 100000002 {}", x + 8, x + 8),
        format!("active c{x}"),
    ]
}

fn teardown() -> Vec<String> {
    let mut lines = Vec::new();
    for x in 0..4 {
        lines.push(format!("rm mport p{x}"));
        lines.push("status".to_owned());
        for v in [x, x + 4, x + 8] {
            lines.push(format!("rm sa a{v}"));
        }
        lines.extend([
            format!("rm sc c{x}"),
            format!("rm entry e{x}"),
            format!("bind {} {} -", x % 2, x / 2),
        ]);
        lines.extend([
            format!("rm table t{x}"),
            format!("rm flow f{x}"),
            format!("ifg {}", x % 2),
        ]);
    }
    lines.extend(["rm macsec m0", "rm macsec m1", "status"].map(str::to_owned));
    lines
}

/// One random operation; variables of index parity p only ever meet port p.
fn random_op(
    phys: [&'static str; 2],
    regs: Vec<&'static str>,
    values: Vec<u16>,
    groups: Vec<String>,
) -> impl Strategy<Value = String> {
    let d = 0..2usize;
    let reg = proptest::sample::select(regs);
    let value = prop_oneof![proptest::sample::select(values), any::<u16>()];
    prop_oneof![
        2 => (0..2usize, d.clone()).prop_map(|(v, d)| format!("macsec m{v} {d}")),
        4 => (0..5usize, d.clone(), 0..4usize).prop_map(|(v, d, q)| format!("mport p{v} {d} {q}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("flow f{v} {d}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("table t{v} {d}")),
        4 => (0..6usize, d.clone(), any::<bool>()).prop_map(|(k, d, unbind)| {
            if unbind { format!("bind {} {d} -", k % 2) } else { format!("bind {} {d} t{k}", k % 2) }
        }),
        4 => (0..6usize, 0..3usize, any::<bool>(), 0..4usize, 0..3usize, any::<bool>()).prop_map(
            |(i, k, admin, j, action, eapol)| {
                let (table, flow) = (2 * k + i % 2, 2 * (j % 3) + i % 2);
                let flow = if j == 3 { "-".to_owned() } else { format!("f{flow}") };
                format!("entry e{i} t{table} {} {flow} {action} {}", u8::from(admin), u8::from(eapol))
            }
        ),
        5 => (0..6usize, 0..4usize).prop_map(|(i, j)| {
            if j == 3 { format!("eflow e{i} -") } else { format!("eflow e{i} f{}", 2 * j + i % 2) }
        }),
        2 => (0..6usize, any::<bool>()).prop_map(|(i, b)| format!("eadmin e{i} {}", u8::from(b))),
        4 => (0..5usize, d.clone(), 0..6usize, 0..5usize, any::<bool>(), 0..3usize).prop_map(
            |(v, d, f, cipher, replay, win)| format!("sc c{v} {d} f{f} {cipher} {} {:x} {v}", u8::from(replay), win * 0x1234)
        ),
        3 => (0..5usize, 0..3usize, 0..5usize).prop_map(|(v, what, val)| match what {
            0 => format!("scset c{v} cipher {val}"),
            1 => format!("scset c{v} replay {}", val % 2),
            _ => format!("scset c{v} window {:x}", val * 0x1234),
        }),
        8 => (0..12usize, d.clone(), 0..5usize, 0..5usize, 0..4usize).prop_map(|(v, d, sc, an, pn)| {
            let pn = ["1", "7", "100000002", "fedcba9876543210"][pn];
            format!("sa a{v} {d} c{sc} {an} {pn} {v}")
        }),
        2 => (0..12usize, 0..3usize).prop_map(|(v, pn)| format!("saminpn a{v} {}", ["1", "63", "100000002"][pn])),
        3 => (0..12usize).prop_map(|v| format!("sapn a{v}")),
        3 => (0..12usize).prop_map(|v| format!("sastat a{v}")),
        1 => (0..12usize).prop_map(|v| format!("saclr a{v}")),
        3 => (0..6usize).prop_map(|v| format!("flowstat f{v}")),
        1 => (0..6usize).prop_map(|v| format!("flowclr f{v}")),
        2 => (0..5usize).prop_map(|v| format!("active c{v}")),
        2 => (0..6usize).prop_map(|v| format!("lists f{v}")),
        1 => (0..2usize).prop_map(|v| format!("caps m{v}")),
        3 => Just("status".to_owned()),
        1 => (0..4usize).prop_map(|q| format!("ifg {q}")),
        8 => (0..12usize).prop_map(|v| format!("rm sa a{v}")),
        3 => (0..5usize).prop_map(|v| format!("rm sc c{v}")),
        3 => (0..6usize).prop_map(|v| format!("rm flow f{v}")),
        3 => (0..6usize).prop_map(|v| format!("rm entry e{v}")),
        2 => (0..6usize).prop_map(|v| format!("rm table t{v}")),
        3 => (0..5usize).prop_map(|v| format!("rm mport p{v}")),
        1 => (0..2usize).prop_map(|v| format!("rm macsec m{v}")),
        8 => proptest::sample::select(groups),
        10 => (0..2usize, reg, value).prop_map(move |(p, reg, val)| format!("reg {} {reg} {val:x}", phys[p])),
        1 => (0..2usize, any::<bool>()).prop_map(move |(p, on)| format!("dead {} {}", phys[p], u8::from(on))),
        2 => (0..2usize, proptest::sample::select(vec!["0", "8000", "-"])).prop_map(move |(p, v)| format!("rdreg {} 9005 {v}", phys[p])),
    ]
}

fn script(board: &Board, max_random: usize) -> impl Strategy<Value = String> {
    let head = board.head.clone();
    (
        proptest::collection::vec((0..2usize, 0..2usize, 0..4usize, any::<bool>()), 1..4),
        proptest::collection::vec(
            (
                any::<prop::sample::Index>(),
                random_op(
                    board.phys,
                    board.regs.clone(),
                    board.values.clone(),
                    board.groups.clone(),
                ),
            ),
            0..max_random,
        ),
    )
        .prop_map(move |(backbones, random)| {
            let mut lines: Vec<String> = Vec::new();
            for (q, d, cipher, replay) in backbones {
                lines.extend(skeleton(q, d, cipher, replay));
            }
            for (at, op) in random {
                let at = at.index(lines.len() + 1);
                lines.insert(at, op);
            }
            lines.extend(["status".to_owned()]);
            format!("{head}{}\n", lines.join("\n"))
        })
}

fn backbones(board: &Board) -> Vec<String> {
    let mut out = Vec::new();
    for cipher in 0..4 {
        let mut lines = Vec::new();
        for (q, d) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            lines.extend(skeleton(q, d, cipher, d == 1));
        }
        lines.extend(teardown());
        out.push(format!("{}{}\n", board.head, lines.join("\n")));
    }
    out
}

/// sai-macsec's D9 script `d9_another_entry_takes_over_after_the_entry_was_disabled` at MDIO level (synthetic table).
fn d9_script() -> String {
    format!(
        "{}flow f5 1\nmport p3 1 1\nflow f3 1\ntable t3 1\nbind 1 1 t3\nentry e3 t3 1 - 1 0\nsc c3 1 f3 0 0 1234 3\n\
         sa a3 1 c3 0 7 3\neflow e3 f3\neadmin e3 0\nsa a7 1 c3 1 1 7\nentry e1 t3 1 f5 0 0\nsapn a3\nstatus\n\
         rm sa a3\nsa a11 1 c3 2 100000002 11\nactive c3\nstatus\n",
        synthetic_board().head
    )
}

// ------------------------------------------------------------------ the golden C++ side (H5-0)
//
// The C++ side is held in files captured from it (2026-11-26, header of the files; the C++ is
// deleted, last in the tree at commit 767d9b0): the
// scripts ('golden/e2e-inputs.txt') and, per script, the normalised bus-access outputs of the
// unmodified C++ and of the C++ with D9 / D10 as digests plus the full text of a few cases
// ('golden/e2e-cpp.txt'). The Rust path is held to them ('rust_agrees_with_the_golden_cpp').
// The AS7772 table is not in the golden (Nix generates it): its cases run only when SAI_MACSEC_E2E_TABLE names a file of the pinned length and FNV-1a.

const GOLDEN_INPUTS: &str = include_str!("golden/e2e-inputs.txt");
const GOLDEN_CPP: &str = include_str!("golden/e2e-cpp.txt");
/// The seeds of the golden's random scripts: first seed .. + count, longest random part.
const SYNTHETIC_SEEDS: (u64, u64, usize) = (20_261_126, 64, 80);
const AS7772_SEEDS: (u64, u64, usize) = (20_261_226, 48, 60);
/// The cases whose full outputs the golden keeps.
const GOLDEN_TEXT: &[&str] = &[
    "syn-bb-c0",
    "syn-bb-c1",
    "syn-bb-c2",
    "syn-bb-c3",
    "as-bb-c0",
    "as-bb-c1",
    "as-bb-c2",
    "as-bb-c3",
    "named-d9",
    "syn-rnd-20261141",
];

struct Input {
    id: String,
    tag: &'static str,
    script: String,
}

fn synthetic_inputs() -> Vec<Input> {
    let board = synthetic_board();
    let mut out: Vec<Input> = backbones(&board)
        .into_iter()
        .enumerate()
        .map(|(c, script)| Input {
            id: format!("syn-bb-c{c}"),
            tag: "synthetic",
            script,
        })
        .collect();
    out.push(Input {
        id: "named-d9".to_owned(),
        tag: "synthetic",
        script: d9_script(),
    });
    let (first, n, max) = SYNTHETIC_SEEDS;
    let strategy = script(&board, max);
    out.extend((0..n).map(|i| Input {
        id: format!("syn-rnd-{}", first + i),
        tag: "synthetic",
        script: seeded::value(&strategy, first + i),
    }));
    out
}

fn as7772_inputs(table: &str) -> Vec<Input> {
    let board = as7772_board(table);
    let mut out: Vec<Input> = as7772_backbones(table, &board)
        .into_iter()
        .enumerate()
        .map(|(c, script)| Input {
            id: format!("as-bb-c{c}"),
            tag: "as7772",
            script,
        })
        .collect();
    let (first, n, max) = AS7772_SEEDS;
    let strategy = script(&board, max);
    out.extend((0..n).map(|i| Input {
        id: format!("as-rnd-{}", first + i),
        tag: "as7772",
        script: seeded::value(&strategy, first + i),
    }));
    out
}

fn golden_files() -> (golden::Golden, golden::Inputs) {
    (
        golden::parse_golden(GOLDEN_CPP).unwrap_or_else(|e| panic!("golden/e2e-cpp.txt: {e}")),
        golden::parse_inputs(GOLDEN_INPUTS).unwrap_or_else(|e| panic!("golden/e2e-inputs.txt: {e}")),
    )
}

/// The AS7772 table SAI_MACSEC_E2E_TABLE names, when it is the one the golden pins; else None
/// (said on stderr: its cases of the golden are SKIPPED).
fn pinned_as7772(inputs: &golden::Inputs) -> Option<(PathBuf, String)> {
    let Some(path) = std::env::var_os("SAI_MACSEC_E2E_TABLE").map(PathBuf::from) else {
        summary("e2e golden: SAI_MACSEC_E2E_TABLE is not set: the AS7772 cases of the golden are SKIPPED");
        return None;
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let pinned = inputs.pinned.get("as7772").copied();
    if pinned != Some((text.len(), golden::fnv1a(&text))) {
        summary(&format!(
            "e2e golden: {} is not the AS7772 table the golden pins ({pinned:?}): its cases are SKIPPED",
            path.display()
        ));
        return None;
    }
    Some((path, text))
}

/// The table text of a golden case: stored, or the pinned AS7772 table (None: not available).
fn table_of(inputs: &golden::Inputs, tag: &str, as7772: Option<&(PathBuf, String)>) -> Option<String> {
    if tag == "as7772" {
        as7772.map(|(_, text)| text.clone())
    } else {
        Some(
            inputs
                .tables
                .get(tag)
                .unwrap_or_else(|| panic!("no stored table {tag}"))
                .clone(),
        )
    }
}

/// The stored inputs are what the generators of this file make for the fixed seeds (the
/// AS7772 part with the pinned table, when it is there).
#[test]
fn the_golden_inputs_are_the_fixed_corpus() {
    let (golden, inputs) = golden_files();
    assert_eq!(inputs.tables.get("synthetic"), Some(&synthetic_table()));
    let mut corpus = synthetic_inputs();
    let have_as7772 = pinned_as7772(&inputs);
    if let Some((_, text)) = &have_as7772 {
        corpus.extend(as7772_inputs(text));
    }
    for input in &corpus {
        let stored = inputs
            .scripts
            .iter()
            .find(|(id, ..)| *id == input.id)
            .unwrap_or_else(|| panic!("no stored script {}", input.id));
        assert_eq!(
            (input.tag, &input.script),
            (stored.1.as_str(), &stored.2),
            "{}",
            input.id
        );
    }
    assert_eq!(inputs.scripts.len(), golden.cases.len());
    assert_eq!(inputs.scripts.len(), 4 + 1 + 64 + 4 + 48);
    assert_eq!(corpus.len(), if have_as7772.is_some() { 121 } else { 69 });
    // the golden keeps full texts for exactly the cases named in GOLDEN_TEXT
    let kept: BTreeSet<&str> = golden.texts.keys().map(|(id, _)| id.as_str()).collect();
    assert_eq!(kept, GOLDEN_TEXT.iter().copied().collect());
}

/// The Rust path against the golden C++: per script the Rust output
/// (every SAI status, value, status text and bus access in order) equals the C++ with D9 / D10,
/// to the end; where that differs from the unmodified C++ the script reached D9 / D10.
#[test]
fn rust_agrees_with_the_golden_cpp() {
    let (golden, inputs) = golden_files();
    let as7772 = pinned_as7772(&inputs);
    let (mut compared, mut ops, mut events, mut listed, mut installs) = (0, 0, 0, 0, 0);
    for case in &golden.cases {
        let Some(table) = table_of(&inputs, &case.tag, as7772.as_ref()) else {
            continue;
        };
        let script = &inputs.scripts.iter().find(|(id, ..)| *id == case.id).unwrap().2;
        let rust = normalise(&rust_output(table.as_bytes(), script));
        let reached =
            golden::rust_vs_golden(&golden, case, &rust).unwrap_or_else(|why| panic!("{why}\n--- script\n{script}"));
        listed += usize::from(reached.is_some());
        compared += 1;
        ops += rust.lines().filter(|l| l.contains(" => ")).count();
        events += rust.matches(" R").count() + rust.matches(" W").count();
        // not vacuous on the board's table: engines on, SAs installed, gates open
        if case.id.starts_with("as-bb-c") {
            installs +=
                rust.matches(" SA AN ").count() + rust.lines().filter(|l| l.contains(" on     open   sa")).count();
        }
    }
    summary(&format!(
        "e2e golden: the Rust path equals the golden C++ (with D9 / D10) on {compared} scripts, {ops} operations, \
         {events} bus accesses; {listed} reached D9 / D10"
    ));
    assert!(compared >= 69);
    if as7772.is_some() {
        assert!(installs >= 16, "the AS7772 backbones installed too little ({installs})");
    }
}

/// The D9 case at MDIO level: after `eadmin e3 0`, a second entry sends port 1's ingress data to a flow
/// without an SC. The C++ opens the gate on the old SC's SA (Q7), the Rust takes that SA out (D9) and
/// leaves the gate closed; the C++ with D9 does the same as the Rust, to the end of the script.
#[test]
fn the_golden_keeps_the_d9_difference() {
    let (golden, inputs) = golden_files();
    let case = golden.cases.iter().find(|c| c.id == "named-d9").unwrap();
    let script = &inputs.scripts.iter().find(|(id, ..)| *id == case.id).unwrap().2;
    let rust = rust_output(inputs.tables["synthetic"].as_bytes(), script);
    let cpp = &golden.texts[&("named-d9".to_owned(), "plain".to_owned())];
    let fixed = &golden.texts[&("named-d9".to_owned(), "fixed".to_owned())];
    let line = |text: &str| text.lines().find(|l| l.starts_with("entry e1")).unwrap().to_owned();
    assert_eq!(line(cpp), "entry e1 t3 1 f5 0 0 => 0 R11.1.1f3=0 W11.1.121=1");
    assert_eq!(line(&rust), "entry e1 t3 1 f5 0 0 => 0 W11.1.310=8001");
    assert_eq!(line(fixed), line(&rust));
    assert!(case.listed.starts_with("D9@"), "{}", case.listed);
}

/// Negative controls of the golden: a changed byte, digest, count or class, or a changed input,
/// is reported by the comparison.
#[test]
fn the_golden_notices_a_change() {
    let (golden, inputs) = golden_files();
    let find = |id: &str| golden.cases.iter().find(|c| c.id == id).unwrap().clone();
    let rust_of = |id: &str, inputs: &golden::Inputs| {
        let script = &inputs.scripts.iter().find(|(i, ..)| i == id).unwrap().2;
        normalise(&rust_output(inputs.tables["synthetic"].as_bytes(), script))
    };
    for id in ["named-d9", "syn-bb-c3", "syn-rnd-20261126"] {
        assert!(
            golden::rust_vs_golden(&golden, &find(id), &rust_of(id, &inputs)).is_ok(),
            "{id}"
        );
    }
    // a changed byte of a kept text (a key word on the bus)
    let mut changed = golden.clone();
    let key = ("syn-bb-c3".to_owned(), "fixed".to_owned());
    let text = changed.texts.get_mut(&key).unwrap();
    let at = text.find("W10.1.6000=").unwrap() + "W10.1.6000=".len();
    text.replace_range(at..=at, "9");
    assert_ne!(changed.texts[&key], golden.texts[&key]);
    assert!(golden::rust_vs_golden(&changed, &find("syn-bb-c3"), &rust_of("syn-bb-c3", &inputs)).is_err());
    // a changed digest of a digest-only case, and a changed line count
    let rnd = find("syn-rnd-20261126");
    let rust = rust_of("syn-rnd-20261126", &inputs);
    let mut bad = rnd.clone();
    bad.fixed.fnv ^= 1;
    assert!(golden::rust_vs_golden(&golden, &bad, &rust).is_err());
    let mut bad = rnd.clone();
    bad.fixed.lines += 1;
    assert!(golden::rust_vs_golden(&golden, &bad, &rust).is_err());
    // a changed class: D9 recorded as none
    let mut bad = find("named-d9");
    assert_ne!(bad.listed, "none");
    bad.listed = "none".to_owned();
    assert!(golden::rust_vs_golden(&golden, &bad, &rust_of("named-d9", &inputs)).is_err());
    // a changed input: one key seed of the script
    let mut other = inputs.clone();
    let script = other.scripts.iter_mut().find(|(id, ..)| id == "syn-bb-c3").unwrap();
    assert!(script.2.contains("sa a4 0 c0 1 1 4"));
    script.2 = script.2.replacen("sa a4 0 c0 1 1 4", "sa a4 0 c0 1 1 5", 1);
    assert!(golden::rust_vs_golden(&golden, &find("syn-bb-c3"), &rust_of("syn-bb-c3", &other)).is_err());
}

// ------------------------------------------------------------------ tests

/// The Rust path alone, always: the backbone completes on the synthetic table, the key words
/// reach the bus in the table's byte order, and the model's invariants hold after each line.
#[test]
fn the_rust_path_runs_the_backbone() {
    let board = synthetic_board();
    let script = format!("{}{}\nstatus\n", board.head, skeleton(0, 0, 3, false).join("\n"));
    let out = rust_output(synthetic_table().as_bytes(), &script);
    assert!(!out.contains("skip"), "{out}");
    // sa a0 (seed 0): SAK bytes 0, 1, ... (seed * 37 + i); 256-bit XPN suite: key[1].key[0] = 0x0100
    let install = out.lines().find(|l| l.starts_with("eflow e0 f0 => 0")).unwrap();
    assert!(install.contains(" W10.1.6000=100 W10.1.6001=302"), "{install}");
    // pn 7: pn[1].pn[0] = 0x0007, salt 0x50.. with the SSCI 01 02 03 04 in iv[0..3]: iv[0] = 0x51
    assert!(install.contains(" W10.1.206=7 W10.1.207=0"), "{install}");
    assert!(install.contains(" W10.1.8000=5151"), "{install}");
    assert!(out.contains("active c0 => 0 active=a8"), "{out}");
}

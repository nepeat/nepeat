//! Shared test fixtures: a made-up MACsec PHY behind [`SeqRunner`], and attribute builders.
//!
//! The PHY is the one of the C++ test (`tests/test_macsec.inc`): [`ms_table`] writes the same
//! table text as its `ms_table()`, and [`FakePhy`] interprets that text on a register map
//! like the C++ fake backend's, so the bus-write strings of the C++ test (`MS_BUS`) can be
//! asserted verbatim. It also records every run with its parameter bytes. The interpreter
//! here is test code for well-formed tables only (a malformed one panics); the real parser
//! and executor are the `phy-seq` crate.

#![allow(
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::assert_is_empty,
    clippy::format_collect,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::write_with_newline,
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_panics_doc,
    reason = "test code: a failed unwrap is a failed test; not every test file uses every helper"
)]

use sai_core::oid::{PortId, RawOid, TypedId};
use sai_core::{Error, Result};
use sai_macsec::{
    AclField, AclParameter, Attr, CounterMode, EngineAddr, Level, Macsec, Options, Params, PortKey, Results, Secret,
    SeqError, SeqRunner, SwitchPort, Table, Value,
};
use sai_sys as sys;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

/// The table of the C++ test, text for text.
pub fn ms_table(gates: bool, probe: bool) -> String {
    let mut t =
        String::from("ext-macsec 1\n# test table\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n");
    let dirs = ["egress", "ingress"];
    let ciphers = ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"];
    for (d, dn) in dirs.iter().enumerate() {
        if probe {
            write!(t, "seq probe.{dn}\nX p 1 10{d} bypass[0]\n").unwrap();
        }
        write!(t, "seq reset.{dn}\nW p 1 11{d} 1\n").unwrap();
        write!(t, "seq enable.{dn}\nW p 1 10{d} 0\nD 10\n").unwrap();
        write!(t, "seq disable.{dn}\nW p 1 10{d} 1\n").unwrap();
        if gates {
            write!(t, "seq gate-open.{dn}\nW p 1 12{d} 1\n").unwrap();
            write!(t, "seq gate-close.{dn}\nW p 1 12{d} 0\n").unwrap();
        }
        write!(
            t,
            "seq stats-flow.{dn}\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 x_unknown[0]\n"
        )
        .unwrap();
        for an in 0..4usize {
            for (c, cipher) in ciphers.iter().enumerate() {
                let (xpn, k256) = (c >= 2, c & 1 == 1);
                for first in 0..2usize {
                    let marker = (first << 8) | (c << 4) | an;
                    write!(
                        t,
                        "seq sa-add{}.{dn}.{an}.{cipher}\nW p 1 2{d}0 {marker:x}\n",
                        if first == 1 { "-first" } else { "" }
                    )
                    .unwrap();
                    write!(
                        t,
                        "W p 1 2{d}1 key[1].key[0]\nW p 1 2{d}2 key[{}].key[{}]\nW p 1 2{d}3 hkey[15].hkey[0]\n",
                        if k256 { 31 } else { 15 },
                        if k256 { 30 } else { 14 }
                    )
                    .unwrap();
                    write!(
                        t,
                        "W p 1 2{d}4 sci[0].sci[7]\nW p 1 2{d}5 pn[1].pn[0]\nW p 1 2{d}6 {}\nW p 1 2{d}7 window[1].window[0]\n",
                        if xpn { "pn[5].pn[4]" } else { "0" }
                    )
                    .unwrap();
                    if xpn {
                        write!(
                            t,
                            "W p 1 2{d}8 iv[0].iv[3]\nW p 1 2{d}9 iv[4].iv[11]\nW p 1 2{d}a ssci[0].salt[0]\n"
                        )
                        .unwrap();
                    }
                    if d == 1 {
                        write!(t, "X p 1 310 $0\nW p 1 310 $0&ff00|{}\n", an + 1).unwrap();
                    }
                    write!(t, "P p 1 2{d}f 1 1 3 0\n").unwrap();
                }
            }
            if d == 1 && an == 0 {
                t.push_str(
                    "seq sa-add-first.ingress.0.gcm-aes-128.replay\nW p 1 210 f00\nW p 1 217 window[1].window[0]\nP p 1 21f 1 1 3 0\n",
                );
            }
            write!(
                t,
                "seq sa-del.{dn}.{an}\nW p 1 3{d}{an} 1\nseq sa-del-last.{dn}.{an}\nW p 1 3{d}{an} 8001\n"
            )
            .unwrap();
            write!(
                t,
                "seq pn.{dn}.{an}\nX p 1 5{d}{} pn[0]\nX p 1 5{d}{} pn[1]\nX p 1 5{d}{} pn[2]\nX p 1 5{d}{} pn[3]\n",
                an * 4,
                an * 4 + 1,
                an * 4 + 2,
                an * 4 + 3
            )
            .unwrap();
            let stat = if d == 1 { "in_pkts_ok" } else { "out_pkts_encrypted" };
            write!(
                t,
                "seq stats-sa.{dn}.{an}\nX p 1 4{d}{} {stat}[0]\nX p 1 4{d}{} {stat}[1]\n",
                an * 2,
                an * 2 + 1
            )
            .unwrap();
        }
    }
    t
}

#[derive(Clone, Debug)]
enum Byte {
    Const(u8),
    Param(String, usize),
}

#[derive(Clone, Debug)]
enum Val {
    Const(u16),
    Kept { n: usize, mask: u16, bits: u16 },
    Bytes(Byte, Byte),
}

#[derive(Clone, Debug)]
enum Dest {
    Kept(usize),
    Result(String, usize),
}

#[derive(Clone, Debug)]
enum Kind {
    Write(Val),
    Read,
    Expect { want: u16, mask: u16 },
    Poll { want: u16, mask: u16, tries: u32 },
    Extract(Dest),
    Delay,
}

#[derive(Clone, Debug)]
struct Op {
    line: usize,
    /// `None`: the PHY of the port the sequence runs for.
    phy: Option<u32>,
    dev: u32,
    reg: u32,
    kind: Kind,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("not hex: {s}"))
}

fn indexed(s: &str) -> Option<(String, usize)> {
    let (name, rest) = s.split_once('[')?;
    Some((name.to_owned(), rest.strip_suffix(']')?.parse().ok()?))
}

fn byte(s: &str) -> Byte {
    indexed(s).map_or_else(
        || Byte::Const(u8::try_from(hex(s)).unwrap()),
        |(n, i)| Byte::Param(n, i),
    )
}

fn value(s: &str) -> Val {
    if let Some(rest) = s.strip_prefix('$') {
        let (rest, bits) = rest.split_once('|').map_or((rest, 0), |(r, b)| (r, hex(b)));
        let (n, mask) = rest.split_once('&').map_or((rest, 0xffff), |(n, m)| (n, hex(m)));
        return Val::Kept {
            n: n.parse().unwrap(),
            mask: u16::try_from(mask).unwrap(),
            bits: u16::try_from(bits).unwrap(),
        };
    }
    match s.split_once('.') {
        Some((hi, lo)) => Val::Bytes(byte(hi), byte(lo)),
        None => Val::Const(u16::try_from(hex(s)).unwrap()),
    }
}

/// A parsed test table.
#[derive(Clone, Debug, Default)]
pub struct FakeTable {
    pub clear_on_read: bool,
    pub ifg_bits: u32,
    /// (first lane, phy)
    pub ports: Vec<(u32, u32)>,
    seqs: BTreeMap<String, Vec<Op>>,
}

impl FakeTable {
    pub fn parse(text: &str) -> Self {
        let mut t = Self {
            clear_on_read: true,
            ..Self::default()
        };
        let mut current: Option<String> = None;
        for (n, line) in text.lines().enumerate() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let line_no = n + 1;
            match w.as_slice() {
                [] | ["ext-macsec", "1"] => {}
                [first, ..] if first.starts_with('#') => {}
                ["counters", mode] => t.clear_on_read = *mode == "clear-on-read",
                ["ifg-bits", bits] => t.ifg_bits = bits.parse().unwrap(),
                ["port", lane, phy] => t.ports.push((lane.parse().unwrap(), hex(phy))),
                ["seq", name] => {
                    t.seqs.insert((*name).to_owned(), Vec::new());
                    current = Some((*name).to_owned());
                }
                ["D", _usec] => t.push(
                    current.as_ref(),
                    Op {
                        line: line_no,
                        phy: None,
                        dev: 0,
                        reg: 0,
                        kind: Kind::Delay,
                    },
                ),
                [op, phy, dev, reg, rest @ ..] => {
                    let kind = match (*op, rest) {
                        ("W", [v]) => Kind::Write(value(v)),
                        ("R", []) => Kind::Read,
                        ("E", [want]) => Kind::Expect {
                            want: hex(want) as u16,
                            mask: 0xffff,
                        },
                        ("E", [want, mask]) => Kind::Expect {
                            want: hex(want) as u16,
                            mask: hex(mask) as u16,
                        },
                        ("P", [want, mask, tries, _interval]) => Kind::Poll {
                            want: hex(want) as u16,
                            mask: hex(mask) as u16,
                            tries: tries.parse().unwrap(),
                        },
                        ("X", [dest]) => Kind::Extract(match dest.strip_prefix('$') {
                            Some(n) => Dest::Kept(n.parse().unwrap()),
                            None => indexed(dest).map(|(name, i)| Dest::Result(name, i)).unwrap(),
                        }),
                        _ => panic!("line {line_no}: {line}"),
                    };
                    let phy = if *phy == "p" { None } else { Some(hex(phy)) };
                    t.push(
                        current.as_ref(),
                        Op {
                            line: line_no,
                            phy,
                            dev: hex(dev),
                            reg: hex(reg),
                            kind,
                        },
                    );
                }
                _ => panic!("line {line_no}: {line}"),
            }
        }
        t
    }

    fn push(&mut self, current: Option<&String>, op: Op) {
        self.seqs
            .get_mut(current.expect("an operation outside a sequence"))
            .unwrap()
            .push(op);
    }

    /// What the model is told about the table.
    pub fn model_table(&self) -> Table {
        Table {
            counters: if self.clear_on_read {
                CounterMode::ClearOnRead
            } else {
                CounterMode::Absolute
            },
            ifg_bits: self.ifg_bits,
            engines: self
                .ports
                .iter()
                .map(|(lane, phy)| EngineAddr { lane: *lane, phy: *phy })
                .collect(),
        }
    }
}

/// One access of the fake bus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusOp {
    pub write: bool,
    pub phy: u32,
    pub dev: u32,
    pub reg: u32,
    /// The value written, or the value read.
    pub val: u16,
}

/// One sequence run as the model asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub port: PortKey,
    pub name: String,
    /// Parameter name to its bytes in hex.
    pub params: BTreeMap<String, String>,
    /// Whether the sequence completed.
    pub ok: bool,
}

impl Run {
    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map_or("", String::as_str)
    }
}

/// The fake PHYs: a register map per (phy, dev, reg) like the C++ fake backend's, the
/// table's sequences run on it, a log of bus accesses and of runs, the ASIC's inter-frame gap.
#[derive(Debug, Default)]
pub struct FakePhy {
    pub table: FakeTable,
    /// chip port to PHY address
    phy_of: BTreeMap<PortKey, u32>,
    regs: BTreeMap<(u32, u32, u32), u16>,
    pub bus: Vec<BusOp>,
    pub runs: Vec<Run>,
    /// One entry is consumed per run: `true` fails that run before it touches the bus.
    pub fail_plan: VecDeque<bool>,
    /// The inter-frame gap the ASIC MAC has, per port (0: never set).
    pub ifg: BTreeMap<PortKey, u32>,
    /// The board's own gap quirk (SAI_OPENBCM_PORT_IFG), 0 = none.
    pub quirk_ifg: u32,
    macsec_ifg: BTreeMap<PortKey, bool>,
    /// How often the gap was programmed.
    pub ifg_calls: u32,
    /// `true`: programming the gap fails.
    pub ifg_fails: bool,
}

impl FakePhy {
    pub fn new(table: FakeTable, ports: &[SwitchPort]) -> Self {
        let phy_of = table
            .ports
            .iter()
            .filter_map(|(lane, phy)| {
                let port = ports.iter().find(|p| p.lanes.first() == Some(lane))?;
                Some((port.key, *phy))
            })
            .collect();
        Self {
            table,
            phy_of,
            ..Self::default()
        }
    }

    /// Sets a register of device 1 (the C++ test's `xp_reg`).
    pub fn reg(&mut self, phy: u32, reg: u32, val: u16) {
        self.regs.insert((phy, 1, reg), val);
    }

    pub fn get_reg(&self, phy: u32, reg: u32) -> u16 {
        self.regs.get(&(phy, 1, reg)).copied().unwrap_or(0)
    }

    fn read(&mut self, phy: u32, dev: u32, reg: u32) -> u16 {
        let val = self.regs.get(&(phy, dev, reg)).copied().unwrap_or(0);
        self.bus.push(BusOp {
            write: false,
            phy,
            dev,
            reg,
            val,
        });
        val
    }

    /// The bus accesses since the last call, as the C++ test prints them
    /// (`ms_log_take`: "W 10 1.200 100", reads with value 0).
    pub fn log_take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.bus)
            .iter()
            .map(|o| {
                format!(
                    "{} {:x} {:x}.{:03x} {:x}",
                    if o.write { 'W' } else { 'R' },
                    o.phy,
                    o.dev,
                    o.reg,
                    if o.write { o.val } else { 0 }
                )
            })
            .collect()
    }

    /// "reg=val" of the writes since the last call, joined by spaces (`ms_join(ms_writes())`).
    pub fn writes(&mut self) -> String {
        let writes: Vec<String> = self
            .log_take()
            .iter()
            .filter(|s| s.starts_with('W'))
            .map(|s| {
                let mut text = s[s.find('.').unwrap() + 1..].to_owned();
                text.replace_range(3..4, "=");
                text
            })
            .collect();
        writes.join(" ")
    }

    /// The runs since the last call.
    pub fn runs_take(&mut self) -> Vec<Run> {
        std::mem::take(&mut self.runs)
    }

    /// The names of the runs since the last call.
    pub fn names_take(&mut self) -> Vec<String> {
        self.runs_take().into_iter().map(|r| r.name).collect()
    }

    fn byte_of(b: &Byte, params: &Params) -> Option<u8> {
        match b {
            Byte::Const(c) => Some(*c),
            Byte::Param(name, i) => params.get(name)?.get(*i).copied(),
        }
    }

    fn fail(status: Error, why: String) -> SeqError {
        SeqError { status, why }
    }
}

impl SeqRunner for FakePhy {
    fn sequence_names(&self) -> Vec<String> {
        self.table.seqs.keys().cloned().collect()
    }

    fn result_names(&self, name: &str) -> Vec<String> {
        self.table.seqs.get(name).map_or_else(Vec::new, |ops| {
            ops.iter()
                .filter_map(|o| match &o.kind {
                    Kind::Extract(Dest::Result(name, _)) => Some(name.clone()),
                    _ => None,
                })
                .collect()
        })
    }

    fn run(&mut self, port: PortKey, name: &str, params: &Params) -> core::result::Result<Results, SeqError> {
        self.runs.push(Run {
            port,
            name: name.to_owned(),
            params: params
                .names()
                .map(|n| {
                    (
                        n.to_owned(),
                        params.get(n).unwrap().iter().map(|b| format!("{b:02x}")).collect(),
                    )
                })
                .collect(),
            ok: false,
        });
        let outcome = self.execute(port, name, params);
        self.runs.last_mut().unwrap().ok = outcome.is_ok();
        outcome
    }

    /// `port_ifg_apply` of the C++ port code: the MACsec gap, else the board quirk, else
    /// back to the Ethernet 96 bit times if this port had the MACsec gap; else nothing.
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> core::result::Result<(), SeqError> {
        let had = self.macsec_ifg.insert(port, macsec_bits != 0).unwrap_or(false);
        let want = if macsec_bits != 0 {
            macsec_bits
        } else if self.quirk_ifg != 0 {
            self.quirk_ifg
        } else if had {
            96
        } else {
            0
        };
        if want == 0 {
            return Ok(());
        }
        if self.ifg_fails {
            return Err(Self::fail(Error::Failure, "the gap cannot be programmed".to_owned()));
        }
        self.ifg.insert(port, want);
        self.ifg_calls += 1;
        Ok(())
    }
}

impl FakePhy {
    fn execute(&mut self, port: PortKey, name: &str, params: &Params) -> core::result::Result<Results, SeqError> {
        if self.fail_plan.pop_front() == Some(true) {
            return Err(Self::fail(Error::Failure, "injected failure".to_owned()));
        }
        let ops = self
            .table
            .seqs
            .get(name)
            .cloned()
            .expect("the model runs only sequences the table has");
        let port_phy = *self.phy_of.get(&port).expect("a port of the table");
        // every parameter first: never start what cannot finish (ext_macsec_exec)
        for o in &ops {
            if let Kind::Write(Val::Bytes(hi, lo)) = &o.kind {
                for b in [hi, lo] {
                    if let (None, Byte::Param(n, i)) = (Self::byte_of(b, params), b) {
                        return Err(Self::fail(
                            Error::InvalidParameter,
                            format!("line {}: parameter {n}[{i}] is not available", o.line),
                        ));
                    }
                }
            }
        }
        let mut kept = [0u16; 16];
        let mut results = Results::new();
        for o in &ops {
            let phy = o.phy.unwrap_or(port_phy);
            match &o.kind {
                Kind::Delay => {}
                Kind::Write(v) => {
                    let val = match v {
                        Val::Const(c) => *c,
                        Val::Kept { n, mask, bits } => (kept[*n] & mask) | bits,
                        Val::Bytes(hi, lo) => {
                            u16::from_be_bytes([Self::byte_of(hi, params).unwrap(), Self::byte_of(lo, params).unwrap()])
                        }
                    };
                    self.regs.insert((phy, o.dev, o.reg), val);
                    self.bus.push(BusOp {
                        write: true,
                        phy,
                        dev: o.dev,
                        reg: o.reg,
                        val,
                    });
                }
                Kind::Read => {
                    self.read(phy, o.dev, o.reg);
                }
                Kind::Extract(dest) => {
                    let v = self.read(phy, o.dev, o.reg);
                    match dest {
                        Dest::Kept(n) => kept[*n] = v,
                        Dest::Result(name, i) => results.set_word(name, *i, v),
                    }
                }
                Kind::Expect { want, mask } | Kind::Poll { want, mask, .. } => {
                    let (tries, op) = if let Kind::Poll { tries, .. } = &o.kind {
                        (*tries, 'P')
                    } else {
                        (1, 'E')
                    };
                    let mut v = 0;
                    let mut hit = false;
                    for _ in 0..tries {
                        v = self.read(phy, o.dev, o.reg);
                        hit = v & mask == *want;
                        if hit {
                            break;
                        }
                    }
                    if !hit {
                        return Err(Self::fail(
                            Error::Failure,
                            format!(
                                "line {}: {op} phy 0x{phy:x} {}.{:04x} read 0x{v:04x}, expected 0x{want:04x} (mask 0x{mask:04x}, {tries} reads)",
                                o.line, o.dev, o.reg
                            ),
                        ));
                    }
                }
            }
        }
        Ok(results)
    }
}

/// The switch of the C++ test's fake backend: `n` ports of four lanes, chip port 1, 5, 9, ...
/// (first lane the same number), named ce0, ce1, ...
pub fn switch_ports(n: u32) -> Vec<SwitchPort> {
    (0..n)
        .map(|i| SwitchPort {
            id: PortId::from_index(u64::from(1 + i * 4)),
            key: PortKey(1 + i * 4),
            name: format!("ce{i}"),
            lanes: (0..4).map(|l| i * 4 + l + 1).collect(),
        })
        .collect()
}

/// A model on the fake PHYs, with its log captured.
pub struct World {
    pub m: Macsec,
    pub phy: FakePhy,
    pub ports: Vec<SwitchPort>,
    pub log: Arc<Mutex<Vec<(Level, String)>>>,
}

impl World {
    /// A switch of four ports with this table (the C++ test's profile).
    pub fn new(table_text: &str) -> Result<Self> {
        Self::with(table_text, Options::default())
    }

    pub fn with(table_text: &str, mut options: Options) -> Result<Self> {
        let ports = switch_ports(4);
        let table = FakeTable::parse(table_text);
        let model_table = table.model_table();
        let mut phy = FakePhy::new(table, &ports);
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&log);
        options.log = Some(Box::new(move |level, text| {
            sink.lock().unwrap().push((level, text.to_owned()));
        }));
        let m = Macsec::attach(&model_table, &ports, options, &mut phy)?;
        Ok(Self { m, phy, ports, log })
    }

    /// Everything logged so far, one line per entry.
    pub fn log_text(&self) -> String {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(level, text)| format!("{level:?} {text}\n"))
            .collect()
    }

    pub fn port(&self, i: usize) -> PortId {
        self.ports[i].id
    }

    pub fn key(&self, i: usize) -> PortKey {
        self.ports[i].key
    }
}

pub const EGR: i32 = sys::SAI_MACSEC_DIRECTION_EGRESS as i32;
pub const ING: i32 = sys::SAI_MACSEC_DIRECTION_INGRESS as i32;

pub fn en(id: sys::sai_attr_id_t, v: i32) -> Attr {
    Attr::new(id, Value::Enum(v))
}
pub fn oid(id: sys::sai_attr_id_t, v: RawOid) -> Attr {
    Attr::new(id, Value::Oid(v))
}
pub fn boolean(id: sys::sai_attr_id_t, v: bool) -> Attr {
    Attr::new(id, Value::Bool(v))
}
pub fn u64v(id: sys::sai_attr_id_t, v: u64) -> Attr {
    Attr::new(id, Value::U64(v))
}
pub fn u32v(id: sys::sai_attr_id_t, v: u32) -> Attr {
    Attr::new(id, Value::U32(v))
}
pub fn u8v(id: sys::sai_attr_id_t, v: u8) -> Attr {
    Attr::new(id, Value::U8(v))
}
pub fn action_s32(id: sys::sai_attr_id_t, enable: bool, v: i32) -> Attr {
    Attr::new(
        id,
        Value::AclAction {
            enable,
            parameter: AclParameter::S32(v),
        },
    )
}
pub fn action_oid(id: sys::sai_attr_id_t, enable: bool, v: RawOid) -> Attr {
    Attr::new(
        id,
        Value::AclAction {
            enable,
            parameter: AclParameter::Oid(v),
        },
    )
}
pub fn ether_type(v: u16) -> Attr {
    Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE,
        Value::AclField(Some(AclField::U16 { data: v, mask: 0xffff })),
    )
}

/// The SAK of the C++ test: bytes 0xc0.. (+ AN) so they are easy to find where they must
/// not be; a 128-bit key sits in the last 16 of the 32 bytes.
pub fn test_sak(an: u8, k256: bool) -> [u8; 32] {
    core::array::from_fn(|i| {
        if k256 || i >= 16 {
            0xc0 + u8::try_from(if k256 { i } else { i - 16 }).unwrap() + an
        } else {
            0
        }
    })
}

/// The attributes of the C++ test's `ms_sa_attrs`.
pub fn sa_attrs(dir: i32, sc: RawOid, an: u8, k256: bool, xpn: bool, pn: u64) -> Vec<Attr> {
    let pn_attr = if dir == EGR {
        sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN
    } else {
        sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
    };
    let auth: [u8; 16] = core::array::from_fn(|i| 0x80 + u8::try_from(i).unwrap());
    let mut v = vec![
        en(sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, dir),
        oid(sys::SAI_MACSEC_SA_ATTR_SC_ID, sc),
        u8v(sys::SAI_MACSEC_SA_ATTR_AN, an),
        u64v(pn_attr, pn),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SAK,
            Value::Sak(Secret::from_slice(&test_sak(an, k256)).unwrap()),
        ),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::AuthKey(Secret::from_slice(&auth).unwrap()),
        ),
    ];
    if xpn {
        let salt: [u8; 12] = core::array::from_fn(|i| 0x50 + u8::try_from(i).unwrap());
        v.push(Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SALT,
            Value::Salt(Secret::from_slice(&salt).unwrap()),
        ));
        v.push(u32v(sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI, 0x0102_0304));
    }
    v
}

/// `SAI_STATUS_ATTR_NOT_SUPPORTED_0 - index` and friends, for expected results.
pub fn not_supported(index: usize) -> Error {
    Error::attr(sai_core::error::AttrError::NotSupported, index)
}
pub fn invalid_value(index: usize) -> Error {
    Error::attr(sai_core::error::AttrError::InvalidValue, index)
}

pub fn raw<T: Into<RawOid>>(id: T) -> RawOid {
    id.into()
}

pub fn port_raw(id: PortId) -> RawOid {
    id.raw()
}

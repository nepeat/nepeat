//! Shared by the test files: the made-up table of sai-openbcm's `test_macsec.inc`, a recording
//! bus, and generators of table texts (well formed ones and ones that are almost).

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test helpers; not every test file uses every helper"
)]

use std::collections::VecDeque;
use std::fmt::Write as _;

use phy_seq::{Bus, Dev, Op};
use proptest::prelude::*;

/// `ms_table()` of sai-openbcm's `tests/test_macsec.inc`: a made-up PHY with one register per
/// thing. Synthetic; no real chip's registers.
pub(crate) fn ms_table(gates: bool, probe: bool) -> String {
    let mut t =
        String::from("ext-macsec 1\n# test table\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n");
    let ciphers = ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"];
    for (d, dn) in ["egress", "ingress"].into_iter().enumerate() {
        if probe {
            writeln!(t, "seq probe.{dn}\nX p 1 10{d} bypass[0]").unwrap();
        }
        writeln!(t, "seq reset.{dn}\nW p 1 11{d} 1").unwrap();
        writeln!(t, "seq enable.{dn}\nW p 1 10{d} 0\nD 10").unwrap();
        writeln!(t, "seq disable.{dn}\nW p 1 10{d} 1").unwrap();
        if gates {
            writeln!(t, "seq gate-open.{dn}\nW p 1 12{d} 1").unwrap();
            writeln!(t, "seq gate-close.{dn}\nW p 1 12{d} 0").unwrap();
        }
        writeln!(
            t,
            "seq stats-flow.{dn}\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 x_unknown[0]"
        )
        .unwrap();
        for an in 0..4 {
            for (c, cipher) in ciphers.into_iter().enumerate() {
                let (xpn, k256) = (c >= 2, c & 1 == 1);
                for first in 0..2 {
                    let marker = (first << 8) | (c << 4) | an;
                    writeln!(
                        t,
                        "seq sa-add{}.{dn}.{an}.{cipher}\nW p 1 2{d}0 {marker:x}",
                        if first == 1 { "-first" } else { "" }
                    )
                    .unwrap();
                    let (hi, lo) = if k256 { (31, 30) } else { (15, 14) };
                    writeln!(
                        t,
                        "W p 1 2{d}1 key[1].key[0]\nW p 1 2{d}2 key[{hi}].key[{lo}]\nW p 1 2{d}3 hkey[15].hkey[0]"
                    )
                    .unwrap();
                    let pn_hi = if xpn { "pn[5].pn[4]" } else { "0" };
                    writeln!(t, "W p 1 2{d}4 sci[0].sci[7]\nW p 1 2{d}5 pn[1].pn[0]\nW p 1 2{d}6 {pn_hi}\nW p 1 2{d}7 window[1].window[0]").unwrap();
                    if xpn {
                        writeln!(
                            t,
                            "W p 1 2{d}8 iv[0].iv[3]\nW p 1 2{d}9 iv[4].iv[11]\nW p 1 2{d}a ssci[0].salt[0]"
                        )
                        .unwrap();
                    }
                    if d == 1 {
                        writeln!(t, "X p 1 310 $0\nW p 1 310 $0&ff00|{}", an + 1).unwrap();
                    }
                    writeln!(t, "P p 1 2{d}f 1 1 3 0").unwrap();
                }
            }
            if d == 1 && an == 0 {
                t.push_str("seq sa-add-first.ingress.0.gcm-aes-128.replay\nW p 1 210 f00\nW p 1 217 window[1].window[0]\nP p 1 21f 1 1 3 0\n");
            }
            writeln!(
                t,
                "seq sa-del.{dn}.{an}\nW p 1 3{d}{an} 1\nseq sa-del-last.{dn}.{an}\nW p 1 3{d}{an} 8001"
            )
            .unwrap();
            writeln!(
                t,
                "seq pn.{dn}.{an}\nX p 1 5{d}{:x} pn[0]\nX p 1 5{d}{:x} pn[1]\nX p 1 5{d}{:x} pn[2]\nX p 1 5{d}{:x} pn[3]",
                an * 4,
                an * 4 + 1,
                an * 4 + 2,
                an * 4 + 3
            )
            .unwrap();
            let stat = if d == 1 { "in_pkts_ok" } else { "out_pkts_encrypted" };
            writeln!(
                t,
                "seq stats-sa.{dn}.{an}\nX p 1 4{d}{:x} {stat}[0]\nX p 1 4{d}{:x} {stat}[1]",
                an * 2,
                an * 2 + 1
            )
            .unwrap();
        }
    }
    t
}

/// A bus that logs every access and answers reads from a queue per register (0 when empty).
/// `fail_at`: the access (0-based, reads and writes counted together) that fails with status -7.
#[derive(Debug, Default)]
pub(crate) struct LogBus {
    pub(crate) log: Vec<String>,
    pub(crate) answers: std::collections::BTreeMap<u16, VecDeque<u16>>,
    pub(crate) fail_at: Option<usize>,
    pub(crate) accesses: usize,
    pub(crate) begun: Vec<usize>,
}

impl LogBus {
    pub(crate) fn with(answers: &[(u16, &[u16])]) -> Self {
        Self {
            answers: answers.iter().map(|&(r, v)| (r, v.iter().copied().collect())).collect(),
            ..Self::default()
        }
    }

    fn access(&mut self) -> Result<(), i32> {
        self.accesses += 1;
        if self.fail_at == Some(self.accesses - 1) {
            Err(-7)
        } else {
            Ok(())
        }
    }

    pub(crate) fn joined(&self) -> String {
        self.log.join("|")
    }
}

impl Bus for LogBus {
    type Error = i32;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, i32> {
        self.access()?;
        self.log.push(format!("R {phy:x} {:x}.{reg:04x}", dev.get()));
        Ok(self.answers.get_mut(&reg).and_then(VecDeque::pop_front).unwrap_or(0))
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), i32> {
        self.access()?;
        self.log
            .push(format!("W {phy:x} {:x}.{reg:04x} {value:04x}", dev.get()));
        Ok(())
    }
    fn delay_us(&mut self, us: u32) {
        self.log.push(format!("D {us}"));
    }
    fn begin(&mut self, index: usize, _op: &Op) {
        self.begun.push(index);
    }
}

// ---- generated table texts ----
//
// A text is a well-formed table (so that parsing gets past the first lines and sequences have
// operations to run) with zero to three small mutations: one token replaced by a nasty one,
// dropped or inserted, a line dropped, repeated, swapped or replaced. Most single mutations make
// exactly one thing wrong, which is what the parsers' "first error" order is about.

const NASTY: &[&str] = &[
    "",
    "0",
    "1",
    "ffff",
    "FFFF",
    "10000",
    "0x10",
    "0X1f",
    "0x",
    "-1",
    "+1",
    "zz",
    "0007",
    "1g",
    "a000\0zz",
    "\0",
    "20",
    "1f",
    "p",
    "q",
    "pp",
    "$0",
    "$15",
    "$16",
    "$0|1&2",
    "$3&ff00|1",
    "$0&",
    "$0|",
    "$",
    "$\0",
    "$x",
    "$1.2",
    "key[0].key[1]",
    "key[32].00",
    "key[31].00",
    "hkey[16].00",
    "0.key[0]",
    "secret[0].00",
    "Key[0].00",
    "key[00].iv[11]",
    "key[0]].00",
    "pn[8]",
    "pn[7]",
    "pn",
    "pn[0]",
    "pn[]",
    "[0]",
    "p.n[0]",
    "1001",
    "1000",
    "100001",
    "100000",
    "2041",
    "60001",
    "99999999999999999999",
    "seq",
    "W",
    "X",
    "C",
    "D",
    "E",
    "R",
    "P",
    "N",
    "L",
    "#",
    "#x",
    "link",
    "port",
    "admin-up",
    "admin-down",
    "counters",
    "absolute",
    "clear-on-read",
    "ifg-bits",
    "poll-ms",
    "up-polls",
    "holdoff-ms",
    "1.2",
    "12.34",
    "ab.cd.ef",
    "12.",
    ".34",
    ".",
    "ext-macsec",
    "ext-phy",
    "a",
    "A",
    "a_b",
    "\u{e9}",
    "\x0b",
    "x\x0cy",
];

const BAD_LINES: &[&str] = &[
    "",
    "   ",
    "# comment",
    "\t# x",
    "ext-macsec 1",
    "ext-phy 1",
    "ext-macsec 2",
    "mdio-script 1",
    "bogus 1 2",
    "seq a",
    "seq b",
    "seq A",
    "seq",
    "port 1 10",
    "port 1",
    "port",
    "link",
    "admin-up",
    "admin-down",
    "link x",
    "counters sometimes",
    "D",
    "D 5",
    "W",
    "W p 1 a000 $0",
    "W p 1 a000 $9|1",
    "X p 1 a000 $9",
    "C p 1 a000 1 1",
    "C 10 1 a101 3 3",
    "R 10 1 8221",
    "poll-ms 100",
    "up-polls 0",
    "ifg-bits 9999",
    "E p 1 a000 3 1",
    "P p 1 a000 1 1 0 0",
];

type Mutation = (u8, usize, usize, &'static str);

fn mutations() -> impl Strategy<Value = Vec<Mutation>> {
    let one = || (0u8..8, any::<usize>(), any::<usize>(), proptest::sample::select(NASTY));
    prop_oneof![
        4 => Just(Vec::new()),
        4 => proptest::collection::vec(one(), 1..=1),
        2 => proptest::collection::vec(one(), 2..=3),
    ]
}

fn mutate(mut lines: Vec<String>, mutations: &[Mutation]) -> Vec<String> {
    for &(kind, a, b, token) in mutations {
        if lines.is_empty() {
            break;
        }
        let at = a % lines.len();
        let mut words: Vec<String> = lines[at].split(' ').map(str::to_owned).collect();
        let word = b % words.len().max(1);
        match kind {
            0 | 1 => {
                if let Some(w) = words.get_mut(word) {
                    token.clone_into(w);
                }
            }
            2 => {
                if words.len() > 1 {
                    words.remove(word);
                }
            }
            3 => words.insert(word, token.to_owned()),
            4 => {
                lines.remove(at);
                continue;
            }
            5 => {
                let copy = lines[at].clone();
                lines.insert(b % (lines.len() + 1), copy);
                continue;
            }
            6 => {
                let other = b % lines.len();
                lines.swap(at, other);
                continue;
            }
            _ => {
                BAD_LINES[b % BAD_LINES.len()].clone_into(&mut lines[at]);
                continue;
            }
        }
        lines[at] = words.join(" ");
    }
    lines
}

/// A number as a table may write it: plain, with leading zeros or with an `0x` prefix.
fn hex(max: u32) -> impl Strategy<Value = String> {
    (0..=max, 0u8..10).prop_map(|(v, style)| match style {
        0 => format!("0x{v:x}"),
        1 => format!("{v:05X}"),
        _ => format!("{v:x}"),
    })
}

fn clean_byte() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => (0u32..=0xff).prop_map(|v| format!("{v:02x}")),
        3 => (0usize..8, any::<u8>()).prop_map(|(n, i)| {
            let name = phy_seq::ParamName::ALL[n];
            format!("{name}[{}]", i % name.max_len())
        }),
    ]
}

fn clean_value() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => hex(0xffff),
        4 => (clean_byte(), clean_byte()).prop_map(|(a, b)| format!("{a}.{b}")),
        2 => (proptest::sample::select(&["$0", "$7"][..]), proptest::option::of(hex(0xffff)), proptest::option::of(hex(0xffff))).prop_map(|(n, m, b)| {
            let mut s = n.to_owned();
            if let Some(m) = m {
                write!(s, "&{m}").unwrap();
            }
            if let Some(b) = b {
                write!(s, "|{b}").unwrap();
            }
            s
        }),
    ]
}

fn clean_compare() -> impl Strategy<Value = (String, String)> {
    (any::<u16>(), any::<u16>()).prop_map(|(v, m)| (format!("{:x}", v & m), format!("{m:x}")))
}

fn clean_reg(port_phy: bool) -> impl Strategy<Value = String> {
    let phy = if port_phy {
        prop_oneof![3 => Just("p".to_owned()), 1 => hex(0xffff)].boxed()
    } else {
        hex(0xffff).boxed()
    };
    (phy, 1u32..=31, hex(0xffff)).prop_map(|(p, d, r)| format!("{p} {d:x} {r}"))
}

fn clean_macsec_op() -> impl Strategy<Value = String> {
    let reg = || clean_reg(true);
    prop_oneof![
        6 => (reg(), clean_value()).prop_map(|(r, v)| format!("W {r} {v}")),
        2 => reg().prop_map(|r| format!("R {r}")),
        2 => (reg(), clean_compare(), any::<bool>()).prop_map(|(r, (v, m), with_mask)| if with_mask { format!("E {r} {v} {m}") } else { format!("E {r} {v}") }),
        2 => (reg(), clean_compare(), prop_oneof![5 => 1u32..5, 1 => Just(1000u32)], prop_oneof![5 => 0u32..300, 1 => Just(100_000u32)])
            .prop_map(|(r, (v, m), t, i)| format!("P {r} {v} {m} {t} {i}")),
        3 => (reg(), prop_oneof![(0u32..16).prop_map(|n| format!("${n}")), (proptest::sample::select(&["pn", "bypass", "in_pkts_ok", "x_9"][..]), 0u32..8).prop_map(|(n, w)| format!("{n}[{w}]"))])
            .prop_map(|(r, d)| format!("X {r} {d}")),
        1 => prop_oneof![5 => 0u32..500, 1 => Just(100_000u32)].prop_map(|u| format!("D {u}")),
    ]
}

const SEQ_NAMES: [&str; 6] = [
    "a",
    "enable.egress",
    "sa-add.ingress.0.gcm-aes-128",
    "x-1",
    "probe.egress",
    "b",
];

fn clean_macsec_lines() -> impl Strategy<Value = Vec<String>> {
    (
        proptest::option::of(proptest::sample::select(
            &["counters clear-on-read", "counters absolute"][..],
        )),
        proptest::option::of(0u32..=2040),
        proptest::collection::btree_map(0u32..40, 0u32..=0xffff, 0..4),
        proptest::collection::vec(proptest::collection::vec(clean_macsec_op(), 0..12), 0..6),
        any::<bool>(),
    )
        .prop_map(|(counters, ifg, ports, seqs, comment)| {
            let mut lines = vec!["ext-macsec 1".to_owned()];
            if comment {
                lines.push("# generated \u{e9}\u{fffd}".to_owned());
            }
            lines.extend(counters.map(str::to_owned));
            lines.extend(ifg.map(|v| format!("ifg-bits {v}")));
            lines.extend(ports.iter().map(|(lane, phy)| format!("port {lane} {phy:x}")));
            for (name, ops) in SEQ_NAMES.iter().zip(seqs) {
                lines.push(format!("seq {name}"));
                // the kept values the generated writes use
                lines.push("X p 1 100 $0".to_owned());
                lines.push("X 1e 1f 101 $7".to_owned());
                lines.extend(ops);
            }
            lines
        })
}

fn join(lines: &[String], eol: &str, last_eol: bool) -> Vec<u8> {
    let mut text = lines.join(eol);
    if last_eol {
        text.push('\n');
    }
    text.into_bytes()
}

/// A whole ext-macsec table text: well formed, or nearly.
pub(crate) fn macsec_text() -> impl Strategy<Value = Vec<u8>> {
    (
        clean_macsec_lines(),
        mutations(),
        prop_oneof![8 => Just("\n"), 1 => Just("\r\n"), 1 => Just("\n\n")],
        any::<bool>(),
    )
        .prop_map(|(lines, mutations, eol, last_eol)| join(&mutate(lines, &mutations), eol, last_eol))
}

fn clean_extphy_op(link: bool) -> BoxedStrategy<String> {
    let reg = || clean_reg(false);
    let cond = (reg(), clean_compare()).prop_map(|(r, (v, m))| format!("C {r} {v} {m}"));
    let other = prop_oneof![
        4 => (reg(), hex(0xffff)).prop_map(|(r, v)| format!("W {r} {v}")),
        2 => reg().prop_map(|r| format!("R {r}")),
        2 => (reg(), clean_compare(), any::<bool>()).prop_map(|(r, (v, m), with_mask)| if with_mask { format!("E {r} {v} {m}") } else { format!("E {r} {v}") }),
        2 => (reg(), clean_compare(), 1u32..5, 0u32..300).prop_map(|(r, (v, m), t, i)| format!("P {r} {v} {m} {t} {i}")),
    ];
    if link {
        prop_oneof![2 => cond, 3 => other].boxed()
    } else {
        other.boxed()
    }
}

fn clean_extphy_lines() -> impl Strategy<Value = Vec<String>> {
    let section = |link: bool| proptest::collection::vec(clean_extphy_op(link), 0..8);
    let port = (
        section(true),
        section(false),
        section(false),
        clean_reg(false),
        clean_compare(),
    );
    (
        proptest::option::of(0u32..=60_000),
        proptest::option::of(1u32..=100),
        proptest::option::of(0u32..=600_000),
        proptest::collection::btree_map(0u32..30, port, 0..4),
    )
        .prop_map(|(poll, up, holdoff, ports)| {
            let mut lines = vec!["ext-phy 1".to_owned()];
            lines.extend(poll.map(|v| format!("poll-ms {v}")));
            lines.extend(up.map(|v| format!("up-polls {v}")));
            lines.extend(holdoff.map(|v| format!("holdoff-ms {v}")));
            for (lane, (mut link, down, up, reg, (want, mask))) in ports {
                lines.push(format!("port {lane}"));
                // a port needs a sequence, and a link sequence a condition
                if down.is_empty() && up.is_empty() || !link.is_empty() {
                    link.push(format!("C {reg} {want} {mask}"));
                }
                for (keyword, ops) in [("link", link), ("admin-down", down), ("admin-up", up)] {
                    if !ops.is_empty() {
                        lines.push(keyword.to_owned());
                        lines.extend(ops);
                    }
                }
            }
            lines
        })
}

/// A whole ext-phy table text: well formed, or nearly.
pub(crate) fn extphy_text() -> impl Strategy<Value = Vec<u8>> {
    (clean_extphy_lines(), mutations(), any::<bool>())
        .prop_map(|(lines, mutations, last_eol)| join(&mutate(lines, &mutations), "\n", last_eol))
}

//! The ext-macsec cases of sai-openbcm's `tests/test_macsec.inc` (parser, executor against a
//! golden rendering), ported, plus cases of this crate's own.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use common::{LogBus, ms_table};
use phy_seq::{
    CounterMode, ExecErrorKind, MacsecTable, OpKind, OpLetter, ParamName, Params, ParseErrorKind, PhySel, Value, run,
};

fn err(text: &str) -> String {
    MacsecTable::parse(text.as_bytes()).unwrap_err().to_string()
}

#[test]
fn the_test_table_parses() {
    let t = MacsecTable::parse(ms_table(true, true).as_bytes()).unwrap();
    assert_eq!(t.ports().collect::<Vec<_>>(), [(1, 0x10), (5, 0x11)]);
    assert_eq!(t.phy_of_lane(5), Some(0x11));
    assert_eq!(t.phy_of_lane(2), None);
    assert_eq!(t.ifg_bits(), 384);
    assert_eq!(t.counters(), CounterMode::ClearOnRead);
    assert!(t.sequence("sa-add-first.ingress.3.gcm-aes-xpn-256").is_some());
    assert!(t.sequence("sa-add.ingress.3.gcm-aes-xpn-512").is_none());
    // without the optional sequences
    let t = MacsecTable::parse(ms_table(false, false).as_bytes()).unwrap();
    assert!(t.sequence("gate-open.egress").is_none() && t.sequence("probe.ingress").is_none());
    assert!(t.sequence("enable.egress").is_some());
}

#[test]
fn defaults_and_the_empty_table() {
    let t = MacsecTable::parse(b"ext-macsec 1\n").unwrap();
    assert_eq!((t.ports().len(), t.sequences().len(), t.ifg_bits()), (0, 0, 0));
    assert_eq!(t.counters(), CounterMode::ClearOnRead);
    let t = MacsecTable::parse(b"# first\n\n  ext-macsec   1  \r\ncounters absolute\nseq a\nseq b").unwrap();
    assert_eq!(t.counters(), CounterMode::Absolute);
    assert_eq!(
        t.sequences().map(|(n, s)| (n, s.ops().len())).collect::<Vec<_>>(),
        [("a", 0), ("b", 0)]
    );
}

/// The `bad[]` list of `test_macsec.inc`: the text and what the message must contain.
#[test]
fn bad_tables_of_the_cxx_test() {
    let bad = [
        ("", "line 1"),
        ("ext-phy 1\n", "line 1"),
        ("ext-macsec 2\n", "line 1"),
        ("ext-macsec 1\nW p 1 a000 6\n", "outside a sequence"),
        ("ext-macsec 1\nseq a\nW p 0 a000 6\n", "clause 22"),
        ("ext-macsec 1\nseq a\nW p 20 a000 6\n", "line 3"),
        ("ext-macsec 1\nseq a\nW q 1 a000 6\n", "phy"),
        ("ext-macsec 1\nseq a\nW p 1 a000 10000\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 key[32].00\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 hkey[16].00\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 secret[0].00\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 0.key[0]\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 $16\n", "value"),
        ("ext-macsec 1\nseq a\nW p 1 a000 $0\n", "before an X"),
        ("ext-macsec 1\nseq a\nX p 1 a000 $16\n", "destination"),
        ("ext-macsec 1\nseq a\nX p 1 a000 pn[8]\n", "destination"),
        ("ext-macsec 1\nseq a\nX p 1 a000 pn\n", "destination"),
        ("ext-macsec 1\nseq a\nE p 1 a000 3 1\n", "outside the mask"),
        ("ext-macsec 1\nseq a\nP p 1 a000 1 1 0 0\n", "tries"),
        ("ext-macsec 1\nseq a\nD 200000\n", "D <usec"),
        ("ext-macsec 1\nseq a\nC p 1 a000 1 1\n", "unknown keyword"),
        ("ext-macsec 1\nseq a\nseq a\n", "twice"),
        ("ext-macsec 1\nseq A\n", "seq <name>"),
        ("ext-macsec 1\nport 1 10\nport 1 11\n", "twice"),
        ("ext-macsec 1\nport 1\n", "port <first lane"),
        ("ext-macsec 1\ncounters sometimes\n", "counters"),
        ("ext-macsec 1\nifg-bits 9999\n", "ifg-bits"),
        (
            "ext-macsec 1\ncounters absolute\nseq a\nW 10 1 a000 $0|1\n",
            "before an X",
        ),
    ];
    for (text, want) in bad {
        let got = err(text);
        assert!(got.contains(want), "{text:?} gave {got:?}, wanted {want:?} in it");
    }
}

/// Every message, whole, with its line: what the C++ parser writes.
#[test]
fn messages_are_those_of_the_cxx_parser() {
    let cases = [
        ("", "line 1: empty table (first line must be 'ext-macsec 1')"),
        ("\n\n# x\n", "line 1: empty table (first line must be 'ext-macsec 1')"),
        (
            "\n# c\next-macsec 1 1\n",
            "line 3: not an ext-macsec table (first line must be 'ext-macsec 1')",
        ),
        ("ext-macsec 1\nfoo\n", "line 2: unknown keyword 'foo'"),
        ("ext-macsec 1\nWW p 1 0 0\n", "line 2: unknown keyword 'WW'"),
        ("ext-macsec 1\nD 5\n", "line 2: operation outside a sequence"),
        (
            "ext-macsec 1\nseq a\nW p 1 a000\n",
            "line 3: wrong number of fields for W",
        ),
        (
            "ext-macsec 1\nseq a\nE p 1 a000 1 1 1\n",
            "line 3: wrong number of fields for E",
        ),
        ("ext-macsec 1\nseq a\nR pp 1 a000\n", "line 3: phy: 'p' or hex"),
        (
            "ext-macsec 1\nseq a\nR p 1 10000\n",
            "line 3: dev / reg: hex, dev 1..1f, reg up to ffff",
        ),
        (
            "ext-macsec 1\nseq a\nR p 0 0\n",
            "line 3: dev 0 (clause 22) is not allowed",
        ),
        (
            "ext-macsec 1\nseq a\nW p 1 0 1.2\n",
            "line 3: value: hhhh, $n[&mask][|bits] or <byte>.<byte> with bytes hh or <param>[<index>]",
        ),
        ("ext-macsec 1\nseq a\nE p 1 0 x\n", "line 3: value: hex up to ffff"),
        ("ext-macsec 1\nseq a\nE p 1 0 1 x\n", "line 3: mask: hex up to ffff"),
        (
            "ext-macsec 1\nseq a\nP p 1 0 2 1 1 1\n",
            "line 3: value has bits outside the mask: can never match",
        ),
        (
            "ext-macsec 1\nseq a\nP p 1 0 1 1 1001 1\n",
            "line 3: P: tries 1..1000 and interval 0..100000 usec, decimal",
        ),
        (
            "ext-macsec 1\nseq a\nP p 1 0 1 1 1 100001\n",
            "line 3: P: tries 1..1000 and interval 0..100000 usec, decimal",
        ),
        ("ext-macsec 1\nseq a\nD 1 2\n", "line 3: D <usec, decimal, 0..100000>"),
        (
            "ext-macsec 1\nseq a\nX p 1 0 $\n",
            "line 3: X destination: $<n> (0..15) or <result>[<word 0..7>]",
        ),
        ("ext-macsec 1\ncounters\n", "line 2: counters clear-on-read | absolute"),
        ("ext-macsec 1\nifg-bits 2041\n", "line 2: ifg-bits <decimal, 0..2040>"),
        (
            "ext-macsec 1\nport 100001 1\n",
            "line 2: port <first lane, decimal> <phy, hex>",
        ),
        ("ext-macsec 1\nport 007 1\nport 7 2\n", "line 3: port 7 twice"),
        ("ext-macsec 1\nport 7 1\nport 007 2\n", "line 3: port 007 twice"),
        ("ext-macsec 1\nseq a.b-c\nseq a.b-c\n", "line 3: sequence a.b-c twice"),
        ("ext-macsec 1\nseq\n", "line 2: seq <name> (a-z 0-9 . -)"),
        // the first sequence in name order is reported, with the line of its operation
        (
            "ext-macsec 1\nseq b\nW p 1 0 $1\nseq a\nX p 1 0 $2\nW p 1 0 $3|1\n",
            "line 6: sequence a: $3 is written before an X reads it",
        ),
    ];
    for (text, want) in cases {
        assert_eq!(err(text), want, "{text:?}");
    }
    let long = format!("ext-macsec 1\nseq a\n{}", "D 0\n".repeat(8193));
    assert_eq!(err(&long), "line 8195: sequence longer than 8192 operations");
    assert!(MacsecTable::parse(format!("ext-macsec 1\nseq a\n{}", "D 0\n".repeat(8192)).as_bytes()).is_ok());
    let name81 = "a".repeat(81);
    assert!(err(&format!("ext-macsec 1\nseq {name81}\n")).contains("seq <name>"));
    assert!(MacsecTable::parse(format!("ext-macsec 1\nseq {}\n", &name81[1..]).as_bytes()).is_ok());
}

#[test]
fn the_error_is_typed_and_keeps_table_bytes() {
    let e = MacsecTable::parse(b"ext-macsec 1\n\xff\xfe 1\n").unwrap_err();
    assert_eq!(
        (e.line, &e.kind),
        (2, &ParseErrorKind::UnknownKeyword(vec![0xff, 0xfe]))
    );
    assert_eq!(e.to_bytes(), b"line 2: unknown keyword '\xff\xfe'");
    assert_eq!(e.to_string(), "line 2: unknown keyword '\u{fffd}\u{fffd}'");
    let e = MacsecTable::parse(b"ext-macsec 1\nseq s\nW p 1 0 \xa71\n").unwrap_err();
    assert_eq!((e.line, e.kind), (3, ParseErrorKind::Value));
}

#[test]
fn values_and_destinations() {
    let t = MacsecTable::parse(
        b"ext-macsec 1\nseq s\nX p 1 1 $15\nW 1e0 1f ffff $15\nW p 1 2 $15&ff00\nW p 1 3 $15|8000\nW p 1 4 $15&f|10\n\
          W p 1 5 00.ff\nW p 1 6 window[3].key[31]\nW p 1 7 0x1F\nX 0x10 0X1 0xa101 in_pkts_ok[7]\n",
    )
    .unwrap();
    let ops = t.sequence("s").unwrap().ops();
    let value = |i: usize| match &ops[i].kind {
        OpKind::Write { value, .. } => value.to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        [value(1), value(2), value(3), value(4)],
        ["$15&ffff|0", "$15&ff00|0", "$15&ffff|8000", "$15&f|10"]
    );
    assert_eq!([value(5), value(6), value(7)], ["00.ff", "window[3].key[31]", "1f"]);
    assert_eq!(
        ops.iter().map(ToString::to_string).collect::<Vec<_>>()[8],
        "X 10 1 a101 in_pkts_ok[7]"
    );
    assert_eq!(ops[1].reg().unwrap().phy, PhySel::Fixed(0x1e0));
    assert_eq!(ops[0].reg().unwrap().phy, PhySel::Port);
    assert_eq!(
        ops.iter().map(|o| o.line).collect::<Vec<_>>(),
        (3..=11).collect::<Vec<_>>()
    );
    assert!(matches!(ops[6].kind, OpKind::Write { value: v @ Value::Bytes { .. }, .. } if v.is_secret()));
    assert_eq!(
        t.sequence("s").unwrap().required_params(),
        [(ParamName::Key, 32), (ParamName::Window, 4)]
    );
}

/// "executor against a golden rendering" of `test_macsec.inc`.
#[test]
fn golden_rendering() {
    let t = MacsecTable::parse(
        b"ext-macsec 1\nseq s\nR p 1 900a\nW p 1 a000 key[1].key[0]\nW 22 1 a001 00.sci[7]\nX p 1 9005 $3\nX p 1 9005 $4\n\
          W p 1 9005 $3&7fff\nW p 1 9005 $4|8002\nD 7\nE p 1 a100 2 f\nP p 1 8221 12e3 ffff 3 5\nX p 1 a101 pn[0]\nX p 1 a102 pn[2]\n",
    )
    .unwrap();
    let seq = t.sequence("s").unwrap();
    let answers: [(u16, &[u16]); 5] = [
        (0x9005, &[0x8001, 0x0010]),
        (0xa100, &[0x12]),
        (0x8221, &[0, 0x12e3]),
        (0xa101, &[0xbeef]),
        (0xa102, &[0x1]),
    ];
    let mut bus = LogBus::with(&answers);
    let mut par = Params::new();
    par.set(ParamName::Key, &[0x11, 0x22]).unwrap();
    par.set(ParamName::Sci, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let res = seq.run(&mut bus, 0x30, &par).unwrap();
    assert_eq!(
        bus.joined(),
        "R 30 1.900a|W 30 1.a000 2211|W 22 1.a001 0008|R 30 1.9005|R 30 1.9005|W 30 1.9005 0001|W 30 1.9005 8012|D 7|R 30 1.a100|\
         R 30 1.8221|D 5|R 30 1.8221|R 30 1.a101|R 30 1.a102"
    );
    assert_eq!(res.u64("pn"), 0x1_0000_beef);
    assert_eq!(res.words("pn"), Some(&[0xbeef, 0, 1][..]));
    assert_eq!(res.u64("absent"), 0);
    assert_eq!(bus.begun, (0..12).collect::<Vec<_>>());

    // a missing parameter byte: nothing is accessed
    let mut bus = LogBus::with(&answers);
    par.set(ParamName::Key, &[0x11]).unwrap();
    let e = seq.run(&mut bus, 0x30, &par).unwrap_err();
    assert!(bus.log.is_empty() && bus.begun.is_empty() && e.before_any_access());
    assert_eq!(e.to_string(), "line 4: parameter key[1] is not available");
    assert!(matches!(e.kind, ExecErrorKind::MissingParam(p) if p.name() == ParamName::Key && p.index() == 1));

    // a compare that fails stops the sequence there and names the line, not a value of ours
    par.set(ParamName::Key, &[0x11, 0x22]).unwrap();
    let mut bus = LogBus::with(&[(0xa100, &[0x13])]);
    let e = seq.run(&mut bus, 0x30, &par).unwrap_err();
    assert_eq!(bus.log.last().unwrap(), "R 30 1.a100");
    assert_eq!(
        e.to_string(),
        "line 11: E phy 0x30 1.a100 read 0x0013, expected 0x0002 (mask 0x000f, 1 reads)"
    );
    assert_eq!((e.line, e.op), (11, OpLetter::E));
    par.wipe();
    assert!(par.is_empty());
}

#[test]
fn a_poll_that_never_matches_and_a_bus_error() {
    let t = MacsecTable::parse(
        b"ext-macsec 1\nseq s\nW p 1f 1 2\nP 5 1 8221 12e3 ffff 3 5\nW p 1 3 4\nseq d\nD 0\nP p 1 1 1 1 2 0\n",
    )
    .unwrap();
    let seq = t.sequence("s").unwrap();
    let mut bus = LogBus::with(&[(0x8221, &[1, 2, 3, 0x12e3])]);
    let e = seq.run(&mut bus, 0x30, &Params::new()).unwrap_err();
    assert_eq!(
        bus.joined(),
        "W 30 1f.0001 0002|R 5 1.8221|D 5|R 5 1.8221|D 5|R 5 1.8221"
    );
    assert_eq!(
        e.to_string(),
        "line 4: P phy 0x5 1.8221 read 0x0003, expected 0x12e3 (mask 0xffff, 3 reads)"
    );
    // the bus master fails at the third access: the status is reported, nothing follows
    let mut bus = LogBus::with(&[]);
    bus.fail_at = Some(2);
    let e = seq.run(&mut bus, 0x1e0, &Params::new()).unwrap_err();
    assert_eq!(bus.joined(), "W 1e0 1f.0001 0002|R 5 1.8221|D 5");
    assert_eq!(e.to_string(), "line 4: P phy 0x5 1.8221: MDIO error -7");
    assert!(matches!(
        e.kind,
        ExecErrorKind::Bus {
            error: -7,
            phy: 5,
            reg: 0x8221,
            ..
        }
    ));
    let mut bus = LogBus::with(&[]);
    bus.fail_at = Some(0);
    let e = seq.run(&mut bus, 0x1e0, &Params::new()).unwrap_err();
    assert_eq!(e.to_string(), "line 3: W phy 0x1e0 31.0001: MDIO error -7");
    // delays of 0 reach the bus too (the renderer prints them)
    let mut bus = LogBus::with(&[]);
    let _ = t.sequence("d").unwrap().run(&mut bus, 1, &Params::new()).unwrap_err();
    assert_eq!(bus.joined(), "D 0|R 1 1.0001|D 0|R 1 1.0001");
}

#[test]
fn every_sequence_of_the_test_table_runs_with_full_parameters() {
    let t = MacsecTable::parse(ms_table(true, true).as_bytes()).unwrap();
    let mut par = Params::new();
    for name in ParamName::ALL {
        let bytes: Vec<u8> = (0..name.max_len()).map(|i| 0xc0 + i).collect();
        par.set(name, &bytes).unwrap();
    }
    for (name, seq) in t.sequences() {
        // every poll of the table wants bit 0 of 2df
        let mut bus = LogBus::with(&[(0x20f, &[1]), (0x21f, &[1])]);
        let out = run(&mut bus, seq.ops(), 0x10, &par).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(out.cond && out.last == 0);
    }
    // the SA install writes the key bytes where the table says, high byte first
    let mut bus = LogBus::with(&[(0x20f, &[1])]);
    t.sequence("sa-add-first.egress.2.gcm-aes-xpn-256")
        .unwrap()
        .run(&mut bus, 0x10, &par)
        .unwrap();
    assert_eq!(
        bus.joined(),
        "W 10 1.0200 0132|W 10 1.0201 c1c0|W 10 1.0202 dfde|W 10 1.0203 cfc0|W 10 1.0204 c0c7|W 10 1.0205 c1c0|W 10 1.0206 c5c4|\
         W 10 1.0207 c1c0|W 10 1.0208 c0c3|W 10 1.0209 c4cb|W 10 1.020a c0c0|R 10 1.020f"
    );
}

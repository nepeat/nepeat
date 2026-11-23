//! The ext-phy parser cases of sai-openbcm's `tests/test_extphy.inc`, ported, and the executor
//! on ext-phy sequences (the C++ `run_seq` is only reachable through a switch there; its rules
//! are tested here directly, its messages word for word).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use common::LogBus;
use phy_seq::{ExtPhyTable, OpKind, Params, PhySel, Section, run};

const GOOD: &str = "ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\n\
    port 1\nlink\n\
    \x20 R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n\
    \x20 C 10 1 a101 3 3\n\
    admin-down\n  W 10 1 d139 351\n\
    admin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\n\
    port 5\nlink\n  C 11 1 a101 3 3\n";

fn err(text: &str) -> String {
    ExtPhyTable::parse(text.as_bytes()).unwrap_err().to_string()
}

#[test]
fn the_good_table_of_the_cxx_test() {
    let t = ExtPhyTable::parse(GOOD.as_bytes()).unwrap();
    assert_eq!(t.ports().len(), 2);
    assert_eq!((t.poll_ms(), t.up_polls(), t.holdoff_ms()), (0, 2, 50));
    let p = &t.ports()[0];
    assert_eq!((p.lane(), p.line()), (1, 6));
    assert_eq!(p.ops(Section::Link).len(), 6);
    assert_eq!(p.ops(Section::AdminDown).len(), 1);
    assert_eq!(p.ops(Section::AdminUp).len(), 2);
    assert!(
        matches!(p.ops(Section::AdminUp)[0].kind, OpKind::Expect { cmp, .. } if cmp.mask() == 0xff7f && cmp.want() == 0xa008)
    );
    assert!(
        matches!(p.ops(Section::Link)[3].kind, OpKind::Poll { tries, interval, .. } if tries.get() == 3 && interval.get() == 0)
    );
    assert!(matches!(p.ops(Section::Link)[4].kind, OpKind::Expect { cmp, .. } if cmp.mask() == 0xffff));
    assert_eq!(p.ops(Section::Link)[5].to_string(), "C 10 1 a101 3 3");
    assert!(
        t.ports()
            .iter()
            .flat_map(|p| Section::ALL.map(|s| p.ops(s)))
            .flatten()
            .all(|op| op.reg().is_some_and(|r| matches!(r.phy, PhySel::Fixed(_))))
    );
    assert_eq!(t.ports()[1].lane(), 5);
    // no ports: valid, nothing to do; the defaults
    let t = ExtPhyTable::parse(b"ext-phy 1\n").unwrap();
    assert_eq!(
        (t.poll_ms(), t.up_polls(), t.holdoff_ms(), t.ports().len()),
        (500, 2, 3000, 0)
    );
}

/// The `bad[]` list of `test_extphy.inc`.
#[test]
fn bad_tables_of_the_cxx_test() {
    let bad = [
        ("", "line 1"),
        ("mdio-script 1\n", "line 1"),
        ("ext-phy 2\n", "line 1"),
        ("ext-phy 1\nport 1\nlink\nC 10 0 1 3 3\n", "clause 22"),
        ("ext-phy 1\nport 1\nlink\nC 10 20 1 3 3\n", "line 4"),
        ("ext-phy 1\nport 1\nlink\nC 10 1 10000 3 3\n", "line 4"),
        ("ext-phy 1\nport 1\nlink\nC 10 1 a101 3\n", "line 4"),
        ("ext-phy 1\nport 1\nlink\nC 10 1 a101 3 1\n", "outside the mask"),
        ("ext-phy 1\nport 1\nlink\nW 10 1 a000 6\n", "without a C"),
        ("ext-phy 1\nport 1\nadmin-down\nC 10 1 a101 3 3\n", "outside a link"),
        ("ext-phy 1\nport 1\n", "no sequence"),
        ("ext-phy 1\nW 10 1 a000 6\n", "outside"),
        ("ext-phy 1\nport 1\nW 10 1 a000 6\n", "outside"),
        ("ext-phy 1\nlink\n", "before any port"),
        (
            "ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nport 1\nlink\nC 10 1 a101 3 3\n",
            "twice",
        ),
        (
            "ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nlink\nC 10 1 a101 3 3\n",
            "twice",
        ),
        ("ext-phy 1\nport x\n", "line 2"),
        (
            "ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 0 0\nC 10 1 a101 3 3\n",
            "tries",
        ),
        (
            "ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 5 200000\nC 10 1 a101 3 3\n",
            "interval",
        ),
        ("ext-phy 1\nport 1\nlink\nX 10 1 8221\n", "unknown keyword"),
        ("ext-phy 1\nport 1\nlink\nW 10 1 a000 zz\n", "line 4"),
        ("ext-phy 1\nport 1\nlink\nW 10 1 a000 -6\n", "line 4"),
        ("ext-phy 1\npoll-ms 70000\n", "poll-ms"),
        ("ext-phy 1\nup-polls 0\n", "up-polls"),
        (
            "ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\npoll-ms 100\n",
            "after the first port",
        ),
    ];
    for (text, want) in bad {
        let got = err(text);
        assert!(got.contains(want), "{text:?} gave {got:?}, wanted {want:?} in it");
    }
}

#[test]
fn messages_are_those_of_the_cxx_parser() {
    let cases = [
        ("", "line 1: empty table (first line must be 'ext-phy 1')"),
        (
            "ext-macsec 1\n",
            "line 1: not an ext-phy table (first line must be 'ext-phy 1')",
        ),
        ("ext-phy 1\nseq a\n", "line 2: unknown keyword 'seq'"),
        (
            "ext-phy 1\nport 1\nR 1 1 1\n",
            "line 3: operation outside a link / admin-down / admin-up sequence",
        ),
        (
            "ext-phy 1\nport 1\nlink\nport 2\nR 1 1 1\n",
            "line 5: operation outside a link / admin-down / admin-up sequence",
        ),
        (
            "ext-phy 1\nport 1\nlink\nC 1 1 1 1\n",
            "line 4: wrong number of fields for C",
        ),
        (
            "ext-phy 1\nport 1\nlink\nR p 1 1\n",
            "line 4: phy / dev / reg: hex, dev 1..1f, reg up to ffff",
        ),
        (
            "ext-phy 1\nport 1\nlink\nR 1 0 1\n",
            "line 4: dev 0 (clause 22) is not allowed",
        ),
        (
            "ext-phy 1\nport 1\nlink\nW 1 1 1 key[0].00\n",
            "line 4: value: hex up to ffff",
        ),
        ("ext-phy 1\nport 1\nlink\nE 1 1 1 1 x\n", "line 4: mask: hex up to ffff"),
        // tries are checked before the mask rule here (ext-macsec: after)
        (
            "ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 0 0\n",
            "line 4: P: tries 1..1000 and interval 0..100000 usec, decimal",
        ),
        (
            "ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 1 0\n",
            "line 4: value has bits outside the mask: can never match",
        ),
        (
            "ext-phy 1\nport 1\nadmin-up\nC 1 1 1 1 1\n",
            "line 4: C (link condition) outside a link sequence",
        ),
        (
            "ext-phy 1\npoll-ms 1 2\n",
            "line 2: poll-ms: one decimal number (poll-ms 0..60000, up-polls 1..100, holdoff-ms 0..600000)",
        ),
        (
            "ext-phy 1\nholdoff-ms 600001\n",
            "line 2: holdoff-ms: one decimal number (poll-ms 0..60000, up-polls 1..100, holdoff-ms 0..600000)",
        ),
        (
            "ext-phy 1\nport 1\nadmin-up\nR 1 1 1\nup-polls 3\n",
            "line 5: up-polls after the first port",
        ),
        ("ext-phy 1\nport 1 2\n", "line 2: port <first lane, decimal>"),
        (
            "ext-phy 1\nport 3\nadmin-up\nR 1 1 1\n\nport 03\n",
            "line 6: port 03 twice (first at line 2)",
        ),
        ("ext-phy 1\nadmin-up\n", "line 2: admin-up before any port"),
        (
            "ext-phy 1\nport 1\nadmin-down now\n",
            "line 3: admin-down takes no argument",
        ),
        (
            "ext-phy 1\nport 9\nadmin-up\nR 1 1 1\nadmin-up\n",
            "line 5: admin-up twice in port 9",
        ),
        (
            "ext-phy 1\nport 1\nadmin-up\nR 1 1 1\nport 2\n",
            "line 5: port 2 has no sequence",
        ),
        (
            "ext-phy 1\nport 1\nport 2\nlink\nR 1 1 1\n",
            "line 2: port 1 has no sequence",
        ),
        (
            "ext-phy 1\n\nport 7\nlink\nR 1 1 1\n",
            "line 3: port 7: link sequence without a C condition",
        ),
    ];
    for (text, want) in cases {
        assert_eq!(err(text), want, "{text:?}");
    }
    let long = format!("ext-phy 1\nport 1\nadmin-up\n{}", "R 1 1 1\n".repeat(4097));
    assert_eq!(err(&long), "line 4100: sequence longer than 4096 operations");
    // a section keyword may come again while the section is still empty
    assert!(ExtPhyTable::parse(b"ext-phy 1\nport 1\nlink\nlink\nC 1 1 1 1 1\n").is_ok());
}

fn link_ops(text: &str) -> Vec<phy_seq::Op> {
    ExtPhyTable::parse(text.as_bytes()).unwrap().ports()[0]
        .ops(Section::Link)
        .to_vec()
}

/// `run_seq` of `sai_extphy.cpp`: C conditions, the last C value, faults and their messages.
#[test]
fn link_sequences_run_as_run_seq_does() {
    let ops = ExtPhyTable::parse(GOOD.as_bytes()).unwrap().ports()[0]
        .ops(Section::Link)
        .to_vec();
    let none = Params::new();
    // all good: the poll matches on its second read
    let mut bus = LogBus::with(&[(0x8221, &[0x7777, 0, 0x11e3]), (0xa100, &[2]), (0xa101, &[0xfff3])]);
    let out = run(&mut bus, &ops, 0, &none).unwrap();
    assert!(out.cond);
    assert_eq!(out.last, 0xfff3);
    assert_eq!(
        bus.joined(),
        "R 10 1.8221|W 10 1.a002 c161|W 10 1.8222 1111|R 10 1.8221|D 0|R 10 1.8221|R 10 1.a100|R 10 1.a101"
    );
    // the condition does not hold: not a fault, the value is reported
    let mut bus = LogBus::with(&[(0x8221, &[0, 0x11e3]), (0xa100, &[2]), (0xa101, &[1])]);
    let out = run(&mut bus, &ops, 0, &none).unwrap();
    assert!(!out.cond && out.last == 1);
    // the poll never sees its value
    let mut bus = LogBus::with(&[(0xa100, &[2])]);
    let e = run(&mut bus, &ops, 0, &none).unwrap_err();
    assert_eq!(
        e.ext_phy_message(),
        "line 11: P phy 0x10 1.8221 never read 0x11e3 (mask 0xffff) in 3 reads, last 0x0000"
    );
    assert_eq!(bus.log.len(), 3 + 3 + 2);
    // the E check fails
    let mut bus = LogBus::with(&[(0x8221, &[0, 0x11e3]), (0xa100, &[7])]);
    let e = run(&mut bus, &ops, 0, &none).unwrap_err();
    assert_eq!(
        e.ext_phy_message(),
        "line 12: E phy 0x10 1.a100 read 0x0007, expected 0x0002 (mask 0xffff)"
    );
    assert_eq!(bus.log.last().unwrap(), "R 10 1.a100");
    // the PHY stops answering
    let mut bus = LogBus::with(&[]);
    bus.fail_at = Some(1);
    let e = run(&mut bus, &ops, 0, &none).unwrap_err();
    assert_eq!(e.ext_phy_message(), "line 9: W phy 0x10 1.a002: MDIO error -7");
    assert_eq!(bus.log.len(), 1);
}

#[test]
fn every_condition_must_hold_and_the_last_value_is_the_last_read() {
    let ops = link_ops("ext-phy 1\nport 1\nlink\nC 1 1 a 3 3\nC 2 1f b 0 8000\nC 1 1 c 1 1\n");
    let run_with = |a, b, c| {
        let mut bus = LogBus::with(&[(0xa, &[a]), (0xb, &[b]), (0xc, &[c])]);
        let out = run(&mut bus, &ops, 0x99, &Params::new()).unwrap();
        assert_eq!(bus.joined(), "R 1 1.000a|R 2 1f.000b|R 1 1.000c");
        (out.cond, out.last)
    };
    assert_eq!(run_with(3, 0, 1), (true, 1));
    assert_eq!(run_with(7, 0x7fff, 0xffff), (true, 0xffff));
    assert_eq!(run_with(2, 0, 1), (false, 1));
    assert_eq!(run_with(3, 0x8000, 1), (false, 1));
    assert_eq!(run_with(3, 0, 0xfffe), (false, 0xfffe));
}

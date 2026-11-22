//! Property tests: the parser takes any bytes, serialisation round-trips, a run never panics and
//! its statistics agree with the parse pass.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::rc::Rc;

use common::{DiffBus, DiffHooks, Mode, Out};
use mdio_script::{DelayUs, Dev, Label, Op, RegAddr, RunErrorKind, RunOptions, Script, Tries, Until, VerifyMode};
use proptest::prelude::*;

fn config(cases: u32) -> ProptestConfig {
    // No regression files: the source tree is read-only in the nix build.
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn addr() -> impl Strategy<Value = RegAddr> {
    (any::<u16>(), 1u8..=31, any::<u16>()).prop_map(|(phy, dev, reg)| RegAddr {
        phy,
        dev: Dev::new(dev).unwrap(),
        reg,
    })
}

fn delay() -> impl Strategy<Value = DelayUs> {
    (0..=DelayUs::MAX).prop_map(|us| DelayUs::new(us).unwrap())
}

/// Any label the type allows, bytes of every value.
fn label() -> impl Strategy<Value = Label> {
    prop::collection::vec(any::<u8>(), 0..=Label::MAX_LEN).prop_filter_map("not a label", Label::new)
}

/// Any operation the types allow, over the full ranges.
fn op() -> impl Strategy<Value = Op> {
    let until = prop_oneof![Just(Until::Equal), Just(Until::NotEqual)];
    let tries = prop_oneof![Just(1u32), 1..=Tries::MAX].prop_map(|t| Tries::new(t).unwrap());
    prop_oneof![
        label().prop_map(Op::Label),
        delay().prop_map(Op::Delay),
        (addr(), any::<u16>()).prop_map(|(at, val)| Op::Write { at, val }),
        addr().prop_map(|at| Op::Read { at }),
        (addr(), any::<u16>(), any::<u16>()).prop_map(|(at, val, mask)| Op::Expect { at, val, mask }),
        (addr(), until, any::<u16>(), any::<u16>(), tries, delay()).prop_map(
            |(at, until, val, mask, tries, interval)| Op::Poll {
                at,
                until,
                val,
                mask,
                tries,
                interval
            }
        ),
    ]
}

fn serialise(ops: &[Op]) -> Vec<u8> {
    let mut text = b"mdio-script 1\n".to_vec();
    for op in ops {
        op.write_to(&mut text);
        text.push(b'\n');
    }
    text
}

/// The operation as it reads after being written out and parsed again.
fn without_trailing_cr(op: &Op) -> Op {
    match op {
        Op::Label(label) => {
            let bytes = label.as_bytes();
            let end = bytes.iter().rposition(|b| *b != b'\r').map_or(0, |i| i + 1);
            Op::Label(Label::new(&bytes[..end]).unwrap())
        }
        other => other.clone(),
    }
}

/// Found by the property test below in the nix build (2026-11-22): the parser strips a line's
/// trailing CRs and then cuts the label to 150 bytes, which can leave CRs at the label's end.
#[test]
fn a_label_cut_inside_a_run_of_carriage_returns() {
    let text = format!("mdio-script 1\nL {}!!!!\nW 0 1 1 1\n", "\r".repeat(200));
    let script = Script::parse_bytes(text.as_bytes()).unwrap();
    let Some(Op::Label(label)) = script.ops().next() else {
        panic!("no label")
    };
    assert_eq!(label.as_bytes(), "\r".repeat(150).as_bytes()); // as C keeps it
    assert!(Label::new(label.as_bytes()).is_none()); // not a label one can ask to be written
    let again = Script::parse_bytes(&script.to_bytes()).unwrap();
    assert_eq!(again.ops().next(), Some(&Op::Label(Label::new("").unwrap())));
    assert!(script.ops().map(without_trailing_cr).eq(again.ops().cloned()));
}

proptest! {
    #![proptest_config(config(2000))]

    #[test]
    fn parser_takes_any_bytes(data in prop::collection::vec(any::<u8>(), 0..2000)) {
        if let Err(e) = Script::parse_bytes(&data) {
            let _ = e.to_string();
        }
    }

    #[test]
    fn parser_takes_any_string(text in any::<String>()) {
        if let Err(e) = Script::parse(&text) {
            let _ = e.to_string();
        }
    }

    #[test]
    fn parser_takes_script_like_noise(data in prop_oneof![common::noise(), common::corrupted_script()]) {
        match Script::parse_bytes(&data) {
            // Whatever parses can be written out and parses to the same operations again.
            Ok(script) => {
                // (A label the parser cut inside a run of CRs loses them on the way: see Label.)
                let again = Script::parse_bytes(&script.to_bytes()).unwrap();
                prop_assert!(script.ops().map(without_trailing_cr).eq(again.ops().cloned()));
                let _ = script.to_string();
            }
            Err(e) => {
                prop_assert!(e.line <= 1 + u64::try_from(data.len()).unwrap());
                prop_assert!(e.stats.ops <= e.line);
                let _ = e.to_string();
            }
        }
    }

    #[test]
    fn operations_round_trip(ops in prop::collection::vec(op(), 0..40)) {
        let script = Script::parse_bytes(&serialise(&ops)).unwrap();
        prop_assert!(script.ops().eq(ops.iter()));
        // line numbers: the header is line 1, one operation per line
        prop_assert!(script.lines().iter().map(|l| l.line).eq((2u64..).take(ops.len())));
        prop_assert_eq!(script.to_bytes(), serialise(&ops));
        // statistics of the parse pass against the operations
        let st = script.stats();
        let count = |f: fn(&Op) -> bool| u64::try_from(ops.iter().filter(|op| f(op)).count()).unwrap();
        prop_assert_eq!(st.labels, count(|op| matches!(op, Op::Label(_))));
        prop_assert_eq!(st.delays, count(|op| matches!(op, Op::Delay(_))));
        prop_assert_eq!(st.writes, count(|op| matches!(op, Op::Write { .. })));
        prop_assert_eq!(st.polls, count(|op| matches!(op, Op::Poll { .. })));
        prop_assert_eq!(st.reads, count(|op| matches!(op, Op::Read { .. } | Op::Expect { .. } | Op::Poll { .. })));
        prop_assert_eq!(st.ops, st.lines + st.delays);
        prop_assert_eq!(st.ops + st.labels, u64::try_from(ops.len()).unwrap());
        let tail = u64::try_from(ops.iter().rev().take_while(|op| op.is_assertion() || matches!(op, Op::Label(_)))
            .filter(|op| op.is_assertion()).count()).unwrap();
        prop_assert_eq!(st.verify_checks, tail);
        prop_assert_eq!(st.verify_line != 0, tail != 0);
    }

    #[test]
    fn text_round_trips_through_display(text in common::valid_script()) {
        let script = Script::parse(&text).unwrap();
        let again: Script = script.to_string().parse().unwrap();
        prop_assert!(script.ops().eq(again.ops()));
        prop_assert_eq!(again.to_string(), script.to_string());
        let (a, b) = (script.stats(), again.stats());
        prop_assert_eq!((a.ops, a.lines, a.labels, a.delay_us, a.verify_checks), (b.ops, b.lines, b.labels, b.delay_us, b.verify_checks));
    }

    #[test]
    fn run_never_panics_and_statistics_are_consistent(
        text in common::valid_script(),
        (seed, tick, abort_k) in common::jobs(),
        mode in prop::sample::select(Mode::RUNS.to_vec()),
    ) {
        let script = Script::parse(&text).unwrap();
        let parsed = *script.stats();
        let out = Out::default();
        let mut bus = DiffBus::new(seed, Rc::clone(&out));
        let mut hooks = DiffHooks::new(tick, abort_k, Rc::clone(&out));
        let verify = match mode { Mode::Verify => VerifyMode::Report, Mode::Only => VerifyMode::Only, _ => VerifyMode::Stop };
        let run = script.run(&mut bus, &mut hooks, RunOptions { verify });
        let (st, kind) = match &run {
            Ok(st) => (*st, None),
            Err(e) => {
                prop_assert!(!e.message.is_empty());
                prop_assert_eq!(e.to_string(), e.message.clone());
                (e.stats, Some(e.kind))
            }
        };
        prop_assert!(st.ops <= parsed.ops && st.lines <= parsed.lines && st.labels <= parsed.labels);
        prop_assert!(st.writes <= parsed.writes && st.delays <= parsed.delays && st.delay_us <= parsed.delay_us);
        prop_assert!(st.verify_failed <= st.verify_checks && st.verify_checks <= parsed.verify_checks);
        prop_assert!(st.reads <= parsed.reads + st.poll_rereads);
        if !tick {
            prop_assert_eq!(hooks.ticks, 0);
        }
        let trace = out.borrow();
        let bus_reads = u64::try_from(trace.lines().filter(|l| l.starts_with("R ")).count()).unwrap();
        let bus_writes = u64::try_from(trace.lines().filter(|l| l.starts_with("W ")).count()).unwrap();
        match kind {
            // Ran to its end: every operation was done, and the counters are those of the parse pass.
            None | Some(RunErrorKind::Verify) => {
                prop_assert_eq!(st.ops, parsed.ops);
                prop_assert_eq!((st.lines, st.writes, st.delays, st.polls, st.labels, st.delay_us),
                    (parsed.lines, parsed.writes, parsed.delays, parsed.polls, parsed.labels, parsed.delay_us));
                prop_assert_eq!(st.reads, parsed.reads + st.poll_rereads);
                prop_assert_eq!(st.verify_line, if mode == Mode::Run { 0 } else { parsed.verify_line });
                prop_assert_eq!(kind.is_some(), st.verify_failed > 0);
                if mode == Mode::Only {
                    prop_assert_eq!((bus_writes, bus_reads), (0, parsed.verify_checks));
                } else {
                    prop_assert_eq!((bus_writes, bus_reads), (st.writes, st.reads));
                }
                if mode != Mode::Run {
                    prop_assert_eq!(st.verify_checks, parsed.verify_checks);
                }
            }
            Some(RunErrorKind::NoVerifySection) => {
                prop_assert!(mode == Mode::Only && parsed.verify_line == 0);
                prop_assert_eq!(st, mdio_script::Stats::default());
                prop_assert!(trace.is_empty());
            }
            Some(RunErrorKind::Abort) => prop_assert!(tick && abort_k != 0 && hooks.ticks == abort_k && st.ops < parsed.ops),
            Some(RunErrorKind::Poll | RunErrorKind::Io) => {
                prop_assert!(st.ops < parsed.ops);
                prop_assert_eq!(st.verify_line, 0);
            }
        }
    }
}

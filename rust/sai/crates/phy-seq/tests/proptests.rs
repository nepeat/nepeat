//! Property tests: the parsers on generated texts, the executor on generated sequences.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::fmt::Write as _;

use common::{LogBus, extphy_text, macsec_text};
use phy_seq::{ExecErrorKind, ExtPhyTable, MacsecTable, OpKind, ParamName, Params, Section, Value, run};
use proptest::prelude::*;

/// A table text written from a parsed table: what [`phy_seq::Op`]'s `Display` promises.
fn macsec_rewritten(t: &MacsecTable) -> String {
    let mut text = String::from("ext-macsec 1\n");
    if t.counters() == phy_seq::CounterMode::Absolute {
        text.push_str("counters absolute\n");
    }
    writeln!(text, "ifg-bits {}", t.ifg_bits()).unwrap();
    for (lane, phy) in t.ports() {
        writeln!(text, "port {lane} {phy:x}").unwrap();
    }
    for (name, seq) in t.sequences() {
        writeln!(text, "seq {name}").unwrap();
        for op in seq.ops() {
            writeln!(text, "{op}").unwrap();
        }
    }
    text
}

fn extphy_rewritten(t: &ExtPhyTable) -> String {
    let mut text = format!(
        "ext-phy 1\npoll-ms {}\nup-polls {}\nholdoff-ms {}\n",
        t.poll_ms(),
        t.up_polls(),
        t.holdoff_ms()
    );
    for port in t.ports() {
        writeln!(text, "port {}", port.lane()).unwrap();
        for section in Section::ALL {
            if !port.ops(section).is_empty() {
                writeln!(text, "{}", section.keyword()).unwrap();
            }
            for op in port.ops(section) {
                writeln!(text, "{op}").unwrap();
            }
        }
    }
    text
}

/// Parameters with every byte set: byte `i` of every parameter is `seed + i`.
fn full_params(seed: u8) -> Params {
    let mut p = Params::new();
    for name in ParamName::ALL {
        let bytes: Vec<u8> = (0..name.max_len()).map(|i| seed.wrapping_add(i)).collect();
        p.set(name, &bytes).unwrap();
    }
    p
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    #[test]
    fn the_parsers_never_panic_on_any_bytes(text in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = MacsecTable::parse(&text);
        let _ = ExtPhyTable::parse(&text);
        let mut with_header = b"ext-macsec 1\nseq s\n".to_vec();
        with_header.extend_from_slice(&text);
        if let Err(e) = MacsecTable::parse(&with_header) {
            prop_assert!(e.line >= 1);
            let prefix = format!("line {}: ", e.line);
            prop_assert!(e.to_bytes().starts_with(prefix.as_bytes()));
        }
    }

    #[test]
    fn a_parsed_macsec_table_survives_being_written_out(text in macsec_text()) {
        match MacsecTable::parse(&text) {
            Ok(table) => {
                let again = MacsecTable::parse(macsec_rewritten(&table).as_bytes());
                // line numbers differ; everything else must not
                let strip = |t: &MacsecTable| {
                    t.sequences().map(|(n, s)| (n.to_owned(), s.ops().iter().map(|o| o.kind.clone()).collect::<Vec<_>>())).collect::<Vec<_>>()
                };
                let again = again.map_err(|e| TestCaseError::fail(format!("rewritten table refused: {e}")))?;
                prop_assert_eq!(strip(&table), strip(&again));
                prop_assert_eq!(table.ports().collect::<Vec<_>>(), again.ports().collect::<Vec<_>>());
                prop_assert_eq!((table.counters(), table.ifg_bits()), (again.counters(), again.ifg_bits()));
                // what the types promise about a parsed table
                for (_, seq) in table.sequences() {
                    prop_assert!(seq.ops().len() <= phy_seq::limits::EXT_MACSEC_MAX_OPS);
                    let no_cond = seq.ops().iter().all(|o| !matches!(o.kind, OpKind::Cond { .. }));
                    prop_assert!(no_cond);
                }
            }
            Err(e) => {
                let lines = text.split(|&b| b == b'\n').count();
                prop_assert!(e.line >= 1 && e.line as usize <= lines.max(1), "line {} of {lines}", e.line);
            }
        }
    }

    #[test]
    fn a_parsed_extphy_table_survives_being_written_out(text in extphy_text()) {
        if let Ok(table) = ExtPhyTable::parse(&text) {
            let again = ExtPhyTable::parse(extphy_rewritten(&table).as_bytes())
                .map_err(|e| TestCaseError::fail(format!("rewritten table refused: {e}")))?;
            let strip = |t: &ExtPhyTable| {
                t.ports()
                    .iter()
                    .map(|p| (p.lane(), Section::ALL.map(|s| p.ops(s).iter().map(|o| o.kind.clone()).collect::<Vec<_>>())))
                    .collect::<Vec<_>>()
            };
            prop_assert_eq!(strip(&table), strip(&again));
            prop_assert_eq!((table.poll_ms(), table.up_polls(), table.holdoff_ms()), (again.poll_ms(), again.up_polls(), again.holdoff_ms()));
            for port in table.ports() {
                let link = port.ops(Section::Link);
                let has_cond = link.iter().any(|o| matches!(o.kind, OpKind::Cond { .. }));
                prop_assert!(link.is_empty() || has_cond);
                for section in [Section::AdminDown, Section::AdminUp] {
                    let no_cond = port.ops(section).iter().all(|o| !matches!(o.kind, OpKind::Cond { .. }));
                    prop_assert!(no_cond);
                }
                prop_assert!(Section::ALL.iter().any(|&s| !port.ops(s).is_empty()));
            }
        }
    }

    /// The parameters reach the bus only through the words of the writes that name them:
    /// everything else of a run (the other words, the accesses, the results, the outcome and
    /// every error message) is the same whatever the key is.
    #[test]
    fn parameters_only_change_the_words_that_name_them(
        text in macsec_text(),
        phy in any::<u16>(),
        reads in proptest::collection::vec(any::<u16>(), 0..12),
        fail_at in proptest::option::of(0usize..40),
    ) {
        let Ok(table) = MacsecTable::parse(&text) else { return Ok(()) };
        for (_, seq) in table.sequences() {
            let run_with = |seed: u8| {
                let mut bus = LogBus::default();
                // every register answers from the same list, in order
                for op in seq.ops() {
                    if let Some(at) = op.reg() {
                        bus.answers.entry(at.reg).or_insert_with(|| reads.iter().copied().collect());
                    }
                }
                bus.fail_at = fail_at;
                let out = run(&mut bus, seq.ops(), phy, &full_params(seed));
                (bus, out.map_err(|e| e.to_string()))
            };
            let (a, out_a) = run_with(0x10);
            let (b, out_b) = run_with(0xa5);
            prop_assert_eq!(&out_a, &out_b);
            prop_assert_eq!(a.log.len(), b.log.len());
            prop_assert_eq!(&a.begun, &b.begun);
            // the operations run in order, each at most once
            prop_assert!(a.begun.iter().copied().eq(0..a.begun.len()));
            let mut writes = seq.ops().iter().filter(|o| matches!(o.kind, OpKind::Write { .. }));
            for (x, y) in a.log.iter().zip(&b.log) {
                if x.starts_with('W') {
                    let op = writes.next().unwrap();
                    let by_param = matches!(&op.kind, OpKind::Write { value: Value::Bytes { hi, lo }, .. }
                        if matches!(hi, phy_seq::ByteSrc::Param(_)) || matches!(lo, phy_seq::ByteSrc::Param(_)));
                    prop_assert!(x == y || by_param, "{x} / {y} for {op}");
                } else {
                    prop_assert_eq!(x, y);
                }
            }
            if let Err(message) = &out_a {
                prop_assert!(message.starts_with("line "));
            }
        }
    }

    /// A sequence is never started with a parameter missing, and after any failure nothing
    /// more is accessed.
    #[test]
    fn nothing_is_accessed_without_the_parameters_or_after_a_failure(
        text in macsec_text(),
        missing in 0usize..8,
        fail_at in 0usize..30,
    ) {
        let Ok(table) = MacsecTable::parse(&text) else { return Ok(()) };
        for (_, seq) in table.sequences() {
            let need = seq.required_params();
            if let Some(&(name, len)) = need.get(missing % need.len().max(1)) {
                let mut params = full_params(1);
                params.set(name, &vec![0; usize::from(len) - 1]).unwrap();
                let mut bus = LogBus::default();
                let e = run(&mut bus, seq.ops(), 1, &params).unwrap_err();
                let named = matches!(e.kind, ExecErrorKind::MissingParam(p) if p.name() == name && p.index() == len - 1);
                prop_assert!(named);
                prop_assert!(bus.log.is_empty() && bus.accesses == 0 && bus.begun.is_empty());
            }
            let mut bus = LogBus { fail_at: Some(fail_at), ..LogBus::default() };
            if let Err(e) = run(&mut bus, seq.ops(), 1, &full_params(1)) {
                let bus_failed = matches!(e.kind, ExecErrorKind::Bus { .. });
                if bus_failed {
                    prop_assert_eq!(bus.accesses, fail_at + 1);
                }
                let begun = *bus.begun.last().unwrap();
                prop_assert_eq!(seq.ops()[begun].line, e.line);
            }
        }
    }
}

#[test]
fn the_generators_make_both_good_and_bad_tables() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let (mut good, mut bad, mut ops) = (0, 0, 0);
    for _ in 0..600 {
        let text = macsec_text().new_tree(&mut runner).unwrap().current();
        match MacsecTable::parse(&text) {
            Ok(t) => {
                good += 1;
                ops += t.sequences().map(|(_, s)| s.ops().len()).sum::<usize>();
            }
            Err(_) => bad += 1,
        }
    }
    assert!(
        good >= 150 && bad >= 150 && ops >= 2000,
        "macsec: {good} good, {bad} bad, {ops} operations"
    );
    let (mut good, mut bad) = (0, 0);
    for _ in 0..600 {
        let text = extphy_text().new_tree(&mut runner).unwrap().current();
        if ExtPhyTable::parse(&text).is_ok() {
            good += 1;
        } else {
            bad += 1;
        }
    }
    assert!(good >= 150 && bad >= 150, "ext-phy: {good} good, {bad} bad");
}

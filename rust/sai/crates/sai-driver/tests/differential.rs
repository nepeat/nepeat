//! The C++-vs-Rust frontend differential, against the golden. Every op script of this crate's
//! `tests/driver-scripts/*.ops` (01-09: the C++ frontend's scripts, moved here unchanged; 10-11: the
//! external-PHY runtime and the `MACsec` objects) runs through this crate's driver (the Rust core on its
//! virtual backend); its output is compared op block by op block (`sai_driver::compare`) with the output
//! of the C++ `frontend_driver` (the C++ libsai on its fake backend; the C++ frontend was deleted in H5 on
//! 2026-11-27, last in the tree at commit 767d9b0), as captured on 2026-11-26 in
//! `tests/golden/differential-cpp.txt`; its inputs, the 11 scripts and the two table files, are
//! `tests/golden/differential-inputs.txt`. Every differing block must be covered by an entry of
//! `tests/differential-allow.txt`, and every entry must cover a block. Always runs: no C++, no compiler, no
//! environment. A script of the tree that differs from the golden's copy fails ("script changed since the
//! golden"). The golden is never regenerated from Rust output (nor at all: the C++ is gone).
//! Negative controls: a changed line of the golden output, a changed script, and a dropped block are
//! reported.
//!
//! Scripts 10 and 11 name two table files by token (`@EXT_PHY_TABLE@`, `@EXT_MACSEC_TABLE@`): the test writes
//! the golden's copies of the tables (`kExtPhyGood` and `ms_table()` of the C++ frontend's
//! `test_tables.h`) into the scratch area. Outputs are compared with the table paths written as the tokens.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test"
)]

use sai_driver::compare::{self, Diff};
use sai_driver::{Driver, OnHardware, Options};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The golden C++ outputs (`@@@@ cpp <stem>` sections) and their inputs (`@@@@ script <stem>`,
/// `@@@@ table <name>`); how they were captured: the header of the first.
const CPP_GOLDEN: &str = include_str!("golden/differential-cpp.txt");
const INPUTS_GOLDEN: &str = include_str!("golden/differential-inputs.txt");
/// The allow-list of the golden leg (the live leg reads it from disk, so it can be rewritten).
const ALLOW: &str = include_str!("differential-allow.txt");
/// The ops of the 11 golden scripts, at capture.
const GOLDEN_OPS: usize = 455;

/// The scripts of this tree, compiled in (so the golden leg needs no file).
const IN_TREE: [(&str, &str); 11] = [
    ("01-switch", include_str!("driver-scripts/01-switch.ops")),
    ("02-ports", include_str!("driver-scripts/02-ports.ops")),
    ("03-vlan-fdb", include_str!("driver-scripts/03-vlan-fdb.ops")),
    ("04-lag", include_str!("driver-scripts/04-lag.ops")),
    ("05-hostif-traps", include_str!("driver-scripts/05-hostif-traps.ops")),
    ("06-l3", include_str!("driver-scripts/06-l3.ops")),
    ("07-acl-copp", include_str!("driver-scripts/07-acl-copp.ops")),
    ("08-qos", include_str!("driver-scripts/08-qos.ops")),
    (
        "09-mirror-sample-hash",
        include_str!("driver-scripts/09-mirror-sample-hash.ops"),
    ),
    ("10-ext-phy", include_str!("driver-scripts/10-ext-phy.ops")),
    ("11-macsec", include_str!("driver-scripts/11-macsec.ops")),
];

/// A line for whoever runs the suite: stderr, and `$SAI_RS_TEST_SUMMARY` (a passing test's stderr
/// is captured).
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The token of a script that names a table file the test writes.
const EXT_PHY_TOKEN: &str = "@EXT_PHY_TABLE@";
const EXT_MACSEC_TOKEN: &str = "@EXT_MACSEC_TABLE@";

/// The two table files: (`MACsec`, external PHY).
type Tables = (PathBuf, PathBuf);

/// A script with its table tokens replaced by the table files.
fn with_tables(text: &str, t: &Tables) -> String {
    text.replace(EXT_PHY_TOKEN, t.1.to_str().unwrap())
        .replace(EXT_MACSEC_TOKEN, t.0.to_str().unwrap())
}

/// An output with the table files written as their tokens (the golden's form).
fn tokens_back(text: &str, t: &Tables) -> String {
    text.replace(t.1.to_str().unwrap(), EXT_PHY_TOKEN)
        .replace(t.0.to_str().unwrap(), EXT_MACSEC_TOKEN)
}

fn scratch() -> PathBuf {
    var("SAI_RS_TEST_TMPDIR").map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
}

fn options() -> Options {
    Options {
        on_hardware: if var("SAI_RS_VIRTUAL_ON_HARDWARE").as_deref() == Some("1") {
            OnHardware::Yes
        } else {
            OnHardware::No
        },
        ..Options::default()
    }
}

// ---- the golden: parsing --------------------------------------------------------------------

/// The sections of a golden: (head, text) per `@@@@ <head>` line, the text being every line up to
/// the next such line, exactly; the `#` header before the first is skipped.
fn sections(golden: &str) -> Vec<(&str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for line in golden.split_inclusive('\n') {
        if let Some(head) = line.strip_prefix("@@@@ ") {
            out.push((head.trim_end(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
        } else {
            assert!(line.starts_with('#'), "golden header line without '#': {line}");
        }
    }
    out
}

/// The golden inputs: the scripts (stem, text) in order and the table files by name.
fn golden_inputs(golden: &str) -> (Vec<(String, String)>, BTreeMap<String, String>) {
    let (mut scripts, mut tables) = (Vec::new(), BTreeMap::new());
    for (head, text) in sections(golden) {
        match head.split_once(' ') {
            Some(("script", stem)) => scripts.push((stem.to_owned(), text)),
            Some(("table", name)) => {
                tables.insert(name.to_owned(), text);
            }
            _ => panic!("differential-inputs.txt: unknown section {head}"),
        }
    }
    (scripts, tables)
}

/// The golden C++ outputs (stem, text) in order.
fn golden_cpp(golden: &str) -> Vec<(String, String)> {
    sections(golden)
        .into_iter()
        .map(|(head, text)| {
            let stem = head
                .strip_prefix("cpp ")
                .unwrap_or_else(|| panic!("differential-cpp.txt: unknown section {head}"));
            (stem.to_owned(), text)
        })
        .collect()
}

/// The golden's table files written into a fresh directory of the scratch area.
fn golden_tables(golden_inputs_text: &str) -> Tables {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = scratch().join(format!(
        "sai-driver-golden-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (_, tables) = golden_inputs(golden_inputs_text);
    let (macsec, ext_phy) = (dir.join("ext-macsec.tbl"), dir.join("ext-phy.tbl"));
    std::fs::write(&macsec, tables.get("ext-macsec.tbl").unwrap()).unwrap();
    std::fs::write(&ext_phy, tables.get("ext-phy.tbl").unwrap()).unwrap();
    (macsec, ext_phy)
}

/// The failures of the scripts of the tree against the golden's copies (empty: all 11 equal).
fn scripts_unchanged(golden: &[(String, String)], tree: &[(&str, &str)]) -> String {
    let mut failures = String::new();
    for (stem, now) in tree {
        match golden.iter().find(|(s, _)| s == stem) {
            Some((_, text)) if text == now => {}
            Some(_) => {
                let _ = writeln!(
                    failures,
                    "script changed since the golden: {stem}; the golden C++ output is of the captured text (golden/differential-inputs.txt)"
                );
            }
            None => {
                let _ = writeln!(failures, "script {stem} of this tree is not in the golden inputs");
            }
        }
    }
    for (stem, _) in golden {
        if !tree.iter().any(|(s, _)| s == stem) {
            let _ = writeln!(failures, "golden script {stem} is no longer in this tree");
        }
    }
    failures
}

/// The Rust driver's output on a script, the table paths written as tokens.
fn rust_output(text: &str, t: &Tables) -> String {
    tokens_back(&Driver::new(options()).run_script(&with_tables(text, t)), t)
}

/// The Rust driver on every script of the tree against `golden` (stem, C++ output): the differing
/// blocks, the per-script (blocks, differing), and the ops of every script.
fn compare_with_golden(
    golden: &[(String, String)],
    tree: &[(&str, &str)],
    t: &Tables,
) -> (Vec<Diff>, BTreeMap<String, (usize, usize)>) {
    let mut diffs = Vec::new();
    let mut per_script = BTreeMap::new();
    for (stem, text) in tree {
        let (_, cpp) = golden
            .iter()
            .find(|(s, _)| s == stem)
            .unwrap_or_else(|| panic!("no golden C++ output for {stem}"));
        let rust = rust_output(text, t);
        let c = compare::compare(stem, cpp, &rust).unwrap_or_else(|e| panic!("{e}"));
        per_script.insert((*stem).to_owned(), (c.blocks, c.diffs.len()));
        diffs.extend(c.diffs);
    }
    (diffs, per_script)
}

/// The failure text of a verdict: every differing block no entry covers, every entry that covers none.
fn failure_text(entries: &[compare::Entry], verdict: &compare::Verdict, diffs: &[Diff]) -> String {
    let mut failures = String::new();
    for i in verdict.uncovered() {
        let d = diffs.get(i).unwrap();
        let _ = write!(
            failures,
            "NOT ALLOWED: {} block {}: > {}\n{}",
            d.script,
            d.index,
            d.op,
            compare::unified(d)
        );
    }
    for i in verdict.stale() {
        let e = entries.get(i).unwrap();
        let _ = writeln!(
            failures,
            "STALE allow-list entry (line {}): {}\t{}\t{}",
            e.line,
            e.script,
            e.pattern.as_str(),
            e.reason
        );
    }
    failures
}

fn tree_scripts() -> Vec<(&'static str, &'static str)> {
    IN_TREE.to_vec()
}

// ---- the golden leg --------------------------------------------------------------------------

#[test]
fn rust_driver_agrees_with_the_golden_cpp_output_but_for_the_allow_list() {
    let (golden_scripts, _) = golden_inputs(INPUTS_GOLDEN);
    let mut failures = scripts_unchanged(&golden_scripts, &IN_TREE);
    let golden = golden_cpp(CPP_GOLDEN);
    assert_eq!(
        golden.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        golden_scripts.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        "the golden output and the golden inputs name different scripts"
    );
    let tables = golden_tables(INPUTS_GOLDEN);
    let (diffs, per_script) = compare_with_golden(&golden, &tree_scripts(), &tables);
    let total: usize = per_script.values().map(|(b, _)| b).sum();
    let entries = compare::parse_allow(ALLOW).unwrap_or_else(|e| panic!("differential-allow.txt: {e}"));
    let verdict = compare::judge(&entries, &diffs);
    summary(&format!(
        "sai-driver differential against the golden C++ output: {} scripts, {total} ops, {} identical, {} differ (NOT allowed {}), {} allow-list entries ({} stale)",
        per_script.len(),
        total - diffs.len(),
        diffs.len(),
        verdict.uncovered().len(),
        entries.len(),
        verdict.stale().len()
    ));
    failures.push_str(&failure_text(&entries, &verdict, &diffs));
    assert!(failures.is_empty(), "{failures}");
    assert_eq!(total, GOLDEN_OPS, "the golden holds {GOLDEN_OPS} ops");
}

/// The negative controls of the golden: the comparison reports a changed byte of the golden output (in
/// every script), a changed script, a dropped op block and a script missing from the golden.
#[test]
fn the_golden_comparison_reports_a_changed_golden_line_script_or_block() {
    let (golden_scripts, _) = golden_inputs(INPUTS_GOLDEN);
    let golden = golden_cpp(CPP_GOLDEN);
    let tables = golden_tables(INPUTS_GOLDEN);
    let tree = tree_scripts();
    let entries = compare::parse_allow(ALLOW).unwrap();

    // a changed byte in each script's golden output (the first value or status line)
    for (i, (stem, _)) in golden.iter().enumerate() {
        let mut changed = golden.clone();
        let text = &changed[i].1;
        let at = text
            .split_inclusive('\n')
            .scan(0usize, |off, l| {
                let start = *off;
                *off += l.len();
                Some((start, l))
            })
            .find(|(_, l)| !l.starts_with("> ") && l.trim_end().len() > 1)
            .unwrap_or_else(|| panic!("{stem}: no output line to change"));
        let at = at.0 + at.1.trim_end().len() - 1;
        let mut bytes = text.clone().into_bytes();
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        changed[i].1 = String::from_utf8(bytes).unwrap();
        let (diffs, _) = compare_with_golden(&changed, &tree, &tables);
        let verdict = compare::judge(&entries, &diffs);
        assert_eq!(
            (diffs.len(), verdict.uncovered().len()),
            (1, 1),
            "negative control on {stem}: a changed byte of the golden output is not reported exactly once"
        );
        assert_eq!(&diffs[0].script, stem);
    }

    // a dropped block: the last op of the golden output of 06-l3 (its echo line and what follows)
    let mut dropped = golden.clone();
    let i = dropped.iter().position(|(s, _)| s == "06-l3").unwrap();
    let cut = dropped[i].1.rfind("\n> ").unwrap() + 1;
    dropped[i].1.truncate(cut);
    let reported = match compare::compare("06-l3", &dropped[i].1, &rust_output(IN_TREE[5].1, &tables)) {
        Err(_) => true,
        Ok(c) => !c.diffs.is_empty(),
    };
    assert!(
        reported,
        "negative control: a dropped op block of the golden output is not reported"
    );

    // a changed script
    let mut edited: Vec<(String, String)> = golden_scripts.clone();
    edited[3].1.push_str("# changed\n");
    assert!(
        scripts_unchanged(&edited, &IN_TREE).contains("script changed since the golden: 04-lag"),
        "negative control: a changed script is not reported"
    );
    // a script missing from the golden
    edited.remove(0);
    assert!(
        scripts_unchanged(&edited, &IN_TREE).contains("script 01-switch of this tree is not in the golden inputs"),
        "negative control: a script missing from the golden is not reported"
    );
    summary(
        "sai-driver differential: negative controls ok (a changed golden byte in each of 11 scripts, a dropped block, a changed script, a missing script)",
    );
}

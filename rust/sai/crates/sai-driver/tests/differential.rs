//! The C++-vs-Rust frontend differential: every op script of this crate's `tests/driver-scripts/*.ops`
//! (01-09: the frozen C++ frontend's scripts, moved here unchanged; 10-11: the external-PHY runtime
//! and the `MACsec` objects) runs through the C++ `frontend_driver` (the C++ libsai on its fake
//! backend, frozen since 2026-11-23 in `ai_re/nix/packages/sai-teralynx/frontend`) and through this
//! crate's driver (the Rust core on its virtual backend); the outputs are compared op block by op
//! block (`sai_driver::compare`). Every differing block must be covered by an entry of
//! `tests/differential-allow.txt`, and every entry must cover a block. Two legs:
//!
//! * GOLDEN (always runs, no C++, no compiler): the Rust driver against the C++ driver's output as
//!   captured on 2026-11-26 (`tests/golden/differential-cpp.txt`; its inputs, the 11 scripts and the two
//!   table files, are `tests/golden/differential-inputs.txt`). A script of the tree that differs from
//!   the golden's copy fails ("script changed since the golden"). Never regenerated from Rust output.
//!   Negative controls: a changed line of the golden output, a changed script, and a dropped block are
//!   reported.
//! * LIVE (only with the C++ sources; else SKIPPED, said on stderr and in `$SAI_RS_TEST_SUMMARY`): the
//!   same comparison against the live C++ driver, and `live_cpp_output_equals_the_golden`: the live C++
//!   output (and its table files) is byte-equal to the golden, the proof that the golden is the C++'s.
//!
//! The C++ driver of the live leg:
//! - `SAI_DRIVER_CPP`: a prebuilt `frontend_driver` (a Nix check passes one in); or
//! - `$SAI_RS_TEST_BIN/sai-cpp-frontend-driver`, with the scripts and the allow-list in
//!   `$SAI_RS_TEST_BIN/sai-driver/` (prebuilt by `build.rs` for a target without a compiler; the scripts
//!   that need the C++ tables are not in it); or
//! - built here from `SAI_OPENBCM_TESTS` (the frozen frontend's `tests`; else `$SAI_OPENBCM_SRC/../tests`)
//!   and `SAI_INC`, with `$CXX` / `$CC` (cached in the target's temp directory by a hash of the inputs).
//!
//! Scripts 10 and 11 name two table files by token (`@EXT_PHY_TABLE@`, `@EXT_MACSEC_TABLE@`): the live leg
//! writes the tables of the frontend's `tests/test_tables.h` (`kExtPhyGood`, without its hold-off so
//! nothing depends on the clock; `ms_table()`), printed by a three-line C++ program built with `$CXX`; the
//! golden leg writes the golden's copies. Outputs are compared with the table paths written as the tokens.
//!
//! With the C++ sources, a missing allow-list fails. `SAI_DRIVER_ALLOW` names another allow-list;
//! `SAI_DRIVER_ALLOW_UPDATE=1` rewrites it from today's differences (reasons kept where an entry still
//! covers) and passes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test"
)]

#[path = "../src/cpp_build.rs"]
mod cpp_build;

use sai_driver::compare::{self, Diff};
use sai_driver::{Driver, OnHardware, Options};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
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

/// A hash of everything the C++ build reads.
fn inputs_hash(s: &cpp_build::Sources, inc: &Path, cxx: &str, cc: &str) -> String {
    let mut h = DefaultHasher::new();
    (inc, cxx, cc).hash(&mut h);
    for dir in [&s.src, &s.tests] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for f in files {
            f.hash(&mut h);
            std::fs::read(&f).unwrap_or_default().hash(&mut h);
        }
    }
    format!("{:016x}", h.finish())
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

// ---- the live leg ----------------------------------------------------------------------------

struct Setup {
    exe: PathBuf,
    scripts: Vec<PathBuf>,
    allow: PathBuf,
    /// The tables of the frontend's `tests/test_tables.h`, written into the scratch area (needs the C++ sources).
    tables: Option<Tables>,
}

/// One script through the live C++ driver.
struct Run {
    stem: String,
    /// The script text as run (tokens replaced by the table files).
    text: String,
    /// The C++ driver's stdout.
    cpp: String,
}

struct Live {
    setup: Setup,
    runs: Vec<Run>,
}

fn setup() -> Option<Setup> {
    let manifest_allow = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/differential-allow.txt");
    let own_scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/driver-scripts");
    let bin_data = var("SAI_RS_TEST_BIN").map(|b| Path::new(&b).join("sai-driver"));
    let sources = cpp_build::find_sources();
    // the scripts: this crate's (with the sources, for the C++ tables), else the prebuilt bundle's copy
    let scripts_dir = match (&sources, &bin_data) {
        (Ok(_), _) => Some(own_scripts),
        (Err(_), Some(d)) if d.join("driver-scripts").is_dir() => Some(d.join("driver-scripts")),
        _ => None,
    };
    let allow = var("SAI_DRIVER_ALLOW").map_or_else(
        || match (&sources, &bin_data) {
            (Err(_), Some(d)) if d.join("differential-allow.txt").is_file() => d.join("differential-allow.txt"),
            _ => manifest_allow.clone(),
        },
        PathBuf::from,
    );
    let prebuilt = var("SAI_DRIVER_CPP").map(PathBuf::from).or_else(|| {
        var("SAI_RS_TEST_BIN")
            .map(|b| Path::new(&b).join(cpp_build::DRIVER))
            .filter(|p| p.is_file())
    });
    let Some(scripts_dir) = scripts_dir else {
        summary(&format!(
            "sai-driver differential: live leg SKIPPED: no op scripts ({}; no $SAI_RS_TEST_BIN/sai-driver/driver-scripts)",
            sources.err().unwrap_or_default()
        ));
        return None;
    };
    let exe = match (prebuilt, &sources) {
        (Some(exe), _) => exe,
        (None, Ok(s)) => {
            let Some(inc) = var("SAI_INC").map(PathBuf::from) else {
                summary("sai-driver differential: live leg SKIPPED: the C++ sources are there but SAI_INC is not set");
                return None;
            };
            let cxx = var("CXX").unwrap_or_else(|| "c++".to_owned());
            let cc = var("CC").unwrap_or_else(|| "cc".to_owned());
            let dir = scratch().join("sai-driver-cpp").join(inputs_hash(s, &inc, &cxx, &cc));
            let exe = dir.join(cpp_build::DRIVER);
            if !exe.is_file() {
                let tc = cpp_build::Toolchain {
                    cxx: &cxx,
                    cc: &cc,
                    static_runtime: false,
                };
                cpp_build::build(s, &inc, &tc, &dir).unwrap_or_else(|e| panic!("building the C++ driver: {e}"));
            }
            exe
        }
        (None, Err(_)) => {
            summary(
                "sai-driver differential: live leg SKIPPED: scripts but no C++ driver (SAI_DRIVER_CPP, $SAI_RS_TEST_BIN)",
            );
            return None;
        }
    };
    let tables = sources.as_ref().ok().and_then(|s| {
        let cxx = var("CXX").unwrap_or_else(|| "c++".to_owned());
        match cpp_build::tables(&s.tests, &scratch().join("sai-driver-extra"), &cxx) {
            Ok(t) => Some(t),
            Err(why) => {
                summary(&format!(
                    "sai-driver differential: the scripts that need the C++ tables SKIPPED in the live leg ({why})"
                ));
                None
            }
        }
    });
    if tables.is_none() && sources.is_err() {
        summary(
            "sai-driver differential: the scripts that need the C++ tables SKIPPED in the live leg (no C++ frontend tests dir)",
        );
    }
    let mut scripts = cpp_build::scripts(&scripts_dir).unwrap();
    assert!(!scripts.is_empty(), "no *.ops in {}", scripts_dir.display());
    // a script that names a table token runs only where the tables are: with them replaced, in the scratch area
    let mut runnable = Vec::new();
    for script in scripts.drain(..) {
        let text = std::fs::read_to_string(&script).unwrap();
        if !text.contains(EXT_PHY_TOKEN) && !text.contains(EXT_MACSEC_TOKEN) {
            runnable.push(script);
        } else if let Some(t) = &tables {
            let to = scratch().join("sai-driver-extra").join(script.file_name().unwrap());
            std::fs::write(&to, with_tables(&text, t)).unwrap();
            runnable.push(to);
        }
    }
    Some(Setup {
        exe,
        scripts: runnable,
        allow,
        tables,
    })
}

/// The live C++ driver on every script, once for all tests of this binary.
fn live() -> Option<&'static Live> {
    static LIVE: OnceLock<Option<Live>> = OnceLock::new();
    LIVE.get_or_init(|| {
        let setup = setup()?;
        let dir = scratch();
        let mut runs = Vec::new();
        for script in &setup.scripts {
            let stem = script.file_stem().unwrap().to_string_lossy().into_owned();
            let out = Command::new(&setup.exe)
                .arg(script)
                .current_dir(&dir)
                .output()
                .unwrap_or_else(|e| panic!("{}: {e}", setup.exe.display()));
            assert!(
                out.status.success(),
                "C++ driver on {stem}: {}\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            runs.push(Run {
                stem,
                text: std::fs::read_to_string(script).unwrap(),
                cpp: String::from_utf8(out.stdout).unwrap(),
            });
        }
        Some(Live { setup, runs })
    })
    .as_ref()
}

/// The proof that the golden is the C++'s: the live C++ driver's output, with the table paths written
/// as tokens, is byte-equal to the golden section of every script it ran, and the tables it printed
/// are byte-equal to the golden's.
#[test]
fn live_cpp_output_equals_the_golden() {
    let Some(live) = live() else { return };
    let golden = golden_cpp(CPP_GOLDEN);
    let mut failures = String::new();
    for run in &live.runs {
        let normalized = live
            .setup
            .tables
            .as_ref()
            .map_or_else(|| run.cpp.clone(), |t| tokens_back(&run.cpp, t));
        match golden.iter().find(|(s, _)| *s == run.stem) {
            Some((_, text)) if *text == normalized => {}
            Some((_, text)) => {
                let _ = writeln!(
                    failures,
                    "{}: the live C++ output differs from the golden ({} bytes live, {} golden)",
                    run.stem,
                    normalized.len(),
                    text.len()
                );
            }
            None => {
                let _ = writeln!(failures, "{}: no golden C++ output", run.stem);
            }
        }
    }
    if let Some((macsec, ext_phy)) = &live.setup.tables {
        let (_, tables) = golden_inputs(INPUTS_GOLDEN);
        for (name, path) in [("ext-macsec.tbl", macsec), ("ext-phy.tbl", ext_phy)] {
            if std::fs::read_to_string(path).unwrap() != tables[name] {
                let _ = writeln!(failures, "table {name}: the live C++ table differs from the golden's");
            }
        }
    }
    let (golden_scripts, _) = golden_inputs(INPUTS_GOLDEN);
    for run in &live.runs {
        // the script as run is the golden's input with the tokens replaced
        if let (Some(t), Some((_, text))) = (&live.setup.tables, golden_scripts.iter().find(|(s, _)| *s == run.stem)) {
            if run.text != with_tables(text, t) {
                let _ = writeln!(
                    failures,
                    "{}: the script run live differs from the golden's input",
                    run.stem
                );
            }
        }
    }
    assert!(failures.is_empty(), "{failures}");
    assert!(!live.runs.is_empty());
    summary(&format!(
        "sai-driver differential: live C++ output equals the golden: {} of {} scripts byte-equal{}",
        live.runs.len(),
        golden.len(),
        if live.setup.tables.is_some() {
            ", tables equal"
        } else {
            ""
        }
    ));
}

#[test]
fn cpp_and_rust_drivers_agree_but_for_the_allow_list() {
    let Some(live) = live() else { return };
    let setup = &live.setup;
    let mut diffs: Vec<Diff> = Vec::new();
    let mut per_script: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // (blocks, differing)
    let mut all_ops: Vec<(String, String)> = Vec::new(); // (script, op)
    for run in &live.runs {
        let stem = &run.stem;
        let rust = Driver::new(options()).run_script(&run.text);
        let c = compare::compare(stem, &run.cpp, &rust).unwrap_or_else(|e| panic!("{e}"));
        per_script.insert(stem.clone(), (c.blocks, c.diffs.len()));
        all_ops.extend(compare::blocks(&run.cpp).into_iter().map(|b| (stem.clone(), b.op)));
        diffs.extend(c.diffs);
    }
    let total: usize = per_script.values().map(|(b, _)| b).sum();

    if var("SAI_DRIVER_ALLOW_UPDATE").as_deref() == Some("1") {
        let old = std::fs::read_to_string(&setup.allow).unwrap_or_default();
        let entries = compare::parse_allow(&old).unwrap_or_default();
        std::fs::write(&setup.allow, compare::regenerate(&entries, &diffs)).unwrap();
        summary(&format!(
            "sai-driver differential: allow-list REWRITTEN ({}): {} ops, {} identical, {} entries",
            setup.allow.display(),
            total,
            total - diffs.len(),
            diffs.len()
        ));
        return;
    }

    let text = std::fs::read_to_string(&setup.allow).unwrap_or_else(|e| {
        panic!(
            "the C++ sources are here, but the allow-list {} is not readable ({e}); SAI_DRIVER_ALLOW_UPDATE=1 writes one",
            setup.allow.display()
        )
    });
    let entries = compare::parse_allow(&text).unwrap_or_else(|e| panic!("{}: {e}", setup.allow.display()));
    let verdict = compare::judge(&entries, &diffs);

    // the burn-down numbers
    summary(&format!(
        "sai-driver differential: {} scripts, {total} ops, {} identical, {} differ (allowed {}, NOT allowed {}), {} allow-list entries ({} stale)",
        live.runs.len(),
        total - diffs.len(),
        diffs.len(),
        diffs.len() - verdict.uncovered().len(),
        verdict.uncovered().len(),
        entries.len(),
        verdict.stale().len()
    ));
    for (script, (blocks, differing)) in &per_script {
        summary(&format!(
            "  {script}: {blocks} ops, {} identical, {differing} differ",
            blocks - differing
        ));
    }
    let ops_per_area = compare::tally(
        all_ops
            .iter()
            .map(|(_, op)| compare::area(op))
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    );
    let diff_areas: Vec<String> = diffs.iter().map(|d| compare::area(&d.op)).collect();
    let diffs_per_area = compare::tally(diff_areas.iter().map(String::as_str));
    let areas: Vec<String> = ops_per_area
        .iter()
        .map(|(a, n)| format!("{a} {}/{n}", diffs_per_area.get(a).copied().unwrap_or(0)))
        .collect();
    summary(&format!("  differing/ops per area: {}", areas.join(", ")));
    for (i, e) in entries.iter().enumerate() {
        let changed: Vec<String> = diffs
            .iter()
            .zip(&verdict.covered_by)
            .filter(|(_, c)| **c == Some(i))
            .map(|(d, _)| d.changed_lines().to_string())
            .collect();
        eprintln!(
            "  entry line {}: {} block(s) [changed lines {}]: {}",
            e.line,
            verdict.uses.get(i).copied().unwrap_or(0),
            changed.join(","),
            e.reason
        );
    }

    let failures = failure_text(&entries, &verdict, &diffs);
    assert!(failures.is_empty(), "{failures}");
}

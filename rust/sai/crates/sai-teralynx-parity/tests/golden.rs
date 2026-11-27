//! The golden C++ side (tests/golden): its integrity, its inputs, and the negative controls of its
//! comparisons. The goldens are embedded (`include_str!`), so this runs without the C++ (deleted in H5 on
//! 2026-11-27). One more test runs the harness itself when the fixtures and programs are there
//! (`SAI_PARITY_FIXTURES`, `SAI_PARITY_RUST_LIB`, `SAI_PARITY_TEST_IFCS`, `SAI_PARITY_SAI_SCRIPT`;
//! nix/teralynx-parity.nix does the same with the program): the Rust side passes against the golden strictly
//! and fails against a changed one.
#![forbid(unsafe_code)]

use sai_driver::compare;
use sai_teralynx_parity::golden::{self, DriverGolden, Golden, HEAD, IfcsGolden, OracleGolden, RefBlock, RefLines};
use sai_teralynx_parity::run::{self, Config, Part};
use sai_teralynx_parity::sha256;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const IFCS: &str = include_str!("golden/test_ifcs-cpp.txt");
const DRIVER: &str = include_str!("golden/driver-cpp.txt");
const ORACLE: &str = include_str!("golden/oracle-cpp.txt");
const SOURCES: &str = include_str!("golden/cpp-sources-sha256.txt");

fn manifest(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// The text with one hex digit of the first sha256 field after the line starting with `tag` (and
/// containing `needle`) changed: a golden with a changed byte.
fn flip(text: &str, tag: &str, needle: &str) -> String {
    let mut done = false;
    text.lines()
        .map(|l| {
            if done || !l.starts_with(tag) || !l.contains(needle) {
                return l.to_owned();
            }
            let mut f: Vec<String> = l.split('\t').map(str::to_owned).collect();
            let hit = f
                .iter_mut()
                .find(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit()));
            if let Some(h) = hit {
                let first = if h.starts_with('0') { "1" } else { "0" };
                h.replace_range(0..1, first);
                done = true;
            }
            f.join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn the_goldens_parse_and_hold_what_the_readme_counts() {
    let i = IfcsGolden::parse(IFCS).unwrap();
    assert_eq!(
        (i.status.as_str(), i.stdout_last.as_str()),
        ("exit 0", "test_ifcs: 1047 checks, 0 failed")
    );
    assert_eq!(i.counts.iter().map(|c| c.checks).sum::<u32>(), 1047);
    assert!(i.counts.iter().all(|c| c.failed == 0 && !c.incomplete));
    assert_eq!(i.sections.len(), 17, "test_ifcs sections of the stream");
    assert_eq!(i.bytes, 3766, "@B lines of the test_ifcs stream");
    let d = DriverGolden::parse(DRIVER).unwrap();
    assert_eq!(d.scripts.len(), 9);
    assert_eq!(
        d.scripts.iter().map(|s| s.blocks.len()).sum::<usize>(),
        385,
        "driver ops with the profile lines"
    );
    assert_eq!(
        d.scripts.iter().map(|s| s.bytes.lines).sum::<usize>(),
        3999,
        "driver @B lines"
    );
    assert!(d.scripts.iter().all(|s| s.status == "exit 0"));
    let o = OracleGolden::parse(ORACLE).unwrap();
    assert_eq!(o.scripts.len(), 27);
    // the records are what render() writes: the live check compares them as text
    assert_eq!(i.render(), golden::data_lines(IFCS));
    assert_eq!(d.render(), golden::data_lines(DRIVER));
    assert_eq!(o.render(), golden::data_lines(ORACLE));
}

#[test]
fn the_headers_cite_the_sources_list_and_the_list_the_files() {
    let sha = sha256::hex(SOURCES.as_bytes());
    for (name, text) in [("test_ifcs", IFCS), ("driver", DRIVER), ("oracle", ORACLE)] {
        assert!(
            text.lines()
                .any(|l| l.starts_with('#') && l.contains(&sha) && l.contains("cpp-sources-sha256.txt")),
            "{name}: the header does not cite the sha256 {sha} of cpp-sources-sha256.txt"
        );
        assert!(text.contains("NOT to be regenerated from the Rust output"), "{name}");
    }
    // every listed file that is still in the checkout is the one the capture saw (once the C++ is
    // deleted, the list is the record: nothing to compare)
    let root = manifest("../../../..");
    let (mut seen, mut missing) = (0, 0);
    for l in SOURCES.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let (want, path) = l.split_once("  ").unwrap();
        match std::fs::read(root.join(path)) {
            Ok(b) => {
                assert_eq!(
                    sha256::hex(&b),
                    want,
                    "{path} changed since the golden C++ side was captured"
                );
                seen += 1;
            }
            Err(_) => missing += 1,
        }
    }
    eprintln!("cpp-sources-sha256.txt: {seen} files checked, {missing} not in this tree");
}

#[test]
fn the_inputs_are_the_ones_the_capture_ran() {
    let d = DriverGolden::parse(DRIVER).unwrap();
    for s in &d.scripts {
        let p = manifest(&format!("../sai-driver/tests/driver-scripts/{}.ops", s.name));
        let text = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert_eq!(sha256::hex(&text), s.input, "driver script {} changed", s.name);
    }
    // the oracle scripts live in ai_re/nix/packages/ifcs-oracle: checked where the tree has them
    let o = OracleGolden::parse(ORACLE).unwrap();
    let dir = manifest("../../../../ai_re/nix/packages/ifcs-oracle/scripts");
    if dir.is_dir() {
        for s in &o.scripts {
            let p = dir.join(format!("{}.json", s.name));
            let text = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert_eq!(sha256::hex(&text), s.input, "oracle script {} changed", s.name);
        }
    }
}

/// A small unit's lines are its head: the digest must equal the digest of those lines (the golden is
/// consistent with itself), and a changed digest or a changed line must not match.
#[test]
fn the_digests_hold_and_a_changed_byte_is_reported() {
    let i = IfcsGolden::parse(IFCS).unwrap();
    let o = OracleGolden::parse(ORACLE).unwrap();
    // units of up to HEAD lines (each shorter than the cap) are stored whole
    let mut whole = 0;
    let units = i.sections.iter().chain(o.scripts.iter().flat_map(|s| s.steps.iter()));
    for u in units {
        let RefLines::Digest(d) = &u.lines else {
            panic!("a kept unit")
        };
        if d.lines <= HEAD && d.head.iter().all(|l| l.chars().count() < golden::CAP) {
            assert!(
                u.lines.matches(&d.head),
                "{}: the digest is not the digest of its stored lines",
                u.name
            );
            whole += 1;
        }
    }
    assert!(whole > 500, "{whole} units stored whole");
    // negative control: the empty section's digest changed -> the same (empty) lines no longer match
    let name = "switch_init";
    let sec = i.sections.iter().find(|u| u.name == name).unwrap();
    assert!(sec.lines.matches(&[]));
    let bad = IfcsGolden::parse(&flip(&golden::data_lines(IFCS), "section", name)).unwrap();
    let bsec = bad.sections.iter().find(|u| u.name == name).unwrap();
    assert!(
        !bsec.lines.matches(&[]),
        "a changed digest in the golden must make the comparison fail"
    );
    assert!(!golden::diff_ref(&bsec.lines, &[]).raw_equal);
    assert!(!sec.lines.matches(&["x".to_owned()]), "a line the golden does not have");
    // the changed record renders differently (the parse is lossless: the digest is what was read)
    assert_ne!(golden::data_lines(IFCS), bad.render());
}

/// The driver's comparison, on small blocks stored whole (op line + answer): equal; one changed digest
/// in the golden or one changed answer line -> a differing block named by its op.
#[test]
fn the_driver_comparison_reports_a_changed_block() {
    let d = DriverGolden::parse(DRIVER).unwrap();
    let script = d.scripts.first().unwrap();
    // the first blocks with all their lines stored
    let take: Vec<&RefBlock> = script
        .blocks
        .iter()
        .take_while(|b| b.d.lines <= HEAD && b.d.head.iter().all(|l| l.chars().count() < golden::CAP))
        .collect();
    assert!(take.len() >= 5, "{} small blocks first", take.len());
    let blocks: Vec<RefBlock> = take.iter().map(|b| (*b).clone()).collect();
    let stdout: String = blocks
        .iter()
        .flat_map(|b| b.d.head.iter())
        .fold(String::new(), |mut s, l| {
            s.push_str(l);
            s.push('\n');
            s
        });
    assert_eq!(compare::blocks(&stdout).len(), blocks.len());
    let ok = golden::compare_blocks(&script.name, &blocks, &stdout).unwrap();
    assert!(ok.diffs.is_empty(), "{:?}", ok.diffs);
    // a changed answer line (the Rust side)
    let changed = stdout.replacen("= SUCCESS", "= FAILURE", 1);
    let c = golden::compare_blocks(&script.name, &blocks, &changed).unwrap();
    assert_eq!(c.diffs.len(), 1, "{:?}", c.diffs);
    assert!(c.diffs[0].first.contains("= SUCCESS") && c.diffs[0].first.contains("= FAILURE"));
    // a changed digest (the golden)
    let one = &blocks[1];
    let text = flip(&golden::data_lines(DRIVER), "block", &one.op);
    let bad = DriverGolden::parse(&text).unwrap();
    let changed_blocks: Vec<RefBlock> = bad.scripts[0].blocks.iter().take(blocks.len()).cloned().collect();
    let c = golden::compare_blocks(&script.name, &changed_blocks, &stdout).unwrap();
    assert_eq!(
        c.diffs.iter().map(|d| d.op.as_str()).collect::<Vec<_>>(),
        [one.op.as_str()]
    );
    // the echo lines stay a hard failure
    assert!(golden::compare_blocks(&script.name, &blocks, &stdout.replacen("> ", "> x", 1)).is_err());
}

fn harness_config(golden_dir: PathBuf, fixtures: &str, work: &str) -> Config {
    let env = |k: &str| PathBuf::from(std::env::var(k).unwrap_or_default());
    Config {
        golden: golden_dir,
        fixtures: fixtures.into(),
        work: work.into(),
        rust_lib: env("SAI_PARITY_RUST_LIB"),
        sai_script: env("SAI_PARITY_SAI_SCRIPT"),
        test_ifcs: env("SAI_PARITY_TEST_IFCS"),
        areas: manifest("../sai-backend-teralynx/tests/areas.txt"),
        expected: manifest("../sai-backend-teralynx/tests/test_ifcs-expected.txt"),
        allow: manifest("tests/parity-allow.txt"),
        pending: manifest("tests/parity-pending.txt"),
        driver_scripts: manifest("../sai-driver/tests/driver-scripts"),
        oracle_scripts: Some(manifest("../../../../ai_re/nix/packages/ifcs-oracle/scripts")),
        strict: true,
        parts: Part::DEFAULT.into_iter().collect::<BTreeSet<_>>(),
        only: Vec::new(),
        mutate: None,
        timeout: Duration::from_secs(600),
    }
}

fn fixtures() -> Option<(String, String)> {
    let fx = std::env::var("SAI_PARITY_FIXTURES").ok()?;
    let work = std::env::var("SAI_PARITY_WORK").unwrap_or_else(|_| "/tmp/sai-parity-golden-test".to_owned());
    Some((fx, work))
}

/// The Rust libsai against the golden alone: strictly green; against a golden with one changed
/// digest per file: red, naming the units.
#[test]
fn the_rust_side_against_the_golden_and_a_changed_one() {
    let programs = ["SAI_PARITY_RUST_LIB", "SAI_PARITY_TEST_IFCS", "SAI_PARITY_SAI_SCRIPT"];
    let (Some((fx, work)), true) = (fixtures(), programs.iter().all(|k| std::env::var_os(k).is_some())) else {
        eprintln!("skipped: no SAI_PARITY_FIXTURES / {}", programs.join(" / "));
        return;
    };
    let dir = manifest("tests/golden");
    let o = run::run(&harness_config(dir.clone(), &fx, &format!("{work}-rust"))).unwrap();
    assert!(o.red.is_empty(), "{}", o.report);
    assert_eq!(o.gate, Some((1047, 1047)));
    // the changed copy
    let bad = PathBuf::from(format!("{work}-bad-golden"));
    std::fs::create_dir_all(&bad).unwrap();
    let w = |f: &str, t: String| std::fs::write(bad.join(f), t).unwrap();
    w(golden::IFCS_FILE, flip(IFCS, "section", "\tl2"));
    w(golden::DRIVER_FILE, flip(DRIVER, "block", "set switch"));
    w(golden::ORACLE_FILE, flip(ORACLE, "step", "create L3 table (aclorch)"));
    assert!(Golden::load(&bad).is_ok());
    let o = run::run(&harness_config(bad, &fx, &format!("{work}-rust2"))).unwrap();
    let text = o.red.join("\n");
    assert!(text.contains("test_ifcs section l2"), "{text}");
    // (the driver part has no program since H5: its changed digest is not run)
    assert!(
        text.contains("oracle acl step '0.3 create L3 table (aclorch)'"),
        "{text}"
    );
}

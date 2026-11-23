//! The C++-vs-Rust frontend differential (live: the C++ frontend is frozen since 2026-11-23 in
//! `ai_re/nix/packages/sai-teralynx/frontend`, which the CS8210's libsai compiles): every op
//! script of its `tests/driver-scripts/*.ops` runs through the C++ `frontend_driver` (the C++ libsai on its fake
//! backend) and through this crate's driver (the Rust core on its virtual backend); the outputs are
//! compared op block by op block (`sai_driver::compare`). Every differing block must be covered by
//! an entry of `tests/differential-allow.txt`, and every entry must cover a block.
//!
//! The C++ driver:
//! - `SAI_DRIVER_CPP`: a prebuilt `frontend_driver` (a Nix check passes one in); or
//! - `$SAI_RS_TEST_BIN/sai-cpp-frontend-driver`, with the scripts and the allow-list in
//!   `$SAI_RS_TEST_BIN/sai-driver/` (prebuilt by `build.rs` for a target without a compiler); or
//! - built here from `SAI_OPENBCM_TESTS` (the frozen frontend's `tests`; else `$SAI_OPENBCM_SRC/../tests`)
//!   and `SAI_INC`, with `$CXX` / `$CC` (cached in the target's temp directory by a hash of the inputs).
//!
//! Besides the frontend's scripts, this crate's own `tests/driver-scripts/*.ops` (the external-PHY
//! runtime and the `MACsec` objects, with key material) run through both drivers on the tables of
//! the frontend's `tests/test_tables.h` ([`extra_scripts`]).
//!
//! Without the C++ sources (the sai-rs package: `SAI_OPENBCM_SRC` is a copy of `src` only) the
//! comparison is SKIPPED, said on stderr and in `$SAI_RS_TEST_SUMMARY`. With them, a missing
//! allow-list fails. `SAI_DRIVER_ALLOW` names another allow-list; `SAI_DRIVER_ALLOW_UPDATE=1`
//! rewrites it from today's differences (reasons kept where an entry still covers) and passes.
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

struct Setup {
    exe: PathBuf,
    scripts: Vec<PathBuf>,
    allow: PathBuf,
}

/// The token of an extra script that names a table file the test writes.
const EXT_PHY_TOKEN: &str = "@EXT_PHY_TABLE@";
const EXT_MACSEC_TOKEN: &str = "@EXT_MACSEC_TABLE@";

/// This crate's own scripts (`tests/driver-scripts`: the external-PHY runtime and the `MACsec`
/// objects, which the frontend's scripts do not reach), with their table tokens replaced by files
/// written into `dir`: the tables of the frontend's `tests/test_tables.h` (`kExtPhyGood`, without
/// its hold-off so nothing depends on the clock; `ms_table()`), printed by a three-line C++
/// program built with `$CXX`. Without the C++ test sources (the prebuilt bundle) or a compiler,
/// none (said in the summary).
fn extra_scripts(sources: &Result<cpp_build::Sources, String>, dir: &Path) -> Vec<PathBuf> {
    let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/driver-scripts");
    let Ok(s) = sources else {
        summary("sai-driver differential: own scripts SKIPPED (no C++ frontend tests dir for the tables)");
        return Vec::new();
    };
    let dir = dir.join("sai-driver-extra");
    std::fs::create_dir_all(&dir).unwrap();
    let printer = dir.join("print-tables.cpp");
    std::fs::write(
        &printer,
        "#include \"test_tables.h\"\n#include <fstream>\nint main(int, char** argv) {\n    std::ofstream(argv[1]) << ms_table();\n    std::ofstream(argv[2]) << kExtPhyGood;\n}\n",
    )
    .unwrap();
    let exe = dir.join("print-tables");
    let cxx = var("CXX").unwrap_or_else(|| "c++".to_owned());
    let mut words = cxx.split_whitespace();
    let built = Command::new(words.next().unwrap_or("c++"))
        .args(words)
        .arg("-std=c++17")
        .arg("-I")
        .arg(&s.tests)
        .arg(&printer)
        .arg("-o")
        .arg(&exe)
        .status()
        .is_ok_and(|st| st.success());
    let (macsec, ext_phy) = (dir.join("ext-macsec.tbl"), dir.join("ext-phy.tbl"));
    if !built
        || !Command::new(&exe)
            .arg(&macsec)
            .arg(&ext_phy)
            .status()
            .is_ok_and(|st| st.success())
    {
        summary(&format!(
            "sai-driver differential: own scripts SKIPPED (cannot build {} with {cxx})",
            printer.display()
        ));
        return Vec::new();
    }
    let phy = std::fs::read_to_string(&ext_phy)
        .unwrap()
        .replace("holdoff-ms 50\n", "holdoff-ms 0\n");
    std::fs::write(&ext_phy, phy).unwrap();
    let mut out = Vec::new();
    for script in cpp_build::scripts(&own).unwrap() {
        let text = std::fs::read_to_string(&script)
            .unwrap()
            .replace(EXT_PHY_TOKEN, ext_phy.to_str().unwrap())
            .replace(EXT_MACSEC_TOKEN, macsec.to_str().unwrap());
        let to = dir.join(script.file_name().unwrap());
        std::fs::write(&to, text).unwrap();
        out.push(to);
    }
    out
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

fn setup() -> Option<Setup> {
    let manifest_allow = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/differential-allow.txt");
    let bin_data = var("SAI_RS_TEST_BIN").map(|b| Path::new(&b).join("sai-driver"));
    let sources = cpp_build::find_sources();
    // the scripts: the sources' tests dir, else the prebuilt bundle's copy
    let scripts_dir = match (&sources, &bin_data) {
        (Ok(s), _) => Some(s.tests.join("driver-scripts")),
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
            "sai-driver differential: SKIPPED: no op scripts ({}; no $SAI_RS_TEST_BIN/sai-driver/driver-scripts)",
            sources.err().unwrap_or_default()
        ));
        return None;
    };
    let exe = match (prebuilt, &sources) {
        (Some(exe), _) => exe,
        (None, Ok(s)) => {
            let Some(inc) = var("SAI_INC").map(PathBuf::from) else {
                summary("sai-driver differential: SKIPPED: the C++ sources are there but SAI_INC is not set");
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
            summary("sai-driver differential: SKIPPED: scripts but no C++ driver (SAI_DRIVER_CPP, $SAI_RS_TEST_BIN)");
            return None;
        }
    };
    let mut scripts = cpp_build::scripts(&scripts_dir).unwrap();
    assert!(!scripts.is_empty(), "no *.ops in {}", scripts_dir.display());
    scripts.extend(extra_scripts(&sources, &scratch()));
    Some(Setup { exe, scripts, allow })
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

#[test]
fn cpp_and_rust_drivers_agree_but_for_the_allow_list() {
    let Some(setup) = setup() else { return };
    let dir = scratch();
    let mut diffs: Vec<Diff> = Vec::new();
    let mut per_script: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // (blocks, differing)
    let mut all_ops: Vec<(String, String)> = Vec::new(); // (script, op)
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
        let cpp = String::from_utf8(out.stdout).unwrap();
        let text = std::fs::read_to_string(script).unwrap();
        let rust = Driver::new(options()).run_script(&text);
        let c = compare::compare(&stem, &cpp, &rust).unwrap_or_else(|e| panic!("{e}"));
        per_script.insert(stem.clone(), (c.blocks, c.diffs.len()));
        all_ops.extend(compare::blocks(&cpp).into_iter().map(|b| (stem.clone(), b.op)));
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
        setup.scripts.len(),
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

    let mut failures = String::new();
    for i in verdict.uncovered() {
        let d = &diffs[i];
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
        let e = &entries[i];
        let _ = writeln!(
            failures,
            "STALE allow-list entry (line {}): {}\t{}\t{}",
            e.line,
            e.script,
            e.pattern.as_str(),
            e.reason
        );
    }
    assert!(failures.is_empty(), "{failures}");
}

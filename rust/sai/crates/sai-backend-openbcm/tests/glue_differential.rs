//! The glue-call differential (gate G2): does the Rust stack tell the SDK what the C++ stack
//! told it? Since the C++ is deleted, against GOLDEN files captured from it at commit 50a6aa7
//! (2026-11-23; how: their headers):
//!
//! * `tests/golden/glue-differential-inputs.txt`: the 12 op scripts (sai-openbcm's frontend
//!   scripts 01-09, sai-driver's `10-ext-phy` / `11-macsec`, this crate's
//!   `tests/glue-scripts/12-glue-extras.ops`) and the two table files of sai-openbcm's
//!   `tests/test_tables.h` that 10 and 11 read, as the goldens were captured from them;
//! * `tests/golden/glue-differential-cpp.txt`: per script, the output of sai-openbcm's
//!   `frontend_driver.cpp` and C++ library on the UNMODIFIED `openbcm_backend.cpp` over a
//!   recording stub of the 75 functions of `sdk_glue.h` that answered by replaying the Rust run's
//!   recording (so: the C++ calls given the Rust fake's answers at capture time).
//!
//! Each script runs through the Rust stack: sai-driver's [`Driver`] over sai-core on
//! [`OpenbcmBackend`] over a Rust fake chip ([`Chip`]) that records every glue call, callback
//! and injection (`sdk_glue::record`, the line format). The output prints, per operation, the
//! glue lines (`  glue ...`) between the op's echo and its status, then the status, values,
//! backend call log (`be`) and notifications; it is compared with the golden C++ output op block
//! by op block (`sai_driver::compare`), the scratch paths written `@RUN@` / `@TABLES@` as in the
//! golden. Every differing block must be covered by an entry of `tests/glue-differential-allow.txt`
//! (script, regex on the op, reason), and every entry must cover a block. Each script starts with
//! `profile SAI_INIT_CONFIG_FILE=<scenario>`: a fake-chip scenario with the script's port count,
//! lanes and speed (the virtual backend's `SAI_OPENBCM_FAKE_*` keys, default 8 x 4 lanes x 100G),
//! link following admin state, and the C++ fake's eight temperature monitors.
//!
//! The goldens, the allow-list and the scripts still in this tree are compiled in: the test needs
//! no file and no compiler at run time (it runs on the switch). A script in this tree, or a moved
//! copy of 01-09 found beside this checkout, that differs from the golden's copy fails the test
//! ("script changed since the golden"). The goldens are never regenerated from the Rust output: a
//! deliberate behaviour change edits the golden by hand or adds an allow-list entry, with a reason.
//! `SAI_GLUE_DIFF_KEEP=<dir>` keeps the golden C++ and the Rust output per script there.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test code: a failed unwrap is a failed test"
)]

use sai_backend_openbcm::OpenbcmBackend;
use sai_core::backend::HwPort;
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, MdioDev, PhyAddr, PortStat};
use sai_driver::compare::{self, Diff, Entry};
use sai_driver::{Driver, Feature, Hooks, OnHardware, Options};
use sdk_glue::rust_fake::Chip;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The prefix of a glue line in both outputs.
const GLUE: &str = "  glue ";

/// The scripts and tables the C++ outputs were captured from (see its header).
const INPUTS: &str = include_str!("golden/glue-differential-inputs.txt");
/// The C++ stack's output per script, captured at 50a6aa7 (see its header).
const CPP: &str = include_str!("golden/glue-differential-cpp.txt");
/// The allow-list.
const ALLOW: &str = include_str!("glue-differential-allow.txt");
/// The scripts that stay in this tree, as they are now: each must equal the golden's copy.
const IN_TREE: [(&str, &str); 3] = [
    (
        "10-ext-phy",
        include_str!("../../sai-driver/tests/driver-scripts/10-ext-phy.ops"),
    ),
    (
        "11-macsec",
        include_str!("../../sai-driver/tests/driver-scripts/11-macsec.ops"),
    ),
    ("12-glue-extras", include_str!("glue-scripts/12-glue-extras.ops")),
];
/// Where scripts 01-09 live (H5-1: they moved from the C++ frontend's tests to sai-driver's
/// `tests/driver-scripts`, unchanged; the C++ frontend was deleted in H5 on 2026-11-27), relative to this
/// crate's directory; compared with the golden, and a missing directory fails.
const MOVED: [&str; 1] = ["../sai-driver/tests/driver-scripts"];
/// The table files of the golden inputs and the script token each replaces.
const TABLES: [(&str, &str); 2] = [
    ("ext-phy.tbl", "@EXT_PHY_TABLE@"),
    ("ext-macsec.tbl", "@EXT_MACSEC_TABLE@"),
];

/// A line for whoever runs the suite: stderr, and `$SAI_RS_TEST_SUMMARY`.
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

fn scratch() -> PathBuf {
    var("SAI_RS_TEST_TMPDIR").map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
}

/// The Rust stack's hooks: the hardware backend on one recording fake chip; the injections are
/// the chip's test hooks, recorded as `inject ...` lines (the C++ test port prints the same).
struct GlueHooks {
    chip: Chip,
}

impl GlueHooks {
    fn inject(&self, line: &str, f: impl FnOnce(&Chip)) {
        self.chip.inject(line, || f(&self.chip));
    }
}

impl Hooks<OpenbcmBackend> for GlueHooks {
    fn backend(&mut self, selected: &str) -> Option<OpenbcmBackend> {
        (selected == "openbcm").then(|| OpenbcmBackend::on_chip(self.chip.clone()))
    }

    fn idle(&mut self) -> OpenbcmBackend {
        OpenbcmBackend::on_chip(self.chip.clone())
    }

    fn link(&mut self, _be: &mut OpenbcmBackend, dest: Dest, up: bool) {
        let port = dest.raw() as i32;
        self.inject(&format!("inject link(port={port}, up={})", i32::from(up)), |c| {
            c.link_event(port, up);
        });
    }

    fn linkscan(&mut self, _be: &mut OpenbcmBackend) {
        self.inject("inject linkscan()", |c| {
            c.linkscan();
        });
    }

    fn fdb(&mut self, _be: &mut OpenbcmBackend, event: FdbEvent) {
        let learned = i32::from(event.change == FdbChange::Learned);
        let mac = event.mac.0;
        let vid = i32::from(event.vid.get());
        let port = event.hw_port.raw() as i32;
        let text = mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":");
        self.inject(
            &format!("inject fdb(learned={learned}, mac={text}, vid={vid}, port={port})"),
            |c| {
                c.l2_event(Some(mac), vid, port, learned);
            },
        );
    }

    fn stat(&mut self, _be: &mut OpenbcmBackend, hw: HwPort, stat: PortStat, value: u64) {
        // sai-core's PortStat is in the C++ ob::PortStat order, the glue's `which`
        let which = stat as i32;
        let port = i32::from(hw.0);
        self.inject(
            &format!("inject stat(port={port}, which={which}, value={value})"),
            |c| {
                if let Some(s) = sdk_glue::PortStat::from_raw(which) {
                    c.set_counter(port, s, value);
                }
            },
        );
    }

    fn mdio(&mut self, _be: &mut OpenbcmBackend, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) {
        let dev = u32::from(dev.get());
        self.inject(
            &format!(
                "inject mdio(phy=0x{:x}, dev={dev}, reg=0x{reg:x}, val=0x{val:x})",
                phy.0
            ),
            |c| {
                c.set_mdio_reg(phy.0, dev, u32::from(reg), u32::from(val));
            },
        );
    }

    fn feature(&mut self, _be: &mut OpenbcmBackend, feature: Feature, on: bool) {
        // the hardware backend has no such switches: only the line (as the C++ test port)
        self.inject(
            &format!("inject feature(name=\"{}\", on={})", feature.word(), i32::from(on)),
            |_| {},
        );
    }

    fn lines(&mut self) -> Vec<String> {
        self.chip
            .take_record()
            .into_iter()
            .map(|l| format!("{}{l}", GLUE.trim_start()))
            .collect()
    }
}

/// The scenario of the fake chip for a script: its `SAI_OPENBCM_FAKE_PORTS` / `_LANES` / `_SPEED`.
fn scenario(script: &str) -> String {
    let (mut ports, mut lanes, mut speed) = (8, 4, 100_000);
    for line in script.lines() {
        let Some(kv) = line.trim().strip_prefix("profile ") else {
            continue;
        };
        let Some((k, v)) = kv.trim().split_once('=') else {
            continue;
        };
        let Ok(v) = v.trim().parse::<u32>() else { continue };
        match k {
            "SAI_OPENBCM_FAKE_PORTS" => ports = v,
            "SAI_OPENBCM_FAKE_LANES" => lanes = v,
            "SAI_OPENBCM_FAKE_SPEED" => speed = v,
            _ => {}
        }
    }
    format!(
        "# glue differential: the virtual backend's chip\nports {ports} {lanes} {speed}\nlink admin\ntemp 400 415 430 445 460 475 490 505\n"
    )
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

/// The glue lines of an output (without their prefix).
fn glue_lines(text: &str) -> Vec<&str> {
    text.lines().filter_map(|l| l.strip_prefix(GLUE)).collect()
}

/// The length of the longest common subsequence of two line lists (the glue lines of one op
/// that the two stacks share, in order).
fn common(a: &[&str], b: &[&str]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diag = 0;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if x == y { diag + 1 } else { up.max(row[j]) };
            diag = up;
        }
    }
    row[b.len()]
}

/// The sections of a golden: (head, text) per `@@@@ <head>` line, the text being every line
/// up to the next such line, exactly; the `#` header before the first is skipped.
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

/// A golden script: its stem, the file it was captured from, its text.
struct Script {
    stem: String,
    origin: String,
    text: String,
}

/// The golden inputs: the table files by name and the scripts in order.
fn inputs() -> (BTreeMap<String, String>, Vec<Script>) {
    let (mut tables, mut scripts) = (BTreeMap::new(), Vec::new());
    for (head, text) in sections(INPUTS) {
        match head.split_once(' ') {
            Some(("table", name)) => {
                tables.insert(name.to_owned(), text);
            }
            Some(("script", rest)) => {
                let (stem, origin) = rest.split_once(' ').unwrap_or((rest, ""));
                scripts.push(Script {
                    stem: stem.to_owned(),
                    origin: origin.to_owned(),
                    text,
                });
            }
            _ => panic!("glue-differential-inputs.txt: unknown section {head}"),
        }
    }
    (tables, scripts)
}

/// The golden C++ outputs by script stem, in order.
fn cpp_outputs() -> Vec<(String, String)> {
    sections(CPP)
        .into_iter()
        .map(|(head, text)| {
            let stem = head
                .strip_prefix("cpp ")
                .unwrap_or_else(|| panic!("glue-differential-cpp.txt: unknown section {head}"));
            (stem.to_owned(), text)
        })
        .collect()
}

/// Each golden script against its copy in this tree (compiled in) or beside it (the moved 01-09):
/// the failures, and a line about what was compared.
fn scripts_unchanged(scripts: &[Script]) -> (String, String) {
    let mut failures = String::new();
    let mut checked = Vec::new();
    for (stem, now) in IN_TREE {
        match scripts.iter().find(|s| s.stem == stem) {
            Some(s) if s.text == now => checked.push(stem.to_owned()),
            Some(s) => {
                let _ = writeln!(
                    failures,
                    "script changed since the golden: {stem} (captured from {}); the golden C++ output is of the captured text",
                    s.origin
                );
            }
            None => {
                let _ = writeln!(failures, "script {stem} of this tree is not in the golden inputs");
            }
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut moved_found = 0;
    for rel in MOVED {
        let dir = manifest.join(rel);
        if !dir.is_dir() {
            let _ = writeln!(
                failures,
                "script changed since the golden: the moved 01-09 are not in {}",
                dir.display()
            );
            continue;
        }
        moved_found += 1;
        for s in scripts
            .iter()
            .filter(|s| !IN_TREE.iter().any(|(stem, _)| *stem == s.stem))
        {
            let path = dir.join(format!("{}.ops", s.stem));
            match std::fs::read_to_string(&path) {
                Ok(now) if now == s.text => checked.push(format!("{}@{rel}", s.stem)),
                Ok(_) => {
                    let _ = writeln!(
                        failures,
                        "script changed since the golden: {} ({} differs from the copy captured from {})",
                        s.stem,
                        path.display(),
                        s.origin
                    );
                }
                Err(e) => {
                    let _ = writeln!(
                        failures,
                        "script changed since the golden: {} ({}: {e}; captured from {})",
                        s.stem,
                        path.display(),
                        s.origin
                    );
                }
            }
        }
    }
    assert!(moved_found > 0, "the moved scripts 01-09 were not checked");
    let note = format!("{} script copies equal to the golden's", checked.len());
    (failures, note)
}

/// An output with the scratch directories written as in the golden.
fn normalized(text: &str, runs: &Path, tables: &Path) -> String {
    text.replace(&runs.display().to_string(), "@RUN@")
        .replace(&tables.display().to_string(), "@TABLES@")
}

/// The differing blocks the allow-list does not cover, and the entries that cover none.
fn judged(entries: &[Entry], diffs: &[Diff]) -> String {
    let verdict = compare::judge(entries, diffs);
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
    failures
}

/// Glue lines of a Rust output: (all, in identical blocks, equal to a C++ glue line of their
/// block in order); `differing`: the indices of the blocks that differ.
fn glue_counts(cpp: &str, rust: &str, differing: &BTreeSet<usize>) -> (usize, usize, usize) {
    let (mut total, mut in_identical, mut matched) = (0, 0, 0);
    for (i, (rb, cb)) in compare::blocks(rust)
        .iter()
        .zip(compare::blocks(cpp).iter())
        .enumerate()
    {
        let glue_of =
            |b: &compare::Block| -> Vec<String> { b.lines.iter().filter(|l| l.starts_with(GLUE)).cloned().collect() };
        let (r, k) = (glue_of(rb), glue_of(cb));
        total += r.len();
        if differing.contains(&i) {
            let rr: Vec<&str> = r.iter().map(String::as_str).collect();
            let kk: Vec<&str> = k.iter().map(String::as_str).collect();
            matched += common(&rr, &kk);
        } else {
            in_identical += r.len();
            matched += r.len();
        }
    }
    (total, in_identical, matched)
}

#[test]
fn rust_and_cpp_backends_make_the_same_glue_calls_but_for_the_allow_list() {
    let (tables, scripts) = inputs();
    let cpp_all = cpp_outputs();
    assert_eq!(
        scripts.len(),
        12,
        "the golden inputs have the 12 scripts of the capture"
    );
    assert_eq!(
        scripts.iter().map(|s| s.stem.as_str()).collect::<Vec<_>>(),
        cpp_all.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        "the golden inputs and outputs name the same scripts"
    );
    let (mut failures, checked) = scripts_unchanged(&scripts);

    let runs = scratch().join("sai-glue-runs");
    let table_dir = scratch().join("sai-glue-tables");
    std::fs::create_dir_all(&runs).unwrap();
    std::fs::create_dir_all(&table_dir).unwrap();
    for (name, _) in TABLES {
        let text = tables
            .get(name)
            .unwrap_or_else(|| panic!("no table {name} in the golden inputs"));
        std::fs::write(table_dir.join(name), text).unwrap();
    }
    let keep = var("SAI_GLUE_DIFF_KEEP").map(PathBuf::from);
    let mut diffs: Vec<Diff> = Vec::new();
    let mut rust_outputs: Vec<(String, String)> = Vec::new();
    let mut per_script: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new(); // blocks, differing, glue rust, glue cpp
    let (mut glue_total, mut glue_in_identical, mut glue_matched) = (0usize, 0usize, 0usize);
    for (s, (_, cpp)) in scripts.iter().zip(&cpp_all) {
        let stem = &s.stem;
        let mut text = s.text.clone();
        for (name, token) in TABLES {
            text = text.replace(token, table_dir.join(name).to_str().unwrap());
        }
        let scen = runs.join(format!("{stem}.scenario"));
        std::fs::write(&scen, scenario(&text)).unwrap();
        let script = format!("profile SAI_INIT_CONFIG_FILE={}\n{text}", scen.display());

        // the Rust stack, recording
        let chip = Chip::new();
        chip.record(true);
        let rust = Driver::with_hooks(options(), Box::new(GlueHooks { chip })).run_script(&script);
        let rust = normalized(&rust, &runs, &table_dir);
        if let Some(k) = &keep {
            std::fs::create_dir_all(k).unwrap();
            std::fs::write(k.join(format!("{stem}.cpp.txt")), cpp).unwrap();
            std::fs::write(k.join(format!("{stem}.rust.txt")), &rust).unwrap();
        }
        let c = compare::compare(stem, cpp, &rust).unwrap_or_else(|e| panic!("{e}"));
        let differing: BTreeSet<usize> = c.diffs.iter().map(|d| d.index).collect();
        let (t, i, m) = glue_counts(cpp, &rust, &differing);
        glue_total += t;
        glue_in_identical += i;
        glue_matched += m;
        if m != t {
            let _ = writeln!(
                failures,
                "{stem}: {} Rust glue line(s) equal no C++ line of their op (in order); SAI_GLUE_DIFF_KEEP=<dir> keeps both outputs",
                t - m
            );
        }
        per_script.insert(
            stem.clone(),
            (c.blocks, c.diffs.len(), glue_lines(&rust).len(), glue_lines(cpp).len()),
        );
        diffs.extend(c.diffs);
        rust_outputs.push((stem.clone(), rust));
    }
    let total: usize = per_script.values().map(|v| v.0).sum();
    let entries = compare::parse_allow(ALLOW).unwrap_or_else(|e| panic!("tests/glue-differential-allow.txt: {e}"));

    let verdict = compare::judge(&entries, &diffs);
    summary(&format!(
        "sai-backend-openbcm glue differential (vs the C++ golden of 50a6aa7): {} scripts, {total} ops, {} identical, {} differ (allowed {}, NOT allowed {}), {} allow-list entries ({} stale); glue lines (Rust) {glue_total}: {glue_in_identical} in identical ops, {glue_matched} equal to a C++ line of the op (in order); {checked}",
        scripts.len(),
        total - diffs.len(),
        diffs.len(),
        diffs.len() - verdict.uncovered().len(),
        verdict.uncovered().len(),
        entries.len(),
        verdict.stale().len()
    ));
    for (script, (blocks, differing, glue_rust, glue_cpp)) in &per_script {
        summary(&format!(
            "  {script}: {blocks} ops, {} identical, {differing} differ; glue lines Rust {glue_rust}, C++ {glue_cpp}",
            blocks - differing
        ));
    }
    for (i, e) in entries.iter().enumerate() {
        eprintln!(
            "  entry line {}: {} block(s): {}",
            e.line,
            verdict.uses.get(i).copied().unwrap_or(0),
            e.reason
        );
    }
    failures.push_str(&judged(&entries, &diffs));
    assert!(failures.is_empty(), "{failures}");
    negative_control(&entries, &cpp_all, &rust_outputs);
}

/// The comparison can fail. A Rust output with one glue line changed (`(perturbed)` appended):
/// (1) in the first op of a script that no allow-list entry covers: a difference that is NOT
/// ALLOWED; (2) in an op an entry covers (the `switch` ops): a Rust glue line that equals no C++
/// line of its op.
fn negative_control(entries: &[Entry], cpp_all: &[(String, String)], rust_outputs: &[(String, String)]) {
    // the output with its first glue line in an op that `covered` says yes (or no) to changed
    let perturb = |rust: &str, stem: &str, want_covered: bool| -> Option<String> {
        let mut op_covered = None;
        let mut done = false;
        let mut out = String::new();
        for line in rust.split_inclusive('\n') {
            if let Some(op) = line.strip_prefix("> ") {
                let op = op.trim_end();
                op_covered = Some(entries.iter().any(|e| e.covers(stem, op)));
            }
            if !done && op_covered == Some(want_covered) && line.starts_with(GLUE) {
                out.push_str(line.trim_end());
                out.push_str(" (perturbed)\n");
                done = true;
            } else {
                out.push_str(line);
            }
        }
        done.then_some(out)
    };
    let ((stem, rust), (_, cpp)) = rust_outputs.iter().zip(cpp_all).next().expect("a script");
    let changed = perturb(rust, stem, false).expect("negative control: a glue line in an op no entry covers");
    let c = compare::compare(stem, cpp, &changed).unwrap_or_else(|e| panic!("{e}"));
    let verdict = compare::judge(entries, &c.diffs);
    assert!(
        verdict
            .uncovered()
            .iter()
            .any(|&i| c.diffs[i].rust.iter().any(|l| l.ends_with(" (perturbed)"))),
        "negative control on {stem}: a changed Rust glue line in an op no entry covers is not reported"
    );
    let matched_of = |rust: &str| {
        let c = compare::compare(stem, cpp, rust).unwrap_or_else(|e| panic!("{e}"));
        let differing: BTreeSet<usize> = c.diffs.iter().map(|d| d.index).collect();
        glue_counts(cpp, rust, &differing).2
    };
    let changed = perturb(rust, stem, true).expect("negative control: a glue line in an op an entry covers");
    assert_eq!(
        matched_of(&changed) + 1,
        matched_of(rust),
        "negative control on {stem}: a changed Rust glue line in an allowed op is not reported"
    );
    summary(&format!(
        "sai-backend-openbcm glue differential: negative control ok ({stem}: a changed Rust glue line is NOT ALLOWED in an op no entry covers, and unmatched in an allowed one)"
    ));
}

/// The recording itself: a line per call in the documented form, callbacks inside their call.
#[test]
fn the_chip_records_calls_callbacks_and_injections() {
    let dir = scratch().join("sai-glue-unit");
    std::fs::create_dir_all(&dir).unwrap();
    let scenario_file = dir.join("two.scenario");
    std::fs::write(&scenario_file, "ports 2 4 100000\nlink admin\n").unwrap();
    let chip = Chip::new();
    chip.record(true);
    let (mut sw, _) = sdk_glue::Switch::init_on(&chip, 0, &scenario_file, None).unwrap();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let s2 = seen.clone();
    sw.set_link_callback(move |_, _| {
        s2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })
    .unwrap();
    sw.port_enable_set(1, false).unwrap();
    sw.port_enable_set(1, true).unwrap();
    chip.inject("inject link(port=1, up=0)", || chip.link_event(1, false));
    assert_eq!(sw.port_speed_get(1).unwrap(), 100_000);
    drop(sw);
    let lines = chip.take_record();
    let path = scenario_file.display().to_string();
    let want = [
        format!(
            "init(unit=0, config=\"{path}\", rc=\"\", errlen=256) -> 0 err=\"ok: stale KNET netifs 0 filters 0 destroyed; IPv4/IPv6 L3 on 2 ports\""
        ),
        "register_callbacks(unit=0, link=set, l2=null, ctx=set) -> 0".to_owned(),
        // the link was down: no callback
        "port_enable_set(unit=0, port=1, enable=0) -> 0".to_owned(),
        "  cb link(port=1, up=1)".to_owned(),
        "port_enable_set(unit=0, port=1, enable=1) -> 0".to_owned(),
        "  cb link(port=1, up=0)".to_owned(),
        "inject link(port=1, up=0)".to_owned(),
        "port_speed_get(unit=0, port=1) -> 0 mbps=100000".to_owned(),
        // ob_sdk_shutdown alone: it unregisters the callbacks inside the glue (as the C++)
        "shutdown(unit=0) -> void".to_owned(),
    ];
    assert_eq!(lines, want, "{lines:#?}");
    assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 2);
}

//! The glue-call differential (gate G2): does the Rust stack tell the SDK what the C++ stack
//! tells it? Every op script of the frontend differential (sai-openbcm's `tests/driver-scripts`
//! and sai-driver's own, external PHY and `MACsec`) runs
//!
//! * through the Rust stack: sai-driver's [`Driver`] over sai-core on [`OpenbcmBackend`] over a
//!   Rust fake chip ([`Chip`]) that records every glue call, callback and injection
//!   (`sdk_glue::record`, the line format); and
//! * through the C++ stack: sai-openbcm's `frontend_driver.cpp` and its library on the
//!   UNMODIFIED `openbcm_backend.cpp`, over `tests/cpp/glue_stub.c`, a stub of all 75 functions
//!   of `sdk_glue.h` that prints the same line for the same call and answers with the Rust run's
//!   recording (replay: the C++ backend sees the answers the Rust backend saw; see the stub's
//!   header), with `tests/cpp/test_port_glue.cpp` as its test port.
//!
//! Both print, per operation, the glue lines (`  glue ...`) between the op's echo and its status,
//! then the status, values, backend call log (`be`) and notifications; the outputs are compared
//! op block by op block (`sai_driver::compare`). Every differing block must be covered by an
//! entry of `tests/glue-differential-allow.txt` (script, regex on the op, reason), and every entry
//! must cover a block. Each script starts with `profile SAI_INIT_CONFIG_FILE=<scenario>` (both
//! drivers): a fake-chip scenario with the script's port count, lanes and speed (the virtual
//! backend's `SAI_OPENBCM_FAKE_*` keys, default 8 x 4 lanes x 100G), link following admin state, and
//! the C++ fake's eight temperature monitors.
//!
//! The C++ driver: `SAI_GLUE_DIFF_CPP` (prebuilt), else `$SAI_RS_TEST_BIN/sai-cpp-glue-driver` with
//! the scripts and the allow-list in `$SAI_RS_TEST_BIN/sai-backend-openbcm/` (build.rs prebuilds
//! it for a cross build or with `SAI_GLUE_DIFF_PREBUILD=1`), else built here from
//! `SAI_OPENBCM_SRC` / `SAI_OPENBCM_TESTS` and `SAI_INC` with `$CXX` / `$CC` (cached by a hash of
//! the inputs). Without the C++ sources the comparison is SKIPPED (stderr and
//! `$SAI_RS_TEST_SUMMARY`). `SAI_GLUE_DIFF_ALLOW` names another allow-list;
//! `SAI_GLUE_DIFF_ALLOW_UPDATE=1` rewrites it from today's differences and passes;
//! `SAI_GLUE_DIFF_KEEP=<dir>` keeps both outputs per script there.
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

#[path = "../../sai-driver/src/cpp_build.rs"]
mod cpp_build;
#[path = "cpp/glue_build.rs"]
mod glue_build;

use sai_backend_openbcm::OpenbcmBackend;
use sai_core::backend::HwPort;
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, MdioDev, PhyAddr, PortStat};
use sai_driver::compare::{self, Diff};
use sai_driver::{Driver, Feature, Hooks, OnHardware, Options};
use sdk_glue::rust_fake::Chip;
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use glue_build::GLUE_DRIVER;
/// The prefix of a glue line in both outputs.
const GLUE: &str = "  glue ";

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

struct Setup {
    exe: PathBuf,
    /// (stem, script text with its tokens replaced)
    scripts: Vec<(String, String)>,
    allow: PathBuf,
}

/// A hash of everything the C++ build reads.
fn inputs_hash(s: &cpp_build::Sources, own: &Path, inc: &Path, cxx: &str, cc: &str) -> String {
    let mut h = DefaultHasher::new();
    (inc, cxx, cc).hash(&mut h);
    for dir in [&s.src, &s.tests, &own.to_path_buf()] {
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
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let own = manifest.join("tests/cpp");
    let bin_data = var("SAI_RS_TEST_BIN").map(|b| Path::new(&b).join("sai-backend-openbcm"));
    let sources = cpp_build::find_sources();
    let scripts_dir = match (&sources, &bin_data) {
        (Ok(s), _) => s.tests.join("driver-scripts"),
        (Err(_), Some(d)) if d.join("driver-scripts").is_dir() => d.join("driver-scripts"),
        (Err(why), _) => {
            summary(&format!(
                "sai-backend-openbcm glue differential: SKIPPED: no op scripts ({why}; no $SAI_RS_TEST_BIN/sai-backend-openbcm/driver-scripts)"
            ));
            return None;
        }
    };
    let allow = var("SAI_GLUE_DIFF_ALLOW").map_or_else(
        || match (&sources, &bin_data) {
            (Err(_), Some(d)) if d.join("glue-differential-allow.txt").is_file() => {
                d.join("glue-differential-allow.txt")
            }
            _ => manifest.join("tests/glue-differential-allow.txt"),
        },
        PathBuf::from,
    );
    let prebuilt = var("SAI_GLUE_DIFF_CPP").map(PathBuf::from).or_else(|| {
        var("SAI_RS_TEST_BIN")
            .map(|b| Path::new(&b).join(GLUE_DRIVER))
            .filter(|p| p.is_file())
    });
    let cxx = var("CXX").unwrap_or_else(|| "c++".to_owned());
    let exe = match (prebuilt, &sources) {
        (Some(exe), _) => exe,
        (None, Ok(s)) => {
            let Some(inc) = var("SAI_INC").map(PathBuf::from) else {
                summary(
                    "sai-backend-openbcm glue differential: SKIPPED: the C++ sources are there but SAI_INC is not set",
                );
                return None;
            };
            let cc = var("CC").unwrap_or_else(|| "cc".to_owned());
            let dir = scratch()
                .join("sai-glue-cpp")
                .join(inputs_hash(s, &own, &inc, &cxx, &cc));
            let exe = dir.join(GLUE_DRIVER);
            if !exe.is_file() {
                let tc = cpp_build::Toolchain {
                    cxx: &cxx,
                    cc: &cc,
                    static_runtime: false,
                };
                cpp_build::build_variant(s, &inc, &tc, &dir, &glue_build::variant(&own, &s.src))
                    .unwrap_or_else(|e| panic!("building the C++ glue driver: {e}"));
            }
            exe
        }
        (None, Err(_)) => {
            summary(
                "sai-backend-openbcm glue differential: SKIPPED: scripts but no C++ glue driver (SAI_GLUE_DIFF_CPP, $SAI_RS_TEST_BIN)",
            );
            return None;
        }
    };
    let mut scripts: Vec<(String, String)> = cpp_build::scripts(&scripts_dir)
        .unwrap()
        .into_iter()
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    assert!(!scripts.is_empty(), "no *.ops in {}", scripts_dir.display());
    // sai-driver's own scripts, on the tables of sai-openbcm's tests/test_tables.h
    let theirs = manifest.join("../sai-driver/tests/driver-scripts");
    match &sources {
        Ok(s) => {
            let (macsec, phy) = cpp_build::tables(&s.tests, &scratch().join("sai-glue-tables"), &cxx)
                .unwrap_or_else(|e| panic!("the external tables: {e}"));
            for p in cpp_build::scripts(&theirs).unwrap() {
                let text = std::fs::read_to_string(&p)
                    .unwrap()
                    .replace("@EXT_PHY_TABLE@", phy.to_str().unwrap())
                    .replace("@EXT_MACSEC_TABLE@", macsec.to_str().unwrap());
                scripts.push((p.file_stem().unwrap().to_string_lossy().into_owned(), text));
            }
        }
        Err(_) => summary(
            "sai-backend-openbcm glue differential: sai-driver's own scripts SKIPPED (no sai-openbcm tests dir for the tables)",
        ),
    }
    // this crate's glue-only script(s): calls the shared scripts do not make
    let extras = manifest.join("tests/cpp/glue-scripts");
    if extras.is_dir() {
        for p in cpp_build::scripts(&extras).unwrap() {
            scripts.push((
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    } else {
        summary(
            "sai-backend-openbcm glue differential: its own glue-only scripts SKIPPED (no tests/cpp/glue-scripts here)",
        );
    }
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

#[test]
fn rust_and_cpp_backends_make_the_same_glue_calls_but_for_the_allow_list() {
    let Some(setup) = setup() else { return };
    let dir = scratch().join("sai-glue-runs");
    std::fs::create_dir_all(&dir).unwrap();
    let keep = var("SAI_GLUE_DIFF_KEEP").map(PathBuf::from);
    let mut diffs: Vec<Diff> = Vec::new();
    let mut per_script: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new(); // blocks, differing, glue rust, glue cpp
    let (mut glue_total, mut glue_in_identical, mut glue_matched) = (0usize, 0usize, 0usize);
    for (stem, text) in &setup.scripts {
        let scen = dir.join(format!("{stem}.scenario"));
        std::fs::write(&scen, scenario(text)).unwrap();
        let script = format!("profile SAI_INIT_CONFIG_FILE={}\n{text}", scen.display());
        let script_path = dir.join(format!("{stem}.ops"));
        std::fs::write(&script_path, &script).unwrap();

        // the Rust stack, recording
        let chip = Chip::new();
        chip.record(true);
        let rust = Driver::with_hooks(options(), Box::new(GlueHooks { chip })).run_script(&script);
        let replay = dir.join(format!("{stem}.replay"));
        let mut record = glue_lines(&rust).join("\n");
        record.push('\n');
        std::fs::write(&replay, record).unwrap();

        // the C++ stack, answered from that recording
        let out = Command::new(&setup.exe)
            .arg(&script_path)
            .env("GLUE_STUB_REPLAY", &replay)
            .current_dir(&dir)
            .output()
            .unwrap_or_else(|e| panic!("{}: {e}", setup.exe.display()));
        assert!(
            out.status.success(),
            "C++ glue driver on {stem}: {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let cpp = String::from_utf8(out.stdout).unwrap();
        if let Some(k) = &keep {
            std::fs::create_dir_all(k).unwrap();
            std::fs::write(k.join(format!("{stem}.cpp.txt")), &cpp).unwrap();
            std::fs::write(k.join(format!("{stem}.rust.txt")), &rust).unwrap();
        }
        let c = compare::compare(stem, &cpp, &rust).unwrap_or_else(|e| panic!("{e}"));
        let differing: std::collections::BTreeSet<usize> = c.diffs.iter().map(|d| d.index).collect();
        for (i, (rb, cb)) in compare::blocks(&rust)
            .iter()
            .zip(compare::blocks(&cpp).iter())
            .enumerate()
        {
            let glue_of = |b: &compare::Block| -> Vec<String> {
                b.lines.iter().filter(|l| l.starts_with(GLUE)).cloned().collect()
            };
            let (r, k) = (glue_of(rb), glue_of(cb));
            glue_total += r.len();
            if differing.contains(&i) {
                let rr: Vec<&str> = r.iter().map(String::as_str).collect();
                let kk: Vec<&str> = k.iter().map(String::as_str).collect();
                glue_matched += common(&rr, &kk);
            } else {
                glue_in_identical += r.len();
                glue_matched += r.len();
            }
        }
        per_script.insert(
            stem.clone(),
            (c.blocks, c.diffs.len(), glue_lines(&rust).len(), glue_lines(&cpp).len()),
        );
        diffs.extend(c.diffs);
    }
    let total: usize = per_script.values().map(|v| v.0).sum();
    negative_control(&setup, &dir);

    if var("SAI_GLUE_DIFF_ALLOW_UPDATE").as_deref() == Some("1") {
        let old = std::fs::read_to_string(&setup.allow).unwrap_or_default();
        let entries = compare::parse_allow(&old).unwrap_or_default();
        std::fs::write(&setup.allow, compare::regenerate(&entries, &diffs)).unwrap();
        summary(&format!(
            "sai-backend-openbcm glue differential: allow-list REWRITTEN ({}): {total} ops, {} identical, {} entries",
            setup.allow.display(),
            total - diffs.len(),
            diffs.len()
        ));
        return;
    }
    let text = std::fs::read_to_string(&setup.allow).unwrap_or_else(|e| {
        panic!(
            "the allow-list {} is not readable ({e}); SAI_GLUE_DIFF_ALLOW_UPDATE=1 writes one",
            setup.allow.display()
        )
    });
    let entries = compare::parse_allow(&text).unwrap_or_else(|e| panic!("{}: {e}", setup.allow.display()));
    let verdict = compare::judge(&entries, &diffs);
    summary(&format!(
        "sai-backend-openbcm glue differential: {} scripts, {total} ops, {} identical, {} differ (allowed {}, NOT allowed {}), {} allow-list entries ({} stale); glue lines (Rust) {glue_total}: {glue_in_identical} in identical ops, {glue_matched} equal to a C++ line of the op (in order)",
        setup.scripts.len(),
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

/// The comparison can fail: the C++ lines are the C++ backend's arguments, not the recording
/// played back. The first script's first recorded `vlan_create` / `port_enable_set` with its
/// numbers changed in the replay: the C++ still prints the numbers it passed.
fn negative_control(setup: &Setup, dir: &Path) {
    let Some((stem, _)) = setup.scripts.iter().find(|(stem, _)| {
        std::fs::read_to_string(dir.join(format!("{stem}.replay")))
            .is_ok_and(|r| r.contains("port_enable_set(unit=0, port=1, enable=0)"))
    }) else {
        panic!("negative control: no recording with port_enable_set(unit=0, port=1, enable=0)");
    };
    let replay = std::fs::read_to_string(dir.join(format!("{stem}.replay"))).unwrap();
    let changed = replay.replacen(
        "port_enable_set(unit=0, port=1, enable=0)",
        "port_enable_set(unit=0, port=1, enable=7)",
        1,
    );
    let path = dir.join(format!("{stem}.changed-replay"));
    std::fs::write(&path, &changed).unwrap();
    let out = Command::new(&setup.exe)
        .arg(dir.join(format!("{stem}.ops")))
        .env("GLUE_STUB_REPLAY", &path)
        .current_dir(dir)
        .output()
        .unwrap();
    let cpp = String::from_utf8(out.stdout).unwrap();
    let first = cpp
        .lines()
        .find(|l| l.starts_with("  glue port_enable_set(unit=0, port=1,"))
        .unwrap_or_default();
    assert!(
        first.contains("enable=0)") && !cpp.contains("enable=7"),
        "negative control on {stem}: the C++ line is not the C++ call: {first}"
    );
    summary(&format!(
        "sai-backend-openbcm glue differential: negative control ok ({stem}: a changed recording is not echoed)"
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

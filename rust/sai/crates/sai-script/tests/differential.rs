//! The Rust runner against `sai_script.cpp` on the mock libifcs (README "Teralynx tools"): every oracle
//! script (its SAI steps: `oracle-prep` drops exec steps, zeroes sleeps) through the C++ runner, the
//! Rust runner with `--cpp-names` and the Rust runner (fixed), on each libsai given (the C++ and the
//! Rust Teralynx library), each run with the parity tap (the mock's IFCS lines and the runner's
//! markers in one stream). Required:
//!
//! - `--cpp-names`: identical to the C++ runner: the stream step by step, stdout (times masked,
//!   `event` lines as a multiset), the exit code, the `script error` line;
//! - fixed: identical for every script without a FIX line; the scripts with FIX lines are exactly
//!   `tests/fixed-scripts.txt`'s, identical up to their first FIX step, and never end in a script
//!   error;
//! - the exec interface (`tests/exec-script.txt`, a JSON script, not prepped): both runners print the same, and
//!   `--tool` runs another program for a `$ORACLE/bin/<name>`.
//!
//! Environment (the nix check sets it; without it the test says it skipped):
//! `SAI_SCRIPT_DIFF_FIXTURES` (crates/sai-teralynx-parity/build-fixtures.sh's output with the
//! runner), `SAI_SCRIPT_DIFF_SCRIPTS` (the oracle scripts), `SAI_SCRIPT_DIFF_LIBS`
//! (`name=libsai.so.1.0.0,...`), `SAI_SCRIPT_DIFF_RUNNER` (the Rust `sai-script`), optional
//! `SAI_SCRIPT_DIFF_OUT` (report and outputs; default: a temporary directory); `SAI_SCRIPT_DIFF_REQUIRE`:
//! a missing input fails instead of skipping.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sai_teralynx_parity::oracle;
use sai_teralynx_parity::stream as oracle_unit;

/// The runs share one diag socket path per process: the two runner tests do not overlap.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    stream: String,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(f) = env("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).open(f) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn lib_dir(work: &Path, name: &str, lib: &str, fx: &Path) -> PathBuf {
    let d = work.join(format!("lib-{name}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::copy(lib, d.join("libsai.so.1.0.0")).unwrap();
    std::os::unix::fs::symlink("libsai.so.1.0.0", d.join("libsai.so.1")).unwrap();
    std::os::unix::fs::symlink("libsai.so.1", d.join("libsai.so")).unwrap();
    fs::copy(
        fx.join("bin/libsai-teralynx-shim.so"),
        d.join("libsai-teralynx-shim.so"),
    )
    .unwrap();
    d
}

fn run(
    fx: &Path,
    lib: &Path,
    work: &Path,
    prog: &str,
    args: &[&str],
    script: &Path,
    extra_env: &[(&str, &str)],
) -> Run {
    let w = work.join("w");
    let _ = fs::remove_dir_all(&w);
    fs::create_dir_all(&w).unwrap();
    // unix socket paths have 107 bytes: the diag socket in a short directory
    let sock = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join(format!("ssd-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock);
    let profile = format!(
        "SAI_OPENBCM_BACKEND=openbcm\nSAI_TERALYNX_NETDEV=create\nSAI_TERALYNX_LIBIFCS={}\nSAI_TERALYNX_PORT_MAP={}\nSAI_INIT_CONFIG_FILE={}\nSAI_TERALYNX_RUN_DIR={}\nSAI_OPENBCM_DIAG_SOCKET={}\n",
        fx.join("mock/libifcs.so").display(),
        fx.join("cfg/ports32.map").display(),
        fx.join("cfg/init.yaml").display(),
        w.join("run").display(),
        sock.display()
    );
    fs::write(w.join("profile"), profile).unwrap();
    let stream = w.join("stream.txt");
    let mut c = Command::new(prog);
    c.args(args)
        .arg("profile")
        .arg(script)
        .current_dir(&w)
        .env("LD_LIBRARY_PATH", lib)
        .env("LD_PRELOAD", fx.join("bin/libparity-tap.so"))
        .env("PARITY_TAP_FILE", &stream)
        .env("PARITY_MOCK", fx.join("mock/libifcs.so"))
        .env("IFCS_TRACE_FILE", &stream)
        .env_remove("PARITY_TAP_MUTATE")
        .env_remove("SAI_SCRIPT_TOOLS")
        .env_remove("SAI_LIBSAI");
    for (k, v) in extra_env {
        c.env(k, v);
    }
    let o = c.output().unwrap();
    Run {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        stream: fs::read_to_string(&stream).unwrap_or_default(),
    }
}

fn script_error(s: &str) -> Vec<&str> {
    s.lines().filter(|l| l.starts_with("script error:")).collect()
}

/// The FIX lines' step tags, in order.
fn fixes(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|l| l.strip_prefix("sai-script: FIX "))
        .filter_map(|l| l.split_once(": ").map(|x| x.0.to_owned()))
        .collect()
}

/// What a run is compared by (the golden holds exactly this): the exit code, the `script error`
/// lines, the stdout without times (the `event` lines apart, sorted) and the stream's steps
/// (`oracle::steps`: names, line counts, digests of the lines and of the lines up to the end marker).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Norm {
    code: Option<i32>,
    errors: Vec<String>,
    out: Vec<String>,
    events: Vec<String>,
    units: Vec<UnitDigest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UnitDigest {
    name: String,
    lines: usize,
    all: u64,
    upto_end: u64,
}

fn fnv_lines(lines: &[String]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for l in lines {
        for b in l.bytes().chain(*b"\n") {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// The unit's lines up to its end marker (the C++ run's script error stops right after it: its
/// "M done" and the teardown's lines follow there).
fn upto_end(u: &oracle_unit::Unit) -> Vec<String> {
    let mut v = Vec::new();
    for l in &u.lines {
        v.push(l.clone());
        if l.starts_with("M end ") {
            break;
        }
    }
    v
}

/// The diag socket's per-run path (`ssd-<pid>.sock` in the test's temporary directory) as `<diag.sock>`.
fn mask_socket(line: &str) -> String {
    line.split(' ')
        .map(|w| {
            if w.contains("/ssd-") && Path::new(w).extension().is_some_and(|x| x.eq_ignore_ascii_case("sock")) {
                "<diag.sock>"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The run's working directory in the stream's `cfg=<dir>/run/config.yaml` words as `<work>`.
fn mask_work_dir(stream: &str) -> String {
    let mut out = String::with_capacity(stream.len());
    for line in stream.lines() {
        let words: Vec<String> = line
            .split(' ')
            .map(|w| match w.strip_prefix("cfg=") {
                Some(p) if p.ends_with("/run/config.yaml") => "cfg=<work>/run/config.yaml".to_owned(),
                _ => w.to_owned(),
            })
            .collect();
        out += &words.join(" ");
        out.push('\n');
    }
    out
}

fn norm(r: &Run) -> Norm {
    let (out, events) = oracle::stdout(&r.stdout);
    Norm {
        code: r.code,
        errors: script_error(&r.stderr).into_iter().map(mask_socket).collect(),
        out,
        events,
        units: oracle::steps(&mask_work_dir(&r.stream))
            .iter()
            .map(|u| UnitDigest {
                name: u.name.clone(),
                lines: u.lines.len(),
                all: fnv_lines(&u.lines),
                upto_end: fnv_lines(&upto_end(u)),
            })
            .collect(),
    }
}

/// The golden's text of one run (`script <stem>` then its parts; `out` and `ev` lines are the text).
fn render(stem: &str, n: &Norm) -> String {
    let mut s = format!(
        "script {stem}\ncode {}\n",
        n.code.map_or("none".to_owned(), |c| c.to_string())
    );
    for e in &n.errors {
        let _ = writeln!(s, "err {e}");
    }
    for l in &n.out {
        let _ = writeln!(s, "out {l}");
    }
    for l in &n.events {
        let _ = writeln!(s, "ev {l}");
    }
    for u in &n.units {
        let _ = writeln!(s, "unit {}\t{}\t{:016x}\t{:016x}", u.name, u.lines, u.all, u.upto_end);
    }
    s
}

fn parse_golden(text: &str) -> BTreeMap<String, Norm> {
    let mut out: BTreeMap<String, Norm> = BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(stem) = line.strip_prefix("script ") {
            cur = Some(stem.to_owned());
            out.insert(
                stem.to_owned(),
                Norm {
                    code: None,
                    errors: Vec::new(),
                    out: Vec::new(),
                    events: Vec::new(),
                    units: Vec::new(),
                },
            );
            continue;
        }
        let n = out.get_mut(cur.as_deref().unwrap()).unwrap();
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "code" => n.code = rest.parse().ok(),
            "err" => n.errors.push(rest.to_owned()),
            "out" => n.out.push(rest.to_owned()),
            "ev" => n.events.push(rest.to_owned()),
            "unit" => {
                let w: Vec<&str> = rest.split('\t').collect();
                n.units.push(UnitDigest {
                    name: w[0].to_owned(),
                    lines: w[1].parse().unwrap(),
                    all: u64::from_str_radix(w[2], 16).unwrap(),
                    upto_end: u64::from_str_radix(w[3], 16).unwrap(),
                });
            }
            other => panic!("golden: unknown line kind {other:?}"),
        }
    }
    out
}

/// The golden of the C++ runner on the C++ libsai over the mock (tests/golden/sai-script-cpp.txt),
/// captured from `sai_script.cpp`, never from the Rust runner.
const GOLDEN: &str = include_str!("golden/sai-script-cpp.txt");

/// The first difference of two runs (stream units, stdout lines, events, exit, script error), if any,
/// and the index of the first differing stream unit.
fn first_difference(a: &Norm, b: &Norm) -> Option<(String, Option<String>)> {
    for i in 0..a.units.len().max(b.units.len()) {
        if a.units.get(i) != b.units.get(i) {
            let unit = a.units.get(i).or(b.units.get(i)).map(|u| u.name.clone());
            return Some((format!("stream unit {i} ({})", unit.clone().unwrap_or_default()), unit));
        }
    }
    let (la, lb) = (&a.out, &b.out);
    if la != lb {
        let i = la
            .iter()
            .zip(lb)
            .position(|(x, y)| x != y)
            .unwrap_or(la.len().min(lb.len()));
        return Some((format!("stdout line {i}: {:?} / {:?}", la.get(i), lb.get(i)), None));
    }
    if a.events != b.events {
        return Some(("event lines".to_owned(), None));
    }
    if a.code != b.code {
        return Some((format!("exit {:?} / {:?}", a.code, b.code), None));
    }
    if a.errors != b.errors {
        return Some(("script error line".to_owned(), None));
    }
    None
}

/// Both runs equal up to (not including) the stream unit `tag` and its stdout line.
fn equal_before(a: &Norm, b: &Norm, tag: &str) -> Result<(), String> {
    // the unit before the fix ends at its end marker
    for (i, (x, y)) in a.units.iter().zip(&b.units).enumerate() {
        if y.name == tag {
            break;
        }
        let next_is_fix = b.units.get(i + 1).is_some_and(|n| n.name == tag);
        let differ = if next_is_fix {
            x.name != y.name || x.upto_end != y.upto_end
        } else {
            x != y
        };
        if differ {
            return Err(format!("stream unit {i} ({}) differs before the first fix", x.name));
        }
    }
    let cut = format!("STEP {tag} ");
    for (x, y) in a.out.iter().zip(&b.out) {
        if x.starts_with(&cut) || y.starts_with(&cut) {
            break;
        }
        if x != y {
            return Err(format!("stdout {x:?} / {y:?} differs before the first fix"));
        }
    }
    Ok(())
}

/// Whether a difference is the listed departure of the Rust libsai: at unit `allow`, equal before it.
fn departs_at(base: &Norm, run: &Norm, unit: Option<&str>, allow: Option<&str>) -> bool {
    matches!((unit, allow), (Some(u), Some(a)) if u == a) && equal_before(base, run, allow.unwrap_or_default()).is_ok()
}

/// `tests/lib-differences.txt`: per script, the unit where the Rust libsai departs from the C++ libsai.
fn lib_differences() -> BTreeMap<String, String> {
    include_str!("lib-differences.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut w = l.split('\t');
            Some((w.next()?.to_owned(), w.next()?.to_owned()))
        })
        .collect()
}

/// The Rust runner's two modes against a base (the C++ runner's run, or its golden): `--cpp-names`
/// identical; fixed identical without FIX lines, else equal up to the first one and never ending in
/// a script error. Returns the first FIX tag; the failures go into `red`.
///
/// `allow`: the stream unit at which the Rust libsai's answers legitimately depart from the C++
/// libsai's (`tests/lib-differences.txt`): the first difference must be at that unit and everything
/// before it equal; what follows is not compared.
fn check_modes(
    who: &str,
    label: &str,
    base: &Norm,
    k: &Run,
    f: &Run,
    allow: Option<&str>,
    red: &mut Vec<String>,
) -> Option<String> {
    let nk = norm(k);
    if let Some((d, unit)) = first_difference(base, &nk) {
        if !departs_at(base, &nk, unit.as_deref(), allow) {
            red.push(format!("{label}: --cpp-names differs from {who}: {d}"));
        }
    }
    let nf = norm(f);
    let tags = fixes(&f.stderr);
    match (tags.first(), first_difference(base, &nf)) {
        (None, Some((d, unit))) => {
            if !departs_at(base, &nf, unit.as_deref(), allow) {
                red.push(format!(
                    "{label}: the fixed run differs from {who} without a FIX line: {d}"
                ));
            }
        }
        (Some(first), _) => {
            if let Err(e) = equal_before(base, &nf, first) {
                red.push(format!("{label}: {who}: {e}"));
            }
            if f.code == Some(2) || !script_error(&f.stderr).is_empty() {
                red.push(format!("{label}: the fixed run ends in a script error"));
            }
        }
        (None, None) => {}
    }
    tags.first().cloned()
}

#[test]
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one differential, its parts in order; c / k / f: the C++, --cpp-names and fixed runs"
)]
fn rust_runner_against_the_cpp_runner_on_the_mock() {
    let _one_at_a_time = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (Some(fx), Some(scripts), Some(libs), Some(rs)) = (
        env("SAI_SCRIPT_DIFF_FIXTURES"),
        env("SAI_SCRIPT_DIFF_SCRIPTS"),
        env("SAI_SCRIPT_DIFF_LIBS"),
        env("SAI_SCRIPT_DIFF_RUNNER"),
    ) else {
        assert!(
            env("SAI_SCRIPT_DIFF_REQUIRE").is_none(),
            "SAI_SCRIPT_DIFF_REQUIRE is set but the SAI_SCRIPT_DIFF_* inputs are not"
        );
        summary("sai-script differential: SKIPPED (no SAI_SCRIPT_DIFF_* environment)");
        return;
    };
    let fx = PathBuf::from(fx);
    let tmp;
    let work = if let Some(o) = env("SAI_SCRIPT_DIFF_OUT") {
        PathBuf::from(o)
    } else {
        tmp = std::env::temp_dir().join(format!("sai-script-diff-{}", std::process::id()));
        tmp.clone()
    };
    fs::create_dir_all(&work).unwrap();
    let cpp = fx.join("bin/sai-script").to_string_lossy().into_owned();
    let listed: BTreeMap<String, String> = include_str!("fixed-scripts.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t').map(|(a, b)| (a.to_owned(), b.to_owned())))
        .collect();
    let mut all: Vec<PathBuf> = fs::read_dir(&scripts)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    all.sort();
    assert!(!all.is_empty(), "no scripts in {scripts}");
    let mut report = Vec::new();
    let mut red = Vec::new();
    let golden = parse_golden(GOLDEN);
    let departures = lib_differences();
    let capture = env("SAI_SCRIPT_CAPTURE").map(PathBuf::from);
    let mut captured = String::new();
    let mut golden_runs = 0;
    for spec in libs.split(',') {
        let (name, lib) = spec.split_once('=').expect("name=path");
        let ld = lib_dir(&work, name, lib, &fx);
        let mut fixed_seen = Vec::new();
        let (mut same, mut fixed_n) = (0, 0);
        for s in &all {
            let stem = s.file_stem().unwrap().to_string_lossy().into_owned();
            let prepped = work.join(format!("{stem}.prep.json"));
            let p = Command::new(fx.join("bin/oracle-prep"))
                .arg(s)
                .arg(&prepped)
                .output()
                .unwrap();
            assert!(p.status.success(), "oracle-prep {stem}");
            let c = run(&fx, &ld, &work, &cpp, &[], &prepped, &[]);
            let k = run(&fx, &ld, &work, &rs, &["--cpp-names"], &prepped, &[]);
            let f = run(&fx, &ld, &work, &rs, &[], &prepped, &[]);
            let out = work.join("out").join(name);
            fs::create_dir_all(&out).unwrap();
            for (tag, r) in [("cpp", &c), ("cpp-names", &k), ("fixed", &f)] {
                fs::write(out.join(format!("{stem}.{tag}.stdout.txt")), &r.stdout).unwrap();
                fs::write(out.join(format!("{stem}.{tag}.stderr.txt")), &r.stderr).unwrap();
                fs::write(out.join(format!("{stem}.{tag}.stream.txt")), &r.stream).unwrap();
            }
            let (nc, nk, nf) = (norm(&c), norm(&k), norm(&f));
            if let Some((d, _)) = first_difference(&nc, &nk) {
                red.push(format!("{name} {stem}: --cpp-names differs from the C++ runner: {d}"));
            }
            // live == golden: the C++ runner on this libsai gives what the golden holds, and the Rust
            // runner agrees with the golden too
            if capture.is_some() && name == "cpp" {
                captured += &render(&stem, &nc);
            } else if capture.is_none() {
                match golden.get(&stem) {
                    Some(g) => {
                        let allow = if name == "cpp" {
                            None
                        } else {
                            departures.get(&stem).map(String::as_str)
                        };
                        if let Some((d, unit)) = first_difference(g, &nc) {
                            if !departs_at(g, &nc, unit.as_deref(), allow) {
                                red.push(format!("{name} {stem}: the C++ runner differs from the golden: {d}"));
                            }
                        }
                        check_modes("the golden", &format!("{name} {stem}"), g, &k, &f, allow, &mut red);
                        golden_runs += 1;
                    }
                    None => red.push(format!("{name} {stem}: not in the golden")),
                }
            }
            let fx_tags = fixes(&f.stderr);
            let row = match (fx_tags.first(), first_difference(&nc, &nf)) {
                (None, None) => {
                    same += 1;
                    "identical".to_owned()
                }
                (None, Some((d, _))) => {
                    red.push(format!("{name} {stem}: the fixed run differs without a FIX line: {d}"));
                    format!("RED: {d}")
                }
                (Some(first), _) => {
                    fixed_n += 1;
                    fixed_seen.push(stem.clone());
                    if let Err(e) = equal_before(&nc, &nf, first) {
                        red.push(format!("{name} {stem}: {e}"));
                    }
                    if f.code == Some(2) || !script_error(&f.stderr).is_empty() {
                        red.push(format!("{name} {stem}: the fixed run ends in a script error"));
                    }
                    format!(
                        "fixed from '{first}' ({} FIX lines); C++ exit {:?} {} -> exit {:?}",
                        fx_tags.len(),
                        c.code,
                        script_error(&c.stderr).first().copied().unwrap_or("(no script error)"),
                        f.code
                    )
                }
            };
            report.push(format!(
                "{name} {stem}: C++ exit {:?}, --cpp-names identical: {}; fixed: {row}",
                c.code,
                first_difference(&nc, &nk).is_none()
            ));
        }
        let want: Vec<&String> = listed.keys().collect();
        let got: Vec<&String> = fixed_seen.iter().collect();
        if want != got {
            red.push(format!(
                "{name}: scripts with FIX lines {got:?}, tests/fixed-scripts.txt lists {want:?}"
            ));
        }
        report.push(format!(
            "== {name}: {} scripts: --cpp-names identical to the C++ runner on {}; fixed: {same} identical, {fixed_n} fixed (listed: {})",
            all.len(),
            all.len() - red.iter().filter(|r| r.starts_with(&format!("{name} ")) && r.contains("--cpp-names")).count(),
            listed.len()
        ));
        // the exec interface on this library
        let tools = work.join("tools/bin");
        fs::create_dir_all(&tools).unwrap();
        let tool = |n: &str, body: &str| {
            let p = tools.join(n);
            fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut perm = fs::metadata(&p).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
            fs::set_permissions(&p, perm).unwrap();
            p
        };
        tool(
            "frame-tool",
            r#"if [ "$1" = fail ]; then echo "frame-tool: fail"; exit 3; fi; echo "frame-tool: $*""#,
        );
        let other = tool(
            "other-tool",
            r#"if [ "$1" = fail ]; then echo "frame-tool: fail"; exit 3; fi; echo "frame-tool: $*""#,
        );
        let oracle_dir = work.join("tools").to_string_lossy().into_owned();
        let exec = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/exec-script.txt");
        let ce = run(&fx, &ld, &work, &cpp, &[], &exec, &[("ORACLE", &oracle_dir)]);
        let re = run(&fx, &ld, &work, &rs, &[], &exec, &[("ORACLE", &oracle_dir)]);
        let as_tool = format!("frame-tool={}", other.display());
        let te = run(
            &fx,
            &ld,
            &work,
            &rs,
            &["--tool", &as_tool],
            &exec,
            &[("ORACLE", "/nonexistent")],
        );
        let nce = norm(&ce);
        for (what, r) in [("plain", &re), ("--tool", &te)] {
            if let Some((d, _)) = first_difference(&nce, &norm(r)) {
                red.push(format!("{name} exec ({what}): {d}"));
            }
            if capture.is_none() {
                match golden.get("exec") {
                    Some(g) => {
                        if let Some((d, _)) = first_difference(g, &norm(r)) {
                            red.push(format!("{name} exec ({what}): differs from the golden: {d}"));
                        }
                    }
                    None => red.push(format!("{name} exec: not in the golden")),
                }
            }
        }
        if capture.is_some() && name == "cpp" {
            captured += &render("exec", &nce);
        } else if let Some(g) = golden.get("exec") {
            if let Some((d, _)) = first_difference(g, &nce) {
                red.push(format!("{name} exec: the C++ runner differs from the golden: {d}"));
            }
        }
        let lines = oracle::stdout(&ce.stdout).0;
        let ok = lines.iter().filter(|l| l.starts_with("EXEC ")).count() == 5 && ce.code == Some(1);
        if !ok {
            red.push(format!(
                "{name} exec: the C++ runner's exec steps did not run as expected: {lines:?} exit {:?}",
                ce.code
            ));
        }
        report.push(format!(
            "{name} exec: 5 EXEC lines, C++ exit {:?}; Rust identical, also with --tool (other program, ORACLE unset)",
            ce.code
        ));
    }
    let text = format!(
        "{}\n{}",
        report.join("\n"),
        if red.is_empty() {
            "GREEN".to_owned()
        } else {
            format!("RED:\n{}", red.join("\n"))
        }
    );
    fs::write(work.join("report.txt"), &text).unwrap();
    if let Some(out) = capture {
        fs::write(out, captured).unwrap();
        return;
    }
    assert_eq!(
        golden.len(),
        all.len() + 1,
        "the golden covers every script and the exec interface"
    );
    summary(&format!(
        "sai-script live == golden: {golden_runs} (library, script) runs of the C++ runner equal the golden, the Rust runner agrees with it"
    ));
    summary(&format!(
        "sai-script differential: {}",
        text.lines()
            .filter(|l| l.starts_with("==") || l.contains(" exec:"))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    println!("{text}");
    assert!(red.is_empty(), "{text}");
}

/// The Rust runner against the golden of the C++ runner alone: no C++ runner, no C++ libsai (the
/// fixtures' mock, tap and oracle-prep, and the Rust libsai(s) of `SAI_SCRIPT_DIFF_LIBS`).
#[test]
fn rust_runner_against_the_golden_of_the_cpp_runner() {
    let _one_at_a_time = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (Some(fx), Some(scripts), Some(libs), Some(rs)) = (
        env("SAI_SCRIPT_DIFF_FIXTURES"),
        env("SAI_SCRIPT_DIFF_SCRIPTS"),
        env("SAI_SCRIPT_DIFF_LIBS"),
        env("SAI_SCRIPT_DIFF_RUNNER"),
    ) else {
        assert!(
            env("SAI_SCRIPT_DIFF_REQUIRE").is_none(),
            "SAI_SCRIPT_DIFF_REQUIRE is set but the SAI_SCRIPT_DIFF_* inputs are not"
        );
        summary("sai-script golden: SKIPPED (no SAI_SCRIPT_DIFF_* environment)");
        return;
    };
    let fx = PathBuf::from(fx);
    let work = std::env::temp_dir().join(format!("sai-script-gold-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let golden = parse_golden(GOLDEN);
    let departures = lib_differences();
    let mut all: Vec<PathBuf> = fs::read_dir(&scripts)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    all.sort();
    assert_eq!(
        golden.len(),
        all.len() + 1,
        "the golden covers every script and the exec interface"
    );
    let mut listed: Vec<String> = include_str!("fixed-scripts.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t').map(|(a, _)| a.to_owned()))
        .collect();
    listed.sort();
    let mut red = Vec::new();
    let mut runs = 0;
    for spec in libs.split(',') {
        let (name, lib) = spec.split_once('=').expect("name=path");
        let ld = lib_dir(&work, name, lib, &fx);
        let mut fixed_seen = Vec::new();
        for s in &all {
            let stem = s.file_stem().unwrap().to_string_lossy().into_owned();
            let prepped = work.join(format!("{stem}.prep.json"));
            let p = Command::new(fx.join("bin/oracle-prep"))
                .arg(s)
                .arg(&prepped)
                .output()
                .unwrap();
            assert!(p.status.success(), "oracle-prep {stem}");
            let k = run(&fx, &ld, &work, &rs, &["--cpp-names"], &prepped, &[]);
            let f = run(&fx, &ld, &work, &rs, &[], &prepped, &[]);
            let g = golden.get(&stem).unwrap_or_else(|| panic!("{stem}: not in the golden"));
            let allow = if name == "cpp" {
                None
            } else {
                departures.get(&stem).map(String::as_str)
            };
            if check_modes("the golden", &format!("{name} {stem}"), g, &k, &f, allow, &mut red).is_some() {
                fixed_seen.push(stem.clone());
            }
            runs += 1;
        }
        fixed_seen.sort();
        if fixed_seen != listed {
            red.push(format!(
                "{name}: scripts with FIX lines {fixed_seen:?}, tests/fixed-scripts.txt lists {listed:?}"
            ));
        }
        // the exec interface against the golden of the C++ runner's exec run
        let tools = work.join("tools/bin");
        fs::create_dir_all(&tools).unwrap();
        let tool = tools.join("frame-tool");
        fs::write(
            &tool,
            "#!/bin/sh\nif [ \"$1\" = fail ]; then echo \"frame-tool: fail\"; exit 3; fi; echo \"frame-tool: $*\"\n",
        )
        .unwrap();
        let mut perm = fs::metadata(&tool).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&tool, perm).unwrap();
        let oracle_dir = work.join("tools").to_string_lossy().into_owned();
        let exec = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/exec-script.txt");
        let re = run(&fx, &ld, &work, &rs, &[], &exec, &[("ORACLE", &oracle_dir)]);
        if let Some((d, _)) = first_difference(&golden["exec"], &norm(&re)) {
            red.push(format!("{name} exec: differs from the golden: {d}"));
        }
    }
    let _ = fs::remove_dir_all(&work);
    assert!(red.is_empty(), "{}", red.join("\n"));
    summary(&format!(
        "sai-script golden: {runs} (library, script) runs of the Rust runner (--cpp-names and fixed) agree with the golden of sai_script.cpp"
    ));
}

/// Negative control: a changed exit code, stdout line, event, unit digest or dropped unit of the
/// golden is a difference, and a golden shifted by one unit is too.
#[test]
fn the_golden_comparison_reports_a_changed_golden() {
    let golden = parse_golden(GOLDEN);
    assert!(golden.len() > 20, "the golden has the scripts");
    assert_eq!(
        parse_golden(&golden.iter().map(|(k, n)| render(k, n)).collect::<String>()),
        golden
    );
    let (stem, base) = golden.iter().max_by_key(|(_, n)| n.units.len()).unwrap();
    assert!(base.units.len() > 2 && !base.out.is_empty(), "{stem}");
    assert!(first_difference(base, base).is_none());
    let mut m = base.clone();
    m.code = m.code.map(|c| c + 1).or(Some(0));
    assert!(first_difference(base, &m).is_some());
    let mut m = base.clone();
    m.out[0].push('x');
    assert!(first_difference(base, &m).is_some());
    let mut m = base.clone();
    m.units[1].all ^= 1;
    assert!(first_difference(base, &m).is_some());
    let mut m = base.clone();
    m.units.remove(1);
    assert!(first_difference(base, &m).is_some());
    let mut m = base.clone();
    m.errors.push("script error: x".to_owned());
    assert!(first_difference(base, &m).is_some());
    // the text form: one changed byte of the golden file is read as a changed run
    let text = render(stem, base);
    let bad = text.replacen("out ", "out Z", 1);
    assert_ne!(parse_golden(&bad)[stem.as_str()], *base);
}

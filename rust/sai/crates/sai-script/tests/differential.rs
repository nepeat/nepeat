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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sai_teralynx_parity::oracle;
use sai_teralynx_parity::stream as oracle_unit;

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

/// The first difference of two runs (stream units, stdout lines, events, exit, script error), if any,
/// and the index of the first differing stream unit.
fn first_difference(a: &Run, b: &Run) -> Option<(String, Option<String>)> {
    let (sa, sb) = (oracle::steps(&a.stream), oracle::steps(&b.stream));
    for i in 0..sa.len().max(sb.len()) {
        if sa.get(i) != sb.get(i) {
            let unit = sa.get(i).or(sb.get(i)).map(|u| u.name.clone());
            return Some((format!("stream unit {i} ({})", unit.clone().unwrap_or_default()), unit));
        }
    }
    let ((la, ea), (lb, eb)) = (oracle::stdout(&a.stdout), oracle::stdout(&b.stdout));
    if la != lb {
        let i = la
            .iter()
            .zip(&lb)
            .position(|(x, y)| x != y)
            .unwrap_or(la.len().min(lb.len()));
        return Some((format!("stdout line {i}: {:?} / {:?}", la.get(i), lb.get(i)), None));
    }
    if ea != eb {
        return Some(("event lines".to_owned(), None));
    }
    if a.code != b.code {
        return Some((format!("exit {:?} / {:?}", a.code, b.code), None));
    }
    if script_error(&a.stderr) != script_error(&b.stderr) {
        return Some(("script error line".to_owned(), None));
    }
    None
}

/// Both runs equal up to (not including) the stream unit `tag` and its stdout line.
fn equal_before(a: &Run, b: &Run, tag: &str) -> Result<(), String> {
    let (sa, sb) = (oracle::steps(&a.stream), oracle::steps(&b.stream));
    // the unit before the fix ends at its end marker (the C++ run's script error stops right after it:
    // its "M done" and the teardown's lines follow there)
    let upto_end = |u: &oracle_unit::Unit| -> Vec<String> {
        let mut v = Vec::new();
        for l in &u.lines {
            v.push(l.clone());
            if l.starts_with("M end ") {
                break;
            }
        }
        v
    };
    for (i, (x, y)) in sa.iter().zip(&sb).enumerate() {
        if y.name == tag {
            break;
        }
        let next_is_fix = sb.get(i + 1).is_some_and(|n| n.name == tag);
        let differ = if next_is_fix {
            upto_end(x) != upto_end(y)
        } else {
            x != y
        };
        if differ {
            return Err(format!("stream unit {i} ({}) differs before the first fix", x.name));
        }
    }
    let ((la, _), (lb, _)) = (oracle::stdout(&a.stdout), oracle::stdout(&b.stdout));
    let cut = format!("STEP {tag} ");
    for (x, y) in la.iter().zip(&lb) {
        if x.starts_with(&cut) || y.starts_with(&cut) {
            break;
        }
        if x != y {
            return Err(format!("stdout {x:?} / {y:?} differs before the first fix"));
        }
    }
    Ok(())
}

#[test]
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one differential, its parts in order; c / k / f: the C++, --cpp-names and fixed runs"
)]
fn rust_runner_against_the_cpp_runner_on_the_mock() {
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
            if let Some((d, _)) = first_difference(&c, &k) {
                red.push(format!("{name} {stem}: --cpp-names differs from the C++ runner: {d}"));
            }
            let fx_tags = fixes(&f.stderr);
            let row = match (fx_tags.first(), first_difference(&c, &f)) {
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
                    if let Err(e) = equal_before(&c, &f, first) {
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
                first_difference(&c, &k).is_none()
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
        for (what, r) in [("plain", &re), ("--tool", &te)] {
            if let Some((d, _)) = first_difference(&ce, r) {
                red.push(format!("{name} exec ({what}): {d}"));
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

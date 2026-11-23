//! Differential test against the C client: the same command lines and environment for
//! `openbcm-diag` built from `openbcm_diag.c` (frozen in sai-teralynx/frontend/src; live) and for this crate's binary, against
//! a fake server that records what it receives; exit code, stdout, stderr and the bytes the
//! server got must be equal.
//!
//! The C client is built by `build.rs` (never at test time). Where the build tree is not (a
//! copied test executable), `SAI_RS_TEST_BIN` names a directory with `openbcm-diag-c` (from
//! `$OUT_DIR/target-tests`) and `openbcm-diag-rs`. Without the C client the test says so and
//! passes.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn tools() -> Option<(PathBuf, PathBuf)> {
    let bundle = std::env::var_os("SAI_RS_TEST_BIN").map(PathBuf::from);
    let built = Some(env!("OPENBCM_DIAG_C_HELPERS"))
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let helpers = bundle
        .clone()
        .filter(|d| d.join("openbcm-diag-c").is_file())
        .or(built)?;
    let rust = bundle
        .map(|d| d.join("openbcm-diag-rs"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openbcm-diag-rs")));
    Some((helpers.join("openbcm-diag-c"), rust))
}

/// A fake syncd: records each request (read to EOF) and answers by its first word.
struct Fake {
    got: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    path: PathBuf,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn answer(request: &[u8]) -> Vec<Vec<u8>> {
    let text = String::from_utf8_lossy(request);
    let line = text.trim_end_matches('\n');
    match line.split(' ').next().unwrap_or("") {
        "fail" => vec![b"some output\nopenbcm-diag: command failed (-1)\n".to_vec()],
        "refuse" => vec![
            format!("openbcm-diag: refused (would end or re-init the SDK, or chains commands): {line}\n").into_bytes(),
        ],
        // the marker split across two reads: neither tool sees it
        "split" => vec![b"xx openbcm-di".to_vec(), b"ag: yy\n".to_vec()],
        "late" => vec![b"line one\n".to_vec(), b"openbcm-diag: no switch\n".to_vec()],
        "big" => vec![(0..200_000u32).map(|i| b"abcdefghij\n"[(i % 11) as usize]).collect()],
        "empty" => vec![],
        _ => vec![[b"fake: ".as_slice(), line.as_bytes(), b"\n"].concat()],
    }
}

impl Fake {
    fn start(path: &Path) -> Self {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path).unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (got, stop) = (Arc::clone(&got), Arc::clone(&stop));
            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(mut conn) = conn else { continue };
                    let mut request = Vec::new();
                    conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let _ = conn.read_to_end(&mut request);
                    let pieces = answer(&request);
                    got.lock().unwrap().push(request);
                    for (i, piece) in pieces.iter().enumerate() {
                        if i > 0 {
                            std::thread::sleep(Duration::from_millis(300));
                        }
                        let _ = conn.write_all(piece);
                    }
                }
            })
        };
        Self {
            got,
            stop,
            path: path.to_owned(),
            thread: Some(thread),
        }
    }

    fn take(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.got.lock().unwrap())
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::os::unix::net::UnixStream::connect(&self.path);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Run {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    received: Vec<Vec<u8>>,
}

fn run(exe: &Path, args: &[OsString], env: Option<&OsString>, fake: &Fake) -> Run {
    let mut cmd = Command::new(exe);
    cmd.arg0("openbcm-diag").args(args).env_remove("OPENBCM_DIAG_SOCKET");
    if let Some(v) = env {
        cmd.env("OPENBCM_DIAG_SOCKET", v);
    }
    let o = cmd.output().unwrap();
    Run {
        code: o.status.code(),
        stdout: o.stdout,
        stderr: o.stderr,
        received: fake.take(),
    }
}

#[test]
fn the_rust_client_behaves_like_openbcm_diag_c() {
    let Some((c, rust)) = tools() else {
        summary(
            "openbcm-diag differential: the C client was not built (SAI_OPENBCM_SRC at build time, or SAI_RS_TEST_BIN): SKIPPED",
        );
        return;
    };
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("obd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("d.sock");
    let s = sock.to_str().unwrap().to_owned();
    let fake = Fake::start(&sock);
    let os = |w: &[&str]| -> Vec<OsString> { w.iter().map(|a| OsString::from(a.replace("{s}", &s))).collect() };
    let long_path = format!("/{}", "p".repeat(107));
    let x1021 = "x".repeat(1021);
    let x1022 = "x".repeat(1022);
    let (a500, b521, b522) = ("a".repeat(500), "b".repeat(521), "b".repeat(522));
    let env_sock = OsString::from(&s);
    let env_empty = OsString::new();
    let env_missing = OsString::from("/nonexistent/diag.sock");
    let mut cases: Vec<(Vec<OsString>, Option<&OsString>)> = vec![
        (os(&[]), None),
        (os(&[]), Some(&env_sock)),
        (os(&["-s"]), Some(&env_sock)),
        (os(&["-s", "{s}"]), None),
        (os(&["-s", "{s}", "l2", "station", "show"]), None),
        (os(&["l2", "show"]), Some(&env_sock)),
        (os(&["l2", "show"]), Some(&env_missing)),
        (os(&["l2", "show"]), Some(&env_empty)),
        (os(&["-s", "{s}", "ps"]), Some(&env_missing)),
        (os(&["-s", "/nonexistent/x.sock", "ps"]), Some(&env_sock)),
        (os(&["-s", &long_path, "ps"]), None),
        (os(&["-s", "", "ps"]), None),
        (os(&["-s", "{s}", "", "ps"]), None),
        (os(&["-s", "{s}", "ps", "", "x"]), None),
        (os(&["-s", "{s}", "a", "-s", "b"]), None),
        (os(&["--", "ps"]), Some(&env_sock)),
        (os(&["-s", "{s}", &x1021]), None),
        (os(&["-s", "{s}", &x1022]), None),
        (os(&["-s", "{s}", &a500, &b521]), None),
        (os(&["-s", "{s}", &a500, &b522]), None),
        (os(&["-s", "{s}", "fail", "x"]), None),
        (os(&["-s", "{s}", "refuse"]), None),
        (os(&["-s", "{s}", "split"]), None),
        (os(&["-s", "{s}", "late"]), None),
        (os(&["-s", "{s}", "big"]), None),
        (os(&["-s", "{s}", "empty"]), None),
        (os(&["-s", "{s}", "phy raw c45 0x10 0x1 0x8b00"]), None),
        (os(&["-s", "{s}", "show\tc", "ce0\n"]), None),
    ];
    let mut odd = os(&["-s", "{s}"]);
    odd.push(OsString::from_vec(vec![b'u', 0xff, 0xfe, b'v']));
    cases.push((odd, None));
    let mut n = 0;
    for (args, env) in &cases {
        let expected = run(&c, args, *env, &fake);
        let got = run(&rust, args, *env, &fake);
        assert_eq!(got, expected, "args {args:?}, OPENBCM_DIAG_SOCKET {env:?}");
        n += 1;
    }
    drop(fake);
    std::fs::remove_dir_all(&dir).unwrap();
    summary(&format!(
        "openbcm-diag differential: {n} command lines agree with openbcm_diag.c (exit code, stdout, stderr, bytes sent)"
    ));
}

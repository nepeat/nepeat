//! Differential test against the C client `openbcm_diag.c`, held to a golden: the same command lines
//! and environment for this crate's binary, against a fake server that records what it receives;
//! exit code, stdout, stderr and the bytes the server got must equal the golden transcript
//! (`golden/diag-client-cpp.txt`) of the C client. The C client (`openbcm_diag.c`, in the C++
//! frontend) was deleted in H5 on 2026-11-27; it was last in the tree at commit 767d9b0, and its
//! outputs are the goldens in `tests/golden` (frozen, never regenerated from the Rust output).
//! `SAI_RS_TEST_BIN` may name a directory with a copied `openbcm-diag-rs` (README "Big-endian runs").
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use std::ffi::OsString;
use std::fmt::Write as _;
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

/// The golden transcript of the C client (tests/golden/diag-client-cpp.txt), captured from the C
/// client `openbcm_diag.c` (deleted in H5, last at commit 767d9b0), never from this crate's binary.
const GOLDEN: &str = include_str!("golden/diag-client-cpp.txt");

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn replace_bytes(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// A byte string as one line: escaped text up to 300 bytes, else its length and FNV-1a 64 (plus
/// the head, for reading).
fn show(raw: &[u8], sock: &str) -> String {
    // the per-run socket path is replaced first, so lengths and digests do not depend on it
    let bytes = &replace_bytes(raw, sock.as_bytes(), b"<sock>");
    let text = |b: &[u8]| String::from_utf8_lossy(&b.escape_ascii().collect::<Vec<u8>>()).into_owned();
    if bytes.len() <= 300 {
        format!("{} bytes {}", bytes.len(), text(bytes))
    } else {
        format!(
            "{} bytes fnv64 {:016x} head {}",
            bytes.len(),
            fnv64(bytes),
            text(&bytes[..40])
        )
    }
}

/// One case's block of the transcript.
fn block(n: usize, args: &[OsString], env: Option<&OsString>, r: &Run, sock: &str) -> String {
    let word = |a: &OsString| show(a.as_encoded_bytes(), sock);
    let mut s = format!("case {n}\n");
    for a in args {
        let _ = writeln!(s, "  arg {}", word(a));
    }
    match env {
        Some(e) => {
            let _ = writeln!(s, "  env OPENBCM_DIAG_SOCKET {}", word(e));
        }
        None => s.push_str("  env OPENBCM_DIAG_SOCKET unset\n"),
    }
    let _ = writeln!(s, "  exit {:?}", r.code);
    let _ = writeln!(s, "  stdout {}", show(&r.stdout, sock));
    let _ = writeln!(s, "  stderr {}", show(&r.stderr, sock));
    let _ = writeln!(s, "  received {}", r.received.len());
    for q in &r.received {
        let _ = writeln!(s, "    {}", show(q, sock));
    }
    s
}

/// The comparison of a transcript with the golden: the first differing line, or Ok.
fn compare(golden: &str, actual: &str) -> Result<(), String> {
    let body = |t: &str| {
        t.lines()
            .filter(|l| !l.starts_with('#'))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (g, a) = (body(golden), body(actual));
    for (i, (x, y)) in g.iter().zip(&a).enumerate() {
        if x != y {
            return Err(format!("line {}: golden {x:?}, actual {y:?}", i + 1));
        }
    }
    if g.len() == a.len() {
        Ok(())
    } else {
        Err(format!("golden has {} lines, actual {}", g.len(), a.len()))
    }
}

/// The 29 command lines, with the environment each one runs under.
fn cases(
    s: &str,
    env_sock: &OsString,
    env_empty: &OsString,
    env_missing: &OsString,
) -> Vec<(Vec<OsString>, Option<OsString>)> {
    let os = |w: &[&str]| -> Vec<OsString> { w.iter().map(|a| OsString::from(a.replace("{s}", s))).collect() };
    let long_path = format!("/{}", "p".repeat(107));
    let x1021 = "x".repeat(1021);
    let x1022 = "x".repeat(1022);
    let (a500, b521, b522) = ("a".repeat(500), "b".repeat(521), "b".repeat(522));
    let (e, m, q) = (
        Some(env_sock.clone()),
        Some(env_missing.clone()),
        Some(env_empty.clone()),
    );
    let mut cases: Vec<(Vec<OsString>, Option<OsString>)> = vec![
        (os(&[]), None),
        (os(&[]), e.clone()),
        (os(&["-s"]), e.clone()),
        (os(&["-s", "{s}"]), None),
        (os(&["-s", "{s}", "l2", "station", "show"]), None),
        (os(&["l2", "show"]), e.clone()),
        (os(&["l2", "show"]), m.clone()),
        (os(&["l2", "show"]), q),
        (os(&["-s", "{s}", "ps"]), m),
        (os(&["-s", "/nonexistent/x.sock", "ps"]), e.clone()),
        (os(&["-s", &long_path, "ps"]), None),
        (os(&["-s", "", "ps"]), None),
        (os(&["-s", "{s}", "", "ps"]), None),
        (os(&["-s", "{s}", "ps", "", "x"]), None),
        (os(&["-s", "{s}", "a", "-s", "b"]), None),
        (os(&["--", "ps"]), e),
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
    cases
}

/// Runs every case with `exe` and renders the transcript (the header lines are the golden's own).
fn transcript(exe: &Path, tag: &str) -> (String, usize) {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("obd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("d.sock");
    let s = sock.to_str().unwrap().to_owned();
    let fake = Fake::start(&sock);
    let (env_sock, env_empty, env_missing) = (
        OsString::from(&s),
        OsString::new(),
        OsString::from("/nonexistent/diag.sock"),
    );
    let list = cases(&s, &env_sock, &env_empty, &env_missing);
    let mut out = String::new();
    for (n, (args, env)) in list.iter().enumerate() {
        let r = run(exe, args, env.as_ref(), &fake);
        out += &block(n + 1, args, env.as_ref(), &r, &s);
    }
    drop(fake);
    std::fs::remove_dir_all(&dir).unwrap();
    (out, list.len())
}

#[test]
fn the_rust_client_matches_the_golden_transcript() {
    let rust = PathBuf::from(std::env::var_os("SAI_RS_TEST_BIN").map_or_else(
        || env!("CARGO_BIN_EXE_openbcm-diag-rs").into(),
        |d| PathBuf::from(d).join("openbcm-diag-rs").into_os_string(),
    ));
    let (got, n) = transcript(&rust, "gold");
    compare(GOLDEN, &got).unwrap();
    summary(&format!(
        "openbcm-diag golden: {n} command lines of the Rust client equal the golden transcript of openbcm_diag.c"
    ));
}

/// Negative control: a changed byte, a dropped line and an added line in the golden are reported.
#[test]
fn the_golden_comparison_reports_a_changed_transcript() {
    assert!(
        GOLDEN.lines().any(|l| l.starts_with("case ")),
        "the golden has no cases"
    );
    compare(GOLDEN, GOLDEN).unwrap();
    let flipped = GOLDEN.replacen("  exit Some(0)", "  exit Some(1)", 1);
    assert_ne!(flipped, GOLDEN);
    assert!(compare(&flipped, GOLDEN).is_err());
    let dropped = GOLDEN
        .lines()
        .filter(|l| !l.starts_with("case 7"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(compare(&dropped, GOLDEN).is_err());
    assert!(compare(&format!("{GOLDEN}extra\n"), GOLDEN).is_err());
}

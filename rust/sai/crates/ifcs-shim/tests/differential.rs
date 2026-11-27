//! The Rust `libsai-teralynx-shim.so` against the C one, live: `tests/c/guard_harness.c` runs each
//! script (the corpus in `tests/corpus` and fixed-seed generated ones) once with the C library,
//! built here from `ai_re/nix/packages/sai-teralynx/src/{shim.c,ifcs_guard.c}` unmodified with the
//! Makefile's flags, and once with this crate's cdylib; the two traces must be equal line for line
//! (but for the one deliberate difference, [`compare_allowing_text_cap`]: a counters' text the C's
//! 4096-byte buffer cannot hold is written and answered by the Rust guard; corpus
//! `allones-overflow.txt`):
//! every return code and value, every original-accessor call (the re-reads), every sleep, every
//! stderr byte, every version of the counters' file, the inject file's removal, the text, the
//! `CPython` stubs' answers and the copy loop's results. Beside the traces, per step: the Rust
//! library allocates no more than the C one, and neither registers a thread-local destructor.
//!
//! Inputs: `IFCS_SHIM_C_SRC` (the directory with `shim.c` / `ifcs_guard.c`; default: the
//! monorepo's), `CC` (default `cc`), `IFCS_SHIM_LIB` (the Rust library; default: the cdylib of
//! this test build). Without the C sources or a compiler the test skips with a line, unless
//! `IFCS_SHIM_DIFF_REQUIRE=1` (`nix/ifcs-shim.nix`): then it fails. `IFCS_SHIM_DIFF_DUMP=<dir>` copies every
//! script there (the aarch64 run of `nix/ifcs-shim.nix` replays them under emulation).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn required() -> bool {
    std::env::var("IFCS_SHIM_DIFF_REQUIRE").is_ok_and(|v| v == "1")
}

fn skip(why: &str) {
    assert!(!required(), "ifcs-shim differential required but: {why}");
    eprintln!("ifcs-shim differential: SKIPPED ({why})");
}

fn c_src() -> Option<PathBuf> {
    let dir = std::env::var_os("IFCS_SHIM_C_SRC").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ai_re/nix/packages/sai-teralynx/src"),
        PathBuf::from,
    );
    (dir.join("ifcs_guard.c").is_file() && dir.join("shim.c").is_file()).then_some(dir)
}

fn rust_lib() -> Option<PathBuf> {
    if let Some(lib) = std::env::var_os("IFCS_SHIM_LIB") {
        return Some(PathBuf::from(lib));
    }
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    [Some(deps), deps.parent()]
        .into_iter()
        .flatten()
        .map(|d| d.join("libsai_teralynx_shim.so"))
        .find(|p| p.is_file())
}

fn cc(args: &[&std::ffi::OsStr]) -> bool {
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let mut words = compiler.split_whitespace();
    let Some(prog) = words.next() else { return false };
    Command::new(prog)
        .args(words)
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

/// A small deterministic generator (xorshift64*), so the generated scripts are pinned by a digest.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[usize::try_from(self.below(xs.len() as u64)).unwrap()]
    }
}

/// One answer of a guarded register: mostly valid, often all-ones, sometimes garbage or an error.
fn answer(r: &mut Rng, off: u32, ring: u32) -> String {
    let ring = if ring == 0 { 0x800 } else { ring };
    match r.below(10) {
        0..=4 => {
            let v = if off == 0x4860 {
                u32::try_from(r.below(u64::from(ring))).unwrap() | (u32::try_from(r.below(2)).unwrap() << 12)
            } else {
                u32::try_from(r.next() >> 32).unwrap() & 0xffff_fffe
            };
            format!("0:{v:x}")
        }
        5..=7 => "0:ffffffff".into(),
        8 => format!("0:{:x}", r.next() >> 32),
        _ => format!("{}:{:x}", i64::try_from(r.below(5)).unwrap() - 2, r.next() >> 40),
    }
}

/// A generated script: the learn thread's traffic and everything else the guard sees, at random.
fn generate(seed: u64) -> String {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut s = String::new();
    let rings = [0_u32, 0x400, 0x800, 0x100, 0x7ff];
    let mut ring = rings[usize::try_from(r.below(5)).unwrap()];
    let limits = ["1073741824", "0", "4096", "1"];
    let mut limit = r.pick(&limits);
    let stats = r.pick(&["file", "file", "file", "none", "empty", "baddir"]);
    let inject = r.below(3) == 0;
    writeln!(
        s,
        "t {} {}.{:03}",
        100 + r.below(100_000),
        1_764_000_000 + r.below(1_000_000),
        r.below(1000)
    )
    .unwrap();
    writeln!(s, "init {ring:x} {limit} {stats} {}", u8::from(inject)).unwrap();
    writeln!(s, "installed 1").unwrap();
    let others: Vec<u32> = (0..24).map(|k| 0x1_0000 + k * 0x104).collect();
    for _ in 0..(150 + r.below(250)) {
        match r.below(100) {
            0..=44 => {
                let off = [0x4860_u32, 0x4864, 0x485c][usize::try_from(r.below(3)).unwrap()];
                if r.below(2) == 0 {
                    let n = 1 + r.below(5);
                    let a: Vec<String> = (0..n).map(|_| answer(&mut r, off, ring)).collect();
                    writeln!(s, "q {off:x} {}", a.join(" ")).unwrap();
                }
                writeln!(s, "{} {off:x}", if r.below(40) == 0 { "rdnull" } else { "rd" }).unwrap();
            }
            45..=54 => writeln!(
                s,
                "wr {} {:x}",
                r.pick(&["4864", "485c", "4860", "1234"]),
                r.next() >> 32
            )
            .unwrap(),
            55..=64 => {
                let off = others[usize::try_from(r.below(24)).unwrap()];
                if r.below(2) == 0 {
                    writeln!(s, "set {off:x} {}", r.pick(&["ffffffff", "ffffffff", "0", "1234"])).unwrap();
                }
                let rep = if r.below(10) == 0 { 1 + r.below(200) } else { 1 };
                writeln!(s, "rd {off:x} {rep}").unwrap();
            }
            65..=74 => writeln!(
                s,
                "adv {}",
                r.pick(&["0.001", "0.003", "0.5", "1", "1", "2", "29.9", "30", "31", "599", "601"])
            )
            .unwrap(),
            75..=81 => {
                let n = match r.below(6) {
                    0 => 1 + r.below(4096),
                    1 => 1_073_741_823 + r.below(3),
                    2 => 4095 + r.below(3),
                    3 => 34_359_721_992,
                    4 => r.next(),
                    _ => r.below(3),
                };
                // a real allocation only below 64 MiB (the limit refuses the rest, except limit 0)
                let n = if limit == "0" && n > 64 << 20 {
                    n % (64 << 20)
                } else {
                    n
                };
                writeln!(s, "malloc {n}").unwrap();
            }
            82..=85 => writeln!(
                s,
                "injectfile {}",
                r.pick(&[
                    "4860 2",
                    "4864 1",
                    "485c 3",
                    "0x4860 4\\n",
                    "4860",
                    "zz",
                    "",
                    "1234 2",
                    " 4860  1x",
                    "4860 -1",
                    "4860 0"
                ])
            )
            .unwrap(),
            86..=88 => writeln!(s, "text").unwrap(),
            89..=90 => writeln!(s, "thread {}", r.below(2)).unwrap(),
            91..=92 => writeln!(s, "installed {}", r.below(3)).unwrap(),
            93 => {
                ring = rings[usize::try_from(r.below(5)).unwrap()];
                limit = r.pick(&limits);
                writeln!(s, "init {ring:x} {limit} {stats} {}", u8::from(inject)).unwrap();
            }
            94 => writeln!(s, "wrrc {}", i64::try_from(r.below(3)).unwrap() - 1).unwrap(),
            95..=97 => writeln!(s, "memcpy {} {} {}", r.below(17), r.below(9), r.below(300)).unwrap(),
            _ => writeln!(
                s,
                "py {}",
                r.pick(&[
                    "Py_BuildValue",
                    "PyObject_CallFunctionObjArgs",
                    "PyErr_Print",
                    "PyList_SetItem",
                    "PyGILState_Release"
                ])
            )
            .unwrap(),
        }
    }
    writeln!(s, "text").unwrap();
    s
}

fn fnv(data: &[u8], mut h: u64) -> u64 {
    for &b in data {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The generated scripts' inputs, pinned: a change of the generator is a change of the test.
const GENERATED: u64 = 400;
const GENERATED_DIGEST: u64 = 0xa445_7383_ef10_8f57;

struct Run {
    trace: String,
    allocs: Vec<(String, u64, u64)>,
}

fn run(harness: &Path, lib: &Path, script: &Path, work: &Path) -> Run {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let out = Command::new(harness)
        .arg(lib)
        .arg(script)
        .arg(work)
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "harness failed on {} with {}:\n{}",
        script.display(),
        lib.display(),
        String::from_utf8_lossy(&out.stdout)
    );
    let allocs = std::fs::read_to_string(work.join("counts.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("total"))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            (w[0].to_string(), w[2].parse().unwrap(), w[4].parse().unwrap())
        })
        .collect();
    Run {
        trace: String::from_utf8(out.stdout).unwrap(),
        allocs,
    }
}

/// The C's counters' text buffer (`ifcs_guard.c` `render`: `char b[4096]`).
const C_TEXT_CAP: usize = ifcs_guard::C_TEXT_CAP;

/// The length of a trace line's payload after its tag, unescaped (`print_escaped`: `\\n`, `\\\\`,
/// `\\xNN` are one byte each).
fn unescaped_len(s: &str) -> usize {
    let b = s.as_bytes();
    let (mut i, mut n) = (0, 0);
    while i < b.len() {
        i += match (b[i], b.get(i + 1)) {
            (b'\\', Some(b'x')) => 4,
            (b'\\', Some(_)) => 2,
            _ => 1,
        };
        n += 1;
    }
    n
}

/// A counters' text the C cannot hold: a `  prom ` or `  text ` line of at least `C_TEXT_CAP` bytes.
fn beyond_c_cap(line: &str, tag: &str) -> bool {
    line.strip_prefix(tag).is_some_and(|t| unescaped_len(t) >= C_TEXT_CAP)
}

/// The traces equal but for the one deliberate difference (README "Teralynx shim"): where the C's
/// counters' text outgrows its 4096-byte buffer, the Rust guard still writes the file (an extra
/// `  prom <text>` line, the text at least `C_TEXT_CAP` bytes long) and `tl_ifcs_guard_text`
/// answers the text (`  text <text>` where the C has `  text `, empty). Nothing else may differ.
/// `Ok(n)`: the lines allowed; `Err((c, rust))`: the first lines that differ otherwise.
fn compare_allowing_text_cap(c_trace: &[&str], rust_trace: &[&str]) -> Result<usize, (usize, usize)> {
    let (mut ci, mut ri, mut allowed) = (0, 0, 0);
    while ci < c_trace.len() || ri < rust_trace.len() {
        match (c_trace.get(ci), rust_trace.get(ri)) {
            (Some(cl), Some(rl)) if cl == rl => {
                ci += 1;
                ri += 1;
            }
            (Some(&"  text "), Some(rl)) if beyond_c_cap(rl, "  text ") => {
                ci += 1;
                ri += 1;
                allowed += 1;
            }
            (_, Some(rl)) if beyond_c_cap(rl, "  prom ") => {
                ri += 1;
                allowed += 1;
            }
            _ => return Err((ci, ri)),
        }
    }
    Ok(allowed)
}

#[test]
fn the_allow_rule_is_exactly_the_text_cap() {
    let long = "x".repeat(C_TEXT_CAP);
    let short = "x".repeat(C_TEXT_CAP - 1);
    let (prom, text) = (format!("  prom {long}"), format!("  text {long}"));
    assert_eq!(compare_allowing_text_cap(&["a", "b"], &["a", "b"]), Ok(0));
    assert_eq!(
        compare_allowing_text_cap(&["a", "  text ", "b"], &["a", &prom, &text, "b"]),
        Ok(2)
    );
    // a shorter text, a C text that is not empty, anything else: a difference
    let sp = format!("  prom {short}");
    assert_eq!(compare_allowing_text_cap(&["a"], &["a", &sp]), Err((1, 1)));
    assert_eq!(compare_allowing_text_cap(&["  text x"], &[&text]), Err((0, 0)));
    assert_eq!(compare_allowing_text_cap(&["a", "c"], &["a", "b"]), Err((1, 1)));
    assert_eq!(compare_allowing_text_cap(&["a", "x"], &["a", &prom]), Err((1, 2)));
    assert_eq!(unescaped_len("a\\nb\\\\c\\x1f"), 6);
}

/// Compiles the harness and lays out every script of the comparison: the corpus (sorted by name)
/// and the `GENERATED` seeded ones, in a directory under `dir`.
fn harness_and_scripts(dir: &Path) -> (PathBuf, Vec<(String, PathBuf)>) {
    let harness = dir.join("guard_harness");
    let hsrc = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/c/guard_harness.c");
    assert!(
        cc(&[
            "-std=gnu11",
            "-O1",
            "-g",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-rdynamic",
            "-o"
        ]
        .map(std::ffi::OsStr::new)
        .iter()
        .copied()
        .chain([
            harness.as_os_str(),
            hsrc.as_os_str(),
            std::ffi::OsStr::new("-ldl"),
            std::ffi::OsStr::new("-lpthread")
        ])
        .collect::<Vec<_>>()),
        "the harness does not compile"
    );

    let mut scripts: Vec<(String, PathBuf)> = Vec::new();
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&corpus).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for p in names {
        scripts.push((p.file_name().unwrap().to_string_lossy().into_owned(), p));
    }
    let gen_dir = dir.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();
    let mut digest = 0xcbf2_9ce4_8422_2325;
    for seed in 0..GENERATED {
        let text = generate(seed);
        digest = fnv(text.as_bytes(), digest);
        let p = gen_dir.join(format!("gen-{seed:03}.txt"));
        std::fs::write(&p, text).unwrap();
        scripts.push((format!("gen-{seed:03}"), p));
    }
    eprintln!("ifcs-shim differential: generated inputs digest {digest:016x} ({GENERATED} scripts)");
    if let Some(dump) = std::env::var_os("IFCS_SHIM_DIFF_DUMP") {
        let dump = PathBuf::from(dump);
        std::fs::create_dir_all(&dump).unwrap();
        for (_, path) in &scripts {
            std::fs::copy(path, dump.join(path.file_name().unwrap())).unwrap();
        }
    }
    if GENERATED_DIGEST != 0 {
        assert_eq!(
            digest, GENERATED_DIGEST,
            "the generator changed: pin the new digest deliberately"
        );
    }
    (harness, scripts)
}

/// The golden of the C guard's traces (tests/golden/guard-traces-cpp.txt), captured from the C
/// library `shim.c` + `ifcs_guard.c` with the harness, never from the Rust output.
const GOLDEN: &str = include_str!("golden/guard-traces-cpp.txt");

/// One script of the golden: the script's digest and the C trace's size and digest; the full
/// trace (without line ends) only where the Rust one differs by the allowed difference.
struct Entry {
    name: String,
    input: u64,
    lines: usize,
    bytes: usize,
    digest: u64,
    full: Option<Vec<String>>,
}

fn parse_golden(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(l) = line.strip_prefix('|') {
            let e = out.last_mut().unwrap();
            e.full.get_or_insert_with(Vec::new).push(l.to_owned());
        } else if let Some(rest) = line.strip_prefix("script ") {
            let w: Vec<&str> = rest.split_whitespace().collect();
            let hex = |s: &str| u64::from_str_radix(s, 16).unwrap();
            out.push(Entry {
                name: w[0].to_owned(),
                input: hex(w[2]),
                lines: w[4].parse().unwrap(),
                bytes: w[6].parse().unwrap(),
                digest: hex(w[8]),
                full: None,
            });
        }
    }
    out
}

fn entry_text(name: &str, input: &str, trace: &str, full: bool) -> String {
    let mut s = format!(
        "script {name} input {:016x} lines {} bytes {} fnv64 {:016x}\n",
        fnv(input.as_bytes(), 0xcbf2_9ce4_8422_2325),
        trace.lines().count(),
        trace.len(),
        fnv(trace.as_bytes(), 0xcbf2_9ce4_8422_2325)
    );
    if full {
        for l in trace.lines() {
            let _ = writeln!(s, "|{l}");
        }
    }
    s
}

fn same_trace(e: &Entry, trace: &str) -> bool {
    e.lines == trace.lines().count()
        && e.bytes == trace.len()
        && e.digest == fnv(trace.as_bytes(), 0xcbf2_9ce4_8422_2325)
}

/// The golden's entry for a script against the script's own digest: the inputs are the pinned ones.
fn entry_for<'a>(golden: &'a [Entry], name: &str, input: &str) -> Result<&'a Entry, String> {
    let e = golden
        .iter()
        .find(|e| e.name == name)
        .ok_or(format!("{name}: not in the golden"))?;
    if e.input == fnv(input.as_bytes(), 0xcbf2_9ce4_8422_2325) {
        Ok(e)
    } else {
        Err(format!("{name}: the script differs from the golden's input digest"))
    }
}

/// The C library's live trace against the golden: equal in every byte (digest, and the full text
/// where stored).
fn golden_equals_c(e: &Entry, trace: &str) -> Result<(), String> {
    if !same_trace(e, trace) {
        return Err(format!(
            "{}: the C trace differs from the golden (size or digest)",
            e.name
        ));
    }
    match &e.full {
        Some(f) if f.iter().map(String::as_str).ne(trace.lines()) => {
            Err(format!("{}: the C trace differs from the golden's text", e.name))
        }
        _ => Ok(()),
    }
}

/// A Rust trace against the golden: the same, or the same but for the one allowed difference
/// (the golden then holds the full text). `Ok(n)`: lines allowed.
fn golden_vs_rust(e: &Entry, trace: &str) -> Result<usize, String> {
    if same_trace(e, trace) {
        return Ok(0);
    }
    let Some(full) = &e.full else {
        return Err(format!(
            "{}: the Rust trace differs from the golden and the golden has no text to allow against",
            e.name
        ));
    };
    let g: Vec<&str> = full.iter().map(String::as_str).collect();
    let r: Vec<&str> = trace.lines().collect();
    compare_allowing_text_cap(&g, &r)
        .map_err(|(c, j)| format!("{}: differs at golden line {} / Rust line {}", e.name, c + 1, j + 1))
}

#[test]
fn rust_shim_equals_the_golden_of_the_c_guard() {
    let Some(rust) = rust_lib() else {
        return skip("no Rust libsai_teralynx_shim.so (IFCS_SHIM_LIB)");
    };
    let dir = std::env::temp_dir().join(format!("ifcs-shim-gold-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (harness, scripts) = harness_and_scripts(&dir);
    let golden = parse_golden(GOLDEN);
    assert_eq!(golden.len(), scripts.len(), "the golden covers every script");
    let (mut allowed, mut full) = (0, 0);
    for (name, path) in &scripts {
        let e = entry_for(&golden, name, &std::fs::read_to_string(path).unwrap()).unwrap();
        let r = run(&harness, &rust, path, &dir.join("work-rust"));
        allowed += golden_vs_rust(e, &r.trace).unwrap();
        full += usize::from(e.full.is_some());
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(allowed, ALLOWED_LINES, "the one deliberate difference: its lines");
    eprintln!(
        "ifcs-shim golden: {} scripts, the Rust traces equal the C guard's golden ({full} with full text; {allowed} lines by the one deliberate difference)",
        scripts.len()
    );
}

/// Negative control: a changed digest, a changed byte of a stored trace, a missing entry and a
/// changed input are each reported; the allow rule does not cover them.
#[test]
fn the_guard_golden_comparison_reports_a_changed_golden() {
    let golden = parse_golden(GOLDEN);
    assert!(golden.len() > 400, "the golden has the scripts");
    let with_text = golden
        .iter()
        .find(|e| e.full.is_some())
        .expect("a script with stored text");
    let trace = with_text.full.as_ref().unwrap().join("\n") + "\n";
    assert!(golden_equals_c(with_text, &trace).is_ok(), "the golden equals itself");
    // one byte of the trace changed
    let bad = trace.replacen("rc ", "rd ", 1).replacen("end: ", "enD: ", 1);
    assert_ne!(bad, trace);
    assert!(golden_equals_c(with_text, &bad).is_err());
    assert!(golden_vs_rust(with_text, &bad).is_err());
    // a changed digest in the golden
    let plain = golden.iter().find(|e| e.full.is_none()).unwrap();
    let flipped = Entry {
        name: plain.name.clone(),
        input: plain.input,
        lines: plain.lines,
        bytes: plain.bytes,
        digest: plain.digest ^ 1,
        full: None,
    };
    let other = "x\n";
    assert!(golden_vs_rust(&flipped, other).is_err());
    assert!(entry_for(&golden, "gen-000", "not the script").is_err());
    assert!(entry_for(&golden, "no-such-script", "").is_err());
}

/// The lines of the one deliberate difference in the whole corpus (see the golden's header).
const ALLOWED_LINES: usize = 6;

#[test]
fn rust_shim_equals_the_c_shim() {
    let Some(src) = c_src() else {
        return skip("no ifcs_guard.c / shim.c (IFCS_SHIM_C_SRC)");
    };
    let Some(rust) = rust_lib() else {
        return skip("no Rust libsai_teralynx_shim.so (IFCS_SHIM_LIB)");
    };
    let dir = std::env::temp_dir().join(format!("ifcs-shim-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let c_lib = dir.join("c/libsai-teralynx-shim.so");
    std::fs::create_dir_all(c_lib.parent().unwrap()).unwrap();
    // exactly the Makefile's rule for build/libsai-teralynx-shim.so (CFLAGS -O2 -g)
    let built = cc(&[
        "-std=gnu11",
        "-fPIC",
        "-shared",
        "-Wall",
        "-fno-builtin",
        "-fno-tree-loop-distribute-patterns",
        "-O2",
        "-g",
        "-Wl,-soname,libsai-teralynx-shim.so",
        "-o",
    ]
    .map(std::ffi::OsStr::new)
    .iter()
    .copied()
    .chain([
        c_lib.as_os_str(),
        src.join("shim.c").as_os_str(),
        src.join("ifcs_guard.c").as_os_str(),
        std::ffi::OsStr::new("-lpthread"),
    ])
    .collect::<Vec<_>>());
    if !built {
        return skip("the C shim does not compile (CC)");
    }
    let (harness, scripts) = harness_and_scripts(&dir);

    let (mut steps, mut lines, mut logs, mut proms, mut reads) = (0_usize, 0_usize, 0_usize, 0_usize, 0_usize);
    let mut allowed = 0_usize;
    let (mut alloc_c, mut alloc_r, mut tls) = (0_u64, 0_u64, 0_u64);
    let mut failures = Vec::new();
    let golden = parse_golden(GOLDEN);
    let capture = std::env::var_os("IFCS_SHIM_CAPTURE").map(PathBuf::from);
    assert!(
        capture.is_some() || golden.len() == scripts.len(),
        "the golden covers every script"
    );
    let capture_all = std::env::var_os("IFCS_SHIM_CAPTURE_ALL").is_some();
    let mut captured = String::new();
    let mut golden_allowed = 0;
    for (name, path) in &scripts {
        let c = run(&harness, &c_lib, path, &dir.join("work-c"));
        let r = run(&harness, &rust, path, &dir.join("work-rust"));
        let input = std::fs::read_to_string(path).unwrap();
        if capture.is_some() {
            // the C trace only; the text is kept for the corpus and where the Rust trace differs
            captured += &entry_text(
                name,
                &input,
                &c.trace,
                capture_all || !name.starts_with("gen-") || c.trace != r.trace,
            );
        }
        // live == golden: the C library's trace equals the golden, and the Rust one agrees with it
        if capture.is_none() {
            let entry = entry_for(&golden, name, &input).unwrap();
            golden_equals_c(entry, &c.trace).unwrap();
            golden_allowed += golden_vs_rust(entry, &r.trace).unwrap();
        }
        if c.trace != r.trace {
            let (cl, rl): (Vec<&str>, Vec<&str>) = (c.trace.lines().collect(), r.trace.lines().collect());
            match compare_allowing_text_cap(&cl, &rl) {
                Ok(n) => allowed += n,
                Err((k, j)) => {
                    let ctx =
                        |v: &[&str], k: usize| v[k.saturating_sub(3).min(v.len())..(k + 3).min(v.len())].join("\n");
                    failures.push(format!(
                        "{name}: traces differ at line {} (C) / {} (Rust)\n--- C\n{}\n--- Rust\n{}",
                        k + 1,
                        j + 1,
                        ctx(&cl, k),
                        ctx(&rl, j)
                    ));
                    continue;
                }
            }
        }
        assert!(c.trace.contains("\nend: "), "{name}: no end line");
        steps += c
            .trace
            .lines()
            .filter(|l| !l.starts_with("  ") && !l.starts_with("end:"))
            .count();
        lines += c.trace.lines().count();
        logs += c
            .trace
            .lines()
            .filter(|l| l.starts_with("  log "))
            .map(|l| l.matches("\\n").count())
            .sum::<usize>();
        proms += c.trace.lines().filter(|l| l.starts_with("  prom ")).count();
        reads += c
            .trace
            .lines()
            .filter(|l| l.starts_with("  calls "))
            .map(|l| l.matches(" R").count() + usize::from(l.starts_with("  calls R")))
            .sum::<usize>();
        assert!(
            !c.trace.contains("NOT in do_malloc")
                && !c.trace.contains("NOT ZEROED")
                && !c.trace.contains("WRONG")
                && !c.trace.contains("OTHER"),
            "{name}: a harness check failed:\n{}",
            c.trace
        );
        assert_eq!(c.allocs.len(), r.allocs.len(), "{name}");
        for ((op, ca, ct), (_, ra, rt)) in c.allocs.iter().zip(&r.allocs) {
            // init copies the inject variable (std::env::var_os; the C's getenv does not copy): the
            // one place the Rust allocates more, once per create_switch. Writing the counters' file,
            // the C's fopen allocates its FILE and std::fs nothing.
            if ra > ca && op != "init" {
                failures.push(format!("{name}: a {op} step allocated {ra} times in Rust, {ca} in C"));
            }
            alloc_c += ca;
            alloc_r += ra;
            tls += ct + rt;
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} scripts differ:\n{}",
        failures.len(),
        scripts.len(),
        failures.join("\n\n")
    );
    assert_eq!(tls, 0, "a thread-local destructor was registered");
    if let Some(out) = capture {
        std::fs::write(out, captured).unwrap();
    } else {
        assert_eq!(golden_allowed, ALLOWED_LINES, "the golden's allowed lines");
        eprintln!(
            "ifcs-shim live == golden: {} C traces equal the golden, the Rust ones agree with it ({golden_allowed} allowed lines)",
            scripts.len()
        );
    }
    eprintln!(
        "ifcs-shim differential: {allowed} trace lines differ by the one deliberate difference (counters' text of at least \
         {C_TEXT_CAP} bytes: written / answered by Rust, not by the C)"
    );
    eprintln!(
        "ifcs-shim differential: {} scripts ({} corpus + {GENERATED} generated), {steps} steps, {lines} trace lines equal \
         ({reads} original-accessor reads, {logs} journal lines, {proms} file versions); allocations in steps: C {alloc_c}, \
         Rust {alloc_r}; thread-local destructors registered: 0",
        scripts.len(),
        scripts.len() - usize::try_from(GENERATED).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

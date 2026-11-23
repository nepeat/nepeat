//! Differential test against the C++ tools of sai-openbcm.
//!
//! `PHY_SEQ_CXX_EXT_MACSEC` and `PHY_SEQ_CXX_EXT_PHY` name the built `openbcm-ext-macsec` and
//! `openbcm-ext-phy` (or they are files of those names in `$SAI_RS_TEST_BIN`, the directory of
//! prebuilt helpers on a machine without a compiler). Every table of a corpus, of
//! `PHY_SEQ_TABLES` (a colon-separated list of table files, e.g. a board's real tables) and
//! generated ones is given to the C++ tool and to this crate's (`phy_seq::cli`, in process):
//! exit code, standard output and standard error of `--check` must be equal byte for byte, and
//! for a table that parses, of `--render` of its sequences without parameters, with parameters,
//! and with parameters and recorded reads. Without the variables the tests say so and pass.
//!
//! Nothing is compiled here. Files are written under `$SAI_RS_TEST_TMPDIR` when set, else
//! under cargo's directory for test files.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use phy_seq::{ExtPhyTable, MacsecTable, ParamName};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

thread_local! {
    /// Comparisons made by this test (every test runs on its own thread).
    static COMPARISONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
static FILE_NO: AtomicU64 = AtomicU64::new(0);

/// A helper executable: from the variable, else by name from `$SAI_RS_TEST_BIN`.
fn helper(var: &str, name: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(var) {
        return Some(PathBuf::from(path));
    }
    let in_bin = PathBuf::from(std::env::var_os("SAI_RS_TEST_BIN")?).join(name);
    in_bin.is_file().then_some(in_bin)
}

fn cxx(var: &str, name: &str) -> Option<PathBuf> {
    let found = helper(var, name);
    if found.is_none() {
        eprintln!(
            "differential: {var} is not set (and no {name} in SAI_RS_TEST_BIN), the comparison with the C++ {name} is SKIPPED"
        );
    }
    found
}

fn scratch() -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("phy-seq-differential-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(content: &[u8]) -> PathBuf {
    let path = scratch().join(format!("f{}", FILE_NO.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, content).unwrap();
    path
}

type Output = (Option<i32>, Vec<u8>, Vec<u8>);

fn run_cxx(exe: &Path, args: &[OsString]) -> Output {
    let out = Command::new(exe)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
    (out.status.code(), out.stdout, out.stderr)
}

fn run_rust(
    tool: fn(&[OsString], &mut dyn std::io::Write, &mut dyn std::io::Write) -> u8,
    args: &[OsString],
) -> Output {
    let mut full = vec![OsString::from("rs")];
    full.extend_from_slice(args);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tool(&full, &mut out, &mut err);
    (Some(i32::from(code)), out, err)
}

fn show(o: &Output) -> String {
    format!(
        "exit {:?}\n--- stdout\n{}\n--- stderr\n{}",
        o.0,
        String::from_utf8_lossy(&o.1),
        String::from_utf8_lossy(&o.2)
    )
}

fn same(what: &str, cxx: &Output, rust: &Output) -> Result<(), String> {
    COMPARISONS.set(COMPARISONS.get() + 1);
    if cxx == rust {
        Ok(())
    } else {
        Err(format!(
            "{what}: the tools disagree\n=== C++: {}\n=== Rust: {}",
            show(cxx),
            show(rust)
        ))
    }
}

fn os(args: &[&std::ffi::OsStr]) -> Vec<OsString> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// Parameter files: every parameter in full, with bytes that tell the parameters apart; and a
/// short key (16 bytes) with odd spellings the C++ reader accepts.
fn param_files() -> [Vec<u8>; 2] {
    let mut full = String::new();
    for (n, name) in (1u8..).zip(ParamName::ALL) {
        write!(full, "{name} ").unwrap();
        for i in 0..name.max_len() {
            write!(full, "{:02x}", 0x10 * n + i).unwrap();
        }
        full.push('\n');
    }
    let short = b"key 000102030405060708090A0b0c0d0e0f\n  sci 1z+2-1 0x 3\nunknown 00\nhkey 0f0e0d0c0b0a09080706050403020100ffee pn 0102\nkey 0f0e0d0c0b0a0908070605040302010\n".to_vec();
    [full.into_bytes(), short]
}

/// `--check` of one ext-macsec table, and `--render` of up to `max_seqs` of its sequences.
fn compare_macsec(exe: &Path, text: &[u8], max_seqs: usize, seed: u64) -> Result<(), String> {
    let file = put(text);
    let f = file.as_os_str();
    let check = os(&["--check".as_ref(), f]);
    same(
        "--check",
        &run_cxx(exe, &check),
        &run_rust(phy_seq::cli::ext_macsec, &check),
    )?;
    let Ok(table) = MacsecTable::parse(text) else {
        let _ = std::fs::remove_file(&file);
        return Ok(());
    };
    let [full, short] = param_files().map(|p| put(&p));
    // recorded reads: a few that make comparisons pass now and then, written the ways strtoul reads
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let reads: String = (0..64)
        .map(|i| match i % 7 {
            0 => "0 ".to_owned(),
            1 => format!("0x{:x} ", next() & 0xffff),
            2 => "ffff ".to_owned(),
            _ => format!("{:x}\n", next() & 0xffff),
        })
        .collect();
    let reads = put(reads.as_bytes());
    for (name, _) in table.sequences().take(max_seqs) {
        let phy = format!("{:x}", next() & 0xffff);
        let variants: [Vec<&std::ffi::OsStr>; 5] = [
            vec![],
            vec!["-".as_ref()],
            vec![full.as_os_str()],
            vec![short.as_os_str()],
            vec![full.as_os_str(), reads.as_os_str()],
        ];
        for extra in variants {
            let mut args = os(&["--render".as_ref(), f, name.as_ref(), phy.as_ref()]);
            args.extend(extra.iter().map(|a| (*a).to_owned()));
            same(
                &format!("--render {name} {phy} (+{} files)", extra.len()),
                &run_cxx(exe, &args),
                &run_rust(phy_seq::cli::ext_macsec, &args),
            )?;
        }
    }
    for path in [file, full, short, reads] {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

fn compare_extphy(exe: &Path, text: &[u8]) -> Result<(), String> {
    let file = put(text);
    let check = os(&["--check".as_ref(), file.as_os_str()]);
    let verdict = same(
        "--check",
        &run_cxx(exe, &check),
        &run_rust(phy_seq::cli::ext_phy, &check),
    );
    let _ = std::fs::remove_file(&file);
    verdict
}

const MACSEC_CORPUS: &[&[u8]] = &[
    b"",
    b"\n",
    b"ext-phy 1\n",
    b"ext-macsec 2\n",
    b"ext-macsec 1",
    b"ext-macsec 1\n",
    b"\n# c\n\text-macsec\x0b1\x0c\r\ncounters absolute\r\nifg-bits 2040\r\nport 100000 ffff\r\nseq a\r\nD 100000\r\n",
    b"ext-macsec 1\nW p 1 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 0 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 20 a000 6\n",
    b"ext-macsec 1\nseq a\nW q 1 a000 6\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 10000\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 key[32].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 hkey[16].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 secret[0].00\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 0.key[0]\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 $16\n",
    b"ext-macsec 1\nseq a\nW p 1 a000 $0\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 $16\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 pn[8]\n",
    b"ext-macsec 1\nseq a\nX p 1 a000 pn\n",
    b"ext-macsec 1\nseq a\nE p 1 a000 3 1\n",
    b"ext-macsec 1\nseq a\nP p 1 a000 1 1 0 0\n",
    b"ext-macsec 1\nseq a\nD 200000\n",
    b"ext-macsec 1\nseq a\nC p 1 a000 1 1\n",
    b"ext-macsec 1\nseq a\nseq a\n",
    b"ext-macsec 1\nseq A\n",
    b"ext-macsec 1\nport 1 10\nport 1 11\n",
    b"ext-macsec 1\nport 1\n",
    b"ext-macsec 1\ncounters sometimes\n",
    b"ext-macsec 1\nifg-bits 9999\n",
    b"ext-macsec 1\ncounters absolute\nseq a\nW 10 1 a000 $0|1\n",
    // the corners of strtoul and of std::string
    b"ext-macsec 1\nport 007 0x1F\nport 7 2\n",
    b"ext-macsec 1\nport 1\x00junk 10\nport 1 11\n",
    b"ext-macsec 1\nport \x00 \x00\nseq s\nW \x00 1\x00 \x00 \x00\nW p 1 0 \x00\x00.1\x00\nX p 1 0 $\x00\nW p 1 1 $\x00&\x00|\x00\nD \x00\nP p 1 0 \x00 \x00 1\x00 \x00\n",
    b"ext-macsec 1\nseq s\nW 0x10 0x1 0xa000 0xffff\nW p 0X1f 0XFFFF 0X0\nE p 1 0 0x1 0x1\nR 0x 1 0\n",
    b"ext-macsec 1\nseq s\nR p 1 0x\n",
    b"ext-macsec 1\nseq s\nD 0x10\n",
    b"ext-macsec 1\n\xff\xfe\x00 1\n",
    b"ext-macsec 1\nseq s\nW p 1 0 00.0x\n",
    b"ext-macsec 1\nseq s\nW p 1 0 $0|1&2\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $1\nW p 1 0 $1&&1\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $1\nW p 1 0 $1.2\nW p 1 0 $1&ff|1|2\n",
    b"ext-macsec 1\nseq s\nX p 1 0 $01\nW p 1 0 $1\nW p 1 0 $001&0ff|0x1\n",
    b"ext-macsec 1\nseq s\nX p 1 0 a[0]]\n",
    b"ext-macsec 1\nseq s\nX p 1 0 [0]\n",
    b"ext-macsec 1\nseq s\nX p 1 0 a[07]\nX p 1 0 _[7]\nX p 1 0 $3[1]\n",
    b"ext-macsec 1\nseq b\nW p 1 0 $1\nseq a\nX p 1 0 $2\nW p 1 0 $3|1\n",
    b"ext-macsec 1\nseq s\nP p 1 8221 12e3 ffff 3 5\nE p 1 a100 2 f\nX p 1 1 r[2]\nX 5 1f ffff $15\nW 1e0 1 2 $15&ff00|1\nD 0\nP 2 1 3 0 0 2 0\n",
    b"ext-macsec 1\nseq s\nW p 1 1 key[1].key[0]\nW p 1 2 key[31].key[30]\nW p 1 3 00.sci[7]\nW p 1 4 iv[11].window[3]\n",
];

const EXTPHY_CORPUS: &[&[u8]] = &[
    b"",
    b"mdio-script 1\n",
    b"ext-phy 2\n",
    b"ext-phy 1\n",
    b"ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\nport 1\nlink\n  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n  C 10 1 a101 3 3\nadmin-down\n  W 10 1 d139 351\nadmin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\nport 5\nlink\n  C 11 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 0 1 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 20 1 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 10000 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 1\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 6\n",
    b"ext-phy 1\nport 1\nadmin-down\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\n",
    b"ext-phy 1\nW 10 1 a000 6\n",
    b"ext-phy 1\nport 1\nW 10 1 a000 6\n",
    b"ext-phy 1\nlink\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nport 1\nlink\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\nlink\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport x\n",
    b"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 0 0\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nP 10 1 8221 11e3 ffff 5 200000\nC 10 1 a101 3 3\n",
    b"ext-phy 1\nport 1\nlink\nX 10 1 8221\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 zz\n",
    b"ext-phy 1\nport 1\nlink\nW 10 1 a000 -6\n",
    b"ext-phy 1\npoll-ms 70000\n",
    b"ext-phy 1\nup-polls 0\n",
    b"ext-phy 1\nport 1\nlink\nC 10 1 a101 3 3\npoll-ms 100\n",
    b"ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 0 0\n",
    b"ext-phy 1\nport 1\nlink\nP 1 1 1 2 1 1 0\n",
    b"ext-phy 1\nport 3\nadmin-up\nR 1 1 1\n\nport 03\n",
    b"ext-phy 1\nport 3\x00x\nadmin-up\nR 1 1 1\n\nport 3\x00y\n",
    b"ext-phy 1\nport 1\nport 2\nlink\nR 1 1 1\n",
    b"ext-phy 1\nport 1\nlink\nlink\nC 0x1 0x1 0x1 0x1 0x1\nadmin-up\nadmin-up\nW 1 1 1 1\nadmin-up\n",
    b"ext-phy 1\nport 1\nadmin-down now\n",
    b"ext-phy 1\nholdoff-ms 600000\npoll-ms 60000\nup-polls 100\nholdoff-ms 0\nport 100000\nadmin-down\nR ffff 1f ffff\n",
    b"ext-phy 1\nport 1\nlink\nC 1 1 1 1 1\nport 2\nlink\nR 1 1 1\n",
];

fn board_tables() -> Vec<(PathBuf, Vec<u8>)> {
    let Some(list) = std::env::var_os("PHY_SEQ_TABLES") else {
        return Vec::new();
    };
    std::env::split_paths(&list)
        .map(|path| {
            let text = std::fs::read(&path).unwrap_or_else(|e| panic!("PHY_SEQ_TABLES: {}: {e}", path.display()));
            (path, text)
        })
        .collect()
}

#[test]
fn ext_macsec_corpus_and_board_tables() {
    let Some(exe) = cxx("PHY_SEQ_CXX_EXT_MACSEC", "openbcm-ext-macsec") else {
        return;
    };
    let before = COMPARISONS.get();
    for (i, text) in MACSEC_CORPUS.iter().enumerate() {
        compare_macsec(&exe, text, usize::MAX, i as u64)
            .unwrap_or_else(|e| panic!("corpus table {i} ({:?}): {e}", String::from_utf8_lossy(text)));
    }
    compare_macsec(&exe, common::ms_table(true, true).as_bytes(), usize::MAX, 99)
        .unwrap_or_else(|e| panic!("the test table: {e}"));
    let mut boards = 0;
    for (path, text) in board_tables() {
        if text.starts_with(b"ext-macsec") {
            assert!(MacsecTable::parse(&text).is_ok(), "{}: refused", path.display());
            compare_macsec(&exe, &text, usize::MAX, 7).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            boards += 1;
        }
    }
    // usage and files that cannot be opened: the same exit codes (the usage text names the program)
    let missing = scratch().join("no-such-file");
    let table = put(b"ext-macsec 1\nseq s\nW p 1 1 key[0].00\n");
    let cases: Vec<Vec<OsString>> = vec![
        vec![],
        os(&["--check".as_ref()]),
        os(&["--frob".as_ref(), table.as_os_str()]),
        os(&["--render".as_ref(), table.as_os_str(), "s".as_ref()]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1".as_ref(),
            "-".as_ref(),
            "-".as_ref(),
            "-".as_ref(),
        ]),
    ];
    for args in &cases {
        assert_eq!(
            run_cxx(&exe, args).0,
            run_rust(phy_seq::cli::ext_macsec, args).0,
            "{args:?}"
        );
        assert_eq!(run_rust(phy_seq::cli::ext_macsec, args).0, Some(2));
    }
    let cases: Vec<Vec<OsString>> = vec![
        os(&["--check".as_ref(), missing.as_os_str()]),
        os(&["--check".as_ref(), scratch().as_os_str()]),
        os(&["--render".as_ref(), table.as_os_str(), "nope".as_ref(), "1".as_ref()]),
        os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "1e0".as_ref()]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            missing.as_os_str(),
        ]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            "-".as_ref(),
            missing.as_os_str(),
        ]),
        os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "0x1E0".as_ref(),
            table.as_os_str(),
            table.as_os_str(),
        ]),
    ];
    for args in &cases {
        same(
            &format!("{args:?}"),
            &run_cxx(&exe, args),
            &run_rust(phy_seq::cli::ext_macsec, args),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }
    let n = COMPARISONS.get() - before;
    eprintln!(
        "differential: ext-macsec corpus: {n} comparisons with the C++ tool, {boards} board table(s), no disagreement"
    );
}

#[test]
fn ext_phy_corpus_and_board_tables() {
    let Some(exe) = cxx("PHY_SEQ_CXX_EXT_PHY", "openbcm-ext-phy") else {
        return;
    };
    let before = COMPARISONS.get();
    for (i, text) in EXTPHY_CORPUS.iter().enumerate() {
        compare_extphy(&exe, text)
            .unwrap_or_else(|e| panic!("corpus table {i} ({:?}): {e}", String::from_utf8_lossy(text)));
    }
    let mut boards = 0;
    for (path, text) in board_tables() {
        if text.starts_with(b"ext-phy") {
            assert!(ExtPhyTable::parse(&text).is_ok(), "{}: refused", path.display());
            compare_extphy(&exe, &text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            boards += 1;
        }
    }
    let missing = scratch().join("no-such-file");
    for args in [
        vec![],
        os(&["--render".as_ref(), missing.as_os_str()]),
        os(&["--check".as_ref(), missing.as_os_str(), "x".as_ref()]),
    ] {
        assert_eq!(
            run_cxx(&exe, &args).0,
            run_rust(phy_seq::cli::ext_phy, &args).0,
            "{args:?}"
        );
    }
    let args = os(&["--check".as_ref(), missing.as_os_str()]);
    same(
        "missing file",
        &run_cxx(&exe, &args),
        &run_rust(phy_seq::cli::ext_phy, &args),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let n = COMPARISONS.get() - before;
    eprintln!(
        "differential: ext-phy corpus: {n} comparisons with the C++ tool, {boards} board table(s), no disagreement"
    );
}

fn cases() -> u32 {
    std::env::var("PHY_SEQ_DIFF_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400)
}

#[test]
fn generated_ext_macsec_tables() {
    let Some(exe) = cxx("PHY_SEQ_CXX_EXT_MACSEC", "openbcm-ext-macsec") else {
        return;
    };
    let before = COMPARISONS.get();
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: cases(),
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&(common::macsec_text(), any::<u64>()), |(text, seed)| {
            compare_macsec(&exe, &text, 3, seed).map_err(TestCaseError::fail)
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let n = COMPARISONS.get() - before;
    eprintln!("differential: generated ext-macsec tables: {n} comparisons with the C++ tool, no disagreement");
}

#[test]
fn generated_ext_phy_tables() {
    let Some(exe) = cxx("PHY_SEQ_CXX_EXT_PHY", "openbcm-ext-phy") else {
        return;
    };
    let before = COMPARISONS.get();
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: cases() * 4,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&common::extphy_text(), |text| {
            compare_extphy(&exe, &text).map_err(TestCaseError::fail)
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let n = COMPARISONS.get() - before;
    eprintln!("differential: generated ext-phy tables: {n} comparisons with the C++ tool, no disagreement");
}

/// The binaries themselves (not only the library functions behind them): arguments, streams and
/// exit codes arrive. Needs no C++.
#[test]
fn the_rust_binaries_run() {
    let find =
        |name: &str, built: &str| helper("PHY_SEQ_NO_SUCH_VARIABLE", name).unwrap_or_else(|| PathBuf::from(built));
    let macsec = find("openbcm-ext-macsec-rs", env!("CARGO_BIN_EXE_openbcm-ext-macsec-rs"));
    let phy = find("openbcm-ext-phy-rs", env!("CARGO_BIN_EXE_openbcm-ext-phy-rs"));
    if !macsec.is_file() || !phy.is_file() {
        eprintln!(
            "differential: the Rust tools are not at {} / {}: running them is SKIPPED",
            macsec.display(),
            phy.display()
        );
        return;
    }
    let table = put(b"ext-macsec 1\nport 1 1e0\nseq s\nW p 1 a000 key[1].key[0]\nE 5 1f 2 3 f\nD 9\n");
    let params = put(b"key 1122\n");
    let out = run_cxx(&macsec, &os(&["--check".as_ref(), table.as_os_str()]));
    assert_eq!(out.0, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.1),
        format!(
            "{}: 1 ports, 1 sequences, 3 operations, counters clear-on-read, ifg-bits 0\n",
            table.display()
        )
    );
    let out = run_cxx(
        &macsec,
        &os(&[
            "--render".as_ref(),
            table.as_os_str(),
            "s".as_ref(),
            "1e0".as_ref(),
            params.as_os_str(),
        ]),
    );
    assert_eq!(
        (out.0, String::from_utf8_lossy(&out.1).as_ref()),
        (Some(0), "W 1e0 1 a000 2211\nR 005 1f 0002 0003 000f\nD 9\n")
    );
    let out = run_cxx(
        &macsec,
        &os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "1e0".as_ref()]),
    );
    assert_eq!(out.0, Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.2),
        format!("{}: s: line 4: parameter key[1] is not available\n", table.display())
    );
    assert_eq!(out.1, Vec::<u8>::new());
    let out = run_cxx(&macsec, &[]);
    assert_eq!(out.0, Some(2));
    assert!(String::from_utf8_lossy(&out.2).ends_with(" --check FILE | --render FILE SEQ PHY [PARAMS|- [READS]]\n"));
    let out = run_cxx(
        &macsec,
        &os(&["--render".as_ref(), table.as_os_str(), "s".as_ref(), "10000".as_ref()]),
    );
    assert_eq!(out.0, Some(2));
    let phy_table = put(b"ext-phy 1\nport 1\nlink\nC 1 1 1 1 1\nadmin-up\nW 1 1 1 1\nW 1 1 1 2\n");
    let out = run_cxx(&phy, &os(&["--check".as_ref(), phy_table.as_os_str()]));
    assert_eq!(out.0, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.1),
        format!(
            "{}: 1 ports (1 with a link poll), poll 500 ms, up after 2 good polls, hold-off 3000 ms; operations: link 1, admin-down 0, admin-up 2\n",
            phy_table.display()
        )
    );
    let out = run_cxx(&phy, &os(&["--check".as_ref(), table.as_os_str()]));
    assert_eq!(out.0, Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.2),
        format!(
            "{}: line 1: not an ext-phy table (first line must be 'ext-phy 1')\n",
            table.display()
        )
    );
    assert_eq!(run_cxx(&phy, &[]).0, Some(2));
}

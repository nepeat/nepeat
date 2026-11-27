//! The Python generator's output on the pinned ishell 2.0.1 ctypes as digests (H5-0 golden,
//! `golden/ishell-2.0.1.model.sha256`, header: sources, command, "NOT to be regenerated from the Rust
//! output"): the two headers and the model facts (43,268 of them), against ifcs-gen's output.
//! Runs when `IFCS_CTYPES` names the pinned file (package.nix sets it; otherwise it says so and skips).
//! The vendor-derived text itself is never stored: only digests, and nothing here writes it anywhere.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, reason = "a test harness")]

// the digest used by the generator's own header, included without widening the library's API
#[path = "../src/sha256.rs"]
#[allow(dead_code, clippy::pedantic, clippy::nursery, reason = "included as is")]
mod sha256;

use ifcs_gen::{Input, dump_model, generate, templates};

const MODEL_GOLDEN: &str = include_str!("golden/ishell-2.0.1.model.sha256");
/// The nix build's check file (unchanged): the header digests.
const HEADERS_GOLDEN: &str = include_str!("golden/ishell-2.0.1.sha256");
/// The pinned ctypes file (nix/ifcs-ctypes.nix).
const CTYPES_SHA: &str = "5a4547fb452a2b4fe85c449ed7089033f81d133f07646168ce01eac2a2843069";

/// (name, digest, lines) rows of a golden text.
fn rows(text: &str) -> Vec<(String, String, usize)> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut c = l.split_whitespace();
            let sha = c.next().expect("digest").to_owned();
            let name = c.next().expect("name").to_owned();
            let n = c.next().map_or(0, |n| n.parse().expect("line count"));
            (name, sha, n)
        })
        .collect()
}

/// Ok when the golden `text` holds `name` with this digest (and line count, when it has one).
fn check(text: &str, name: &str, sha: &str, lines: usize) -> Result<(), String> {
    let r = rows(text);
    let (_, want, n) = r
        .iter()
        .find(|(k, ..)| k == name)
        .ok_or(format!("{name}: not in the golden"))?;
    if want != sha {
        return Err(format!("{name}: sha256 {sha}, golden {want}"));
    }
    if *n != 0 && *n != lines {
        return Err(format!("{name}: {lines} lines, golden {n}"));
    }
    Ok(())
}

fn pinned() -> Option<(String, Vec<u8>)> {
    let Ok(path) = std::env::var("IFCS_CTYPES") else {
        eprintln!("SKIPPED: IFCS_CTYPES is not set (the pinned ishell 2.0.1 ifcs_ctypes.py)");
        return None;
    };
    let bytes = std::fs::read(&path).expect("IFCS_CTYPES readable");
    assert_eq!(
        sha256::hex(&bytes),
        CTYPES_SHA,
        "{path} is not the pinned ishell 2.0.1 ctypes"
    );
    Some((path, bytes))
}

fn headers(path: &str, bytes: &[u8]) -> Vec<(String, String)> {
    let g = generate(&Input {
        ctypes_path: path,
        ctypes: bytes,
        overrides: Some(templates::OVERRIDES),
        source: None,
        templates: &templates::TEMPLATES,
    })
    .expect("generate");
    assert_eq!(g.errors, Vec::<String>::new());
    g.outputs
}

fn group(text: &str, kind: &str) -> String {
    text.lines()
        .filter(|l| l.starts_with(kind))
        .map(|l| [l, "\n"].concat())
        .collect()
}

#[test]
fn headers_and_model_equal_the_pythons_golden() {
    let Some((path, bytes)) = pinned() else { return };
    let outs = headers(&path, &bytes);
    assert_eq!(outs.len(), 2);
    for (name, text) in &outs {
        check(MODEL_GOLDEN, name, &sha256::hex(text.as_bytes()), 0).unwrap();
    }
    let facts = dump_model(&path, &bytes).expect("dump");
    check(
        MODEL_GOLDEN,
        "model-facts.txt",
        &sha256::hex(facts.as_bytes()),
        facts.lines().count(),
    )
    .unwrap();
    for (k, name) in [
        ("v ", "model-facts-v"),
        ("a ", "model-facts-a"),
        ("f ", "model-facts-f"),
    ] {
        let g = group(&facts, k);
        check(MODEL_GOLDEN, name, &sha256::hex(g.as_bytes()), g.lines().count()).unwrap();
    }
}

/// The header digests of the new golden are the nix build's own (ishell-2.0.1.sha256).
#[test]
fn header_digests_agree_with_the_nix_golden() {
    let nix: Vec<(String, String)> = HEADERS_GOLDEN
        .lines()
        .map(|l| {
            let (sha, name) = l.split_once("  ").expect("sha256sum line");
            (name.to_owned(), sha.to_owned())
        })
        .collect();
    assert_eq!(nix.len(), 2);
    for (name, sha) in &nix {
        check(MODEL_GOLDEN, name, sha, 0).unwrap();
    }
    assert!(MODEL_GOLDEN.contains("NOT to be regenerated from the Rust output"));
    let all = rows(MODEL_GOLDEN);
    assert_eq!(all.len(), 6);
    // the groups add up to the whole
    let sum: usize = all
        .iter()
        .filter(|(k, ..)| k.starts_with("model-facts-"))
        .map(|r| r.2)
        .sum();
    assert_eq!(sum, 43_268);
    assert_eq!(all.iter().find(|r| r.0 == "model-facts.txt").unwrap().2, 43_268);
}

/// Negative control: a changed digest in the golden, or one changed byte of the generated header or
/// of the model facts, is reported.
#[test]
fn golden_negative_control() {
    let want = rows(MODEL_GOLDEN);
    // the golden itself, one hex digit changed in memory
    for (name, sha, n) in &want {
        assert!(check(MODEL_GOLDEN, name, sha, *n).is_ok(), "{name}: unchanged matches");
        let mut bad = sha.clone().into_bytes();
        bad[0] = if bad[0] == b'0' { b'1' } else { b'0' };
        let mutated = MODEL_GOLDEN.replacen(sha.as_str(), &String::from_utf8(bad).unwrap(), 1);
        assert!(
            check(&mutated, name, sha, *n).is_err(),
            "{name}: a changed golden digest must be reported"
        );
        assert!(check(MODEL_GOLDEN, name, &sha256::hex(b"other"), *n).is_err());
    }
    assert!(
        check(MODEL_GOLDEN, "model-facts.txt", &want[2].1, 43_267).is_err(),
        "a lost fact must be reported"
    );
    assert!(check(MODEL_GOLDEN, "ifcs_nope.h", "0", 0).is_err());
    // the actual outputs, one byte changed (needs the pinned file)
    let Some((path, bytes)) = pinned() else { return };
    let outs = headers(&path, &bytes);
    let mut h = outs[0].1.clone().into_bytes();
    let mid = h.len() / 2;
    h[mid] ^= 1;
    assert!(
        check(MODEL_GOLDEN, &outs[0].0, &sha256::hex(&h), 0).is_err(),
        "a changed header byte must be reported"
    );
    let facts = dump_model(&path, &bytes).expect("dump");
    let altered = facts.replacen("v ", "v X", 1);
    assert!(
        check(
            MODEL_GOLDEN,
            "model-facts.txt",
            &sha256::hex(altered.as_bytes()),
            43_268
        )
        .is_err()
    );
}

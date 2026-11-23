//! Golden test: this crate against the C interpreter `mdio_script.c` as it was when it was deleted.
//!
//! Until 2026-11-23 this file compiled `mdio_script.c` with a driver (`differential_driver.c`)
//! and compared its canonical output with this crate's on the same fake bus (tests/common/mod.rs)
//! for every script and job (7253 comparisons per run). Those C outputs, captured at commit
//! 50a6aa7 (sha256 of the C files in the golden headers), are now the reference:
//!
//! - `golden/corpus.txt`: the hand-written corpus below, every job, the full C output text;
//!   each script's FNV-1a digest is stored too, so an edited corpus is reported as such;
//! - `golden/generated.txt`: 400 generated valid scripts (parse, run, verify, verify-only) and
//!   600 damaged ones (parse pass), drawn from a fixed proptest seed; per case the digest of the
//!   input, the job and the 64-bit FNV-1a digest of the C output. A changed generator shows as
//!   "golden inputs differ", a changed behaviour as an output digest mismatch that prints the case.
//!
//! The goldens are not regenerated from this crate's output: a deliberate behaviour change edits
//! the affected entries by hand with a reason. Nothing is compiled or run besides this crate, so
//! the test executable runs as it is where the source tree is not (README "Big-endian runs").
//!
//! Scripts with a syntax error are compared on the parse pass only: the C interpreter would run
//! the lines before the error, this crate runs nothing (see the crate documentation).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::collections::HashMap;

use common::{Job, Mode, esc, rust_output};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};

const CORPUS: &str = include_str!("golden/corpus.txt");
const GENERATED: &str = include_str!("golden/generated.txt");

/// The seed of the generated cases (proptest's `ChaCha` RNG): the golden holds what it drew.
const SEED: [u8; 32] = *b"mdio-script differential golden!";

/// 64-bit FNV-1a.
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn mode(word: &str) -> Mode {
    match word {
        "parse" => Mode::Parse,
        "run" => Mode::Run,
        "verify" => Mode::Verify,
        "only" => Mode::Only,
        _ => panic!("golden: unknown mode {word:?}"),
    }
}

/// `<mode> <seed> <tick> <abort_k>`
fn job(words: &[&str]) -> Job {
    let [m, seed, tick, abort_k] = words else {
        panic!("golden: bad job {words:?}")
    };
    Job {
        mode: mode(m),
        seed: seed.parse().unwrap(),
        tick: match *tick {
            "0" => false,
            "1" => true,
            _ => panic!("golden: bad tick {tick:?}"),
        },
        abort_k: abort_k.parse().unwrap(),
    }
}

fn job_text(job: Job) -> String {
    format!("{} {} {} {}", job.mode.arg(), job.seed, u8::from(job.tick), job.abort_k)
}

fn hex64(word: &str) -> u64 {
    u64::from_str_radix(word, 16).unwrap_or_else(|e| panic!("golden: bad digest {word:?}: {e}"))
}

/// One corpus script of the golden: its digest and its jobs with the id of the C output.
struct GoldenScript {
    digest: u64,
    jobs: Vec<(Job, usize)>,
}

/// `golden/corpus.txt`: `script <i> <digest>`, its `job <mode> <seed> <tick> <abort_k> <out>`
/// lines, then each distinct output once: `out <id>` and its lines, each prefixed by `|`.
fn golden_corpus() -> (Vec<GoldenScript>, HashMap<usize, String>) {
    let (mut scripts, mut outputs) = (Vec::<GoldenScript>::new(), HashMap::<usize, String>::new());
    let mut current = None;
    for line in CORPUS.lines() {
        if let Some(text) = line.strip_prefix('|') {
            let id = current.unwrap_or_else(|| panic!("golden: an output line outside an out block"));
            let out: &mut String = outputs.get_mut(&id).unwrap();
            out.push_str(text);
            out.push('\n');
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] => {}
            [w, ..] if w.starts_with('#') => {}
            ["script", i, digest] => {
                assert_eq!(
                    i.parse::<usize>().unwrap(),
                    scripts.len(),
                    "golden: scripts out of order"
                );
                scripts.push(GoldenScript {
                    digest: hex64(digest),
                    jobs: Vec::new(),
                });
            }
            ["job", m, seed, tick, abort_k, out] => {
                scripts
                    .last_mut()
                    .unwrap_or_else(|| panic!("golden: a job before any script"))
                    .jobs
                    .push((job(&[m, seed, tick, abort_k]), out.parse().unwrap()));
            }
            ["out", id] => {
                let id: usize = id.parse().unwrap();
                assert!(outputs.insert(id, String::new()).is_none(), "golden: out {id} twice");
                current = Some(id);
            }
            _ => panic!("golden: unexpected line {line:?}"),
        }
    }
    (scripts, outputs)
}

/// A generated case of `golden/generated.txt`: `<kind> <case> <input digest> <mode> <seed>
/// <tick> <abort_k> <output digest>`.
struct GoldenCase {
    case: usize,
    input: u64,
    job: Job,
    output: u64,
}

fn golden_generated(kind: &str) -> Vec<GoldenCase> {
    GENERATED
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let [k, case, input, m, seed, tick, abort_k, output] = words.as_slice() else {
                panic!("golden: unexpected line {line:?}")
            };
            (*k == kind).then(|| GoldenCase {
                case: case.parse().unwrap(),
                input: hex64(input),
                job: job(&[m, seed, tick, abort_k]),
                output: hex64(output),
            })
        })
        .collect()
}

fn verify(phy: &str) -> String {
    format!(
        "L verify phy 0x{phy}\nP {phy} 1 8b00 0 3 1 0\nN {phy} 1 2 1 ffff 1 0\n# note\nP {phy} 1f ffff 0303 0303 1 0\n"
    )
}

/// Hand-written scripts: valid ones, each error class, verification sections.
fn corpus() -> Vec<String> {
    let body = "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 9 10\nW 1 1 1 1\n";
    let mut scripts: Vec<String> = [
        // valid, no verification section
        "mdio-script 1\n",
        "mdio-script 1",
        "mdio-script 1\r\n# c\r\nL step one\r\nW 17c 1 8201 7e\r\nD 30\r\nR 17c 1 900a\r\n",
        "mdio-script 1\n# c\nL step one\nW 17c 1 8201 7e\nD 30\nR 17c 1 900a\nE 17c 1 8b00 2391\nE 17c 1 8b00 0391 0fff\n\
         E 17c 1 8b00 1111\n\nW 0 1f ffff ffff\nE 0 1f ffff ffff\nE 0 1f ffff 00ff 00ff\n",
        "mdio-script 1\nL\nL \nL  spaced  \nW 0x1 0X1f 0002 3\n \t W\t1  1 2\t4 \nR 1 1 2\nE 1 1 2 4\nE 1 1 2 5\n",
        "mdio-script 1\nD 0\nD 499999\nD 500000\nD 500001\nD 1700000\nD 60000000\nW 1 1 2 3\n",
        "mdio-script 1\nP 24 1 8221 0303 ffff 10 1000\nN 24 1 8221 0303 ffff 3 500\nP 1 1 8221 0303 0303 1 0\nW 1 1 2 3\n",
        "mdio-script 1\nL slow poll\nP 1 1 8221 0303 ffff 9 1200000\nN 1 1 8221 0303 ffff 2 0\nW 1 1 2 3\n",
        // E_POLL, E_IO (read, write, inside a poll), mismatch counting beyond the 20 logged
        "mdio-script 1\nL download bus 1\nP 24 1 8b00 0303 ffff 5 10\nW 24 1 1 1\n",
        "mdio-script 1\nL never\nN 24 1 2 1900 ffff 4 600000\nW 24 1 1 1\n",
        "mdio-script 1\nL assert\nW 1 1 2 3\nP 1 1 2 4 ffff 1 0\nW 1 1 1 1\n",
        "mdio-script 1\nW 1 1 2 3\nR 24 1 dead\nW 24 1 1 1\n",
        "mdio-script 1\nL e\nE 26 1 2 3 ff\nW 24 1 1 1\n",
        "mdio-script 1\nL w\nW 1 1 2 3\nW 1 3 bad0 77\nW 24 1 1 1\n",
        "mdio-script 1\nL p\nP 26 1 2 3 ffff 5 10\nW 24 1 1 1\n",
        // syntax errors (parse pass only)
        "",
        "\n",
        "W 1 1 2 3\n",
        "mdio-script 2\n",
        "mdio-script 1\nW 1 0 2 3\n",
        "mdio-script 1\nW 1 20 2 3\n",
        "mdio-script 1\nW 1 1 2\n",
        "mdio-script 1\nW 1 1 2 10000\n",
        "mdio-script 1\nW 1 1 2 3 4\n",
        "mdio-script 1\nX 1 1 2 3\n",
        "mdio-script 1\nP 1 1 2 3 ffff 0 5\n",
        "mdio-script 1\nP 1 1 2 3 ffff 1000001 5\n",
        "mdio-script 1\nP 1 1 2 3 ffff 5 60000001\n",
        "mdio-script 1\nP 1 1 2 3 ffff 5 6 7 8 9 10\n",
        "mdio-script 1\nD -5\n",
        "mdio-script 1\nD +5\n",
        "mdio-script 1\nD 60000001\n",
        "mdio-script 1\nD 0x10\n",
        "mdio-script 1\nW 0x 1 2 3\n",
        "mdio-script 1\nW 1 1 2 3\nW 1 1 zz 3\n",
        "mdio-script 1\nL a\nW 1 1 2 3\nD 5\nbogus\nW 1 1 4 5\n",
        "mdio-script 1\nE 1 1 2\n",
        "mdio-script 1\nE 1 1 2 3 4 5\n",
        "mdio-script 1\nL\tx\n",
        "mdio-script 1\n # c\n",
        "mdio-script 1\nW 1 1 2 3\0\nW 1 1 2 4\n",
        "mdio-script 1\n\0W 1 1 2 3\nW 1 1 2 4\n",
        "mdio-script 1\nW 1 1 2 3\0",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    // 25 mismatches: 20 logged
    scripts.push(format!("mdio-script 1\nL many\n{}", "E 24 1 2 5 ff\n".repeat(25)));
    // long label (cut to 150 bytes; 60 in the verification message), long lines
    scripts.push(format!(
        "mdio-script 1\nL {}\nP 24 1 2 5 ffff 2 0\n",
        "label ".repeat(40)
    ));
    scripts.push(format!(
        "mdio-script 1\nL {}\nP 24 1 2 5 ffff 1 0\nP 26 1 2 5 ffff 1 0\n",
        "label ".repeat(40)
    ));
    for n in [509, 510, 511, 512, 1021, 1022] {
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}\nW 1 1 2 4\n", "c".repeat(n - 1)));
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}", "c".repeat(n - 1)));
        scripts.push(format!("mdio-script 1\nW 1 1 2 3\n#{}\r\n", "c".repeat(n - 1)));
    }
    // verification sections: all good depends on the seed; hung PHY 24; failing PHY 26; mixed
    for phys in [
        &["0"][..],
        &["0", "1"],
        &["24"],
        &["26"],
        &["0", "24", "25", "26"],
        &["26", "24", "1"],
    ] {
        let section: String = phys.iter().map(|p| verify(p)).collect();
        scripts.push(format!("{body}{section}"));
        scripts.push(format!("mdio-script 1\n{section}"));
        scripts.push(format!("{body}{section}L trailing label\n# and a comment\n"));
    }
    // assertions in the middle are not the section; a multi-try poll ends it
    scripts.push(format!("{body}{}W 1 1 2 3\n{}", verify("0"), verify("1")));
    scripts.push(format!("{body}{}P 1 1 2 3 ffff 2 0\n", verify("0")));
    scripts.push(format!("{body}{}D 5\n", verify("24")));
    scripts
}

fn all_jobs(parse_only: bool) -> Vec<Job> {
    let mut jobs = vec![Job {
        mode: Mode::Parse,
        seed: 0,
        tick: false,
        abort_k: 0,
    }];
    if parse_only {
        return jobs;
    }
    for mode in Mode::RUNS {
        for seed in [0, 1, 3, 6, 12, 99] {
            jobs.push(Job {
                mode,
                seed,
                tick: false,
                abort_k: 0,
            });
            jobs.push(Job {
                mode,
                seed,
                tick: true,
                abort_k: 0,
            });
        }
        for abort_k in 1..=24 {
            jobs.push(Job {
                mode,
                seed: 5,
                tick: true,
                abort_k,
            });
        }
    }
    jobs
}

#[test]
fn hand_written_corpus() {
    let (golden, outputs) = golden_corpus();
    let scripts = corpus();
    assert_eq!(
        scripts.len(),
        golden.len(),
        "the corpus has {} scripts, the golden {}: golden inputs differ (the corpus was edited)",
        scripts.len(),
        golden.len()
    );
    let mut compared = 0u64;
    for (i, (text, want)) in scripts.iter().zip(&golden).enumerate() {
        assert_eq!(
            fnv1a64(text.as_bytes()),
            want.digest,
            "corpus script {i} is not the one of the golden: golden inputs differ (the corpus was edited)\n--- script\n{text}"
        );
        // the jobs follow from the C parse result
        let parse_only = !want
            .jobs
            .first()
            .is_some_and(|&(_, out)| outputs[&out].starts_with("parse rc=0 "));
        let jobs: Vec<String> = all_jobs(parse_only).into_iter().map(job_text).collect();
        let golden_jobs: Vec<String> = want.jobs.iter().map(|&(job, _)| job_text(job)).collect();
        assert_eq!(
            jobs, golden_jobs,
            "corpus script {i}: the job list is not the golden's: golden inputs differ"
        );
        for &(job, out) in &want.jobs {
            let (c, rust) = (&outputs[&out], rust_output(text.as_bytes(), job));
            assert!(
                *c == rust,
                "corpus script {i}: Rust differs from the C golden for {job:?}\n--- script\n{text}\n--- C (golden)\n{c}--- Rust\n{rust}"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 5053, "the golden holds 5053 corpus comparisons");
    eprintln!(
        "differential: hand-written corpus: {} scripts, {compared} comparisons with the C golden agree",
        scripts.len()
    );
}

/// The `count` values the strategy draws from the fixed seed, in order.
fn generated<S: Strategy>(count: usize, strategy: &S) -> Vec<S::Value> {
    let mut runner = TestRunner::new_with_rng(
        ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &SEED),
    );
    (0..count)
        .map(|_| strategy.new_tree(&mut runner).unwrap().current())
        .collect()
}

/// Compares the Rust output of each (case, job) with the golden entry at the same place.
fn check_generated(kind: &str, cases: &[(Vec<u8>, Vec<Job>)], show: impl Fn(&[u8]) -> String) -> u64 {
    let golden = golden_generated(kind);
    let mut at = golden.iter();
    let mut compared = 0;
    for (case, (data, jobs)) in cases.iter().enumerate() {
        for &job in jobs {
            let want = at.next().unwrap_or_else(|| {
                panic!(
                    "{kind} case {case}: not in the golden ({} entries): golden inputs differ",
                    golden.len()
                )
            });
            assert!(
                want.case == case && want.input == fnv1a64(data) && job_text(want.job) == job_text(job),
                "{kind} case {case} {job:?}: the generator changed, golden inputs differ (golden: case {} input {:016x} job {})\n--- input\n{}",
                want.case,
                want.input,
                job_text(want.job),
                show(data)
            );
            let rust = rust_output(data, job);
            assert!(
                fnv1a64(rust.as_bytes()) == want.output,
                "{kind} case {case}: the Rust output for {job:?} is not the C golden's (digest {:016x}, Rust {:016x})\n--- input\n{}\n--- Rust\n{rust}",
                want.output,
                fnv1a64(rust.as_bytes()),
                show(data)
            );
            compared += 1;
        }
    }
    assert!(
        at.next().is_none(),
        "{kind}: the golden has more entries than generated: golden inputs differ"
    );
    compared
}

#[test]
fn generated_valid_scripts() {
    let cases: Vec<(Vec<u8>, Vec<Job>)> = generated(400, &(common::valid_script(), common::jobs()))
        .into_iter()
        .map(|(text, (seed, tick, abort_k))| {
            if let Err(e) = mdio_script::Script::parse(&text) {
                panic!("the generator made an invalid script: {e}\n{text}");
            }
            let jobs = [Mode::Parse, Mode::Run, Mode::Verify, Mode::Only].map(|mode| Job {
                mode,
                seed,
                tick,
                abort_k,
            });
            (text.into_bytes(), jobs.to_vec())
        })
        .collect();
    let n = check_generated("valid", &cases, |d| String::from_utf8_lossy(d).into_owned());
    assert_eq!(n, 1600);
    eprintln!(
        "differential: 400 generated valid scripts, {n} comparisons (parse, run, verify, verify-only) with the C golden agree"
    );
}

#[test]
fn generated_damaged_scripts() {
    let strategy = prop_oneof![4 => common::corrupted_script(), 1 => common::noise()];
    // The parse pass only: a damaged script that the C code would half run is never run here.
    let cases: Vec<(Vec<u8>, Vec<Job>)> = generated(600, &strategy)
        .into_iter()
        .map(|data| (data, all_jobs(true)))
        .collect();
    let n = check_generated("damaged", &cases, esc);
    assert_eq!(n, 600);
    eprintln!("differential: 600 generated damaged scripts, {n} parse-pass comparisons with the C golden agree");
}

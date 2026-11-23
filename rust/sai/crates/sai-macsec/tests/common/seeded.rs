//! Random tests that are deterministic per seed. Case `i` of a run is generated from seed
//! `first + i` alone (proptest's ChaCha generator), so a failure names the one seed that replays
//! it: `SAI_MACSEC_SEED=<seed>` with one case. A run's first seed is `SAI_MACSEC_SEED`, else a
//! fresh random one (every run tries other scripts); the test prints it either way.
//!
//! Shared by sai-macsec's `tests/differential.rs` and sai-macsec-phy's `tests/e2e.rs`.
#![allow(dead_code, reason = "each test file uses part of it")]

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};
use std::fmt::Write as _;

/// At most this many failing seeds are shrunk and reported; the run stops there.
const MAX_FAILED: usize = 10;

/// The first seed of a run: `SAI_MACSEC_SEED`, else a random one.
pub(crate) fn first_seed() -> u64 {
    std::env::var("SAI_MACSEC_SEED")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| {
            use std::hash::{BuildHasher, Hasher};
            std::collections::hash_map::RandomState::new().build_hasher().finish()
        })
}

/// A number of cases from `var`, else `default`.
pub(crate) fn cases(var: &str, default: u64) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// The runner of one seed: one case, shrunk on failure, no regression files (the source tree
/// is read-only in the Nix build).
fn runner(seed: u64) -> TestRunner {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let config = Config {
        cases: 1,
        failure_persistence: None,
        ..Config::default()
    };
    TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &bytes))
}

/// What seed `seed` generates (the case a run tests for it).
pub(crate) fn value<S: Strategy>(strategy: &S, seed: u64) -> S::Value {
    match strategy.new_tree(&mut runner(seed)) {
        Ok(tree) => tree.current(),
        Err(why) => panic!("seed {seed}: no value: {why}"),
    }
}

/// The outcome of a run.
pub(crate) struct Run {
    /// Its first seed.
    pub(crate) first: u64,
    /// Seeds tried.
    pub(crate) cases: u64,
    /// Failing seeds with proptest's minimal case and its reason.
    pub(crate) failed: Vec<(u64, String)>,
}

impl Run {
    /// "seeds <first>.. (<n>)", for the summary line.
    pub(crate) fn seeds(&self) -> String {
        format!("seeds {}.. ({} cases)", self.first, self.cases)
    }

    /// Panics with every failing seed, how to replay it and its minimal case.
    pub(crate) fn assert_ok(&self, what: &str, cases_var: &str) {
        if self.failed.is_empty() {
            return;
        }
        let mut text = format!(
            "{what}: {} failing seeds of the run from seed {} ({} tried); replay one with SAI_MACSEC_SEED=<seed> {cases_var}=1\n",
            self.failed.len(),
            self.first,
            self.cases
        );
        for (seed, why) in &self.failed {
            let _ = write!(text, "=== seed {seed} (proptest's minimal case)\n{why}\n");
        }
        panic!("{text}");
    }
}

/// Runs seeds `first`, `first + 1`, ... (`cases` of them): every one, a failure does not stop
/// the run (after `MAX_FAILED` failures it does).
pub(crate) fn run<S: Strategy>(
    first: u64,
    cases: u64,
    strategy: &S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) -> Run {
    let mut failed = Vec::new();
    let mut tried = 0;
    for i in 0..cases {
        let seed = first.wrapping_add(i);
        tried += 1;
        match runner(seed).run(strategy, &test) {
            Ok(()) => {}
            Err(TestError::Fail(why, _)) => failed.push((seed, why.to_string())),
            Err(TestError::Abort(why)) => failed.push((seed, format!("aborted: {why}"))),
        }
        if failed.len() >= MAX_FAILED {
            break;
        }
    }
    Run {
        first,
        cases: tried,
        failed,
    }
}

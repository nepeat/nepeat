//! Shared by the integration tests: scenario files for the fake glue and profiles.
#![allow(dead_code, missing_docs, unreachable_pub, clippy::unwrap_used, clippy::panic)]
use sai_core::backend::Profile;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// A line the test run shows also when the test passes (the harness captures `println!`):
/// why a test skipped, or what it compared.
pub fn note(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// The glue's state is one per process: one test at a time.
pub fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn scratch() -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("sai-backend-openbcm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a scenario (the fake glue's "config.bcm") and returns its path.
pub fn scenario(name: &str, text: &str) -> PathBuf {
    let path = scratch().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// A profile whose SDK config file is the scenario `text`, plus `extra` keys.
pub fn profile(name: &str, text: &str, extra: &[(&str, &str)]) -> Profile {
    let config = scenario(name, text);
    let mut pairs = vec![("SAI_INIT_CONFIG_FILE".to_owned(), config.display().to_string())];
    pairs.extend(extra.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    Profile::from_pairs(pairs)
}

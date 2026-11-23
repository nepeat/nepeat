//! Shared by the integration tests: scenario files for the fake glue, profiles, and backends on
//! Rust fake chips of their own (so tests run in parallel).
#![allow(dead_code, missing_docs, unreachable_pub, clippy::unwrap_used, clippy::panic)]
use sai_backend_openbcm::OpenbcmBackend;
use sai_core::backend::{Backend, BackendEvent, EventSink, Profile};
use sdk_glue::rust_fake::Chip;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, mpsc};

/// A line the test run shows also when the test passes (the harness captures `println!`):
/// why a test skipped, or what it compared.
pub fn note(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// For tests on the process's glue (one per process): one test at a time.
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

/// A file name in [`scratch`] no other test of this process uses.
pub fn unique(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    scratch().join(format!("{}-{name}", N.fetch_add(1, Ordering::Relaxed)))
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

/// A profile on a scenario file of its own.
pub fn own_profile(text: &str, extra: &[(&str, &str)]) -> Profile {
    let name = unique("scenario.cfg");
    profile(name.file_name().unwrap().to_str().unwrap(), text, extra)
}

/// A backend on a fake chip of its own, brought up on `text`; the chip (to inspect and script)
/// and the receiving end of the backend's events.
pub struct Rig {
    pub chip: Chip,
    pub backend: OpenbcmBackend,
    pub events: mpsc::Receiver<BackendEvent>,
}

pub fn rig(text: &str, extra: &[(&str, &str)]) -> Rig {
    let chip = Chip::new();
    let mut backend = OpenbcmBackend::on_chip(chip.clone());
    let (sink, events) = EventSink::channel();
    backend.init(&own_profile(text, extra), sink).unwrap();
    Rig { chip, backend, events }
}

/// A scenario line that traces every glue call into a new file; the file.
pub fn trace() -> (String, PathBuf) {
    let path = unique("trace.txt");
    (format!("trace {}\n", path.display()), path)
}

pub fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

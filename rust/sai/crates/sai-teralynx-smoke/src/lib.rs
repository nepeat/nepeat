//! `sai-teralynx-smoke`: drives a CS8210 libsai directly (no redis, syncd or orchagent), through the
//! SAI C API only (crate `sai-client`: `dlopen` of the library), on either Teralynx libsai.
//!
//! Two command lines:
//!
//! - `sai-teralynx-smoke PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]`: the C++ program
//!   (`tools/sai_smoke.cpp` of the C++ sai-teralynx, deleted in H5 on 2026-11-27, last in the tree at commit
//!   767d9b0; its outputs are `tests/golden/smoke-cpp.txt`) line for line ([`legacy`]): the same steps,
//!   output and exit codes (0 / 1 / 2).
//! - `sai-teralynx-smoke --dry` / `--sonic-is-stopped --profile P ...`: the H1 gate of the CS-B plan
//!   ([`gate`]): PASS / FAIL / SKIP per step, the cabled loops as devport pairs (default the CS8210's
//!   1<->2, 3<->17, 31<->32), the learn-ring guard's GOT redirects from the library's stderr, the
//!   post-init and LED watch registers through the diag socket's `regs`, temperatures, teardown,
//!   `--recreate`. Refuses without `--sonic-is-stopped`.
//!
//! The library is `$SAI_LIBSAI`, `--libsai` (gate) or `lib/libsai.so.1` beside the program's `bin/`
//! (`sai_client::default_libsai`); the C++ program linked it (its RUNPATH).
#![forbid(unsafe_code)]

pub mod gate;
pub mod legacy;

use std::collections::BTreeMap;
use std::io::Write as _;

/// A line on stdout, flushed.
pub fn say(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.write_all(b"\n");
    let _ = o.flush();
}

/// A line on stderr.
pub fn warn(s: &str) {
    let _ = writeln!(std::io::stderr(), "{s}");
}

/// `%-<w>s` (bytes, as printf).
pub fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.len())))
}

/// The profile as the C++ reads it: `KEY=VALUE` lines (the first `=`), empty and `#` lines skipped,
/// a repeated key: the last (`std::map`: served in key order); a missing file: empty.
pub fn read_profile(path: &str) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read(path).ok()?;
    let mut m = BTreeMap::new();
    for line in String::from_utf8_lossy(&text).split('\n') {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            m.insert(k.to_owned(), v.to_owned());
        }
    }
    Some(m)
}

/// The program: the C++ form unless the first argument is an option.
pub fn main(args: &[String]) -> i32 {
    if args.get(1).is_some_and(|a| a.starts_with("--")) {
        gate::main(args)
    } else {
        legacy::main(args)
    }
}

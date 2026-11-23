//! Two jobs:
//!
//! 1. The enumerator name table (`$OUT_DIR/names.rs`): every enumerator of every
//!    `typedef enum _sai_<x>_t { ... } sai_<x>_t;` block of `$SAI_INC/sai*.h`, in header order,
//!    as `(type, name, value)`. It is what `tests/driver_names.awk` gives the C++ driver
//!    (`E(T, N)`); the values are `sai-sys` constants, so the numbers are the C compiler's.
//! 2. For a cross build (or `SAI_DRIVER_PREBUILD=1`): the C++ `frontend_driver` built for the
//!    target into `$OUT_DIR/target-tests/sai-cpp-frontend-driver`, with the op scripts and the
//!    allow-list beside it in `target-tests/sai-driver/` (README "Big-endian runs": no compiler
//!    at run time; the test finds them through `$SAI_RS_TEST_BIN`). Never with
//!    `SAI_DRIVER_PREBUILD=0`; a native test run compiles the driver itself.
//!
//! Inputs of 2: `SAI_OPENBCM_TESTS` (the frozen C++ frontend's `tests` directory,
//! `ai_re/nix/packages/sai-teralynx/frontend/tests`) or `$SAI_OPENBCM_SRC/../tests`,
//! `SAI_OPENBCM_SRC` (else `<tests>/../src`), `SAI_INC`, and the target's compilers:
//! `SAI_DRIVER_CXX` / `SAI_DRIVER_CC`, else derived from `SAI_SYS_ABI_CC`, else (native) `CXX` / `CC`.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

#[path = "src/cpp_build.rs"]
mod cpp_build;

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn names(inc: &Path, out: &Path) {
    let mut headers: Vec<PathBuf> = fs::read_dir(inc)
        .expect("SAI include directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "h")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("sai"))
        })
        .collect();
    // the shell's glob order (the awk reads `$(SAI_INC)/sai*.h`)
    headers.sort();
    let mut table = String::from("&[\n");
    let mut count = 0usize;
    for header in &headers {
        let src = fs::read_to_string(header).expect("header");
        let mut open: Option<String> = None;
        for line in src.lines() {
            if let Some(rest) = line.strip_prefix("typedef enum _sai_") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                if name.ends_with("_t") {
                    open = Some(format!("sai_{name}"));
                }
                continue;
            }
            let Some(etype) = &open else { continue };
            if line.starts_with('}') {
                open = None;
                continue;
            }
            let trimmed = line.trim_start();
            if !trimmed.starts_with("SAI_") {
                continue;
            }
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            let rest = trimmed[name.len()..].trim_start();
            // the awk's /^[ \t]*SAI_[A-Z0-9_]+[ \t]*(=|,|$)/
            if rest.is_empty() || rest.starts_with('=') || rest.starts_with(',') {
                writeln!(table, "    (\"{etype}\", \"{name}\", sys::{name} as i64),").expect("write");
                count += 1;
            }
        }
    }
    table.push_str("]\n");
    assert!(
        count > 1000,
        "sai-driver: only {count} enumerators found in {}",
        inc.display()
    );
    fs::write(out.join("names.rs"), table).expect("write names.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/cpp_build.rs");
    // DEP_SAI_HEADERS_INCLUDE: exported by sai-sys's build script (links = "sai-headers").
    let inc = PathBuf::from(env::var_os("DEP_SAI_HEADERS_INCLUDE").expect("sai-sys exports the header directory"));
    println!("cargo:rerun-if-changed={}", inc.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    names(&inc, &out);
    match cpp_build::prebuild(&out) {
        Ok(note) if note.is_empty() => {}
        Ok(note) => println!("cargo:warning=C++ frontend driver {note}"),
        Err(why) => {
            eprintln!("sai-driver build.rs: SAI_DRIVER_PREBUILD is set, but: {why}");
            std::process::exit(1);
        }
    }
}

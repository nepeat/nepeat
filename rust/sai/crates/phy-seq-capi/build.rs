//! Generates the Rust declarations of `include/phy_seq.h` (bindgen): the structs the functions
//! of this crate take are the header's, and `src/api.rs` checks its functions against the
//! header's prototypes at compile time.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::path::PathBuf;

fn main() {
    let header =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("include/phy_seq.h");
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
    // Big-endian ppc64 with the ELFv2 ABI: clang wants the plain triple and the ABI as a flag
    // (as sai-sys/build.rs; in a nix build bindgenHook's BINDGEN_EXTRA_CLANG_ARGS decides).
    let target = env::var("TARGET").expect("TARGET");
    let clang_target: Vec<String> = match target
        .strip_suffix("gnuelfv2")
        .or_else(|| target.strip_suffix("gnuabielfv2"))
    {
        Some(stem) => vec![format!("--target={stem}gnu"), "-mabi=elfv2".into()],
        None => Vec::new(),
    };
    let bindings = bindgen::Builder::default()
        .header(header.to_str().expect("utf-8 path"))
        .clang_args(clang_target)
        .allowlist_item("(phy_seq|PHY_SEQ)_.*")
        .layout_tests(true)
        .derive_default(false) // for structs with pointers bindgen writes unsafe { zeroed() }
        .derive_debug(false)
        .generate_comments(false)
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .expect("bindgen over include/phy_seq.h");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("phy_seq.rs");
    bindings.write_to_file(&out).expect("write the bindings");
}

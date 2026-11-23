//! Generates the Rust declarations of `include/sai_macsec_rs.h` (bindgen; the SAI types it uses
//! are left out and taken from sai-sys): `src/api.rs` checks the exported functions against them
//! at compile time.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let header = dir.join("include/sai_macsec_rs.h");
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rerun-if-env-changed=SAI_INC");
    println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
    let inc = PathBuf::from(env::var_os("SAI_INC").expect("SAI_INC: the SAI headers (SAI/inc), as for sai-sys"));
    // big-endian ppc64 with the ELFv2 ABI: as sai-sys/build.rs and phy-seq-capi/build.rs
    let target = env::var("TARGET").expect("TARGET");
    let mut args: Vec<String> = match target
        .strip_suffix("gnuelfv2")
        .or_else(|| target.strip_suffix("gnuabielfv2"))
    {
        Some(stem) => vec![format!("--target={stem}gnu"), "-mabi=elfv2".into()],
        None => Vec::new(),
    };
    args.push(format!("-I{}", inc.display()));
    args.push(format!("-I{}", inc.join("../experimental").display()));
    let bindings = bindgen::Builder::default()
        .header(header.to_str().expect("utf-8 path"))
        .clang_args(args)
        .allowlist_item("(ob_rs|OB_RS)_.*")
        .blocklist_type("_?sai_.*")
        .layout_tests(true)
        .derive_default(false)
        .derive_debug(false)
        .generate_comments(false)
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .expect("bindgen over include/sai_macsec_rs.h");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("sai_macsec_rs.rs");
    bindings.write_to_file(&out).expect("write the bindings");
}

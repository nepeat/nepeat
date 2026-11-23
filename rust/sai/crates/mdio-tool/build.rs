//! `SDK_GLUE_RPATH` adds a run path to the binary (the real glue's libsdk).
//!
//! Until 2026-11-23 this script also built the C `openbcm-mdio-script` from `MDIO_SCRIPT_C_DIR`
//! as the reference of tests/differential.rs; that test now compares with the C tool's golden
//! (`tests/golden/differential.txt`, commit 50a6aa7) and nothing here compiles C.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SDK_GLUE_RPATH");
    if let Some(rpath) = env::var_os("SDK_GLUE_RPATH") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", rpath.display());
    }
}

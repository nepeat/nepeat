//! Builds the loader's test fixtures (`tests/c/*.c`) for the target into `$OUT_DIR/target-tests/`,
//! as sai-ffi builds its C test helper (test executables never compile C): a stand-in libifcs that
//! exports every `IFCS_SYMBOLS` entry (from the generated header ifcs-sys binds:
//! `DEP_IFCS_HEADERS_INCLUDE`), the same with `ifcs_deinit` missing, and a call tracer
//! (`ifcs_trace_bind` + one traced function). Compiler and skip rules as elf-got's `build.rs`.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

fn main() {
    for f in ["tests/c/libifcs_stub.c", "tests/c/trace_stub.c"] {
        println!("cargo:rerun-if-changed={f}");
    }
    for var in [
        "IFCS_SYS_ABI_CC",
        "CC",
        "IFCS_SYS_ABI_CHECK",
        "DEP_IFCS_HEADERS_INCLUDE",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if !matches!(arch.as_str(), "x86_64" | "aarch64") {
        return;
    }
    let required = std::env::var("IFCS_SYS_ABI_CHECK").is_ok_and(|v| v == "require");
    let native = std::env::var("HOST").ok() == std::env::var("TARGET").ok();
    let compiler = std::env::var("IFCS_SYS_ABI_CC")
        .or_else(|_| std::env::var("CC"))
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| native.then(|| "cc".to_string()));
    let inc = std::env::var("DEP_IFCS_HEADERS_INCLUDE").ok();
    let (Some(compiler), Some(inc), Some(out_dir)) = (compiler, inc, std::env::var_os("OUT_DIR")) else {
        assert!(
            !required,
            "ifcs: no C compiler or header for the loader's test fixtures"
        );
        return;
    };
    let dir = Path::new(&out_dir).join("target-tests");
    std::fs::create_dir_all(&dir).expect("create target-tests");
    let build = |env: &str, out: &str, src: &str, defines: &[&str]| {
        let so = dir.join(out);
        let mut words = compiler.split_whitespace();
        let status = Command::new(words.next().expect("compiler"))
            .args(words)
            .args(["-std=gnu11", "-O1", "-fPIC", "-shared", "-Wall"])
            .arg(format!("-I{inc}"))
            .args(defines)
            .arg("-o")
            .arg(&so)
            .arg(src)
            .status();
        match status {
            Ok(s) if s.success() => println!("cargo:rustc-env={env}={}", so.display()),
            other => assert!(!required, "ifcs: building {out} failed: {other:?}"),
        }
    };
    build("IFCS_STUB_FULL", "libifcs_stub_full.so", "tests/c/libifcs_stub.c", &[]);
    build(
        "IFCS_STUB_MISSING",
        "libifcs_stub_missing.so",
        "tests/c/libifcs_stub.c",
        &["-DOMIT_DEINIT"],
    );
    build("IFCS_TRACE_STUB", "libifcs_trace_stub.so", "tests/c/trace_stub.c", &[]);
    build(
        "IFCS_TRACE_NOBIND",
        "libifcs_trace_nobind.so",
        "tests/c/trace_stub.c",
        &["-DNO_BIND"],
    );
}

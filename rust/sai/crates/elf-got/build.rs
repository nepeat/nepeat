//! Builds the test fixture `tests/c/got_fixture.c` (a shared object that calls one of its own
//! exported functions through its PLT and takes another one's address through the GOT) for the
//! target into `$OUT_DIR/target-tests/`, as sai-ffi builds its C test helper: test executables
//! never compile C. The compiler: `IFCS_SYS_ABI_CC` (the target's, as ifcs-sys uses it), else
//! `CC`, else `cc` on a native build. Without one the tests that need the fixture skip with a
//! message, unless `IFCS_SYS_ABI_CHECK=require` (the Nix build): then the build fails.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

fn main() {
    let source = Path::new("tests/c/got_fixture.c");
    println!("cargo:rerun-if-changed={}", source.display());
    for var in ["IFCS_SYS_ABI_CC", "CC", "IFCS_SYS_ABI_CHECK"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if !matches!(arch.as_str(), "x86_64" | "aarch64") {
        return; // the crate is empty there
    }
    let required = std::env::var("IFCS_SYS_ABI_CHECK").is_ok_and(|v| v == "require");
    let native = std::env::var("HOST").ok() == std::env::var("TARGET").ok();
    let compiler = std::env::var("IFCS_SYS_ABI_CC")
        .or_else(|_| std::env::var("CC"))
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| native.then(|| "cc".to_string()));
    let (Some(compiler), Some(out_dir)) = (compiler, std::env::var_os("OUT_DIR")) else {
        assert!(
            !required,
            "elf-got: no C compiler for the test fixture (IFCS_SYS_ABI_CC)"
        );
        return;
    };
    let dir = Path::new(&out_dir).join("target-tests");
    std::fs::create_dir_all(&dir).expect("create target-tests");
    let so = dir.join("libgot_fixture.so");
    let mut words = compiler.split_whitespace();
    let status = Command::new(words.next().expect("compiler"))
        .args(words)
        // -z lazy: a PLT with its own .got.plt slot (JUMP_SLOT) as libifcs has; with -z now (Nix's
        // bindnow hardening) ld may route the PLT through the GLOB_DAT slot instead.
        .args([
            "-std=gnu11",
            "-O1",
            "-fPIC",
            "-shared",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wl,-z,lazy",
            "-o",
        ])
        .arg(&so)
        .arg(source)
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-env=ELF_GOT_FIXTURE={}", so.display()),
        other => assert!(!required, "elf-got: building the test fixture failed: {other:?}"),
    }
}

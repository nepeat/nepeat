//! Prebuilds the hardware smoke program (`smoke/sai_smoke.c`, a C program against `sai.h` that
//! loads libsai.so with dlopen) when the SAI headers and a C compiler for the target are
//! around, into `OUT_DIR/target-tests/sai-openbcm-smoke`: the test bundle for the switch copies
//! that directory, and `tests/smoke.rs` runs the program. Without them the program is not built
//! and the test says so. Nothing here is linked into the crate.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

// The C++ driver of tests/glue_differential.rs, prebuilt for a target without a compiler
// (README "Big-endian runs") or with SAI_GLUE_DIFF_PREBUILD=1: glue_prebuild() below.
// (both files allow dead code themselves: the build script uses a part)
#[path = "../sai-driver/src/cpp_build.rs"]
mod cpp_build;
#[path = "tests/cpp/glue_build.rs"]
mod glue_build;

/// Prebuilds the glue differential's C++ driver into `OUT_DIR/target-tests` (its scripts and
/// allow-list into `target-tests/sai-backend-openbcm/`) for a cross build or
/// `SAI_GLUE_DIFF_PREBUILD=1`; a native test run builds it itself.
fn glue_prebuild(out: &Path) {
    let own = Path::new(&env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("tests/cpp");
    match cpp_build::prebuild_variant(out, &glue_build::PREBUILD, |s| glue_build::variant(&own, &s.src)) {
        Ok(note) if note.is_empty() => {}
        Ok(note) => println!("cargo:warning=glue differential C++ driver {note}"),
        Err(why) => panic!("sai-backend-openbcm build.rs: SAI_GLUE_DIFF_PREBUILD is set, but: {why}"),
    }
}

fn env_any(names: &[String]) -> Option<String> {
    names.iter().find_map(|v| {
        println!("cargo:rerun-if-env-changed={v}");
        env::var(v).ok().filter(|s| !s.is_empty())
    })
}

/// The target's C compiler, found as sai-sys and sdk-glue-sys find it.
fn c_compiler(target: &str, host: &str) -> Option<String> {
    let under = target.replace('-', "_");
    env_any(&[
        "SAI_SMOKE_CC".into(),
        "SAI_SYS_ABI_CC".into(),
        format!("CC_{under}"),
        format!("CC_{}", under.to_uppercase()),
        "TARGET_CC".into(),
    ])
    .or_else(|| (host == target).then(|| env_any(&["CC".into()]).unwrap_or_else(|| "cc".into())))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=smoke/sai_smoke.c");
    println!("cargo:rerun-if-env-changed=SAI_INC");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    glue_prebuild(&out);
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let source = Path::new(&env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("smoke/sai_smoke.c");

    let built = (|| {
        let inc = PathBuf::from(env::var_os("SAI_INC").filter(|v| !v.is_empty())?);
        if !inc.join("sai.h").is_file() {
            return None;
        }
        let cc = c_compiler(&target, &host)?;
        let dir = out.join("target-tests");
        std::fs::create_dir_all(&dir).expect("mkdir target-tests");
        let exe = dir.join("sai-openbcm-smoke");
        let mut words = cc.split_whitespace();
        let run = Command::new(words.next()?)
            .args(words)
            .args(["-std=gnu11", "-Wall", "-Wextra", "-Werror", "-O1", "-g", "-I"])
            .arg(&inc)
            .arg("-I")
            .arg(inc.parent()?.join("experimental"))
            .arg(&source)
            .arg("-o")
            .arg(&exe)
            .args(["-ldl", "-lpthread"])
            .output()
            .ok()?;
        // The headers and a compiler are there: a program that does not compile is an error.
        assert!(
            run.status.success(),
            "sai-backend-openbcm: smoke/sai_smoke.c does not compile with {cc}:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        Some(exe)
    })();
    if let Some(exe) = built {
        println!("cargo:rustc-env=SAI_OPENBCM_SMOKE_BIN={}", exe.display());
    } else {
        println!("cargo:rustc-env=SAI_OPENBCM_SMOKE_BIN=");
        println!("cargo:warning=smoke program not built: needs SAI_INC (sai.h) and a C compiler for {target}");
    }
}

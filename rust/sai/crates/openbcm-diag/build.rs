//! Builds the C client `openbcm_diag.c` (frozen since 2026-11-23 with the C++ frontend in
//! `ai_re/nix/packages/sai-teralynx/frontend/src`; the CS8210 still ships it: a live comparison)
//! as the reference for tests/differential.rs, from `SAI_OPENBCM_SRC` (that src
//! directory), into `$OUT_DIR/target-tests/openbcm-diag-c`: tests never compile C when they run,
//! so the test executable can be copied to a machine without a compiler together with that file
//! (`SAI_RS_TEST_BIN`, README "Big-endian runs"). `OPENBCM_DIAG_C_PARITY=require` makes a
//! missing source or compiler a build failure.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn env_any(names: &[String]) -> Option<String> {
    names.iter().find_map(|v| {
        println!("cargo:rerun-if-env-changed={v}");
        env::var(v).ok().filter(|s| !s.is_empty())
    })
}

/// The target's C compiler, found as mdio-tool's build script finds it.
fn c_compiler() -> Option<String> {
    let (target, host) = (env::var("TARGET").expect("TARGET"), env::var("HOST").expect("HOST"));
    let under = target.replace('-', "_");
    env_any(&[
        "SDK_GLUE_SYS_CC".into(),
        "SAI_SYS_ABI_CC".into(),
        format!("CC_{under}"),
        format!("CC_{}", under.to_uppercase()),
        "TARGET_CC".into(),
    ])
    .or_else(|| (host == target).then(|| env_any(&["CC".into()]).unwrap_or_else(|| "cc".into())))
}

fn reference_client() -> Result<PathBuf, String> {
    let source = ["SAI_OPENBCM_SRC"]
        .iter()
        .filter_map(|v| {
            println!("cargo:rerun-if-env-changed={v}");
            env::var_os(v)
        })
        .map(|d| PathBuf::from(d).join("openbcm_diag.c"))
        .find(|p| p.is_file())
        .ok_or("no openbcm_diag.c in SAI_OPENBCM_SRC")?;
    println!("cargo:rerun-if-changed={}", source.display());
    let cc = c_compiler().ok_or("no C compiler for the target (set SDK_GLUE_SYS_CC)")?;
    let dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("target-tests");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join("openbcm-diag-c");
    let mut words = cc.split_whitespace();
    let o = Command::new(words.next().ok_or("no compiler")?)
        .args(words)
        .args(["-std=gnu11", "-O1", "-Wall", "-o"])
        .arg(&out)
        .arg(&source)
        .output()
        .map_err(|e| format!("cannot run {cc}: {e}"))?;
    if o.status.success() {
        Ok(dir)
    } else {
        Err(String::from_utf8_lossy(&o.stderr).into_owned())
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OPENBCM_DIAG_C_PARITY");
    let required = env::var("OPENBCM_DIAG_C_PARITY").is_ok_and(|v| v == "require");
    let helpers = match reference_client() {
        Ok(dir) => dir.display().to_string(),
        Err(why) => {
            assert!(
                !required,
                "openbcm-diag: OPENBCM_DIAG_C_PARITY=require, but the C client was not built: {why}"
            );
            println!("cargo:warning=the C openbcm-diag was not built, tests/differential.rs will skip: {why}");
            String::new()
        }
    };
    println!("cargo:rustc-env=OPENBCM_DIAG_C_HELPERS={helpers}");
}

//! Builds the C tool as the reference for tests/differential.rs when its sources are around
//! (`SAI_OPENBCM_SRC` or `MDIO_SCRIPT_C_DIR`: sai-openbcm's src directory): the unmodified `asic_temp.c` and
//! `temp_files.c` on `tests/c/stub_glue.c`, a C model of the few glue functions the tool calls
//! that answers as `sdk-glue`'s Rust fake chip does, from the same scenario file. Into
//! `$OUT_DIR/target-tests/openbcm-asic-temp-c`: tests never compile C when they run, so the test
//! executable can be copied to a machine without a compiler together with that file
//! (`SAI_RS_TEST_BIN`, README "Big-endian runs"). `ASIC_TEMP_C_PARITY=require` makes a missing
//! source or compiler a build failure. Also: `SDK_GLUE_RPATH` adds a run path to the binary (the
//! real glue's libsdk).
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

fn reference_tool() -> Result<PathBuf, String> {
    let src = ["SAI_OPENBCM_SRC", "MDIO_SCRIPT_C_DIR"]
        .iter()
        .filter_map(|v| {
            println!("cargo:rerun-if-env-changed={v}");
            env::var_os(v)
        })
        .map(PathBuf::from)
        .find(|d| d.join("asic_temp.c").is_file())
        .ok_or("no asic_temp.c in SAI_OPENBCM_SRC or MDIO_SCRIPT_C_DIR")?;
    for file in ["asic_temp.c", "temp_files.c", "temp_files.h", "sdk_glue.h"] {
        let path = src.join(file);
        if !path.is_file() {
            return Err(format!("no {file} in {}", src.display()));
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let stub =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("tests/c/stub_glue.c");
    println!("cargo:rerun-if-changed={}", stub.display());
    let cc = c_compiler().ok_or("no C compiler for the target (set SDK_GLUE_SYS_CC)")?;
    let dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("target-tests");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut words = cc.split_whitespace();
    let o = Command::new(words.next().ok_or("no compiler")?)
        .args(words)
        .args(["-std=gnu11", "-O1", "-g", "-Wall", "-Wextra", "-I"])
        .arg(&src)
        .arg(src.join("asic_temp.c"))
        .arg(src.join("temp_files.c"))
        .arg(&stub)
        .arg("-o")
        .arg(dir.join("openbcm-asic-temp-c"))
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
    println!("cargo:rerun-if-env-changed=SDK_GLUE_RPATH");
    if let Some(rpath) = env::var_os("SDK_GLUE_RPATH") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", rpath.display());
    }
    println!("cargo:rerun-if-env-changed=ASIC_TEMP_C_PARITY");
    let required = env::var("ASIC_TEMP_C_PARITY").is_ok_and(|v| v == "require");
    let helpers = match reference_tool() {
        Ok(dir) => dir.display().to_string(),
        Err(why) => {
            assert!(
                !required,
                "openbcm-asic-temp: ASIC_TEMP_C_PARITY=require, but the C tool was not built: {why}"
            );
            println!("cargo:warning=the C openbcm-asic-temp was not built, tests/differential.rs will skip: {why}");
            String::new()
        }
    };
    println!("cargo:rustc-env=ASIC_TEMP_C_HELPERS={helpers}");
}

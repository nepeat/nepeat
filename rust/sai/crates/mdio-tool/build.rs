//! Builds the C tool as a reference for tests/differential.rs when its sources are around
//! (`MDIO_SCRIPT_C_DIR`: sai-openbcm's src directory): once as it is built without the SDK and
//! once with `-DOB_HAVE_SDK` on the fake glue of sdk-glue-sys, the glue the Rust tool's tests
//! link. Tests never compile C when they run, so the test executables can be copied to a
//! machine without a compiler together with `$OUT_DIR/target-tests/*` (`SAI_RS_TEST_BIN`).
//! Also: `SDK_GLUE_RPATH` adds a run path to the binary (the real glue's libsdk).
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn env_any(names: &[String]) -> Option<String> {
    names.iter().find_map(|v| {
        println!("cargo:rerun-if-env-changed={v}");
        env::var(v).ok().filter(|s| !s.is_empty())
    })
}

/// The target's C compiler, found as sdk-glue-sys and sai-sys find it.
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

fn compile(cc: &str, c_dir: &Path, out: &Path, sdk: bool, shim: &Path) -> Result<(), String> {
    let mut words = cc.split_whitespace();
    let mut c = Command::new(words.next().ok_or("no compiler")?);
    c.args(words).args(["-std=gnu11", "-O1", "-g", "-I"]).arg(c_dir);
    for file in ["mdio_script_main.c", "mdio_script.c", "mdio_report.c"] {
        c.arg(c_dir.join(file));
    }
    if sdk {
        let fake = Path::new(&env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
            .join("../sdk-glue-sys/fake/fake_glue.c");
        println!("cargo:rerun-if-changed={}", fake.display());
        c.arg("-DOB_HAVE_SDK")
            .arg("-I")
            .arg(shim)
            .arg(c_dir.join("temp_files.c"))
            .arg(fake);
    }
    c.arg("-o").arg(out).arg("-lpthread");
    let o = c.output().map_err(|e| format!("cannot run {cc}: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).into_owned())
    }
}

fn reference_tools() -> Result<PathBuf, String> {
    println!("cargo:rerun-if-env-changed=MDIO_SCRIPT_C_DIR");
    let c_dir = PathBuf::from(env::var_os("MDIO_SCRIPT_C_DIR").ok_or("MDIO_SCRIPT_C_DIR is not set")?);
    for file in [
        "mdio_script_main.c",
        "mdio_script.c",
        "mdio_report.c",
        "temp_files.c",
        "sdk_glue.h",
    ] {
        let path = c_dir.join(file);
        if !path.is_file() {
            return Err(format!("no {file} in MDIO_SCRIPT_C_DIR={}", c_dir.display()));
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let cc = c_compiler().ok_or("no C compiler for the target (set SDK_GLUE_SYS_CC)")?;
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let dir = out.join("target-tests");
    let shim = out.join("shim");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&shim).map_err(|e| e.to_string())?;
    // fake_glue.c includes the declarations sdk-glue-sys generates; next to the C tool the
    // real header (a superset) takes their place, which also checks the fake against it.
    std::fs::write(shim.join("ob_sdk_glue_decls.h"), "#include \"sdk_glue.h\"\n").map_err(|e| e.to_string())?;
    compile(&cc, &c_dir, &dir.join("openbcm-mdio-script-c"), false, &shim)?;
    compile(&cc, &c_dir, &dir.join("openbcm-mdio-script-c-fake"), true, &shim)?;
    Ok(dir)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SDK_GLUE_RPATH");
    if let Some(rpath) = env::var_os("SDK_GLUE_RPATH") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", rpath.display());
    }
    println!("cargo:rerun-if-env-changed=MDIO_TOOL_C_PARITY");
    let required = env::var("MDIO_TOOL_C_PARITY").is_ok_and(|v| v == "require");
    let helpers = match reference_tools() {
        Ok(dir) => dir.display().to_string(),
        Err(why) => {
            assert!(
                !required,
                "mdio-tool: MDIO_TOOL_C_PARITY=require, but the C tool was not built: {why}"
            );
            println!("cargo:warning=the C openbcm-mdio-script was not built, tests/differential.rs will skip: {why}");
            String::new()
        }
    };
    println!("cargo:rustc-env=MDIO_TOOL_C_HELPERS={helpers}");
}

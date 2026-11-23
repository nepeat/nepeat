//! Builds `tests/harness.c` with the C compiler against the real SAI headers and runs it on
//! the built `libsai.so`: the slice through the real C ABI, loaded with `dlopen` as syncd's
//! link would see it.
//!
//! Where no C compiler is (the ppc64 test bundle run on a switch, README "Big-endian runs"):
//! `SAI_FFI_HARNESS` names the harness built beforehand and `SAI_FFI_LIBSAI` the library.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
use std::path::{Path, PathBuf};
use std::process::Command;

/// The cdylib cargo built for this test run: next to the test executable or one level up.
fn libsai() -> PathBuf {
    if let Some(lib) = std::env::var_os("SAI_FFI_LIBSAI") {
        return PathBuf::from(lib);
    }
    let exe = std::env::current_exe().expect("test executable path");
    let deps = exe.parent().expect("deps directory");
    [deps, deps.parent().expect("profile directory")]
        .iter()
        .map(|dir| dir.join("libsai.so"))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("libsai.so not found near {}", exe.display()))
}

/// The harness executable and whether this run built it (and so removes it).
fn harness() -> (PathBuf, bool) {
    if let Some(prebuilt) = std::env::var_os("SAI_FFI_HARNESS") {
        return (PathBuf::from(prebuilt), false);
    }
    let inc = Path::new(env!("SAI_FFI_SAI_INC"));
    let experimental = inc.parent().expect("SAI directory").join("experimental");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("harness-{}", std::process::id()));
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let mut words = cc.split_whitespace();
    let compile = Command::new(words.next().expect("C compiler"))
        .args(words)
        .args(["-std=gnu11", "-Wall", "-Wextra", "-Werror", "-O1", "-g", "-I"])
        .arg(inc)
        .arg("-I")
        .arg(experimental)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/harness.c"))
        .arg("-o")
        .arg(&out)
        .args(["-ldl", "-lpthread"])
        .output()
        .unwrap_or_else(|e| panic!("cannot run the C compiler {cc:?}: {e} (set CC)"));
    assert!(
        compile.status.success(),
        "harness.c does not compile:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    (out, true)
}

#[test]
fn c_harness_drives_the_slice_through_dlopen() {
    let (exe, built_here) = harness();
    let run = Command::new(&exe).arg(libsai()).output().expect("run the harness");
    if built_here {
        let _ = std::fs::remove_file(&exe);
    }
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    println!("{stdout}{stderr}");
    assert!(
        run.status.success() && stdout.contains("HARNESS_OK"),
        "harness failed ({}):\n{stdout}{stderr}",
        run.status
    );
}

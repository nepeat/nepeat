//! Builds `tests/harness.c` with the C compiler against the real SAI headers and runs it on
//! the built `libsai.so`: the slice through the real C ABI, loaded with `dlopen` as syncd's
//! link would see it.
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
    let exe = std::env::current_exe().expect("test executable path");
    let deps = exe.parent().expect("deps directory");
    [deps, deps.parent().expect("profile directory")]
        .iter()
        .map(|dir| dir.join("libsai.so"))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("libsai.so not found near {}", exe.display()))
}

#[test]
fn c_harness_drives_the_slice_through_dlopen() {
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

    let run = Command::new(&out).arg(libsai()).output().expect("run the harness");
    let _ = std::fs::remove_file(&out);
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    println!("{stdout}{stderr}");
    assert!(
        run.status.success() && stdout.contains("HARNESS_OK"),
        "harness failed ({}):\n{stdout}{stderr}",
        run.status
    );
}

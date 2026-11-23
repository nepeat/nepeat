//! Link settings of libsai.so, where the SAI headers are (for the C harness test), and the
//! C helper of the union test, prebuilt: test executables run on the target without a
//! compiler (README "Big-endian runs"), so no test compiles C at run time.
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // The name syncd's link line and the dynamic loader look for.
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libsai.so.1");
    // Exported by sai-sys's build script to its direct dependents (links = "sai-headers").
    let Some(inc) = std::env::var_os("DEP_SAI_HEADERS_INCLUDE") else {
        return;
    };
    let inc = PathBuf::from(inc);
    println!("cargo:rustc-env=SAI_FFI_SAI_INC={}", inc.display());
    union_check(&inc);
}

/// Builds `tests/union_check.c` for the target into `$OUT_DIR/target-tests/union-check`
/// (the directory the target test bundle collects) and tells the unit test where it is.
/// The compiler is the one sai-sys checks the bindings with (`SAI_SYS_ABI_CC`: the
/// target's), else `CC`, else `cc` on a native build. Without a compiler the test skips
/// with a message, unless `SAI_SYS_ABI_CHECK=require` (the Nix package): then the build fails.
fn union_check(inc: &Path) {
    let source = Path::new("tests/union_check.c");
    println!("cargo:rerun-if-changed={}", source.display());
    for var in ["SAI_SYS_ABI_CC", "CC", "SAI_SYS_ABI_CHECK"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let required = std::env::var("SAI_SYS_ABI_CHECK").is_ok_and(|v| v == "require");
    let native = std::env::var("HOST").ok() == std::env::var("TARGET").ok();
    let compiler = std::env::var("SAI_SYS_ABI_CC")
        .or_else(|_| std::env::var("CC"))
        .ok()
        .or_else(|| native.then(|| "cc".to_string()));
    let (Some(compiler), Some(out_dir)) = (compiler, std::env::var_os("OUT_DIR")) else {
        assert!(
            !required,
            "sai-ffi: no C compiler for the union test helper (SAI_SYS_ABI_CC)"
        );
        println!("cargo:warning=sai-ffi: no C compiler: the union test will skip");
        return;
    };
    let dir = PathBuf::from(out_dir).join("target-tests");
    let exe = dir.join("union-check");
    let experimental = inc.parent().map(|sai| sai.join("experimental"));
    let mut words = compiler.split_whitespace();
    let Some(program) = words.next() else { return };
    let built = std::fs::create_dir_all(&dir).is_ok()
        && Command::new(program)
            .args(words)
            .args(["-std=gnu11", "-Wall", "-Wextra", "-Werror", "-O1", "-g", "-I"])
            .arg(inc)
            .args(experimental.iter().flat_map(|dir| [Path::new("-I"), dir.as_path()]))
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .is_ok_and(|status| status.success());
    if built {
        println!("cargo:rustc-env=SAI_FFI_UNION_CHECK={}", exe.display());
    } else {
        assert!(
            !required,
            "sai-ffi: tests/union_check.c does not compile with {compiler}"
        );
        println!(
            "cargo:warning=sai-ffi: tests/union_check.c did not compile with {compiler}: the union test will skip"
        );
    }
}

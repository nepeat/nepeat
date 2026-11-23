//! Builds sai-openbcm's C++ `tests/frontend_driver.cpp` out of tree, as its Makefile's
//! `build/frontend_driver` rule does: the library's CORE sources on the fake backend (without
//! `backend_none`: the driver defines `make_openbcm_backend` itself), `recording_backend`,
//! `tests/test_port_cpp.cpp`, `temp_files.c` and `driver_names.inc` (`tests/driver_names.awk`
//! over `$SAI_INC/sai*.h`). Shared by `build.rs` (the prebuilt copy for a target without a
//! compiler) and `tests/differential.rs` (a native build at test time).
#![allow(dead_code, reason = "shared by build.rs and tests/differential.rs; each uses a part")]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The file name of the built C++ driver.
pub(crate) const DRIVER: &str = "sai-cpp-frontend-driver";

/// The library on its fake backend: the Makefile's CORE, and the recording decorator.
const CPP: &[&str] = &[
    "sai_api",
    "sai_switch",
    "sai_port",
    "sai_l2",
    "sai_hostif",
    "sai_l3",
    "sai_lag",
    "sai_temp",
    "sai_caps",
    "sai_diag",
    "sai_acl",
    "sai_qos",
    "sai_mirror",
    "sai_hash",
    "sai_extphy",
    "sai_extmacsec",
    "sai_macsec",
    "fake_backend",
    "recording_backend",
];

/// Where the C++ sources are.
pub(crate) struct Sources {
    /// sai-openbcm's `src`.
    pub(crate) src: PathBuf,
    /// sai-openbcm's `tests` (`frontend_driver.cpp`, `driver-scripts/`).
    pub(crate) tests: PathBuf,
}

fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

/// `SAI_OPENBCM_TESTS`, else `$SAI_OPENBCM_SRC/../tests`; the sources: `SAI_OPENBCM_SRC`, else
/// `<tests>/../src`. `Err` says what is missing.
pub(crate) fn find_sources() -> Result<Sources, String> {
    let tests = var("SAI_OPENBCM_TESTS")
        .map(PathBuf::from)
        .or_else(|| var("SAI_OPENBCM_SRC").map(|s| Path::new(&s).join("../tests")))
        .filter(|t| t.join("frontend_driver.cpp").is_file() && t.join("driver-scripts").is_dir())
        .ok_or("no sai-openbcm tests directory with frontend_driver.cpp and driver-scripts/ (SAI_OPENBCM_TESTS, or SAI_OPENBCM_SRC/../tests)")?;
    let src = var("SAI_OPENBCM_SRC").map_or_else(|| tests.join("../src"), PathBuf::from);
    if !src.join("recording_backend.cpp").is_file() {
        return Err(format!(
            "no sai-openbcm sources with recording_backend.cpp in {}",
            src.display()
        ));
    }
    Ok(Sources { src, tests })
}

/// The op scripts (`driver-scripts/*.ops`), sorted.
pub(crate) fn scripts(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ops"))
        .collect();
    v.sort();
    Ok(v)
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let out = command.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed:\n{}", String::from_utf8_lossy(&out.stderr)))
    }
}

/// Compilers and the build directory.
pub(crate) struct Toolchain<'a> {
    /// C++ compiler.
    pub(crate) cxx: &'a str,
    /// C compiler.
    pub(crate) cc: &'a str,
    /// Link the C++ runtime statically (a target without libstdc++).
    pub(crate) static_runtime: bool,
}

/// Builds the driver into `dir`; returns its path.
pub(crate) fn build(s: &Sources, inc: &Path, tc: &Toolchain<'_>, dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // driver_names.inc: the awk over sai*.h, sorted as the shell's glob
    let mut headers: Vec<PathBuf> = fs::read_dir(inc)
        .map_err(|e| format!("{}: {e}", inc.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "h")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("sai"))
        })
        .collect();
    headers.sort();
    let awk = Command::new("awk")
        .arg("-f")
        .arg(s.tests.join("driver_names.awk"))
        .args(&headers)
        .output()
        .map_err(|e| format!("cannot run awk: {e}"))?;
    if !awk.status.success() {
        return Err(format!("awk failed: {}", String::from_utf8_lossy(&awk.stderr)));
    }
    fs::write(dir.join("driver_names.inc"), &awk.stdout).map_err(|e| e.to_string())?;

    let includes = |c: &mut Command| {
        c.arg("-I")
            .arg(inc)
            .arg("-I")
            .arg(inc.join("../experimental"))
            .arg("-I")
            .arg(&s.src)
            .arg("-I")
            .arg(&s.tests)
            .arg("-I")
            .arg(dir);
    };
    let mut jobs = Vec::new();
    let units = CPP
        .iter()
        .map(|u| (*u, s.src.join(format!("{u}.cpp"))))
        .chain([("test_port_cpp", s.tests.join("test_port_cpp.cpp"))]);
    for (unit, path) in units {
        let mut c = Command::new(tc.cxx);
        c.args(["-std=c++17", "-O1", "-w", "-c"]);
        includes(&mut c);
        c.arg(path).arg("-o").arg(dir.join(format!("{unit}.o")));
        jobs.push((unit, c.spawn().map_err(|e| format!("cannot run {}: {e}", tc.cxx))?));
    }
    let mut c = Command::new(tc.cc);
    c.args(["-std=gnu11", "-O1", "-w", "-c", "-I"]).arg(&s.src);
    c.arg(s.src.join("temp_files.c"))
        .arg("-o")
        .arg(dir.join("temp_files.o"));
    run(c, "the C compiler on temp_files.c")?;
    for (unit, job) in jobs {
        let done = job.wait_with_output().map_err(|e| format!("{unit}: {e}"))?;
        if !done.status.success() {
            return Err(format!(
                "{unit} does not compile:\n{}",
                String::from_utf8_lossy(&done.stderr)
            ));
        }
    }
    let exe = dir.join(DRIVER);
    let mut link = Command::new(tc.cxx);
    link.args(["-std=c++17", "-O1", "-w"]);
    if tc.static_runtime {
        link.args(["-static-libstdc++", "-static-libgcc"]);
    }
    includes(&mut link);
    link.arg(s.tests.join("frontend_driver.cpp"));
    for unit in CPP.iter().chain(&["test_port_cpp", "temp_files"]) {
        link.arg(dir.join(format!("{unit}.o")));
    }
    link.arg("-o").arg(&exe).arg("-lpthread");
    run(link, "the C++ compiler on frontend_driver.cpp")?;
    Ok(exe)
}

/// The build script's part: the prebuilt driver, scripts and allow-list in
/// `$OUT_DIR/target-tests` for a cross build or `SAI_DRIVER_PREBUILD=1`. Returns a note.
pub(crate) fn prebuild(out: &Path) -> Result<String, String> {
    for v in [
        "SAI_DRIVER_PREBUILD",
        "SAI_OPENBCM_TESTS",
        "SAI_OPENBCM_SRC",
        "SAI_INC",
        "SAI_DRIVER_CXX",
        "SAI_DRIVER_CC",
        "SAI_SYS_ABI_CC",
        "CXX",
        "CC",
    ] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let target = var("TARGET").ok_or("TARGET is not set")?;
    let native = var("HOST").is_some_and(|h| h == target);
    let must = match var("SAI_DRIVER_PREBUILD").as_deref() {
        Some("0") => return Ok("not prebuilt (SAI_DRIVER_PREBUILD=0)".to_owned()),
        Some(_) => true,
        None if native => return Ok(String::new()), // a native test run compiles it itself
        None => false,
    };
    let inputs = (|| {
        let s = find_sources()?;
        let inc = var("SAI_INC").map(PathBuf::from).ok_or("SAI_INC is not set")?;
        let abi_cc = var("SAI_SYS_ABI_CC");
        let cxx = var("SAI_DRIVER_CXX")
            .or_else(|| {
                abi_cc
                    .as_ref()
                    .and_then(|cc| cc.strip_suffix("cc").map(|s| format!("{s}c++")))
            })
            .or_else(|| native.then(|| var("CXX").unwrap_or_else(|| "c++".to_owned())))
            .ok_or("no C++ compiler for the target (SAI_DRIVER_CXX)")?;
        let cc = var("SAI_DRIVER_CC")
            .or(abi_cc)
            .or_else(|| native.then(|| var("CC").unwrap_or_else(|| "cc".to_owned())))
            .ok_or("no C compiler for the target (SAI_DRIVER_CC)")?;
        Ok::<_, String>((s, inc, cxx, cc))
    })();
    let (s, inc, cxx, cc) = match inputs {
        Ok(i) => i,
        Err(missing) if must => return Err(missing),
        Err(missing) => return Ok(format!("not prebuilt: {missing}")),
    };
    println!("cargo:rerun-if-changed={}", s.src.display());
    println!("cargo:rerun-if-changed={}", s.tests.display());
    let bin = out.join("target-tests");
    let tc = Toolchain {
        cxx: &cxx,
        cc: &cc,
        static_runtime: true,
    };
    let objects = out.join("cpp-frontend-driver");
    let exe = build(&s, &inc, &tc, &objects)?;
    let data = bin.join("sai-driver");
    let scripts_out = data.join("driver-scripts");
    fs::create_dir_all(&scripts_out).map_err(|e| e.to_string())?;
    fs::copy(&exe, bin.join(DRIVER)).map_err(|e| e.to_string())?;
    for script in scripts(&s.tests.join("driver-scripts"))? {
        if let Some(name) = script.file_name() {
            fs::copy(&script, scripts_out.join(name)).map_err(|e| e.to_string())?;
        }
    }
    let manifest = var("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?;
    let allow = Path::new(&manifest).join("tests/differential-allow.txt");
    println!("cargo:rerun-if-changed={}", allow.display());
    if allow.is_file() {
        fs::copy(&allow, data.join("differential-allow.txt")).map_err(|e| e.to_string())?;
    }
    Ok(format!("prebuilt for the target: {}", bin.join(DRIVER).display()))
}

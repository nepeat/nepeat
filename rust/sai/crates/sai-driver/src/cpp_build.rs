//! Builds the C++ frontend's `tests/frontend_driver.cpp` out of tree, as sai-openbcm's Makefile's
//! `build/frontend_driver` rule did: the library's CORE sources on the fake backend (the driver
//! defines `make_openbcm_backend` itself), `recording_backend`,
//! `tests/test_port_cpp.cpp`, `temp_files.c` and `driver_names.inc` (`tests/driver_names.awk`
//! over `$SAI_INC/sai*.h`). Shared by `build.rs` (the prebuilt copy for a target without a
//! compiler) and `tests/differential.rs` (a native build at test time). The C++ is the frontend
//! frozen on 2026-11-23 in `ai_re/nix/packages/sai-teralynx/frontend/{src,tests}` (the CS8210's
//! libsai still compiles it): the comparison stays live, beside a golden of its output
//! (`tests/golden/differential-cpp.txt`) that the test also checks. The op scripts are this crate's
//! own (`tests/driver-scripts`), not the frontend's.
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
    /// The frozen frontend's `src`.
    pub(crate) src: PathBuf,
    /// The frozen frontend's `tests` (`frontend_driver.cpp`, the test port, the tables).
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
        .filter(|t| t.join("frontend_driver.cpp").is_file())
        .ok_or(
            "no C++ frontend tests directory with frontend_driver.cpp (SAI_OPENBCM_TESTS, or SAI_OPENBCM_SRC/../tests)",
        )?;
    let src = var("SAI_OPENBCM_SRC").map_or_else(|| tests.join("../src"), PathBuf::from);
    if !src.join("recording_backend.cpp").is_file() {
        return Err(format!(
            "no C++ frontend sources with recording_backend.cpp in {}",
            src.display()
        ));
    }
    Ok(Sources { src, tests })
}

/// The tables of the frontend's `tests/test_tables.h` as files in `dir`, for sai-driver's own
/// scripts: the external `MACsec` table (`ms_table()`) and the external PHY table (`kExtPhyGood`,
/// without its hold-off so nothing depends on the clock), printed by a three-line C++ program
/// built with `cxx`. Returns (`MACsec` table, PHY table).
pub(crate) fn tables(tests: &Path, dir: &Path, cxx: &str) -> Result<(PathBuf, PathBuf), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let printer = dir.join("print-tables.cpp");
    fs::write(
        &printer,
        "#include \"test_tables.h\"\n#include <fstream>\nint main(int, char** argv) {\n    std::ofstream(argv[1]) << ms_table();\n    std::ofstream(argv[2]) << kExtPhyGood;\n}\n",
    )
    .map_err(|e| e.to_string())?;
    let exe = dir.join("print-tables");
    let mut words = cxx.split_whitespace();
    let mut c = Command::new(words.next().unwrap_or("c++"));
    c.args(words)
        .arg("-std=c++17")
        .arg("-I")
        .arg(tests)
        .arg(&printer)
        .arg("-o")
        .arg(&exe);
    run(c, &format!("{cxx} on {}", printer.display()))?;
    let (macsec, ext_phy) = (dir.join("ext-macsec.tbl"), dir.join("ext-phy.tbl"));
    let mut p = Command::new(&exe);
    p.arg(&macsec).arg(&ext_phy);
    run(p, "the table printer")?;
    let phy = fs::read_to_string(&ext_phy)
        .map_err(|e| e.to_string())?
        .replace("holdoff-ms 50\n", "holdoff-ms 0\n");
    fs::write(&ext_phy, phy).map_err(|e| e.to_string())?;
    Ok((macsec, ext_phy))
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

/// What differs between builds of `frontend_driver.cpp`: [`Variant::frontend`] is the op-script
/// differential's (the library on its fake backend, `tests/test_port_cpp.cpp`); sai-backend-openbcm's
/// glue differential builds another (`tests/cpp/glue_build.rs` there).
pub(crate) struct Variant {
    /// The file name of the program.
    pub(crate) exe: String,
    /// `-D` options for `frontend_driver.cpp`.
    pub(crate) driver_defines: Vec<String>,
    /// More C++ units: (object name, source, `-D` options).
    pub(crate) cpp: Vec<(String, PathBuf, Vec<String>)>,
    /// More C units: (object name, source).
    pub(crate) c: Vec<(String, PathBuf)>,
    /// Link `tests/test_port_cpp.cpp` (the test port over the fake backend).
    pub(crate) test_port_cpp: bool,
}

impl Variant {
    /// The op-script differential's driver.
    pub(crate) fn frontend() -> Self {
        Self {
            exe: DRIVER.to_owned(),
            driver_defines: Vec::new(),
            cpp: Vec::new(),
            c: Vec::new(),
            test_port_cpp: true,
        }
    }
}

/// Builds the driver into `dir`; returns its path.
pub(crate) fn build(s: &Sources, inc: &Path, tc: &Toolchain<'_>, dir: &Path) -> Result<PathBuf, String> {
    build_variant(s, inc, tc, dir, &Variant::frontend())
}

/// Builds a variant of the driver into `dir`; returns its path.
#[allow(clippy::too_many_lines, reason = "one build recipe")]
pub(crate) fn build_variant(
    s: &Sources,
    inc: &Path,
    tc: &Toolchain<'_>,
    dir: &Path,
    v: &Variant,
) -> Result<PathBuf, String> {
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
    let mut objects: Vec<String> = Vec::new();
    let units = CPP
        .iter()
        .map(|u| ((*u).to_owned(), s.src.join(format!("{u}.cpp")), Vec::new()))
        .chain(v.test_port_cpp.then(|| {
            (
                "test_port_cpp".to_owned(),
                s.tests.join("test_port_cpp.cpp"),
                Vec::new(),
            )
        }))
        .chain(v.cpp.iter().cloned());
    for (unit, path, defines) in units {
        let mut c = Command::new(tc.cxx);
        c.args(["-std=c++17", "-O1", "-w", "-c"]).args(&defines);
        includes(&mut c);
        c.arg(path).arg("-o").arg(dir.join(format!("{unit}.o")));
        jobs.push((
            unit.clone(),
            c.spawn().map_err(|e| format!("cannot run {}: {e}", tc.cxx))?,
        ));
        objects.push(unit);
    }
    let c_units = std::iter::once(("temp_files".to_owned(), s.src.join("temp_files.c"))).chain(v.c.iter().cloned());
    for (unit, path) in c_units {
        let mut c = Command::new(tc.cc);
        c.args(["-std=gnu11", "-O1", "-w", "-c", "-I"]).arg(&s.src);
        c.arg(&path).arg("-o").arg(dir.join(format!("{unit}.o")));
        run(c, &format!("the C compiler on {}", path.display()))?;
        objects.push(unit);
    }
    for (unit, job) in jobs {
        let done = job.wait_with_output().map_err(|e| format!("{unit}: {e}"))?;
        if !done.status.success() {
            return Err(format!(
                "{unit} does not compile:\n{}",
                String::from_utf8_lossy(&done.stderr)
            ));
        }
    }
    let exe = dir.join(&v.exe);
    let mut link = Command::new(tc.cxx);
    link.args(["-std=c++17", "-O1", "-w"]).args(&v.driver_defines);
    if tc.static_runtime {
        link.args(["-static-libstdc++", "-static-libgcc"]);
    }
    includes(&mut link);
    link.arg(s.tests.join("frontend_driver.cpp"));
    for unit in &objects {
        link.arg(dir.join(format!("{unit}.o")));
    }
    link.arg("-o").arg(&exe).arg("-lpthread");
    run(link, "the C++ compiler on frontend_driver.cpp")?;
    Ok(exe)
}

/// The build script's part: the prebuilt driver, scripts and allow-list in
/// `$OUT_DIR/target-tests` for a cross build or `SAI_DRIVER_PREBUILD=1`. Returns a note.
pub(crate) fn prebuild(out: &Path) -> Result<String, String> {
    prebuild_variant(
        out,
        &Prebuild {
            switch: "SAI_DRIVER_PREBUILD",
            data: "sai-driver",
            allow: "differential-allow.txt",
        },
        |_| Variant::frontend(),
    )
}

/// What a prebuild is called and where its data go.
pub(crate) struct Prebuild<'a> {
    /// The environment variable that forces (`1`) or forbids (`0`) it; unset: cross builds only.
    pub(crate) switch: &'a str,
    /// The directory under `target-tests` for the scripts and the allow-list.
    pub(crate) data: &'a str,
    /// The allow-list's file name in the crate's `tests`, copied beside the scripts.
    pub(crate) allow: &'a str,
}

/// [`prebuild`] of any variant: the driver built for the target into `$OUT_DIR/target-tests`, with
/// the frontend's op scripts and the crate's allow-list in `target-tests/<data>/`. Returns a note.
pub(crate) fn prebuild_variant(
    out: &Path,
    p: &Prebuild<'_>,
    variant: impl FnOnce(&Sources) -> Variant,
) -> Result<String, String> {
    for v in [
        p.switch,
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
    let must = match var(p.switch).as_deref() {
        Some("0") => return Ok(format!("not prebuilt ({}=0)", p.switch)),
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
    if let Some(m) = var("CARGO_MANIFEST_DIR") {
        println!("cargo:rerun-if-changed={m}/tests/driver-scripts");
    }
    let v = variant(&s);
    for (_, path, _) in &v.cpp {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for (_, path) in &v.c {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let bin = out.join("target-tests");
    let tc = Toolchain {
        cxx: &cxx,
        cc: &cc,
        static_runtime: true,
    };
    let objects = out.join(format!("cpp-{}", v.exe));
    let exe = build_variant(&s, &inc, &tc, &objects, &v)?;
    let data = bin.join(p.data);
    let scripts_out = data.join("driver-scripts");
    fs::create_dir_all(&scripts_out).map_err(|e| e.to_string())?;
    fs::copy(&exe, bin.join(&v.exe)).map_err(|e| e.to_string())?;
    // the crate's own scripts (01-09 are the frontend's, moved here unchanged); the ones that name a
    // table token need the C++ tables at run time and stay out of the bundle, as before
    let manifest = var("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?;
    for script in scripts(&Path::new(&manifest).join("tests/driver-scripts"))? {
        let text = fs::read_to_string(&script).map_err(|e| format!("{}: {e}", script.display()))?;
        if text.contains("@EXT_PHY_TABLE@") || text.contains("@EXT_MACSEC_TABLE@") {
            continue;
        }
        if let Some(name) = script.file_name() {
            fs::copy(&script, scripts_out.join(name)).map_err(|e| e.to_string())?;
        }
    }
    let allow = Path::new(&manifest).join("tests").join(p.allow);
    println!("cargo:rerun-if-changed={}", allow.display());
    if allow.is_file() {
        fs::copy(&allow, data.join(p.allow)).map_err(|e| e.to_string())?;
    }
    Ok(format!("prebuilt for the target: {}", bin.join(&v.exe).display()))
}

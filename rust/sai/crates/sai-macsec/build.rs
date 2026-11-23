//! Prebuilds the C++ side of the differential test (`tests/differential.rs`) for runs on a
//! machine without a compiler: `tests/cpp/driver.cpp` and the unmodified sai-openbcm sources
//! on their fake backend, as `$OUT_DIR/target-tests/sai-macsec-cpp-driver`, and its "chip" variant
//! `sai-macsec-cpp-chip-driver` for sai-macsec-phy's `tests/e2e.rs` (the test bundle copies that
//! directory; the tests find the files through `$SAI_RS_TEST_BIN`).
//!
//! Only when it is needed: a cross build (`TARGET != HOST`), or `SAI_MACSEC_PREBUILD_DRIVER=1`
//! (then a missing input fails the build); never with `SAI_MACSEC_PREBUILD_DRIVER=0`. A native
//! test run compiles the driver itself.
//!
//! Inputs: `SAI_OPENBCM_SRC` (or `MDIO_SCRIPT_C_DIR`, the same directory in the Nix build),
//! `SAI_INC`, and the target's compilers: `SAI_MACSEC_CXX` / `SAI_MACSEC_CC`, else derived from
//! `SAI_SYS_ABI_CC` (the C compiler sai-sys checks the ABI with: `...cc` and `...c++`), else for a
//! native build `CXX` / `CC`.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DRIVER: &str = "sai-macsec-cpp-driver";
const CHIP_DRIVER: &str = "sai-macsec-cpp-chip-driver";
/// The library on its fake backend: the Makefile's CORE and `backend_none`.
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
    "backend_none",
];

fn var(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}

/// What to build with and from; `Err` says what is missing.
struct Inputs {
    src: PathBuf,
    inc: PathBuf,
    driver: PathBuf,
    cxx: String,
    cc: String,
}

fn inputs(native: bool) -> Result<Inputs, String> {
    let src = [var("SAI_OPENBCM_SRC"), var("MDIO_SCRIPT_C_DIR")]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .find(|d| d.join("sai_macsec.cpp").is_file())
        .ok_or("no sai-openbcm sources (SAI_OPENBCM_SRC = its src directory)")?;
    let inc = var("SAI_INC").map(PathBuf::from).ok_or("SAI_INC is not set")?;
    let driver = Path::new(&required("CARGO_MANIFEST_DIR")?).join("tests/cpp/driver.cpp");
    if !driver.is_file() {
        return Err(format!("{} is not in this source tree", driver.display()));
    }
    println!("cargo:rerun-if-changed={}", driver.display());
    println!("cargo:rerun-if-changed={}", src.display());
    let abi_cc = var("SAI_SYS_ABI_CC");
    let cxx = var("SAI_MACSEC_CXX")
        .or_else(|| {
            abi_cc
                .as_ref()
                .and_then(|cc| cc.strip_suffix("cc").map(|stem| format!("{stem}c++")))
        })
        .or_else(|| {
            if native {
                Some(var("CXX").unwrap_or_else(|| "c++".to_owned()))
            } else {
                None
            }
        })
        .ok_or("no C++ compiler for the target (SAI_MACSEC_CXX)")?;
    let cc = var("SAI_MACSEC_CC")
        .or(abi_cc)
        .or_else(|| {
            if native {
                Some(var("CC").unwrap_or_else(|| "cc".to_owned()))
            } else {
                None
            }
        })
        .ok_or("no C compiler for the target (SAI_MACSEC_CC)")?;
    Ok(Inputs {
        src,
        inc,
        driver,
        cxx,
        cc,
    })
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let out = command.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed:\n{}", String::from_utf8_lossy(&out.stderr)))
    }
}

fn build(i: &Inputs, out: &Path) -> Result<PathBuf, String> {
    let objects = out.join("cpp-driver");
    let bin = out.join("target-tests");
    for dir in [&objects, &bin] {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let includes = |c: &mut Command| {
        c.arg("-I")
            .arg(&i.inc)
            .arg("-I")
            .arg(i.inc.join("../experimental"))
            .arg("-I")
            .arg(&i.src);
    };
    // every translation unit at once
    let mut jobs = Vec::new();
    for unit in CPP {
        let mut c = Command::new(&i.cxx);
        c.args(["-std=c++17", "-O1", "-w", "-c"]);
        includes(&mut c);
        c.arg(i.src.join(format!("{unit}.cpp")))
            .arg("-o")
            .arg(objects.join(format!("{unit}.o")));
        jobs.push((*unit, c.spawn().map_err(|e| format!("cannot run {}: {e}", i.cxx))?));
    }
    let mut c = Command::new(&i.cc);
    c.args(["-std=gnu11", "-O1", "-w", "-c", "-I"]).arg(&i.src);
    c.arg(i.src.join("temp_files.c"))
        .arg("-o")
        .arg(objects.join("temp_files.o"));
    run(c, "the C compiler on temp_files.c")?;
    for (unit, job) in jobs {
        let done = job.wait_with_output().map_err(|e| format!("{unit}: {e}"))?;
        if !done.status.success() {
            return Err(format!(
                "{unit}.cpp does not compile:\n{}",
                String::from_utf8_lossy(&done.stderr)
            ));
        }
    }
    let exe = bin.join(DRIVER);
    // the driver, and its "chip" variant for sai-macsec-phy's tests/e2e.rs (its own backend
    // factory instead of backend_none, driver.cpp's SAI_MACSEC_DRIVER_CHIP)
    for (name, chip) in [(DRIVER, false), (CHIP_DRIVER, true)] {
        let mut link = Command::new(&i.cxx);
        // the C++ runtime inside: the machine it runs on need not have a libstdc++
        link.args(["-std=c++17", "-O1", "-Wall", "-static-libstdc++", "-static-libgcc"]);
        if chip {
            link.arg("-DSAI_MACSEC_DRIVER_CHIP");
        }
        includes(&mut link);
        link.arg(&i.driver);
        for unit in CPP.iter().chain(&["temp_files"]) {
            if !(chip && *unit == "backend_none") {
                link.arg(objects.join(format!("{unit}.o")));
            }
        }
        link.arg("-o").arg(bin.join(name)).arg("-lpthread");
        run(link, "the C++ compiler on the driver")?;
    }
    Ok(exe)
}

fn prebuild() -> Result<String, String> {
    println!("cargo:rerun-if-changed=build.rs");
    let native = required("TARGET")? == required("HOST")?;
    let must = match var("SAI_MACSEC_PREBUILD_DRIVER").as_deref() {
        Some("0") => return Ok("not prebuilt (SAI_MACSEC_PREBUILD_DRIVER=0)".to_owned()),
        Some(_) => true,
        None if native => return Ok(String::new()), // a native test run compiles it itself
        None => false,
    };
    let out = PathBuf::from(required("OUT_DIR")?);
    match inputs(native) {
        Ok(i) => build(&i, &out).map(|exe| format!("prebuilt for the target: {}", exe.display())),
        Err(missing) if must => Err(missing),
        Err(missing) => Ok(format!("not prebuilt: {missing}")),
    }
}

fn main() {
    match prebuild() {
        Ok(note) if note.is_empty() => {}
        Ok(note) => println!("cargo:warning=C++ differential driver {note}"),
        Err(why) => {
            eprintln!("sai-macsec build.rs: SAI_MACSEC_PREBUILD_DRIVER is set, but: {why}");
            std::process::exit(1);
        }
    }
}

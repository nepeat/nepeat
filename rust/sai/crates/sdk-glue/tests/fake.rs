//! The MDIO bring-up and its wrappers through the plain bring-up (`Sdk::init_mdio`), i.e. on the
//! process's Rust fake (`sdk_glue::fake` = `rust_fake::process` in a build without the real glue):
//! the path the MDIO tool's tests take. Once the C fake's tests; the per-chip versions are in
//! `rust_fake.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use sdk_glue::{Code, Error, InitError, Mmd, Sdk};

/// The glue's state is one per process: one test at a time.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn scratch() -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("sdk-glue-fake-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scenario(name: &str, text: &str) -> PathBuf {
    let path = scratch().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn init_error(text: &str) -> (Error, String) {
    match Sdk::init_mdio(0, &scenario("fail.cfg", text)) {
        Err(InitError::Sdk { error, message }) => (error, message),
        other => panic!("expected the glue to refuse, got {other:?}"),
    }
}

const D1: Mmd = Mmd::new(1).unwrap();

#[test]
fn bring_up_then_registers_read_back() {
    let _g = serial();
    let trace = scratch().join("trace.txt");
    let _ = std::fs::remove_file(&trace);
    let cfg = scenario(
        "ok.cfg",
        &format!(
            "# scenario\nname TEST56960_B1\nreg 24 1 8b00 2391\ntrace {}\n",
            trace.display()
        ),
    );
    let (mut sdk, line) = Sdk::init_mdio(0, &cfg).unwrap();
    assert_eq!(
        line,
        "TEST56960_B1: init soc + init misc done; external MDIO rate register 0x000d0001, hold delay 13 (as configured)"
    );
    assert_eq!(sdk.unit(), 0);
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b00), Ok(0x2391));
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b01), Ok(0));
    assert_eq!(sdk.mdio_c45_write(0x17c, Mmd::new(31).unwrap(), 0xffff, 0xfffe), Ok(()));
    assert_eq!(sdk.mdio_c45_read(0x17c, Mmd::new(31).unwrap(), 0xffff), Ok(0xfffe));
    drop(sdk);
    // After init_mdio there is no BCM layer: dropping the handle calls nothing (as the C tool).
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        "init unit 0\nR 24 1 8b00 = 2391\nR 24 1 8b01 = 0000\nW 17c 1f ffff fffe\nR 17c 1f ffff = fffe\n"
    );
}

#[test]
fn one_handle_at_a_time_and_again_after_drop() {
    let _g = serial();
    let cfg = scenario("one.cfg", "");
    let (sdk, _) = Sdk::init_mdio(0, &cfg).unwrap();
    // refused twice: a refused attempt must not release the live handle's claim
    assert_eq!(Sdk::init_mdio(0, &cfg).unwrap_err(), InitError::AlreadyOpen);
    assert_eq!(Sdk::init_mdio(0, &cfg).unwrap_err(), InitError::AlreadyOpen);
    assert_eq!(sdk.unit(), 0);
    drop(sdk);
    let (mut sdk, _) = Sdk::init_mdio(0, &cfg).unwrap();
    assert_eq!(sdk.mdio_c45_read(1, D1, 2), Ok(0));
}

#[test]
fn bring_up_failures_carry_code_and_reason() {
    let _g = serial();
    let missing = scratch().join("no-such.cfg");
    let _ = std::fs::remove_file(&missing);
    match Sdk::init_mdio(0, &missing) {
        Err(InitError::Sdk { error, message }) => {
            assert_eq!(error.code(), Code::Config);
            assert_eq!(
                message,
                format!("SDK config file '{}' missing or unreadable (2)", missing.display())
            );
            assert_eq!(error.message(), "Invalid configuration");
        }
        other => panic!("{other:?}"),
    }
    let (e, m) = init_error("init busy\n");
    assert_eq!(e.code(), Code::Busy);
    assert!(
        m.starts_with("another SDK process owns the switch (lock /run/openbcm-unit0.lock held"),
        "{m}"
    );
    let (e, m) = init_error("init delay\n");
    assert_eq!((e.code(), e.raw()), (Code::MdioClock, -100));
    assert!(m.contains("refusing MDIO access"), "{m}");
    assert_eq!(e.to_string(), "MDIO clock setup not as configured");
    assert_eq!(
        init_error("init soc\n"),
        (Error::from_raw(-11).unwrap(), "init soc failed (-1)".to_owned())
    );
    assert_eq!(init_error("init unit\n").0.code(), Code::Unit);
    assert_eq!(init_error("bogus line\n").0.code(), Code::Config);
    // a failed bring-up leaves no claim behind
    let (sdk, _) = Sdk::init_mdio(0, &scenario("ok2.cfg", "")).unwrap();
    drop(sdk);
    assert_eq!(
        Sdk::init_mdio(1, &scenario("ok2.cfg", ""))
            .map(|_| ())
            .unwrap_err()
            .to_string(),
        "no attached switch unit (soc_ndev 1) (Invalid unit)"
    );
    assert_eq!(
        Sdk::init_mdio(0, std::path::Path::new("a\0b")).unwrap_err(),
        InitError::BadPath
    );
}

#[test]
fn scripted_faults_reach_the_caller() {
    let _g = serial();
    let cfg = scenario(
        "faults.cfg",
        "const 24 1900\ndead 26\nafter 2 1 1 8221 0303\nhang 2 1 1900\nfail-read 5\nfail-write 3\n",
    );
    let (mut sdk, _) = Sdk::init_mdio(0, &cfg).unwrap();
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b00), Ok(0x1900));
    assert_eq!(sdk.mdio_c45_read(0x26, D1, 0x8b00).unwrap_err().code(), Code::Timeout);
    assert_eq!(
        sdk.mdio_c45_write(0x26, D1, 0x8b00, 1).unwrap_err().code(),
        Code::Timeout
    );
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0));
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0));
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221).unwrap_err().code(), Code::Fail); // the 5th read
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0x0303));
    assert_eq!(sdk.mdio_c45_write(0, D1, 1, 7), Ok(())); // 2nd write: phy 1 hangs from here
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0x1900));
    assert_eq!(sdk.mdio_c45_write(0, D1, 1, 8).unwrap_err().code(), Code::Fail); // the 3rd write
    assert_eq!(sdk.mdio_c45_read(0, D1, 1), Ok(7));
}

#[test]
fn temperatures() {
    let _g = serial();
    let (mut sdk, _) = Sdk::init_mdio(0, &scenario("t0.cfg", "")).unwrap();
    assert_eq!(sdk.temperatures().unwrap_err().code(), Code::Unavail);
    drop(sdk);
    let (mut sdk, _) = Sdk::init_mdio(0, &scenario("t1.cfg", "temp 413 398 -20\ntemp-fail 3\n")).unwrap();
    let t = sdk.temperatures().unwrap();
    assert_eq!(t.current(), &[413, 398, -20]);
    assert_eq!(t.peak(), &[428, 413, -5]);
    assert_eq!(sdk.temperatures().unwrap().current().len(), 3);
    assert_eq!(sdk.temperatures().unwrap_err().code(), Code::Timeout);
    drop(sdk);
    let many = format!(
        "temp {}\n",
        (0..16).map(|i| (400 + i).to_string()).collect::<Vec<_>>().join(" ")
    );
    let (mut sdk, _) = Sdk::init_mdio(0, &scenario("t2.cfg", &many)).unwrap();
    let t = sdk.temperatures().unwrap();
    assert_eq!(t.current().len(), 16);
    assert_eq!(t.current()[15], 415);
}

#[test]
fn codes_and_messages() {
    assert_eq!(Error::from_raw(0), None);
    assert_eq!(Error::from_raw(5), None);
    let names = [
        (-1, Code::Internal, "Internal error"),
        (-4, Code::Param, "Invalid parameter"),
        (-9, Code::Timeout, "Operation timed out"),
        (-10, Code::Busy, "Operation still running"),
        (-16, Code::Unavail, "Feature unavailable"),
        (-18, Code::Port, "Invalid port"),
        (-57, Code::Other, "Unknown error"),
    ];
    for (raw, code, text) in names {
        let e = Error::from_raw(raw).unwrap();
        assert_eq!((e.raw(), e.code(), e.message().as_str()), (raw, code, text));
    }
    assert_eq!(Mmd::new(0), None);
    assert_eq!(Mmd::new(32), None);
    assert_eq!(Mmd::new(31).map(Mmd::get), Some(31));
    assert!(sdk_glue::HEADER_CHECK.starts_with("checked") || sdk_glue::HEADER_CHECK.starts_with("skipped"));
}

//! Runs the hardware smoke program (`smoke/sai_smoke.c`) against a `libsai.so` built with the
//! openbcm backend on the FAKE glue: the program's own logic (refusals, clean-up, the slice
//! through dlopen) without hardware.
//!
//! `cargo test` of this crate does not build that library (the workspace's libsai.so has no
//! openbcm backend unless asked), so the test needs
//!   `SAI_SMOKE_LIBSAI`  a libsai.so from `cargo build -p sai-ffi --features openbcm-fake`
//! and skips with a message without it. The program: `$SAI_RS_TEST_BIN/sai-openbcm-smoke`
//! (the test bundle) or the one this crate's build script made.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, reason = "test code")]

mod common;

use common::{note, scenario};
use std::path::PathBuf;
use std::process::{Command, Output};

fn program() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SAI_RS_TEST_BIN") {
        let exe = PathBuf::from(dir).join("sai-openbcm-smoke");
        if exe.is_file() {
            return Some(exe);
        }
    }
    let built = env!("SAI_OPENBCM_SMOKE_BIN");
    (!built.is_empty())
        .then(|| PathBuf::from(built))
        .filter(|p| p.is_file())
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_smoke_program_refuses_without_the_confirmation() {
    let Some(exe) = program() else {
        note("SKIPPED: the smoke program was not built (needs SAI_INC and a C compiler at build time)");
        return;
    };
    let profile = scenario("smoke-refuse.profile", "SAI_INIT_CONFIG_FILE=/nonexistent\n");
    // no flag: refused before the library is even loaded (the path does not exist)
    let out = Command::new(&exe)
        .args(["--libsai", "/nonexistent/libsai.so", "--profile"])
        .arg(&profile)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("refusing: creating the switch resets the switch chip"),
        "{}",
        text(&out)
    );
    // usage errors
    let out = Command::new(&exe).arg("--sonic-is-stopped").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(&exe)
        .args(["--sonic-is-stopped", "--libsai", "x", "--vlan", "1", "--profile"])
        .arg(&profile)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out).contains("--vlan: 2..4094"));
}

#[test]
fn the_smoke_program_passes_on_the_fake_glue_and_cleans_up_after_a_failure() {
    let (Some(exe), Some(libsai)) = (program(), std::env::var_os("SAI_SMOKE_LIBSAI")) else {
        note("SKIPPED: needs the smoke program and SAI_SMOKE_LIBSAI=<libsai.so built with --features openbcm-fake>");
        return;
    };
    let trace = common::scratch().join("smoke-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let config = scenario(
        "smoke.cfg",
        &format!(
            "name BCM56960_B1\nports 32 4 100000\nlink admin\ntrace {}\n",
            trace.display()
        ),
    );
    let profile = scenario(
        "smoke.profile",
        &format!(
            "# as SONiC writes it\nSAI_INIT_CONFIG_FILE={}\nSAI_OPENBCM_PORT_INTERFACE=caui\nSAI_OPENBCM_PORT_AUTONEG=off\n\
             SAI_OPENBCM_PORT_FEC=none\nSAI_OPENBCM_PORT_IFG=128\n",
            config.display()
        ),
    );
    let run = |extra: &[&str]| {
        Command::new(&exe)
            .arg("--sonic-is-stopped")
            .arg("--libsai")
            .arg(&libsai)
            .arg("--profile")
            .arg(&profile)
            .args(extra)
            .output()
            .unwrap()
    };
    let out = run(&[
        "--expect-ports",
        "32",
        "--oper",
        "0,1,30,31",
        "--admin",
        "30",
        "--vlan",
        "100",
        "--wait",
        "1",
    ]);
    let all = text(&out);
    println!("{all}");
    assert!(out.status.success() && all.contains("SMOKE_OK"), "{all}");
    note(all.lines().find(|l| l.starts_with("SMOKE_OK")).unwrap_or("SMOKE_OK"));
    assert!(all.contains("32 front-panel ports"), "{all}");
    assert!(all.contains("ADMIN port 30 up: oper UP"), "{all}");
    assert!(all.contains("VLAN 100 created with port 30 untagged"), "{all}");
    assert!(all.contains("OPER port 31: down"), "{all}");
    // what reached the chip, in order, after the core's creation pass (each port disabled, its
    // quirks applied): port 31 up and down again, the VLAN and its member created and removed,
    // and the shutdown at the end
    // (the board quirks reach every port twice: in the backend's own bring-up, a first-slice
    // leftover, and from the core's creation pass as the C++ frontend applies them; G2 drops the
    // backend's: openbcm_backend.cpp has none)
    let chip = std::fs::read_to_string(&trace).unwrap();
    let after_init = chip.rsplit("port 32 ifg 128\n").next().unwrap();
    let tail: Vec<&str> = after_init
        .lines()
        .filter(|l| !l.contains("enable 0") || l.starts_with("port 31 "))
        .collect();
    assert_eq!(
        tail,
        [
            "port 31 enable 1",
            "port 31 enable 0",
            "vlan 100 create",
            "vlan 100 add 31 untagged",
            "vlan 100 remove 31",
            "vlan 100 destroy",
            "unregister unit 0",
            "unregister unit 0",
            "shutdown unit 0"
        ],
        "{chip}"
    );

    // a failed check (33 ports expected) still cleans up and exits 1
    let _ = std::fs::remove_file(&trace);
    let out = run(&["--expect-ports", "33", "--admin", "0"]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(
        all.contains("SMOKE_FAILED") && all.contains("32 ports, expected 33"),
        "{all}"
    );
    assert!(std::fs::read_to_string(&trace).unwrap().ends_with("shutdown unit 0\n"));

    // a chip that does not come up: exit 1, nothing left
    let busy = scenario("smoke-busy.cfg", "ports 32 4 100000\ninit busy\n");
    let busy_profile = scenario(
        "smoke-busy.profile",
        &format!("SAI_INIT_CONFIG_FILE={}\n", busy.display()),
    );
    let out = Command::new(&exe)
        .arg("--sonic-is-stopped")
        .arg("--libsai")
        .arg(&libsai)
        .arg("--profile")
        .arg(&busy_profile)
        .output()
        .unwrap();
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("another SDK process owns the switch"), "{all}");
}

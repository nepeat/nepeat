//! The binary itself, with the SDK backend on the fake glue: real signals, systemd's status
//! line, and the guard that keeps a fake-glue build from passing for the tool.
//! `SAI_RS_TEST_BIN` names the directory with `openbcm-mdio-script-rs` where the build tree
//! is not (a copied test executable).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{put, scratch, script_text};

fn exe() -> PathBuf {
    std::env::var_os("SAI_RS_TEST_BIN")
        .map(|d| PathBuf::from(d).join("openbcm-mdio-script-rs"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openbcm-mdio-script-rs")))
}

fn tool() -> Command {
    let mut c = Command::new(exe());
    c.env("MDIO_TOOL_FAKE_SDK", "1")
        .env("TZ", "UTC")
        .env_remove("NOTIFY_SOCKET");
    c
}

const HEALTHY: &str = "reg 0 1 8b00 2391\nreg 0 1 8b01 80a0\nreg 0 1 8215 d00b\nreg 0 1 8251 2d\n\
                       reg 1 1 8b00 2391\nreg 1 1 8b01 80a0\nreg 1 1 8215 d00b\nreg 1 1 8251 2d\nreg 1 1 8221 303\n";

#[test]
fn a_fake_glue_build_does_not_pass_for_the_tool() {
    let dir = scratch("guard");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let cfg = put(&dir, "chip.cfg", HEALTHY);
    let o = tool()
        .env_remove("MDIO_TOOL_FAKE_SDK")
        .args(["--config"])
        .arg(&cfg)
        .arg(&script)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("built without the SDK"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn systemd_is_told_what_is_going_on() {
    let dir = scratch("notify");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let cfg = put(&dir, "chip.cfg", HEALTHY);
    let socket = dir.join("notify");
    let server = UnixDatagram::bind(&socket).unwrap();
    server.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let o = tool()
        .env("NOTIFY_SOCKET", &socket)
        .arg("--config")
        .arg(&cfg)
        .arg("--status-file")
        .arg(dir.join("status.json"))
        .arg(&script)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let mut got = Vec::new();
    let mut buf = [0u8; 2048];
    for _ in 0..3 {
        let n = server.recv(&mut buf).unwrap();
        got.push(String::from_utf8_lossy(&buf[..n]).into_owned());
    }
    assert_eq!(got[0], "STATUS=bringing up the SDK (init soc, init misc)");
    assert_eq!(got[1], "STATUS=running the script");
    assert!(
        got[2].starts_with("STATUS=ok: 2 PHYs verified after 13 operations in "),
        "{got:?}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// SIGTERM from outside while the script sits in a long delay: the tool stops at the next tick
/// (within about half a second), writes nothing more to the chip, records "interrupted", exits 7.
#[test]
fn sigterm_stops_the_run_between_two_operations() {
    let dir = scratch("sigterm");
    let script = put(
        &dir,
        "init.mdio",
        "mdio-script 1\nL long step\nW 0 1 1 1\nD 20000000\nW 0 1 2 2\nP 0 1 2 2 ffff 1 0\n",
    );
    let trace = dir.join("trace.txt");
    let cfg = put(&dir, "chip.cfg", &format!("trace {}\ntemp 400\n", trace.display()));
    let (json, temp) = (dir.join("status.json"), dir.join("temperature"));
    let child = tool()
        .arg("--config")
        .arg(&cfg)
        .arg("--status-file")
        .arg(&json)
        .arg("--temp-out")
        .arg(&temp)
        .arg(&script)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // wait for the first write, and for the temperature file that is published meanwhile
    let deadline = Instant::now() + Duration::from_secs(20);
    while !(std::fs::read_to_string(&trace).is_ok_and(|t| t.contains("W 0 1 1 1\n")) && temp.exists()) {
        assert!(Instant::now() < deadline, "the tool never wrote");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&temp).unwrap(), "40.0\n");
    let running = std::fs::read_to_string(&json).unwrap();
    assert!(
        running.contains("\"state\": \"running\"") && running.contains("\"finished\": null"),
        "{running}"
    );
    let sent = Instant::now();
    let kill = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(kill.success());
    let o = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(7), "{err}");
    assert!(
        sent.elapsed() < Duration::from_secs(5),
        "stopping took {:?}",
        sent.elapsed()
    );
    assert!(
        err.contains(":4: stopped on request during a delay [long step]\n"),
        "{err}"
    );
    assert!(
        err.contains("FAILED (interrupted): stopped on request (signal); 1 of 4 operations had run\n"),
        "{err}"
    );
    let t = std::fs::read_to_string(&trace).unwrap();
    assert!(!t.contains("W 0 1 2 2"), "{t}");
    let j = std::fs::read_to_string(&json).unwrap();
    assert!(
        j.contains("\"state\": \"interrupted\"") && j.contains("\"class\": \"interrupted\""),
        "{j}"
    );
    assert!(!temp.exists(), "the temperature file must be removed on the way out");
    std::fs::remove_dir_all(dir).unwrap();
}

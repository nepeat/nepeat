//! The tool in this process over a chip written in Rust: every path of the run, without any C.
//! (The port of the command-line cases of sai-openbcm's `tests/test_mdio_script.c`, and the
//! cases that the C test could not make because they need the SDK.)
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;

use std::ffi::OsStr;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::{Board, NAMES, Outcome, open_error, put, scratch, script_text, tool};
use mdio_tool::OpenFailure;

fn os(parts: &[&dyn AsRef<OsStr>]) -> Vec<std::ffi::OsString> {
    parts.iter().map(|p| p.as_ref().to_owned()).collect()
}

fn go(args: &[std::ffi::OsString], board: Option<&Arc<Board>>) -> Outcome {
    let refs: Vec<&OsStr> = args.iter().map(std::ffi::OsString::as_os_str).collect();
    tool(&refs, board, Path::new("/nonexistent-lock-dir"))
}

fn has(text: &str, needle: &str) {
    assert!(text.contains(needle), "missing: {needle}\nin: {text}");
}

fn read(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn a_run_that_succeeds() {
    let dir = scratch("ok");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let names = put(&dir, "names", NAMES);
    let (json, temp, prom) = (
        dir.join("st/status.json"),
        dir.join("asic/temperature"),
        dir.join("asic/t.prom"),
    );
    let board = Arc::new(Board {
        temps: vec![413, 418],
        ..Board::default()
    });
    let r = go(
        &os(&[
            &"--config",
            &"/etc/config.bcm",
            &"--status-file",
            &json,
            &"--names",
            &names,
            &"--temp-out",
            &temp,
            &"--temp-prom",
            &prom,
            &script,
        ]),
        Some(&board),
    );
    assert_eq!((r.code, r.out.as_str(), r.armed), (0, "", 1), "{}", r.err);
    let s = script.display();
    let lines: Vec<&str> = r.err.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "openbcm-mdio-script: {s}: 3 writes, 9 reads (9 polls), 1 delays (0.0 s), 3 steps; verification: 8 reads from line 8"
        )
    );
    assert_eq!(
        lines[1],
        "openbcm-mdio-script: MOCK56960_B1: init soc + init misc done; external MDIO rate register 0x000d0001, hold delay 13 (as configured)"
    );
    assert_eq!(
        lines[3],
        format!(
            "openbcm-mdio-script: ASIC temperature: 2 monitors, max 41.8 C; every 5 s to {} while the script runs",
            temp.display()
        )
    );
    assert_eq!(lines[2], "openbcm-mdio-script: init");
    assert_eq!(lines[4], "openbcm-mdio-script: verify phy 0x0");
    assert_eq!(lines[5], "openbcm-mdio-script: verify phy 0x1");
    assert!(lines[6].starts_with(&format!("openbcm-mdio-script: {s}: done in 0.0 s: 13 of 13 operations, 3 writes, 9 reads (9 polls, 0 re-reads), 0 expected values differed")));
    assert!(
        lines[7].starts_with("openbcm-mdio-script: ok: 2 PHYs verified after 13 operations in 0."),
        "{}",
        lines[7]
    );
    assert_eq!(lines.len(), 8);
    // one bring-up, the script's writes and no others; the first temperature read is discarded
    assert_eq!(
        (
            board.opens.load(Ordering::Relaxed),
            board.writes.load(Ordering::Relaxed)
        ),
        (1, 3)
    );
    assert_eq!(board.temp_reads.load(Ordering::Relaxed), 2);
    // the temperature files are removed at the end; their directory was created
    assert!(dir.join("asic").is_dir() && !temp.exists() && !prom.exists());
    let j = read(&json);
    for needle in [
        "\"state\": \"ok\",\n \"mode\": \"run\",\n \"summary\": \"2 PHYs verified after 13 operations in 0.",
        "\"operations_done\": 13,\n \"operations_total\": 13,\n \"sdk\": \"MOCK56960_B1: init soc",
        "\"result\": \"ok\"",
        "\"failure\": null",
        "\"asic_temperature\": {\"readings\": 1, \"failed\": 0, \"last_celsius\": 41.8, \"longest_read_ms\": 0.",
        "\"verified\": {\"when\": \"the end of the script\", \"phys_ok\": 2, \"phys_total\": 2,",
        "{\"phy\": \"0x1\", \"name\": \"cage2\", \"state\": \"ok\"",
    ] {
        has(&j, needle);
    }
    let t = read(&dir.join("st/status.txt"));
    assert!(t.starts_with("openbcm-mdio-script: ok: 2 PHYs verified after 13 operations"));
    has(&t, "cage2  0x001  2391    80a0    d00b    002d    ok\n");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_poll_that_never_matches_then_the_post_mortem() {
    let dir = scratch("poll");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let json = dir.join("status.json");
    let board = Arc::new(Board {
        stuck: true,
        ..Board::default()
    });
    let r = go(
        &os(&[&"--config", &"c", &"--status-file", &json, &script]),
        Some(&board),
    );
    assert_eq!(r.code, 2, "{}", r.err);
    has(
        &r.err,
        &format!(
            "{}:6: phy 1 1.8221 reads 0000, script wants 0303 (mask ffff) after 5 reads [init]\n",
            script.display()
        ),
    );
    has(
        &r.err,
        "FAILED (sequence-diverged): init.mdio line 6 [init]: phy 0x1 1.8221 read 0x0000, the script expects 0x0303 (mask 0xffff) after 5 reads; 3 of 13 operations had run; the PHY still answers (its registers are not one constant): it left the recorded sequence\n",
    );
    has(&r.err, "what to do: start the unit again; if it stops at the same step");
    // W W D ran; the poll read 5 times; the post-mortem read the 8 assertions and wrote nothing
    assert_eq!(
        (
            board.writes.load(Ordering::Relaxed),
            board.reads.load(Ordering::Relaxed)
        ),
        (2, 13)
    );
    let j = read(&json);
    has(&j, "\"state\": \"failed\"");
    has(&j, "\"class\": \"sequence-diverged\"");
    has(&j, "\"result\": \"poll\"");
    has(&j, "\"line\": 6, \"label\": \"init\", \"op\": \"P\"");
    has(
        &j,
        "\"when\": \"after the failure (reads only)\", \"phys_ok\": 2, \"phys_total\": 2",
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_phy_that_hangs_during_the_script() {
    let dir = scratch("hang");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let json = dir.join("status.json");
    let board = Arc::new(Board {
        hang_after_writes: Some(3),
        ..Board::default()
    });
    let r = go(
        &os(&[
            &"--config",
            &"c",
            &"--status-file",
            &json,
            &"--hint",
            &"phy-not-answering=cold power cycle (lab plug 1)",
            &"--hint",
            &"verify-mismatch=other",
            &script,
        ]),
        Some(&board),
    );
    assert_eq!(r.code, 2, "{}", r.err);
    has(
        &r.err,
        "openbcm-mdio-script: PHY registers read at the end of the script:\nname  phy    1.8b00  1.8b01  1.8215  1.8251  state\n",
    );
    has(
        &r.err,
        "-     0x001  1900!   1900!   1900!   1900!   CONSTANT 0x1900: not answering or hung\n",
    );
    has(
        &r.err,
        "FAILED (phy-not-answering): init.mdio line 14 [verify phy 0x1]: verification: 1 of 2 PHYs differ: phy 0x1 reads 0x1900 on every verified register; 13 of 13 operations had run; phy 0x1 reads 0x1900 on every verified register: not answering or hung\n",
    );
    has(
        &r.err,
        "openbcm-mdio-script: what to do: cold power cycle (lab plug 1)\n",
    );
    let j = read(&json);
    has(
        &j,
        "\"class\": \"phy-not-answering\", \"hint\": \"cold power cycle (lab plug 1)\"",
    );
    has(&j, "\"result\": \"verify\"");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_stop_request_is_honoured_between_two_operations() {
    let dir = scratch("stop");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let (json, temp) = (dir.join("status.json"), dir.join("temperature"));
    let board = Arc::new(Board {
        stop_after_writes: Some(1),
        temps: vec![400],
        ..Board::default()
    });
    let r = go(
        &os(&[
            &"--config",
            &"c",
            &"--status-file",
            &json,
            &"--temp-out",
            &temp,
            &script,
        ]),
        Some(&board),
    );
    assert_eq!(r.code, 7, "{}", r.err);
    // the write in flight completed, the next operation did not start, nothing was read back
    assert_eq!(
        (
            board.writes.load(Ordering::Relaxed),
            board.reads.load(Ordering::Relaxed)
        ),
        (1, 0)
    );
    has(
        &r.err,
        &format!("{}:4: stopped on request before this line [init]\n", script.display()),
    );
    has(
        &r.err,
        "FAILED (interrupted): init.mdio line 4 [init]: stopped on request (signal); 1 of 13 operations had run\n",
    );
    has(
        &r.err,
        "what to do: the PHYs may be half-programmed: start the unit again\n",
    );
    let j = read(&json);
    has(&j, "\"state\": \"interrupted\"");
    has(&j, "\"class\": \"interrupted\"");
    has(&j, "\"result\": \"aborted\"");
    has(&j, "\"line\": 4, \"label\": \"init\", \"op\": \"\"}");
    has(&j, "\"verified\": null");
    assert!(read(&dir.join("status.txt")).starts_with("openbcm-mdio-script: FAILED (interrupted): "));
    assert!(!temp.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_mdio_write_error_and_scripts_that_were_not_run() {
    let dir = scratch("io");
    let a = put(&dir, "a.mdio", "mdio-script 1\nL a\nW 0 1 1 1\n");
    let b = put(&dir, "b.mdio", &script_text(&["0"]));
    let c = put(&dir, "c.mdio", "mdio-script 1\nW 0 1 2 2\n");
    let json = dir.join("status.json");
    let board = Arc::new(Board {
        fail_write: Some(3),
        ..Board::default()
    });
    let r = go(
        &os(&[&"--config", &"c", &"--status-file", &json, &a, &b, &c]),
        Some(&board),
    );
    assert_eq!(r.code, 2, "{}", r.err);
    has(
        &r.err,
        "FAILED (mdio-io): b.mdio line 4 [init]: MDIO write of phy 0x1 1.8201 failed in the switch chip's MDIO master; 2 of 11 operations had run\n",
    );
    let j = read(&json);
    has(&j, "a.mdio\", \"result\": \"ok\"");
    has(&j, "b.mdio\", \"result\": \"io\"");
    has(&j, "c.mdio\", \"result\": null");
    has(&j, "\"read\": null, \"reads\": 0}");
    has(
        &read(&dir.join("status.txt")),
        &format!("  {}: not run (1 operations)\n", c.display()),
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn temperature_reads_that_fail_remove_the_files() {
    let dir = scratch("temp");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let (json, temp) = (dir.join("status.json"), dir.join("temperature"));
    let board = Arc::new(Board {
        temps: vec![400],
        temp_fail_from: Some(2),
        ..Board::default()
    });
    let r = go(
        &os(&[
            &"--config",
            &"c",
            &"--status-file",
            &json,
            &"--temp-out",
            &temp,
            &script,
        ]),
        Some(&board),
    );
    assert_eq!(r.code, 0, "{}", r.err);
    has(
        &r.err,
        "openbcm-mdio-script: ASIC temperature read failed: Operation timed out (0 monitors); files removed\n",
    );
    has(
        &read(&json),
        "\"asic_temperature\": {\"readings\": 0, \"failed\": 1, \"last_celsius\": 0.0,",
    );
    // an implausible value is a failed read too
    let board = Arc::new(Board {
        temps: vec![400, 1501],
        ..Board::default()
    });
    let r = go(&os(&[&"--config", &"c", &"--temp-out", &temp, &script]), Some(&board));
    assert_eq!(r.code, 0);
    has(
        &r.err,
        "ASIC temperature read failed: ok (2 monitors, implausible value); files removed\n",
    );
    assert!(!temp.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_sdk_does_not_come_up() {
    let dir = scratch("open");
    let script = put(&dir, "init.mdio", &script_text(&["0"]));
    let json = dir.join("status.json");
    for (failure, message, code, exit, class, summary) in [
        (
            OpenFailure::Busy,
            "another SDK process owns the switch (lock /run/openbcm-unit0.lock held; e.g. systemctl stop openbcm-asic-temp)",
            "Operation still running",
            3,
            "sdk-busy",
            "another SDK process owns the switch (lock /run/openbcm-unit0.lock held; e.g. systemctl stop openbcm-asic-temp); nothing was written to any PHY",
        ),
        (
            OpenFailure::Config,
            "SDK config file 'c' missing or unreadable (2)",
            "Invalid configuration",
            4,
            "sdk-config-missing",
            "SDK config file 'c' missing or unreadable (2) (Invalid configuration); nothing was written to any PHY",
        ),
        (
            OpenFailure::MdioClock,
            "MDIO hold delay on the chip is 2, the config's mdio_output_delay is 13: refusing MDIO access",
            "MDIO clock setup not as configured",
            4,
            "mdio-clock",
            "MDIO hold delay on the chip is 2, the config's mdio_output_delay is 13: refusing MDIO access (MDIO clock setup not as configured); nothing was written to any PHY",
        ),
        (
            OpenFailure::Init,
            "init misc failed (-1)",
            "Operation failed",
            4,
            "sdk-init",
            "init misc failed (-1) (Operation failed); nothing was written to any PHY",
        ),
    ] {
        let board = Arc::new(Board {
            open_error: Some(open_error(failure, message, code)),
            ..Board::default()
        });
        let _ = std::fs::remove_file(&json);
        let r = go(
            &os(&[&"--config", &"c", &"--status-file", &json, &script]),
            Some(&board),
        );
        assert_eq!(r.code, exit, "{}", r.err);
        has(&r.err, &format!("openbcm-mdio-script: FAILED ({class}): {summary}\n"));
        let j = read(&json);
        has(&j, &format!("\"class\": \"{class}\""));
        has(&j, "\"result\": null");
        has(&j, "\"sdk\": \"\"");
        assert_eq!(
            (
                board.writes.load(Ordering::Relaxed),
                board.reads.load(Ordering::Relaxed)
            ),
            (0, 0)
        );
        // read-only: says why, points at the diag socket, leaves the last run's record alone
        std::fs::remove_file(&json).unwrap();
        let r = go(
            &os(&[&"--verify-only", &"--config", &"c", &"--status-file", &json, &script]),
            Some(&board),
        );
        assert_eq!(r.code, exit);
        assert!(!json.exists());
        has(
            &r.err,
            &format!(
                "openbcm-mdio-script: {}",
                if failure == OpenFailure::Busy {
                    summary.replace("; nothing was written to any PHY", "")
                } else {
                    summary.to_owned()
                }
            ),
        );
        assert_eq!(
            r.err
                .contains("while syncd owns the chip, read through it: --verify-only --diag-socket SOCKET"),
            failure == OpenFailure::Busy
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_and_scripts_that_do_not_parse() {
    let dir = scratch("parse");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let json = dir.join("status.json");
    let board = Arc::new(Board::default());
    // --check: any build, nothing opened
    let r = go(&os(&[&"--check", &script]), None);
    assert_eq!((r.code, r.armed), (0, 0));
    has(&r.err, "3 writes");
    has(&r.err, "verification: 8 reads from line 8");
    // a build without the SDK
    let r = go(&os(&[&"--config", &"c", &script]), None);
    assert_eq!(r.code, 1);
    has(
        &r.err,
        "openbcm-mdio-script: built without the SDK: --check and --verify-only --diag-socket only\n",
    );
    // a missing script: exit 5, both status files say why, nothing was opened
    let missing = dir.join("missing.mdio");
    let r = go(
        &os(&[
            &"--config",
            &"c",
            &"--status-file",
            &json,
            &"--hint",
            &"script-missing=run install-script.sh",
            &script,
            &missing,
        ]),
        Some(&board),
    );
    assert_eq!((r.code, board.opens.load(Ordering::Relaxed)), (5, 0));
    has(
        &r.err,
        &format!(
            "FAILED (script-missing): {}: No such file or directory; nothing was written to any PHY\n",
            missing.display()
        ),
    );
    has(&r.err, "what to do: run install-script.sh\n");
    let j = read(&json);
    has(&j, "\"class\": \"script-missing\", \"hint\": \"run install-script.sh\"");
    has(&j, "\"state\": \"failed\"");
    has(&j, "\"verified\": null");
    has(&j, "\"operations_done\": 0,\n \"operations_total\": 13,");
    assert!(read(&dir.join("status.txt")).starts_with("openbcm-mdio-script: FAILED (script-missing): "));
    // a truncated one
    let bad = put(&dir, "bad.mdio", "mdio-script 1\nW 0 1 1 1\nW 0 1 2\n");
    let r = go(&os(&[&"--config", &"c", &"--status-file", &json, &bad]), Some(&board));
    assert_eq!((r.code, board.opens.load(Ordering::Relaxed)), (5, 0));
    has(
        &r.err,
        &format!(
            "FAILED (script-malformed): {}:3: bad arguments for 'W' (clause 45 only: dev 1..1f); nothing was written to any PHY\n",
            bad.display()
        ),
    );
    has(&r.err, "what to do: the file is truncated");
    has(&read(&json), "\"class\": \"script-malformed\"");
    // clause 22 cannot be asked for
    let c22 = put(&dir, "c22.mdio", "mdio-script 1\nW 0 0 1 1\n");
    assert_eq!(go(&os(&[&"--config", &"c", &c22]), Some(&board)).code, 5);
    assert_eq!(board.opens.load(Ordering::Relaxed), 0);
    // --check of a bad script: one line, no status file
    std::fs::remove_file(&json).unwrap();
    let r = go(&os(&[&"--check", &"--status-file", &json, &bad]), None);
    assert_eq!(r.code, 5);
    assert_eq!(
        r.err,
        format!(
            "openbcm-mdio-script: {}:3: bad arguments for 'W' (clause 45 only: dev 1..1f)\n",
            bad.display()
        )
    );
    assert!(!json.exists());
    // each message of the C interpreter, with the path
    for (text, message) in [
        ("", ": empty file"),
        (
            "mdio-script 2\n",
            ": not an MDIO script (first line must be 'mdio-script 1')",
        ),
        ("mdio-script 1\nbogus\n", ":2: unknown operation"),
        ("mdio-script 1\nX 1 1 2 3\n", ":2: unknown operation 'X'"),
        ("mdio-script 1\nD -5\n", ":2: D <usec up to 60000000>"),
    ] {
        let p = put(&dir, "m.mdio", text);
        let r = go(&os(&[&"--check", &p]), None);
        assert_eq!(r.err, format!("openbcm-mdio-script: {}{message}\n", p.display()));
    }
    // a names file that cannot be used is said and ignored
    let r = go(&os(&[&"--check", &"--names", &missing, &script]), None);
    assert_eq!(r.code, 0);
    has(
        &r.err,
        &format!(
            "openbcm-mdio-script: {}: No such file or directory (names ignored)\n",
            missing.display()
        ),
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn verify_only_reads_and_never_writes() {
    let dir = scratch("verify");
    let script = put(&dir, "init.mdio", &script_text(&["0", "1"]));
    let names = put(&dir, "names", NAMES);
    let json = dir.join("status.json");
    let sock = dir.join("diag.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let commands = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        scope.spawn(|| common::diag_server(&listener, &commands));
        let _stop = common::StopOnDrop(&sock);
        // through the diag socket: all good; no SDK, the status file is not written
        let r = go(
            &os(&[
                &"--verify-only",
                &"--diag-socket",
                &sock,
                &"--names",
                &names,
                &"--status-file",
                &json,
                &script,
            ]),
            None,
        );
        assert_eq!((r.code, r.err.as_str(), r.armed), (0, "", 1));
        assert_eq!(
            r.out,
            "name   phy    id      1.8b01  fw      1.8251  state\n\
             cage1  0x000  2391    80a0    d00b    002d    ok\n\
             cage2  0x001  2391    80a0    d00b    002d    ok\n\
             2 of 2 PHYs as the script expects\n"
        );
        assert!(!json.exists());
        // only reads went over the socket
        let sent = std::mem::take(&mut *commands.lock().unwrap());
        assert_eq!(sent.len(), 8);
        assert_eq!(sent[0], "phy raw c45 0x0 0x1 0x8b00\n");
        assert!(sent.iter().all(|c| c.starts_with("phy raw c45 0x")));
        let r = go(
            &os(&[&"--verify-only", &"--diag-socket", &sock, &"--json", &script]),
            None,
        );
        assert_eq!(r.code, 0);
        has(&r.out, "\"mode\": \"verify\"");
        has(&r.out, "\"phys_ok\": 2, \"phys_total\": 2");
        has(&r.out, "\"failure\": null");
        has(
            &r.out,
            &format!("\"sdk\": \"through syncd's diag socket {}\"", sock.display()),
        );
        has(&r.out, "\"summary\": \"2 PHYs verified, all as the script expects\"");
        // a hung PHY and one the bus master fails on: exit 2, the rows and the class say so
        let bad = put(&dir, "bad.mdio", &script_text(&["0", "24", "26"]));
        let r = go(
            &os(&[
                &"--verify-only",
                &"--diag-socket",
                &sock,
                &"--names",
                &names,
                &"--hint",
                &"phy-not-answering=cold power cycle (lab plug 1)",
                &bad,
            ]),
            None,
        );
        assert_eq!(r.code, 2);
        has(
            &r.out,
            "cage5  0x024  1900!   1900!   1900!   1900!   CONSTANT 0x1900: not answering or hung\n",
        );
        has(&r.out, "-      0x026  error!  error!  error!  error!  MDIO ERROR\n");
        has(
            &r.out,
            "2 of 3 PHYs are not as the script expects (phy-not-answering): verification: 2 of 3 PHYs differ: cage5 (phy 0x24) reads 0x1900 on every verified register; phy 0x26 id (1.8b00): read failed; 17 of 17 operations had run; cage5 (phy 0x24) reads 0x1900 on every verified register: not answering or hung\n",
        );
        has(&r.out, "what to do: cold power cycle (lab plug 1)\n");
        assert_eq!(r.err, "openbcm-mdio-script: openbcm-diag: command failed (-1)\n");
    });
    // with the SDK itself: reads only
    let board = Arc::new(Board::default());
    let r = go(
        &os(&[&"--verify-only", &"--config", &"c", &"--status-file", &json, &script]),
        Some(&board),
    );
    assert_eq!(r.code, 0);
    assert_eq!(
        (
            board.writes.load(Ordering::Relaxed),
            board.reads.load(Ordering::Relaxed)
        ),
        (0, 8)
    );
    has(&r.out, "2 of 2 PHYs as the script expects\n");
    has(&r.err, "MOCK56960_B1: init soc + init misc done");
    assert!(!json.exists());
    // no verification section; no socket and no config
    let plain = put(&dir, "plain.mdio", "mdio-script 1\nW 0 1 1 1\n");
    let r = go(&os(&[&"--verify-only", &"--diag-socket", &sock, &plain]), None);
    assert_eq!(r.code, 5);
    has(&r.err, "no script has a verification section");
    let r = go(&os(&[&"--verify-only", &script]), None);
    assert_eq!(r.code, 1);
    has(&r.err, "usage:");
    // nobody listening
    std::fs::remove_file(&sock).unwrap();
    let r = go(&os(&[&"--verify-only", &"--diag-socket", &sock, &script]), None);
    assert_eq!(r.code, 2);
    has(
        &r.err,
        "No such file or directory (is syncd running with SAI_OPENBCM_DIAG_SOCKET?)\n",
    );
    std::fs::remove_dir_all(dir).unwrap();
}

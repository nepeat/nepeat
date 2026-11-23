//! Shared by the test binaries: scripts, a chip in Rust, a fake of syncd's diag socket, and
//! running the tool in this process.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]
#![allow(dead_code, reason = "each test binary uses a part of this module")]

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use mdio_script::{BusError, Dev, MdioBus};
use mdio_tool::{Chip, ChipOpener, Env, OpenError, OpenFailure, Temperatures};

pub(crate) const BODY: &str =
    "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 5 10\nW 1 1 1 1\n";

pub(crate) fn verify(phy: &str) -> String {
    format!(
        "L verify phy 0x{phy}\nP {phy} 1 8b00 2391 ffff 1 0\nP {phy} 1 8b01 80a0 ffff 1 0\n\
         P {phy} 1 8215 d00b ffff 1 0\nP {phy} 1 8251 000d 000d 1 0\n"
    )
}

pub(crate) fn script_text(phys: &[&str]) -> String {
    let section: String = phys.iter().map(|p| verify(p)).collect();
    format!("{BODY}{section}")
}

pub(crate) const NAMES: &str = "phy 0 cage1\nphy 1 cage2\nphy 24 cage5\nreg 1.8b00 id\nreg 1.8215 fw\n";

/// A directory of this test.
pub(crate) fn scratch(name: &str) -> PathBuf {
    // Short on purpose: unix socket paths are limited to 107 bytes.
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("mdio-tool-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn put(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// The register values of the C test's bus (`tests/test_mdio_script.c`, `b_read`).
pub(crate) fn bus_value(phy: u16, reg: u16, stuck: bool, hang: bool) -> Result<u16, BusError> {
    Ok(match (phy, reg) {
        (0x26, _) => return Err(BusError),
        (0x24, _) => 0x1900,
        (1, _) if hang => 0x1900,
        (_, 0x8b00) => 0x2391,
        (_, 0x8b01) => 0x80a0,
        (0x25, 0x8215) => 0x0001,
        (_, 0x8215) => 0xd00b,
        (_, 0x8251) => 0x002d,
        (_, 0x8221) => {
            if stuck {
                0
            } else {
                0x0303
            }
        }
        _ => 0,
    })
}

/// What a test wants the chip to do, and what the chip saw.
#[derive(Default)]
pub(crate) struct Board {
    pub(crate) open_error: Option<OpenError>,
    pub(crate) stuck: bool,
    /// PHY 1 reads a constant once this many writes were made.
    pub(crate) hang_after_writes: Option<u32>,
    /// The n-th write fails.
    pub(crate) fail_write: Option<u32>,
    /// "SIGTERM" right after the n-th write.
    pub(crate) stop_after_writes: Option<u32>,
    pub(crate) temps: Vec<i32>,
    /// Temperature reads from the n-th on fail.
    pub(crate) temp_fail_from: Option<u32>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) opens: AtomicU32,
    pub(crate) writes: AtomicU32,
    pub(crate) reads: AtomicU32,
    pub(crate) temp_reads: AtomicU32,
}

struct MockChip(Arc<Board>);

impl MdioBus for MockChip {
    fn read(&mut self, phy: u16, _dev: Dev, reg: u16) -> Result<u16, BusError> {
        let b = &self.0;
        b.reads.fetch_add(1, Ordering::Relaxed);
        let hang = b
            .hang_after_writes
            .is_some_and(|n| b.writes.load(Ordering::Relaxed) >= n);
        bus_value(phy, reg, b.stuck, hang)
    }
    fn write(&mut self, _phy: u16, _dev: Dev, _reg: u16, _val: u16) -> Result<(), BusError> {
        let b = &self.0;
        let n = b.writes.fetch_add(1, Ordering::Relaxed) + 1;
        if b.fail_write == Some(n) {
            return Err(BusError);
        }
        if b.stop_after_writes == Some(n) {
            b.stop.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

impl Chip for MockChip {
    fn temperatures(&mut self) -> Result<Temperatures, String> {
        let b = &self.0;
        let n = b.temp_reads.fetch_add(1, Ordering::Relaxed) + 1;
        if b.temp_fail_from.is_some_and(|from| n >= from) {
            return Err("Operation timed out".to_owned());
        }
        Ok(Temperatures {
            current: b.temps.clone(),
            peak: b.temps.iter().map(|t| t + 15).collect(),
        })
    }
}

struct MockOpener(Arc<Board>);

impl ChipOpener for MockOpener {
    fn open(&mut self, unit: i32, config: &OsStr) -> Result<(Box<dyn Chip>, String), OpenError> {
        self.0.opens.fetch_add(1, Ordering::Relaxed);
        assert_eq!(unit, 0);
        assert!(!config.is_empty());
        match &self.0.open_error {
            Some(e) => Err(e.clone()),
            None => Ok((
                Box::new(MockChip(Arc::clone(&self.0))),
                "MOCK56960_B1: init soc + init misc done; external MDIO rate register 0x000d0001, hold delay 13 (as configured)"
                    .to_owned(),
            )),
        }
    }
}

pub(crate) fn open_error(failure: OpenFailure, message: &str, code_text: &str) -> OpenError {
    OpenError {
        failure,
        message: message.to_owned(),
        code_text: code_text.to_owned(),
    }
}

pub(crate) struct Outcome {
    pub(crate) code: u8,
    pub(crate) out: String,
    pub(crate) err: String,
    pub(crate) armed: u32,
}

/// Runs the tool in this process. `board`: the SDK backend; `None`: a build without the SDK.
pub(crate) fn tool(args: &[&OsStr], board: Option<&Arc<Board>>, lock_dir: &Path) -> Outcome {
    let args: Vec<OsString> = args.iter().map(|a| (*a).to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut armed = 0;
    let never = AtomicBool::new(false);
    let mut opener = board.map(|b| MockOpener(Arc::clone(b)));
    let mut env = Env {
        out: &mut out,
        err: &mut err,
        stop: board.map_or(&never, |b| &b.stop),
        arm_signals: &mut || armed += 1,
        opener: opener.as_mut().map(|o| o as &mut dyn ChipOpener),
        notify_socket: None,
        lock_dir: lock_dir.to_owned(),
    };
    let code = mdio_tool::run(OsStr::new("openbcm-mdio-script"), &args, &mut env);
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        armed,
    }
}

/// A fake of syncd's diag socket (`sai_diag.cpp`): one command line per connection, answered
/// like the SDK shell's "phy raw c45" from the bus above; anything else is refused. Serves
/// until `stop` is connected to with the command "stop".
pub(crate) fn diag_server(listener: &UnixListener, commands: &std::sync::Mutex<Vec<String>>) {
    loop {
        let (mut stream, _) = listener.accept().unwrap();
        let mut cmd = String::new();
        stream.read_to_string(&mut cmd).unwrap();
        if cmd == "stop" {
            return;
        }
        commands.lock().unwrap().push(cmd.clone());
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let num = |i: usize| {
            words
                .get(i)
                .and_then(|w| w.strip_prefix("0x"))
                .and_then(|w| u16::from_str_radix(w, 16).ok())
        };
        let answer = match (words.get(..3), num(3), num(4), num(5)) {
            (Some(["phy", "raw", "c45"]), Some(phy), Some(_), Some(reg)) => match bus_value(phy, reg, false, false) {
                Ok(v) => format!("\t0x{reg:04x}: 0x{v:04x}\n"),
                Err(BusError) => "openbcm-diag: command failed (-1)\n".to_owned(),
            },
            _ => format!("openbcm-diag: refused (would end or re-init the SDK, or chains commands): {cmd}"),
        };
        stream.write_all(answer.as_bytes()).unwrap();
    }
}

/// Stops the server when dropped, so a failed assertion inside the scope does not hang the test.
pub(crate) struct StopOnDrop<'a>(pub(crate) &'a Path);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(self.0) {
            let _ = s.write_all(b"stop");
        }
    }
}

//! The server over a handler of its own (no core): the lock is never held while the server
//! waits for a client, a backend that keeps a copy of the output descriptor does not hang a
//! command, a large output reaches a slow client after the lock is free again.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use sai_diag::{DiagHandler, DiagHost, DiagServer, Level, MacsecRun, ShellCommand, ShellFailed};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Chip {
    /// Bytes each shell command writes.
    output: usize,
    /// Copies of the output descriptor the "SDK" kept.
    leaked: Vec<OwnedFd>,
    ran: Vec<String>,
}

impl DiagHandler for Chip {
    fn switch_created(&self) -> bool {
        true
    }
    fn ext_phy_status(&mut self) -> String {
        "phy\n".into()
    }
    fn ext_macsec_status(&mut self) -> String {
        "macsec\n".into()
    }
    fn ext_macsec_trace(&mut self, args: &str) -> String {
        format!("trace[{args}]\n")
    }
    fn ext_macsec_run(&mut self, run: &MacsecRun<'_>) -> String {
        format!("run[{}|{}|{:?}]\n", run.port, run.sequence, run.param_file)
    }
    fn shell(&mut self, command: &ShellCommand<'_>, out: BorrowedFd<'_>) -> Result<(), ShellFailed> {
        self.ran.push(command.line.to_owned());
        let fd = out.try_clone_to_owned().map_err(|_| ShellFailed(-1))?;
        let mut file = std::fs::File::from(fd.try_clone().map_err(|_| ShellFailed(-1))?);
        self.leaked.push(fd);
        let body = vec![b'o'; self.output];
        file.write_all(&body).map_err(|_| ShellFailed(-1))?;
        if command.line == "fail" {
            Err(ShellFailed(-7))
        } else {
            Ok(())
        }
    }
}

struct Host {
    chip: Mutex<Chip>,
    uid: u32,
}

impl DiagHost for Host {
    fn with_handler(&self, body: &mut dyn FnMut(Option<&mut dyn DiagHandler>)) {
        let mut chip = self.chip.lock().unwrap();
        body(Some(&mut *chip));
    }
    fn peer_uid(&self, _conn: BorrowedFd<'_>) -> Option<u32> {
        Some(self.uid)
    }
    fn log(&self, _level: Level, _text: &str) {}
}

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("sai-diag-srv-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn serve(dir: &Path, output: usize) -> (Arc<Host>, DiagServer, PathBuf) {
    let host = Arc::new(Host {
        chip: Mutex::new(Chip {
            output,
            ..Chip::default()
        }),
        uid: std::fs::metadata(dir).unwrap().uid(),
    });
    let path = dir.join("d.sock");
    let as_dyn: Arc<dyn DiagHost> = host.clone();
    let server = DiagServer::start(path.to_str().unwrap(), as_dyn).unwrap();
    (host, server, path)
}

fn ask(path: &Path, line: &str) -> Vec<u8> {
    let mut s = UnixStream::connect(path).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    s.shutdown(Shutdown::Write).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    out
}

#[test]
fn own_commands_reach_the_handler_as_parsed() {
    let dir = scratch("own");
    let (_host, _server, path) = serve(&dir, 3);
    assert_eq!(ask(&path, "extphy\n"), b"phy\n");
    assert_eq!(ask(&path, "extmacsec\n"), b"macsec\n");
    assert_eq!(ask(&path, "extmacsec trace on all\n"), b"trace[ on all]\n");
    assert_eq!(
        ask(&path, "extmacsec run ce1 x.ingress /k\n"),
        b"run[ce1|x.ingress|Some(\"/k\")]\n"
    );
    assert_eq!(ask(&path, "ps\n"), b"ooo");
    assert_eq!(ask(&path, "fail\n"), b"oooopenbcm-diag: command failed (-7)\n");
}

#[test]
fn a_backend_keeping_the_descriptor_does_not_hang_the_command() {
    let dir = scratch("leak");
    let (host, _server, path) = serve(&dir, 5);
    for _ in 0..3 {
        assert_eq!(ask(&path, "ps\n"), b"ooooo");
    }
    assert_eq!(host.chip.lock().unwrap().leaked.len(), 3);
}

#[test]
fn the_lock_is_free_while_a_slow_client_takes_a_large_answer() {
    let dir = scratch("slow");
    let (host, _server, path) = serve(&dir, 4 << 20);
    let mut s = UnixStream::connect(&path).unwrap();
    s.write_all(b"l2 show\n").unwrap();
    s.shutdown(Shutdown::Write).unwrap();
    // the command runs (4 MiB into the spool), then the lock is released although the client
    // has not read a byte: the server is now blocked writing to the client, not under the lock
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        {
            let chip = host.chip.lock().unwrap();
            if chip.ran.len() == 1 {
                break;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let t = Instant::now();
    drop(host.chip.lock().unwrap());
    assert!(t.elapsed() < Duration::from_millis(500));
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    assert_eq!(out.len(), 4 << 20);
}

#[test]
fn a_client_that_never_reads_is_dropped_after_the_write_timeout() {
    let dir = scratch("noread");
    let (host, server, path) = serve(&dir, 4 << 20);
    let mut s = UnixStream::connect(&path).unwrap();
    s.write_all(b"l2 show\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // stop waits until the server gives up on the client (a write without progress for 5 s; the
    // first write's own 5 s end with the part the socket buffer took), then removes the socket
    let t = Instant::now();
    drop(server);
    assert!(t.elapsed() < Duration::from_secs(13), "{:?}", t.elapsed());
    assert!(!path.exists());
    assert_eq!(host.chip.lock().unwrap().ran, ["l2 show"]);
    drop(s);
}

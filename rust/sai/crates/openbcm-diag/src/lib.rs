//! `openbcm-diag` in Rust, the port of sai-openbcm's `openbcm_diag.c`: runs one Broadcom SDK
//! diag-shell command inside syncd (which owns the chip) through the SAI library's diag socket
//! (`SAI_OPENBCM_DIAG_SOCKET`; server: crate `sai-diag`).
//!
//! ```text
//! openbcm-diag [-s SOCKET] COMMAND...
//! ```
//!
//! Default socket: `$OPENBCM_DIAG_SOCKET`, else `/run/sai-openbcm/diag.sock`. Root only (the
//! server checks the peer's uid). Exit 0 when connected, 1 when the server refused or the command
//! failed (its answer holds `openbcm-diag: `), 2 when there is no server or the command line is
//! unusable. The same command line, bytes sent, output and exit codes as the C tool
//! (`tests/differential.rs` holds it to that), including its quirks:
//! - `-s` is an option only with at least one word after its argument: `openbcm-diag -s` sends
//!   the command `-s`;
//! - an empty word adds no space when nothing came before it;
//! - a socket path of 108 bytes or more ends the run with exit 2 and no message, as does a short
//!   write of the command;
//! - the failure marker is looked for in each piece read (4096 bytes at most), so a marker split
//!   across two reads is missed.
//!
//! Difference: on a closed standard output the C tool dies of `SIGPIPE`; this one (std ignores
//! `SIGPIPE`) stops copying the answer and exits with the status so far.
#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;

/// The socket when neither `-s` nor `OPENBCM_DIAG_SOCKET` names one.
pub const DEFAULT_SOCKET: &str = "/run/sai-openbcm/diag.sock";
/// The environment variable naming the socket.
pub const SOCKET_ENV: &str = "OPENBCM_DIAG_SOCKET";
/// The C buffer for the command line (`char cmd[1024]`).
const CMD_BUF: usize = 1024;
/// `sizeof(sockaddr_un::sun_path)`.
const SUN_PATH: usize = 108;
/// The C read buffer.
const READ_BUF: usize = 4096;
/// What the server's failures start with.
const MARKER: &[u8] = b"openbcm-diag: ";

/// The exit code: connected and the answer had no failure in it.
pub const EXIT_OK: u8 = 0;
/// The server refused or the command failed.
pub const EXIT_FAILED: u8 = 1;
/// No server, or a command line that cannot be sent.
pub const EXIT_UNUSABLE: u8 = 2;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The command line as the C tool joins it, newline included; `None` when it does not fit.
fn join(words: &[OsString]) -> Option<Vec<u8>> {
    let mut cmd = Vec::with_capacity(CMD_BUF);
    for word in words {
        let word = word.as_bytes();
        // C: len + l + 2 >= sizeof(cmd): room for the space, the newline and the NUL
        if cmd.len().saturating_add(word.len()).saturating_add(2) >= CMD_BUF {
            return None;
        }
        if !cmd.is_empty() {
            cmd.push(b' ');
        }
        cmd.extend_from_slice(word);
    }
    cmd.push(b'\n');
    Some(cmd)
}

/// The whole tool: `args` with the program name first, the value of [`SOCKET_ENV`], standard
/// output and error. Returns the exit code.
pub fn run(args: &[OsString], env_socket: Option<&OsStr>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let program = args.first().map_or(&b"openbcm-diag"[..], |p| p.as_bytes());
    let mut path = env_socket
        .map(OsStrExt::as_bytes)
        .filter(|p| !p.is_empty())
        .unwrap_or(DEFAULT_SOCKET.as_bytes());
    let mut first = 1;
    if args.len() > 2 && args.get(1).is_some_and(|a| a == "-s") {
        path = args.get(2).map_or(path, |p| p.as_bytes());
        first = 3;
    }
    let words = args.get(first..).unwrap_or_default();
    if words.is_empty() {
        let mut usage = b"usage: ".to_vec();
        usage.extend_from_slice(program);
        usage.extend_from_slice(b" [-s SOCKET] COMMAND...   e.g. ");
        usage.extend_from_slice(program);
        usage.extend_from_slice(b" l2 station show\n");
        let _ = err.write_all(&usage);
        return EXIT_UNUSABLE;
    }
    let Some(cmd) = join(words) else {
        let _ = err.write_all(b"openbcm-diag: command too long\n");
        return EXIT_UNUSABLE;
    };
    if path.len() >= SUN_PATH {
        return EXIT_UNUSABLE;
    }
    let Ok(mut conn) = UnixStream::connect(OsStr::from_bytes(path)) else {
        let mut m = MARKER.to_vec();
        m.extend_from_slice(path);
        m.extend_from_slice(b": no syncd diag socket (SAI_OPENBCM_DIAG_SOCKET set? syncd running?)\n");
        let _ = err.write_all(&m);
        return EXIT_UNUSABLE;
    };
    // One write, as in C: a short one is a failure.
    if !conn.write(&cmd).is_ok_and(|n| n == cmd.len()) {
        return EXIT_UNUSABLE;
    }
    let _ = conn.shutdown(Shutdown::Write);
    let mut failed = EXIT_OK;
    let mut buf = [0u8; READ_BUF];
    loop {
        let piece = match conn.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.get(..n).unwrap_or_default(),
        };
        if contains(piece, MARKER) {
            failed = EXIT_FAILED;
        }
        if out.write_all(piece).is_err() {
            break; // C: SIGPIPE ends the process
        }
    }
    let _ = out.flush();
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_command_line_is_joined_as_in_c() {
        assert_eq!(join(&os(&["l2", "station", "show"])).unwrap(), b"l2 station show\n");
        assert_eq!(join(&os(&["", "ps"])).unwrap(), b"ps\n");
        assert_eq!(join(&os(&["ps", "", "x"])).unwrap(), b"ps  x\n");
        assert_eq!(join(&os(&[&"x".repeat(1021)])).unwrap().len(), 1022);
        assert_eq!(join(&os(&[&"x".repeat(1022)])), None);
        // the C check does not count the space: 500 + 1 + 521 + newline = 1023 bytes fit
        assert_eq!(
            join(&os(&[&"x".repeat(500), &"y".repeat(521)])).map(|c| c.len()),
            Some(1023)
        );
        assert_eq!(join(&os(&[&"x".repeat(500), &"y".repeat(522)])).map(|c| c.len()), None);
    }

    #[test]
    fn usage_and_unusable_sockets() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(&os(&["od"]), None, &mut out, &mut err), EXIT_UNUSABLE);
        assert_eq!(err, b"usage: od [-s SOCKET] COMMAND...   e.g. od l2 station show\n");
        err.clear();
        assert_eq!(run(&os(&["od", "-s", "/x"]), None, &mut out, &mut err), EXIT_UNUSABLE);
        assert!(err.starts_with(b"usage: "));
        err.clear();
        let long = format!("/{}", "s".repeat(107));
        assert_eq!(
            run(&os(&["od", "-s", &long, "ps"]), None, &mut out, &mut err),
            EXIT_UNUSABLE
        );
        assert_eq!(err, b"");
        let missing = OsStr::new("/nonexistent/diag.sock");
        assert_eq!(
            run(&os(&["od", "ps"]), Some(missing), &mut out, &mut err),
            EXIT_UNUSABLE
        );
        assert_eq!(
            err,
            b"openbcm-diag: /nonexistent/diag.sock: no syncd diag socket (SAI_OPENBCM_DIAG_SOCKET set? syncd running?)\n"
        );
        assert_eq!(out, b"");
    }
}

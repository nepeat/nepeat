//! MDIO reads through syncd's diag socket, while syncd owns the chip.
//!
//! The protocol (sai-openbcm `sai_diag.cpp`): one command line per connection, the answer is
//! the SDK diag shell's output, then EOF. The command is `phy raw c45 <phy> <dev> <reg>`, which
//! the shell answers with `\t0x<reg>: 0x<value>`; the server prefixes its own refusals and
//! failures with `openbcm-diag: `. Root only, as the socket is. Reads only: this type has no
//! way to send anything else.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use mdio_script::{BusError, Dev, MdioBus};

use crate::text::strerror;

/// What C reads of an answer at most (`buf[1024]`).
const ANSWER_MAX: u64 = 1023;
/// `sizeof(sun_path)`.
const SUN_PATH: usize = 108;

/// A read-only MDIO bus through syncd's diag socket. Writes are refused without touching the
/// socket: verification sections have none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagBus {
    socket: PathBuf,
    last_error: Option<Vec<u8>>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn first_line(text: &[u8]) -> &[u8] {
    text.split(|&b| b == b'\n').next().unwrap_or(text)
}

/// The value in the shell's answer to `cmd`.
fn parse_answer(cmd: &str, answer: &[u8]) -> Result<u16, Vec<u8>> {
    // A C string: it ends at a NUL byte.
    let answer = answer.split(|&b| b == 0).next().unwrap_or(answer);
    if let Some(at) = find(answer, b"openbcm-diag: ") {
        return Err(first_line(answer.get(at..).unwrap_or_default()).to_vec());
    }
    // The last ": 0x" starts the value.
    let Some(at) = answer.windows(4).rposition(|w| w == b": 0x") else {
        let line = first_line(answer);
        let mut e = format!("no value in the diag shell's answer to '{}': '", cmd.trim_end()).into_bytes();
        e.extend_from_slice(line.get(..80).unwrap_or(line));
        e.push(b'\'');
        return Err(e);
    };
    let value = answer.get(at.saturating_add(2)..).unwrap_or_default();
    let digits = value.get(2..).unwrap_or_default();
    let count = digits.iter().take_while(|b| b.is_ascii_hexdigit()).count();
    let parsed = digits
        .get(..count)
        .and_then(|d| std::str::from_utf8(d).ok())
        .and_then(|d| u16::from_str_radix(d, 16).ok());
    parsed.ok_or_else(|| {
        let mut e = b"unparsable value in the diag shell's answer: '".to_vec();
        e.extend_from_slice(value.get(..40).unwrap_or(value));
        e.push(b'\'');
        e
    })
}

impl DiagBus {
    /// A bus through the socket at `socket`. Nothing is opened until a read.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            last_error: None,
        }
    }

    /// The socket's path.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The message of the most recent failed read.
    pub fn last_error(&self) -> Option<&[u8]> {
        self.last_error.as_deref()
    }

    /// One clause-45 read; the error is one line.
    pub fn c45_read(&self, phy: u16, dev: Dev, reg: u16) -> Result<u16, Vec<u8>> {
        let shown = self.socket.as_os_str().as_bytes();
        let fail = |what: &str| {
            let mut e = shown.to_vec();
            e.extend_from_slice(what.as_bytes());
            e
        };
        if shown.len() >= SUN_PATH {
            return Err(fail(": path too long"));
        }
        let mut stream = UnixStream::connect(&self.socket).map_err(|e| {
            fail(&format!(
                ": {} (is syncd running with SAI_OPENBCM_DIAG_SOCKET?)",
                strerror(&e)
            ))
        })?;
        let cmd = format!("phy raw c45 0x{phy:x} 0x{:x} 0x{reg:x}\n", dev.get());
        stream
            .write_all(cmd.as_bytes())
            .map_err(|e| fail(&format!(": write: {}", strerror(&e))))?;
        // The server reads up to the line feed; C ignores a failed shutdown too.
        let _ = stream.shutdown(Shutdown::Write);
        let mut answer = Vec::new();
        // A read error ends the answer, as in C.
        let _ = stream.take(ANSWER_MAX).read_to_end(&mut answer);
        parse_answer(&cmd, &answer)
    }
}

impl MdioBus for DiagBus {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.c45_read(phy, dev, reg).map_err(|e| {
            self.last_error = Some(e);
            BusError
        })
    }

    fn write(&mut self, _phy: u16, _dev: Dev, _reg: u16, _val: u16) -> Result<(), BusError> {
        Err(BusError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "phy raw c45 0x24 0x1 0x8b00\n";

    fn err(answer: &[u8]) -> String {
        String::from_utf8(parse_answer(CMD, answer).unwrap_err()).unwrap()
    }

    #[test]
    fn answers() {
        assert_eq!(parse_answer(CMD, b"\t0x8b00: 0x2391\n"), Ok(0x2391));
        assert_eq!(parse_answer(CMD, b"\t0x8b00: 0x0\n"), Ok(0));
        assert_eq!(parse_answer(CMD, b"noise: 0x1\n\t0x8b00: 0xFFFF trailing"), Ok(0xffff));
        assert_eq!(parse_answer(CMD, b"\t0x8b00: 0x12\0: 0x34\n"), Ok(0x12));
        assert_eq!(
            err(b"openbcm-diag: command failed (-1)\n"),
            "openbcm-diag: command failed (-1)"
        );
        assert_eq!(
            err(b"\t0x8b00: 0x2391\nopenbcm-diag: root only\nmore"),
            "openbcm-diag: root only"
        );
        assert_eq!(
            err(b"Unknown command\nsecond line\n"),
            "no value in the diag shell's answer to 'phy raw c45 0x24 0x1 0x8b00': 'Unknown command'"
        );
        assert_eq!(
            err(b""),
            "no value in the diag shell's answer to 'phy raw c45 0x24 0x1 0x8b00': ''"
        );
        assert_eq!(
            err(b"\t0x8b00: 0x10000\n"),
            "unparsable value in the diag shell's answer: '0x10000\n'"
        );
        // stricter than C, which reads this as 0
        assert_eq!(
            err(b"\t0x8b00: 0xzz\n"),
            "unparsable value in the diag shell's answer: '0xzz\n'"
        );
        let long = format!("{}\n", "x".repeat(200));
        assert!(err(long.as_bytes()).ends_with(&format!("'{}'", "x".repeat(80))));
    }

    #[test]
    fn writes_are_refused_and_a_missing_socket_is_named() {
        let mut bus = DiagBus::new("/nonexistent/diag.sock");
        let d1 = Dev::new(1).unwrap();
        assert_eq!(bus.write(0, d1, 0, 0), Err(BusError));
        assert_eq!(bus.last_error(), None);
        assert_eq!(bus.read(0, d1, 0x8b00), Err(BusError));
        assert_eq!(
            String::from_utf8_lossy(bus.last_error().unwrap()),
            "/nonexistent/diag.sock: No such file or directory (is syncd running with SAI_OPENBCM_DIAG_SOCKET?)"
        );
        let long = format!("/tmp/{}", "s".repeat(120));
        let e = DiagBus::new(&long).c45_read(0, d1, 0).unwrap_err();
        assert_eq!(String::from_utf8(e).unwrap(), format!("{long}: path too long"));
    }
}

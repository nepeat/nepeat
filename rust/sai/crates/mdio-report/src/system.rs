//! What the tool needs from the system: who holds the SDK lock, systemd's status line.

use std::ffi::OsStr;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;

/// The process that holds an `flock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    /// Its process id.
    pub pid: i32,
    /// Its command name (`/proc/<pid>/comm`), empty when it cannot be read.
    pub comm: Vec<u8>,
}

/// glibc's `major()` and `minor()` of a `dev_t`.
const fn dev_numbers(dev: u64) -> (u64, u64) {
    (
        ((dev >> 32) & 0xffff_f000) | ((dev >> 8) & 0xfff),
        ((dev >> 12) & 0xffff_ff00) | (dev & 0xff),
    )
}

/// The pid of the holder of an `flock` on the file (device `major:minor`, inode `ino`) in the
/// text of `/proc/locks`.
fn holder_in(locks: &[u8], major: u64, minor: u64, ino: u64) -> Option<i32> {
    locks.split(|&b| b == b'\n').find_map(|line| {
        // "12: FLOCK  ADVISORY  WRITE 812 00:1a:1234 0 EOF"; waiters are "12: -> FLOCK ...".
        let line = std::str::from_utf8(line).ok()?;
        if line.contains("->") {
            return None;
        }
        let mut words = line.split_ascii_whitespace();
        let id = words.next()?.strip_suffix(':')?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let kind = words.next()?;
        let pid: i32 = words.nth(2)?.parse().ok()?;
        let mut file = words.next()?.splitn(3, ':');
        let maj = u64::from_str_radix(file.next()?, 16).ok()?;
        let min = u64::from_str_radix(file.next()?, 16).ok()?;
        let inode: u64 = file.next()?.parse().ok()?;
        (kind == "FLOCK" && inode == ino && maj == major && min == minor).then_some(pid)
    })
}

/// The process holding an `flock` on `lock_path` (from `/proc/locks`); `None` if nobody does, or
/// it cannot be told.
pub fn lock_holder(lock_path: &Path) -> Option<LockHolder> {
    let meta = std::fs::metadata(lock_path).ok()?;
    let locks = std::fs::read("/proc/locks").ok()?;
    let (major, minor) = dev_numbers(meta.dev());
    let pid = holder_in(&locks, major, minor, meta.ino())?;
    let comm = std::fs::read(format!("/proc/{pid}/comm")).unwrap_or_default();
    let comm = comm.split(|&b| b == b'\n').next().unwrap_or_default().to_vec();
    Some(LockHolder { pid, comm })
}

/// `STATUS=<text>` to systemd's notify socket (the value of `NOTIFY_SOCKET`: a path, or
/// `@name` for an abstract socket), when there is one. At most 899 bytes are sent.
pub fn sd_status(notify_socket: Option<&OsStr>, text: &[u8]) {
    let Some(path) = notify_socket.map(OsStrExt::as_bytes) else {
        return;
    };
    let mut msg = b"STATUS=".to_vec();
    msg.extend_from_slice(text);
    msg.truncate(899);
    let addr = match path.split_first() {
        Some((b'/', _)) => SocketAddr::from_pathname(OsStr::from_bytes(path)),
        Some((b'@', name)) => SocketAddr::from_abstract_name(name),
        _ => return,
    };
    if let (Ok(addr), Ok(socket)) = (addr, UnixDatagram::unbound()) {
        // Best effort, as in C.
        let _ = socket.send_to_addr(&msg, &addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_locks_lines() {
        let locks = b"1: POSIX  ADVISORY  WRITE 700 00:1a:1234 0 EOF\n\
                      2: FLOCK  ADVISORY  WRITE 811 00:1a:99 0 EOF\n\
                      3: -> FLOCK  ADVISORY  WRITE 900 00:1a:1234 0 EOF\n\
                      4: FLOCK  ADVISORY  WRITE 812 00:1a:1234 0 EOF\n\
                      5: FLOCK  ADVISORY  WRITE 813 00:1a:1234 0 EOF\n";
        assert_eq!(holder_in(locks, 0, 0x1a, 1234), Some(812));
        assert_eq!(holder_in(locks, 0, 0x1a, 99), Some(811));
        assert_eq!(holder_in(locks, 0, 0x1b, 1234), None);
        assert_eq!(holder_in(locks, 1, 0x1a, 1234), None);
        assert_eq!(holder_in(b"", 0, 0, 0), None);
        assert_eq!(holder_in(b"garbage\nx: FLOCK A W 1 0:0:0 0 EOF\n", 0, 0, 0), None);
        assert_eq!(
            holder_in(b"7: FLOCK  ADVISORY  WRITE 5 fd:01:42 0 EOF", 0xfd, 1, 42),
            Some(5)
        );
    }

    #[test]
    fn device_numbers() {
        assert_eq!(dev_numbers(0x1a), (0, 0x1a));
        assert_eq!(dev_numbers(0xfd01), (0xfd, 1));
        assert_eq!(dev_numbers(0x0012_3456_7890_abcd), (0x0012_30ab, 0x4567_89cd));
    }

    #[test]
    fn nobody_holds_a_missing_file() {
        assert_eq!(lock_holder(Path::new("/nonexistent/lock")), None);
    }

    #[test]
    fn status_goes_to_the_notify_socket() {
        // an abstract socket of this process: nothing in the file system
        let name = format!("mdio-report-test-{}", std::process::id());
        let addr = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let server = UnixDatagram::bind_addr(&addr).unwrap();
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut socket = std::ffi::OsString::from("@");
        socket.push(&name);
        sd_status(Some(&socket), b"ok: 32 PHYs verified");
        let mut buf = [0u8; 2048];
        let n = server.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"STATUS=ok: 32 PHYs verified");
        sd_status(Some(&socket), &[b'x'; 2000]);
        assert_eq!(server.recv(&mut buf).unwrap(), 899);
        // not a socket address, no socket: nothing happens
        sd_status(Some(OsStr::new("relative")), b"x");
        sd_status(Some(OsStr::new("")), b"x");
        sd_status(Some(OsStr::new("/nonexistent/notify")), b"x");
        sd_status(None, b"x");
    }
}

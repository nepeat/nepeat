//! Raw mode for the client's terminal, restored on drop.

use std::io;

pub struct RawMode {
    fd: libc::c_int,
    saved: libc::termios,
}

impl RawMode {
    /// None when `fd` is not a terminal (piped input: nothing to change).
    pub fn enable(fd: libc::c_int) -> io::Result<Option<RawMode>> {
        unsafe {
            if libc::isatty(fd) == 0 {
                return Ok(None);
            }
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut saved) < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = saved;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(RawMode { fd, saved }))
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved);
        }
    }
}

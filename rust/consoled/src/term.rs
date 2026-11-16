//! Raw mode and the alternate screen for the client's terminal, restored on drop, and by
//! `restore()` when the client exits early (`die`), so a failure never leaves the
//! terminal raw.

use std::io::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static SAVED: Mutex<Option<(libc::c_int, libc::termios)>> = Mutex::new(None);
static ALT: AtomicBool = AtomicBool::new(false);

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
            let mut g = SAVED.lock().unwrap_or_else(|e| e.into_inner());
            if g.is_none() {
                *g = Some((fd, saved));
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
        let mut g = SAVED.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some_and(|(fd, s)| fd == self.fd && s.c_lflag == self.saved.c_lflag) {
            *g = None;
        }
    }
}

/// The full-screen views (menu, chooser, scrollback) draw on the alternate screen; the
/// console's own output stays on the main one.
pub struct Screen;

impl Screen {
    pub fn enter() -> Screen {
        let mut o = io::stdout().lock();
        let _ = o.write_all(b"\x1b[?1049h\x1b[?25l");
        let _ = o.flush();
        ALT.store(true, Ordering::SeqCst);
        Screen
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let mut o = io::stdout().lock();
        let _ = o.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = o.flush();
        ALT.store(false, Ordering::SeqCst);
    }
}

/// Put the terminal back (leave the alternate screen, cooked mode); for exits that skip
/// the destructors.
pub fn restore() {
    if ALT.swap(false, Ordering::SeqCst) {
        let mut o = io::stdout().lock();
        let _ = o.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = o.flush();
    }
    if let Some((fd, saved)) = SAVED.lock().unwrap_or_else(|e| e.into_inner()).take() {
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &saved);
        }
    }
}

pub fn size() -> (usize, usize) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row as usize, ws.ws_col as usize)
    } else {
        (24, 80)
    }
}

//! The process's stderr through a pipe: every line is passed on to the original stderr at once
//! and kept, so a program can read what the library wrote there (both Teralynx libsais write the
//! loader's lines with `fprintf(stderr)` / `eprintln!`). A stalled reader would block the
//! library's writes; the reader only copies.
#![allow(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// A running tee.
#[derive(Debug)]
pub struct Tee {
    orig: OwnedFd,
    lines: Arc<Mutex<Vec<String>>>,
    reader: Option<JoinHandle<()>>,
}

fn dup2(src: BorrowedFd<'_>, dst: i32) -> io::Result<()> {
    // SAFETY: dup2 on two open descriptors: `src` is borrowed (open), `dst` is 2, which std's
    // stderr refers to by number and never closes; dup2 replaces it atomically.
    if unsafe { libc::dup2(src.as_raw_fd(), dst) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Starts the tee: fd 2 becomes a pipe's write end; a thread copies each line to the original
/// stderr and keeps it.
pub fn tee_stderr() -> io::Result<Tee> {
    let orig = io::stderr().as_fd().try_clone_to_owned()?;
    let (r, w) = io::pipe()?;
    let mut out = std::fs::File::from(orig.try_clone()?);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&lines);
    let reader = std::thread::Builder::new()
        .name("stderr-tee".to_owned())
        .spawn(move || {
            let mut r = BufReader::new(r);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let _ = out.write_all(&buf);
                        let text = String::from_utf8_lossy(&buf).trim_end_matches('\n').to_owned();
                        keep.lock().unwrap_or_else(PoisonError::into_inner).push(text);
                    }
                }
            }
        })?;
    dup2(w.as_fd(), 2)?;
    drop(w);
    Ok(Tee {
        orig,
        lines,
        reader: Some(reader),
    })
}

impl Tee {
    /// The lines so far.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Restores the original stderr and waits for the copy of what was written before.
    pub fn finish(mut self) -> io::Result<Vec<String>> {
        dup2(self.orig.as_fd(), 2)?;
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        Ok(self.lines())
    }
}

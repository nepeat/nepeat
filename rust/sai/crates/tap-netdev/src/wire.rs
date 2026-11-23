//! A "wire" between a TAP and a host interface: what the C++ fake's pump thread does
//! (`FakeBackend::pump`): frames the TAP sends go out of the interface, frames the interface
//! receives come up the TAP, while the gate (the port's admin and link state) is open. No
//! switching, no filtering: the CPU path of one port, as KNET carries it.
use crate::packet::PacketSocket;
use crate::sys;
use crate::tap::Tap;
use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

/// Largest frame carried (the C++ pump's buffer).
const FRAME_MAX: usize = 65536;

/// What [`forward`] did with one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forwarded {
    /// Read and written, this many bytes.
    Frame(usize),
    /// Read, and dropped: the gate was closed.
    Gated,
    /// Nothing read (end of file or a read error), or the write failed: the frame is lost,
    /// as on a full queue.
    Lost,
}

/// Moves one frame from `from` to `to` if `gate` is open. One read, at most one write.
pub fn forward(mut from: impl Read, mut to: impl Write, buf: &mut [u8], gate: &AtomicBool) -> Forwarded {
    let n = match from.read(buf) {
        Ok(n) if n > 0 => n,
        _ => return Forwarded::Lost,
    };
    if !gate.load(Ordering::Acquire) {
        return Forwarded::Gated;
    }
    match buf.get(..n).map(|frame| to.write(frame)) {
        Some(Ok(written)) if written == n => Forwarded::Frame(n),
        _ => Forwarded::Lost,
    }
}

/// The pump thread of one TAP. Dropping it stops the thread and waits for it; the TAP itself
/// lives on as long as other owners of the `Arc` keep it.
pub struct Wire {
    stop: Option<PipeWriter>,
    pump: Option<JoinHandle<io::Result<()>>>,
}

impl core::fmt::Debug for Wire {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Wire").field("running", &self.is_running()).finish()
    }
}

impl Wire {
    /// Starts carrying frames between `tap` and `socket` while `gate` is true.
    pub fn start(tap: Arc<Tap>, socket: PacketSocket, gate: Arc<AtomicBool>) -> io::Result<Self> {
        let (stop_read, stop_write) = io::pipe()?;
        let pump = std::thread::Builder::new()
            .name(format!("wire {}", tap.name()))
            .spawn(move || pump(&tap, &socket, &gate, &stop_read))?;
        Ok(Self {
            stop: Some(stop_write),
            pump: Some(pump),
        })
    }

    /// Whether the thread still runs (it ends on its own when a descriptor fails).
    pub fn is_running(&self) -> bool {
        self.pump.as_ref().is_some_and(|p| !p.is_finished())
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        // Closing the write end wakes the pump's poll (POLLHUP on the read end).
        drop(self.stop.take());
        if let Some(pump) = self.pump.take() {
            // The thread's own result is of no use to anyone at this point.
            let _ = pump.join();
        }
    }
}

fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

fn pump(tap: &Tap, socket: &PacketSocket, gate: &AtomicBool, stop: &PipeReader) -> io::Result<()> {
    let mut buf = vec![0_u8; FRAME_MAX];
    loop {
        let mut fds = [
            pollfd(tap.file().as_raw_fd()),
            pollfd(socket.file().as_raw_fd()),
            pollfd(stop.as_raw_fd()),
        ];
        match sys::poll(&mut fds, -1) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let [from_tap, from_wire, stopping] = fds.map(|f| f.revents);
        if stopping != 0 {
            return Ok(());
        }
        if from_tap & libc::POLLIN != 0 {
            forward(tap.file(), socket.file(), &mut buf, gate);
        }
        if from_wire & libc::POLLIN != 0 {
            forward(socket.file(), tap.file(), &mut buf, gate);
        }
        if (from_tap | from_wire) & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("wire: a descriptor failed"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    struct Failing;
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("gone"))
        }
    }
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_frame_passes_an_open_gate_only() {
        let open = AtomicBool::new(true);
        let closed = AtomicBool::new(false);
        let mut buf = [0; 64];
        let mut out = Vec::new();
        assert_eq!(forward(&b"frame"[..], &mut out, &mut buf, &open), Forwarded::Frame(5));
        assert_eq!(out, b"frame");
        let mut out = Vec::new();
        assert_eq!(forward(&b"frame"[..], &mut out, &mut buf, &closed), Forwarded::Gated);
        assert_eq!(out, b"");
        assert_eq!(forward(&b""[..], Vec::new(), &mut buf, &open), Forwarded::Lost);
        assert_eq!(forward(Failing, Vec::new(), &mut buf, &open), Forwarded::Lost);
        assert_eq!(forward(&b"x"[..], Failing, &mut buf, &open), Forwarded::Lost);
    }

    proptest! {
        #[test]
        fn forwarding_copies_one_read_exactly(frame in proptest::collection::vec(any::<u8>(), 1..300), cap in 1_usize..300, up: bool) {
            let gate = AtomicBool::new(up);
            let mut buf = vec![0; cap];
            let mut out = Vec::new();
            let got = forward(frame.as_slice(), &mut out, &mut buf, &gate);
            let n = frame.len().min(cap);
            if up {
                prop_assert_eq!(got, Forwarded::Frame(n));
                prop_assert_eq!(out.as_slice(), &frame[..n]);
            } else {
                prop_assert_eq!(got, Forwarded::Gated);
                prop_assert!(out.is_empty());
            }
        }
    }
}

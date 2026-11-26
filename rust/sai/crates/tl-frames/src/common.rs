//! Pieces the five tools share: the clock, console output, interface setup, the receive socket.

use crate::os::{self, ETH_P_ALL, Sock};
use std::io::Write;
use std::time::{Duration, Instant};

/// Seconds since the tool started (the C tools' `now()` is `CLOCK_MONOTONIC`; only differences are used).
#[derive(Debug, Clone, Copy)]
pub struct Clock(Instant);

impl Default for Clock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock {
    /// Monotonic seconds.
    pub fn now(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
}

/// `usleep`.
pub fn usleep(us: u64) {
    std::thread::sleep(Duration::from_micros(us));
}

/// Waits (sleeping `step_us` at a time) until `due` seconds on the clock.
pub fn wait_until(clock: &Clock, due: f64, step_us: u64) {
    while clock.now() < due {
        usleep(step_us);
    }
}

/// Writes text to standard output and flushes it; a closed pipe is ignored (as `printf` ignores it).
pub fn say(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// Writes a line to standard error.
pub fn complain(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// `up_and_index` with the C tools' failure: `perror` text on standard error, exit status 2.
pub fn up(name: &str) -> Result<(u32, [u8; 6]), i32> {
    os::up_and_index(name).map_err(|e| {
        complain(&e.to_string());
        2
    })
}

/// A packet socket on one interface for every protocol (`bind rx` failing is exit status 2).
pub fn rx_socket(ifindex: u32) -> Result<Sock, i32> {
    let bound = Sock::packet(ETH_P_ALL).and_then(|s| s.bind_packet(ifindex, ETH_P_ALL).map(|()| s));
    bound.map_err(|e| {
        complain(&format!("bind rx: {}", crate::cstd::strerror(&e)));
        2
    })
}

/// The send-only packet socket (`socket(AF_PACKET, SOCK_RAW, 0)`); the C tools do not check it, a
/// failure here shows as every send failing, so it is reported when the first send is attempted.
pub fn tx_socket() -> Option<Sock> {
    Sock::packet(0).ok()
}

/// Sends one frame; `true` when the whole frame went (`sendto(..) == len`).
pub fn send_frame(tx: Option<&Sock>, ifindex: u32, dmac: [u8; 6], frame: &[u8]) -> bool {
    tx.is_some_and(|s| s.send_to_packet(ifindex, dmac, frame).is_ok_and(|n| n == frame.len()))
}

/// `SO_RCVTIMEO` of 100 ms, as the receive loops use.
pub fn short_timeout(sock: &Sock) {
    let _ = sock.set_recv_timeout(Duration::from_millis(100));
}

/// The destination MAC a frame is addressed to (its first six bytes).
pub fn dest_of(frame: &[u8]) -> [u8; 6] {
    crate::bytes::array(frame, 0)
}

//! The sd_notify protocol, implemented directly: one datagram per message
//! to the unix socket named by $NOTIFY_SOCKET (a path, or "@name" for the
//! abstract namespace). Without the variable every call is a no-op.

use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::time::Duration;

pub struct Notifier {
    target: Option<(UnixDatagram, SocketAddr)>,
    watchdog: Option<Duration>,
}

impl Notifier {
    pub fn from_env() -> Notifier {
        let target = std::env::var("NOTIFY_SOCKET").ok().and_then(|s| Self::connect(&s));
        // WATCHDOG_PID, when set, must name us (otherwise the watchdog is
        // meant for another process, e.g. a wrapper).
        let for_us = std::env::var("WATCHDOG_PID")
            .ok()
            .is_none_or(|p| p.trim().parse::<u32>().ok() == Some(std::process::id()));
        let watchdog = std::env::var("WATCHDOG_USEC")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|us| *us > 0 && for_us)
            .map(Duration::from_micros);
        Notifier { target, watchdog }
    }

    fn connect(s: &str) -> Option<(UnixDatagram, SocketAddr)> {
        let addr = if let Some(name) = s.strip_prefix('@') {
            abstract_addr(name.as_bytes())?
        } else if s.starts_with('/') {
            SocketAddr::from_pathname(s).ok()?
        } else {
            return None;
        };
        let sock = UnixDatagram::unbound().ok()?;
        // Never block the control loop on notification: if the receiver's
        // queue is full the message is dropped (a missed WATCHDOG=1 is what
        // the watchdog is for). A blocking send here once wedged the loop.
        sock.set_nonblocking(true).ok()?;
        Some((sock, addr))
    }

    pub fn enabled(&self) -> bool {
        self.target.is_some()
    }

    /// Watchdog timeout requested by systemd; ping at least twice per period.
    pub fn watchdog(&self) -> Option<Duration> {
        self.watchdog
    }

    pub fn send(&self, msg: &str) {
        if let Some((sock, addr)) = &self.target {
            if let Err(e) = sock.send_to_addr(msg.as_bytes(), addr) {
                crate::log_debug!("sd_notify failed: {e}");
            }
        }
    }

    pub fn ready(&self, status: &str) {
        self.send(&format!("READY=1\nSTATUS={status}"));
    }

    pub fn ping(&self, status: &str) {
        if self.watchdog.is_some() {
            self.send(&format!("WATCHDOG=1\nSTATUS={status}"));
        } else {
            self.send(&format!("STATUS={status}"));
        }
    }

    pub fn stopping(&self) {
        self.send("STOPPING=1\nSTATUS=stopping, fans to exit duty");
    }
}

#[cfg(target_os = "linux")]
fn abstract_addr(name: &[u8]) -> Option<SocketAddr> {
    use std::os::linux::net::SocketAddrExt;
    SocketAddr::from_abstract_name(name).ok()
}

#[cfg(not(target_os = "linux"))]
fn abstract_addr(_name: &[u8]) -> Option<SocketAddr> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_datagrams_to_a_path_socket() {
        let fs = crate::fakesys::FakeSys::new("notify");
        let path = fs.root.join("notify.sock");
        let server = UnixDatagram::bind(&path).unwrap();
        server.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let n = Notifier {
            target: Notifier::connect(path.to_str().unwrap()),
            watchdog: Some(Duration::from_secs(10)),
        };
        assert!(n.enabled());
        n.ready("ok");
        n.ping("tick");
        let mut buf = [0u8; 256];
        let len = server.recv(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"READY=1\nSTATUS=ok");
        let len = server.recv(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"WATCHDOG=1\nSTATUS=tick");
    }

    #[test]
    fn a_full_receiver_queue_never_blocks() {
        let fs = crate::fakesys::FakeSys::new("notify-full");
        let path = fs.root.join("full.sock");
        let _server = UnixDatagram::bind(&path).unwrap(); // never read
        let n = Notifier {
            target: Notifier::connect(path.to_str().unwrap()),
            watchdog: Some(Duration::from_secs(1)),
        };
        let t = std::time::Instant::now();
        for _ in 0..5000 {
            n.ping("x");
        }
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn abstract_and_invalid_addresses() {
        let name = format!("fand-test-{}", std::process::id());
        let addr = abstract_addr(name.as_bytes()).unwrap();
        let server = UnixDatagram::bind_addr(&addr).unwrap();
        server.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let n = Notifier {
            target: Notifier::connect(&format!("@{name}")),
            watchdog: None,
        };
        n.send("STATUS=x");
        let mut buf = [0u8; 64];
        let len = server.recv(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"STATUS=x");
        assert!(Notifier::connect("relative/path").is_none());
        let off = Notifier {
            target: None,
            watchdog: None,
        };
        off.ready("nothing happens");
        assert!(!off.enabled());
    }
}

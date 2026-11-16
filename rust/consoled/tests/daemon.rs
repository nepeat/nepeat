//! The real daemon against pseudo-terminals standing in for serial ports.

use consoled::proto::{self, Cmd, Hello, PortStatus};
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Pty {
    master: std::fs::File,
    path: PathBuf,
}

fn pty() -> Pty {
    let (mut m, mut s) = (0, 0);
    let mut name = [0 as libc::c_char; 128];
    let r = unsafe { libc::openpty(&mut m, &mut s, name.as_mut_ptr(), std::ptr::null(), std::ptr::null()) };
    assert_eq!(r, 0, "openpty");
    let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    unsafe {
        // raw on the master side too, so bytes arrive as written
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(s, libc::TCSANOW, &t);
        libc::close(s); // the daemon opens the slave by path (exclusively)
    }
    let master = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(m) });
    Pty { master, path: path.into() }
}

struct Daemon {
    child: Child,
    socket: PathBuf,
    _dir: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}

fn start(ports: &[(&str, &str, &Path)], scrollback: usize) -> Daemon {
    let dir = std::env::temp_dir().join(format!("consoled-test-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("console.sock");
    let ports: Vec<_> = ports
        .iter()
        .map(|(name, label, dev)| serde_json::json!({"name": name, "label": label, "device": dev, "baud": 115200}))
        .collect();
    let cfg = serde_json::json!({"socket": socket, "scrollback": scrollback, "ports": ports});
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, cfg.to_string()).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_consoled")).arg("--config").arg(&cfg_path).spawn().unwrap();
    let t0 = Instant::now();
    while UnixStream::connect(&socket).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(5), "daemon socket");
        std::thread::sleep(Duration::from_millis(20));
    }
    Daemon { child, socket, _dir: dir }
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64 % 1_000_000_007
}

struct Client {
    s: UnixStream,
    dec: proto::Decoder,
}

impl Client {
    fn hello(d: &Daemon, port: Option<&str>, write: bool, who: &str) -> Client {
        let mut s = UnixStream::connect(&d.socket).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut out = Vec::new();
        let h = Hello { port: port.map(String::from), write, who: who.into(), replay: true, dump: false };
        proto::encode_json(proto::HELLO, &h, &mut out);
        s.write_all(&out).unwrap();
        Client { s, dec: proto::Decoder::default() }
    }
    fn send(&mut self, kind: u8, payload: &[u8]) {
        let mut out = Vec::new();
        proto::encode(kind, payload, &mut out);
        self.s.write_all(&out).unwrap();
    }
    fn cmd(&mut self, c: Cmd) {
        let mut out = Vec::new();
        proto::encode_json(proto::CMD, &c, &mut out);
        self.s.write_all(&out).unwrap();
    }
    /// Frames until `pred` matches one (returned) or 3 s pass (panic).
    fn until(&mut self, what: &str, pred: impl Fn(u8, &[u8]) -> bool) -> (u8, Vec<u8>) {
        let t0 = Instant::now();
        let mut buf = [0u8; 65536];
        let mut seen = Vec::new();
        loop {
            while let Some((k, p)) = self.dec.next().unwrap() {
                if pred(k, &p) {
                    return (k, p);
                }
                seen.push(format!("{k}:{}", String::from_utf8_lossy(&p)));
            }
            assert!(t0.elapsed() < Duration::from_secs(3), "waiting for {what}; saw {seen:?}");
            match self.s.read(&mut buf) {
                Ok(0) => panic!("closed while waiting for {what}; saw {seen:?}"),
                Ok(n) => self.dec.push(&buf[..n]),
                Err(_) => {}
            }
        }
    }
    fn info(&mut self, needle: &str) {
        self.until(needle, |k, p| k == proto::INFO && String::from_utf8_lossy(p).contains(needle));
    }
    /// DATA bytes, accumulated until they contain `needle`.
    fn data(&mut self, needle: &[u8]) {
        let acc = std::cell::RefCell::new(Vec::new());
        self.until(&String::from_utf8_lossy(needle), |k, p| {
            if k == proto::DATA {
                acc.borrow_mut().extend_from_slice(p);
            }
            acc.borrow().windows(needle.len()).any(|w| w == needle)
        });
    }
}

fn read_master(m: &mut std::fs::File, needle: &[u8], within: Duration) -> bool {
    let fd = std::os::fd::AsRawFd::as_raw_fd(m);
    let t0 = Instant::now();
    let mut acc = Vec::new();
    let mut buf = [0u8; 4096];
    while t0.elapsed() < within {
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut p, 1, 50) } > 0 {
            if let Ok(n) = m.read(&mut buf) {
                acc.extend_from_slice(&buf[..n]);
            }
        }
        if acc.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
    }
    false
}

#[test]
fn scrollback_writer_rules_takeover_and_list() {
    let mut a = pty();
    let b = pty();
    let d = start(&[("A", "alpha", &a.path), ("B", "", &b.path), ("C", "gone", Path::new("/nonexistent/tty"))], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"boot log line\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // first client: banner, scrollback, write
    let mut c1 = Client::hello(&d, Some("A"), true, "alice@test");
    c1.info("A (alpha), 115200 8N1");
    c1.data(b"boot log line");
    c1.info("you have write");
    c1.send(proto::DATA, b"ls -l\n");
    assert!(read_master(&mut a.master, b"ls -l\n", Duration::from_secs(2)), "writer's input reaches the port");

    // second client by label: read-only, its input is dropped
    let mut c2 = Client::hello(&d, Some("ALPHA"), true, "bob@test");
    c2.info("read-only: alice@test has write");
    c1.info("bob@test attached (read-only)");
    c2.send(proto::DATA, b"rm -rf /\n");
    c2.info("read-only: ^] w takes write");
    assert!(!read_master(&mut a.master, b"rm -rf", Duration::from_millis(500)), "watcher input must not reach the port");

    // port output reaches both
    a.master.write_all(b"total 0\r\n").unwrap();
    c1.data(b"total 0");
    c2.data(b"total 0");

    // takeover
    c2.cmd(Cmd::Take);
    c2.info("you have write");
    c1.info("bob@test took write; you are read-only");
    c2.send(proto::DATA, b"uptime\n");
    assert!(read_master(&mut a.master, b"uptime\n", Duration::from_secs(2)));

    // list
    let mut l = Client::hello(&d, None, false, "list");
    let (_, body) = l.until("LIST", |k, _| k == proto::LIST);
    let ports: Vec<PortStatus> = serde_json::from_slice(&body).unwrap();
    assert_eq!(ports.len(), 3);
    assert!(ports[0].up && ports[0].clients == 2 && ports[0].writer.as_deref() == Some("bob@test"));
    assert!(!ports[2].up, "missing device is listed as absent");

    // the writer leaves: the other is told write is free
    drop(c2);
    c1.info("bob@test left; write is free");

    // missing device: attach works and says so
    let mut c3 = Client::hello(&d, Some("C"), true, "carol@test");
    c3.info("not present, retrying");

    // unknown port
    let mut c4 = Client::hello(&d, Some("nope"), true, "dave@test");
    c4.until("ERROR", |k, p| k == proto::ERROR && String::from_utf8_lossy(p).contains("no port"));
}

#[test]
fn scrollback_is_capped() {
    let mut a = pty();
    let d = start(&[("A", "", &a.path)], 64);
    std::thread::sleep(Duration::from_millis(200));
    let mut line = Vec::new();
    for i in 0..40 {
        line.extend_from_slice(format!("{i:02}-").as_bytes());
    }
    a.master.write_all(&line).unwrap(); // 120 bytes, cap 64
    std::thread::sleep(Duration::from_millis(300));
    let mut c = Client::hello(&d, Some("A"), false, "w");
    c.data(b"39-");
    // the oldest bytes are gone; the last 64 (from "18-" on, partly) remain
    let mut s = UnixStream::connect(&d.socket).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, &Hello { port: Some("A".into()), write: false, who: "x".into(), replay: true, dump: false }, &mut out);
    s.write_all(&out).unwrap();
    let mut dec = proto::Decoder::default();
    let mut buf = [0u8; 4096];
    let mut replay = Vec::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(1) {
        if let Ok(n) = s.read(&mut buf) {
            dec.push(&buf[..n]);
        }
        while let Some((k, p)) = dec.next().unwrap() {
            if k == proto::DATA {
                replay.extend_from_slice(&p);
            }
        }
    }
    assert_eq!(replay.len(), 64);
    assert_eq!(&replay[..], &line[line.len() - 64..]);
}

#[test]
fn dump_and_status_details() {
    let mut a = pty();
    let d = start(&[("A", "alpha", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"kernel: hello\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // dump: the scrollback, then the daemon closes (no banner, no attach)
    let mut s = UnixStream::connect(&d.socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, &Hello { port: Some("alpha".into()), write: false, who: "d".into(), replay: true, dump: true }, &mut out);
    s.write_all(&out).unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let mut dec = proto::Decoder::default();
    dec.push(&all);
    let mut data = Vec::new();
    while let Some((k, p)) = dec.next().unwrap() {
        assert_eq!(k, proto::DATA, "a dump carries only data");
        data.extend_from_slice(&p);
    }
    assert_eq!(data, b"kernel: hello\r\n");

    // status: users writer first, scrollback size, idle time
    let _w1 = Client::hello(&d, Some("A"), false, "watcher@test");
    let mut w2 = Client::hello(&d, Some("A"), true, "writer@test");
    w2.info("you have write");
    let mut l = Client::hello(&d, None, false, "list");
    let (_, body) = l.until("LIST", |k, _| k == proto::LIST);
    let ports: Vec<PortStatus> = serde_json::from_slice(&body).unwrap();
    assert_eq!(ports[0].users, vec!["writer@test".to_string(), "watcher@test".to_string()]);
    assert_eq!(ports[0].scrollback_bytes, 15);
    assert!(ports[0].idle_secs.is_some_and(|s| s < 5));
}

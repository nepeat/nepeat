//! The real daemon against pseudo-terminals standing in for serial ports.

use consoled::proto::{self, Cmd, Hello, PortState, PortStatus};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
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
    let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        // raw on the master side too, so bytes arrive as written
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(s, libc::TCSANOW, &t);
        libc::close(s); // the daemon opens the slave by path (exclusively)
    }
    let master = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(m) });
    Pty {
        master,
        path: path.into(),
    }
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
    let child = Command::new(env!("CARGO_BIN_EXE_consoled"))
        .arg("--config")
        .arg(&cfg_path)
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while UnixStream::connect(&socket).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(5), "daemon socket");
        std::thread::sleep(Duration::from_millis(20));
    }
    Daemon {
        child,
        socket,
        _dir: dir,
    }
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
        % 1_000_000_007
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
        let h = Hello {
            port: port.map(String::from),
            write,
            who: who.into(),
            replay: true,
            dump: false,
            ..Hello::default()
        };
        proto::encode_json(proto::HELLO, &h, &mut out);
        s.write_all(&out).unwrap();
        Client {
            s,
            dec: proto::Decoder::default(),
        }
    }
    fn with(d: &Daemon, h: &Hello) -> Client {
        let mut s = UnixStream::connect(&d.socket).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut out = Vec::new();
        proto::encode_json(proto::HELLO, h, &mut out);
        s.write_all(&out).unwrap();
        Client {
            s,
            dec: proto::Decoder::default(),
        }
    }
    fn state(&mut self, what: &str, pred: impl Fn(&PortState) -> bool) -> PortState {
        let (_, b) = self.until(what, |k, p| {
            k == proto::STATE && serde_json::from_slice::<PortState>(p).is_ok_and(|s| pred(&s))
        });
        serde_json::from_slice(&b).unwrap()
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
            assert!(
                t0.elapsed() < Duration::from_secs(3),
                "waiting for {what}; saw {seen:?}"
            );
            match self.s.read(&mut buf) {
                Ok(0) => panic!("closed while waiting for {what}; saw {seen:?}"),
                Ok(n) => self.dec.push(&buf[..n]),
                Err(_) => {}
            }
        }
    }
    fn info(&mut self, needle: &str) {
        self.until(needle, |k, p| {
            k == proto::INFO && String::from_utf8_lossy(p).contains(needle)
        });
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
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
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
    let d = start(
        &[
            ("A", "alpha", &a.path),
            ("B", "", &b.path),
            ("C", "gone", Path::new("/nonexistent/tty")),
        ],
        4096,
    );
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"boot log line\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // first client: banner, scrollback, write
    let mut c1 = Client::hello(&d, Some("A"), true, "alice@test");
    c1.info("A (alpha), 115200 8N1");
    c1.data(b"boot log line");
    c1.info("you have write");
    c1.send(proto::DATA, b"ls -l\n");
    assert!(
        read_master(&mut a.master, b"ls -l\n", Duration::from_secs(2)),
        "writer's input reaches the port"
    );

    // second client by label: read-only, its input is dropped
    let mut c2 = Client::hello(&d, Some("ALPHA"), true, "bob@test");
    c2.info("read-only: alice@test has write");
    c1.info("bob@test attached (read-only)");
    c2.send(proto::DATA, b"rm -rf /\n");
    c2.info("read-only: ^] w takes write");
    assert!(
        !read_master(&mut a.master, b"rm -rf", Duration::from_millis(500)),
        "watcher input must not reach the port"
    );

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
    c4.until("ERROR", |k, p| {
        k == proto::ERROR && String::from_utf8_lossy(p).contains("no port")
    });
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
    proto::encode_json(
        proto::HELLO,
        &Hello {
            port: Some("A".into()),
            write: false,
            who: "x".into(),
            replay: true,
            dump: false,
            ..Hello::default()
        },
        &mut out,
    );
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
    proto::encode_json(
        proto::HELLO,
        &Hello {
            port: Some("alpha".into()),
            write: false,
            who: "d".into(),
            replay: true,
            dump: true,
            ..Hello::default()
        },
        &mut out,
    );
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
    assert_eq!(
        ports[0].users,
        vec!["writer@test".to_string(), "watcher@test".to_string()]
    );
    assert_eq!(ports[0].scrollback_bytes, 15);
    assert!(ports[0].idle_secs.is_some_and(|s| s < 5));
}

fn list(d: &Daemon) -> Vec<PortStatus> {
    let mut l = Client::hello(d, None, false, "list");
    let (_, body) = l.until("LIST", |k, _| k == proto::LIST);
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn baud_change_runs_on_the_worker_and_is_announced() {
    let a = pty();
    let d = start(&[("A", "alpha", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    let mut w = Client::hello(&d, Some("A"), true, "alice@test");
    w.info("you have write");
    let mut v = Client::hello(&d, Some("A"), false, "bob@test");
    v.info("read-only (watching)");

    v.cmd(Cmd::Baud(9600));
    v.info("only the writer can change the baud rate");
    w.cmd(Cmd::Baud(12345));
    w.info("unsupported baud rate 12345; one of 1200");
    w.cmd(Cmd::Baud(115200));
    w.info("the line is already 115200 8N1");

    w.cmd(Cmd::Baud(9600));
    v.info("alice@test set the line to 9600 8N1 (was 115200 baud)");
    w.info("alice@test set the line to 9600 8N1");
    let st = v.state("STATE at 9600", |s| s.settings == "9600 8N1");
    assert!(!st.write && st.writer.as_deref() == Some("alice@test"));
    // the tty itself (a pty master reports its slave's settings)
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(a.master.as_raw_fd(), &mut t) }, 0);
    assert_eq!(unsafe { libc::cfgetospeed(&t) }, libc::B9600);
    assert_eq!(list(&d)[0].settings, "9600 8N1");
}

#[test]
fn scrollback_command_take_at_hello_and_state() {
    let mut a = pty();
    let d = start(&[("A", "", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"old line\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let mut c = Client::with(
        &d,
        &Hello {
            port: Some("A".into()),
            write: true,
            who: "alice@test".into(),
            prefix: "C-b".into(),
            ..Hello::default()
        },
    );
    c.info("Keys: C-b d detach, C-b ? help");
    let st = c.state("STATE with write", |s| s.write);
    assert_eq!(
        (st.port.as_str(), st.settings.as_str(), st.up),
        ("A", "115200 8N1", true)
    );

    // the scrollback on request, in SCROLLBACK frames ending with an empty one
    c.cmd(Cmd::Scrollback);
    let acc = std::cell::RefCell::new(Vec::new());
    c.until("end of SCROLLBACK", |k, p| {
        if k == proto::SCROLLBACK {
            acc.borrow_mut().extend_from_slice(p);
        }
        k == proto::SCROLLBACK && p.is_empty()
    });
    assert_eq!(acc.into_inner(), b"old line\r\n");
    a.master.write_all(b"live\r\n").unwrap();
    c.data(b"live");

    // a watcher is told to use C-b t; take at hello takes write over at once
    let mut v = Client::with(
        &d,
        &Hello {
            port: Some("A".into()),
            who: "bob@test".into(),
            prefix: "C-a".into(),
            ..Hello::default()
        },
    );
    v.info("read-only (watching); C-a t takes write");
    let mut t = Client::with(
        &d,
        &Hello {
            port: Some("A".into()),
            write: true,
            take: true,
            who: "carol@test".into(),
            ..Hello::default()
        },
    );
    t.info("you have write (taken over from alice@test)");
    c.info("carol@test took write; you are read-only");
    c.state("STATE without write", |s| {
        !s.write && s.writer.as_deref() == Some("carol@test")
    });
    drop(t);
    v.info("carol@test left; write is free (C-a t)");
    c.info("carol@test left; write is free (C-b t)");
    v.state("STATE: nobody writes", |s| s.writer.is_none());
}

// ---- the line client ------------------------------------------------------------------------

fn line(d: &Daemon, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_line"));
    c.args(args)
        .env("CONSOLED_SOCKET", &d.socket)
        .env("USER", "agent")
        .env_remove("SUDO_USER")
        .env_remove("SSH_CONNECTION")
        .env_remove("CONSOLED_PREFIX");
    c
}

fn text(o: &Output) -> (Option<i32>, String, String) {
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// A shell on the pty: answers `uname -a` (echo, bracketed-paste noise, output, prompt).
fn fake_shell(mut master: std::fs::File, prompt: &'static [u8]) -> std::thread::JoinHandle<std::fs::File> {
    std::thread::spawn(move || {
        assert!(
            read_master(&mut master, b"uname -a\r", Duration::from_secs(5)),
            "the command reaches the port"
        );
        master.write_all(b"uname -a\r\n\x1b[?2004l\r").unwrap();
        master
            .write_all(b"Linux box 6.1.0 #1 SMP \x1b[1maarch64\x1b[0m\r\n")
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        master.write_all(b"\x1b[?2004h\x1b]0;root@box: ~\x07").unwrap();
        master.write_all(prompt).unwrap();
        master
    })
}

#[test]
fn line_send_refuses_or_takes_and_gives_write_back() {
    let mut a = pty();
    let d = start(&[("A", "alpha", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    let mut alice = Client::hello(&d, Some("A"), true, "alice@test");
    alice.info("you have write");

    let (code, _, err) = text(&line(&d, &["send", "A", "hello"]).output().unwrap());
    assert_eq!(code, Some(2), "{err}");
    assert!(
        err.contains("A: refused: alice@test has write (--take takes it over)"),
        "{err}"
    );
    assert!(!read_master(&mut a.master, b"hello", Duration::from_millis(300)));

    let (code, _, err) = text(&line(&d, &["send", "alpha", "--take", "hello"]).output().unwrap());
    assert_eq!(code, Some(0), "{err}");
    alice.info("agent [send] took write; you are read-only");
    assert!(read_master(&mut a.master, b"hello\r", Duration::from_secs(2)));
    alice.info("agent [send] left; write is free (^] w)");
    assert!(
        list(&d)[0].writer.is_none(),
        "write acquired for send is released after"
    );

    // free: no --take needed; stdin; --no-newline sends the bytes as they are
    let mut c = line(&d, &["send", "A", "-n", "-"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(b"y\n").unwrap();
    assert_eq!(c.wait().unwrap().code(), Some(0));
    assert!(read_master(&mut a.master, b"y\n", Duration::from_secs(2)));
    let (code, _, err) = text(&line(&d, &["send", "nope", "x"]).output().unwrap());
    assert_eq!(code, Some(3), "{err}");
    assert!(err.contains("no port"), "{err}");
}

#[test]
fn line_expect_matches_new_output_or_times_out() {
    let mut a = pty();
    let d = start(&[("A", "", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"old login: \r\n").unwrap(); // scrollback: not new, must not match
    std::thread::sleep(Duration::from_millis(200));

    let c = line(&d, &["expect", "A", "login: $", "--timeout", "5"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    a.master.write_all(b"\x1b[0mbooting\r\nbox log").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    a.master.write_all(b"in: ").unwrap();
    let (code, out, _) = text(&c.wait_with_output().unwrap());
    assert_eq!(code, Some(0));
    assert_eq!(out, "booting\nbox login: \n");

    let t0 = Instant::now();
    let (code, _, err) = text(
        &line(&d, &["expect", "A", "never", "--timeout", "0.3"])
            .output()
            .unwrap(),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("nothing matching \"never\" within 0.3 s"), "{err}");
    assert!(t0.elapsed() < Duration::from_secs(3));
}

#[test]
fn line_run_prints_the_output_between_command_and_prompt() {
    let a = pty();
    let d = start(&[("A", "cs8210", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));

    let sh = fake_shell(a.master, b"root@box:~# ");
    let (code, out, err) = text(
        &line(&d, &["run", "cs8210", "--prompt", "# $", "uname -a"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        (code, out.as_str()),
        (Some(0), "Linux box 6.1.0 #1 SMP aarch64\n"),
        "{err}"
    );
    let master = sh.join().unwrap();
    assert!(list(&d)[0].writer.is_none(), "write acquired for run is released after");

    // over ssh: the words come from $SSH_ORIGINAL_COMMAND, quoted like a shell would; the
    // default prompt; U-Boot's "=> "
    let sh = fake_shell(master, b"=> ");
    let o = line(&d, &["ssh"])
        .env("SSH_ORIGINAL_COMMAND", "run A 'uname -a'")
        .output()
        .unwrap();
    let (code, out, err) = text(&o);
    assert_eq!(
        (code, out.as_str()),
        (Some(0), "Linux box 6.1.0 #1 SMP aarch64\n"),
        "{err}"
    );
    let master = sh.join().unwrap();

    // nobody answers: timeout, exit 1, prints what came
    let (code, _, err) = text(&line(&d, &["run", "A", "--timeout", "0.5", "true"]).output().unwrap());
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("no prompt matching"), "{err}");

    // someone else has write: refused, exit 2
    let mut alice = Client::hello(&d, Some("A"), true, "alice@test");
    alice.info("you have write");
    let (code, _, err) = text(&line(&d, &["run", "A", "uname -a"]).output().unwrap());
    assert_eq!(code, Some(2), "{err}");
    // over ssh the forbidden words stay forbidden, and nothing runs a shell
    for bad in ["metrics", "ssh", "--socket /tmp/x list", "attach 'A"] {
        let o = line(&d, &["ssh"]).env("SSH_ORIGINAL_COMMAND", bad).output().unwrap();
        assert_ne!(o.status.code(), Some(0), "{bad}");
    }
    drop(master);
}

#[test]
fn line_log_follow_streams() {
    let mut a = pty();
    let d = start(&[("A", "", &a.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"before\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let mut c = line(&d, &["log", "A", "--follow"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = c.stdout.take().unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 {
                break;
            }
            g.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    std::thread::sleep(Duration::from_millis(300));
    a.master.write_all(b"after\r\n").unwrap();
    let t0 = Instant::now();
    while !got.lock().unwrap().ends_with(b"before\r\nafter\r\n") {
        assert!(
            t0.elapsed() < Duration::from_secs(3),
            "{:?}",
            String::from_utf8_lossy(&got.lock().unwrap())
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = c.kill();
    let _ = c.wait();
}

// ---- line attach on a terminal ----------------------------------------------------------------

/// `line` with a pty as its terminal; we hold the master side.
struct Term {
    child: Child,
    m: std::fs::File,
    /// Output read but not yet looked for.
    buf: Vec<u8>,
}

impl Term {
    fn spawn(d: &Daemon, args: &[&str]) -> Term {
        let (mut m, mut s) = (0, 0);
        let ws = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) },
            0
        );
        let slave = unsafe { OwnedFd::from_raw_fd(s) };
        let child = line(d, args)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        Term {
            child,
            m: std::fs::File::from(unsafe { OwnedFd::from_raw_fd(m) }),
            buf: Vec::new(),
        }
    }
    fn keys(&mut self, b: &[u8]) {
        self.m.write_all(b).unwrap();
        std::thread::sleep(Duration::from_millis(80)); // one read per key group
    }
    /// Output until `needle` (in order: what follows it stays for the next call).
    fn sees(&mut self, needle: &str) {
        let n = needle.as_bytes();
        let t0 = Instant::now();
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(p) = self.buf.windows(n.len()).position(|w| w == n) {
                self.buf.drain(..p + n.len());
                return;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(3),
                "the terminal shows {needle:?}; it showed {:?}",
                String::from_utf8_lossy(&self.buf)
            );
            let mut p = libc::pollfd {
                fd: self.m.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut p, 1, 50) } > 0 {
                if let Ok(k) = self.m.read(&mut chunk) {
                    self.buf.extend_from_slice(&chunk[..k]);
                }
            }
        }
    }
    fn exits(&mut self) {
        let t0 = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(t0.elapsed() < Duration::from_secs(3), "line exits");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn users_on(d: &Daemon, port: usize) -> (Vec<String>, Option<String>) {
    let p = list(d).swap_remove(port);
    (p.users, p.writer)
}

#[test]
fn attach_keys_prefix_literal_help_baud_scrollback() {
    let mut a = pty();
    let b = pty();
    let d = start(&[("A", "alpha", &a.path), ("B", "beta", &b.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));
    a.master.write_all(b"ring text\r\n").unwrap();

    let mut t = Term::spawn(&d, &["attach", "A"]);
    t.sees("you have write");
    t.keys(b"ls\r");
    assert!(read_master(&mut a.master, b"ls\r", Duration::from_secs(2)));
    t.keys(b"\x02\x02"); // C-b C-b: a literal C-b
    t.keys(b"\x1d\x1d"); // ^] ^]: a literal ^]
    assert!(read_master(&mut a.master, b"\x02\x1d", Duration::from_secs(2)));
    t.keys(b"\x02?");
    t.sees("keys: press C-b (or ^]), then");
    t.keys(b"\x02z");
    t.sees("C-b z: no such key");

    // : baud, on the worker; the daemon tells everyone
    t.keys(b"\x02:baud 9600\r");
    t.sees("set the line to 9600 8N1");
    t.keys(b"\x02:bogus\r");
    t.sees("take, release, break, baud RATE");

    // [ scrollback view; output meanwhile is kept for later
    t.keys(b"\x02[");
    t.sees("ring text");
    t.sees("scrollback A (alpha)");
    a.master.write_all(b"during\r\n").unwrap();
    t.sees("new output waiting");
    t.keys(b"\x1b[5~g");
    t.keys(b"q");
    t.sees("during");

    // r releases, t takes (and ^] w, the old key)
    t.keys(b"\x02r");
    t.sees("released write");
    t.keys(b"\x1dw");
    t.sees("you have write");
    t.keys(b"\x02d");
    t.sees("detached");
    t.exits();
}

#[test]
fn attach_keys_next_chooser_and_watch_mode() {
    let a = pty();
    let b = pty();
    let d = start(&[("A", "alpha", &a.path), ("B", "beta", &b.path)], 4096);
    std::thread::sleep(Duration::from_millis(200));

    // n: the next port, still asking for write
    let mut t = Term::spawn(&d, &["attach", "A"]);
    t.sees("you have write");
    t.keys(b"\x02n");
    t.sees("A -> B");
    t.sees("you have write");
    assert_eq!(users_on(&d, 1), (vec!["agent".to_string()], Some("agent".to_string())));
    assert!(users_on(&d, 0).0.is_empty());
    // s: the chooser; q goes back to the same session (still on B, still writing)
    t.keys(b"\x02s");
    t.sees("q back");
    t.keys(b"q");
    t.keys(b"\x02?");
    t.sees("keys: press");
    assert_eq!(users_on(&d, 1).1.as_deref(), Some("agent"));
    // s, up to A, Enter: switches
    t.keys(b"\x02s");
    t.sees("q back");
    t.keys(b"k\r");
    t.sees("B -> A");
    t.sees("you have write");
    // ^] . still detaches
    t.keys(b"\x1d.");
    t.sees("detached");
    t.exits();

    // watch stays watch across p; --prefix C-a: C-a is the prefix, C-b goes to the port
    let mut w = Term::spawn(&d, &["--prefix", "C-a", "watch", "B"]);
    w.sees("C-a t takes write");
    w.keys(b"\x01p");
    w.sees("B -> A");
    w.sees("read-only (watching)");
    assert_eq!(users_on(&d, 0), (vec!["agent".to_string()], None));
    w.keys(b"\x01d");
    w.sees("detached");
    w.exits();
}

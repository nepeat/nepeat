//! consoled: owns the serial ports, keeps their scrollback, serves `console` clients.
//!
//!   consoled --config /etc/consoled/config.json [--check]
//!
//! One poll(2) loop over the listener, the ports and the clients; it never blocks on a
//! device: opening a tty (~1 s each on CP210x), closing one (the kernel may wait for output
//! to drain) and tcsendbreak run on a worker thread that reports back through a pipe. A port
//! that is missing or goes away (USB serial) is retried every 2 s; its clients stay attached.
//! A client too far behind (4 MiB queued) is dropped rather than slowing everyone down.

use consoled::config::{Config, Port as PortCfg};
use consoled::proto::{self, Cmd, Hello, PortStatus};
use consoled::{ring::Ring, serial};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::os::fd::FromRawFd;
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

const CLIENT_OUT_MAX: usize = 4 << 20;
const TX_MAX: usize = 64 << 10;
const REPLAY_CHUNK: usize = 64 << 10;
const REOPEN: Duration = Duration::from_secs(2);
const HINT_EVERY: Duration = Duration::from_secs(5);

struct Client {
    stream: UnixStream,
    dec: proto::Decoder,
    out: Vec<u8>,
    port: Option<usize>,
    who: String,
    close_after_flush: bool,
    dead: bool,
    last_hint: Option<Instant>,
}

impl Client {
    fn send(&mut self, kind: u8, payload: &[u8]) {
        proto::encode(kind, payload, &mut self.out);
    }
    fn info(&mut self, msg: &str) {
        self.send(proto::INFO, msg.as_bytes());
    }
    fn hint(&mut self, msg: &str) {
        let now = Instant::now();
        if self.last_hint.is_none_or(|t| now - t >= HINT_EVERY) {
            self.last_hint = Some(now);
            self.info(msg);
        }
    }
}

enum Job {
    Open(usize, PortCfg),
    Close(OwnedFd),
    Break(usize, OwnedFd), // requesting client, a dup of the port fd
}

enum Done {
    Opened(usize, Result<OwnedFd, String>),
    Broke(usize, Result<(), String>), // client
}

struct Worker {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    wake: OwnedFd, // readable when `done` has something
}

fn spawn_worker() -> Worker {
    let (jtx, jrx) = mpsc::channel::<Job>();
    let (dtx, drx) = mpsc::channel::<Done>();
    let mut p = [0; 2];
    let r = unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
    assert_eq!(r, 0, "pipe2");
    let (rd, wr) = unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) };
    std::thread::spawn(move || {
        let wake = |d: Done| {
            let _ = dtx.send(d);
            unsafe { libc::write(wr.as_raw_fd(), b"x".as_ptr().cast(), 1) };
        };
        for job in jrx {
            match job {
                Job::Open(i, cfg) => wake(Done::Opened(i, serial::open(&cfg).map_err(|e| e.to_string()))),
                Job::Close(fd) => drop(fd),
                Job::Break(client, fd) => {
                    let r = serial::send_break(fd.as_raw_fd()).map_err(|e| e.to_string());
                    drop(fd);
                    wake(Done::Broke(client, r));
                }
            }
        }
    });
    Worker { jobs: jtx, done: drx, wake: rd }
}

struct Port {
    cfg: PortCfg,
    fd: Option<OwnedFd>,
    ring: Ring,
    clients: Vec<usize>,
    writer: Option<usize>,
    tx: Vec<u8>,
    rx_bytes: u64,
    tx_bytes: u64,
    next_try: Instant,
    opening: bool,
    tried: bool,
    last_err: Option<String>,
}

struct Server {
    cfg: Config,
    listener: UnixListener,
    ports: Vec<Port>,
    clients: Vec<Option<Client>>,
    worker: Worker,
    initial: usize, // ports whose first open hasn't finished
}

enum Slot {
    Wake,
    Listener,
    Port(usize),
    Client(usize),
}

fn log(msg: &str) {
    eprintln!("{msg}");
}

impl Server {
    fn client(&mut self, id: usize) -> &mut Client {
        self.clients[id].as_mut().expect("live client")
    }

    fn broadcast(&mut self, port: usize, except: Option<usize>, msg: &str) {
        for id in self.ports[port].clients.clone() {
            if Some(id) != except {
                self.client(id).info(msg);
            }
        }
    }

    fn opened(&mut self, i: usize, r: Result<OwnedFd, String>) {
        let first = !self.ports[i].tried;
        self.ports[i].tried = true;
        self.ports[i].opening = false;
        if first {
            self.initial -= 1;
        }
        let p = &mut self.ports[i];
        match r {
            Ok(fd) => {
                p.fd = Some(fd);
                if p.last_err.take().is_some() {
                    log(&format!("{} ({}) is up, {}", p.cfg.name, p.cfg.device.display(), p.cfg.line_settings()));
                }
                self.broadcast(i, None, "device is up");
            }
            Err(err) => {
                p.next_try = Instant::now() + REOPEN;
                if p.last_err.as_deref() != Some(&err) {
                    log(&format!("{} ({}): {err}; retrying every {} s", p.cfg.name, p.cfg.device.display(), REOPEN.as_secs()));
                    p.last_err = Some(err);
                }
            }
        }
        if first && self.initial == 0 {
            let up = self.ports.iter().filter(|p| p.fd.is_some()).count();
            log(&format!("{up} of {} ports present", self.ports.len()));
        }
    }

    fn device_gone(&mut self, i: usize, why: &str) {
        let p = &mut self.ports[i];
        if let Some(fd) = p.fd.take() {
            let _ = self.worker.jobs.send(Job::Close(fd)); // close can block: not here
        }
        let p = &mut self.ports[i];
        p.tx.clear();
        p.next_try = Instant::now() + REOPEN;
        p.last_err = Some(why.to_string());
        log(&format!("{} ({}): {why}; retrying every {} s", p.cfg.name, p.cfg.device.display(), REOPEN.as_secs()));
        self.broadcast(i, None, &format!("device gone ({why}); retrying"));
    }

    fn read_port(&mut self, i: usize) {
        let mut buf = [0u8; 8192];
        loop {
            let Some(fd) = self.ports[i].fd.as_ref() else { return };
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                let data = &buf[..n as usize];
                let p = &mut self.ports[i];
                p.ring.push(data);
                p.rx_bytes += n as u64;
                for id in p.clients.clone() {
                    let c = self.client(id);
                    c.send(proto::DATA, data);
                    if c.out.len() > CLIENT_OUT_MAX && !c.dead {
                        c.dead = true;
                        log(&format!("dropping {}: more than {} MiB behind", c.who, CLIENT_OUT_MAX >> 20));
                    }
                }
                continue;
            }
            if n == 0 {
                return; // VMIN=0: nothing right now
            }
            let e = std::io::Error::last_os_error();
            match e.kind() {
                ErrorKind::WouldBlock | ErrorKind::Interrupted => return,
                _ => return self.device_gone(i, &e.to_string()),
            }
        }
    }

    fn write_port(&mut self, i: usize) {
        let p = &mut self.ports[i];
        let Some(fd) = p.fd.as_ref() else { return };
        if p.tx.is_empty() {
            return;
        }
        let n = unsafe { libc::write(fd.as_raw_fd(), p.tx.as_ptr().cast(), p.tx.len()) };
        if n > 0 {
            p.tx.drain(..n as usize);
            p.tx_bytes += n as u64;
        } else if n < 0 {
            let e = std::io::Error::last_os_error();
            if !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) {
                self.device_gone(i, &e.to_string());
            }
        }
    }

    fn accept(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    let c = Client {
                        stream,
                        dec: proto::Decoder::default(),
                        out: Vec::new(),
                        port: None,
                        who: "?".into(),
                        close_after_flush: false,
                        dead: false,
                        last_hint: None,
                    };
                    match self.clients.iter().position(Option::is_none) {
                        Some(id) => self.clients[id] = Some(c),
                        None => self.clients.push(Some(c)),
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    log(&format!("accept: {e}"));
                    return;
                }
            }
        }
    }

    fn status(&self) -> Vec<PortStatus> {
        self.ports
            .iter()
            .map(|p| PortStatus {
                name: p.cfg.name.clone(),
                label: p.cfg.label.clone(),
                device: p.cfg.device.display().to_string(),
                settings: p.cfg.line_settings(),
                up: p.fd.is_some(),
                clients: p.clients.len(),
                writer: p.writer.and_then(|w| self.clients[w].as_ref()).map(|c| c.who.clone()),
                rx_bytes: p.rx_bytes,
                tx_bytes: p.tx_bytes,
            })
            .collect()
    }

    fn hello(&mut self, id: usize, h: Hello) {
        let Some(key) = h.port else {
            let list = self.status();
            let c = self.client(id);
            proto::encode_json(proto::LIST, &list, &mut c.out);
            c.close_after_flush = true;
            return;
        };
        let Some(i) = self.cfg.find(&key) else {
            let c = self.client(id);
            c.send(proto::ERROR, format!("no port {key:?} (run `console` for the list)").as_bytes());
            c.close_after_flush = true;
            return;
        };
        let who = if h.who.is_empty() { format!("client{id}") } else { h.who };
        let p = &self.ports[i];
        let label = if p.cfg.label.is_empty() { String::new() } else { format!(" ({})", p.cfg.label) };
        let banner = format!(
            "{}{}, {}, {}{}. Escape: ^] then . quit, w write, r release, b break, ? help",
            p.cfg.name,
            label,
            p.cfg.line_settings(),
            p.cfg.device.display(),
            if p.fd.is_some() { "" } else { " (not present, retrying)" }
        );
        let chunks = if h.replay { p.ring.chunks(REPLAY_CHUNK) } else { Vec::new() };
        let holder = p.writer.and_then(|w| self.clients[w].as_ref()).map(|c| c.who.clone());
        let name = p.cfg.name.clone();
        {
            let c = self.client(id);
            c.port = Some(i);
            c.who = who.clone();
            c.info(&banner);
            for chunk in &chunks {
                c.send(proto::DATA, chunk);
            }
        }
        self.ports[i].clients.push(id);
        let mode = match (h.write, holder) {
            (true, None) => {
                self.ports[i].writer = Some(id);
                self.client(id).info("you have write");
                "write"
            }
            (true, Some(h)) => {
                self.client(id).info(&format!("read-only: {h} has write; ^] w takes it over"));
                "read-only"
            }
            (false, _) => {
                self.client(id).info("read-only (watching); ^] w takes write");
                "watching"
            }
        };
        self.broadcast(i, Some(id), &format!("{who} attached ({mode})"));
        log(&format!("{who} attached to {name} ({mode})"));
    }

    fn data(&mut self, id: usize, bytes: &[u8]) {
        let Some(i) = self.client(id).port else { return };
        if self.ports[i].writer != Some(id) {
            self.client(id).hint("read-only: ^] w takes write");
            return;
        }
        if self.ports[i].fd.is_none() {
            self.client(id).hint("device not present: input dropped");
            return;
        }
        let room = TX_MAX.saturating_sub(self.ports[i].tx.len());
        let take = bytes.len().min(room);
        self.ports[i].tx.extend_from_slice(&bytes[..take]);
        if take < bytes.len() {
            self.client(id).hint(&format!("port output buffer full: dropped {} bytes", bytes.len() - take));
        }
    }

    fn cmd(&mut self, id: usize, cmd: Cmd) {
        let Some(i) = self.client(id).port else { return };
        let who = self.client(id).who.clone();
        let name = self.ports[i].cfg.name.clone();
        match cmd {
            Cmd::Take => {
                let old = self.ports[i].writer;
                if old == Some(id) {
                    return self.client(id).info("you already have write");
                }
                self.ports[i].writer = Some(id);
                self.client(id).info("you have write");
                if let Some(o) = old {
                    self.client(o).info(&format!("{who} took write; you are read-only"));
                }
                self.broadcast(i, Some(id), &format!("{who} has write"));
                log(&format!("{who} took write on {name}"));
            }
            Cmd::Release => {
                if self.ports[i].writer == Some(id) {
                    self.ports[i].writer = None;
                    self.client(id).info("released write");
                    self.broadcast(i, Some(id), &format!("{who} released write; ^] w takes it"));
                } else {
                    self.client(id).info("you don't have write");
                }
            }
            Cmd::Break => {
                if self.ports[i].writer != Some(id) {
                    return self.client(id).info("only the writer can send a break");
                }
                let dup = self.ports[i].fd.as_ref().map(|fd| unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) });
                match dup {
                    Some(d) if d >= 0 => {
                        let _ = self.worker.jobs.send(Job::Break(id, unsafe { OwnedFd::from_raw_fd(d) }));
                    }
                    Some(_) => self.client(id).info("break failed: dup"),
                    None => self.client(id).info("device not present"),
                }
            }
        }
    }

    fn read_client(&mut self, id: usize) {
        let mut buf = [0u8; 8192];
        loop {
            let c = self.client(id);
            match c.stream.read(&mut buf) {
                Ok(0) => {
                    c.dead = true;
                    break;
                }
                Ok(n) => c.dec.push(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    c.dead = true;
                    break;
                }
            }
        }
        loop {
            let frame = match self.client(id).dec.next() {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    let c = self.client(id);
                    c.send(proto::ERROR, format!("protocol: {e}").as_bytes());
                    c.close_after_flush = true;
                    break;
                }
            };
            let attached = self.client(id).port.is_some();
            match (frame.0, attached) {
                (proto::HELLO, false) => match serde_json::from_slice::<Hello>(&frame.1) {
                    Ok(h) => self.hello(id, h),
                    Err(e) => {
                        let c = self.client(id);
                        c.send(proto::ERROR, format!("bad hello: {e}").as_bytes());
                        c.close_after_flush = true;
                    }
                },
                (proto::DATA, true) => self.data(id, &frame.1),
                (proto::CMD, true) => {
                    if let Ok(cmd) = serde_json::from_slice::<Cmd>(&frame.1) {
                        self.cmd(id, cmd);
                    }
                }
                _ => {
                    let c = self.client(id);
                    c.send(proto::ERROR, b"unexpected frame");
                    c.close_after_flush = true;
                }
            }
        }
    }

    fn flush_client(&mut self, id: usize) {
        let c = self.client(id);
        while !c.out.is_empty() {
            match c.stream.write(&c.out) {
                Ok(0) => {
                    c.dead = true;
                    return;
                }
                Ok(n) => {
                    c.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    c.dead = true;
                    return;
                }
            }
        }
        if c.close_after_flush {
            c.dead = true;
        }
    }

    fn reap(&mut self) {
        for id in 0..self.clients.len() {
            if !self.clients[id].as_ref().is_some_and(|c| c.dead) {
                continue;
            }
            let c = self.clients[id].take().expect("live");
            if let Some(i) = c.port {
                self.ports[i].clients.retain(|&x| x != id);
                let name = self.ports[i].cfg.name.clone();
                if self.ports[i].writer == Some(id) {
                    self.ports[i].writer = None;
                    self.broadcast(i, None, &format!("{} left; write is free (^] w)", c.who));
                } else {
                    self.broadcast(i, None, &format!("{} left", c.who));
                }
                log(&format!("{} detached from {name}", c.who));
            }
        }
    }

    fn run(&mut self) {
        while !STOP.load(Ordering::SeqCst) {
            let now = Instant::now();
            for i in 0..self.ports.len() {
                let p = &mut self.ports[i];
                if p.fd.is_none() && !p.opening && now >= p.next_try {
                    p.opening = true;
                    let _ = self.worker.jobs.send(Job::Open(i, p.cfg.clone()));
                }
            }

            let mut fds = vec![
                libc::pollfd { fd: self.worker.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let mut slots = vec![Slot::Wake, Slot::Listener];
            for (i, p) in self.ports.iter().enumerate() {
                if let Some(fd) = &p.fd {
                    let ev = libc::POLLIN | if p.tx.is_empty() { 0 } else { libc::POLLOUT };
                    fds.push(libc::pollfd { fd: fd.as_raw_fd(), events: ev, revents: 0 });
                    slots.push(Slot::Port(i));
                }
            }
            for (id, c) in self.clients.iter().enumerate() {
                if let Some(c) = c {
                    let ev = libc::POLLIN | if c.out.is_empty() { 0 } else { libc::POLLOUT };
                    fds.push(libc::pollfd { fd: c.stream.as_raw_fd(), events: ev, revents: 0 });
                    slots.push(Slot::Client(id));
                }
            }
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 1000) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() != ErrorKind::Interrupted {
                    log(&format!("poll: {e}"));
                }
                continue;
            }
            for (pfd, slot) in fds.iter().zip(&slots) {
                let r = pfd.revents;
                if r == 0 {
                    continue;
                }
                match *slot {
                    Slot::Wake => {
                        let mut b = [0u8; 256];
                        while unsafe { libc::read(self.worker.wake.as_raw_fd(), b.as_mut_ptr().cast(), b.len()) } > 0 {}
                        while let Ok(d) = self.worker.done.try_recv() {
                            match d {
                                Done::Opened(i, r) => self.opened(i, r),
                                Done::Broke(id, r) => {
                                    if let Some(c) = self.clients.get_mut(id).and_then(Option::as_mut) {
                                        c.info(&match r {
                                            Ok(()) => "break sent".to_string(),
                                            Err(e) => format!("break failed: {e}"),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Slot::Listener => self.accept(),
                    Slot::Port(i) => {
                        if r & libc::POLLIN != 0 {
                            self.read_port(i);
                        }
                        if r & libc::POLLOUT != 0 {
                            self.write_port(i);
                        }
                        if r & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 && self.ports[i].fd.is_some() {
                            self.device_gone(i, "hang-up");
                        }
                    }
                    Slot::Client(id) => {
                        if self.clients[id].is_none() {
                            continue;
                        }
                        if r & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                            self.read_client(id);
                        }
                    }
                }
            }
            for id in 0..self.clients.len() {
                if self.clients[id].as_ref().is_some_and(|c| !c.out.is_empty() || c.close_after_flush) {
                    self.flush_client(id);
                }
            }
            for i in 0..self.ports.len() {
                if !self.ports[i].tx.is_empty() {
                    self.write_port(i);
                }
            }
            self.reap();
        }
        log("stopping");
        let _ = std::fs::remove_file(&self.cfg.socket);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut config: Option<PathBuf> = None;
    let mut check = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => config = args.next().map(PathBuf::from),
            "--check" => check = true,
            "--help" | "-h" => {
                println!("usage: consoled --config FILE [--check]");
                return;
            }
            _ => {
                eprintln!("consoled: unknown argument {a:?}");
                std::process::exit(2);
            }
        }
    }
    let Some(config) = config else {
        eprintln!("usage: consoled --config FILE [--check]");
        std::process::exit(2);
    };
    let cfg = match Config::load(&config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("consoled: {e}");
            std::process::exit(1);
        }
    };
    if check {
        println!("{}: {} ports, scrollback {} bytes each", config.display(), cfg.ports.len(), cfg.scrollback);
        return;
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let _ = std::fs::remove_file(&cfg.socket);
    let listener = match UnixListener::bind(&cfg.socket) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("consoled: {}: {e}", cfg.socket.display());
            std::process::exit(1);
        }
    };
    let _ = std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(0o660));
    listener.set_nonblocking(true).expect("nonblocking listener");
    let ports: Vec<Port> = cfg
        .ports
        .iter()
        .map(|p| Port {
            cfg: p.clone(),
            fd: None,
            ring: Ring::new(cfg.scrollback),
            clients: Vec::new(),
            writer: None,
            tx: Vec::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            next_try: Instant::now(),
            opening: false,
            tried: false,
            last_err: None,
        })
        .collect();
    let initial = ports.len();
    log(&format!("serving {} ports on {} (opening the devices)", initial, cfg.socket.display()));
    let mut server = Server { cfg, listener, ports, clients: Vec::new(), worker: spawn_worker(), initial };
    server.run();
}

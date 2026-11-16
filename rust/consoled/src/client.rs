//! The client side: what `line` does (list, info, watch, attach, log, the menu, metrics).

use crate::proto::{self, Cmd, Hello, PortStatus};
use crate::term::RawMode;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub const DEFAULT_SOCKET: &str = "/run/consoled/console.sock";
const ESC: u8 = 0x1d; // ^]
pub const ESCAPES: &str = "^] then: . detach, w take write, r release write, b send break, ? help, ^] a literal ^]";

pub fn die(msg: &str) -> ! {
    eprintln!("line: {msg}");
    std::process::exit(1);
}

pub fn who() -> String {
    let user = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| format!("uid{}", unsafe { libc::getuid() }));
    match std::env::var("SSH_CONNECTION").ok().and_then(|s| s.split_whitespace().next().map(String::from)) {
        Some(ip) => format!("{user}@{ip}"),
        None => user,
    }
}

fn connect(socket: &str) -> UnixStream {
    UnixStream::connect(socket).unwrap_or_else(|e| die(&format!("{socket}: {e} (is consoled running?)")))
}

fn hello(socket: &str, h: &Hello) -> UnixStream {
    let mut s = connect(socket);
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, h, &mut out);
    s.write_all(&out).unwrap_or_else(|e| die(&format!("write: {e}")));
    s
}

fn read_frame(s: &mut UnixStream, dec: &mut proto::Decoder) -> Option<(u8, Vec<u8>)> {
    let mut buf = [0u8; 65536];
    loop {
        match dec.next() {
            Ok(Some(f)) => return Some(f),
            Ok(None) => {}
            Err(e) => die(&format!("protocol: {e}")),
        }
        match s.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => dec.push(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => die(&format!("read: {e}")),
        }
    }
}

pub fn status(socket: &str) -> Vec<PortStatus> {
    let mut s = hello(socket, &Hello { port: None, write: false, who: who(), replay: false, dump: false });
    let mut dec = proto::Decoder::default();
    match read_frame(&mut s, &mut dec) {
        Some((proto::LIST, body)) => serde_json::from_slice(&body).unwrap_or_else(|e| die(&format!("list: {e}"))),
        _ => die("no port list from consoled"),
    }
}

fn find<'a>(ports: &'a [PortStatus], key: &str) -> &'a PortStatus {
    let k = key.to_ascii_lowercase();
    ports
        .iter()
        .find(|p| p.name.to_ascii_lowercase() == k)
        .or_else(|| ports.iter().find(|p| !p.label.is_empty() && p.label.to_ascii_lowercase() == k))
        .unwrap_or_else(|| die(&format!("no port {key:?} (line list)")))
}

pub fn human_bytes(n: u64) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1048576 => format!("{:.1} KiB", n as f64 / 1024.0),
        _ => format!("{:.1} MiB", n as f64 / 1048576.0),
    }
}

pub fn human_idle(s: Option<u64>) -> String {
    match s {
        None => "-".into(),
        Some(s) if s < 120 => format!("{s}s"),
        Some(s) if s < 7200 => format!("{}m", s / 60),
        Some(s) if s < 172800 => format!("{}h", s / 3600),
        Some(s) => format!("{}d", s / 86400),
    }
}

fn row(p: &PortStatus, width: usize) -> String {
    let dev = if p.up { "up" } else { "absent" };
    let users = match (p.users.len(), &p.writer) {
        (0, _) => String::new(),
        (n, Some(w)) if n == 1 => format!("{w} (w)"),
        (n, Some(w)) => format!("{w} (w) +{}", n - 1),
        (n, None) => format!("{n} watching"),
    };
    let line = format!(
        "{:<5} {:<16} {:<14} {:<6} {:>9} {:>5}  {}",
        p.name,
        p.label,
        p.settings,
        dev,
        human_bytes(p.rx_bytes),
        human_idle(p.idle_secs),
        users
    );
    line.chars().take(width).collect()
}

const HEADER: &str = "PORT  LABEL            SETTINGS       DEVICE        RX  IDLE  USERS";

pub fn list(socket: &str, json: bool) {
    let ports = status(socket);
    if json {
        println!("{}", serde_json::to_string_pretty(&ports).expect("json"));
        return;
    }
    println!("{HEADER}");
    for p in &ports {
        println!("{}", row(p, 200));
    }
}

pub fn info(socket: &str, key: &str) {
    let ports = status(socket);
    let p = find(&ports, key);
    let label = if p.label.is_empty() { String::new() } else { format!(" ({})", p.label) };
    println!("{}{label}", p.name);
    println!("  device     {} ({})", p.device, if p.up { "open" } else { "absent, retrying every 2 s" });
    println!("  settings   {}", p.settings);
    if p.users.is_empty() {
        println!("  attached   nobody");
    } else {
        for (i, u) in p.users.iter().enumerate() {
            let role = if i == 0 && p.writer.as_deref() == Some(u.as_str()) { "write" } else { "watching" };
            println!("  {:<10} {u} ({role})", if i == 0 { "attached" } else { "" });
        }
    }
    println!("  received   {} (scrollback {})", human_bytes(p.rx_bytes), human_bytes(p.scrollback_bytes as u64));
    println!("  sent       {}", human_bytes(p.tx_bytes));
    println!("  last rx    {}", match p.idle_secs {
        None => "nothing since consoled started".into(),
        Some(s) => format!("{} ago", human_idle(Some(s))),
    });
}

/// The scrollback, raw, to stdout.
pub fn log(socket: &str, key: &str) {
    let mut s = hello(socket, &Hello { port: Some(key.into()), write: false, who: who(), replay: true, dump: true });
    let mut dec = proto::Decoder::default();
    let mut o = std::io::stdout().lock();
    while let Some((kind, body)) = read_frame(&mut s, &mut dec) {
        match kind {
            proto::DATA => {
                if o.write_all(&body).is_err() {
                    return;
                }
            }
            proto::ERROR => die(&String::from_utf8_lossy(&body)),
            _ => {}
        }
    }
    let _ = o.flush();
}

pub fn metrics(socket: &str) {
    let ports = status(socket);
    let lbl = |p: &PortStatus| format!("port=\"{}\",label=\"{}\"", p.name, p.label.replace(['"', '\\', '\n'], ""));
    let mut s = String::new();
    let mut metric = |name: &str, kind: &str, help: &str, v: &dyn Fn(&PortStatus) -> u64| {
        s.push_str(&format!("# HELP consoled_{name} {help}\n# TYPE consoled_{name} {kind}\n"));
        for p in &ports {
            s.push_str(&format!("consoled_{name}{{{}}} {}\n", lbl(p), v(p)));
        }
    };
    metric("port_up", "gauge", "1 while the serial device is open", &|p| p.up as u64);
    metric("port_clients", "gauge", "attached clients", &|p| p.clients as u64);
    metric("port_writer", "gauge", "1 while a client holds write", &|p| p.writer.is_some() as u64);
    metric("port_rx_bytes_total", "counter", "bytes read from the device", &|p| p.rx_bytes);
    metric("port_tx_bytes_total", "counter", "bytes written to the device", &|p| p.tx_bytes);
    print!("{s}");
}

fn say(msg: &str) {
    let mut o = std::io::stdout().lock();
    let _ = write!(o, "\r\n[line] {msg}\r\n");
    let _ = o.flush();
}

/// Attach this terminal to a port until ^] . or the daemon goes away.
pub fn attach(socket: &str, port: &str, write: bool) {
    let mut s = hello(socket, &Hello { port: Some(port.into()), write, who: who(), replay: true, dump: false });
    let in_fd = std::io::stdin().as_raw_fd();
    let raw = RawMode::enable(in_fd).unwrap_or_else(|e| die(&format!("terminal: {e}")));
    if raw.is_none() {
        eprintln!("line: stdin is not a terminal (over ssh use `ssh -t`); input is passed through as is");
    }
    let mut dec = proto::Decoder::default();
    let mut esc = false;
    let mut stdin_open = true;
    let mut buf = [0u8; 65536];
    loop {
        let mut fds = [
            libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: if stdin_open { in_fd } else { -1 }, events: libc::POLLIN, revents: 0 },
        ];
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[0].revents != 0 {
            match s.read(&mut buf) {
                Ok(0) => {
                    say("disconnected");
                    return;
                }
                Ok(n) => dec.push(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    drop(raw);
                    die(&format!("read: {e}"));
                }
            }
            loop {
                match dec.next() {
                    Ok(Some((proto::DATA, d))) => {
                        let mut o = std::io::stdout().lock();
                        let _ = o.write_all(&d);
                        let _ = o.flush();
                    }
                    Ok(Some((proto::INFO, m))) => say(&String::from_utf8_lossy(&m)),
                    Ok(Some((proto::ERROR, m))) => {
                        drop(raw);
                        die(&String::from_utf8_lossy(&m));
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(e) => {
                        drop(raw);
                        die(&format!("protocol: {e}"));
                    }
                }
            }
        }
        if stdin_open && fds[1].revents != 0 {
            let n = match std::io::stdin().lock().read(&mut buf) {
                Ok(0) => {
                    stdin_open = false; // piped input done: keep watching until the daemon or ^C ends it
                    continue;
                }
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let mut data = Vec::with_capacity(n);
            let mut frames = Vec::new();
            for &b in &buf[..n] {
                if esc {
                    esc = false;
                    match b {
                        b'.' => {
                            if !data.is_empty() {
                                proto::encode(proto::DATA, &data, &mut frames);
                            }
                            let _ = s.write_all(&frames);
                            say("detached");
                            return;
                        }
                        b'w' | b'W' => proto::encode_json(proto::CMD, &Cmd::Take, &mut frames),
                        b'r' | b'R' => proto::encode_json(proto::CMD, &Cmd::Release, &mut frames),
                        b'b' | b'B' => proto::encode_json(proto::CMD, &Cmd::Break, &mut frames),
                        ESC => data.push(ESC),
                        _ => say(ESCAPES),
                    }
                    continue;
                }
                if b == ESC {
                    if !data.is_empty() {
                        proto::encode(proto::DATA, &data, &mut frames);
                        data.clear();
                    }
                    esc = true;
                } else {
                    data.push(b);
                }
            }
            if !data.is_empty() {
                proto::encode(proto::DATA, &data, &mut frames);
            }
            if !frames.is_empty() && s.write_all(&frames).is_err() {
                break;
            }
        }
    }
}

// ---- the menu ------------------------------------------------------------------------------

fn term_size() -> (usize, usize) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_row > 0 {
        (ws.ws_row as usize, ws.ws_col as usize)
    } else {
        (24, 80)
    }
}

fn hostname() -> String {
    let mut b = [0u8; 256];
    if unsafe { libc::gethostname(b.as_mut_ptr().cast(), b.len()) } == 0 {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..n]).into_owned()
    } else {
        "?".into()
    }
}

enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Char(u8),
    Quit,
}

fn keys(bytes: &[u8]) -> Vec<Key> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let (k, n) = match rest {
            [0x1b, b'[', b'A', ..] | [0x1b, b'O', b'A', ..] => (Key::Up, 3),
            [0x1b, b'[', b'B', ..] | [0x1b, b'O', b'B', ..] => (Key::Down, 3),
            [0x1b, b'[', b'5', b'~', ..] => (Key::PageUp, 4),
            [0x1b, b'[', b'6', b'~', ..] => (Key::PageDown, 4),
            [0x1b, b'[', b'H', ..] | [0x1b, b'O', b'H', ..] => (Key::Home, 3),
            [0x1b, b'[', b'F', ..] | [0x1b, b'O', b'F', ..] => (Key::End, 3),
            [0x1b, b'[', ..] => (Key::Char(0), rest.iter().skip(2).position(|c| c.is_ascii_alphabetic() || *c == b'~').map_or(rest.len(), |p| p + 3)),
            [0x1b] => (Key::Quit, 1),
            [b'\r', ..] | [b'\n', ..] => (Key::Enter, 1),
            [3, ..] | [b'q', ..] | [b'Q', ..] => (Key::Quit, 1),
            [b'k', ..] => (Key::Up, 1),
            [b'j', ..] => (Key::Down, 1),
            [b'g', ..] => (Key::Home, 1),
            [b'G', ..] => (Key::End, 1),
            [c, ..] => (Key::Char(*c), 1),
            [] => break,
        };
        out.push(k);
        i += n;
    }
    out
}

struct Screen;

impl Screen {
    fn enter() -> Screen {
        print!("\x1b[?1049h\x1b[?25l");
        let _ = std::io::stdout().flush();
        Screen
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        print!("\x1b[?25h\x1b[?1049l");
        let _ = std::io::stdout().flush();
    }
}

/// Full-screen port picker; attach/watch from it and come back after ^] .
pub fn menu(socket: &str) {
    let in_fd = std::io::stdin().as_raw_fd();
    if unsafe { libc::isatty(in_fd) == 0 || libc::isatty(1) == 0 } {
        return list(socket, false);
    }
    let host = hostname();
    let mut sel = 0usize;
    let mut top = 0usize;
    let mut jump = String::new();
    let mut jump_at = Instant::now();
    let mut msg = String::new();
    loop {
        let ports = status(socket);
        if ports.is_empty() {
            return list(socket, false);
        }
        sel = sel.min(ports.len() - 1);
        let action;
        {
            let _raw = RawMode::enable(in_fd).unwrap_or_else(|e| die(&format!("terminal: {e}")));
            let _screen = Screen::enter();
            let mut ports = ports;
            loop {
                let (rows, cols) = term_size();
                let body = rows.saturating_sub(4).max(1);
                if sel < top {
                    top = sel;
                }
                if sel >= top + body {
                    top = sel + 1 - body;
                }
                let up = ports.iter().filter(|p| p.up).count();
                let mut f = String::from("\x1b[H\x1b[2J");
                let title = format!(" line on {host}: {} ports, {up} up", ports.len());
                let keys_help = "↑↓/jk move  ⏎ attach  w watch  i info  q quit ";
                let pad = cols.saturating_sub(title.chars().count() + keys_help.chars().count());
                f.push_str(&format!("\x1b[1m{title}{}{keys_help}\x1b[0m\r\n", " ".repeat(pad)));
                f.push_str(&format!("\x1b[2m {}\x1b[0m\r\n", &HEADER.chars().take(cols.saturating_sub(1)).collect::<String>()));
                for (i, p) in ports.iter().enumerate().skip(top).take(body) {
                    let r = row(p, cols.saturating_sub(1));
                    if i == sel {
                        f.push_str(&format!("\x1b[7m {r:<w$}\x1b[0m\r\n", w = cols.saturating_sub(1)));
                    } else if !p.up {
                        f.push_str(&format!("\x1b[2m {r}\x1b[0m\r\n"));
                    } else {
                        f.push_str(&format!(" {r}\r\n"));
                    }
                }
                f.push_str(&format!("\x1b[{rows};1H\x1b[2m {}\x1b[0m", msg.chars().take(cols.saturating_sub(1)).collect::<String>()));
                print!("{f}");
                let _ = std::io::stdout().flush();

                let mut pfd = libc::pollfd { fd: in_fd, events: libc::POLLIN, revents: 0 };
                let n = unsafe { libc::poll(&mut pfd, 1, 2000) };
                if n == 0 {
                    ports = status(socket); // refresh every 2 s
                    sel = sel.min(ports.len() - 1);
                    continue;
                }
                let mut buf = [0u8; 64];
                let got = std::io::stdin().lock().read(&mut buf).unwrap_or(0);
                if got == 0 {
                    return;
                }
                let mut act = None;
                for k in keys(&buf[..got]) {
                    match k {
                        Key::Up => sel = sel.saturating_sub(1),
                        Key::Down => sel = (sel + 1).min(ports.len() - 1),
                        Key::PageUp => sel = sel.saturating_sub(body),
                        Key::PageDown => sel = (sel + body).min(ports.len() - 1),
                        Key::Home => sel = 0,
                        Key::End => sel = ports.len() - 1,
                        Key::Enter => act = Some(('a', ports[sel].name.clone())),
                        Key::Char(b'w') | Key::Char(b'W') => act = Some(('w', ports[sel].name.clone())),
                        Key::Char(b'i') | Key::Char(b'I') => {
                            let p = &ports[sel];
                            msg = format!(
                                "{}: {} {}, rx {}, tx {}, scrollback {}, idle {}{}",
                                p.name,
                                p.device,
                                if p.up { "open" } else { "absent" },
                                human_bytes(p.rx_bytes),
                                human_bytes(p.tx_bytes),
                                human_bytes(p.scrollback_bytes as u64),
                                human_idle(p.idle_secs),
                                if p.users.is_empty() { String::new() } else { format!(", users: {}", p.users.join(", ")) }
                            );
                        }
                        Key::Char(c) if c.is_ascii_digit() => {
                            // type a port number to jump to it
                            if jump_at.elapsed() > Duration::from_millis(1500) {
                                jump.clear();
                            }
                            jump.push(c as char);
                            jump_at = Instant::now();
                            if let Some(i) = ports.iter().position(|p| p.name.trim_start_matches(|c: char| !c.is_ascii_digit()) == jump) {
                                sel = i;
                            }
                        }
                        Key::Quit => return,
                        Key::Char(_) => {}
                    }
                }
                if act.is_some() {
                    action = act.unwrap();
                    break;
                }
            }
        }
        // the screen and raw mode are restored here; attach sets its own raw mode
        let (kind, port) = action;
        msg = String::new();
        attach(socket, &port, kind == 'a');
    }
}

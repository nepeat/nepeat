//! The interactive side of `line`: attach/watch with tmux-style keys, the port menu (also
//! the in-session chooser), the scrollback view and the `:` command prompt.
//!
//! While attached, the prefix key (C-b unless --prefix / $CONSOLED_PREFIX say otherwise;
//! ^] always works too) starts a command; see `help_text`. Port output that arrives
//! while a full-screen view or the prompt is up is kept and shown when it closes.

use crate::cli::key_name;
use crate::client::{self, HEADER, die, human_bytes, human_idle, row, who};
use crate::proto::{self, Cmd, Hello, PortState, PortStatus};
use crate::term::{self, RawMode, Screen};
use crate::text;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// ^]: the old escape key, still a prefix.
pub const ALIAS: u8 = 0x1d;
/// Port output kept while a view is up; older bytes go first beyond this.
const PENDING_MAX: usize = 8 << 20;

pub struct Keys {
    pub prefix: u8,
}

impl Keys {
    pub fn name(&self) -> String {
        key_name(self.prefix)
    }
    fn is_prefix(&self, b: u8) -> bool {
        b == self.prefix || b == ALIAS
    }
}

/// The keys, for ? and `line help`.
pub fn help_text(k: &Keys) -> String {
    let p = k.name();
    let mut s = if k.prefix == ALIAS {
        format!("keys: press {p}, then\n")
    } else {
        format!("keys: press {p} (or ^]), then\n")
    };
    let rows = [
        ("d", "detach", "?", "this help"),
        ("[", "scrollback (arrows PgUp PgDn g G; q, Esc leave)", "", ""),
        ("s w", "port chooser", "n p", "next / previous port"),
        ("t", "take write", "r", "release write"),
        (
            "B",
            "send a break",
            ":",
            "command: take, release, break, baud RATE, detach",
        ),
    ];
    for (a, b, c, d) in rows {
        if c.is_empty() {
            s.push_str(&format!("  {a:<4} {b}\n"));
        } else {
            s.push_str(&format!("  {a:<4} {b:<22} {c:<4} {d}\n"));
        }
    }
    if k.prefix == ALIAS {
        s.push_str(&format!(
            "  {p}  a literal {p}\nafter ^] the old keys work too: . detach, w take write, b break\n"
        ));
    } else {
        s.push_str(&format!("  {p}  a literal {p} (^] ^] a literal ^])\nafter ^] the old keys work too: . detach, w take write, b break\n"));
    }
    s
}

pub enum Exit {
    Detach,
    Gone,
    Error(String),
    Switch(String, bool),
}

struct Scroll {
    /// Bytes of a scrollback still arriving (None once it is all here).
    loading: Option<Vec<u8>>,
    lines: Vec<String>,
    top: usize,
    news: bool,
    _screen: Screen,
}

enum Overlay {
    None,
    Prompt(String),
    Scroll(Scroll),
    Chooser,
}

struct Session<'a> {
    socket: &'a str,
    keys: &'a Keys,
    s: UnixStream,
    dec: proto::Decoder,
    key: String,
    write: bool,
    state: Option<PortState>,
    overlay: Overlay,
    pending: Vec<u8>,
    prefix: Option<u8>,
}

fn stdout_write(b: &[u8]) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(b);
    let _ = o.flush();
}

fn say(msg: &str) {
    stdout_write(format!("\r\n[line] {}\r\n", msg.trim_end().replace('\n', "\r\n")).as_bytes());
}

impl<'a> Session<'a> {
    fn open(socket: &'a str, keys: &'a Keys, port: &str, write: bool) -> Result<Session<'a>, String> {
        let h = Hello {
            port: Some(port.into()),
            write,
            who: who(),
            replay: true,
            prefix: keys.name(),
            ..Hello::default()
        };
        let s = client::try_hello(socket, &h)?;
        Ok(Session {
            socket,
            keys,
            s,
            dec: proto::Decoder::default(),
            key: port.into(),
            write,
            state: None,
            overlay: Overlay::None,
            pending: Vec::new(),
            prefix: None,
        })
    }

    fn port_name(&self) -> String {
        self.state.as_ref().map_or_else(|| self.key.clone(), |s| s.port.clone())
    }

    /// Port output: to the terminal, or kept while a view is up.
    fn out(&mut self, b: &[u8]) {
        match self.overlay {
            Overlay::None => stdout_write(b),
            _ => {
                self.pending.extend_from_slice(b);
                if self.pending.len() > PENDING_MAX {
                    let over = self.pending.len() - PENDING_MAX;
                    self.pending.drain(..over);
                }
            }
        }
    }

    fn say(&mut self, msg: &str) {
        self.out(format!("\r\n[line] {}\r\n", msg.trim_end().replace('\n', "\r\n")).as_bytes());
    }

    /// Close any view and show what came meanwhile.
    fn leave(&mut self) {
        if matches!(self.overlay, Overlay::Prompt(_)) {
            stdout_write(b"\r\n");
        }
        self.overlay = Overlay::None; // drops the alternate screen
        let p = std::mem::take(&mut self.pending);
        if !p.is_empty() {
            stdout_write(&p);
        }
    }

    fn send(&mut self, frames: &[u8]) -> Result<(), Exit> {
        if frames.is_empty() {
            return Ok(());
        }
        self.s.write_all(frames).map_err(|_| Exit::Gone)
    }

    fn cmd(&mut self, c: Cmd) -> Result<(), Exit> {
        let mut f = Vec::new();
        proto::encode_json(proto::CMD, &c, &mut f);
        self.send(&f)
    }

    /// Read what the daemon sent (the socket is readable).
    fn pump(&mut self) -> Result<(), Exit> {
        let mut buf = [0u8; 65536];
        let mut redraw = false;
        match self.s.read(&mut buf) {
            Ok(0) => return Err(Exit::Gone),
            Ok(n) => self.dec.push(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(Exit::Error(format!("read: {e}"))),
        }
        loop {
            match self.dec.next() {
                Ok(Some((proto::DATA, d))) => self.out(&d),
                Ok(Some((proto::INFO, m))) => self.say(&String::from_utf8_lossy(&m)),
                Ok(Some((proto::ERROR, m))) => return Err(Exit::Error(String::from_utf8_lossy(&m).into_owned())),
                Ok(Some((proto::STATE, b))) => {
                    if let Ok(st) = serde_json::from_slice::<PortState>(&b) {
                        self.state = Some(st);
                    }
                }
                Ok(Some((proto::SCROLLBACK, b))) => {
                    if let Overlay::Scroll(Scroll { loading: Some(acc), .. }) = &mut self.overlay {
                        acc.extend_from_slice(&b);
                    }
                    if let (true, Overlay::Scroll(sc)) = (b.is_empty(), &mut self.overlay) {
                        if let Some(all) = sc.loading.take() {
                            let (rows, cols) = term::size();
                            sc.lines = view_lines(&all, cols);
                            sc.top = sc.lines.len().saturating_sub(rows.saturating_sub(1));
                            redraw = true;
                        }
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => return Err(Exit::Error(format!("protocol: {e}"))),
            }
        }
        if let Overlay::Scroll(sc) = &mut self.overlay {
            if !self.pending.is_empty() && !sc.news && sc.loading.is_none() {
                sc.news = true;
                redraw = true;
            }
        }
        if redraw {
            self.draw_scroll();
        }
        Ok(())
    }

    fn run(&mut self) -> Exit {
        let in_fd = std::io::stdin().as_raw_fd();
        let mut stdin_open = true;
        let mut buf = [0u8; 4096];
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: self.s.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: if stdin_open { in_fd } else { -1 },
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
                if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Exit::Error("poll failed".into());
            }
            if fds[0].revents != 0 {
                if let Err(e) = self.pump() {
                    self.leave();
                    return e;
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
                    Err(_) => {
                        stdin_open = false;
                        continue;
                    }
                };
                if let Err(e) = self.input(&buf[..n]) {
                    self.leave();
                    return e;
                }
            }
        }
    }

    /// Keystrokes: to the port, or to the prefix commands and the views.
    fn input(&mut self, bytes: &[u8]) -> Result<(), Exit> {
        let mut frames = Vec::new();
        let mut data = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match self.overlay {
                Overlay::None | Overlay::Chooser => {
                    let b = bytes[i];
                    i += 1;
                    if let Some(p) = self.prefix.take() {
                        if !data.is_empty() {
                            proto::encode(proto::DATA, &data, &mut frames);
                            data.clear();
                        }
                        if self.keys.is_prefix(b) {
                            data.push(b); // the prefix twice: the key itself
                            continue;
                        }
                        self.send(&std::mem::take(&mut frames))?;
                        if self.prefixed(p, b)? {
                            i = bytes.len(); // a view took over: the rest of this read is dropped
                        }
                    } else if self.keys.is_prefix(b) {
                        self.prefix = Some(b);
                    } else {
                        data.push(b);
                    }
                }
                Overlay::Prompt(_) => i += self.prompt_key(&bytes[i..])?,
                Overlay::Scroll(_) => {
                    let (k, n) = key_at(&bytes[i..]);
                    i += n;
                    self.scroll_key(k);
                }
            }
        }
        if !data.is_empty() {
            proto::encode(proto::DATA, &data, &mut frames);
        }
        self.send(&frames)
    }

    /// After the prefix `p`: true when a view or another port takes over the terminal.
    fn prefixed(&mut self, p: u8, b: u8) -> Result<bool, Exit> {
        let legacy = p == ALIAS; // ^] w, ^] b and ^] . mean what they always did
        match b {
            b'd' | b'.' => return Err(Exit::Detach),
            b'?' | b'h' => {
                let h = help_text(self.keys);
                self.say(&h);
            }
            b'[' => {
                self.cmd(Cmd::Scrollback)?;
                self.overlay = Overlay::Scroll(Scroll {
                    loading: Some(Vec::new()),
                    lines: Vec::new(),
                    top: 0,
                    news: false,
                    _screen: Screen::enter(),
                });
                self.draw_scroll();
                return Ok(true);
            }
            b'w' | b'W' if legacy => self.cmd(Cmd::Take)?,
            b'b' if legacy => self.cmd(Cmd::Break)?,
            b's' | b'w' => {
                self.choose()?;
                return Ok(true);
            }
            b'n' => return self.neighbour(1).map(|_| true),
            b'p' => return self.neighbour(-1).map(|_| true),
            b':' => {
                self.overlay = Overlay::Prompt(String::new());
                stdout_write(b"\r\n[line] :");
            }
            b't' | b'T' => self.cmd(Cmd::Take)?,
            b'r' | b'R' => self.cmd(Cmd::Release)?,
            b'B' => self.cmd(Cmd::Break)?,
            _ => {
                let p = key_name(p);
                self.say(&format!("{p} {}: no such key; {p} ? lists them", printable(b)));
            }
        }
        Ok(false)
    }

    /// The next (1) or previous (-1) port in the menu's order, in the same mode.
    fn neighbour(&mut self, step: isize) -> Result<(), Exit> {
        let ports = match client::try_status(self.socket) {
            Ok(p) => p,
            Err(e) => {
                self.say(&e);
                return Ok(());
            }
        };
        let cur = self.port_name();
        let Some(i) = ports.iter().position(|p| p.name.eq_ignore_ascii_case(&cur)) else {
            return Ok(());
        };
        if ports.len() < 2 {
            self.say("there is only this port");
            return Ok(());
        }
        let j = (i as isize + step).rem_euclid(ports.len() as isize) as usize;
        Err(Exit::Switch(ports[j].name.clone(), self.write))
    }

    /// The port chooser (the menu) over this session, which stays attached meanwhile.
    fn choose(&mut self) -> Result<(), Exit> {
        self.overlay = Overlay::Chooser;
        let cur = self.port_name();
        let socket = self.socket;
        let mut msg = String::new();
        let keys = self.keys;
        let pick = picker(socket, keys, Some(&cur), Some(self), &mut msg);
        match pick {
            Pick::Port(name, write) if !name.eq_ignore_ascii_case(&cur) => {
                self.overlay = Overlay::None;
                self.pending.clear();
                Err(Exit::Switch(name, write))
            }
            Pick::Exit(e) => Err(e),
            _ => {
                self.leave();
                Ok(())
            }
        }
    }

    /// One key (or escape sequence) at the `:` prompt; returns the bytes it used.
    fn prompt_key(&mut self, rest: &[u8]) -> Result<usize, Exit> {
        let Overlay::Prompt(line) = &mut self.overlay else {
            return Ok(rest.len());
        };
        match rest[0] {
            0x1b => {
                // an arrow key and friends: ignored; Esc alone: cancel
                match rest.get(1) {
                    Some(b'[') => {
                        return Ok(rest
                            .iter()
                            .skip(2)
                            .position(|c| (0x40..=0x7e).contains(c))
                            .map_or(rest.len(), |p| p + 3));
                    }
                    Some(b'O') => return Ok(rest.len().min(3)),
                    _ => {
                        self.leave();
                        return Ok(1);
                    }
                }
            }
            0x03 | 0x07 => self.leave(),
            b'\r' | b'\n' => {
                let line = std::mem::take(line);
                self.leave();
                self.command(line.trim())?;
            }
            0x7f | 0x08 => {
                if line.pop().is_some() {
                    stdout_write(b"\x08 \x08");
                }
            }
            0x15 => {
                stdout_write("\x08 \x08".repeat(line.chars().count()).as_bytes());
                line.clear();
            }
            c @ 0x20..=0x7e if line.len() < 200 => {
                line.push(c as char);
                stdout_write(&[c]);
            }
            _ => {}
        }
        Ok(1)
    }

    fn command(&mut self, line: &str) -> Result<(), Exit> {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] => Ok(()),
            ["take" | "t"] => self.cmd(Cmd::Take),
            ["release" | "r"] => self.cmd(Cmd::Release),
            ["break" | "B"] => self.cmd(Cmd::Break),
            ["baud", rate] => match rate.parse::<u32>() {
                Ok(r) if crate::serial::baud_constant(r).is_some() => self.cmd(Cmd::Baud(r)),
                _ => {
                    let all: Vec<String> = crate::serial::BAUD_RATES.iter().map(u32::to_string).collect();
                    self.say(&format!("baud {rate}: one of {}", all.join(" ")));
                    Ok(())
                }
            },
            ["detach" | "d" | "quit"] => Err(Exit::Detach),
            ["help" | "?"] => {
                let h = help_text(self.keys);
                self.say(&h);
                Ok(())
            }
            ["next" | "n"] => self.neighbour(1),
            ["prev" | "p"] => self.neighbour(-1),
            _ => {
                self.say(&format!(
                    "{line:?}: take, release, break, baud RATE, next, prev, detach, help"
                ));
                Ok(())
            }
        }
    }

    fn scroll_key(&mut self, k: Key) {
        let (rows, _) = term::size();
        let body = rows.saturating_sub(1).max(1);
        let Overlay::Scroll(sc) = &mut self.overlay else { return };
        let max = sc.lines.len().saturating_sub(body);
        match k {
            Key::Quit => return self.leave(),
            _ if sc.loading.is_some() => return,
            Key::Up => sc.top = sc.top.saturating_sub(1),
            Key::Down => sc.top = (sc.top + 1).min(max),
            Key::PageUp | Key::Char(b'b') | Key::Char(0x02) => sc.top = sc.top.saturating_sub(body),
            Key::PageDown | Key::Char(b' ') | Key::Char(0x06) => sc.top = (sc.top + body).min(max),
            Key::Home => sc.top = 0,
            Key::End => sc.top = max,
            _ => return,
        }
        self.draw_scroll();
    }

    fn draw_scroll(&self) {
        let Overlay::Scroll(sc) = &self.overlay else { return };
        let (rows, cols) = term::size();
        let body = rows.saturating_sub(1).max(1);
        let mut f = String::from("\x1b[H\x1b[2J");
        if sc.loading.is_some() {
            f.push_str(" fetching the scrollback (q leaves)");
        } else {
            for (r, l) in sc.lines.iter().skip(sc.top).take(body).enumerate() {
                f.push_str(&format!("\x1b[{};1H{l}", r + 1));
            }
        }
        let name = self.port_name();
        let label = self
            .state
            .as_ref()
            .filter(|s| !s.label.is_empty())
            .map(|s| format!(" ({})", s.label))
            .unwrap_or_default();
        let pos = if sc.lines.is_empty() {
            "empty".to_string()
        } else {
            format!(
                "lines {}-{} of {}",
                sc.top + 1,
                (sc.top + body).min(sc.lines.len()),
                sc.lines.len()
            )
        };
        let news = if sc.news { ", new output waiting" } else { "" };
        let status = format!(" scrollback {name}{label}: {pos}{news}  ↑↓ PgUp PgDn g G move, q leaves");
        let status: String = status.chars().take(cols.saturating_sub(1)).collect();
        f.push_str(&format!(
            "\x1b[{rows};1H\x1b[7m{status:<w$}\x1b[0m",
            w = cols.saturating_sub(1)
        ));
        stdout_write(f.as_bytes());
    }
}

fn printable(b: u8) -> String {
    match b {
        0x20..=0x7e => (b as char).to_string(),
        0..0x20 => format!("^{}", (b + 0x40) as char),
        _ => format!("0x{b:02x}"),
    }
}

/// The scrollback as screen lines: cleaned, tabs expanded, wrapped at `cols`.
fn view_lines(raw: &[u8], cols: usize) -> Vec<String> {
    let cols = cols.max(20);
    let mut out = Vec::new();
    let mut lines = text::lines(&text::clean(raw));
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for l in lines {
        let mut cur = String::new();
        let mut n = 0;
        for c in l.chars() {
            if c == '\t' {
                let to = (n / 8 + 1) * 8;
                while n < to && n < cols {
                    cur.push(' ');
                    n += 1;
                }
            } else {
                cur.push(c);
                n += 1;
            }
            if n >= cols {
                out.push(std::mem::take(&mut cur));
                n = 0;
            }
        }
        if n > 0 || cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// Attach (or watch, `write` false) and switch ports on request; the last port and why it ended.
fn drive(socket: &str, keys: &Keys, mut port: String, mut write: bool) -> (Exit, String) {
    loop {
        let mut s = match Session::open(socket, keys, &port, write) {
            Ok(s) => s,
            Err(e) => return (Exit::Error(e), port),
        };
        match s.run() {
            Exit::Switch(p, w) => {
                say(&format!("{} -> {p}", s.port_name()));
                (port, write) = (p, w);
            }
            e => return (e, s.port_name()),
        }
    }
}

/// Attach this terminal to a port until the prefix and d, or the daemon goes away.
pub fn attach(socket: &str, port: &str, write: bool, keys: &Keys) {
    let in_fd = std::io::stdin().as_raw_fd();
    let raw = RawMode::enable(in_fd).unwrap_or_else(|e| die(&format!("terminal: {e}")));
    if raw.is_none() {
        eprintln!("line: stdin is not a terminal (over ssh use `ssh -t`); input is passed through as is");
    }
    match drive(socket, keys, port.into(), write).0 {
        Exit::Detach => say("detached"),
        Exit::Gone => say("disconnected"),
        Exit::Error(m) => {
            drop(raw);
            die(&m);
        }
        Exit::Switch(..) => {}
    }
}

/// Full-screen port picker; attach/watch from it and come back after detaching.
pub fn menu(socket: &str, keys: &Keys) {
    let in_fd = std::io::stdin().as_raw_fd();
    if unsafe { libc::isatty(in_fd) == 0 || libc::isatty(1) == 0 } || client::status(socket).is_empty() {
        return client::list(socket, false);
    }
    let _raw = RawMode::enable(in_fd).unwrap_or_else(|e| die(&format!("terminal: {e}")));
    let mut last: Option<String> = None;
    let mut msg = String::new();
    loop {
        match picker(socket, keys, last.as_deref(), None, &mut msg) {
            Pick::Port(name, write) => {
                let (e, port) = drive(socket, keys, name, write);
                last = Some(port);
                msg = match e {
                    Exit::Gone => "disconnected".into(),
                    Exit::Error(m) => m,
                    _ => String::new(),
                };
            }
            Pick::Exit(Exit::Error(m)) => die(&m),
            _ => return,
        }
    }
}

// ---- the menu / chooser ----------------------------------------------------------------------

fn hostname() -> String {
    let mut b = [0u8; 256];
    if unsafe { libc::gethostname(b.as_mut_ptr().cast(), b.len()) } == 0 {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..n]).into_owned()
    } else {
        "?".into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

/// One key from the start of `rest`, and how many bytes it took.
fn key_at(rest: &[u8]) -> (Key, usize) {
    match rest {
        [0x1b, b'[', b'A', ..] | [0x1b, b'O', b'A', ..] => (Key::Up, 3),
        [0x1b, b'[', b'B', ..] | [0x1b, b'O', b'B', ..] => (Key::Down, 3),
        [0x1b, b'[', b'5', b'~', ..] => (Key::PageUp, 4),
        [0x1b, b'[', b'6', b'~', ..] => (Key::PageDown, 4),
        [0x1b, b'[', b'H', ..] | [0x1b, b'O', b'H', ..] => (Key::Home, 3),
        [0x1b, b'[', b'F', ..] | [0x1b, b'O', b'F', ..] => (Key::End, 3),
        [0x1b, b'[', b'1', b'~', ..] => (Key::Home, 4),
        [0x1b, b'[', b'4', b'~', ..] => (Key::End, 4),
        [0x1b, b'[', ..] => (
            Key::Char(0),
            rest.iter()
                .skip(2)
                .position(|c| c.is_ascii_alphabetic() || *c == b'~')
                .map_or(rest.len(), |p| p + 3),
        ),
        [0x1b, b'O', _, ..] => (Key::Char(0), 3),
        [0x1b, ..] => (Key::Quit, 1),
        [b'\r', ..] | [b'\n', ..] => (Key::Enter, 1),
        [3, ..] | [b'q', ..] | [b'Q', ..] => (Key::Quit, 1),
        [b'k', ..] => (Key::Up, 1),
        [b'j', ..] => (Key::Down, 1),
        [b'g', ..] => (Key::Home, 1),
        [b'G', ..] => (Key::End, 1),
        [c, ..] => (Key::Char(*c), 1),
        [] => (Key::Char(0), 1),
    }
}

enum Pick {
    Port(String, bool),
    Cancel,
    Exit(Exit),
}

/// The port list, full screen. `bg`: the session it was opened from (the chooser); it
/// stays attached, its output kept for later.
fn picker(socket: &str, keys: &Keys, current: Option<&str>, mut bg: Option<&mut Session>, msg: &mut String) -> Pick {
    let in_fd = std::io::stdin().as_raw_fd();
    let host = hostname();
    let mut ports: Vec<PortStatus> = match client::try_status(socket) {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => return Pick::Exit(Exit::Error("no ports configured".into())),
        Err(e) => return Pick::Exit(Exit::Error(e)),
    };
    let find = |ports: &[PortStatus], c: &str| {
        ports
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(c) || (!p.label.is_empty() && p.label.eq_ignore_ascii_case(c)))
    };
    let mut sel = current.and_then(|c| find(&ports, c)).unwrap_or(0);
    let chooser = bg.is_some();
    let p = keys.name();
    let mut top = 0usize;
    let mut jump = String::new();
    let mut jump_at = Instant::now();
    let mut dirty = true;
    let _screen = Screen::enter();
    loop {
        let (rows, cols) = term::size();
        let body = rows.saturating_sub(4).max(1);
        if dirty {
            dirty = false;
            if sel < top {
                top = sel;
            }
            if sel >= top + body {
                top = sel + 1 - body;
            }
            let up = ports.iter().filter(|p| p.up).count();
            let mut f = String::from("\x1b[H\x1b[2J");
            let title = match current {
                Some(c) if chooser => format!(" line on {host}: {} ports, {up} up; on {c}", ports.len()),
                _ => format!(" line on {host}: {} ports, {up} up", ports.len()),
            };
            let keys_help = if chooser {
                "↑↓/jk move  ⏎ attach  w watch  i info  q back "
            } else {
                "↑↓/jk move  ⏎ attach  w watch  i info  q quit "
            };
            let pad = cols.saturating_sub(title.chars().count() + keys_help.chars().count());
            f.push_str(&format!("\x1b[1m{title}{}{keys_help}\x1b[0m\r\n", " ".repeat(pad)));
            f.push_str(&format!(
                "\x1b[2m {}\x1b[0m\r\n",
                HEADER.chars().take(cols.saturating_sub(1)).collect::<String>()
            ));
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
            let hint = if !msg.is_empty() {
                msg.clone()
            } else if chooser {
                "⏎ or w switches to the port (attach / watch); q or Esc goes back".to_string()
            } else {
                format!("attached: {p} d detach, {p} ? keys, {p} s this list")
            };
            f.push_str(&format!(
                "\x1b[{rows};1H\x1b[2m {}\x1b[0m",
                hint.chars().take(cols.saturating_sub(1)).collect::<String>()
            ));
            stdout_write(f.as_bytes());
        }

        let mut fds = [
            libc::pollfd {
                fd: in_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: bg.as_ref().map_or(-1, |s| s.s.as_raw_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, 2000) };
        if n < 0 {
            continue;
        }
        if n == 0 {
            if let Ok(p) = client::try_status(socket) {
                if !p.is_empty() {
                    ports = p; // refresh every 2 s
                    sel = sel.min(ports.len() - 1);
                }
            }
            dirty = true;
            continue;
        }
        if fds[1].revents != 0 {
            if let Some(s) = bg.as_deref_mut() {
                if let Err(e) = s.pump() {
                    return Pick::Exit(e);
                }
            }
        }
        if fds[0].revents == 0 {
            continue;
        }
        let mut buf = [0u8; 64];
        let got = std::io::stdin().lock().read(&mut buf).unwrap_or(0);
        if got == 0 {
            return Pick::Cancel;
        }
        dirty = true;
        let mut i = 0;
        while i < got {
            let (k, used) = key_at(&buf[i..got]);
            i += used;
            match k {
                Key::Up => sel = sel.saturating_sub(1),
                Key::Down => sel = (sel + 1).min(ports.len() - 1),
                Key::PageUp => sel = sel.saturating_sub(body),
                Key::PageDown => sel = (sel + body).min(ports.len() - 1),
                Key::Home => sel = 0,
                Key::End => sel = ports.len() - 1,
                Key::Enter => return Pick::Port(ports[sel].name.clone(), true),
                Key::Char(b'w') | Key::Char(b'W') => return Pick::Port(ports[sel].name.clone(), false),
                Key::Char(b'i') | Key::Char(b'I') => {
                    let p = &ports[sel];
                    *msg = format!(
                        "{}: {} {}, rx {}, tx {}, scrollback {}, idle {}{}",
                        p.name,
                        p.device,
                        if p.up { "open" } else { "absent" },
                        human_bytes(p.rx_bytes),
                        human_bytes(p.tx_bytes),
                        human_bytes(p.scrollback_bytes as u64),
                        human_idle(p.idle_secs),
                        if p.users.is_empty() {
                            String::new()
                        } else {
                            format!(", users: {}", p.users.join(", "))
                        }
                    );
                }
                Key::Char(c) if c.is_ascii_digit() => {
                    // type a port number to jump to it
                    if jump_at.elapsed() > Duration::from_millis(1500) {
                        jump.clear();
                    }
                    jump.push(c as char);
                    jump_at = Instant::now();
                    if let Some(i) = ports
                        .iter()
                        .position(|p| p.name.trim_start_matches(|c: char| !c.is_ascii_digit()) == jump)
                    {
                        sel = i;
                    }
                }
                Key::Quit => return Pick::Cancel,
                Key::Char(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_from_bytes() {
        assert_eq!(key_at(b"\x1b[5~x"), (Key::PageUp, 4));
        assert_eq!(key_at(b"\x1bOA"), (Key::Up, 3));
        assert_eq!(key_at(b"\x1b"), (Key::Quit, 1));
        assert_eq!(key_at(b"\x1b[1;5C"), (Key::Char(0), 6));
        assert_eq!(key_at(b"G"), (Key::End, 1));
    }

    #[test]
    fn scrollback_lines_wrap_and_expand_tabs() {
        let l = view_lines(b"\x1b[1mab\x1b[0m\tc\r\n0123456789012345678901234\r\nlast", 20);
        assert_eq!(l, vec!["ab      c", "01234567890123456789", "01234", "last"]);
        assert_eq!(view_lines(b"x\r\n\r\ny\r\n", 80), vec!["x", "", "y"]);
    }

    #[test]
    fn help_names_the_prefix() {
        let h = help_text(&Keys { prefix: 0x01 });
        assert!(h.starts_with("keys: press C-a (or ^])"));
        assert!(h.contains("C-a  a literal C-a"));
        assert!(help_text(&Keys { prefix: ALIAS }).starts_with("keys: press C-]"));
    }
}

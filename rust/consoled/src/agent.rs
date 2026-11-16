//! The non-interactive commands, for agents and scripts: send, expect, run.
//!
//! Exit codes: 0 done, 1 timed out (expect, run), 2 refused (someone else has write;
//! --take takes it over), 3 anything else (no such port, device absent, consoled gone).
//! Write that send or run gets is theirs only for that command: it ends with the
//! connection, and whoever is attached is told write is free again.

use crate::client::{self, who};
use crate::pattern::Pattern;
use crate::proto::{self, Hello, PortState};
use crate::text::{self, Cleaner};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub const OK: i32 = 0;
pub const TIMEOUT: i32 = 1;
pub const REFUSED: i32 = 2;
pub const FAILED: i32 = 3;

/// The daemon queues at most 64 KiB per port; more would be dropped.
const SEND_MAX: usize = 32 << 10;
/// A match at the end of a line that is still being written (a prompt) counts once nothing
/// more has arrived for this long.
const SETTLE: Duration = Duration::from_millis(200);

pub fn fail(msg: &str) -> ! {
    eprintln!("line: {msg}");
    std::process::exit(FAILED);
}

enum Next {
    Frame(u8, Vec<u8>),
    Timeout,
    Closed,
}

struct Conn {
    s: UnixStream,
    dec: proto::Decoder,
    state: PortState,
}

impl Conn {
    /// Attach (no scrollback) and wait for the port's state.
    fn open(socket: &str, port: &str, write: bool, take: bool, what: &str) -> Conn {
        let who = format!("{} [{what}]", who());
        let h = Hello {
            port: Some(port.into()),
            write,
            take,
            who,
            replay: false,
            ..Hello::default()
        };
        let s = client::try_hello(socket, &h).unwrap_or_else(|e| fail(&e));
        let mut c = Conn {
            s,
            dec: proto::Decoder::default(),
            state: PortState::default(),
        };
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match c.next(until) {
                Next::Frame(proto::STATE, b) => {
                    c.state = serde_json::from_slice(&b).unwrap_or_else(|e| fail(&format!("state: {e}")));
                    return c;
                }
                Next::Frame(proto::ERROR, m) => fail(&String::from_utf8_lossy(&m)),
                Next::Frame(..) => {}
                Next::Timeout => fail("no port state from consoled (is it older than this line?)"),
                Next::Closed => fail("consoled closed the connection"),
            }
        }
    }

    fn next(&mut self, until: Instant) -> Next {
        let mut buf = [0u8; 65536];
        loop {
            match self.dec.next() {
                Ok(Some((k, p))) => return Next::Frame(k, p),
                Ok(None) => {}
                Err(e) => fail(&format!("protocol: {e}")),
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Next::Timeout;
            }
            let mut pfd = libc::pollfd {
                fd: self.s.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = left.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
            if unsafe { libc::poll(&mut pfd, 1, ms) } <= 0 {
                continue; // timeout (checked above) or EINTR
            }
            match self.s.read(&mut buf) {
                Ok(0) => return Next::Closed,
                Ok(n) => self.dec.push(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Next::Closed,
            }
        }
    }

    fn data(&mut self, b: &[u8]) {
        let mut frames = Vec::new();
        for chunk in b.chunks(16 << 10) {
            proto::encode(proto::DATA, chunk, &mut frames);
        }
        if let Err(e) = self.s.write_all(&frames) {
            fail(&format!("consoled: {e}"));
        }
    }
}

/// Attach asking for write (taking it over with `take`); exit 2 if someone else keeps it.
fn writer(socket: &str, port: &str, take: bool, what: &str) -> Conn {
    let c = Conn::open(socket, port, true, take, what);
    if !c.state.write {
        let holder = c.state.writer.as_deref().unwrap_or("someone");
        eprintln!(
            "line: {}: refused: {holder} has write (--take takes it over)",
            c.state.port
        );
        std::process::exit(REFUSED);
    }
    if !c.state.up {
        fail(&format!("{}: the device is not present", c.state.port));
    }
    c
}

/// Text as typed: each newline becomes Enter (\r), and Enter at the end.
pub fn typed(t: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(t.len() + 1);
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            b'\r' if t.get(i + 1) == Some(&b'\n') => {
                out.push(b'\r');
                i += 1;
            }
            b'\n' => out.push(b'\r'),
            c => out.push(c),
        }
        i += 1;
    }
    if out.last() != Some(&b'\r') {
        out.push(b'\r');
    }
    out
}

pub fn send(socket: &str, port: &str, take: bool, newline: bool, text: &[u8]) -> i32 {
    let bytes = if newline { typed(text) } else { text.to_vec() };
    if bytes.len() > SEND_MAX {
        fail(&format!(
            "{} bytes is more than send takes at once ({} KiB)",
            bytes.len(),
            SEND_MAX >> 10
        ));
    }
    let mut c = writer(socket, port, take, "send");
    c.data(&bytes);
    // closing gives write up; the daemon reads what was sent before it sees the close
    OK
}

/// Output as it arrives, cleaned; where the pattern was looked for so far.
struct Watch {
    c: Cleaner,
    /// Start of the first line not yet checked.
    checked: usize,
    /// `run`: the command whose echo is still to be dropped.
    echo: Option<String>,
    /// Where the output proper starts (after the echo).
    body: usize,
    /// `run`: only the last line, still without a newline, can be the prompt.
    prompt: bool,
}

struct Found {
    /// The matching line: its start and end in `Watch::c.out`.
    start: usize,
    end: usize,
    /// It is the last line, still without a newline.
    partial: bool,
}

impl Watch {
    fn new(echo: Option<String>, prompt: bool) -> Watch {
        Watch {
            c: Cleaner::default(),
            checked: 0,
            echo,
            body: 0,
            prompt,
        }
    }

    fn scan(&mut self, p: &Pattern) -> Option<Found> {
        if let Some(cmd) = &self.echo {
            let rest = text::drop_echo(&self.c.out, cmd).len();
            if rest < self.c.out.len() {
                self.body = self.c.out.len() - rest;
                self.checked = self.body;
                self.echo = None;
            } else if self.c.out.len() > cmd.len() + 2048 {
                self.echo = None; // no echo (echo off, or it scrolled past): look everywhere
            }
        }
        let out = &self.c.out;
        if self.prompt {
            // a prompt is the last thing printed: only the unfinished last line counts
            self.checked = out.rfind('\n').map_or(0, |n| n + 1).max(self.body);
        }
        let mut pos = self.checked;
        loop {
            match out[pos..].find('\n') {
                Some(n) => {
                    if p.is_match(&text::render_line(&out[pos..pos + n])) {
                        return Some(Found {
                            start: pos,
                            end: pos + n,
                            partial: false,
                        });
                    }
                    pos += n + 1;
                    self.checked = pos;
                }
                None => {
                    let hit = p.is_match(&text::render_line(&out[pos..]));
                    return hit.then_some(Found {
                        start: pos,
                        end: out.len(),
                        partial: true,
                    });
                }
            }
        }
    }
}

enum Why {
    Timeout,
    Closed,
}

fn wait(c: &mut Conn, w: &mut Watch, p: &Pattern, deadline: Instant) -> Result<Found, Why> {
    let mut settle: Option<(Instant, Found)> = None;
    loop {
        let until = settle.as_ref().map_or(deadline, |(t, _)| (*t).min(deadline));
        match c.next(until) {
            Next::Frame(proto::DATA, d) => {
                w.c.push(&d);
                settle = None;
                if let Some(f) = w.scan(p) {
                    if f.partial && (w.prompt || p.anchored_end()) {
                        settle = Some((Instant::now() + SETTLE, f));
                    } else {
                        return Ok(f);
                    }
                }
            }
            Next::Frame(proto::ERROR, m) => fail(&String::from_utf8_lossy(&m)),
            Next::Frame(proto::STATE, b) => {
                if let Ok(st) = serde_json::from_slice(&b) {
                    c.state = st;
                }
            }
            Next::Frame(..) => {}
            Next::Timeout => return settle.map(|(_, f)| f).ok_or(Why::Timeout),
            Next::Closed => return Err(Why::Closed),
        }
    }
}

fn print_lines(s: &str) {
    let lines = text::lines(s);
    let mut o = std::io::stdout().lock();
    for (i, l) in lines.iter().enumerate() {
        if i + 1 == lines.len() && l.is_empty() {
            break;
        }
        let _ = writeln!(o, "{l}");
    }
    let _ = o.flush();
}

fn secs(d: Duration) -> String {
    format!("{}", (d.as_secs_f64() * 10.0).round() / 10.0)
}

/// Wait for new output matching `pattern`; print what came, up to the matching line.
pub fn expect(socket: &str, port: &str, pattern: &str, timeout: Duration) -> i32 {
    let p = Pattern::parse(pattern).unwrap_or_else(|e| fail(&e));
    let mut c = Conn::open(socket, port, false, false, "expect");
    let mut w = Watch::new(None, false);
    match wait(&mut c, &mut w, &p, Instant::now() + timeout) {
        Ok(f) => {
            print_lines(&w.c.out[..f.end]);
            OK
        }
        Err(why) => {
            print_lines(&w.c.out);
            match why {
                Why::Timeout => {
                    eprintln!(
                        "line: {}: nothing matching {pattern:?} within {} s",
                        c.state.port,
                        secs(timeout)
                    );
                    TIMEOUT
                }
                Why::Closed => fail("consoled closed the connection"),
            }
        }
    }
}

/// Type `cmd` and Enter; print the output up to the next prompt, without the echo.
pub fn run(socket: &str, port: &str, take: bool, prompt: &str, timeout: Duration, cmd: &str) -> i32 {
    let p = Pattern::parse(prompt).unwrap_or_else(|e| fail(&e));
    let mut c = writer(socket, port, take, "run");
    let mut w = Watch::new(Some(cmd.to_string()), true);
    c.data(&typed(cmd.as_bytes()));
    match wait(&mut c, &mut w, &p, Instant::now() + timeout) {
        Ok(f) => {
            print_lines(&w.c.out[w.body..f.start]);
            OK
        }
        Err(why) => {
            print_lines(&w.c.out[w.body..]);
            match why {
                Why::Timeout => {
                    eprintln!(
                        "line: {}: no prompt matching {prompt:?} within {} s",
                        c.state.port,
                        secs(timeout)
                    );
                    TIMEOUT
                }
                Why::Closed => fail("consoled closed the connection"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_ends_with_enter() {
        assert_eq!(typed(b"uname -a"), b"uname -a\r");
        assert_eq!(typed(b"a\nb\r\nc\n"), b"a\rb\rc\r");
        assert_eq!(typed(b""), b"\r");
    }

    fn watch(p: &str, echo: Option<&str>, chunks: &[&[u8]]) -> (Option<(String, bool)>, Watch) {
        let p = Pattern::parse(p).unwrap();
        let mut w = Watch::new(echo.map(String::from), echo.is_some());
        let mut last = None;
        for ch in chunks {
            w.c.push(ch);
            last = w
                .scan(&p)
                .map(|f| (text::render_line(&w.c.out[f.start..f.end]), f.partial));
        }
        (last, w)
    }

    #[test]
    fn prompts_are_found_after_the_echo() {
        // run: output lines that look like a prompt are not it; the unfinished last line is
        let (f, w) = watch("[#$>] ?$", Some("echo #"), &[b"echo #", b"\r\n#\r\n> \r\n"]);
        assert_eq!(f, None);
        assert_eq!(&w.c.out[w.body..], "#\r\n> \r\n");
        let (f, _) = watch("[#$>] ?$", Some("echo #"), &[b"echo #", b"\r\n#\r\n", b"root@box:~# "]);
        assert_eq!(f, Some(("root@box:~# ".into(), true)));
        // a line in the middle that looks like a prompt is a complete line: found at once
        let (f, _) = watch("^login: ", None, &[b"\x1b[1mbox\x1b[0m\r\nlogin: "]);
        assert_eq!(f, Some(("login: ".into(), true)));
        let (f, _) = watch("panic", None, &[b"Kernel pa", b"nic - not syncing\r\n"]);
        assert_eq!(f, Some(("Kernel panic - not syncing".into(), false)));
    }
}

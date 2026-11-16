//! console: attach this terminal to a consoled port, or list the ports.
//!
//!   console                 list ports
//!   console P12             attach (asks for write; read-only if someone has it)
//!   console -r P12          attach read-only (watch)
//!   console --metrics       Prometheus text (a node-exporter textfile producer)
//!   console --ssh           sshd ForceCommand: the port comes from $SSH_ORIGINAL_COMMAND
//!                           (`ssh -t console@box P12`, `... watch P12`), the list when empty
//!
//! Escape: ^] then  .  quit   w  take write   r  release write   b  send break
//!                  ?  help   ^] send a literal ^]

use consoled::proto::{self, Cmd, Hello, PortStatus};
use consoled::term::RawMode;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

const DEFAULT_SOCKET: &str = "/run/consoled/console.sock";
const ESC: u8 = 0x1d; // ^]

fn die(msg: &str) -> ! {
    eprintln!("console: {msg}");
    std::process::exit(1);
}

fn who() -> String {
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

fn status(socket: &str) -> Vec<PortStatus> {
    let mut s = connect(socket);
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, &Hello { port: None, write: false, who: who(), replay: false }, &mut out);
    s.write_all(&out).unwrap_or_else(|e| die(&format!("write: {e}")));
    let mut dec = proto::Decoder::default();
    let Some((proto::LIST, body)) = read_frame(&mut s, &mut dec) else { die("no port list from consoled") };
    serde_json::from_slice(&body).unwrap_or_else(|e| die(&format!("list: {e}")))
}

fn metrics(socket: &str) {
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

fn list(socket: &str) {
    let ports = status(socket);
    println!("{:<6} {:<20} {:<16} {:<8} {:>7}  writer", "port", "label", "settings", "device", "clients");
    for p in ports {
        println!(
            "{:<6} {:<20} {:<16} {:<8} {:>7}  {}",
            p.name,
            p.label,
            p.settings,
            if p.up { "up" } else { "absent" },
            p.clients,
            p.writer.unwrap_or_default()
        );
    }
}

fn say(msg: &str) {
    let mut o = std::io::stdout().lock();
    let _ = write!(o, "\r\n[console] {msg}\r\n");
    let _ = o.flush();
}

const HELP: &str = "escape ^] then: . quit, w take write, r release write, b send break, ? help, ^] a literal ^]";

fn attach(socket: &str, port: &str, write: bool) {
    let mut s = connect(socket);
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, &Hello { port: Some(port.into()), write, who: who(), replay: true }, &mut out);
    s.write_all(&out).unwrap_or_else(|e| die(&format!("write: {e}")));
    let stdin = std::io::stdin();
    let in_fd = stdin.as_raw_fd();
    let raw = RawMode::enable(in_fd).unwrap_or_else(|e| die(&format!("terminal: {e}")));
    if raw.is_none() {
        eprintln!("console: stdin is not a terminal (use `ssh -t`); input is passed through as is");
    }
    let mut dec = proto::Decoder::default();
    let mut esc = false;
    let mut stdin_open = true;
    let mut buf = [0u8; 65536];
    let stdout = std::io::stdout();
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
                    drop(raw);
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
                        let mut o = stdout.lock();
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
                            drop(raw);
                            say("detached");
                            return;
                        }
                        b'w' | b'W' => proto::encode_json(proto::CMD, &Cmd::Take, &mut frames),
                        b'r' | b'R' => proto::encode_json(proto::CMD, &Cmd::Release, &mut frames),
                        b'b' | b'B' => proto::encode_json(proto::CMD, &Cmd::Break, &mut frames),
                        ESC => data.push(ESC),
                        _ => say(HELP),
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
    drop(raw);
}

fn main() {
    // `console | head`: exit quietly on a closed pipe, like other CLI tools
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let mut socket = std::env::var("CONSOLED_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.into());
    let mut write = true;
    let mut port: Option<String> = None;
    let mut ssh = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-r" | "--read-only" => write = false,
            "--socket" => socket = args.next().unwrap_or_else(|| die("--socket needs a path")),
            "--ssh" => ssh = true,
            "--metrics" => return metrics(&socket),
            "-h" | "--help" => {
                println!("usage: console [-r] [PORT]   (no PORT: list)\n       {HELP}");
                return;
            }
            _ if a.starts_with('-') => die(&format!("unknown option {a:?}")),
            _ => port = Some(a),
        }
    }
    if ssh {
        // `ssh -t console@box P12` / `ssh -t console@box watch P12` (read-only; ssh itself
        // would take a bare `-r`)
        let cmd = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
        let mut words = cmd.split_whitespace();
        for w in words.by_ref() {
            match w {
                "-r" | "--read-only" | "watch" | "ro" => write = false,
                "list" => break,
                _ => {
                    port = Some(w.to_string());
                    break;
                }
            }
        }
        if port.is_none() {
            list(&socket);
            println!("\nattach: ssh -t console@<box> <port>   (read-only: ssh -t console@<box> watch <port>)");
            return;
        }
    }
    match port.as_deref() {
        None | Some("list") => list(&socket),
        Some(p) => attach(&socket, p, write),
    }
}

//! The client side: what `line` does (list, info, log, metrics, and what they share);
//! attach, watch and the menu are in `session`, send/expect/run in `agent`.

use crate::proto::{self, Hello, PortStatus};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;

pub const DEFAULT_SOCKET: &str = "/run/consoled/console.sock";

pub fn die(msg: &str) -> ! {
    crate::term::restore();
    eprintln!("line: {msg}");
    std::process::exit(1);
}

pub fn who() -> String {
    let user = std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| format!("uid{}", unsafe { libc::getuid() }));
    match std::env::var("SSH_CONNECTION")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(String::from))
    {
        Some(ip) => format!("{user}@{ip}"),
        None => user,
    }
}

pub fn try_hello(socket: &str, h: &Hello) -> Result<UnixStream, String> {
    let mut s = UnixStream::connect(socket).map_err(|e| format!("{socket}: {e} (is consoled running?)"))?;
    let mut out = Vec::new();
    proto::encode_json(proto::HELLO, h, &mut out);
    s.write_all(&out).map_err(|e| format!("write: {e}"))?;
    Ok(s)
}

pub fn hello(socket: &str, h: &Hello) -> UnixStream {
    try_hello(socket, h).unwrap_or_else(|e| die(&e))
}

pub fn read_frame(s: &mut UnixStream, dec: &mut proto::Decoder) -> Option<(u8, Vec<u8>)> {
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

pub fn try_status(socket: &str) -> Result<Vec<PortStatus>, String> {
    let mut s = try_hello(
        socket,
        &Hello {
            port: None,
            who: who(),
            replay: false,
            ..Hello::default()
        },
    )?;
    let mut dec = proto::Decoder::default();
    match read_frame(&mut s, &mut dec) {
        Some((proto::LIST, body)) => serde_json::from_slice(&body).map_err(|e| format!("list: {e}")),
        _ => Err("no port list from consoled".into()),
    }
}

pub fn status(socket: &str) -> Vec<PortStatus> {
    try_status(socket).unwrap_or_else(|e| die(&e))
}

fn find<'a>(ports: &'a [PortStatus], key: &str) -> &'a PortStatus {
    let k = key.to_ascii_lowercase();
    ports
        .iter()
        .find(|p| p.name.to_ascii_lowercase() == k)
        .or_else(|| {
            ports
                .iter()
                .find(|p| !p.label.is_empty() && p.label.to_ascii_lowercase() == k)
        })
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

pub fn row(p: &PortStatus, width: usize) -> String {
    let dev = if p.up { "up" } else { "absent" };
    let users = match (p.users.len(), &p.writer) {
        (0, _) => String::new(),
        (1, Some(w)) => format!("{w} (w)"),
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

pub const HEADER: &str = "PORT  LABEL            SETTINGS       DEVICE        RX  IDLE  USERS";

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
    let label = if p.label.is_empty() {
        String::new()
    } else {
        format!(" ({})", p.label)
    };
    println!("{}{label}", p.name);
    println!(
        "  device     {} ({})",
        p.device,
        if p.up { "open" } else { "absent, retrying every 2 s" }
    );
    println!("  settings   {}", p.settings);
    if p.users.is_empty() {
        println!("  attached   nobody");
    } else {
        for (i, u) in p.users.iter().enumerate() {
            let role = if i == 0 && p.writer.as_deref() == Some(u.as_str()) {
                "write"
            } else {
                "watching"
            };
            println!("  {:<10} {u} ({role})", if i == 0 { "attached" } else { "" });
        }
    }
    println!(
        "  received   {} (scrollback {})",
        human_bytes(p.rx_bytes),
        human_bytes(p.scrollback_bytes as u64)
    );
    println!("  sent       {}", human_bytes(p.tx_bytes));
    println!(
        "  last rx    {}",
        match p.idle_secs {
            None => "nothing since consoled started".into(),
            Some(s) => format!("{} ago", human_idle(Some(s))),
        }
    );
}

/// The scrollback, raw, to stdout; with `follow`, then what the port says after it
/// (attached as a watcher until the daemon goes away or the reader does).
pub fn log(socket: &str, key: &str, follow: bool) {
    let who = if follow { format!("{} [log -f]", who()) } else { who() };
    let mut s = hello(
        socket,
        &Hello {
            port: Some(key.into()),
            who,
            replay: true,
            dump: !follow,
            ..Hello::default()
        },
    );
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
    let lbl = |p: &PortStatus| {
        format!(
            "port=\"{}\",label=\"{}\"",
            p.name,
            p.label.replace(['"', '\\', '\n'], "")
        )
    };
    let mut s = String::new();
    let mut metric = |name: &str, kind: &str, help: &str, v: &dyn Fn(&PortStatus) -> u64| {
        s.push_str(&format!(
            "# HELP consoled_{name} {help}\n# TYPE consoled_{name} {kind}\n"
        ));
        for p in &ports {
            s.push_str(&format!("consoled_{name}{{{}}} {}\n", lbl(p), v(p)));
        }
    };
    metric("port_up", "gauge", "1 while the serial device is open", &|p| {
        p.up as u64
    });
    metric("port_clients", "gauge", "attached clients", &|p| p.clients as u64);
    metric("port_writer", "gauge", "1 while a client holds write", &|p| {
        p.writer.is_some() as u64
    });
    metric("port_rx_bytes_total", "counter", "bytes read from the device", &|p| {
        p.rx_bytes
    });
    metric("port_tx_bytes_total", "counter", "bytes written to the device", &|p| {
        p.tx_bytes
    });
    print!("{s}");
}

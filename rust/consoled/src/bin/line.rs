//! line: use consoled's serial ports.
//!
//!   line                  menu (full screen): ↑↓ pick, ⏎ attach, w watch, i info, q quit
//!   line list [--json]    the ports
//!   line info PORT        one port in detail
//!   line watch PORT       attach read-only
//!   line attach PORT      attach (write if nobody has it); `line PORT` is the same
//!   line log PORT         print the scrollback and exit
//!   line metrics          Prometheus text (a node-exporter textfile producer)
//!   line ssh              sshd ForceCommand: the words come from $SSH_ORIGINAL_COMMAND
//!                         (`ssh -t console@box watch P12`; nothing: the menu)
//! PORT is a name (P12) or a label (cs8210). While attached: ^] then . detach, w take
//! write, r release write, b send break, ? help.

use consoled::client::{self, DEFAULT_SOCKET, ESCAPES};

const USAGE: &str = "usage: line [list [--json] | info PORT | watch PORT | attach PORT | log PORT | PORT]";

fn run(socket: &str, args: &[String]) {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let port = |i: usize| -> &str { a.get(i).copied().unwrap_or_else(|| client::die(USAGE)) };
    match a.as_slice() {
        [] | ["menu"] => client::menu(socket),
        ["list", rest @ ..] => client::list(socket, rest.contains(&"--json")),
        ["info", ..] => client::info(socket, port(1)),
        ["watch", ..] | ["-r", ..] | ["ro", ..] => client::attach(socket, port(1), false),
        ["attach", ..] => client::attach(socket, port(1), true),
        ["log", ..] => client::log(socket, port(1)),
        ["metrics"] => client::metrics(socket),
        ["help"] | ["-h"] | ["--help"] => println!("{USAGE}\nwhile attached: {ESCAPES}"),
        [p] if !p.starts_with('-') => client::attach(socket, p, true),
        _ => client::die(USAGE),
    }
}

fn main() {
    // `line list | head`: exit quietly on a closed pipe, like other CLI tools
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let mut socket = std::env::var("CONSOLED_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.into());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() >= 2 && args[0] == "--socket" {
        socket = args[1].clone();
        args.drain(..2);
    }
    if args.first().map(String::as_str) == Some("ssh") {
        // only these words, never a shell: the ssh user can't run anything else
        let cmd = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
        let words: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        if matches!(words.first().map(String::as_str), Some("ssh" | "metrics")) {
            client::die(USAGE);
        }
        return run(&socket, &words);
    }
    run(&socket, &args);
}

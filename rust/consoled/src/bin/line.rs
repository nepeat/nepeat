//! line: use consoled's serial ports (see `line help` and README.md).
//!
//! The same words work locally and over ssh: `line ssh` is the sshd ForceCommand and
//! reads them from $SSH_ORIGINAL_COMMAND (split with shell-like quoting, never run by a
//! shell; `ssh`, `metrics` and `--socket` are refused there).

use consoled::cli::{self, Command, Text, USAGE};
use consoled::client::{self, DEFAULT_SOCKET, die};
use consoled::session::{self, Keys};
use consoled::{agent, term};
use std::io::Read;

fn main() {
    // `line list | head`: exit quietly on a closed pipe, like other CLI tools
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut inv = cli::parse(&args, false).unwrap_or_else(|e| die(&e));
    if inv.cmd == Command::Ssh {
        // only these words, never a shell: the ssh user can't run anything else
        let cmd = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
        let words = cli::split_words(&cmd).unwrap_or_else(|e| die(&e));
        let remote = cli::parse(&words, true).unwrap_or_else(|e| die(&e));
        inv = cli::Invocation {
            socket: inv.socket,
            prefix: remote.prefix.or(inv.prefix),
            cmd: remote.cmd,
        };
    }
    let socket = inv
        .socket
        .or_else(|| std::env::var("CONSOLED_SOCKET").ok())
        .unwrap_or_else(|| DEFAULT_SOCKET.into());
    let prefix = inv
        .prefix
        .or_else(|| std::env::var("CONSOLED_PREFIX").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "C-b".into());
    let keys = Keys {
        prefix: cli::parse_key(&prefix).unwrap_or_else(|e| die(&e)),
    };
    let code = match inv.cmd {
        Command::Menu => {
            session::menu(&socket, &keys);
            0
        }
        Command::List { json } => {
            client::list(&socket, json);
            0
        }
        Command::Info(p) => {
            client::info(&socket, &p);
            0
        }
        Command::Attach { port, write } => {
            session::attach(&socket, &port, write, &keys);
            0
        }
        Command::Log { port, follow } => {
            client::log(&socket, &port, follow);
            0
        }
        Command::Metrics => {
            client::metrics(&socket);
            0
        }
        Command::Help => {
            println!("{USAGE}\n\nwhile attached, {}", session::help_text(&keys));
            0
        }
        Command::Send {
            port,
            take,
            newline,
            text,
        } => {
            let bytes = match text {
                Text::Arg(s) => s.into_bytes(),
                Text::Stdin => {
                    let mut b = Vec::new();
                    std::io::stdin()
                        .read_to_end(&mut b)
                        .unwrap_or_else(|e| agent::fail(&format!("stdin: {e}")));
                    b
                }
            };
            agent::send(&socket, &port, take, newline, &bytes)
        }
        Command::Expect { port, pattern, timeout } => agent::expect(&socket, &port, &pattern, timeout),
        Command::Run {
            port,
            take,
            prompt,
            timeout,
            cmd,
        } => agent::run(&socket, &port, take, &prompt, timeout, &cmd),
        Command::Ssh => die(USAGE), // `line ssh` over ssh: refused by the parser already
    };
    term::restore();
    std::process::exit(code);
}

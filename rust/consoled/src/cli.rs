//! Command line parsing for `line`: the same grammar for argv and for `line ssh`, where
//! the words come from `$SSH_ORIGINAL_COMMAND` (split here with shell-like quoting, never
//! by a shell: nothing is expanded or run).

use crate::pattern::Pattern;
use std::time::Duration;

pub const USAGE: &str = "usage: line [--prefix KEY] [COMMAND]
  (nothing)              menu: pick a port, attach or watch it
  list [--json]          the ports: label, settings, device, rx, idle, who is attached
  info PORT              one port in detail
  attach PORT, PORT      attach (write if nobody has it)
  watch PORT             attach read-only
  log PORT [--follow]    print the scrollback (and with --follow, -f: what comes after)
  send PORT [--take] [--no-newline] TEXT|-
                         type TEXT (- reads stdin) and Enter; exit 2 if someone else has write
  expect PORT PATTERN [--timeout S]
                         wait for new output matching PATTERN (exit 0), or time out (exit 1)
  run PORT [--take] [--prompt PATTERN] [--timeout S] CMD
                         type CMD, print its output up to the next prompt (0 ok, 1 timeout, 2 refused)
  metrics                Prometheus text (not over ssh)
PORT is a name (P12) or a label (cs8210). --prefix (or $CONSOLED_PREFIX) sets the key that
starts a command while attached: C-b by default, ^] always works too. Options of send and
run go before the text; send, expect and run exit 3 on other errors (no such port, ...).";

/// Command words that are not a port name.
const COMMANDS: &[&str] = &["list", "info", "log", "metrics", "ssh", "help", "send", "expect", "run"];

/// Default `--prompt` of `run`: a shell, U-Boot or switch prompt at the end of the output.
pub const DEFAULT_PROMPT: &str = "[#$>] ?$";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    Arg(String),
    Stdin,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Menu,
    List {
        json: bool,
    },
    Info(String),
    Attach {
        port: String,
        write: bool,
    },
    Log {
        port: String,
        follow: bool,
    },
    Metrics,
    Help,
    Send {
        port: String,
        take: bool,
        newline: bool,
        text: Text,
    },
    Expect {
        port: String,
        pattern: String,
        timeout: Duration,
    },
    Run {
        port: String,
        take: bool,
        prompt: String,
        timeout: Duration,
        cmd: String,
    },
    /// `line ssh`: parse `$SSH_ORIGINAL_COMMAND` next.
    Ssh,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub socket: Option<String>,
    pub prefix: Option<String>,
    pub cmd: Command,
}

/// Words the way a shell would split them, without expanding anything: whitespace
/// separates, '...' is literal, "..." is literal except \" \\ \$ and \`, a backslash
/// outside quotes makes the next character literal.
pub fn split_words(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut w = String::new();
    let mut in_word = false;
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut w));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match cs.next() {
                        Some('\'') => break,
                        Some(c) => w.push(c),
                        None => return Err("unbalanced ' in the command".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match cs.next() {
                        Some('"') => break,
                        Some('\\') => match cs.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => w.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                w.push('\\');
                                w.push(c);
                            }
                            None => return Err("unbalanced \" in the command".into()),
                        },
                        Some(c) => w.push(c),
                        None => return Err("unbalanced \" in the command".into()),
                    }
                }
            }
            '\\' => match cs.next() {
                Some('\n') => {}
                Some(c) => {
                    in_word = true;
                    w.push(c);
                }
                None => return Err("the command ends with \\".into()),
            },
            c => {
                in_word = true;
                w.push(c);
            }
        }
    }
    if in_word {
        words.push(w);
    }
    Ok(words)
}

/// "C-a", "c-a", "^A", "ctrl-a" to the byte the key sends (C-a: 0x01).
pub fn parse_key(s: &str) -> Result<u8, String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("c-")
        .or_else(|| lower.strip_prefix("ctrl-"))
        .or_else(|| t.strip_prefix('^'));
    let bad = || format!("prefix {s:?}: use C-a .. C-z, C-\\, C-], C-^ or C-_ (also written ^A)");
    let Some(rest) = rest else { return Err(bad()) };
    let mut it = rest.chars();
    let (Some(c), None) = (it.next(), it.next()) else {
        return Err(bad());
    };
    let b = match c.to_ascii_uppercase() {
        c @ ('A'..='Z' | '\\' | ']' | '^' | '_') => c as u8 & 0x1f,
        _ => return Err(bad()),
    };
    match b {
        0x08 | 0x09 | 0x0a | 0x0d => Err(format!(
            "prefix {s:?}: that key is Backspace, Tab or Enter on most terminals"
        )),
        _ => Ok(b),
    }
}

/// The byte a prefix key sends, as a name ("C-b").
pub fn key_name(b: u8) -> String {
    match b {
        1..=26 => format!("C-{}", (b'a' + b - 1) as char),
        0x1c..=0x1f => format!("C-{}", (b + 0x40) as char),
        _ => format!("0x{b:02x}"),
    }
}

fn timeout(v: Option<&String>) -> Result<Duration, String> {
    let v = v.ok_or("--timeout needs seconds")?;
    match v.parse::<f64>() {
        Ok(s) if s > 0.0 && s <= 86400.0 => Ok(Duration::from_secs_f64(s)),
        _ => Err(format!("--timeout {v:?}: seconds, more than 0 (at most a day)")),
    }
}

/// The text after the options: one word, or several joined by spaces (`line run P1 ls -l`).
fn rest_text(rest: &[String], what: &str) -> Result<String, String> {
    if rest.is_empty() {
        return Err(format!("{what} is missing\n{USAGE}"));
    }
    Ok(rest.join(" "))
}

/// Parse the words after `line` (`ssh`: they came over ssh, so no `--socket`, no
/// `metrics`, no `ssh`).
pub fn parse(words: &[String], ssh: bool) -> Result<Invocation, String> {
    let mut inv = Invocation {
        socket: None,
        prefix: None,
        cmd: Command::Menu,
    };
    let mut i = 0;
    while let Some(w) = words.get(i) {
        if let Some(v) = w.strip_prefix("--prefix=") {
            inv.prefix = Some(v.to_string());
            i += 1;
        } else if w == "--prefix" {
            inv.prefix = Some(words.get(i + 1).ok_or("--prefix needs a key (C-a)")?.clone());
            i += 2;
        } else if w == "--socket" && !ssh {
            inv.socket = Some(words.get(i + 1).ok_or("--socket needs a path")?.clone());
            i += 2;
        } else {
            break;
        }
    }
    // attach, watch and the menu also take --prefix after their words: `ssh host --prefix`
    // would be ssh's own option, `ssh host attach P1 --prefix C-a` is not
    let mut words: Vec<String> = words[i..].to_vec();
    let interactive = match words.first().map(String::as_str) {
        Some("menu" | "attach" | "watch" | "ro" | "-r") => true,
        Some(w) => !w.starts_with('-') && !COMMANDS.contains(&w),
        None => false,
    };
    let mut j = 1;
    while interactive && j < words.len() {
        if words[j] == "--prefix" {
            inv.prefix = Some(words.get(j + 1).ok_or("--prefix needs a key (C-a)")?.clone());
            words.drain(j..j + 2);
        } else if let Some(v) = words[j].strip_prefix("--prefix=") {
            inv.prefix = Some(v.to_string());
            words.remove(j);
        } else {
            j += 1;
        }
    }
    if let Some(p) = &inv.prefix {
        parse_key(p)?;
    }
    let a: Vec<&str> = words.iter().map(String::as_str).collect();
    let port = |j: usize| -> Result<String, String> {
        match a.get(j) {
            Some(p) if !p.starts_with('-') => Ok(p.to_string()),
            _ => Err(USAGE.to_string()),
        }
    };
    let owned: Vec<String> = a.iter().map(|s| s.to_string()).collect();
    inv.cmd = match a.as_slice() {
        [] | ["menu"] => Command::Menu,
        ["list", rest @ ..] => Command::List {
            json: rest.contains(&"--json"),
        },
        ["info", ..] => Command::Info(port(1)?),
        ["watch", ..] | ["-r", ..] | ["ro", ..] => Command::Attach {
            port: port(1)?,
            write: false,
        },
        ["attach", ..] => Command::Attach {
            port: port(1)?,
            write: true,
        },
        ["log", ..] => {
            let mut follow = false;
            for w in &a[2.min(a.len())..] {
                match *w {
                    "--follow" | "-f" => follow = true,
                    w => return Err(format!("log: unknown argument {w:?}")),
                }
            }
            Command::Log { port: port(1)?, follow }
        }
        ["metrics"] if !ssh => Command::Metrics,
        ["ssh"] if !ssh => Command::Ssh,
        ["help"] | ["-h"] | ["--help"] => Command::Help,
        ["send", ..] => {
            let p = port(1)?;
            let (mut take, mut newline) = (false, true);
            let mut j = 2;
            while let Some(w) = a.get(j) {
                match *w {
                    "--take" => take = true,
                    "--no-newline" | "-n" => newline = false,
                    "--" => {
                        j += 1;
                        break;
                    }
                    w if w.starts_with("--") => return Err(format!("send: unknown option {w:?}")),
                    _ => break,
                }
                j += 1;
            }
            let rest = &owned[j.min(owned.len())..];
            let text = if rest == ["-"] {
                Text::Stdin
            } else {
                Text::Arg(rest_text(rest, "send: TEXT (or - for stdin)")?)
            };
            Command::Send {
                port: p,
                take,
                newline,
                text,
            }
        }
        ["expect", ..] => {
            let p = port(1)?;
            let mut t = DEFAULT_TIMEOUT;
            let mut pat = Vec::new();
            let mut j = 2;
            while let Some(w) = owned.get(j) {
                match w.as_str() {
                    "--timeout" | "-t" => {
                        t = timeout(owned.get(j + 1))?;
                        j += 1;
                    }
                    w if w.starts_with("--timeout=") => t = timeout(Some(&w["--timeout=".len()..].to_string()))?,
                    "--" => {
                        pat.extend_from_slice(&owned[j + 1..]);
                        break;
                    }
                    w if w.starts_with("--") => return Err(format!("expect: unknown option {w:?}")),
                    _ => pat.push(w.clone()),
                }
                j += 1;
            }
            let pattern = rest_text(&pat, "expect: PATTERN")?;
            Pattern::parse(&pattern)?;
            Command::Expect {
                port: p,
                pattern,
                timeout: t,
            }
        }
        ["run", ..] => {
            let p = port(1)?;
            let (mut take, mut prompt, mut t) = (false, DEFAULT_PROMPT.to_string(), DEFAULT_TIMEOUT);
            let mut j = 2;
            while let Some(w) = owned.get(j) {
                match w.as_str() {
                    "--take" => take = true,
                    "--prompt" => {
                        prompt = owned.get(j + 1).ok_or("--prompt needs a pattern")?.clone();
                        j += 1;
                    }
                    w if w.starts_with("--prompt=") => prompt = w["--prompt=".len()..].to_string(),
                    "--timeout" | "-t" => {
                        t = timeout(owned.get(j + 1))?;
                        j += 1;
                    }
                    w if w.starts_with("--timeout=") => t = timeout(Some(&w["--timeout=".len()..].to_string()))?,
                    "--" => {
                        j += 1;
                        break;
                    }
                    w if w.starts_with("--") => return Err(format!("run: unknown option {w:?}")),
                    _ => break,
                }
                j += 1;
            }
            Pattern::parse(&prompt)?;
            let cmd = rest_text(&owned[j.min(owned.len())..], "run: CMD")?;
            Command::Run {
                port: p,
                take,
                prompt,
                timeout: t,
                cmd,
            }
        }
        [p] if !p.starts_with('-') && !(ssh && matches!(*p, "ssh" | "metrics")) => Command::Attach {
            port: p.to_string(),
            write: true,
        },
        _ => return Err(USAGE.to_string()),
    };
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        split_words(s).unwrap()
    }

    fn cmd(s: &str, ssh: bool) -> Result<Command, String> {
        parse(&w(s), ssh).map(|i| i.cmd)
    }

    #[test]
    fn words_split_like_a_shell_without_one() {
        assert_eq!(
            w("run cs8210 --prompt '# $' 'uname -a'"),
            ["run", "cs8210", "--prompt", "# $", "uname -a"]
        );
        assert_eq!(
            w(r#"send P1 "echo \"\$HOME\" \x" a\ b ''"#),
            ["send", "P1", "echo \"$HOME\" \\x", "a b", ""]
        );
        assert_eq!(w("  watch   P12  "), ["watch", "P12"]);
        assert_eq!(
            w("$(reboot) `id` ;|&"),
            ["$(reboot)", "`id`", ";|&"],
            "nothing is expanded or run"
        );
        assert!(split_words("send P1 'oops").is_err());
        assert!(split_words("send P1 \"oops").is_err());
    }

    #[test]
    fn commands() {
        assert_eq!(cmd("", true).unwrap(), Command::Menu);
        assert_eq!(
            cmd("cs8210", true).unwrap(),
            Command::Attach {
                port: "cs8210".into(),
                write: true
            }
        );
        assert_eq!(
            cmd("log P1 -f", true).unwrap(),
            Command::Log {
                port: "P1".into(),
                follow: true
            }
        );
        assert_eq!(
            cmd("send P1 --take -n hello world", true).unwrap(),
            Command::Send {
                port: "P1".into(),
                take: true,
                newline: false,
                text: Text::Arg("hello world".into())
            }
        );
        assert_eq!(
            cmd("send P1 -", true).unwrap(),
            Command::Send {
                port: "P1".into(),
                take: false,
                newline: true,
                text: Text::Stdin
            }
        );
        assert_eq!(
            cmd("send P1 ''", true).unwrap(),
            Command::Send {
                port: "P1".into(),
                take: false,
                newline: true,
                text: Text::Arg("".into())
            }
        );
        assert_eq!(
            cmd("send P1 -- --take", true).unwrap(),
            Command::Send {
                port: "P1".into(),
                take: false,
                newline: true,
                text: Text::Arg("--take".into())
            }
        );
        assert_eq!(
            cmd("expect P1 'login: $' --timeout 2.5", true).unwrap(),
            Command::Expect {
                port: "P1".into(),
                pattern: "login: $".into(),
                timeout: Duration::from_millis(2500)
            }
        );
        assert_eq!(
            cmd("run cs8210 --prompt '# $' 'uname -a'", true).unwrap(),
            Command::Run {
                port: "cs8210".into(),
                take: false,
                prompt: "# $".into(),
                timeout: DEFAULT_TIMEOUT,
                cmd: "uname -a".into()
            }
        );
        // options go before CMD; after it they are part of it
        assert_eq!(
            cmd("run P1 --take ls --timeout 3", true).unwrap(),
            Command::Run {
                port: "P1".into(),
                take: true,
                prompt: DEFAULT_PROMPT.into(),
                timeout: DEFAULT_TIMEOUT,
                cmd: "ls --timeout 3".into()
            }
        );
        for s in ["watch P2 --prefix C-a", "watch --prefix=C-a P2"] {
            let inv = parse(&w(s), true).unwrap();
            assert_eq!(
                (inv.prefix.as_deref(), inv.cmd),
                (
                    Some("C-a"),
                    Command::Attach {
                        port: "P2".into(),
                        write: false
                    }
                ),
                "{s}"
            );
        }
        let inv = parse(&w("cs8210 --prefix ^a"), true).unwrap();
        assert_eq!(
            (inv.prefix.as_deref(), inv.cmd),
            (
                Some("^a"),
                Command::Attach {
                    port: "cs8210".into(),
                    write: true
                }
            )
        );
        // not for send/run: there it is text
        let inv = parse(&w("send P1 echo --prefix x"), true).unwrap();
        assert_eq!(inv.prefix, None);
        let inv = parse(&w("--prefix C-a watch P2"), true).unwrap();
        assert_eq!(inv.prefix.as_deref(), Some("C-a"));
        assert_eq!(
            inv.cmd,
            Command::Attach {
                port: "P2".into(),
                write: false
            }
        );
    }

    #[test]
    fn refused_or_wrong() {
        for bad in [
            "ssh",
            "metrics",
            "--socket /tmp/x list",
            "send P1",
            "send",
            "run P1",
            "expect P1",
            "expect P1 '[x'",
            "run P1 --timeout 0 ls",
            "run P1 --bogus ls",
            "--prefix C-m",
            "-x",
            "log P1 --bogus",
        ] {
            assert!(cmd(bad, true).is_err(), "{bad}");
        }
        assert_eq!(cmd("metrics", false).unwrap(), Command::Metrics);
        assert_eq!(
            parse(&w("--socket /tmp/x list"), false).unwrap().socket.as_deref(),
            Some("/tmp/x")
        );
    }

    #[test]
    fn prefix_keys() {
        assert_eq!(parse_key("C-b"), Ok(0x02));
        assert_eq!(parse_key("c-A"), Ok(0x01));
        assert_eq!(parse_key("^a"), Ok(0x01));
        assert_eq!(parse_key("ctrl-]"), Ok(0x1d));
        assert_eq!(parse_key("C-\\"), Ok(0x1c));
        for bad in ["C-m", "C-h", "C-i", "C-j", "C-[", "b", "C-bb", "", "C-1"] {
            assert!(parse_key(bad).is_err(), "{bad}");
        }
        assert_eq!(key_name(0x02), "C-b");
        assert_eq!(key_name(0x1d), "C-]");
        assert_eq!(key_name(parse_key("C-_").unwrap()), "C-_");
    }
}

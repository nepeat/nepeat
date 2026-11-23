//! The command line: the options of the C tool, parsed as GNU `getopt_long` parses them
//! (options and scripts in any order, `--` ends the options, unique prefixes of long options,
//! `--name=value` or `--name value`, clustered short options).

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// The name in every message (the C tool's; the status files and the units match on it).
pub const ME: &str = "openbcm-mdio-script";
/// `--hint` may be given this many times.
pub const MAX_HINTS: usize = 16;
/// Scripts per run (`MDIO_STATUS_MAX_SCRIPTS`).
pub const MAX_SCRIPTS: usize = 8;

/// The usage text.
pub fn usage(err: &mut dyn Write) {
    let _ = write!(
        err,
        "usage: {ME} --check SCRIPT...\n       {ME} --config CONFIG.BCM [--unit N] [--status-file FILE.json] [--names FILE]\n              \
         [--hint CLASS=TEXT]... [--temp-out FILE [--temp-prom FILE] [--temp-interval S]] SCRIPT...\n       \
         {ME} --verify-only (--diag-socket SOCKET | --config CONFIG.BCM) [--names FILE] [--json] SCRIPT...\n"
    );
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Options {
    /// `--check`: parse only.
    pub check: bool,
    /// `--config`: the SDK config file.
    pub config: Option<OsString>,
    /// `--unit`.
    pub unit: i32,
    /// `--verify-only`.
    pub verify: bool,
    /// `--diag-socket`.
    pub diag_socket: Option<OsString>,
    /// `--names`.
    pub names: Option<OsString>,
    /// `--json`.
    pub json: bool,
    /// `--status-file`.
    pub status_file: Option<OsString>,
    /// `--hint CLASS=TEXT`, in order.
    pub hints: Vec<(Vec<u8>, Vec<u8>)>,
    /// `--temp-out`.
    pub temp_out: Option<OsString>,
    /// `--temp-prom`.
    pub temp_prom: Option<OsString>,
    /// `--temp-interval`, seconds.
    pub temp_interval: i32,
    /// The scripts.
    pub scripts: Vec<OsString>,
}

/// The command line is not one to act on: the usage text was printed, exit with this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit(pub u8);

/// (long name, short letter, takes an argument)
const OPTIONS: &[(&str, u8, bool)] = &[
    ("check", b'c', false),
    ("config", b'f', true),
    ("unit", b'u', true),
    ("verify-only", b'v', false),
    ("diag-socket", b'd', true),
    ("names", b'n', true),
    ("json", b'j', false),
    ("status-file", b's', true),
    ("hint", b'H', true),
    ("temp-out", b'o', true),
    ("temp-prom", b'p', true),
    ("temp-interval", b'i', true),
    ("help", b'h', false),
];

/// C `atoi`: optional white space and sign, then digits; anything else ends the number.
/// Saturates where C's behaviour is undefined.
pub fn atoi(text: &[u8]) -> i32 {
    let start = text
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .unwrap_or(text.len());
    let text = text.get(start..).unwrap_or_default();
    let (negative, digits) = match text.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, text),
    };
    let mut value = 0i64;
    for d in digits.iter().map_while(|b| char::from(*b).to_digit(10)) {
        value = value.saturating_mul(10).saturating_add(i64::from(d));
    }
    let value = if negative { value.saturating_neg() } else { value };
    i32::try_from(value).unwrap_or(if negative { i32::MIN } else { i32::MAX })
}

struct Parser<'a> {
    program: &'a OsStr,
    err: &'a mut dyn Write,
    opts: Options,
}

impl Parser<'_> {
    fn complain(&mut self, what: &str) {
        let _ = self.err.write_all(self.program.as_bytes());
        let _ = writeln!(self.err, ": {what}");
    }

    /// One option with its argument, as the C `switch` does. `Some`: stop with usage.
    fn apply(&mut self, letter: u8, arg: Option<&[u8]>) -> Option<Exit> {
        let arg = arg.unwrap_or_default();
        let os = || Some(OsString::from_vec(arg.to_vec()));
        match letter {
            b'c' => self.opts.check = true,
            b'f' => self.opts.config = os(),
            b'u' => self.opts.unit = atoi(arg),
            b'v' => self.opts.verify = true,
            b'd' => self.opts.diag_socket = os(),
            b'n' => self.opts.names = os(),
            b'j' => self.opts.json = true,
            b's' => self.opts.status_file = os(),
            b'H' => {
                let eq = arg.iter().position(|&b| b == b'=');
                let (Some(eq), true) = (eq, self.opts.hints.len() < MAX_HINTS) else {
                    return Some(Exit(1));
                };
                let (class, text) = arg.split_at_checked(eq).unwrap_or((arg, &[]));
                self.opts
                    .hints
                    .push((class.to_vec(), text.get(1..).unwrap_or_default().to_vec()));
            }
            b'o' => self.opts.temp_out = os(),
            b'p' => self.opts.temp_prom = os(),
            b'i' => self.opts.temp_interval = atoi(arg),
            b'h' => return Some(Exit(0)),
            _ => return Some(Exit(1)),
        }
        None
    }

    /// `--name` or `--name=value` (without the dashes). `next`: the following argument.
    /// Returns whether `next` was used.
    fn long(&mut self, word: &[u8], next: Option<&[u8]>) -> Result<bool, Exit> {
        let (name, value) = match word.iter().position(|&b| b == b'=') {
            Some(eq) => (word.get(..eq).unwrap_or(word), word.get(eq.saturating_add(1)..)),
            None => (word, None),
        };
        let shown = String::from_utf8_lossy(name).into_owned();
        let exact = OPTIONS.iter().find(|o| o.0.as_bytes() == name);
        let matches: Vec<_> = OPTIONS.iter().filter(|o| o.0.as_bytes().starts_with(name)).collect();
        let &(long, letter, takes) = match (exact, matches.as_slice()) {
            (Some(o), _) | (None, &[o]) => o,
            (None, []) => {
                self.complain(&format!("unrecognized option '--{shown}'"));
                return Err(Exit(1));
            }
            (None, several) => {
                let all: Vec<String> = several.iter().map(|o| format!("'--{}'", o.0)).collect();
                self.complain(&format!(
                    "option '--{shown}' is ambiguous; possibilities: {}",
                    all.join(" ")
                ));
                return Err(Exit(1));
            }
        };
        let (arg, used_next) = match (takes, value) {
            (false, Some(_)) => {
                self.complain(&format!("option '--{long}' doesn't allow an argument"));
                return Err(Exit(1));
            }
            (false, None) => (None, false),
            (true, Some(v)) => (Some(v), false),
            (true, None) => {
                let Some(v) = next else {
                    self.complain(&format!("option '--{long}' requires an argument"));
                    return Err(Exit(1));
                };
                (Some(v), true)
            }
        };
        self.apply(letter, arg).map_or(Ok(used_next), Err)
    }

    /// `-abc` (without the dash). Returns whether `next` was used.
    fn short(&mut self, word: &[u8], next: Option<&[u8]>) -> Result<bool, Exit> {
        let mut rest = word;
        while let Some((&letter, tail)) = rest.split_first() {
            rest = tail;
            let Some(&(_, _, takes)) = OPTIONS.iter().find(|o| o.1 == letter) else {
                self.complain(&format!("invalid option -- '{}'", char::from(letter)));
                return Err(Exit(1));
            };
            if !takes {
                if let Some(exit) = self.apply(letter, None) {
                    return Err(exit);
                }
                continue;
            }
            let (arg, used_next) = match (rest.is_empty(), next) {
                (false, _) => (rest, false),
                (true, Some(v)) => (v, true),
                (true, None) => {
                    self.complain(&format!("option requires an argument -- '{}'", char::from(letter)));
                    return Err(Exit(1));
                }
            };
            return self.apply(letter, Some(arg)).map_or(Ok(used_next), Err);
        }
        Ok(false)
    }
}

/// Parses the arguments after the program name. On `Err` the usage text (and for a bad option
/// a line saying which) is on `err`.
pub fn parse(program: &OsStr, args: &[OsString], err: &mut dyn Write) -> Result<Options, Exit> {
    let mut p = Parser {
        program,
        err,
        opts: Options {
            temp_interval: 5,
            ..Options::default()
        },
    };
    let mut scripts = Vec::new();
    let mut i = 0usize;
    let mut failed = None;
    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        let word = arg.as_bytes();
        let next = args.get(i).map(|a| a.as_bytes());
        let used = match word {
            b"--" => {
                scripts.extend(args.get(i..).unwrap_or_default().iter().cloned());
                break;
            }
            [b'-', b'-', long @ ..] => p.long(long, next),
            [b'-', short @ ..] if !short.is_empty() => p.short(short, next),
            _ => {
                scripts.push(arg.clone());
                Ok(false)
            }
        };
        match used {
            Ok(true) => i = i.saturating_add(1),
            Ok(false) => {}
            Err(exit) => {
                failed = Some(exit);
                break;
            }
        }
    }
    let mut o = p.opts;
    o.scripts = scripts;
    let exit = failed.or_else(|| {
        let bad = o.scripts.is_empty()
            || o.scripts.len() > MAX_SCRIPTS
            || !(1..=3600).contains(&o.temp_interval)
            || (o.check && o.verify)
            || (!o.check && o.config.is_none() && !(o.verify && o.diag_socket.is_some()))
            || (o.diag_socket.is_some() && !o.verify)
            || (o.temp_prom.is_some() && o.temp_out.is_none());
        bad.then_some(Exit(1))
    });
    match exit {
        Some(exit) => {
            usage(err);
            Err(exit)
        }
        None => Ok(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> (Result<Options, Exit>, String) {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let mut err = Vec::new();
        let r = parse(OsStr::new("prog"), &args, &mut err);
        (r, String::from_utf8(err).unwrap())
    }

    #[test]
    fn the_units_command_line() {
        let (o, err) = run(&[
            "--config",
            "/etc/config.bcm",
            "--status-file",
            "/run/x/status.json",
            "--names",
            "/n",
            "--hint",
            "phy-not-answering=cold power cycle (plug 1)",
            "--hint=script-missing=a=b",
            "--temp-out",
            "/run/asic/temperature",
            "--temp-prom",
            "/p",
            "--temp-interval",
            "7",
            "--unit",
            "1",
            "/a.mdio",
            "/b.mdio",
        ]);
        let o = o.unwrap();
        assert_eq!(err, "");
        assert_eq!(o.config.as_deref(), Some(OsStr::new("/etc/config.bcm")));
        assert_eq!(o.status_file.as_deref(), Some(OsStr::new("/run/x/status.json")));
        assert_eq!(
            o.hints,
            [
                (b"phy-not-answering".to_vec(), b"cold power cycle (plug 1)".to_vec()),
                (b"script-missing".to_vec(), b"a=b".to_vec())
            ]
        );
        assert_eq!((o.unit, o.temp_interval), (1, 7));
        assert_eq!(o.scripts, ["/a.mdio", "/b.mdio"]);
        assert!(!o.check && !o.verify && !o.json);
    }

    #[test]
    fn getopt_forms() {
        // scripts between options, prefixes, short options clustered and with attached values
        let o = run(&["a.mdio", "--verify", "--diag=/s", "-jn", "/names", "b.mdio"])
            .0
            .unwrap();
        assert!(o.verify && o.json);
        assert_eq!(o.diag_socket.as_deref(), Some(OsStr::new("/s")));
        assert_eq!(o.names.as_deref(), Some(OsStr::new("/names")));
        assert_eq!(o.scripts, ["a.mdio", "b.mdio"]);
        let o = run(&["-cu3", "--", "--config", "-x"]).0.unwrap();
        assert!(o.check);
        assert_eq!(o.unit, 3);
        assert_eq!(o.scripts, ["--config", "-x"]);
        // an option's argument may look like an option; "-" is a script
        let o = run(&["--config", "--check", "-"]).0.unwrap();
        assert_eq!(o.config.as_deref(), Some(OsStr::new("--check")));
        assert_eq!(o.scripts, ["-"]);
        assert_eq!(run(&["--check", "--unit", "x7", "s"]).0.unwrap().unit, 0);
    }

    #[test]
    fn refusals_print_usage() {
        for (args, code, first) in [
            (&["--help"][..], 0, "usage: "),
            (&["-h", "--bogus"], 0, "usage: "),
            (&["--bogus", "-h"], 1, "prog: unrecognized option '--bogus'\n"),
            (&["-x"], 1, "prog: invalid option -- 'x'\n"),
            (&["--config"], 1, "prog: option '--config' requires an argument\n"),
            (&["-f"], 1, "prog: option requires an argument -- 'f'\n"),
            (
                &["--check=1", "s"],
                1,
                "prog: option '--check' doesn't allow an argument\n",
            ),
            (
                &["--te", "x", "s"],
                1,
                "prog: option '--te' is ambiguous; possibilities: '--temp-out' '--temp-prom' '--temp-interval'\n",
            ),
            (&["--check"], 1, "usage: "),
            (&["s"], 1, "usage: "),
            (&["--check", "--verify-only", "s"], 1, "usage: "),
            (&["--verify-only", "s"], 1, "usage: "),
            (&["--config", "c", "--diag-socket", "d", "s"], 1, "usage: "),
            (&["--config", "c", "--temp-prom", "p", "s"], 1, "usage: "),
            (&["--config", "c", "--temp-interval", "0", "s"], 1, "usage: "),
            (&["--config", "c", "--temp-interval", "3601", "s"], 1, "usage: "),
            (&["--config", "c", "--hint", "no-equals", "s"], 1, "usage: "),
            (&["--check", "1", "2", "3", "4", "5", "6", "7", "8", "9"], 1, "usage: "),
        ] {
            let (r, err) = run(args);
            assert_eq!(r, Err(Exit(code)), "{args:?}");
            assert!(err.starts_with(first), "{args:?}: {err}");
            assert!(err.contains("usage: openbcm-mdio-script --check SCRIPT...\n"), "{err}");
        }
        let mut many = vec!["--check", "s"];
        for _ in 0..17 {
            many.extend(["--hint", "a=b"]);
        }
        assert_eq!(run(&many).0, Err(Exit(1)));
        assert_eq!(run(&many[..many.len() - 2]).0.unwrap().hints.len(), 16);
    }

    #[test]
    fn atoi_is_c() {
        assert_eq!(atoi(b"12"), 12);
        assert_eq!(atoi(b"  -7x"), -7);
        assert_eq!(atoi(b"+3"), 3);
        assert_eq!(atoi(b"x"), 0);
        assert_eq!(atoi(b""), 0);
        assert_eq!(atoi(b"99999999999"), i32::MAX);
    }
}

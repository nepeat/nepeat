//! The command line of `asic_temp.c`, parsed as GNU `getopt_long` parses it (options in any
//! order between other words, which are ignored; `--` ends the options; unique prefixes of long
//! options; `--name=value` or `--name value`; clustered short options), with C's `atoi` for
//! the numbers.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// `--unit` (0).
    pub unit: i32,
    /// `--interval`, seconds between reads (5; 1..=3600).
    pub interval: i32,
    /// `--out`: the plain file (the hottest monitor).
    pub out: Option<OsString>,
    /// `--prom`: the Prometheus textfile.
    pub prom: Option<OsString>,
    /// `--shutdown-at`, degrees C (125; 0 = off; at most 150).
    pub shutdown_at: i32,
    /// `--once`: print the monitors and exit.
    pub once: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            unit: 0,
            interval: 5,
            out: None,
            prom: None,
            shutdown_at: 125,
            once: false,
        }
    }
}

/// The command line is not one to act on: the usage text was printed, exit with this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit(pub u8);

/// (long name, short letter, takes an argument), in the order of the C `opts[]` (the order
/// `getopt_long` lists the candidates of an ambiguous prefix in).
const OPTIONS: &[(&str, u8, bool)] = &[
    ("unit", b'u', true),
    ("interval", b'i', true),
    ("out", b'o', true),
    ("prom", b'p', true),
    ("shutdown-at", b's', true),
    ("once", b'1', false),
    ("help", b'h', false),
];

/// The usage text (C `usage(argv[0])`).
pub fn usage(err: &mut dyn Write, program: &OsStr) {
    let _ = err.write_all(b"usage: ");
    let _ = err.write_all(program.as_bytes());
    let _ = err.write_all(
        b" [--unit N] (--once | --out FILE [--prom FILE] [--interval SECONDS] \
          [--shutdown-at CELSIUS (0 = off, default 125)])\n",
    );
}

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
    /// `getopt`'s own message (`argv[0]: ...`).
    fn complain(&mut self, what: &str) {
        let _ = self.err.write_all(self.program.as_bytes());
        let _ = writeln!(self.err, ": {what}");
    }

    /// One option with its argument, as the C `switch` does. `Some`: stop with usage.
    fn apply(&mut self, letter: u8, arg: Option<&[u8]>) -> Option<Exit> {
        let arg = arg.unwrap_or_default();
        let os = || Some(OsString::from_vec(arg.to_vec()));
        match letter {
            b'u' => self.opts.unit = atoi(arg),
            b'i' => self.opts.interval = atoi(arg),
            b'o' => self.opts.out = os(),
            b'p' => self.opts.prom = os(),
            b's' => self.opts.shutdown_at = atoi(arg),
            b'1' => self.opts.once = true,
            b'h' => return Some(Exit(0)),
            _ => return Some(Exit(1)),
        }
        None
    }

    /// `--name` or `--name=value` (without the dashes); `next`: the following word. Returns
    /// whether `next` was used.
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

/// Parses the arguments after the program name. On `Err` the usage text (after `getopt`'s line
/// for a bad option) is on `err`.
pub fn parse(program: &OsStr, args: &[OsString], err: &mut dyn Write) -> Result<Options, Exit> {
    let mut p = Parser {
        program,
        err,
        opts: Options::default(),
    };
    let mut i = 0usize;
    let mut failed = None;
    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        let next = args.get(i).map(|a| a.as_bytes());
        let used = match arg.as_bytes() {
            b"--" => break,
            [b'-', b'-', long @ ..] => p.long(long, next),
            [b'-', short @ ..] if !short.is_empty() => p.short(short, next),
            _ => Ok(false), // not an option: asic_temp.c never looks at the rest
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
    let o = p.opts;
    let exit = failed.or_else(|| {
        let bad =
            (!o.once && o.out.is_none()) || !(1..=3600).contains(&o.interval) || !(0..=150).contains(&o.shutdown_at);
        bad.then_some(Exit(1))
    });
    match exit {
        Some(exit) => {
            usage(err, program);
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

    const USAGE: &str = "usage: prog [--unit N] (--once | --out FILE [--prom FILE] [--interval SECONDS] \
                         [--shutdown-at CELSIUS (0 = off, default 125)])\n";

    #[test]
    fn the_units_command_line() {
        let (o, err) = run(&[
            "--interval",
            "5",
            "--out",
            "/run/asic/temperature",
            "--prom",
            "/run/prometheus-textfile/asic.prom",
        ]);
        assert_eq!(err, "");
        assert_eq!(
            o.unwrap(),
            Options {
                out: Some("/run/asic/temperature".into()),
                prom: Some("/run/prometheus-textfile/asic.prom".into()),
                ..Options::default()
            }
        );
    }

    #[test]
    fn getopt_forms() {
        let o = run(&["x", "-1u3", "--shut=0", "--interval=7", "-o/f", "y", "--", "--bogus"])
            .0
            .unwrap();
        assert_eq!(
            o,
            Options {
                unit: 3,
                interval: 7,
                out: Some("/f".into()),
                prom: None,
                shutdown_at: 0,
                once: true,
            }
        );
        let o = run(&["--on", "--u", "x7", "-s", "-5x"]).0;
        assert_eq!(o, Err(Exit(1)), "atoi(-5x) = -5 is refused");
        let o = run(&["--on", "--u", "x7", "-s", "150", "--p", "--out"]).0.unwrap();
        assert_eq!(
            (o.unit, o.shutdown_at, o.prom.as_deref()),
            (0, 150, Some(OsStr::new("--out")))
        );
        assert!(o.out.is_none() && o.once);
    }

    #[test]
    fn refusals_print_usage() {
        for (args, code, first) in [
            (&["--help"][..], 0, "usage: "),
            (&["-h", "--bogus"], 0, "usage: "),
            (&["--bogus", "-h"], 1, "prog: unrecognized option '--bogus'\n"),
            (&["-x"], 1, "prog: invalid option -- 'x'\n"),
            (&["--out"], 1, "prog: option '--out' requires an argument\n"),
            (&["-u"], 1, "prog: option requires an argument -- 'u'\n"),
            (&["--once=1"], 1, "prog: option '--once' doesn't allow an argument\n"),
            (
                &["--o", "x"],
                1,
                "prog: option '--o' is ambiguous; possibilities: '--out' '--once'\n",
            ),
            (&[], 1, "usage: "),
            (&["--prom", "p"], 1, "usage: "),
            (&["--once", "--interval", "0"], 1, "usage: "),
            (&["--once", "--interval", "3601"], 1, "usage: "),
            (&["--once", "--shutdown-at", "-1"], 1, "usage: "),
            (&["--once", "--shutdown-at", "151"], 1, "usage: "),
        ] {
            let (r, err) = run(args);
            assert_eq!(r, Err(Exit(code)), "{args:?}");
            assert!(err.starts_with(first), "{args:?}: {err}");
            assert!(err.ends_with(USAGE), "{args:?}: {err}");
        }
        assert!(run(&["--once", "--interval", "3600", "--shutdown-at", "150"]).0.is_ok());
        assert!(run(&["--once", "--interval", "1", "--shutdown-at", "0"]).0.is_ok());
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

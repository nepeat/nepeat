//! What a request is: one command line, refused or not, and which command it names. Pure
//! functions over the received bytes (`sai_diag.cpp` `serve`, `refused`, `raw_mdio`).

/// Reading a request stops once this much arrived without a line feed (C++ `cmd.size() < 1024`).
pub const READ_LIMIT: usize = 1024;

/// A command line must be shorter than this (C++ `cmd.size() >= 1000` is refused).
pub const LINE_LIMIT: usize = 1000;

/// First words that would end syncd's SDK session or the process (compared in lower case).
pub const REFUSED_WORDS: [&str; 14] = [
    "exit", "quit", "init", "deinit", "reset", "detach", "attach", "reload", "rcload", "shell", "sh", "!", "warmboot",
    "dbm",
];

/// Why a request is answered with an error and not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection<'a> {
    /// Empty, 1000 characters or longer, or (Rust only) not UTF-8 text without NUL bytes.
    NotOneLine,
    /// A refused first word, a `!` command or a `;` (the line as received).
    Refused(&'a [u8]),
}

impl Rejection<'_> {
    /// The answer the client gets.
    pub fn message(&self) -> Vec<u8> {
        match self {
            Self::NotOneLine => b"openbcm-diag: send one command line (< 1000 characters)\n".to_vec(),
            Self::Refused(line) => {
                let mut m = b"openbcm-diag: refused (would end or re-init the SDK, or chains commands): ".to_vec();
                m.extend_from_slice(line);
                m.push(b'\n');
                m
            }
        }
    }
}

/// `extmacsec run PORT SEQUENCE [PARAMETER FILE]`, split as C++ `istringstream >>` splits it: a
/// missing word is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacsecRun<'a> {
    /// The table's port name (may be empty).
    pub port: &'a str,
    /// The sequence name (may be empty).
    pub sequence: &'a str,
    /// A root-owned file of key parameters.
    pub param_file: Option<&'a str>,
}

/// Whether a shell command is raw MDIO (`phy raw ...`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdioAccess {
    /// `phy raw ...`: MDIO frames as typed; the external PHY table's polls pause around it.
    Raw,
    /// Any other diag shell command.
    Other,
}

/// A command for the backend's diag shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellCommand<'a> {
    /// The line as it is run.
    pub line: &'a str,
    /// `phy raw ...` or not.
    pub mdio: MdioAccess,
}

/// A command line that is not refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// `extphy`
    ExtPhy,
    /// `extmacsec`
    ExtMacsec,
    /// `extmacsec trace` or `extmacsec trace ARGS`: the text after `extmacsec trace`.
    ExtMacsecTrace(&'a str),
    /// `extmacsec run ...`
    ExtMacsecRun(MacsecRun<'a>),
    /// Everything else.
    Shell(ShellCommand<'a>),
}

/// C `isspace` in the C locale (Rust's `is_ascii_whitespace` leaves out the vertical tab).
fn c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// The words of `text` as C++ `istringstream >> word` reads them.
pub fn c_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(c_space).filter(|w| !w.is_empty())
}

/// The command line of a request (C++ `serve`): up to the first line feed, without trailing
/// carriage returns and spaces and without leading spaces (tabs stay).
pub fn line(request: &[u8]) -> &[u8] {
    let mut line = request.split(|&b| b == b'\n').next().unwrap_or_default();
    while let [rest @ .., b'\r' | b' '] = line {
        line = rest;
    }
    while let [b' ', rest @ ..] = line {
        line = rest;
    }
    line
}

/// The line would end or re-initialise the SDK, or chains commands (C++ `refused`). The first
/// word ends at a space, a tab or a `;`, so a line that starts with a tab has an empty first
/// word and is not refused (as in the C++).
pub fn refused(line: &[u8]) -> bool {
    if line.contains(&b';') {
        return true;
    }
    let word: Vec<u8> = line
        .iter()
        .take_while(|&&b| !matches!(b, b' ' | b'\t' | b';'))
        .map(u8::to_ascii_lowercase)
        .collect();
    REFUSED_WORDS.iter().any(|bad| bad.as_bytes() == word.as_slice()) || word.first() == Some(&b'!')
}

/// `phy raw ...` (C++ `raw_mdio`): the first two words, split at spaces and tabs, in lower case.
fn raw_mdio(line: &str) -> bool {
    let mut words = line.split([' ', '\t']).filter(|w| !w.is_empty());
    let mut next = || words.next().map(str::to_ascii_lowercase);
    next().as_deref() == Some("phy") && next().as_deref() == Some("raw")
}

/// What a request asks for, in the C++ order of checks: one line of 1..1000 characters, not
/// refused, then our own commands, then the diag shell.
pub fn parse(request: &[u8]) -> Result<Command<'_>, Rejection<'_>> {
    let line = line(request);
    if line.is_empty() || line.len() >= LINE_LIMIT {
        return Err(Rejection::NotOneLine);
    }
    if refused(line) {
        return Err(Rejection::Refused(line));
    }
    // Rust only: the backend's shell takes text, and a NUL would end the SDK's C string early.
    let text = std::str::from_utf8(line).map_err(|_| Rejection::NotOneLine)?;
    if text.contains('\0') {
        return Err(Rejection::NotOneLine);
    }
    Ok(classify(text))
}

fn classify(line: &str) -> Command<'_> {
    if line == "extphy" {
        return Command::ExtPhy;
    }
    if line == "extmacsec" {
        return Command::ExtMacsec;
    }
    if let Some(args) = line.strip_prefix("extmacsec trace") {
        if args.is_empty() || args.starts_with(' ') {
            return Command::ExtMacsecTrace(args);
        }
    }
    if let Some(args) = line.strip_prefix("extmacsec run ") {
        let mut words = c_words(args);
        let port = words.next().unwrap_or_default();
        let sequence = words.next().unwrap_or_default();
        let param_file = words.next();
        return Command::ExtMacsecRun(MacsecRun {
            port,
            sequence,
            param_file,
        });
    }
    Command::Shell(ShellCommand {
        line,
        mdio: if raw_mdio(line) {
            MdioAccess::Raw
        } else {
            MdioAccess::Other
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(line: &str) -> Command<'_> {
        Command::Shell(ShellCommand {
            line,
            mdio: MdioAccess::Other,
        })
    }

    #[test]
    fn lines_are_cut_and_trimmed_as_in_the_cpp() {
        assert_eq!(parse(b"l2 station show\n"), Ok(shell("l2 station show")));
        assert_eq!(parse(b"  l3 intf show  \r\n"), Ok(shell("l3 intf show")));
        assert_eq!(parse(b"ps\nexit\n"), Ok(shell("ps")));
        assert_eq!(parse(b"ps"), Ok(shell("ps")));
        assert_eq!(parse(b"ps \r \r"), Ok(shell("ps")));
        assert_eq!(parse(b"\tps\t"), Ok(shell("\tps\t")));
        for empty in [&b""[..], b"\n", b"   \r\n", b"\r", b"\nps"] {
            assert_eq!(parse(empty), Err(Rejection::NotOneLine), "{empty:?}");
        }
        let long = "x".repeat(999);
        assert_eq!(parse(long.as_bytes()), Ok(shell(&long)));
        assert_eq!(parse("x".repeat(1000).as_bytes()), Err(Rejection::NotOneLine));
        assert_eq!(parse(b"ps \xff"), Err(Rejection::NotOneLine));
        assert_eq!(parse(b"ps\0x"), Err(Rejection::NotOneLine));
    }

    #[test]
    fn the_refusal_list() {
        for word in REFUSED_WORDS {
            let line = format!("{word} 1");
            assert_eq!(parse(line.as_bytes()), Err(Rejection::Refused(line.as_bytes())));
            assert!(refused(word.to_uppercase().as_bytes()));
        }
        for bad in [
            "exit", "Quit", "init all", "rcload x", "!ls", "!", "ps; exit", ";", "ps;", "SH", "dbm\tx",
        ] {
            assert!(refused(bad.as_bytes()), "{bad}");
        }
        for fine in [
            "exits",
            "shx",
            "ps",
            "phy raw c45 1 1 1",
            "l2 show !",
            "init_x",
            "\texit",
            "\0exit",
        ] {
            assert!(!refused(fine.as_bytes()), "{fine}");
        }
        assert_eq!(
            String::from_utf8(Rejection::Refused(b"ps; exit").message()).unwrap(),
            "openbcm-diag: refused (would end or re-init the SDK, or chains commands): ps; exit\n"
        );
    }

    #[test]
    fn own_commands() {
        assert_eq!(parse(b"extphy"), Ok(Command::ExtPhy));
        assert_eq!(parse(b"extmacsec \n"), Ok(Command::ExtMacsec));
        assert_eq!(parse(b"EXTPHY"), Ok(shell("EXTPHY")));
        assert_eq!(parse(b"extphy x"), Ok(shell("extphy x")));
        assert_eq!(parse(b"extmacsec trace"), Ok(Command::ExtMacsecTrace("")));
        assert_eq!(parse(b"extmacsec trace on all"), Ok(Command::ExtMacsecTrace(" on all")));
        assert_eq!(parse(b"extmacsec traces"), Ok(shell("extmacsec traces")));
        assert_eq!(parse(b"extmacsec run"), Ok(shell("extmacsec run")));
        assert_eq!(parse(b"extmacsec  run a b"), Ok(shell("extmacsec  run a b")));
        assert_eq!(
            parse(b"extmacsec run ce0 sa.install.egress /run/k extra"),
            Ok(Command::ExtMacsecRun(MacsecRun {
                port: "ce0",
                sequence: "sa.install.egress",
                param_file: Some("/run/k"),
            }))
        );
        assert_eq!(
            parse(b"extmacsec run \x0b ce1"),
            Ok(Command::ExtMacsecRun(MacsecRun {
                port: "ce1",
                sequence: "",
                param_file: None,
            }))
        );
        assert_eq!(
            parse(b"extmacsec run \t"),
            Ok(Command::ExtMacsecRun(MacsecRun {
                port: "",
                sequence: "",
                param_file: None,
            }))
        );
    }

    #[test]
    fn raw_mdio_is_the_first_two_words() {
        let raw = |l: &'static str| {
            Ok(Command::Shell(ShellCommand {
                line: l,
                mdio: MdioAccess::Raw,
            }))
        };
        assert_eq!(parse(b"phy raw c45 0x10 1 0x8b00"), raw("phy raw c45 0x10 1 0x8b00"));
        assert_eq!(parse(b"PHY\t  Raw"), raw("PHY\t  Raw"));
        assert_eq!(parse(b"\tphy raw"), raw("\tphy raw"));
        assert_eq!(parse(b"phy rawx"), Ok(shell("phy rawx")));
        assert_eq!(parse(b"phy control"), Ok(shell("phy control")));
        assert_eq!(parse(b"phyraw"), Ok(shell("phyraw")));
    }
}

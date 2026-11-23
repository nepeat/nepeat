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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// `extphy`
    ExtPhy,
    /// `extmacsec`
    ExtMacsec,
    /// `extmacsec trace` or `extmacsec trace ARGS`: the text after `extmacsec trace`.
    ExtMacsecTrace(&'a str),
    /// `extmacsec run ...`
    ExtMacsecRun(MacsecRun<'a>),
    /// `extphy txn ...`: a diag transaction on external PHYs (Rust only; [`Txn`]).
    Txn {
        /// The line as received (for the log).
        line: &'a str,
        /// What it asks for.
        txn: Txn<'a>,
    },
    /// Everything else.
    Shell(ShellCommand<'a>),
}

/// What an `extphy txn` line asks for (Rust only; the C++ had no transactions). A transaction
/// leases PHYs across connections: `begin` answers its number, every raw access of the client
/// carries it (`extphy txn <n> phy raw ...`), `end` gives the PHYs back. See `sai-extphy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Txn<'a> {
    /// `extphy txn begin <phy[,phy...]|all> [ms]`: `phys` `None` = all PHYs.
    Begin {
        /// The MDIO address words (sorted, no doubles), or all.
        phys: Option<Vec<u16>>,
        /// The lease after each access, ms (the library's default when absent).
        ms: Option<u64>,
    },
    /// `extphy txn renew <n> [ms]`
    Renew {
        /// The transaction.
        txn: u64,
        /// A new lease per access, ms.
        ms: Option<u64>,
    },
    /// `extphy txn end <n>`
    End {
        /// The transaction.
        txn: u64,
    },
    /// `extphy txn` or `extphy txn list`: the leases.
    List,
    /// `extphy txn <n> phy raw ...`: one raw access of transaction `n`.
    Raw(RawRequest<'a>),
    /// Anything else after `extphy txn`: the usage line.
    Usage,
}

/// The usage line of `extphy txn`.
pub const TXN_USAGE: &str = "usage: extphy txn begin <phy[,phy...]|all> [ms] | renew <n> [ms] | end <n> | list | \
                             <n> phy raw c45 <phy> <dev> <reg> [<value>]";

/// What a raw MDIO line addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawTarget {
    /// One PHY: a clause-45 access to an address word without the broadcast bit (0x400).
    Phy(u16),
    /// No single PHY: clause 22, `sbus` / `sim`, a broadcast address, a line that does not parse.
    Global,
}

/// `phy raw c45 PHY DEV REG [VALUE]` as the library serves it itself (every number in range:
/// an address word up to 0x7ff without the broadcast bit, device 1..=31, 16-bit register and
/// value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawC45 {
    /// The MDIO address word.
    pub phy: u16,
    /// The clause-45 device.
    pub dev: u8,
    /// The register.
    pub reg: u16,
    /// The value of a write; `None` for a read.
    pub value: Option<u16>,
}

/// One raw MDIO command of the diag socket: a bare `phy raw ...` line, or one carried by a
/// transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawRequest<'a> {
    /// The `phy raw ...` line (what the SDK shell would run).
    pub line: &'a str,
    /// The transaction it belongs to (`None`: a client without one).
    pub txn: Option<u64>,
    /// What it addresses.
    pub target: RawTarget,
    /// The line as a clause-45 access the library can serve itself, when it is one.
    pub c45: Option<RawC45>,
}

impl<'a> RawRequest<'a> {
    /// The request of a `phy raw ...` line of transaction `txn`.
    pub fn new(line: &'a str, txn: Option<u64>) -> Self {
        Self {
            line,
            txn,
            target: raw_target(line),
            c45: raw_c45(line),
        }
    }
}

/// A whole word as the SDK shell's `sal_strtoul(word, NULL, 0)` reads a number: `0x` / `0X`
/// hex, a leading `0` octal, else decimal. Anything `strtoul` would read differently (a sign,
/// characters after the number, a value over 32 bits) is `None`.
pub fn c_number(word: &str) -> Option<u32> {
    if let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        return (!hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten();
    }
    if !word.bytes().all(|b| b.is_ascii_digit()) || word.is_empty() {
        return None;
    }
    if word.len() > 1 && word.starts_with('0') {
        return u32::from_str_radix(word, 8).ok();
    }
    word.parse().ok()
}

/// The words of a raw line, split as the SDK shell splits them (spaces and tabs).
fn raw_words(line: &str) -> Vec<&str> {
    line.split([' ', '\t']).filter(|w| !w.is_empty()).collect()
}

fn is_word(word: Option<&&str>, keyword: &str) -> bool {
    word.is_some_and(|w| w.eq_ignore_ascii_case(keyword))
}

/// The address word of a clause-45 `phy raw` line, when it names one PHY.
fn c45_phy(words: &[&str]) -> Option<u16> {
    if !(is_word(words.first(), "phy") && is_word(words.get(1), "raw") && is_word(words.get(2), "c45")) {
        return None;
    }
    let phy = c_number(words.get(3)?)?;
    (phy <= 0x7ff && phy & 0x400 == 0)
        .then(|| u16::try_from(phy).ok())
        .flatten()
}

/// What a `phy raw` line addresses ([`RawTarget`]).
pub fn raw_target(line: &str) -> RawTarget {
    c45_phy(&raw_words(line)).map_or(RawTarget::Global, RawTarget::Phy)
}

/// A `phy raw c45 PHY DEV REG [VALUE]` line the library can serve itself ([`RawC45`]).
pub fn raw_c45(line: &str) -> Option<RawC45> {
    let words = raw_words(line);
    let phy = c45_phy(&words)?;
    let dev = u8::try_from(c_number(words.get(4)?)?)
        .ok()
        .filter(|d| (1..=31).contains(d))?;
    let reg = u16::try_from(c_number(words.get(5)?)?).ok()?;
    let value = match words.get(6..).unwrap_or_default() {
        [] => None,
        [v] => Some(u16::try_from(c_number(v)?).ok()?),
        _ => return None,
    };
    Some(RawC45 { phy, dev, reg, value })
}

/// The rest of `text` after its first word and the blanks behind it (`None`: no first word).
fn after_word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start_matches(c_space);
    let end = text.find(c_space).unwrap_or(text.len());
    let (word, rest) = text.split_at(end);
    (!word.is_empty()).then(|| (word, rest.trim_start_matches(c_space)))
}

/// `extphy txn ...` (`None`: the line is not one).
fn txn(line: &str) -> Option<Txn<'_>> {
    let (first, rest) = after_word(line)?;
    let (second, rest) = after_word(rest)?;
    if first != "extphy" || second != "txn" {
        return None;
    }
    let mut words = c_words(rest);
    let sub = words.next();
    let a = words.next();
    let b = words.next();
    let more = words.next().is_some();
    let ms = |w: Option<&str>| -> Option<Option<u64>> {
        match w {
            None => Some(None),
            Some(w) => w.parse::<u64>().ok().filter(|m| *m > 0).map(Some),
        }
    };
    let number = |w: Option<&str>| w.and_then(|w| w.parse::<u64>().ok());
    Some(match sub {
        None | Some("list") if a.is_none() => Txn::List,
        Some("begin") if !more => {
            let Some(scope) = a else {
                return Some(Txn::Usage);
            };
            let phys = if scope == "all" {
                None
            } else {
                let parsed: Option<std::collections::BTreeSet<u16>> = scope
                    .split(',')
                    .map(|w| {
                        c_number(w)
                            .filter(|p| *p <= 0x7ff && p & 0x400 == 0)
                            .and_then(|p| u16::try_from(p).ok())
                    })
                    .collect();
                match parsed {
                    Some(set) if !set.is_empty() => Some(set.into_iter().collect()),
                    _ => return Some(Txn::Usage),
                }
            };
            match ms(b) {
                Some(ms) => Txn::Begin { phys, ms },
                None => Txn::Usage,
            }
        }
        Some("renew") if !more => match (number(a), ms(b)) {
            (Some(txn), Some(ms)) => Txn::Renew { txn, ms },
            _ => Txn::Usage,
        },
        Some("end") if b.is_none() => match number(a) {
            Some(txn) => Txn::End { txn },
            None => Txn::Usage,
        },
        Some(n) => match (n.parse::<u64>().ok(), after_word(rest)) {
            (Some(txn), Some((_, raw))) if raw_mdio(raw) => Txn::Raw(RawRequest::new(raw, Some(txn))),
            _ => Txn::Usage,
        },
        None => Txn::Usage,
    })
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
    if let Some(txn) = txn(line) {
        return Command::Txn { line, txn };
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
    fn transactions_and_raw_targets() {
        let t = |l: &'static str| match parse(l.as_bytes()) {
            Ok(Command::Txn { line, txn }) => {
                assert_eq!(line, l.trim_end());
                txn
            }
            other => panic!("{l:?}: {other:?}"),
        };
        assert_eq!(t("extphy txn"), Txn::List);
        assert_eq!(t("extphy txn list"), Txn::List);
        assert_eq!(
            t("extphy txn begin 0x10"),
            Txn::Begin {
                phys: Some(vec![0x10]),
                ms: None
            }
        );
        assert_eq!(
            t("extphy txn begin 0x17f,0x0,1,0x17f 1500"),
            Txn::Begin {
                phys: Some(vec![0, 1, 0x17f]),
                ms: Some(1500)
            }
        );
        assert_eq!(t("extphy txn begin all"), Txn::Begin { phys: None, ms: None });
        assert_eq!(t("extphy txn renew 7 300"), Txn::Renew { txn: 7, ms: Some(300) });
        assert_eq!(t("extphy txn end 7"), Txn::End { txn: 7 });
        assert_eq!(
            t("extphy txn 7 phy raw c45 0x10 1 0x8221"),
            Txn::Raw(RawRequest {
                line: "phy raw c45 0x10 1 0x8221",
                txn: Some(7),
                target: RawTarget::Phy(0x10),
                c45: Some(RawC45 {
                    phy: 0x10,
                    dev: 1,
                    reg: 0x8221,
                    value: None
                })
            })
        );
        for bad in [
            "extphy txn begin",
            "extphy txn begin 0x400",
            "extphy txn begin 0x10 0",
            "extphy txn begin 0x10 x",
            "extphy txn begin 0x10,",
            "extphy txn begin 0x10 5 6",
            "extphy txn end",
            "extphy txn end x",
            "extphy txn end 1 2",
            "extphy txn renew",
            "extphy txn 7 ps",
            "extphy txn 7",
            "extphy txn list x",
            "extphy txn x",
        ] {
            assert_eq!(t(bad), Txn::Usage, "{bad}");
        }
        // not a transaction line
        assert_eq!(parse(b"extphy txnx"), Ok(shell("extphy txnx")));
        assert_eq!(parse(b"EXTPHY txn"), Ok(shell("EXTPHY txn")));
        // what a raw line addresses, and what the library serves itself
        assert_eq!(raw_target("phy raw c45 0x10 1 0x8b00"), RawTarget::Phy(0x10));
        assert_eq!(raw_target("PHY\tRAW C45 020 0 0x8b00"), RawTarget::Phy(0o20));
        assert_eq!(raw_target("phy raw c45 0x410 1 0x8b00"), RawTarget::Global);
        assert_eq!(raw_target("phy raw 0x10 2"), RawTarget::Global);
        assert_eq!(raw_target("phy raw sbus 0x10 1 2"), RawTarget::Global);
        assert_eq!(raw_target("phy raw c45 x 1 2"), RawTarget::Global);
        assert_eq!(
            raw_c45("phy raw c45 0x17e 0x1 0xd139 0x351"),
            Some(RawC45 {
                phy: 0x17e,
                dev: 1,
                reg: 0xd139,
                value: Some(0x351)
            })
        );
        for not_served in [
            "phy raw c45 0x10 0 0x8b00",
            "phy raw c45 0x10 32 0x8b00",
            "phy raw c45 0x10 1 0x10000",
            "phy raw c45 0x10 1 2 0x10000",
            "phy raw c45 0x10 1 2 3 4",
            "phy raw c45 0x10 1",
            "phy raw c45 0x800 1 2",
            "phy raw c45 -1 1 2",
            "phy raw c45 0x10 1 2x",
        ] {
            assert_eq!(raw_c45(not_served), None, "{not_served}");
        }
        assert_eq!(c_number("010"), Some(8));
        assert_eq!(c_number("0"), Some(0));
        assert_eq!(c_number("0x"), None);
        assert_eq!(c_number("08"), None);
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

//! Fuzz-style property tests of the request parser against a literal transcription of the C++
//! (`sai_diag.cpp` `serve`, `refused`, `raw_mdio`, the own-command checks) on generated
//! requests: words from the commands people send, refused words, separators and random bytes.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
// `cpp` transcribes the C++ statement by statement, on purpose.
#![allow(
    clippy::many_single_char_names,
    clippy::manual_strip,
    clippy::manual_pattern_char_comparison
)]

use proptest::prelude::*;
use sai_diag::command::{self, Command, MdioAccess, Rejection};

/// What the C++ does with a request, as text: "reject-line", "refused", "extphy", "extmacsec",
/// "trace:<args>", "run:<port>|<seq>|<file>", "shell:<raw?>:<line>".
fn cpp(request: &[u8]) -> String {
    // serve(): cut at the first \n, pop trailing \r and ' ', drop leading ' '
    let mut cmd: Vec<u8> = request.to_vec();
    if let Some(nl) = cmd.iter().position(|&b| b == b'\n') {
        cmd.truncate(nl);
    }
    while matches!(cmd.last(), Some(b'\r' | b' ')) {
        cmd.pop();
    }
    let b = cmd.iter().position(|&c| c != b' ');
    let cmd = b.map_or(Vec::new(), |b| cmd[b..].to_vec());
    if cmd.is_empty() || cmd.len() >= 1000 {
        return "reject-line".into();
    }
    // refused()
    let mut w = Vec::new();
    for &ch in &cmd {
        if ch == b' ' || ch == b'\t' || ch == b';' {
            break;
        }
        w.push(ch.to_ascii_lowercase());
    }
    let bad: [&[u8]; 14] = [
        b"exit",
        b"quit",
        b"init",
        b"deinit",
        b"reset",
        b"detach",
        b"attach",
        b"reload",
        b"rcload",
        b"shell",
        b"sh",
        b"!",
        b"warmboot",
        b"dbm",
    ];
    if cmd.contains(&b';') || bad.contains(&w.as_slice()) || w.first() == Some(&b'!') {
        return "refused".into();
    }
    // Rust only: not text
    let Ok(text) = String::from_utf8(cmd.clone()) else {
        return "reject-line".into();
    };
    if text.contains('\0') {
        return "reject-line".into();
    }
    if text == "extphy" {
        return "extphy".into();
    }
    if text == "extmacsec" {
        return "extmacsec".into();
    }
    if text == "extmacsec trace" || text.starts_with("extmacsec trace ") {
        return format!("trace:{}", &text[15..]);
    }
    if text.starts_with("extmacsec run ") {
        // istringstream >> port >> seq >> file (isspace)
        let mut words = text[14..]
            .split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
            .filter(|w| !w.is_empty());
        let (p, s, f) = (words.next(), words.next(), words.next());
        return format!("run:{}|{}|{}", p.unwrap_or(""), s.unwrap_or(""), f.unwrap_or(""));
    }
    // raw_mdio(): two words split at ' ' / '\t', lower case
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut ws: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    for word in &mut ws {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\t' {
            word.push(bytes[i].to_ascii_lowercase());
            i += 1;
        }
    }
    let raw = ws[0] == b"phy" && ws[1] == b"raw";
    format!("shell:{raw}:{text}")
}

fn rust(request: &[u8]) -> String {
    match command::parse(request) {
        Err(Rejection::NotOneLine) => "reject-line".into(),
        Err(Rejection::Refused(line)) => {
            assert_ne!(line, b"");
            "refused".into()
        }
        Ok(Command::ExtPhy) => "extphy".into(),
        Ok(Command::ExtMacsec) => "extmacsec".into(),
        Ok(Command::ExtMacsecTrace(args)) => format!("trace:{args}"),
        Ok(Command::ExtMacsecRun(r)) => format!("run:{}|{}|{}", r.port, r.sequence, r.param_file.unwrap_or("")),
        Ok(Command::Shell(s)) => format!("shell:{}:{}", s.mdio == MdioAccess::Raw, s.line),
    }
}

fn piece() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => prop::sample::select(vec![
            "phy", "PHY", "raw", "Raw", "c45", "0x10", "1", "0x8b00", "extphy", "extmacsec", "trace", "run", "on",
            "all", "dump", "ce0", "sa.install.egress", "/run/k", "l2", "show", "ps", "exit", "Quit", "init", "sh",
            "shell", "rcload", "dbm", "warmboot", "attach", "!ls", "!", "x", "",
        ])
        .prop_map(|w| w.as_bytes().to_vec()),
        3 => prop::sample::select(vec![" ", "  ", "\t", "\r", "\n", ";", "\x0b", "\x0c", " \r\n"])
            .prop_map(|w| w.as_bytes().to_vec()),
        1 => prop::collection::vec(any::<u8>(), 0..4),
        1 => (0usize..1100).prop_map(|n| vec![b'a'; n]),
    ]
}

fn request() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(piece(), 0..12).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20_000))]

    #[test]
    fn the_parser_does_what_the_cpp_does(req in request()) {
        prop_assert_eq!(rust(&req), cpp(&req), "request {:?}", String::from_utf8_lossy(&req));
    }

    #[test]
    fn arbitrary_bytes_never_panic(req in prop::collection::vec(any::<u8>(), 0..1500)) {
        prop_assert_eq!(rust(&req), cpp(&req));
    }

    /// The answer to a refusal names the line, and a refused line is never run.
    #[test]
    fn a_line_with_a_semicolon_is_never_run(head in request(), tail in request()) {
        let mut req = command::line(&head).to_vec();
        req.push(b';');
        req.extend_from_slice(&tail);
        if let Ok(c) = command::parse(&req) {
            prop_assert!(false, "ran {:?}", c);
        }
    }
}

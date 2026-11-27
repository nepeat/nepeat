//! The golden format of the C++ side of sai-macsec's differential (`tests/differential.rs`) and
//! sai-macsec-phy's end-to-end test (`tests/e2e.rs`), shared by both through `#[path]`.
//!
//! Two text files per test (header comments start with `#`; every other line is a record):
//!
//! - **inputs**: `== table <tag>` and `== script <id> <tag>` sections whose body lines each start with
//!   `>` (the text after it is the file's line), so blank lines and leading blanks survive. A script runs
//!   on the table of its tag. `pinned <tag> <bytes> <fnv>` names a table the golden does not carry (Nix generates it).
//! - **outputs of the C++**: one `case <id> <tag> rust=<fixed|differs> plain=<lines>:<fnv> fixed=<lines>:<fnv> listed=<none|D9@n|D10@n>`
//!   line per script (`plain`: the unmodified C++, `fixed`: the C++ with the owner's D9 / D10; lines and FNV-1a 64 of
//!   the NORMALISED output text; `listed`: where the two first differ), and `== text <id> <plain|fixed>` sections with the
//!   full output of the named small cases. `rust=differs`: the Rust differs from the C++ there by design (D1).
//!
//! The Rust side must equal the `fixed` output of every case with `rust=fixed` (digest, and the text where
//! there is one).
#![allow(dead_code, reason = "each test file uses part of it")]

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// FNV-1a 64 of the bytes of `text`.
pub(crate) fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Lines and FNV-1a of an output text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Digest {
    pub(crate) lines: usize,
    pub(crate) fnv: u64,
}

impl Digest {
    pub(crate) fn of(text: &str) -> Self {
        Self {
            lines: text.lines().count(),
            fnv: fnv1a(text),
        }
    }

    fn show(self) -> String {
        format!("{}:{:016x}", self.lines, self.fnv)
    }

    fn parse(s: &str) -> Option<Self> {
        let (lines, fnv) = s.split_once(':')?;
        Some(Self {
            lines: lines.parse().ok()?,
            fnv: u64::from_str_radix(fnv, 16).ok()?,
        })
    }
}

/// One script's record of the C++ side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Case {
    pub(crate) id: String,
    pub(crate) tag: String,
    /// The Rust differs from the C++ here by design (D1): not held to the `fixed` output.
    pub(crate) rust_differs: bool,
    pub(crate) plain: Digest,
    pub(crate) fixed: Digest,
    /// `none`, `D9@<line>` or `D10@<line>` (0-based output line where plain and fixed first differ).
    pub(crate) listed: String,
}

/// The parsed outputs file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Golden {
    pub(crate) cases: Vec<Case>,
    /// (case id, "plain" | "fixed") -> the full normalised output.
    pub(crate) texts: BTreeMap<(String, String), String>,
}

impl Golden {
    /// The kept text of `case`'s `plain` or `fixed` output; where the two digests are equal only the `fixed` text is kept.
    pub(crate) fn text(&self, case: &Case, kind: &str) -> Option<&str> {
        let key = |k: &str| (case.id.clone(), k.to_owned());
        self.texts
            .get(&key(kind))
            .or_else(|| {
                (kind == "plain" && case.plain == case.fixed)
                    .then(|| self.texts.get(&key("fixed")))
                    .flatten()
            })
            .map(String::as_str)
    }

    /// How many cases keep a full text.
    pub(crate) fn kept(&self) -> usize {
        self.texts
            .keys()
            .map(|(id, _)| id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }
}

/// The parsed inputs file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inputs {
    pub(crate) tables: BTreeMap<String, String>,
    /// Tables the golden does not carry (the AS7772's, which Nix generates): tag -> (bytes, FNV-1a 64
    /// of the text); a test uses the table a run names only when it is this one.
    pub(crate) pinned: BTreeMap<String, (usize, u64)>,
    /// (id, table tag, script), in file order.
    pub(crate) scripts: Vec<(String, String, String)>,
}

/// `(header words, body)` of each `== ...` section; other non-comment lines are returned as
/// sections of their own with no body (`case` records).
fn sections(text: &str) -> Result<Vec<(Vec<String>, String)>, String> {
    let mut out: Vec<(Vec<String>, String)> = Vec::new();
    let mut in_body = false;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("== ") {
            out.push((rest.split_whitespace().map(str::to_owned).collect(), String::new()));
            in_body = true;
        } else if let Some(body) = line.strip_prefix('>') {
            let Some(last) = out.last_mut().filter(|_| in_body) else {
                return Err(format!("line {}: a body line outside a section", n + 1));
            };
            last.1.push_str(body);
            last.1.push('\n');
        } else if !line.trim().is_empty() {
            out.push((line.split_whitespace().map(str::to_owned).collect(), String::new()));
            in_body = false;
        }
    }
    Ok(out)
}

pub(crate) fn parse_inputs(text: &str) -> Result<Inputs, String> {
    let mut inputs = Inputs::default();
    for (head, body) in sections(text)? {
        match head.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["table", tag] => {
                inputs.tables.insert((*tag).to_owned(), body);
            }
            ["script", id, tag] => inputs.scripts.push(((*id).to_owned(), (*tag).to_owned(), body)),
            ["pinned", tag, len, fnv] => {
                let (Ok(len), Ok(fnv)) = (len.parse(), u64::from_str_radix(fnv, 16)) else {
                    return Err(format!("inputs: malformed pinned table {tag}"));
                };
                inputs.pinned.insert((*tag).to_owned(), (len, fnv));
            }
            other => return Err(format!("inputs: unknown record {other:?}")),
        }
    }
    Ok(inputs)
}

pub(crate) fn parse_golden(text: &str) -> Result<Golden, String> {
    let mut golden = Golden::default();
    for (head, body) in sections(text)? {
        let words: Vec<&str> = head.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["text", id, kind] => {
                golden.texts.insert(((*id).to_owned(), (*kind).to_owned()), body);
            }
            ["case", id, tag, rust, plain, fixed, listed] => {
                let field = |s: &str, key: &str| s.strip_prefix(key).map(str::to_owned);
                let (Some(rust), Some(plain), Some(fixed), Some(listed)) = (
                    field(rust, "rust="),
                    field(plain, "plain="),
                    field(fixed, "fixed="),
                    field(listed, "listed="),
                ) else {
                    return Err(format!("golden: malformed case {id}"));
                };
                let (Some(plain), Some(fixed)) = (Digest::parse(&plain), Digest::parse(&fixed)) else {
                    return Err(format!("golden: malformed digest of case {id}"));
                };
                golden.cases.push(Case {
                    id: (*id).to_owned(),
                    tag: (*tag).to_owned(),
                    rust_differs: match rust.as_str() {
                        "fixed" => false,
                        "differs" => true,
                        other => return Err(format!("golden: case {id}: rust={other}")),
                    },
                    plain,
                    fixed,
                    listed,
                });
            }
            other => return Err(format!("golden: unknown record {other:?}")),
        }
    }
    Ok(golden)
}

fn body(out: &mut String, text: &str) {
    for l in text.lines() {
        out.push('>');
        out.push_str(l);
        out.push('\n');
    }
}

pub(crate) fn emit_inputs(inputs: &Inputs) -> String {
    let mut out = String::new();
    for (tag, (len, fnv)) in &inputs.pinned {
        writeln!(out, "pinned {tag} {len} {fnv:016x}").unwrap();
    }
    for (tag, table) in &inputs.tables {
        writeln!(out, "== table {tag}").unwrap();
        body(&mut out, table);
    }
    for (id, tag, script) in &inputs.scripts {
        writeln!(out, "== script {id} {tag}").unwrap();
        body(&mut out, script);
    }
    out
}

pub(crate) fn emit_golden(golden: &Golden) -> String {
    let mut out = String::new();
    for c in &golden.cases {
        writeln!(
            out,
            "case {} {} rust={} plain={} fixed={} listed={}",
            c.id,
            c.tag,
            if c.rust_differs { "differs" } else { "fixed" },
            c.plain.show(),
            c.fixed.show(),
            c.listed
        )
        .unwrap();
    }
    for ((id, kind), text) in &golden.texts {
        writeln!(out, "== text {id} {kind}").unwrap();
        body(&mut out, text);
    }
    out
}

/// The first line (1-based) where two texts differ, with both lines.
fn first_line_difference(a: &str, b: &str) -> String {
    let (mut x, mut y) = (a.lines(), b.lines());
    let mut n = 0;
    loop {
        n += 1;
        match (x.next(), y.next()) {
            (None, None) => return "no line differs (a trailing difference)".to_owned(),
            (l, r) if l == r => {}
            (l, r) => {
                return format!(
                    "line {n}: golden {:?} / other {:?}",
                    l.unwrap_or("<end>"),
                    r.unwrap_or("<end>")
                );
            }
        }
    }
}

/// `text` (normalised output of `what`) against a digest and, where the golden keeps it, the text.
pub(crate) fn held_to(
    case: &Case,
    kind: &str,
    digest: Digest,
    golden_text: Option<&str>,
    text: &str,
    what: &str,
) -> Result<(), String> {
    if let Some(golden) = golden_text
        && golden != text
    {
        return Err(format!(
            "case {}: {what} differs from the golden {kind} text at {}",
            case.id,
            first_line_difference(golden, text)
        ));
    }
    let want = Digest::of(text);
    if want != digest {
        return Err(format!(
            "case {}: {what} has {} lines / fnv {:016x}, the golden {kind} {} / {:016x}",
            case.id, want.lines, want.fnv, digest.lines, digest.fnv
        ));
    }
    Ok(())
}

/// The Rust output (normalised) against the golden C++ with D9 / D10 of `case`; also the golden's own
/// consistency: `listed=none` exactly when the two C++ digests are equal. `Ok(Some(..))`: the case reaches
/// D9 / D10 ("D9" / "D10").
pub(crate) fn rust_vs_golden(golden: &Golden, case: &Case, rust: &str) -> Result<Option<String>, String> {
    if (case.listed == "none") != (case.plain == case.fixed) {
        return Err(format!(
            "case {}: listed={} but the plain / fixed digests are {}",
            case.id,
            case.listed,
            if case.plain == case.fixed { "equal" } else { "different" }
        ));
    }
    if !case.rust_differs {
        held_to(case, "fixed", case.fixed, golden.text(case, "fixed"), rust, "the Rust")?;
    }
    Ok(case
        .listed
        .split('@')
        .next()
        .filter(|k| *k != "none")
        .map(str::to_owned))
}

/// What the live C++ printed (normalised) against the golden of `case`: the proof that the golden was
/// captured from this C++.
pub(crate) fn live_vs_golden(
    golden: &Golden,
    case: &Case,
    plain: &str,
    fixed: &str,
    listed: &str,
) -> Result<(), String> {
    held_to(
        case,
        "plain",
        case.plain,
        golden.text(case, "plain"),
        plain,
        "the live C++",
    )?;
    held_to(
        case,
        "fixed",
        case.fixed,
        golden.text(case, "fixed"),
        fixed,
        "the live C++ with D9 / D10",
    )?;
    if listed != case.listed {
        return Err(format!(
            "case {}: live listed={listed}, golden {}",
            case.id, case.listed
        ));
    }
    Ok(())
}

//! The tap's stream (`c/tap.c`): every IFCS call line the mock logged, in order, with the markers
//! of the program that ran (written into the stream through `PARITY_TAP_FILE`: `@@ begin <section>` /
//! `@@ end <section>` / `@@ died-in <section>`, as the C++ `c/sections.cpp` did, deleted in H5), cut into
//! named units and compared unit by unit.
#![forbid(unsafe_code)]

use regex::Regex;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

/// A named run of stream lines (a `test_ifcs` section, an oracle step).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// Its name.
    pub name: String,
    /// Its lines, in order.
    pub lines: Vec<String>,
    /// The program died inside it (`@@ died-in`).
    pub died: bool,
}

impl Unit {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            lines: Vec::new(),
            died: false,
        }
    }
}

/// The sections of a `test_ifcs` stream: `main` up to `@@ end main` (the refusals and
/// `create_switch`), one per `@@ begin`, and `uninit` for lines after an `@@ end` (what `main()`
/// does after the last area).
pub fn sections(text: &str) -> Vec<Unit> {
    let mut out = vec![Unit::new("main")];
    let mut open = true;
    for line in text.lines() {
        if let Some(marker) = line.strip_prefix("@@ ") {
            let (what, name) = marker.split_once(' ').unwrap_or((marker, ""));
            match what {
                "begin" => {
                    out.push(Unit::new(name));
                    open = true;
                }
                "end" => open = false,
                "died-in" => {
                    if let Some(u) = out.iter_mut().rev().find(|u| u.name == name) {
                        u.died = true;
                    }
                }
                _ => {}
            }
            continue;
        }
        if !open && out.last().is_none_or(|u| u.name != "uninit") {
            out.push(Unit::new("uninit"));
            open = true;
        }
        if let Some(u) = out.last_mut() {
            u.lines.push(line.to_owned());
        }
    }
    out
}

static HEX: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"0x[0-9a-fA-F]+").ok());

/// The lines with every hex number replaced by `#<n>`, n counting distinct numbers in order of first
/// appearance: equal canonical forms mean the same calls with the handles the mock allocated
/// numbered differently (a triage hint only: a consistently different value is hidden too).
pub fn canonical(lines: &[String]) -> Vec<String> {
    let Some(re) = HEX.as_ref() else {
        return lines.to_vec();
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    lines
        .iter()
        .map(|l| {
            re.replace_all(l, |c: &regex::Captures<'_>| {
                let k = c.get(0).map_or("", |m| m.as_str()).to_ascii_lowercase();
                let n = seen.len();
                format!("#{}", *seen.entry(k).or_insert(n))
            })
            .into_owned()
        })
        .collect()
}

/// How two runs of lines differ.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineDiff {
    /// Line counts (C++, Rust).
    pub counts: (usize, usize),
    /// Equal as they are.
    pub raw_equal: bool,
    /// Equal after [`canonical`].
    pub canonical_equal: bool,
    /// The first differing position and the two lines there (`None`: past the end).
    pub first: Option<(usize, Option<String>, Option<String>)>,
    /// Lines only one side has, counted as multisets (C++ only, Rust only).
    pub only: (usize, usize),
    /// The reference is a golden's digest, not its lines: `first` is looked for in the stored head
    /// only, `only` and `canonical_equal` are not known ([`crate::golden::diff_ref`]).
    pub reduced: bool,
}

/// Compares two runs of lines.
pub fn diff_lines(cpp: &[String], rust: &[String]) -> LineDiff {
    let raw_equal = cpp == rust;
    let first = if raw_equal {
        None
    } else {
        let i = cpp.iter().zip(rust).take_while(|(a, b)| a == b).count();
        Some((i, cpp.get(i).cloned(), rust.get(i).cloned()))
    };
    let mut bag: BTreeMap<&str, i64> = BTreeMap::new();
    for l in cpp {
        *bag.entry(l).or_insert(0) += 1;
    }
    for l in rust {
        *bag.entry(l).or_insert(0) -= 1;
    }
    let only_cpp = bag.values().filter(|v| **v > 0).map(|v| v.unsigned_abs()).sum::<u64>();
    let only_rust = bag.values().filter(|v| **v < 0).map(|v| v.unsigned_abs()).sum::<u64>();
    LineDiff {
        counts: (cpp.len(), rust.len()),
        raw_equal,
        canonical_equal: raw_equal || canonical(cpp) == canonical(rust),
        first,
        only: (
            usize::try_from(only_cpp).unwrap_or(usize::MAX),
            usize::try_from(only_rust).unwrap_or(usize::MAX),
        ),
        reduced: false,
    }
}

/// [`describe`] of a difference against a golden (only the digest and the head are known).
fn describe_reduced(d: &LineDiff) -> String {
    let mut text = format!(
        "DIFFERS from the golden C++ ({} golden / {} Rust lines)",
        d.counts.0, d.counts.1
    );
    match &d.first {
        Some((at, golden, rust)) => {
            let show = |l: &Option<String>| l.as_deref().map_or_else(|| "(end)".to_owned(), |l| format!("'{l}'"));
            let _ = write!(
                text,
                "; first at line {}: C++ {} / Rust {}",
                at + 1,
                show(golden),
                show(rust)
            );
        }
        None => text.push_str("; beyond the stored head (digest differs)"),
    }
    text
}

/// One line of a [`LineDiff`] for a report.
pub fn describe(d: &LineDiff) -> String {
    if d.raw_equal {
        return format!("equal ({} lines)", d.counts.0);
    }
    if d.reduced {
        return describe_reduced(d);
    }
    let mut s = format!(
        "{} ({} C++ / {} Rust lines; {} only in C++, {} only in Rust)",
        if d.canonical_equal {
            "equal modulo hex numbering"
        } else {
            "DIFFERS"
        },
        d.counts.0,
        d.counts.1,
        d.only.0,
        d.only.1
    );
    if let Some((i, c, r)) = &d.first {
        let show = |x: &Option<String>| x.as_deref().map_or_else(|| "(end)".to_owned(), |l| format!("'{l}'"));
        let _ = write!(s, "; first at line {}: C++ {} / Rust {}", i + 1, show(c), show(r));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn sections_follow_the_markers() {
        let t = "a\nb\n@@ end main\n@@ begin l2\nc\n@@ end l2\n@@ begin hooks\nd\n@@ died-in hooks\n";
        let s = sections(t);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].name.as_str(), s[0].lines.len()), ("main", 2));
        assert_eq!((s[1].name.as_str(), s[1].lines.clone()), ("l2", v(&["c"])));
        assert!(s[2].died && !s[1].died);
        let u = sections("x\n@@ end main\n@@ begin r\n@@ end r\ny\nz\n@@ end uninit\n");
        assert_eq!(u.last().map(|x| (x.name.as_str(), x.lines.len())), Some(("uninit", 2)));
    }

    #[test]
    fn hex_renumbering_absorbs_shifted_handles_only() {
        let c = v(&["create h=0x100", "use 0x100 dest=0x3e200001", "create h=0x101"]);
        let r = v(&["create h=0x105", "use 0x105 dest=0x3e200001", "create h=0x106"]);
        let d = diff_lines(&c, &r);
        assert!(!d.raw_equal && d.canonical_equal);
        assert_eq!(d.first, Some((0, Some(c[0].clone()), Some(r[0].clone()))));
        let w = v(&["create h=0x105", "use 0x106 dest=0x3e200001", "create h=0x106"]);
        assert!(!diff_lines(&c, &w).canonical_equal);
        assert!(diff_lines(&c, &c).raw_equal);
    }

    #[test]
    fn multiset_counts_and_description() {
        let d = diff_lines(&v(&["a", "b", "b"]), &v(&["a", "b", "c", "d"]));
        assert_eq!(d.only, (1, 2));
        assert_eq!(d.counts, (3, 4));
        assert!(
            describe(&d).starts_with("DIFFERS (3 C++ / 4 Rust lines; 1 only in C++, 2 only in Rust); first at line 3")
        );
        assert_eq!(describe(&diff_lines(&v(&["a"]), &v(&["a"]))), "equal (1 lines)");
    }
}

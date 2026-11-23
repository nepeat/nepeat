//! The comparison of two driver outputs, op block by op block, and the allow-list of the
//! differences still expected (`tests/differential-allow.txt`).
//!
//! A block is an op's `> ` line and every line up to the next `> ` line. Blocks are paired by
//! position; both drivers echo the same script lines, so differing echo lines are a hard failure.
//! A differing block passes only if an allow-list entry covers it:
//!
//! ```text
//! <script stem or *> TAB <regex on the op line, without its "> "> TAB <reason>
//! ```
//!
//! Blank lines and lines starting with `#` are comments. An entry covers a block when its script
//! is the block's (or `*`) and its regex matches somewhere in the op line (anchor it with `^` /
//! `$`). A block is counted for the first entry that covers it; an entry no block is counted for
//! is stale and fails the comparison too, so the list can only shrink honestly.
use regex::Regex;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// One op block of a driver's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The op line without `> ` (empty for lines before the first op).
    pub op: String,
    /// Every line of the block, the op line included.
    pub lines: Vec<String>,
}

/// The op blocks of an output.
pub fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for line in text.lines() {
        match (line.strip_prefix("> "), out.last_mut()) {
            (Some(op), _) => out.push(Block {
                op: op.to_owned(),
                lines: vec![line.to_owned()],
            }),
            (None, Some(block)) => block.lines.push(line.to_owned()),
            (None, None) => out.push(Block {
                op: String::new(),
                lines: vec![line.to_owned()],
            }),
        }
    }
    out
}

/// One allow-list entry.
#[derive(Clone, Debug)]
pub struct Entry {
    /// A script stem, or `*` for every script.
    pub script: String,
    /// The regex on the op line.
    pub pattern: Regex,
    /// Why the difference is expected.
    pub reason: String,
    /// The entry's line in the file (1-based).
    pub line: usize,
}

impl Entry {
    /// Whether the entry covers an op of a script.
    pub fn covers(&self, script: &str, op: &str) -> bool {
        (self.script == "*" || self.script == script) && self.pattern.is_match(op)
    }
}

/// Reads an allow-list.
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(script), Some(pattern), Some(reason)) = (fields.next(), fields.next(), fields.next()) else {
            return Err(format!(
                "allow-list line {}: expected <script>\\t<regex>\\t<reason>",
                i + 1
            ));
        };
        let pattern = Regex::new(pattern).map_err(|e| format!("allow-list line {}: {e}", i + 1))?;
        if script.is_empty() || reason.trim().is_empty() {
            return Err(format!("allow-list line {}: empty script or reason", i + 1));
        }
        entries.push(Entry {
            script: script.to_owned(),
            pattern,
            reason: reason.to_owned(),
            line: i + 1,
        });
    }
    Ok(entries)
}

/// A pair of blocks that differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    /// The script stem.
    pub script: String,
    /// The block's position in the output (0-based).
    pub index: usize,
    /// The op line without `> `.
    pub op: String,
    /// The C++ driver's block.
    pub cpp: Vec<String>,
    /// The Rust driver's block.
    pub rust: Vec<String>,
}

impl Diff {
    /// The first line pair that differs, shortened (`TODO` reasons).
    pub fn first_difference(&self) -> String {
        let short = |s: Option<&String>| {
            let s = s.map_or("<none>", String::as_str);
            let cut: String = s.chars().take(70).collect();
            if cut.len() < s.len() { format!("{cut}...") } else { cut }
        };
        let n = self.cpp.len().max(self.rust.len());
        (0..n)
            .find(|i| self.cpp.get(*i) != self.rust.get(*i))
            .map(|i| format!("cpp {} | rust {}", short(self.cpp.get(i)), short(self.rust.get(i))))
            .unwrap_or_default()
            .replace('\t', " ")
    }

    /// How many lines of the longer block have no equal line in the other (a rough size).
    pub fn changed_lines(&self) -> usize {
        let common = lcs(&self.cpp, &self.rust).len();
        self.cpp.len().max(self.rust.len()) - common
    }
}

/// The result of comparing one script's outputs.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    /// Blocks compared.
    pub blocks: usize,
    /// The differing ones.
    pub diffs: Vec<Diff>,
}

/// Compares the outputs of one script. `Err`: the block structure differs (echo lines or count).
pub fn compare(script: &str, cpp: &str, rust: &str) -> Result<Comparison, String> {
    let (a, b) = (blocks(cpp), blocks(rust));
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x.op != y.op {
            return Err(format!(
                "{script}: block {i}: the echo lines differ: cpp '> {}', rust '> {}'",
                x.op, y.op
            ));
        }
    }
    if a.len() != b.len() {
        return Err(format!("{script}: cpp printed {} op blocks, rust {}", a.len(), b.len()));
    }
    let diffs = a
        .iter()
        .zip(&b)
        .enumerate()
        .filter(|(_, (x, y))| x.lines != y.lines)
        .map(|(index, (x, y))| Diff {
            script: script.to_owned(),
            index,
            op: x.op.clone(),
            cpp: x.lines.clone(),
            rust: y.lines.clone(),
        })
        .collect();
    Ok(Comparison { blocks: a.len(), diffs })
}

/// The longest common subsequence of two line lists, as index pairs.
fn lcs(left: &[String], right: &[String]) -> Vec<(usize, usize)> {
    // table[i][j]: the LCS length of left[i..] and right[j..]
    let mut table = vec![vec![0usize; right.len() + 1]; left.len() + 1];
    let at = |table: &[Vec<usize>], i: usize, j: usize| table.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0);
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            let len = if left.get(i) == right.get(j) {
                at(&table, i + 1, j + 1) + 1
            } else {
                at(&table, i + 1, j).max(at(&table, i, j + 1))
            };
            if let Some(cell) = table.get_mut(i).and_then(|row| row.get_mut(j)) {
                *cell = len;
            }
        }
    }
    let (mut i, mut j, mut pairs) = (0, 0, Vec::new());
    while i < left.len() && j < right.len() {
        if left.get(i) == right.get(j) {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if at(&table, i + 1, j) >= at(&table, i, j + 1) {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// A unified-style diff of a block pair: `-` the C++ line, `+` the Rust line, ` ` both.
pub fn unified(d: &Diff) -> String {
    let mut s = format!(
        "--- cpp  {} block {}\n+++ rust {} block {}\n",
        d.script, d.index, d.script, d.index
    );
    let (mut i, mut j) = (0, 0);
    let common = lcs(&d.cpp, &d.rust);
    for (ci, cj) in common.into_iter().chain([(d.cpp.len(), d.rust.len())]) {
        for line in d.cpp.get(i..ci).unwrap_or_default() {
            let _ = writeln!(s, "-{line}");
        }
        for line in d.rust.get(j..cj).unwrap_or_default() {
            let _ = writeln!(s, "+{line}");
        }
        if let Some(line) = d.cpp.get(ci) {
            let _ = writeln!(s, " {line}");
        }
        (i, j) = (ci + 1, cj + 1);
    }
    s
}

/// Which entry covers each differing block, and the entries nothing is counted for.
#[derive(Clone, Debug, Default)]
pub struct Verdict {
    /// Per diff (same order): the index of the entry it is counted for.
    pub covered_by: Vec<Option<usize>>,
    /// Blocks counted per entry (same order as the entries).
    pub uses: Vec<usize>,
}

impl Verdict {
    /// The entries no block is counted for.
    pub fn stale(&self) -> Vec<usize> {
        self.uses
            .iter()
            .enumerate()
            .filter(|(_, n)| **n == 0)
            .map(|(i, _)| i)
            .collect()
    }
    /// The diffs no entry covers.
    pub fn uncovered(&self) -> Vec<usize> {
        self.covered_by
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_none())
            .map(|(i, _)| i)
            .collect()
    }
    /// Green: everything covered, nothing stale.
    pub fn green(&self) -> bool {
        self.stale().is_empty() && self.uncovered().is_empty()
    }
}

/// Matches the diffs against the entries (first covering entry counts).
pub fn judge(entries: &[Entry], diffs: &[Diff]) -> Verdict {
    let mut uses = vec![0; entries.len()];
    let covered_by = diffs
        .iter()
        .map(|d| {
            let hit = entries.iter().position(|e| e.covers(&d.script, &d.op));
            if let Some(n) = hit.and_then(|i| uses.get_mut(i)) {
                *n += 1;
            }
            hit
        })
        .collect();
    Verdict { covered_by, uses }
}

/// The header of a regenerated allow-list.
pub const HEADER: &str = "\
# Allow-list of the C++-vs-Rust frontend differential (crates/sai-driver/tests/differential.rs).
# <script stem or *> TAB <regex on the op line, without its \"> \"> TAB <reason>
# A differing op block passes only if an entry covers it (the first covering entry counts it);
# an entry that counts no block fails the test, so this list only shrinks.
# SAI_DRIVER_ALLOW_UPDATE=1 rewrites it: one entry per differing block, the reasons of entries
# that still cover kept, \"TODO: <first differing line pair>\" otherwise.
";

/// A regenerated allow-list: one entry per differing block (the exact escaped op line), with the
/// reason of the entry that covers it now, else `TODO: <first differing line pair>`.
pub fn regenerate(entries: &[Entry], diffs: &[Diff]) -> String {
    let mut s = HEADER.to_owned();
    for d in diffs {
        let reason = entries
            .iter()
            .find(|e| e.covers(&d.script, &d.op))
            .map_or_else(|| format!("TODO: {}", d.first_difference()), |e| e.reason.clone());
        let _ = writeln!(s, "{}\t^{}$\t{}", d.script, regex::escape(&d.op), reason);
    }
    s
}

/// The area of an op for the counts: the object type of create / remove / set / get / stats /
/// clear, else the operation (`switch`, `inject`, ...).
pub fn area(op: &str) -> String {
    let mut w = op.split_whitespace();
    match (w.next(), w.next()) {
        (Some("create" | "remove" | "set" | "get" | "stats" | "clear"), Some(ty)) => ty.to_owned(),
        (Some(other), _) => other.to_owned(),
        (None, _) => String::new(),
    }
}

/// Counts per key, sorted by key.
pub fn tally<'a>(keys: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for k in keys {
        *m.entry(k.to_owned()).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPP: &str = "> switch\n= SUCCESS\n  be init() -> SUCCESS\n> get port port0 SPEED\n= SUCCESS\n  SPEED=100000\n> remove vlan v100\n= SUCCESS\n";
    const RUST: &str = "> switch\n= SUCCESS\n  be init() -> SUCCESS\n  be ports() -> []\n> get port port0 SPEED\n= SUCCESS\n  SPEED=100000\n> remove vlan v100\n= OBJECT_IN_USE\n";

    #[test]
    fn blocks_pair_by_position_and_differences_are_found() {
        let c = compare("02-x", CPP, RUST).unwrap();
        assert_eq!(c.blocks, 3);
        assert_eq!(c.diffs.len(), 2);
        assert_eq!(c.diffs[0].op, "switch");
        assert_eq!(c.diffs[1].op, "remove vlan v100");
        assert_eq!(c.diffs[1].first_difference(), "cpp = SUCCESS | rust = OBJECT_IN_USE");
        assert_eq!(c.diffs[0].changed_lines(), 1);
        let u = unified(&c.diffs[0]);
        assert!(u.contains("\n+  be ports() -> []\n"), "{u}");
        assert!(u.contains("\n   be init() -> SUCCESS\n"), "{u}");
        // differing echo lines are a hard failure
        assert!(compare("x", CPP, &RUST.replace("v100", "v200")).is_err());
        assert!(compare("x", CPP, "> switch\n= SUCCESS\n").is_err());
    }

    #[test]
    fn the_first_covering_entry_counts_and_unused_entries_are_stale() {
        let c = compare("02-x", CPP, RUST).unwrap();
        let entries = parse_allow(
            "# comment\n\n*\t^switch$\tprofile key\n02-x\t^remove vlan \tvlan removal\n03-y\t^remove\tother script\n*\t^switch\tshadowed\n",
        )
        .unwrap();
        let v = judge(&entries, &c.diffs);
        assert_eq!(v.covered_by, [Some(0), Some(1)]);
        assert_eq!(v.uses, [1, 1, 0, 0]);
        assert_eq!(v.stale(), [2, 3]);
        assert!(!v.green());
        let exact = parse_allow("*\t^switch$\ta\n02-x\tvlan v100\tb\n").unwrap();
        assert!(judge(&exact, &c.diffs).green());
        let none = judge(&[], &c.diffs);
        assert_eq!(none.uncovered(), [0, 1]);
        assert!(parse_allow("*\t(unclosed\treason\n").is_err());
        assert!(parse_allow("*\tno reason\n").is_err());
    }

    #[test]
    fn regenerating_keeps_reasons_and_escapes_the_op() {
        let c = compare("02-x", CPP, RUST).unwrap();
        let entries = parse_allow("*\t^switch\tthe profile key\n").unwrap();
        let text = regenerate(&entries, &c.diffs);
        let again = parse_allow(&text).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(again[0].reason, "the profile key");
        assert_eq!(again[1].pattern.as_str(), "^remove vlan v100$");
        assert!(again[1].reason.starts_with("TODO: cpp = SUCCESS"));
        assert!(judge(&again, &c.diffs).green());
        assert_eq!(regex::escape("get port port0 [a]"), "get port port0 \\[a\\]");
    }

    #[test]
    fn areas_are_object_types_or_operations() {
        assert_eq!(area("create router_interface rif0 TYPE=PORT"), "router_interface");
        assert_eq!(area("inject fdb learned 00:aa:bb:cc:dd:01 1 port2"), "inject");
        assert_eq!(area("switch"), "switch");
        assert_eq!(tally(["a", "b", "a"]).get("a"), Some(&2));
    }
}

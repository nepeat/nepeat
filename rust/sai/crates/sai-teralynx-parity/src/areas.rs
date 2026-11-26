//! The areas of the Teralynx backend (the C++ `src/tl_<area>.cpp` files) and who owns what the
//! comparisons see: a `test_ifcs` check (its file and line), a `test_ifcs` section, a driver op,
//! an oracle script. One file says it, `crates/sai-backend-teralynx/tests/areas.txt` (records below;
//! `tests/test-ifcs.sh` reads its `area` and `check` records too):
//!
//! ```text
//! area <name> <group> <C++ sources...>    group: g1 (ported in G1) or the G2 package's area group
//! check <test file>[:<first>[-<last>]] <area>   the last matching record wins
//! section <test_ifcs section> <area>[,<area>...]
//! object <driver op key> <area>           key: the object type of create / remove / set / get, or
//!                                         stats:<type>, clear:<type>, inject:<kind>, the op word; *
//! oracle <script stem> <area>[,<area>...]
//! step <oracle step op> <area>            a step of that op belongs to the area, not the script's
//! ```
//!
//! An area is PENDING while `tests/test_ifcs-expected.txt` still lists a FAIL line of it (the line's
//! check record, or its `# needs <area>` mark: a check that fails because ANOTHER area is not
//! ported yet). A g1 area is never pending. Differences in pending areas are expected while G2
//! runs; a difference in an area that is not pending is red (unless allow-listed).
#![forbid(unsafe_code)]

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// The group of the areas ported in G1 (never pending).
pub const G1: &str = "g1";

/// One area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Area {
    /// Its name (`l2`, `acl`, ...).
    pub name: String,
    /// Its group: [`G1`] or the G2 package's area group.
    pub group: String,
    /// The C++ sources it ports.
    pub sources: Vec<String>,
}

#[derive(Clone, Debug)]
struct CheckRule {
    file: String,
    range: Option<(u32, u32)>,
    area: String,
}

/// The parsed areas file.
#[derive(Clone, Debug, Default)]
pub struct Areas {
    by_name: BTreeMap<String, Area>,
    checks: Vec<CheckRule>,
    sections: BTreeMap<String, Vec<String>>,
    objects: BTreeMap<String, String>,
    oracle: BTreeMap<String, Vec<String>>,
    steps: BTreeMap<String, String>,
}

fn list(s: &str) -> Vec<String> {
    s.split(',').filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

impl Areas {
    /// Parses the file; every area a record names must have its `area` record.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut a = Self::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let at = |what: &str| format!("areas line {}: {what}: {raw}", n + 1);
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.as_slice() {
                ["area", name, group, sources @ ..] => {
                    let area = Area {
                        name: (*name).to_owned(),
                        group: (*group).to_owned(),
                        sources: sources.iter().map(|s| (*s).to_owned()).collect(),
                    };
                    if a.by_name.insert((*name).to_owned(), area).is_some() {
                        return Err(at("area named twice"));
                    }
                }
                ["check", spec, area] => {
                    let (file, range) = match spec.split_once(':') {
                        None => ((*spec).to_owned(), None),
                        Some((f, r)) => {
                            let (lo, hi) = r.split_once('-').unwrap_or((r, r));
                            let lo: u32 = lo.parse().map_err(|_| at("bad line number"))?;
                            let hi: u32 = hi.parse().map_err(|_| at("bad line number"))?;
                            if hi < lo {
                                return Err(at("empty line range"));
                            }
                            (f.to_owned(), Some((lo, hi)))
                        }
                    };
                    a.checks.push(CheckRule {
                        file,
                        range,
                        area: (*area).to_owned(),
                    });
                }
                ["section", name, areas] => {
                    a.sections.insert((*name).to_owned(), list(areas));
                }
                ["object", key, area] => {
                    a.objects.insert((*key).to_owned(), (*area).to_owned());
                }
                ["oracle", stem, areas] => {
                    a.oracle.insert((*stem).to_owned(), list(areas));
                }
                ["step", op, area] => {
                    a.steps.insert((*op).to_owned(), (*area).to_owned());
                }
                _ => return Err(at("unknown record")),
            }
        }
        let named = a
            .checks
            .iter()
            .map(|c| &c.area)
            .chain(a.objects.values())
            .chain(a.steps.values())
            .chain(a.sections.values().flatten())
            .chain(a.oracle.values().flatten());
        for area in named {
            if !a.by_name.contains_key(area) {
                return Err(format!("areas: '{area}' is used but has no area record"));
            }
        }
        Ok(a)
    }

    /// Every area, by name.
    pub fn all(&self) -> impl Iterator<Item = &Area> {
        self.by_name.values()
    }

    /// The area of that name.
    pub fn get(&self, name: &str) -> Option<&Area> {
        self.by_name.get(name)
    }

    /// The area of the check at `file:line` (the last matching record).
    pub fn of_check(&self, file: &str, line: u32) -> Option<&str> {
        self.checks
            .iter()
            .rev()
            .find(|r| r.file == file && r.range.is_none_or(|(lo, hi)| (lo..=hi).contains(&line)))
            .map(|r| r.area.as_str())
    }

    /// The areas of a `test_ifcs` section (empty: not in the file).
    pub fn of_section(&self, section: &str) -> &[String] {
        self.sections.get(section).map_or(&[], Vec::as_slice)
    }

    /// The area of a driver op line (without its `> `).
    pub fn of_op(&self, op: &str) -> Option<&str> {
        let key = op_key(op);
        self.objects
            .get(&key)
            .or_else(|| self.objects.get("*"))
            .map(String::as_str)
    }

    /// The areas of an oracle script (empty: not in the file).
    pub fn of_oracle(&self, stem: &str) -> &[String] {
        self.oracle.get(stem).map_or(&[], Vec::as_slice)
    }

    /// The area an oracle step's op belongs to (`step` records), if not the script's.
    pub fn of_step_op(&self, op: &str) -> Option<&str> {
        self.steps.get(op).map(String::as_str)
    }

    /// The area of a FAIL line: its `# needs` mark, else its check record.
    pub fn of_fail(&self, f: &Fail) -> Option<String> {
        f.needs
            .clone()
            .or_else(|| self.of_check(&f.file, f.line).map(str::to_owned))
    }

    /// The pending areas: those an expected-FAIL list still names (never a g1 one).
    pub fn pending(&self, expected: &str) -> BTreeSet<String> {
        expected
            .lines()
            .filter_map(Fail::parse)
            .filter_map(|f| self.of_fail(&f))
            .filter(|a| self.get(a).is_some_and(|x| x.group != G1))
            .collect()
    }
}

/// The key of a driver op for the `object` records.
pub fn op_key(op: &str) -> String {
    let mut w = op.split_whitespace();
    match (w.next(), w.next()) {
        (Some("create" | "remove" | "set" | "get"), Some(ty)) => ty.to_owned(),
        (Some(kind @ ("stats" | "clear" | "inject")), Some(ty)) => format!("{kind}:{ty}"),
        (Some(word), _) => word.to_owned(),
        (None, _) => String::new(),
    }
}

/// One FAIL line of `test_ifcs` (its stderr, or a line of the expected list).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    /// The test file (`t_l2.cpp`).
    pub file: String,
    /// Its line.
    pub line: u32,
    /// The line as `tests/test-ifcs.sh` compares it: `FAIL <file>:<line>: <expression>` (no directory,
    /// no compared values of a `CHECK_EQ`, no mark).
    pub text: String,
    /// The `# needs <area>` mark of an expected line.
    pub needs: Option<String>,
}

static NEEDS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[ \t]+#[ \t]*needs[ \t]*:?[ \t]*([A-Za-z0-9_-]+)[ \t]*$").ok());
static VALUES: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r" \([^()]* vs [^()]*\)$").ok());
static HEAD: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^FAIL (?:[^ \t]*/)?([A-Za-z0-9_]+\.cpp):([0-9]+): ").ok());

impl Fail {
    /// Parses a FAIL line; `None` for anything else.
    pub fn parse(raw: &str) -> Option<Self> {
        let mut s = raw.trim_end().to_owned();
        let mut needs = None;
        if let Some(c) = NEEDS.as_ref().and_then(|re| re.captures(&s)) {
            needs = c.get(1).map(|m| m.as_str().to_owned());
            let cut = c.get(0).map_or(s.len(), |m| m.start());
            s.truncate(cut);
        }
        if let Some(m) = VALUES.as_ref().and_then(|re| re.find(&s)) {
            let cut = m.start();
            s.truncate(cut);
        }
        let c = HEAD.as_ref()?.captures(&s)?;
        let file = c.get(1)?.as_str().to_owned();
        let line: u32 = c.get(2)?.as_str().parse().ok()?;
        let rest = s.get(c.get(0)?.end()..)?;
        Some(Self {
            text: format!("FAIL {file}:{line}: {rest}"),
            file,
            line,
            needs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "area switch g1 tl_switch.cpp\narea l2 l2-lag tl_l2.cpp\narea acl hostif-copp-acl tl_acl.cpp\n\
check t_l2.cpp l2\ncheck t_counters.cpp switch\ncheck t_counters.cpp:93 acl # the diag acl command\n\
section l2 l2\nsection hooks l2,acl\nobject vlan l2\nobject stats:port switch\nobject * switch\noracle acl acl\nstep qstats l2\n";

    #[test]
    fn records_and_their_lookups() {
        let a = Areas::parse(FILE).unwrap();
        assert_eq!(a.of_check("t_l2.cpp", 5), Some("l2"));
        assert_eq!(a.of_check("t_counters.cpp", 92), Some("switch"));
        assert_eq!(a.of_check("t_counters.cpp", 93), Some("acl"));
        assert_eq!(a.of_check("t_x.cpp", 1), None);
        assert_eq!(a.of_section("hooks"), ["l2", "acl"]);
        assert_eq!(a.of_section("nope"), [] as [String; 0]);
        assert_eq!(a.of_op("create vlan v100 VLAN_ID=100"), Some("l2"));
        assert_eq!(a.of_op("stats port port0 IF_IN_OCTETS"), Some("switch"));
        assert_eq!(a.of_op("linkscan"), Some("switch"));
        assert_eq!(a.of_oracle("acl"), ["acl"]);
        assert_eq!(a.of_step_op("qstats"), Some("l2"));
        assert_eq!(a.of_step_op("get"), None);
        assert_eq!(a.get("l2").map(|x| x.group.as_str()), Some("l2-lag"));
    }

    #[test]
    fn an_unknown_area_or_record_is_refused() {
        assert!(Areas::parse("check t_l2.cpp l2\n").is_err());
        assert!(Areas::parse("area l2 x\nbogus 1 2\n").is_err());
        assert!(Areas::parse("area l2 x\narea l2 y\n").is_err());
        assert!(Areas::parse("area l2 x\ncheck t.cpp:9-3 l2\n").is_err());
    }

    #[test]
    fn fail_lines_normalise_as_the_runner_does() {
        let f = Fail::parse(
            "FAIL /src/tests/t_l2.cpp:62: (va->create_vlan(&vlan, s, 1, &a)) == SAI_STATUS_SUCCESS (-5 vs 0)",
        )
        .unwrap();
        assert_eq!(f.file, "t_l2.cpp");
        assert_eq!(f.line, 62);
        assert_eq!(
            f.text,
            "FAIL t_l2.cpp:62: (va->create_vlan(&vlan, s, 1, &a)) == SAI_STATUS_SUCCESS"
        );
        let m = Fail::parse("FAIL t_lag.cpp:111: x == y\t# needs l3").unwrap();
        assert_eq!(m.needs.as_deref(), Some("l3"));
        assert_eq!(m.text, "FAIL t_lag.cpp:111: x == y");
        assert!(Fail::parse("END aborted").is_none());
        assert!(Fail::parse("# FAIL t_l2.cpp:1: x").is_none());
    }

    #[test]
    fn pending_areas_come_from_the_expected_list() {
        let a = Areas::parse(FILE).unwrap();
        let p =
            a.pending("END x\nFAIL t_l2.cpp:1: a\nFAIL t_counters.cpp:92: b\nFAIL t_counters.cpp:5: c  # needs acl\n");
        assert_eq!(p.into_iter().collect::<Vec<_>>(), ["acl", "l2"]);
    }
}

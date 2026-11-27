//! `test_ifcs`'s checks per section (the counts file `PARITY_COUNTS_FILE` of the program that runs them:
//! `section <name> checks <n> failed <n>[ incomplete]` per line; the C++ `c/sections.cpp` wrote it, deleted in H5)
//! and its FAIL lines: the gate value "Rust passes N of M" (M: the golden C++ reference's checks), every
//! failure with its area.
#![forbid(unsafe_code)]

use crate::areas::Fail;

/// One section's counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    /// The section.
    pub name: String,
    /// Checks run.
    pub checks: u32,
    /// Checks failed.
    pub failed: u32,
    /// The program died inside it.
    pub incomplete: bool,
}

/// Parses the counts file (`section <name> checks <n> failed <n>[ incomplete]`).
pub fn parse_counts(text: &str) -> Result<Vec<Count>, String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["section", name, "checks", c, "failed", f, rest @ ..] => out.push(Count {
                name: (*name).to_owned(),
                checks: c.parse().map_err(|_| format!("counts: {line}"))?,
                failed: f.parse().map_err(|_| format!("counts: {line}"))?,
                incomplete: rest == ["incomplete"],
            }),
            _ => return Err(format!("counts: {line}")),
        }
    }
    Ok(out)
}

/// The FAIL lines of a run's stderr, normalised ([`Fail::parse`]).
pub fn fails(stderr: &str) -> Vec<Fail> {
    stderr.lines().filter_map(Fail::parse).collect()
}

/// One section of the gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The section.
    pub name: String,
    /// The C++ reference's checks there.
    pub reference: u32,
    /// Checks the Rust run passed there, at most `reference`.
    pub passed: u32,
    /// Checks the Rust run failed there.
    pub failed: u32,
    /// Checks of the reference the Rust run did not reach (it died here or before).
    pub not_run: u32,
    /// Checks the Rust run made beyond the reference's (a loop that went on).
    pub extra: u32,
    /// The Rust run died inside it.
    pub died: bool,
}

/// The gate: per section and in total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    /// The sections, in the reference's order.
    pub rows: Vec<Row>,
}

impl Gate {
    /// Every section of `reference`, with what `rust` did there.
    pub fn new(reference: &[Count], rust: &[Count]) -> Self {
        let rows = reference
            .iter()
            .map(|c| match rust.iter().find(|r| r.name == c.name) {
                Some(r) => {
                    let passed = r.checks.saturating_sub(r.failed).min(c.checks);
                    Row {
                        name: c.name.clone(),
                        reference: c.checks,
                        passed,
                        failed: r.failed,
                        not_run: c.checks.saturating_sub(r.checks),
                        extra: r.checks.saturating_sub(c.checks),
                        died: r.incomplete,
                    }
                }
                None => Row {
                    name: c.name.clone(),
                    reference: c.checks,
                    passed: 0,
                    failed: 0,
                    not_run: c.checks,
                    extra: 0,
                    died: false,
                },
            })
            .collect();
        Self { rows }
    }

    /// The reference's checks.
    pub fn total(&self) -> u32 {
        self.rows.iter().map(|r| r.reference).sum()
    }

    /// The checks the Rust run passed.
    pub fn passed(&self) -> u32 {
        self.rows.iter().map(|r| r.passed).sum()
    }

    /// The first section the Rust run did not finish (died in, or never reached).
    pub fn stopped_at(&self) -> Option<&str> {
        self.rows
            .iter()
            .find(|r| r.died || (r.not_run > 0 && r.not_run == r.reference))
            .map(|r| r.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_the_gate() {
        let c = parse_counts(
            "section main checks 20 failed 0\nsection l2 checks 42 failed 0\nsection hooks checks 119 failed 0\nsection tail checks 4 failed 0\n",
        )
        .unwrap();
        let r = parse_counts("section main checks 20 failed 0\nsection l2 checks 45 failed 33\nsection hooks checks 15 failed 11 incomplete\n").unwrap();
        assert!(r[2].incomplete && !r[1].incomplete);
        let g = Gate::new(&c, &r);
        assert_eq!(g.total(), 185);
        assert_eq!(g.passed(), 20 + 12 + 4);
        assert_eq!(g.rows[1].extra, 3);
        assert_eq!(g.rows[2].not_run, 104);
        assert_eq!(g.rows[3].not_run, 4);
        assert_eq!(g.stopped_at(), Some("hooks"));
        assert!(parse_counts("section x checks y failed 0\n").is_err());
    }

    #[test]
    fn fail_lines_of_a_stderr() {
        let f = fails("sai-teralynx: noise\nFAIL /a/tests/t_l2.cpp:62: x == y (1 vs 2)\n--- mock log at failure ---\n");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].text, "FAIL t_l2.cpp:62: x == y");
    }
}

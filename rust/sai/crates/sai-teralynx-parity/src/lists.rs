//! The two hand-kept lists of the harness (`tests/`), both in the frontend differential's format
//! (`sai_driver::compare`: `<scope> TAB <regex> TAB ...`; `#` comments):
//!
//! - `parity-allow.txt`: DELIBERATE differences, `<scope> TAB <regex> TAB <reason>`; empty by
//!   default. An entry covers a unit of its scope whose name the regex matches; an entry that covers
//!   nothing is stale (red).
//! - `parity-pending.txt`: differences that belong to ANOTHER area than the unit's own, while that
//!   area is pending: `<scope> TAB <regex> TAB <area> TAB <why>` (e.g. the switch create of a script
//!   programs the FDB aging time, which is the L2 area's). Inactive once the area is no longer
//!   pending (the difference is then judged as the unit's own: red); one that covers nothing is
//!   reported (the area's owner removes it).
//!
//! Scopes: a driver script's stem (`01-switch`), `test_ifcs` (units: sections), `test_ifcs-check`
//! (units: normalised FAIL lines), `oracle/<stem>` (units: step tags), or `*`.
#![forbid(unsafe_code)]

use regex::Regex;
use sai_driver::compare::{self, Entry};

/// A pending attribution.
#[derive(Clone, Debug)]
pub struct Pending {
    /// The entry (scope, regex; its reason: the why).
    pub entry: Entry,
    /// The pending area the difference belongs to.
    pub area: String,
}

/// Parses `parity-allow.txt`.
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    compare::parse_allow(text)
}

/// Parses `parity-pending.txt`.
pub fn parse_pending(text: &str) -> Result<Vec<Pending>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let (Some(scope), Some(re), Some(area), Some(why)) = (f.first(), f.get(1), f.get(2), f.get(3)) else {
            return Err(format!(
                "pending line {}: want <scope> TAB <regex> TAB <area> TAB <why>: {line}",
                i + 1
            ));
        };
        let pattern = Regex::new(re).map_err(|e| format!("pending line {}: {e}", i + 1))?;
        if area.trim().is_empty() || why.trim().is_empty() {
            return Err(format!("pending line {}: empty area or why", i + 1));
        }
        out.push(Pending {
            entry: Entry {
                script: (*scope).to_owned(),
                pattern,
                reason: (*why).to_owned(),
                line: i + 1,
            },
            area: area.trim().to_owned(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_entries() {
        let p = parse_pending("# c\n01-switch\t^switch \tl2\tthe aging time at create (tl_l2)\n").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].area, "l2");
        assert!(p[0].entry.covers("01-switch", "switch SRC_MAC_ADDRESS=1"));
        assert!(!p[0].entry.covers("02-ports", "switch"));
        assert!(parse_pending("a\tb\tc\n").is_err());
        assert!(parse_pending("a\t(\tc\td\n").is_err());
        assert!(parse_allow("").unwrap().is_empty());
    }
}

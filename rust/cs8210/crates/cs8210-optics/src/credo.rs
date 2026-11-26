//! The Credo CMS50216 loader's `-m` spec (vendor `c-phy/auto_load_user.sh`) with each
//! `MACsec` cage's line-side media taken from the module in it. Pure functions; the CLI
//! (`cs8210-optics credo-spec`) asks the daemon for its status and prints the result.
//!
//! Spec: `;`-separated `CAGE:MEDIA_MODE[:REST]`, e.g. `3:copper_1x400G:fec-def`. A passive
//! copper cable (CMIS / SFF-8024 media type 03h) -> `copper` (line link training), anything
//! else -> `optics` (no AN/LT); empty or unknown cages keep the given entry.

use std::collections::BTreeMap;

use serde_json::Value;

/// Line media per `MACsec` cage (1-16) from the daemon's `status` answer, and the cages whose
/// module is present and out of reset but not identified yet.
#[must_use]
pub fn media(status: &Value) -> (BTreeMap<u64, &'static str>, Vec<u64>) {
    let mut known = BTreeMap::new();
    let mut pending = Vec::new();
    for c in status.get("cages").and_then(Value::as_array).into_iter().flatten() {
        let n = c.get("cage").and_then(Value::as_u64).unwrap_or(0);
        if !(1..=16).contains(&n)
            || c.get("present") != Some(&Value::Bool(true))
            || c.get("reset") == Some(&Value::Bool(true))
        {
            continue;
        }
        match (
            c.pointer("/identity/media_type_code").and_then(Value::as_u64),
            c.pointer("/bringup/state/phase").and_then(Value::as_str),
        ) {
            (Some(m), _) => {
                known.insert(n, if m == 3 { "copper" } else { "optics" });
            }
            (None, Some("failed")) => {}
            _ => pending.push(n),
        }
    }
    (known, pending)
}

/// `spec` with each listed cage's media replaced by the one in `media`; also one note per
/// change. Errors on a malformed entry or characters a shell or the loader would misread.
pub fn rewrite(spec: &str, media: &BTreeMap<u64, &str>) -> Result<(String, Vec<String>), String> {
    if !spec.chars().all(|c| c.is_ascii_alphanumeric() || ":;_-".contains(c)) {
        return Err(format!("spec {spec:?}: unexpected characters"));
    }
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for entry in spec.split(';').filter(|e| !e.is_empty()) {
        let mut parts = entry.splitn(3, ':');
        let (Some(cage), Some(mode)) = (parts.next(), parts.next()) else {
            return Err(format!("entry {entry:?}"));
        };
        let rest = parts.next();
        let new_mode = match (
            cage.parse::<u64>().ok().and_then(|n| media.get(&n)),
            mode.split_once('_'),
        ) {
            (Some(m), Some((old, tail))) if *m != old => {
                notes.push(format!("cage {cage}: {old} -> {m} (the module in it)"));
                format!("{m}_{tail}")
            }
            _ => mode.to_string(),
        };
        out.push(rest.map_or_else(|| format!("{cage}:{new_mode}"), |r| format!("{cage}:{new_mode}:{r}")));
    }
    Ok((out.join(";"), notes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn media_from_status() {
        let s = json!({"cages": [
            {"cage": 1, "present": true, "reset": false, "identity": {"media_type_code": 2}},
            {"cage": 3, "present": true, "reset": false, "identity": {"media_type_code": 3}},
            {"cage": 4, "present": true, "reset": false, "bringup": {"state": {"phase": "identify"}}},
            {"cage": 5, "present": true, "reset": false, "bringup": {"state": {"phase": "failed"}}},
            {"cage": 6, "present": true, "reset": true},
            {"cage": 17, "present": true, "reset": false, "identity": {"media_type_code": 3}},
        ]});
        let (m, pending) = media(&s);
        assert_eq!(m.into_iter().collect::<Vec<_>>(), [(1, "optics"), (3, "copper")]);
        assert_eq!(pending, [4]);
    }

    #[test]
    fn spec_follows_the_modules() {
        let m = BTreeMap::from([(1, "optics"), (3, "copper"), (2, "optics")]);
        let (s, notes) = rewrite(
            "1:copper_1x400G:fec-def;2:optics_1x400G;3:optics_1x400G:fec-def;4:copper_1x400G",
            &m,
        )
        .unwrap();
        assert_eq!(
            s,
            "1:optics_1x400G:fec-def;2:optics_1x400G;3:copper_1x400G:fec-def;4:copper_1x400G"
        );
        assert_eq!(notes.len(), 2);
        assert!(rewrite("1:optics_1x400G;rm -rf", &m).is_err());
        assert!(rewrite("1", &m).is_err());
    }
}

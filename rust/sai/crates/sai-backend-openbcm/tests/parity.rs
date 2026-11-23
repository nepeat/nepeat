//! The error table against the C++ backend's: reads `map_rv` from sai-openbcm's
//! `openbcm_backend.cpp` (directory in `SAI_OPENBCM_SRC`, or `MDIO_SCRIPT_C_DIR` when the file
//! is there) and compares every glue code. Skips with a message when the source is not around
//! (a switch, a build that only has the MDIO sources).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: helpers of tests are not exempted by clippy.toml"
)]

mod common;

use common::note;
use sai_backend_openbcm::status::sai_error;
use sai_sys as sys;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The glue's codes by name (`sdk_glue.h`).
const CODES: [(&str, i32); 19] = [
    ("OB_E_INTERNAL", -1),
    ("OB_E_MEMORY", -2),
    ("OB_E_UNIT", -3),
    ("OB_E_PARAM", -4),
    ("OB_E_EMPTY", -5),
    ("OB_E_FULL", -6),
    ("OB_E_NOT_FOUND", -7),
    ("OB_E_EXISTS", -8),
    ("OB_E_TIMEOUT", -9),
    ("OB_E_BUSY", -10),
    ("OB_E_FAIL", -11),
    ("OB_E_DISABLED", -12),
    ("OB_E_BADID", -13),
    ("OB_E_RESOURCE", -14),
    ("OB_E_CONFIG", -15),
    ("OB_E_UNAVAIL", -16),
    ("OB_E_INIT", -17),
    ("OB_E_PORT", -18),
    ("OB_E_MDIO_CLOCK", -100),
];

fn status(name: &str) -> sys::sai_status_t {
    match name {
        "SAI_STATUS_FAILURE" => sys::SAI_STATUS_FAILURE,
        "SAI_STATUS_NO_MEMORY" => sys::SAI_STATUS_NO_MEMORY,
        "SAI_STATUS_INVALID_PARAMETER" => sys::SAI_STATUS_INVALID_PARAMETER,
        "SAI_STATUS_ITEM_NOT_FOUND" => sys::SAI_STATUS_ITEM_NOT_FOUND,
        "SAI_STATUS_TABLE_FULL" => sys::SAI_STATUS_TABLE_FULL,
        "SAI_STATUS_ITEM_ALREADY_EXISTS" => sys::SAI_STATUS_ITEM_ALREADY_EXISTS,
        "SAI_STATUS_OBJECT_IN_USE" => sys::SAI_STATUS_OBJECT_IN_USE,
        "SAI_STATUS_INVALID_OBJECT_ID" => sys::SAI_STATUS_INVALID_OBJECT_ID,
        "SAI_STATUS_INSUFFICIENT_RESOURCES" => sys::SAI_STATUS_INSUFFICIENT_RESOURCES,
        "SAI_STATUS_NOT_SUPPORTED" => sys::SAI_STATUS_NOT_SUPPORTED,
        "SAI_STATUS_UNINITIALIZED" => sys::SAI_STATUS_UNINITIALIZED,
        "SAI_STATUS_INVALID_PORT_NUMBER" => sys::SAI_STATUS_INVALID_PORT_NUMBER,
        other => panic!("map_rv returns {other}, which this test does not know"),
    }
}

/// Codes this backend maps differently from the C++: (code, what the C++ returns, what we
/// return). None.
const DIFFERENCES: [(&str, &str, &str); 0] = [];

fn source() -> Option<String> {
    ["SAI_OPENBCM_SRC", "MDIO_SCRIPT_C_DIR"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join("openbcm_backend.cpp"))
        .find_map(|path| std::fs::read_to_string(path).ok())
}

/// `case A: case B: return STATUS;` lines of `map_rv`, and its default.
fn cpp_table(text: &str) -> (BTreeMap<String, String>, String) {
    let start = text
        .find("static sai_status_t map_rv(int rv)")
        .expect("map_rv in openbcm_backend.cpp");
    let body = &text[start..];
    let body = &body[..body.find("\n}\n").expect("end of map_rv")];
    let mut table = BTreeMap::new();
    let mut default = None;
    for line in body.lines() {
        let line = line.trim();
        let Some((cases, ret)) = line.split_once("return ") else {
            continue;
        };
        let ret = ret.trim_end_matches(';').to_owned();
        if cases.starts_with("default:") {
            default = Some(ret);
            continue;
        }
        for case in cases.split("case ").filter(|c| !c.trim().is_empty()) {
            table.insert(case.trim().trim_end_matches(':').to_owned(), ret.clone());
        }
    }
    (table, default.expect("map_rv has a default"))
}

#[test]
fn error_codes_map_as_in_the_cpp_backend() {
    let Some(text) = source() else {
        note("SKIPPED: no openbcm_backend.cpp (set SAI_OPENBCM_SRC to sai-openbcm's src directory)");
        return;
    };
    let (mut table, default) = cpp_table(&text);
    assert_eq!(
        table.remove_entry("OB_E_NONE").map(|(_, v)| v).as_deref(),
        Some("SAI_STATUS_SUCCESS")
    );
    // every case of the C++ switch is a code this test walks
    for name in table.keys() {
        assert!(
            CODES.iter().any(|(n, _)| n == name),
            "map_rv has a case for {name}, unknown here"
        );
    }
    let mut compared = 0;
    for (name, raw) in CODES {
        let cpp = table.get(name).unwrap_or(&default);
        let ours = sai_error(sdk_glue::Error::from_raw(raw).unwrap()).status();
        match DIFFERENCES.iter().find(|(n, _, _)| *n == name) {
            Some((_, cpp_expected, ours_expected)) => {
                assert_eq!(cpp, cpp_expected, "{name}: the C++ changed, revisit the difference");
                assert_eq!(ours, status(ours_expected), "{name}");
            }
            None => assert_eq!(ours, status(cpp), "{name}: C++ returns {cpp}"),
        }
        compared += 1;
    }
    // codes the SDK may return that have no name
    assert_eq!(
        sai_error(sdk_glue::Error::from_raw(-57).unwrap()).status(),
        status(&default)
    );
    note(&format!(
        "{compared} comparisons with openbcm_backend.cpp map_rv, {} listed differences",
        DIFFERENCES.len()
    ));
}

#[test]
fn the_cpp_reads_the_same_profile_keys() {
    let Some(text) = source() else {
        note("SKIPPED: no openbcm_backend.cpp (set SAI_OPENBCM_SRC to sai-openbcm's src directory)");
        return;
    };
    for key in [
        sai_backend_openbcm::CONFIG_KEY,
        sai_backend_openbcm::RC_KEY,
        sai_backend_openbcm::UNIT_KEY,
    ] {
        assert!(
            text.contains(&format!("cfg.get(\"{key}\"")),
            "openbcm_backend.cpp does not read {key}"
        );
    }
}

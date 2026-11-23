//! The error table against the C++ backend's, from a golden: `tests/golden/parity-map-rv.txt` is
//! `map_rv` of sai-openbcm's `openbcm_backend.cpp` for every code of `sdk_glue.h` (19 codes,
//! success, the default) and the profile keys that backend reads, captured at commit 50a6aa7
//! before the C++ was deleted (the file's header says how). Every code is compared with this
//! backend's `sai_error`; nothing is read at run time (the golden is compiled in), so the test
//! also runs on the switch.
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

/// The C++ table as captured (see its header); edited by hand only, never from the Rust side.
const GOLDEN: &str = include_str!("golden/parity-map-rv.txt");

fn status(name: &str) -> sys::sai_status_t {
    match name {
        "SAI_STATUS_SUCCESS" => sys::SAI_STATUS_SUCCESS,
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

/// The golden's rows.
struct Golden {
    /// (name, value, the C++ status) per glue error code
    codes: Vec<(String, i32, String)>,
    /// (name, value, status) of the success code
    success: (String, i32, String),
    /// `map_rv`'s answer for a code it has no case for
    default: String,
    /// the keys `openbcm_backend.cpp` reads from the profile
    keys: Vec<String>,
}

fn golden() -> Golden {
    let (mut codes, mut success, mut default, mut keys) = (Vec::new(), None, None, Vec::new());
    for (n, line) in GOLDEN.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let row = |w: &[&str]| -> (String, i32, String) {
            match w {
                [name, value, status] => (
                    (*name).to_owned(),
                    value
                        .parse()
                        .unwrap_or_else(|e| panic!("golden line {}: {value}: {e}", n + 1)),
                    (*status).to_owned(),
                ),
                _ => panic!("golden line {}: {line}", n + 1),
            }
        };
        match words.as_slice() {
            ["code", rest @ ..] => codes.push(row(rest)),
            ["success", rest @ ..] => success = Some(row(rest)),
            ["default", status] => default = Some((*status).to_owned()),
            ["profile-key", key] => keys.push((*key).to_owned()),
            _ => panic!("golden line {}: {line}", n + 1),
        }
    }
    Golden {
        codes,
        success: success.expect("a success row in the golden"),
        default: default.expect("a default row in the golden"),
        keys,
    }
}

#[test]
fn error_codes_map_as_in_the_cpp_backend() {
    let g = golden();
    assert_eq!(g.codes.len(), 19, "the golden has the 19 error codes of sdk_glue.h");
    // a success is not an error: the backend never maps it
    assert_eq!(
        (g.success.0.as_str(), g.success.1, g.success.2.as_str()),
        ("OB_E_NONE", 0, "SAI_STATUS_SUCCESS")
    );
    assert!(sdk_glue::Error::from_raw(g.success.1).is_none());
    let mut compared = 0;
    for (name, raw, cpp) in &g.codes {
        let error = sdk_glue::Error::from_raw(*raw).unwrap_or_else(|| panic!("{name} = {raw} is not an error code"));
        let ours = sai_error(error).status();
        match DIFFERENCES.iter().find(|(n, _, _)| n == name) {
            Some((_, cpp_expected, ours_expected)) => {
                assert_eq!(
                    cpp, cpp_expected,
                    "{name}: the golden and the listed difference disagree"
                );
                assert_eq!(ours, status(ours_expected), "{name}");
            }
            None => assert_eq!(ours, status(cpp), "{name}: the C++ returned {cpp} (golden)"),
        }
        compared += 1;
    }
    // codes the SDK may return that have no name
    assert!(g.codes.iter().all(|(_, raw, _)| *raw != -57));
    assert_eq!(
        sai_error(sdk_glue::Error::from_raw(-57).unwrap()).status(),
        status(&g.default)
    );
    note(&format!(
        "{compared} comparisons with openbcm_backend.cpp map_rv (golden of 50a6aa7), {} listed differences",
        DIFFERENCES.len()
    ));
}

#[test]
fn the_cpp_reads_the_same_profile_keys() {
    let mut cpp = golden().keys;
    cpp.sort();
    let mut ours = vec![
        sai_backend_openbcm::CONFIG_KEY,
        sai_backend_openbcm::RC_KEY,
        sai_backend_openbcm::UNIT_KEY,
    ];
    ours.sort_unstable();
    assert_eq!(
        cpp, ours,
        "the profile keys of openbcm_backend.cpp (golden of 50a6aa7) and this backend's"
    );
}

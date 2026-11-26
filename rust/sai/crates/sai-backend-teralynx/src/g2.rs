//! The operations of the areas not ported yet (G2 of the plan: L2 + LAG, L3, host interfaces +
//! `CoPP` + ACL + policers, `QoS` + mirror + sampling + `sFlow`): each answers `NotSupported`, said once
//! per operation in the log, so nothing silently pretends to program the chip. The C++ backend
//! implements every one of them (`ifcs_backend.cpp`); the probes (`qos_supported`, ...) answer as
//! the C++ does, so the SAI layer takes the same paths and a missing port shows as an error.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use sai_core::error::Error;
use sai_core::log;
use sai_sys as ssys;

static SAID: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// `NotSupported` for `op`, warned once per process.
pub fn not_ported(op: &'static str) -> Error {
    if SAID.lock().unwrap_or_else(PoisonError::into_inner).insert(op) {
        log::warn(
            ssys::SAI_API_SWITCH,
            format_args!("teralynx backend: {op} is not ported to Rust yet (G2): NOT_SUPPORTED"),
        );
    }
    Error::NotSupported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_as_not_supported() {
        assert_eq!(not_ported("x"), Error::NotSupported);
        assert_eq!(not_ported("x"), Error::NotSupported);
    }
}

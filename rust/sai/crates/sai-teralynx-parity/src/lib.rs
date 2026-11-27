//! The G2 parity harness of the Teralynx backend (README "Parity", "Teralynx backend"): the Rust libsai
//! of the CS8210 (sai-ffi with the teralynx feature) on the mock libifcs (`rust/sai/tests/teralynx-mock`,
//! which records every IFCS call), judged against the golden C++ side: what the C++ libsai
//! (the C++ sai-teralynx package, deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0) gave on
//! the same mock for the same programs, captured once into `tests/golden/` (its outputs ARE the goldens; they
//! are frozen). Programs, each part compared per unit:
//!
//! | part | program | compared per |
//! |---|---|---|
//! | checks | `test_ifcs` clients (`bin/test_ifcs-sections`: none built since H5) | section: checks run / failed; FAIL lines with their area |
//! | sections | the same run's IFCS call stream | section: the IFCS call lines |
//! | driver | `bin/tl-frontend-driver` (none built since H5), sai-driver's `driver-scripts/*.ops` | op: status, values, IFCS lines, notifications |
//! | oracle | the Rust `sai-script --cpp-names`, the oracle's `scripts/*.json` (SAI steps) | step: IFCS lines, status, values |
//!
//! The C and C++ beside them (`c/`, built by `build-fixtures.sh`): `tap.c` (every mock line into one
//! stream, also across the tests' log clears), `oracle_prep.cpp` (a JSON script without its frame-sender
//! steps), `bytes_rt.c` (the byte tap: the oracle tracer's generated wrappers, `gen_tracer.py` of
//! ai_re/nix/packages/ifcs-oracle, over a runtime that writes ALL bytes of every struct and attribute argument
//! of every IFCS call into the stream as `@B` lines; loaded as `SAI_TERALYNX_IFCS_TRACE` by the libsai, so the
//! parts compare what libifcs receives byte for byte, padding included, where the mock compares fields).
//! [`run`] runs and judges; [`areas`] says what is pending; [`golden`] reads the goldens.
#![forbid(unsafe_code)]

pub mod areas;
pub mod checks;
pub mod golden;
pub mod lists;
pub mod oracle;
pub mod run;
pub mod sha256;
pub mod stream;

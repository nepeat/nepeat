//! The G2 parity harness of the Teralynx backend (README "Parity", "Teralynx backend"): the C++
//! libsai of the CS8210 (ai_re/nix/packages/sai-teralynx, the specification) and the Rust one
//! (sai-ffi with the teralynx feature), each on the SAME mock libifcs (sai-teralynx's
//! `tests/mock`, which records every IFCS call), driven by the same unmodified programs, compared:
//!
//! | part | program (unmodified) | compared per |
//! |---|---|---|
//! | checks | `test_ifcs` (`tests/test_ifcs.cpp` + `t_*.cpp`) | section: checks run / failed; FAIL lines with their area |
//! | sections | the same run, `TEST_IFCS_DUMP=1` | section: the IFCS call lines |
//! | driver | the frozen frontend's `frontend_driver.cpp`, its `driver-scripts/*.ops` | op: status, values, IFCS lines, notifications |
//! | oracle | the oracle's runner `sai_script.cpp`, `oracle/scripts/*.json` (SAI steps) | step: IFCS lines, status, values |
//!
//! The C and C++ beside them (`c/`, built by `build-fixtures.sh`): `tap.c` (every mock line into one
//! stream, also across the tests' log clears), `sections.cpp` (`test_ifcs`'s areas marked and counted
//! through `-Wl,--wrap`), `tl_port.cpp` (the driver's test port over the mock's hooks),
//! `oracle_prep.cpp` (a JSON script without its frame-sender steps). Nothing of the compared
//! programs or the mock is changed. [`run`] runs and judges; [`areas`] says what is pending.
#![forbid(unsafe_code)]

pub mod areas;
pub mod checks;
pub mod lists;
pub mod oracle;
pub mod run;
pub mod stream;

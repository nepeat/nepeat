//! Hardware test frame senders and counters for the CS8210 SAI tests: the Rust ports of
//! `tools/{trap,qos,raw,l3}_frames.c` and `tools/sflow_probe.c` of the C++ sai-teralynx package, with the
//! same command lines, output text and exit codes (the oracle's `hw:*` scripts call them by name and read
//! their output).
//!
//! - one module per tool (`trap`, `qos`, `raw`, `l3`, `sflow`): frame building, counting and the report text
//!   are safe functions on byte slices (unit-tested); `main` wires them to the sockets;
//! - `cstd` (`atoi`, `sscanf("%x:..")`, `printf("%f")` as the C library does them), `csum`, `bytes`
//!   (non-panicking slice helpers), `common` (clock, console, interface set-up);
//! - `os`: the only unsafe code, one system call per block (`AF_PACKET` and generic-netlink sockets,
//!   `poll`, interface lookup and the up / MAC ioctls).
//!
//! The binaries (`trap-frames`, `qos-frames`, `raw-frames`, `l3-frames`, `sflow-probe`) are behind the
//! `cli` feature; `tests/differential.rs` runs them against the C tools over veth pairs in a user
//! namespace.
#![deny(unsafe_code)]
// the ports keep the C tools' short names (b, s, i, n, o, f) so they read against the originals
#![allow(clippy::many_single_char_names)]

pub mod bytes;

/// A `u64` as a `double` (the C tools' `bytes * 8.0`; statistics, precision loss above 2^53 is irrelevant).
#[allow(clippy::cast_precision_loss)]
pub(crate) fn u64_f64(v: u64) -> f64 {
    v as f64
}

/// A size as a `double`.
pub(crate) fn usize_f64(v: usize) -> f64 {
    u64_f64(u64::try_from(v).unwrap_or(0))
}
pub mod common;
pub mod cstd;
pub mod csum;
pub mod l3;
pub mod os;
pub mod qos;
pub mod raw;
pub mod sflow;
pub mod trap;

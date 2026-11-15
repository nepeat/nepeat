//! fand: a vendor-neutral fan control daemon for network appliances.
//!
//! See README.md for the configuration reference and the safety model.

// The only unsafe block is the kill(2) call in exec::kill_group.
#![deny(unsafe_code)]

pub mod error;
pub mod log;

pub mod calibrate;
pub mod calibration;
pub mod check;
pub mod config;
pub mod control;
pub mod curves;
pub mod daemon;
pub mod engine;
pub mod exec;
pub mod fakesys;
pub mod notify;
pub mod sysfs;

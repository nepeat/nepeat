//! consoled: a serial console server.
//!
//! The daemon (`consoled`) owns every configured serial port, keeps a RAM scrollback per
//! port and serves clients over one local unix socket. The client (`console`) attaches a
//! terminal to a port; it is also what sshd runs (ForceCommand) for remote users.
//! Per port, at most one client writes; the others watch.

pub mod config;
pub mod proto;
pub mod ring;
pub mod serial;
pub mod term;

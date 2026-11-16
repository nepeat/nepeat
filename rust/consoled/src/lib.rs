//! consoled: a serial console server.
//!
//! The daemon (`consoled`) owns every configured serial port, keeps a RAM scrollback per
//! port and serves clients over one local unix socket. The client (`line`) lists ports,
//! attaches a terminal to one (or a menu), and drives them from scripts (send, expect,
//! run); it is also what sshd runs (ForceCommand). Per port, at most one client writes;
//! the others watch.

pub mod agent;
pub mod cli;
pub mod client;
pub mod config;
pub mod pattern;
pub mod proto;
pub mod ring;
pub mod serial;
pub mod session;
pub mod term;
pub mod text;

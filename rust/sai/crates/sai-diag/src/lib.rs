//! The diag socket of the SAI library: a port of sai-openbcm's `sai_diag.cpp`.
//!
//! With the profile key [`SOCKET_KEY`] (`SAI_OPENBCM_DIAG_SOCKET`, an absolute path) the library
//! listens on a unix socket while it owns the chip, for SDK diag-shell commands (no second SDK
//! process may attach). One command per connection: the client sends one line, gets the
//! command's output, then EOF. Off unless the key is set. Client: `openbcm-diag` (crate
//! `openbcm-diag`, the port of `openbcm_diag.c`).
//!
//! - Root only: the directory is created 0700, the socket 0600, and a peer whose uid is neither 0
//!   nor the socket owner's is answered `root only`.
//! - One thread ([`DiagServer`]), serving one connection at a time. A request is read with a 5 s
//!   timeout and **without** the library's lock; the command then runs under it
//!   ([`DiagHost::with_handler`]), never beside a SAI call. Its output is collected while the lock
//!   is held and written to the client after the lock is released: a client that does not read
//!   never holds up a SAI call.
//! - Refused ([`command::REFUSED_WORDS`]): a first word that would end or re-initialise the SDK,
//!   anything starting with `!`, any `;`. Our own commands ([`command::Command`]): `extphy`,
//!   `extmacsec`, `extmacsec trace ...`, `extmacsec run ...`; everything else goes to the
//!   backend's diag shell, writes included: a debug tool.
//! - Every failure the server reports starts with [`MARKER`] (`openbcm-diag: `): the client exits 1
//!   when it sees it.
//!
//! The library side implements two traits: [`DiagHandler`] (the core: what a command does) and
//! [`DiagHost`] (the rim: the lock, the peer's credentials, the log).
//!
//! Differences from the C++ (each stated where it is implemented):
//! - The command's output reaches the client after the command, not while it runs (the C++ hands
//!   the client's socket to the SDK, under the lock).
//! - A command line that is not UTF-8 or holds a NUL byte is refused like an empty one (the C++
//!   passes the bytes on; the SDK sees the line up to the NUL).
//! - A write to the client that makes no progress for 5 s ends the answer (the C++ has no write
//!   timeout: a client that sends a command and never reads holds its SAI lock, then
//!   `remove_switch`, for ever).
//! - The socket is bound with the process umask and then set to 0600 (std has no umask); a peer that
//!   connects in between is refused by the uid check. The listen backlog is std's (128; C++ 4).
//! - When setting up the socket fails after it was bound, the socket file is removed (the C++ leaves
//!   it behind). A failed `accept` is retried after 10 ms (the C++ retries at once).
#![forbid(unsafe_code)]

pub mod command;
mod server;

pub use command::{Command, MacsecRun, MdioAccess, Rejection, ShellCommand};
pub use server::{DiagServer, StartError};

use std::os::fd::BorrowedFd;

/// The profile key naming the socket's path (the C++ key).
pub const SOCKET_KEY: &str = "SAI_OPENBCM_DIAG_SOCKET";

/// The client's default socket (the path the boards set).
pub const DEFAULT_SOCKET: &str = "/run/sai-openbcm/diag.sock";

/// What every failure the server reports starts with.
pub const MARKER: &str = "openbcm-diag: ";

/// A diag shell command failed: the SAI status code the backend answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellFailed(pub i32);

/// What the library does with a command (implemented by the core). Every method runs under the
/// library's lock and only once [`DiagHandler::switch_created`] said yes.
pub trait DiagHandler {
    /// A switch exists (C++ `ctx().switch_created`): commands are answered `no switch` otherwise.
    fn switch_created(&self) -> bool;
    /// `extphy`: the external PHY table's state.
    fn ext_phy_status(&mut self) -> String;
    /// `extmacsec`: the external `MACsec` table's state.
    fn ext_macsec_status(&mut self) -> String;
    /// `extmacsec trace[ ARGS]`: the bus operation trace; `args` is the text after
    /// `extmacsec trace` (empty, or starting with a space).
    fn ext_macsec_trace(&mut self, args: &str) -> String;
    /// `extmacsec run PORT SEQUENCE [PARAMETER FILE]`: one sequence by hand.
    fn ext_macsec_run(&mut self, run: &MacsecRun<'_>) -> String;
    /// Anything else: the backend's diag shell, its output written to `out`.
    /// [`MdioAccess::Raw`] (`phy raw ...`) must not run inside one of the external PHY table's
    /// sequences and pauses its link polls (C++ `ext_phy_foreign_mdio`).
    fn shell(&mut self, command: &ShellCommand<'_>, out: BorrowedFd<'_>) -> Result<(), ShellFailed>;
}

/// The importance of a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// A command was run, the server started.
    Notice,
    /// The server could not start.
    Error,
}

/// What the server needs from the library around it (implemented by the rim).
pub trait DiagHost: Send + Sync + 'static {
    /// Takes the library's lock (the one every SAI call takes) and calls `body` exactly once:
    /// with the core, or with `None` when there is none (API not initialised, or unusable).
    fn with_handler(&self, body: &mut dyn FnMut(Option<&mut dyn DiagHandler>));
    /// The uid of the process at the other end of `conn` (`SO_PEERCRED`); `None` when it cannot
    /// be read, which refuses the peer.
    fn peer_uid(&self, conn: BorrowedFd<'_>) -> Option<u32>;
    /// Writes a line to the library's log.
    fn log(&self, level: Level, text: &str);
}

/// An I/O error as C's `strerror` prints it (without std's " (os error N)").
pub(crate) fn strerror(e: &std::io::Error) -> String {
    let text = e.to_string();
    match (e.raw_os_error(), text.rfind(" (os error ")) {
        (Some(_), Some(at)) => text.get(..at).unwrap_or(&text).to_owned(),
        _ => text,
    }
}

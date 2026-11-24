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
//! - An accepting thread and one thread per connection being served ([`DiagServer`]), at most
//!   [`MAX_CONNECTIONS`] (8) at once; a further connection waits in the socket's backlog
//!   until one is done. A request is read with a 5 s timeout and **without** the library's lock;
//!   the command then runs under it ([`DiagHost::with_handler`]), never beside a SAI call, one at a
//!   time with the other locked commands. Its output is collected while the lock is held and
//!   written to the client after the lock is released: a client that does not read never holds up
//!   a SAI call. A command that waits (for the lock, for a transaction's begin) never holds up
//!   another client's: a tool's raw MDIO inside its transaction is served while another client's
//!   `extmacsec` waits for the lock (the C++ serves one connection at a time).
//! - **Raw MDIO and transactions do not take the library's lock** (Rust only): `phy raw c45 ...`
//!   and `extphy txn ...` go to the [`PhyDiag`] the library handed to
//!   [`DiagServer::start_with`] (the core's PHY arbitration and a handle of the MDIO master), so a
//!   tool's multi-second session never stalls notifications or port sets. A raw line it does not
//!   serve itself (clause 22, another bus, devad 0, or a library that serves none) runs through the
//!   backend's shell under the lock as before ([`DiagHandler::raw_mdio`]).
//! - **Transactions** (`extphy txn begin <phy[,phy...]|all> [ms]` -> `txn <n> begun ...`,
//!   `extphy txn <n> phy raw ...`, `extphy txn renew <n> [ms]`, `extphy txn end <n>`, `extphy txn
//!   list`): a client's lease on PHYs across connections, so that a multi-register firmware
//!   conversation is not cut by the library's own MDIO users; time-limited (crate `sai-extphy`). A
//!   bare `phy raw ...` keeps working: an implicit lease of the table's `holdoff-ms` on its PHY.
//! - Refused ([`command::REFUSED_WORDS`]): a first word that would end or re-initialise the SDK,
//!   anything starting with `!`, any `;`. Our own commands ([`command::Command`]): `extphy`,
//!   `extmacsec`, `extmacsec trace ...`, `extmacsec run ...`, `extphy txn ...`; everything else goes
//!   to the backend's diag shell, writes included: a debug tool.
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
//! - Connections are served concurrently, at most [`MAX_CONNECTIONS`] (the C++: one at a time, on
//!   its one thread); stopping the server cuts off the clients still connected and waits for every
//!   connection's thread (the C++ waits for the one being served, at most its timeouts).
#![forbid(unsafe_code)]

pub mod command;
mod server;

pub use command::{Command, MacsecRun, MdioAccess, RawC45, RawRequest, RawTarget, Rejection, ShellCommand, Txn};
pub use server::{DiagServer, MAX_CONNECTIONS, StartError};

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

/// Why a raw MDIO command was not run, or failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawFailed {
    /// The backend's shell failed it.
    Shell(ShellFailed),
    /// The PHY arbitration refused it (a transaction that expired, another client's transaction):
    /// the client's message.
    Refused(String),
}

/// A command's answer from the library: its text, then (when it failed) the failure line the
/// server writes after [`MARKER`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// What the command printed.
    pub text: String,
    /// Why it failed (without the marker and the line feed).
    pub failure: Option<String>,
}

impl Reply {
    /// A successful answer.
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failure: None,
        }
    }

    /// A failure without other output.
    pub fn failed(why: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            failure: Some(why.into()),
        }
    }

    /// The bytes the client gets.
    pub fn into_bytes(self) -> Vec<u8> {
        let mut out = self.text.into_bytes();
        if let Some(why) = self.failure {
            out.extend_from_slice(MARKER.as_bytes());
            out.extend_from_slice(why.as_bytes());
            out.push(b'\n');
        }
        out
    }
}

/// The external PHYs' raw MDIO and transactions, served **without** the library's lock (so a
/// long diag session never holds up SAI calls or notifications): implemented by the core over
/// its PHY arbitration and a handle of the MDIO master, handed to [`DiagServer::start_with`].
pub trait PhyDiag: Send + Sync {
    /// A raw MDIO command: `None` when it is not one the library serves itself (the server
    /// then runs it through the backend's shell, under the lock: [`DiagHandler::raw_mdio`]).
    fn raw(&self, request: &RawRequest<'_>) -> Option<Reply>;
    /// `extphy txn ...` but a raw access ([`Txn::Raw`] goes to [`PhyDiag::raw`]).
    fn txn(&self, txn: &Txn<'_>) -> Reply;
}

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
    /// Anything else: the backend's diag shell, its output written to `out` (raw MDIO goes to
    /// [`DiagHandler::raw_mdio`]).
    fn shell(&mut self, command: &ShellCommand<'_>, out: BorrowedFd<'_>) -> Result<(), ShellFailed>;
    /// `phy raw ...` that [`PhyDiag`] did not serve: through the backend's shell, never inside
    /// one of the library's PHY sequences, within the PHY arbitration (C++
    /// `ext_phy_foreign_mdio`). Default: the shell as it is.
    fn raw_mdio(&mut self, request: &RawRequest<'_>, out: BorrowedFd<'_>) -> Result<(), RawFailed> {
        let command = ShellCommand {
            line: request.line,
            mdio: MdioAccess::Raw,
        };
        self.shell(&command, out).map_err(RawFailed::Shell)
    }
    /// `extphy txn ...` when no [`PhyDiag`] serves it. Default: not served.
    fn ext_phy_txn(&mut self, txn: &Txn<'_>) -> Reply {
        let _ = txn;
        Reply::failed("extphy txn: no external PHY arbitration in this library")
    }
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

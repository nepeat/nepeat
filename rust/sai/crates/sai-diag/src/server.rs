//! The server thread (`sai_diag.cpp` `diag_start`, `loop`, `serve`, `diag_stop`).
use crate::command::{self, Command, READ_LIMIT};
use crate::{DiagHandler, DiagHost, Level, MARKER, ShellFailed, strerror};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// `sizeof(sockaddr_un::sun_path)` on Linux.
const SUN_PATH: usize = 108;
/// How long a peer may take to send its line, and to take each part of the answer.
const PEER_TIMEOUT: Duration = Duration::from_secs(5);
/// The C++ reads in pieces of this size, and checks the limit between pieces.
const READ_PIECE: usize = 256;

/// Why the server did not start. `Display` is the C++ log line.
#[derive(Debug)]
pub enum StartError {
    /// Not absolute, or too long for a unix socket address.
    NotUsable(String),
    /// The socket's directory could not be created.
    Mkdir(String, io::Error),
    /// Something that is not a socket is in the way.
    NotASocket(String),
    /// Binding, setting the mode or starting the thread failed.
    Socket(String, io::Error),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUsable(path) => write!(
                f,
                "SAI_OPENBCM_DIAG_SOCKET {path}: not an absolute path that fits a unix socket"
            ),
            Self::Mkdir(dir, e) => write!(f, "diag socket: mkdir {dir}: {}", strerror(e)),
            Self::NotASocket(path) => write!(f, "diag socket: {path} exists and is not a socket"),
            Self::Socket(path, e) => write!(f, "diag socket {path}: {}", strerror(e)),
        }
    }
}

impl std::error::Error for StartError {}

/// The running server. Stopping it (dropping it) ends the thread, waits for it and removes the
/// socket file.
///
/// Drop it **without** holding the library's lock: the thread may be waiting for that lock to
/// run a command, and dropping waits for the thread.
#[derive(Debug)]
pub struct DiagServer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    /// The listening socket, as a stream: `shutdown` on it wakes the thread's `accept`.
    waker: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl DiagServer {
    /// Creates the socket at `path` (its directory 0700 when it is missing, the socket 0600; a
    /// stale socket there is replaced) and starts the thread. Logs the C++ notice on success; the
    /// caller logs the error.
    pub fn start(path: &str, host: Arc<dyn DiagHost>) -> Result<Self, StartError> {
        if path.len() >= SUN_PATH || !path.starts_with('/') {
            return Err(StartError::NotUsable(path.to_owned()));
        }
        let dir = path.rfind('/').and_then(|at| path.get(..at)).unwrap_or_default();
        if !dir.is_empty() {
            if let Err(e) = fs::DirBuilder::new().mode(0o700).create(dir) {
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(StartError::Mkdir(dir.to_owned(), e));
                }
            }
        }
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.file_type().is_socket() {
                return Err(StartError::NotASocket(path.to_owned()));
            }
            let _ = fs::remove_file(path); // a stale socket; bind says it if this failed
        }
        let failed = |e: io::Error| StartError::Socket(path.to_owned(), e);
        let logger = Arc::clone(&host);
        let listener = UnixListener::bind(path).map_err(failed)?;
        let rest = (|| {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            // Our own uid as the file system sees it: who may connect besides root.
            let owner = fs::symlink_metadata(path)?.uid();
            let waker = UnixStream::from(OwnedFd::from(listener.try_clone()?));
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let stop = Arc::clone(&stop);
                std::thread::Builder::new()
                    .name("sai-diag".into())
                    .spawn(move || accept_loop(&listener, &stop, &*host, owner))?
            };
            Ok((waker, stop, thread))
        })();
        let (waker, stop, thread) = rest.map_err(|e| {
            let _ = fs::remove_file(path);
            failed(e)
        })?;
        logger.log(
            Level::Notice,
            &format!("diag socket {path}: SDK diag shell commands (root, one per connection)"),
        );
        Ok(Self {
            path: PathBuf::from(path),
            stop,
            waker,
            thread: Some(thread),
        })
    }

    /// The socket's path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Stops the server (the same as dropping it).
    pub fn stop(self) {}
}

impl Drop for DiagServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the blocked accept (EINVAL) and refuses new connections; a connection being
        // served is finished first (at most the peer timeouts).
        let _ = self.waker.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            // A panic of the thread already poisoned what it held; nothing to add here.
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn accept_loop(listener: &UnixListener, stop: &AtomicBool, host: &dyn DiagHost, owner: u32) {
    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match accepted {
            Ok((conn, _)) => serve(host, conn, owner),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // EMFILE and the like: try again, without spinning
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// Writes all of `text`; a peer that went away or stopped reading is not our problem.
fn say(conn: &mut UnixStream, text: &[u8]) {
    let _ = conn.write_all(text);
}

/// Up to the first line feed, or until [`READ_LIMIT`] bytes, EOF, an error or the timeout,
/// in pieces as the C++ reads them.
fn read_request(conn: &mut UnixStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut piece = [0u8; READ_PIECE];
    while request.len() < READ_LIMIT && !request.contains(&b'\n') {
        match conn.read(&mut piece) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(piece.get(..n).unwrap_or_default()),
        }
    }
    request
}

fn serve(host: &dyn DiagHost, mut conn: UnixStream, owner: u32) {
    match host.peer_uid(conn.as_fd()) {
        Some(uid) if uid == 0 || uid == owner => {}
        _ => return say(&mut conn, b"openbcm-diag: root only\n"),
    }
    let _ = conn.set_read_timeout(Some(PEER_TIMEOUT));
    let _ = conn.set_write_timeout(Some(PEER_TIMEOUT));
    let request = read_request(&mut conn);
    match command::parse(&request) {
        Ok(command) => {
            let answer = execute(host, &command);
            say(&mut conn, &answer);
        }
        Err(rejection) => say(&mut conn, &rejection.message()),
    }
}

/// Collects what the backend's shell writes to a descriptor, on its own thread, so that the
/// shell never waits for the client while the lock is held.
struct Spool {
    tx: UnixStream,
    drain: JoinHandle<Vec<u8>>,
}

impl Spool {
    fn new() -> io::Result<Self> {
        let (tx, mut rx) = UnixStream::pair()?;
        let drain = std::thread::Builder::new().name("sai-diag-out".into()).spawn(move || {
            let mut out = Vec::new();
            let _ = rx.read_to_end(&mut out);
            out
        })?;
        Ok(Self { tx, drain })
    }

    fn fd(&self) -> BorrowedFd<'_> {
        self.tx.as_fd()
    }

    /// Everything written. Shutting the writing side down ends the reader even if the backend
    /// kept a copy of the descriptor.
    fn finish(self) -> Vec<u8> {
        let _ = self.tx.shutdown(Shutdown::Write);
        drop(self.tx);
        self.drain.join().unwrap_or_default()
    }
}

/// What a command produced under the lock.
enum Outcome {
    NoSwitch,
    Text(String),
    Shell(Result<(), ShellFailed>),
}

/// Runs a command under the lock; the answer is written by the caller, after the lock.
fn execute(host: &dyn DiagHost, command: &Command<'_>) -> Vec<u8> {
    let spool = match command {
        Command::Shell(_) => match Spool::new() {
            Ok(spool) => Some(spool),
            Err(e) => return format!("{MARKER}cannot collect the output: {}\n", strerror(&e)).into_bytes(),
        },
        _ => None,
    };
    let mut outcome = Outcome::NoSwitch;
    host.with_handler(&mut |handler| {
        if let Some(handler) = handler.filter(|h| h.switch_created()) {
            outcome = run(host, handler, command, spool.as_ref().map(Spool::fd));
        }
    });
    let mut answer = spool.map(Spool::finish).unwrap_or_default();
    match outcome {
        Outcome::NoSwitch => answer.extend_from_slice(b"openbcm-diag: no switch\n"),
        Outcome::Text(text) => answer.extend_from_slice(text.as_bytes()),
        Outcome::Shell(Ok(())) => {}
        Outcome::Shell(Err(ShellFailed(status))) => {
            answer.extend_from_slice(format!("{MARKER}command failed ({status})\n").as_bytes());
        }
    }
    answer
}

fn run(
    host: &dyn DiagHost,
    handler: &mut dyn DiagHandler,
    command: &Command<'_>,
    out: Option<BorrowedFd<'_>>,
) -> Outcome {
    match command {
        Command::ExtPhy => Outcome::Text(handler.ext_phy_status()),
        Command::ExtMacsec => Outcome::Text(handler.ext_macsec_status()),
        Command::ExtMacsecTrace(args) => Outcome::Text(handler.ext_macsec_trace(args)),
        Command::ExtMacsecRun(run) => {
            host.log(
                Level::Notice,
                &format!(
                    "diag socket: extmacsec run {} {}{}",
                    run.port,
                    run.sequence,
                    if run.param_file.is_some() {
                        " <parameter file>"
                    } else {
                        ""
                    }
                ),
            );
            Outcome::Text(handler.ext_macsec_run(run))
        }
        Command::Shell(shell) => {
            host.log(Level::Notice, &format!("diag socket: {}", shell.line));
            // Shell commands always come with the spool (`execute`).
            Outcome::Shell(out.map_or(Err(ShellFailed(-1)), |fd| handler.shell(shell, fd)))
        }
    }
}

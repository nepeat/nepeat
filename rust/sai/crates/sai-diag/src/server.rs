//! The server (`sai_diag.cpp` `diag_start`, `loop`, `serve`, `diag_stop`): one accepting thread and
//! one thread per connection being served, at most [`MAX_CONNECTIONS`] at once. The C++ serves one
//! connection at a time; here a command that waits (for the library's lock, a transaction's begin)
//! never holds up the next client: a tool's raw MDIO inside its transaction is served while another
//! client's `extmacsec` waits for the lock (the AS7772's expired-lease loop). Commands that need
//! the library's lock still run one at a time on it.
use crate::command::{self, Command, READ_LIMIT, RawRequest, Txn};
use crate::{DiagHandler, DiagHost, Level, MARKER, PhyDiag, RawFailed, Reply, ShellFailed, strerror};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// `sizeof(sockaddr_un::sun_path)` on Linux.
const SUN_PATH: usize = 108;
/// How long a peer may take to send its line, and to take each part of the answer.
const PEER_TIMEOUT: Duration = Duration::from_secs(5);
/// The C++ reads in pieces of this size, and checks the limit between pieces.
const READ_PIECE: usize = 256;
/// Connections served at once. A further one waits in the socket's backlog (std's 128) until one
/// of them is done: each is done within the peer timeouts plus its command, and no command waits
/// for another connection (a transaction's begin waits at most `LEASE_MAX_MS`). The tools use one
/// or two at a time (`phystat.py`, the rekey test's `extmacsec` poll, `phy-status`).
pub const MAX_CONNECTIONS: usize = 8;

/// The connections being served: their threads and a handle of each socket (to wake them at
/// stop), and whether the accepting thread waits for one of them to finish.
#[derive(Default)]
struct Conns {
    next: u64,
    live: BTreeMap<u64, (UnixStream, Option<JoinHandle<()>>)>,
    /// Threads that finished, to be joined (by the accepting thread or at stop).
    done: Vec<JoinHandle<()>>,
    /// The accepting thread is at [`MAX_CONNECTIONS`] and waits.
    full: bool,
}

#[derive(Default)]
struct Pool {
    conns: Mutex<Conns>,
    /// Signalled when a connection is done (and at stop).
    freed: Condvar,
}

impl Pool {
    fn lock(&self) -> std::sync::MutexGuard<'_, Conns> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

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

/// The running server. Stopping it (dropping it) ends the accepting thread and every connection's
/// thread (their sockets are shut down: a silent client does not keep it waiting), waits for all of
/// them and removes the socket file: no thread of the server outlives it.
///
/// Drop it **without** holding the library's lock: a connection's thread may be waiting for that
/// lock to run a command, and dropping waits for the thread.
pub struct DiagServer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    /// The listening socket, as a stream: `shutdown` on it wakes the thread's `accept`.
    waker: UnixStream,
    thread: Option<JoinHandle<()>>,
    pool: Arc<Pool>,
}

impl fmt::Debug for DiagServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagServer")
            .field("path", &self.path)
            .field("serving", &self.serving())
            .finish_non_exhaustive()
    }
}

impl DiagServer {
    /// Creates the socket at `path` (its directory 0700 when it is missing, the socket 0600; a
    /// stale socket there is replaced) and starts the thread. Logs the C++ notice on success; the
    /// caller logs the error.
    pub fn start(path: &str, host: Arc<dyn DiagHost>) -> Result<Self, StartError> {
        Self::start_with(path, host, None)
    }

    /// [`DiagServer::start`] with the library's PHY arbitration: raw MDIO and `extphy txn` are
    /// served by `phy` without the library's lock (crate documentation).
    pub fn start_with(path: &str, host: Arc<dyn DiagHost>, phy: Option<Arc<dyn PhyDiag>>) -> Result<Self, StartError> {
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
            let pool = Arc::new(Pool::default());
            let thread = {
                let (stop, pool) = (Arc::clone(&stop), Arc::clone(&pool));
                let served = Served { host, phy, owner };
                std::thread::Builder::new()
                    .name("sai-diag".into())
                    .spawn(move || accept_loop(&listener, &stop, &pool, &served))?
            };
            Ok((waker, stop, thread, pool))
        })();
        let (waker, stop, thread, pool) = rest.map_err(|e| {
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
            pool,
        })
    }

    /// The socket's path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Connections being served now (at most [`MAX_CONNECTIONS`]).
    pub fn serving(&self) -> usize {
        self.pool.lock().live.len()
    }

    /// Whether the server serves [`MAX_CONNECTIONS`] and the next one waits in the backlog.
    pub fn full(&self) -> bool {
        self.pool.lock().full
    }

    /// Stops the server (the same as dropping it).
    pub fn stop(self) {}
}

impl Drop for DiagServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the blocked accept (EINVAL) and refuses new connections.
        let _ = self.waker.shutdown(Shutdown::Both);
        // under the pool's lock: the accepting thread either sees `stop` or is already waiting
        drop(self.pool.lock());
        self.pool.freed.notify_all();
        if let Some(thread) = self.thread.take() {
            // A panic of the thread already poisoned what it held; nothing to add here.
            let _ = thread.join();
        }
        // The connections: a client that is silent or does not read is cut off (its read or
        // write returns); a command running finishes first (it never waits for another client).
        let threads: Vec<JoinHandle<()>> = {
            let mut c = self.pool.lock();
            for (conn, _) in c.live.values() {
                let _ = conn.shutdown(Shutdown::Both);
            }
            let mut threads: Vec<JoinHandle<()>> = c.live.values_mut().filter_map(|(_, t)| t.take()).collect();
            threads.append(&mut c.done);
            threads
        };
        for thread in threads {
            let _ = thread.join();
        }
        let mut c = self.pool.lock();
        c.live.clear();
        for thread in std::mem::take(&mut c.done) {
            let _ = thread.join();
        }
        drop(c);
        let _ = fs::remove_file(&self.path);
    }
}

/// What a connection's thread serves with.
struct Served {
    host: Arc<dyn DiagHost>,
    phy: Option<Arc<dyn PhyDiag>>,
    owner: u32,
}

impl Clone for Served {
    fn clone(&self) -> Self {
        Self {
            host: Arc::clone(&self.host),
            phy: self.phy.clone(),
            owner: self.owner,
        }
    }
}

fn accept_loop(listener: &UnixListener, stop: &AtomicBool, pool: &Arc<Pool>, served: &Served) {
    loop {
        // at most MAX_CONNECTIONS at once: the next one waits in the backlog
        {
            let mut c = pool.lock();
            for thread in std::mem::take(&mut c.done) {
                let _ = thread.join();
            }
            while c.live.len() >= MAX_CONNECTIONS && !stop.load(Ordering::SeqCst) {
                c.full = true;
                c = pool.freed.wait(c).unwrap_or_else(PoisonError::into_inner);
                for thread in std::mem::take(&mut c.done) {
                    let _ = thread.join();
                }
            }
            c.full = false;
        }
        let accepted = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match accepted {
            Ok((conn, _)) => spawn(pool, served, conn),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // EMFILE and the like: try again, without spinning
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// Serves `conn` on a thread of its own, registered in `pool` until it is done. When the socket
/// cannot be copied (for the stop), it is served on the accepting thread, as the C++ serves every
/// connection; when no thread can be started, it is closed unserved (the client reads EOF).
fn spawn(pool: &Arc<Pool>, served: &Served, conn: UnixStream) {
    let Ok(handle) = conn.try_clone() else {
        return serve(&*served.host, served.phy.as_deref(), conn, served.owner);
    };
    let mut c = pool.lock();
    let id = c.next;
    c.next += 1;
    let job = {
        let (pool, served) = (Arc::clone(pool), served.clone());
        move || {
            serve(&*served.host, served.phy.as_deref(), conn, served.owner);
            let mut c = pool.lock();
            if let Some((_, Some(thread))) = c.live.remove(&id) {
                c.done.push(thread);
            }
            drop(c);
            pool.freed.notify_all();
        }
    };
    // registered before the thread can finish (it removes itself under the same lock)
    match std::thread::Builder::new().name("sai-diag-conn".into()).spawn(job) {
        Ok(thread) => {
            c.live.insert(id, (handle, Some(thread)));
        }
        Err(_) => {
            drop(c);
            // the closure (and the connection) went with the failed spawn: nothing to serve
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

fn serve(host: &dyn DiagHost, phy: Option<&dyn PhyDiag>, mut conn: UnixStream, owner: u32) {
    match host.peer_uid(conn.as_fd()) {
        Some(uid) if uid == 0 || uid == owner => {}
        _ => return say(&mut conn, b"openbcm-diag: root only\n"),
    }
    let _ = conn.set_read_timeout(Some(PEER_TIMEOUT));
    let _ = conn.set_write_timeout(Some(PEER_TIMEOUT));
    let request = read_request(&mut conn);
    match command::parse(&request) {
        Ok(command) => {
            let answer = execute(host, phy, &command);
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
    Reply(Reply),
    Shell(Result<(), ShellFailed>),
    Raw(Result<(), RawFailed>),
}

/// Runs a command (under the lock unless the PHY arbitration serves it); the answer is written
/// by the caller, after the lock.
fn execute(host: &dyn DiagHost, phy: Option<&dyn PhyDiag>, command: &Command<'_>) -> Vec<u8> {
    match command {
        Command::Txn { line, txn } => {
            host.log(Level::Notice, &format!("diag socket: {line}"));
            return match (txn, phy) {
                (Txn::Raw(request), _) => raw(host, phy, request),
                (txn, Some(phy)) => phy.txn(txn).into_bytes(),
                (txn, None) => locked(host, None, &mut |h, _| Outcome::Reply(h.ext_phy_txn(txn))),
            };
        }
        Command::Shell(shell) if shell.mdio == command::MdioAccess::Raw => {
            host.log(Level::Notice, &format!("diag socket: {}", shell.line));
            return raw(host, phy, &RawRequest::new(shell.line, None));
        }
        _ => {}
    }
    let spool = match command {
        Command::Shell(_) => match Spool::new() {
            Ok(spool) => Some(spool),
            Err(e) => return format!("{MARKER}cannot collect the output: {}\n", strerror(&e)).into_bytes(),
        },
        _ => None,
    };
    let mut answer = Vec::new();
    let outcome = {
        let fd = spool.as_ref().map(Spool::fd);
        let mut outcome = Outcome::NoSwitch;
        host.with_handler(&mut |handler| {
            if let Some(handler) = handler.filter(|h| h.switch_created()) {
                outcome = run(host, handler, command, fd);
            }
        });
        outcome
    };
    if let Some(spool) = spool {
        answer = spool.finish();
    }
    finish(answer, outcome)
}

/// The answer after the spooled output.
fn finish(mut answer: Vec<u8>, outcome: Outcome) -> Vec<u8> {
    match outcome {
        Outcome::NoSwitch => answer.extend_from_slice(b"openbcm-diag: no switch\n"),
        Outcome::Text(text) => answer.extend_from_slice(text.as_bytes()),
        Outcome::Reply(reply) => answer.extend(reply.into_bytes()),
        Outcome::Shell(Ok(())) | Outcome::Raw(Ok(())) => {}
        Outcome::Shell(Err(ShellFailed(status))) | Outcome::Raw(Err(RawFailed::Shell(ShellFailed(status)))) => {
            answer.extend_from_slice(format!("{MARKER}command failed ({status})\n").as_bytes());
        }
        Outcome::Raw(Err(RawFailed::Refused(why))) => {
            answer.extend_from_slice(format!("{MARKER}{why}\n").as_bytes());
        }
    }
    answer
}

/// Runs `body` on the handler under the lock, with a spool for shell output when `spooled`.
fn locked(
    host: &dyn DiagHost,
    spooled: Option<()>,
    body: &mut dyn FnMut(&mut dyn DiagHandler, Option<BorrowedFd<'_>>) -> Outcome,
) -> Vec<u8> {
    let spool = match spooled {
        Some(()) => match Spool::new() {
            Ok(spool) => Some(spool),
            Err(e) => return format!("{MARKER}cannot collect the output: {}\n", strerror(&e)).into_bytes(),
        },
        None => None,
    };
    let outcome = {
        let fd = spool.as_ref().map(Spool::fd);
        let mut outcome = Outcome::NoSwitch;
        host.with_handler(&mut |handler| {
            if let Some(handler) = handler.filter(|h| h.switch_created()) {
                outcome = body(handler, fd);
            }
        });
        outcome
    };
    let answer = spool.map(Spool::finish).unwrap_or_default();
    finish(answer, outcome)
}

/// A raw MDIO command: the PHY arbitration without the lock when it serves it, else the
/// backend's shell under the lock (within the arbitration: [`DiagHandler::raw_mdio`]).
fn raw(host: &dyn DiagHost, phy: Option<&dyn PhyDiag>, request: &RawRequest<'_>) -> Vec<u8> {
    if let Some(reply) = phy.and_then(|p| p.raw(request)) {
        return reply.into_bytes();
    }
    locked(host, Some(()), &mut |handler, fd| {
        Outcome::Raw(fd.map_or(Err(RawFailed::Shell(ShellFailed(-1))), |fd| {
            handler.raw_mdio(request, fd)
        }))
    })
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
        // served before the lock (`execute`)
        Command::Txn { .. } => Outcome::NoSwitch,
    }
}

//! The diag socket on the core (`sai-diag`'s server, `sai-core::diag`'s handler) over the
//! virtual backend: the `test_diag` checks of sai-openbcm's `tests/test_sai.cpp`, the refusal
//! list, our own commands, concurrent and silent clients, oversized lines, a socket path that is
//! already taken, failure injection and the backend call log.
//!
//! The host here is what the rim does: one mutex around the core, taken per command. Its peer
//! check answers a configured uid (the rim reads `SO_PEERCRED`; this crate has no unsafe code).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::format_collect,
    missing_docs
)]

mod common;

use sai_core::backend::recording::{Mode, Recording};
use sai_core::backend::{Backend, Profile};
use sai_core::error::Error;
use sai_core::oid::TypedId as _;
use sai_core::sai::Sai;
use sai_core::virt::{ShellMdio, VirtualBackend};
use sai_diag::{DiagHandler, DiagHost, DiagServer, Level, StartError};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::BorrowedFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Host<B: Backend> {
    sai: Mutex<Option<Sai<B>>>,
    uid: Mutex<Option<u32>>,
    logs: Mutex<Vec<String>>,
}

impl<B: Backend + Send + 'static> DiagHost for Host<B> {
    fn with_handler(&self, body: &mut dyn FnMut(Option<&mut dyn DiagHandler>)) {
        let mut sai = self.sai.lock().unwrap();
        body(sai.as_mut().map(|s| s as &mut dyn DiagHandler));
    }
    fn peer_uid(&self, _conn: BorrowedFd<'_>) -> Option<u32> {
        *self.uid.lock().unwrap()
    }
    fn log(&self, level: Level, text: &str) {
        self.logs.lock().unwrap().push(format!("{level:?} {text}"));
    }
}

impl<B: Backend + Send + 'static> Host<B> {
    fn lock(&self) -> MutexGuard<'_, Option<Sai<B>>> {
        self.sai.lock().unwrap()
    }
}

/// A directory for one test (short: socket paths have 107 bytes); not created.
fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("sai-diag-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Our uid as the file system records it (what the server compares a peer with).
fn own_uid() -> u32 {
    // one probe directory per call: the tests run in parallel threads of one process
    static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = scratch(&format!("uid-probe-{n}"));
    std::fs::create_dir_all(&file).unwrap();
    let uid = std::fs::metadata(&file).unwrap().uid();
    std::fs::remove_dir_all(&file).unwrap();
    uid
}

fn host_with<B: Backend + Send + 'static>(backend: B, switch: bool) -> Arc<Host<B>> {
    let mut sai = Sai::new(backend);
    if switch {
        sai.create_switch(&common::init_attrs(), &common::profile(4)).unwrap();
    }
    Arc::new(Host {
        sai: Mutex::new(Some(sai)),
        uid: Mutex::new(Some(own_uid())),
        logs: Mutex::new(Vec::new()),
    })
}

fn host() -> Arc<Host<VirtualBackend>> {
    host_with(VirtualBackend::new(), true)
}

fn start<B: Backend + Send + 'static>(dir: &Path, host: &Arc<Host<B>>) -> (DiagServer, PathBuf) {
    let path = dir.join("diag.sock");
    let as_dyn: Arc<dyn DiagHost> = host.clone();
    let server = DiagServer::start(path.to_str().unwrap(), as_dyn).unwrap();
    (server, path)
}

/// `tests/test_sai.cpp` `diag_cmd`: one line, shut down the writing side, read to EOF.
fn diag_cmd(path: &Path, line: &[u8]) -> String {
    let Ok(mut s) = UnixStream::connect(path) else {
        return "(no connect)".into();
    };
    let _ = s.write_all(line);
    let _ = s.shutdown(Shutdown::Write);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn shell_log(host: &Host<VirtualBackend>) -> Vec<String> {
    host.lock().as_ref().unwrap().backend().shell_log().to_vec()
}

#[test]
fn the_checks_of_test_diag() {
    let dir = scratch("test-diag");
    let path = dir.join("diag.sock");
    let host = host();
    // no key (or an empty one): nothing
    let as_dyn: Arc<dyn DiagHost> = host.clone();
    assert!(sai_core::diag::start(&Profile::default(), as_dyn.clone(), None).is_none());
    assert!(sai_core::diag::start(&Profile::from_pairs([(sai_diag::SOCKET_KEY, "")]), as_dyn.clone(), None).is_none());
    assert!(!path.exists());
    let server = sai_core::diag::start(
        &Profile::from_pairs([(sai_diag::SOCKET_KEY, path.to_str().unwrap())]),
        as_dyn,
        None,
    )
    .unwrap();
    let meta = std::fs::symlink_metadata(&path).unwrap();
    assert!(std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()));
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    let before = shell_log(&host).len();
    assert_eq!(diag_cmd(&path, b"l2 station show\n"), "fake: l2 station show\n");
    assert_eq!(diag_cmd(&path, b"  l3 intf show  \r\n"), "fake: l3 intf show\n");
    assert_eq!(shell_log(&host).len(), before + 2);
    for bad in [
        "exit\n",
        "Quit\n",
        "init all\n",
        "rcload x\n",
        "!ls\n",
        "ps; exit\n",
        "\n",
    ] {
        assert!(diag_cmd(&path, bad.as_bytes()).starts_with("openbcm-diag: "), "{bad:?}");
    }
    assert_eq!(shell_log(&host).len(), before + 2);
    drop(server);
    assert!(!path.exists());
    assert_eq!(diag_cmd(&path, b"l2 show\n"), "(no connect)");
    let logs = host.logs.lock().unwrap().clone();
    assert_eq!(
        logs,
        [
            format!(
                "Notice diag socket {}: SDK diag shell commands (root, one per connection)",
                path.display()
            ),
            "Notice diag socket: l2 station show".into(),
            "Notice diag socket: l3 intf show".into(),
        ]
    );
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn the_refusal_list_and_other_rejections() {
    let dir = scratch("refused");
    let host = host();
    let (_server, path) = start(&dir, &host);
    let words = [
        "exit", "quit", "init", "deinit", "reset", "detach", "attach", "reload", "rcload", "shell", "sh", "!",
        "warmboot", "dbm",
    ];
    for word in words {
        for line in [
            word.to_string(),
            format!("{} soc", word.to_uppercase()),
            format!("{word}\tx"),
        ] {
            assert_eq!(
                diag_cmd(&path, format!("{line}\n").as_bytes()),
                format!("openbcm-diag: refused (would end or re-init the SDK, or chains commands): {line}\n")
            );
        }
    }
    for line in ["!ls -l", "ps;", "l2 show; exit", ";"] {
        assert_eq!(
            diag_cmd(&path, format!("{line}\n").as_bytes()),
            format!("openbcm-diag: refused (would end or re-init the SDK, or chains commands): {line}\n")
        );
    }
    // words that only start like a refused one, and a first word that is empty (leading tab:
    // the C++ does not refuse it either)
    for line in ["exits", "shx", "initx", "\texit"] {
        assert_eq!(
            diag_cmd(&path, format!("{line}\n").as_bytes()),
            format!("fake: {line}\n")
        );
    }
    let one_line = "openbcm-diag: send one command line (< 1000 characters)\n";
    for empty in [&b""[..], b"\n", b"  \r\n", b"\nps\n", b"ps \xff\n", b"ps\0x\n"] {
        assert_eq!(diag_cmd(&path, empty), one_line, "{empty:?}");
    }
    assert_eq!(shell_log(&host), ["exits", "shx", "initx", "\texit"]);
}

#[test]
fn oversized_lines() {
    let dir = scratch("long");
    let host = host();
    let (_server, path) = start(&dir, &host);
    let one_line = "openbcm-diag: send one command line (< 1000 characters)\n";
    let longest = "x".repeat(999);
    assert_eq!(
        diag_cmd(&path, format!("{longest}\n").as_bytes()),
        format!("fake: {longest}\n")
    );
    assert_eq!(
        diag_cmd(&path, format!("{longest}   \r\n").as_bytes()),
        format!("fake: {longest}\n")
    );
    assert_eq!(diag_cmd(&path, format!("{longest}y\n").as_bytes()), one_line);
    // no line feed: the server stops reading at 1024 bytes
    assert_eq!(diag_cmd(&path, "z".repeat(5000).as_bytes()), one_line);
    assert_eq!(
        diag_cmd(&path, format!("{}\nps\n", "z".repeat(1050)).as_bytes()),
        one_line
    );
    // a line feed after 1000 bytes of spaces: trimmed to nothing
    assert_eq!(diag_cmd(&path, format!("{}\n", " ".repeat(1010)).as_bytes()), one_line);
    assert_eq!(shell_log(&host), [longest.clone(), longest]);
}

#[test]
fn own_commands_without_tables() {
    let dir = scratch("own");
    let host = host();
    let (_server, path) = start(&dir, &host);
    assert_eq!(
        diag_cmd(&path, b"extphy\n"),
        "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n"
    );
    assert_eq!(
        diag_cmd(&path, b"extmacsec\n"),
        "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n"
    );
    assert!(diag_cmd(&path, b"extmacsec trace\n").starts_with("extmacsec trace: "));
    assert!(diag_cmd(&path, b"extmacsec trace bogus\n").starts_with("usage: extmacsec trace"));
    assert_eq!(
        diag_cmd(&path, b"extmacsec run ce0 sa.install.egress /run/k\n"),
        "extmacsec: no external MACsec table\n"
    );
    // not ours: to the shell
    assert_eq!(diag_cmd(&path, b"extmacsec run\n"), "fake: extmacsec run\n");
    assert_eq!(diag_cmd(&path, b"EXTPHY\n"), "fake: EXTPHY\n");
    assert_eq!(shell_log(&host), ["extmacsec run", "EXTPHY"]);
    let logs = host.logs.lock().unwrap().clone();
    assert!(logs.contains(&"Notice diag socket: extmacsec run ce0 sa.install.egress <parameter file>".to_string()));
}

#[test]
fn no_switch_and_not_root() {
    let dir = scratch("noswitch");
    let host = host_with(VirtualBackend::new(), false);
    let (server, path) = start(&dir, &host);
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: no switch\n");
    assert_eq!(diag_cmd(&path, b"extphy\n"), "openbcm-diag: no switch\n");
    // refusals come before the switch check
    assert!(diag_cmd(&path, b"exit\n").starts_with("openbcm-diag: refused"));
    // the rim has no core at all (API uninitialised)
    let gone = host.lock().take();
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: no switch\n");
    *host.lock() = gone;
    // a peer that is neither root nor us, or whose credentials cannot be read
    *host.uid.lock().unwrap() = Some(own_uid() + 1).filter(|u| *u != 0);
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: root only\n");
    *host.uid.lock().unwrap() = None;
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: root only\n");
    *host.uid.lock().unwrap() = Some(0);
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: no switch\n");
    drop(server);
}

#[test]
fn concurrent_clients_are_all_served() {
    let dir = scratch("many");
    let host = host();
    let (_server, path) = start(&dir, &host);
    let answers: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..24)
            .map(|i| {
                let path = &path;
                scope.spawn(move || diag_cmd(path, format!("show c ce{i}\n").as_bytes()))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, answer) in answers.iter().enumerate() {
        assert_eq!(*answer, format!("fake: show c ce{i}\n"));
    }
    let mut log = shell_log(&host);
    log.sort();
    let mut expected: Vec<String> = (0..24).map(|i| format!("show c ce{i}")).collect();
    expected.sort();
    assert_eq!(log, expected);
}

#[test]
fn a_silent_client_does_not_block_sai_calls() {
    let dir = scratch("silent");
    let host = host();
    let (server, path) = start(&dir, &host);
    let mut silent = UnixStream::connect(&path).unwrap();
    until("the server reads from it", || server.serving() == 1);
    // SAI calls go on: the lock is free while the server waits for the line
    let started = Instant::now();
    for vid in 10..20u16 {
        let mut guard = host.lock();
        let sai = guard.as_mut().unwrap();
        let switch = sai.switch_id().unwrap().raw();
        sai.create(
            sai_sys::SAI_OBJECT_TYPE_VLAN,
            switch,
            &[sai_core::value::Attr::new(
                sai_sys::SAI_VLAN_ATTR_VLAN_ID,
                sai_core::value::Value::U16(vid),
            )],
        )
        .unwrap();
    }
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    // the next client is served beside the silent one (the C++ makes it wait for its 5 s)
    assert_eq!(diag_cmd(&path, b"ps\n"), "fake: ps\n");
    until("only the silent one is left", || server.serving() == 1);
    let mut answer = String::new();
    silent.read_to_string(&mut answer).unwrap();
    assert_eq!(answer, "openbcm-diag: send one command line (< 1000 characters)\n");
    // stopping while a silent client is connected cuts it off: no waiting for its timeout
    let mut silent = UnixStream::connect(&path).unwrap();
    until("the server reads from it", || server.serving() == 1);
    let t = Instant::now();
    drop(server);
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(!path.exists());
    let mut rest = Vec::new();
    let _ = silent.read_to_end(&mut rest); // EOF (or a reset): the server is gone
}

/// Waits until `cond` holds (a handshake on the server's state, no fixed sleep; the 60 s
/// watchdog only turns a hang into a failure).
fn until(what: &str, cond: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(60), "never happened: {what}");
        std::thread::yield_now();
    }
}

#[test]
fn a_socket_path_that_is_taken_or_unusable() {
    let host = host();
    let as_dyn = || -> Arc<dyn DiagHost> { host.clone() };
    let dir = scratch("taken");
    std::fs::create_dir_all(&dir).unwrap();
    // a regular file: refused and left alone
    let file = dir.join("diag.sock");
    std::fs::write(&file, "keep").unwrap();
    let e = DiagServer::start(file.to_str().unwrap(), as_dyn()).unwrap_err();
    assert!(matches!(e, StartError::NotASocket(_)));
    assert_eq!(
        e.to_string(),
        format!("diag socket: {} exists and is not a socket", file.display())
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");
    // a stale socket (its listener gone): replaced
    let stale = dir.join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    assert!(stale.exists());
    let server = DiagServer::start(stale.to_str().unwrap(), as_dyn()).unwrap();
    assert_eq!(diag_cmd(&stale, b"ps\n"), "fake: ps\n");
    // a second server on a live socket takes the path over, as the C++ (unlink + bind)
    drop(server);
    assert!(!stale.exists());
    // not absolute, too long, no parent for the directory
    for bad in ["relative/diag.sock".to_string(), format!("/{}", "s".repeat(107))] {
        let e = DiagServer::start(&bad, as_dyn()).unwrap_err();
        assert_eq!(
            e.to_string(),
            format!("SAI_OPENBCM_DIAG_SOCKET {bad}: not an absolute path that fits a unix socket")
        );
    }
    let deep = dir.join("a/b/diag.sock");
    let e = DiagServer::start(deep.to_str().unwrap(), as_dyn()).unwrap_err();
    assert_eq!(
        e.to_string(),
        format!(
            "diag socket: mkdir {}: No such file or directory",
            dir.join("a/b").display()
        )
    );
    // a directory that exists keeps its mode
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).unwrap();
    let server = DiagServer::start(dir.join("x.sock").to_str().unwrap(), as_dyn()).unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o750);
    drop(server);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_shell_command_changes_nothing() {
    let dir = scratch("fail");
    let host = host();
    let (_server, path) = start(&dir, &host);
    let before = format!("{:?}", host.lock().as_ref().unwrap().store().unwrap());
    host.lock()
        .as_mut()
        .unwrap()
        .backend_mut()
        .fail_nth("run_shell_to", 1, Error::Failure)
        .unwrap();
    assert_eq!(diag_cmd(&path, b"l2 show\n"), "openbcm-diag: command failed (-1)\n");
    host.lock()
        .as_mut()
        .unwrap()
        .backend_mut()
        .fail_nth("run_shell_to", 2, Error::NotSupported)
        .unwrap();
    assert_eq!(diag_cmd(&path, b"l2 show\n"), "fake: l2 show\n");
    assert_eq!(diag_cmd(&path, b"l2 show\n"), "openbcm-diag: command failed (-2)\n");
    assert_eq!(shell_log(&host), ["l2 show"]);
    let after = format!("{:?}", host.lock().as_ref().unwrap().store().unwrap());
    assert_eq!(before, after);
}

#[test]
fn phy_raw_reads_the_mdio_model() {
    use sai_core::backend::types::{MdioDev, PhyAddr};
    let dir = scratch("phyraw");
    let host = host();
    let mdio = {
        let mut guard = host.lock();
        let backend = guard.as_mut().unwrap().backend_mut();
        backend.set_shell_mdio(ShellMdio::Answer);
        backend.mdio()
    };
    let d1 = MdioDev::new(1).unwrap();
    mdio.lock().unwrap().regs.insert((PhyAddr(0x10), d1, 0x8b00), 0x2391);
    mdio.lock().unwrap().dead.insert(PhyAddr(0x11));
    let (_server, path) = start(&dir, &host);
    assert_eq!(diag_cmd(&path, b"phy raw c45 0x10 1 0x8b00\n"), "\t0x8b00: 0x2391\n");
    assert_eq!(diag_cmd(&path, b"PHY RAW C45 16 0x1 35584\n"), "\t0x8b00: 0x2391\n");
    assert_eq!(diag_cmd(&path, b"phy raw c45 0x10 1 0x8b01\n"), "\t0x8b01: 0x0000\n");
    assert_eq!(
        diag_cmd(&path, b"phy raw c45 0x11 1 0x8b00\n"),
        "openbcm-diag: command failed (-1)\n"
    );
    // not a read: the echo
    assert_eq!(
        diag_cmd(&path, b"phy raw c45 0x10 1 0x8b00 5\n"),
        "fake: phy raw c45 0x10 1 0x8b00 5\n"
    );
    assert_eq!(
        diag_cmd(&path, b"phy raw c45 0x10 0 0x8b00\n"),
        "fake: phy raw c45 0x10 0 0x8b00\n"
    );
    let reads = mdio.lock().unwrap().log.len();
    assert_eq!(reads, 3);
    // the C++ fake's echo is the default (test_extphy.inc)
    host.lock()
        .as_mut()
        .unwrap()
        .backend_mut()
        .set_shell_mdio(ShellMdio::Echo);
    assert_eq!(
        diag_cmd(&path, b"phy raw c45 0x10 1 0x8b00\n"),
        "fake: phy raw c45 0x10 1 0x8b00\n"
    );
}

/// The C++ server as the oracle (`SAI_DIAG_CPP_ORACLE`: a program over the unmodified
/// `sai_diag.cpp` + fake backend that answers hex-encoded requests, one per line, with the
/// answer and the shell log's length; see the A8 report). Generated requests from the words people
/// send, refused words, separators and random bytes: the answers and the commands the backend ran
/// must be equal. Not compared: lines that are not UTF-8 or hold a NUL (refused here, passed on
/// by the C++), and `extmacsec trace` (the trace is agent A7's stub today).
#[test]
fn the_cpp_server_answers_the_same() {
    let Some(oracle) = std::env::var_os("SAI_DIAG_CPP_ORACLE") else {
        eprintln!("diag differential: SAI_DIAG_CPP_ORACLE is not set, the comparison with sai_diag.cpp is SKIPPED");
        return;
    };
    let words: [&[u8]; 40] = [
        b"phy",
        b"raw",
        b"c45",
        b"0x10",
        b"1",
        b"extphy",
        b"extmacsec",
        b"trace",
        b"run",
        b"ce0",
        b"seq.egress",
        b"l2",
        b"show",
        b"ps",
        b"exit",
        b"Quit",
        b"init",
        b"sh",
        b"shell",
        b"rcload",
        b"dbm",
        b"warmboot",
        b"attach",
        b"!ls",
        b"!",
        b"x",
        b" ",
        b"  ",
        b"\t",
        b"\r",
        b"\n",
        b";",
        b"\x0b",
        b" \r\n",
        b"show c",
        b"EXTPHY",
        b"detach",
        b"reload",
        b"deinit",
        b"reset",
    ];
    let mut seed: u64 = 0x5a1d_1a65;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % n
    };
    let mut requests: Vec<Vec<u8>> = vec![
        b"l2 station show\n".to_vec(),
        b"  l3 intf show  \r\n".to_vec(),
        format!("{}\n", "x".repeat(999)).into_bytes(),
        format!("{}\n", "x".repeat(1000)).into_bytes(),
        "z".repeat(5000).into_bytes(),
        b"extmacsec run ce0 sa.install.egress /run/k\n".to_vec(),
        b"extmacsec run \t\n".to_vec(),
        b"".to_vec(),
    ];
    for _ in 0..1500 {
        let mut r = Vec::new();
        for _ in 0..next(8) {
            match next(10) {
                0 => r.extend((0..next(4)).map(|_| u8::try_from(next(256)).unwrap())),
                1 => r.extend(std::iter::repeat_n(b'a', usize::try_from(next(1100)).unwrap())),
                _ => r.extend_from_slice(words[usize::try_from(next(40)).unwrap()]),
            }
        }
        if next(3) > 0 {
            r.push(b'\n');
        }
        requests.push(r);
    }
    // the C++
    let dir = scratch("oracle");
    std::fs::create_dir_all(&dir).unwrap();
    let cpp_sock = dir.join("cpp/d.sock");
    let input: String = requests
        .iter()
        .map(|r| {
            if r.is_empty() {
                "-\n".to_owned()
            } else {
                r.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n"
            }
        })
        .collect();
    let mut child = std::process::Command::new(&oracle)
        .arg(&cpp_sock)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let feeder = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    feeder.join().unwrap();
    let cpp: Vec<(Vec<u8>, usize)> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let (hex, n) = l.split_once(' ').unwrap();
            let bytes = if hex == "-" {
                Vec::new()
            } else {
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                    .collect()
            };
            (bytes, n.parse().unwrap())
        })
        .collect();
    assert_eq!(cpp.len(), requests.len(), "the oracle stopped early");
    // the Rust
    let host = host();
    let (_server, path) = start(&dir.join("rs"), &host);
    let (mut compared, mut skipped) = (0, 0);
    let mut cpp_before = 0;
    let mut rs_before = shell_log(&host).len();
    for (request, (cpp_answer, cpp_log)) in requests.iter().zip(&cpp) {
        let answer = diag_cmd(&path, request).into_bytes();
        let rs_log = shell_log(&host).len();
        let line = sai_diag::command::line(request);
        let not_text = std::str::from_utf8(line).map_or(true, |t| t.contains('\0'));
        let trace = matches!(
            sai_diag::command::parse(request),
            Ok(sai_diag::Command::ExtMacsecTrace(_))
        );
        if (not_text && !sai_diag::command::refused(line) && !line.is_empty() && line.len() < 1000) || trace {
            skipped += 1;
        } else {
            assert_eq!(
                (String::from_utf8_lossy(&answer), rs_log - rs_before),
                (String::from_utf8_lossy(cpp_answer), cpp_log - cpp_before),
                "request {:?}",
                String::from_utf8_lossy(request)
            );
            compared += 1;
        }
        cpp_before = *cpp_log;
        rs_before = rs_log;
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("diag differential: {compared} requests answered as sai_diag.cpp answers them ({skipped} not compared)");
    assert!(compared > 1000);
}

#[test]
fn the_backend_call_log() {
    let dir = scratch("calllog");
    let host = host_with(Recording::new(VirtualBackend::new(), Mode::Normal), true);
    let log = host.lock().as_ref().unwrap().backend().log();
    log.take();
    let (_server, path) = start(&dir, &host);
    assert_eq!(diag_cmd(&path, b"l2 station show\n"), "fake: l2 station show\n");
    assert!(diag_cmd(&path, b"exit\n").starts_with("openbcm-diag: refused"));
    assert!(diag_cmd(&path, b"extphy\n").starts_with("extphy: "));
    assert_eq!(
        diag_cmd(&path, b"phy raw c45 0x10 1 0x8b00\n"),
        "fake: phy raw c45 0x10 1 0x8b00\n"
    );
    assert_eq!(diag_cmd(&path, b"say \"hi\"\n"), "fake: say \"hi\"\n");
    host.lock()
        .as_mut()
        .unwrap()
        .backend_mut()
        .inner_mut()
        .fail_nth("run_shell_to", 1, Error::Failure)
        .unwrap();
    assert_eq!(diag_cmd(&path, b"ps\n"), "openbcm-diag: command failed (-1)\n");
    assert_eq!(
        log.take(),
        [
            r#"run_shell_to(cmd="l2 station show") -> SUCCESS"#,
            r#"run_shell_to(cmd="phy raw c45 0x10 1 0x8b00") -> SUCCESS"#,
            r#"run_shell_to(cmd="say \"hi\"") -> SUCCESS"#,
            r#"run_shell_to(cmd="ps") -> FAILURE"#,
        ]
    );
}

//! Differential test: the five Rust tools against the C tools they replace
//! (ai_re/nix/packages/sai-teralynx/tools/*.c, read only), over veth pairs inside an unprivileged
//! network namespace (`unshare -rn`).
//!
//! The outer test builds the C tools from the sources (`TL_FRAMES_C_TOOLS`, default: the checkout's
//! tools directory), then re-runs this test binary under `unshare -rn` (the inner test, selected by
//! `TL_FRAMES_INNER`). Inside, every scenario runs the C tool and the Rust tool with the same
//! command line against the same topology and compares, after masking times and run ids:
//! exit status, standard output, standard error and every frame the tool put on the wire (captured
//! on the veth peer). The outer test skips (loudly) where user namespaces, ip(8) or a C compiler are
//! missing, unless `TL_FRAMES_REQUIRE_DIFF=1`.
// test harness: a failure is a panic with its message
#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::missing_panics_doc
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tl_frames::bytes::{find, ne32};
use tl_frames::os::{PACKET_OUTGOING, Sock, if_nametoindex};

const INNER: &str = "TL_FRAMES_INNER";

/// The C tools' recorded behaviour (H5-0 golden, captured once from the C tools; see its header).
/// Never regenerate it from the Rust output.
const GOLDEN: &str = include_str!("golden/frames-cpp.txt");

/// Records seen by this run, by scenario key (checked for coverage; written in capture mode).
static SEEN: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// One scenario's behaviour as text: exit status, masked stdout / stderr (escaped, one line each),
/// then the frames on the wire, one hex line each.
fn record(status: i32, stdout: &str, stderr: &str, frames: &[Vec<u8>]) -> String {
    let mut r = format!(
        "status {status}\nstdout {stdout:?}\nstderr {stderr:?}\nframes {}\n",
        frames.len()
    );
    for f in frames {
        r.push_str("f ");
        for b in f {
            write!(r, "{b:02x}").unwrap();
        }
        r.push('\n');
    }
    r
}

fn run_record(r: &Run) -> String {
    record(r.status, &r.stdout, &r.stderr, &r.frames)
}

/// Golden text -> records by key (`@@ key` starts a record; `#` lines before the first are the header).
fn parse_golden(text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if let Some(k) = line.strip_prefix("@@ ") {
            assert!(
                m.insert(k.to_owned(), String::new()).is_none(),
                "duplicate golden key {k}"
            );
            cur = Some(k.to_owned());
        } else if let Some(k) = &cur {
            let e = m.get_mut(k).unwrap();
            e.push_str(line);
            e.push('\n');
        }
    }
    m
}

fn golden() -> &'static BTreeMap<String, String> {
    static G: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    G.get_or_init(|| parse_golden(GOLDEN))
}

/// Ok when `rec` is exactly the golden record of `key`; else the first difference.
fn golden_check(golden_map: &BTreeMap<String, String>, key: &str, rec: &str) -> Result<(), String> {
    let Some(want) = golden_map.get(key) else {
        return Err(format!("{key}: no golden record"));
    };
    if want == rec {
        return Ok(());
    }
    let (mut want_lines, mut got_lines) = (want.lines(), rec.lines());
    let mut line_no = 1;
    loop {
        let (want_line, got_line) = (want_lines.next(), got_lines.next());
        if want_line != got_line {
            return Err(format!("{key}: line {line_no}: golden {want_line:?} vs {got_line:?}"));
        }
        if want_line.is_none() {
            return Err(format!("{key}: records differ"));
        }
        line_no += 1;
    }
}

fn capturing() -> bool {
    std::env::var("TL_FRAMES_CAPTURE").is_ok()
}

/// Judges one run (`who` is "C" or "Rust") against the golden, and notes the scenario as covered.
/// In capture mode (`TL_FRAMES_CAPTURE=<file>`, C tools only) it records the C run instead.
fn judge(key: &str, who: &str, rec: String) {
    if capturing() {
        if who == "C" {
            SEEN.lock().unwrap().insert(key.to_owned(), rec);
        }
        return;
    }
    if let Err(e) = golden_check(golden(), key, &rec) {
        panic!("{who} vs golden: {e}");
    }
    SEEN.lock().unwrap().insert(key.to_owned(), rec);
}

fn tools_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("TL_FRAMES_C_TOOLS") {
        return Some(PathBuf::from(d));
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = here.join("../../../../ai_re/nix/packages/sai-teralynx/tools");
    d.join("trap_frames.c").exists().then_some(d)
}

fn have(cmd: &str, arg: &str) -> bool {
    Command::new(cmd)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn skip(reason: &str) {
    assert!(
        std::env::var("TL_FRAMES_REQUIRE_DIFF").is_err(),
        "differential required but: {reason}"
    );
    eprintln!("SKIPPED differential: {reason}");
}

/// (source file, binary name, extra libs)
const C_TOOLS: [(&str, &str, &[&str]); 5] = [
    ("trap_frames.c", "trap-frames", &["-lpthread"]),
    ("qos_frames.c", "qos-frames", &["-lpthread"]),
    ("raw_frames.c", "raw-frames", &["-lpthread"]),
    ("l3_frames.c", "l3-frames", &[]),
    ("sflow_probe.c", "sflow-probe", &["-lpthread", "-lm"]),
];

fn build_c(src: &Path, out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    for (file, bin, libs) in C_TOOLS {
        let st = Command::new(&cc)
            .args(["-O2", "-w", "-o"])
            .arg(out.join(bin))
            .arg(src.join(file))
            .args(libs)
            .status()
            .unwrap();
        assert!(st.success(), "compiling {file}");
    }
}

#[test]
fn differential_in_a_user_namespace() {
    if std::env::var(INNER).is_ok() {
        return;
    }
    // without the C sources the run is golden-only (the Rust tools against the recorded C behaviour)
    let src = tools_dir().filter(|_| std::env::var("TL_FRAMES_NO_C").is_err());
    assert!(
        src.is_some() || !capturing(),
        "capture needs the C tool sources (TL_FRAMES_C_TOOLS)"
    );
    if !have("unshare", "--help")
        || !Command::new("unshare")
            .args(["-rn", "true"])
            .status()
            .is_ok_and(|s| s.success())
    {
        return skip("unprivileged user + network namespaces are not available");
    }
    if !have("ip", "-V") {
        return skip("ip(8) is not available");
    }
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    if !have(&cc, "--version") {
        return skip("no C compiler (the psample shim needs one)");
    }
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tl-frames-diff-{}", std::process::id()));
    let cbin = tmp.join("c");
    std::fs::create_dir_all(&tmp).unwrap();
    if let Some(src) = &src {
        build_c(src, &cbin);
    }
    let shim = tmp.join("psample_shim.so");
    let st = Command::new(&cc)
        .args(["-shared", "-fPIC", "-w", "-o"])
        .arg(&shim)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psample_shim.c"))
        .args(["-ldl", "-lpthread"])
        .status()
        .unwrap();
    assert!(st.success(), "compiling the psample shim");
    let exe = std::env::current_exe().unwrap();
    let out = Command::new("unshare")
        .args(["-rn"])
        .arg(&exe)
        .args(["--exact", "inner", "--nocapture", "--test-threads=1"])
        .env(INNER, "1")
        .envs(src.as_ref().map(|_| ("TL_FRAMES_CBIN", &cbin)))
        .env("TL_FRAMES_SHIM", &shim)
        .env("TL_FRAMES_TMP", &tmp)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(out.status.success(), "inner differential failed:\n{text}");
    eprintln!("{text}");
}

// ------------------------------------------------------------------------------ inner

struct Tool {
    name: &'static str,
    /// the C tool binary; used only when `live`
    c: PathBuf,
    /// the C tools were built: compare live as well as against the golden
    live: bool,
    rust: PathBuf,
}

fn tool(name: &'static str) -> Tool {
    let cbin = std::env::var("TL_FRAMES_CBIN").ok().map(PathBuf::from);
    let rust = match name {
        "trap-frames" => env!("CARGO_BIN_EXE_trap-frames"),
        "qos-frames" => env!("CARGO_BIN_EXE_qos-frames"),
        "raw-frames" => env!("CARGO_BIN_EXE_raw-frames"),
        "l3-frames" => env!("CARGO_BIN_EXE_l3-frames"),
        _ => env!("CARGO_BIN_EXE_sflow-probe"),
    };
    Tool {
        name,
        c: cbin.clone().unwrap_or_default().join(name),
        live: cbin.is_some(),
        rust: PathBuf::from(rust),
    }
}

fn ip(args: &[&str]) {
    let st = Command::new("ip").args(args).status().unwrap();
    assert!(st.success(), "ip {args:?}");
}

/// A fixed hardware address for a veth end (the kernel picks random ones, which tools put in the
/// source MAC of their frames: the golden needs the same bytes every run).
fn mac_of(name: &str) -> String {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |acc, byte| {
        (acc ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    let [b0, b1, b2, b3] = hash.to_be_bytes();
    format!("02:7e:{b0:02x}:{b1:02x}:{b2:02x}:{b3:02x}")
}

/// A veth pair, both ends up.
fn veth(a: &str, b: &str) {
    ip(&[
        "link",
        "add",
        "name",
        a,
        "address",
        &mac_of(a),
        "type",
        "veth",
        "peer",
        "name",
        b,
        "address",
        &mac_of(b),
    ]);
    ip(&["link", "set", "dev", a, "up"]);
    ip(&["link", "set", "dev", b, "up"]);
}

/// Everything one run produced.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    status: i32,
    stdout: String,
    stderr: String,
    frames: Vec<Vec<u8>>,
}

type Inject = Box<dyn FnOnce() + Send>;

/// Captures what arrives on `peer` while `f` runs: frames that contain `tag`.
fn capture<R: Send>(peer: &str, tag: &[u8], f: impl FnOnce() -> R + Send) -> (R, Vec<Vec<u8>>) {
    let ix = if_nametoindex(peer);
    let sock = Sock::packet(3).unwrap();
    sock.bind_packet(ix, 3).unwrap();
    sock.set_recv_timeout(Duration::from_millis(50)).unwrap();
    let stop = AtomicBool::new(false);
    thread::scope(|s| {
        let h = s.spawn(|| {
            let mut frames = Vec::new();
            let mut buf = vec![0u8; 20000];
            loop {
                match sock.recv_from_packet(&mut buf, false) {
                    Ok((n, t)) if n > 0 && t != PACKET_OUTGOING => {
                        let fr = buf[..n].to_vec();
                        if find(&fr, tag).is_some() {
                            frames.push(fr);
                        }
                    }
                    _ if stop.load(Ordering::Relaxed) => break,
                    _ => {}
                }
            }
            frames
        });
        let r = f();
        thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::Relaxed);
        (r, h.join().unwrap())
    })
}

fn exec(bin: &Path, args: &[String], env: &[(&str, &str)], inject: Option<Inject>) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args.iter().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().unwrap();
    let h = inject.map(|i| {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(1000));
            i();
        })
    });
    let out = child.wait_with_output().unwrap();
    if let Some(h) = h {
        h.join().unwrap();
    }
    out
}

/// Replaces the value of `key=value` tokens (times, rates) and the program path.
fn mask(text: &str, keys: &[&str], prog: &Path) -> String {
    let prog = prog.to_string_lossy();
    text.replace(prog.as_ref(), "PROG")
        .lines()
        .map(|l| {
            l.split(' ')
                .map(|tok| {
                    for k in keys {
                        if tok.starts_with(&format!("{k}=")) {
                            return format!("{k}=*");
                        }
                    }
                    tok.to_owned()
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const TIME_KEYS: [&str; 6] = ["seconds", "rate", "rx_seconds", "rx_mbps", "tx_mbps", "pps"];

struct Scenario<'a> {
    label: String,
    args: Vec<String>,
    env: Vec<(&'a str, &'a str)>,
    peer: &'a str,
    tag: &'a [u8],
    /// mask the run id after the tag
    mask_run_id: bool,
    /// at least this many frames must have been captured (so an empty == empty match cannot pass)
    min_frames: usize,
    inject: Option<fn(&str, &str) -> Inject>,
    tx: &'a str,
}

fn one(t: &Tool, bin: &Path, sc: &Scenario) -> Run {
    let inject = sc.inject.map(|f| f(sc.tx, sc.peer));
    let (out, frames) = capture(sc.peer, sc.tag, || exec(bin, &sc.args, &sc.env, inject));
    let frames = frames
        .into_iter()
        .map(|mut f| {
            if sc.mask_run_id {
                if sc.tag == b"L3FRAMES" {
                    zero_valid_v6_checksum(&mut f);
                }
                if let Some(p) = find(&f, sc.tag) {
                    f[p + 8..p + 12].fill(0);
                }
            }
            f
        })
        .collect();
    Run {
        status: out.status.code().unwrap_or(-1),
        stdout: mask(&String::from_utf8_lossy(&out.stdout), &TIME_KEYS, bin)
            .replace(&t.rust.to_string_lossy().into_owned(), "PROG"),
        stderr: mask(&String::from_utf8_lossy(&out.stderr), &TIME_KEYS, bin)
            .replace(&t.rust.to_string_lossy().into_owned(), "PROG"),
        frames,
    }
}

/// An IPv6 UDP checksum covers the run id (random per run): it must verify, then it is zeroed so
/// the C and Rust frames can be compared.
fn zero_valid_v6_checksum(f: &mut [u8]) {
    if f.get(12..14) != Some(&[0x86, 0xdd][..]) {
        return;
    }
    let udp = &f[54..];
    let mut ph = f[22..54].to_vec();
    ph.extend_from_slice(&u32::try_from(udp.len()).unwrap().to_be_bytes());
    ph.extend_from_slice(&[0, 0, 0, 17]);
    ph.extend_from_slice(udp);
    assert_eq!(
        tl_frames::csum::csum(&ph),
        0,
        "IPv6 UDP checksum of a sent frame (len {}): {f:02x?}",
        udp.len()
    );
    f[60..62].fill(0);
}

fn compare(t: &Tool, sc: &Scenario) {
    let c = if t.live { one(t, &t.c, sc) } else { one(t, &t.rust, sc) };
    let r = one(t, &t.rust, sc);
    println!(
        "[{}] {}: {} status={} frames={} / Rust status={} frames={}",
        t.name,
        sc.label,
        if t.live { "C" } else { "golden-only (Rust)" },
        c.status,
        c.frames.len(),
        r.status,
        r.frames.len()
    );
    let key = format!("{}/{}", t.name, sc.label);
    if t.live {
        // live C == golden (the capture proof), then Rust == golden
        judge(&key, "C", run_record(&c));
    }
    judge(&key, "Rust", run_record(&r));
    assert_eq!(c.status, r.status, "{} {}: exit status", t.name, sc.label);
    assert_eq!(c.stderr, r.stderr, "{} {}: stderr", t.name, sc.label);
    assert_eq!(
        c.stdout, r.stdout,
        "{} {}: stdout\n--- C\n{}\n--- Rust\n{}",
        t.name, sc.label, c.stdout, r.stdout
    );
    assert!(
        c.frames.len() >= sc.min_frames,
        "{} {}: only {} frames captured",
        t.name,
        sc.label,
        c.frames.len()
    );
    assert_eq!(
        c.frames.len(),
        r.frames.len(),
        "{} {}: frames on the wire",
        t.name,
        sc.label
    );
    for (k, (a, b)) in c.frames.iter().zip(&r.frames).enumerate() {
        assert_eq!(a, b, "{} {}: frame {k} differs", t.name, sc.label);
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

fn sc<'a>(label: &str, args: &[&str], peer: &'a str, tag: &'a [u8], min_frames: usize) -> Scenario<'a> {
    Scenario {
        label: label.to_owned(),
        args: s(args),
        env: Vec::new(),
        peer,
        tag,
        mask_run_id: false,
        min_frames,
        inject: None,
        tx: "",
    }
}

fn trap_scenarios() {
    let t = tool("trap-frames");
    veth("tpa", "tpb");
    for kind in ["plain", "lldp", "lacp", "eapol", "arp"] {
        compare(
            &t,
            &sc(
                kind,
                &["trap-frames", "tpa", "tpb", kind, "20", "0"],
                "tpb",
                b"TRAPTEST",
                20,
            ),
        );
    }
    for kind in ["bgp", "ip2me"] {
        compare(
            &t,
            &sc(
                kind,
                &[
                    "trap-frames",
                    "tpa",
                    "tpb",
                    kind,
                    "12",
                    "400",
                    "02:00:00:00:01:20",
                    "10.99.0.1",
                ],
                "tpb",
                b"TRAPTEST",
                12,
            ),
        );
    }
    compare(&t, &sc("no args", &["trap-frames"], "tpb", b"TRAPTEST", 0));
    compare(
        &t,
        &sc(
            "unknown kind",
            &["trap-frames", "tpa", "tpb", "nope", "1", "1"],
            "tpb",
            b"TRAPTEST",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "bad dmac",
            &["trap-frames", "tpa", "tpb", "bgp", "1", "1", "zz"],
            "tpb",
            b"TRAPTEST",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "bad ip",
            &["trap-frames", "tpa", "tpb", "bgp", "1", "1", "02:00:00:00:01:20", "x.y"],
            "tpb",
            b"TRAPTEST",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "no such tx",
            &["trap-frames", "nonesuch0", "tpb", "lldp", "1", "1"],
            "tpb",
            b"TRAPTEST",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "count 0",
            &["trap-frames", "tpa", "tpb", "lldp", "0", "1"],
            "tpb",
            b"TRAPTEST",
            0,
        ),
    );
}

const HEXES: [(&str, &str); 5] = [
    (
        "icmp",
        "02000000012000000000000008004500001c000000004001667c0a0000020a6300010800e5ca12340001",
    ),
    (
        "tcp",
        "020000000120000000000000080045b8002800000000400665b30a0000020a6300019c4022b800000000000000005002ffff00000000",
    ),
    (
        "icmp6",
        "02000000012000000000000086dd6000000000083a4020010db800000000000000000000000220010db80000000000000000000000018000000012340001",
    ),
    (
        "igmp",
        "01005e000001000000000000080045c0001c000000000102cf1d0a000002e00000011164ee9b00000000",
    ),
    ("short", "ffffffffffff0000000000000800"),
];

fn raw_scenarios() {
    let t = tool("raw-frames");
    veth("rwa", "rwb");
    for (name, hex) in HEXES {
        let mut c = sc(
            name,
            &["raw-frames", "rwa", "rwb", hex, "15", "0"],
            "rwb",
            b"RAWFRAME",
            15,
        );
        c.mask_run_id = true;
        compare(&t, &c);
    }
    let mut c = sc(
        "paced",
        &["raw-frames", "rwa", "rwb", HEXES[0].1, "10", "200"],
        "rwb",
        b"RAWFRAME",
        10,
    );
    c.mask_run_id = true;
    compare(&t, &c);
    compare(
        &t,
        &sc(
            "bad hex",
            &["raw-frames", "rwa", "rwb", "zz", "1", "1"],
            "rwb",
            b"RAWFRAME",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "too short",
            &["raw-frames", "rwa", "rwb", "0011", "1", "1"],
            "rwb",
            b"RAWFRAME",
            0,
        ),
    );
    compare(&t, &sc("usage", &["raw-frames", "rwa"], "rwb", b"RAWFRAME", 0));
}

/// Injects GRE-encapsulated copies of a `QoS` frame (an ERSPAN-like mirror copy) from the TX side.
fn gre_copy(tx: &str, _peer: &str) -> Inject {
    let tx = tx.to_owned();
    Box::new(move || {
        let inner = tl_frames::qos::build_udp(
            128,
            [2, 0x99, 0x99, 0x99, 0x99, 0x99],
            [2, 0, 0, 0, 0, 1],
            10,
            0,
            [10, 99, 9, 1],
            [10, 99, 9, 2],
            0,
            9,
        );
        let mut f = vec![0u8; 14 + 20 + 4];
        f[0..6].copy_from_slice(&[2, 0, 0, 0, 0, 0x77]);
        f[12] = 8;
        f[14] = 0x45;
        f[15] = 0xb8;
        f[22] = 63;
        f[23] = 47;
        f[26..30].copy_from_slice(&[192, 0, 2, 1]);
        f[30..34].copy_from_slice(&[192, 0, 2, 2]);
        f[36..38].copy_from_slice(&[0x88, 0xbe]);
        f.extend_from_slice(&inner);
        let sock = Sock::packet(0).unwrap();
        let ix = if_nametoindex(&tx);
        for _ in 0..3 {
            sock.send_to_packet(ix, [2, 0, 0, 0, 0, 0x77], &f).unwrap();
        }
    })
}

fn qos_scenarios() {
    let t = tool("qos-frames");
    veth("qsa", "qsb");
    let dm = "02:99:99:99:99:99";
    let base = |l: &str, cnt: &str, pps: &str, size: &str, dscp: &str| {
        sc(
            l,
            &[
                "qos-frames",
                "qsa",
                "qsb",
                cnt,
                pps,
                size,
                dm,
                "10.99.9.1",
                "10.99.9.2",
                dscp,
            ],
            "qsb",
            b"QOSFRAME",
            1,
        )
    };
    compare(&t, &base("one dscp", "30", "0", "256", "0"));
    compare(&t, &base("three dscps", "30", "0", "256", "0,10,46"));
    compare(&t, &base("paced", "20", "400", "1500", "46"));
    compare(&t, &base("small size clamps", "10", "0", "10", "5"));
    compare(
        &t,
        &sc(
            "huge size clamps (veth refuses 9000)",
            &[
                "qos-frames",
                "qsa",
                "qsb",
                "5",
                "0",
                "99999",
                dm,
                "10.99.9.1",
                "10.99.9.2",
                "5",
            ],
            "qsb",
            b"QOSFRAME",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "negative size (clamps to 9000)",
            &[
                "qos-frames",
                "qsa",
                "qsb",
                "5",
                "0",
                "-5",
                dm,
                "10.99.9.1",
                "10.99.9.2",
                "5",
            ],
            "qsb",
            b"QOSFRAME",
            0,
        ),
    );
    let mut e = base("ecn ce", "12", "0", "128", "8,16");
    e.env = vec![("ECN", "3")];
    compare(&t, &e);
    let mut e = base("ecn ect0", "6", "0", "128", "8");
    e.env = vec![("ECN", "2")];
    compare(&t, &e);
    let mut g = base("gre copies", "10", "0", "128", "10");
    g.inject = Some(gre_copy);
    g.tx = "qsa";
    compare(&t, &g);
    let mut m = sc(
        "two rx",
        &[
            "qos-frames",
            "qsa",
            "qsb,qsa",
            "10",
            "0",
            "128",
            dm,
            "10.99.9.1",
            "10.99.9.2",
            "3",
        ],
        "qsb",
        b"QOSFRAME",
        1,
    );
    m.tx = "qsa";
    compare(&t, &m);
    compare(
        &t,
        &sc(
            "pfc",
            &[
                "qos-frames",
                "qsa",
                "qsb",
                "8",
                "0",
                "64",
                "pfc",
                "10.99.9.1",
                "10.99.9.2",
                "3,5",
            ],
            "qsb",
            b"QOSFRAME",
            8,
        ),
    );
    compare(&t, &sc("usage", &["qos-frames", "qsa"], "qsb", b"QOSFRAME", 0));
    compare(
        &t,
        &sc(
            "bad mac",
            &[
                "qos-frames",
                "qsa",
                "qsb",
                "1",
                "0",
                "64",
                "zz",
                "1.1.1.1",
                "1.1.1.2",
                "0",
            ],
            "qsb",
            b"QOSFRAME",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "bad ip",
            &["qos-frames", "qsa", "qsb", "1", "0", "64", dm, "1.1.1", "1.1.1.2", "0"],
            "qsb",
            b"QOSFRAME",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "no such rx",
            &[
                "qos-frames",
                "qsa",
                "nonesuch0",
                "1",
                "0",
                "64",
                dm,
                "1.1.1.1",
                "1.1.1.2",
                "0",
            ],
            "qsb",
            b"QOSFRAME",
            0,
        ),
    );
}

fn l3_scenarios() {
    let t = tool("l3-frames");
    veth("l3a", "l3b");
    let mk = |l: &str, extra: &[&str], min: usize| {
        let mut a = vec![
            "l3-frames",
            "tx=l3a",
            "rx=l3b,l3a",
            "dmac=02:00:00:00:01:0a",
            "wait_ms=300",
            "pps=0",
        ];
        a.extend_from_slice(extra);
        let mut c = sc(l, &a, "l3b", b"L3FRAMES", min);
        c.mask_run_id = true;
        c
    };
    compare(&t, &mk("v4 basic", &["dst=10.30.0.1", "src=10.1.0.2", "count=40"], 40));
    compare(&t, &mk("v4 flows", &["dst=10.30.0.1", "count=64", "flows=16"], 64));
    compare(&t, &mk("v4 dst_n", &["dst=10.2.0.250", "count=40", "dst_n=20"], 40));
    compare(
        &t,
        &mk(
            "v4 opts tos ttl",
            &["dst=10.2.0.1", "count=12", "opts=1", "tos=184", "ttl=2", "dport=53"],
            12,
        ),
    );
    compare(&t, &mk("v4 sizes", &["dst=10.2.0.1", "count=6", "size=1514"], 6));
    compare(&t, &mk("v4 tiny and huge", &["dst=10.2.0.1", "count=3", "size=20"], 3));
    compare(
        &t,
        &mk(
            "v4 jumbo clamps (veth refuses them)",
            &["dst=10.2.0.1", "count=3", "size=20000"],
            0,
        ),
    );
    compare(
        &t,
        &mk(
            "v6",
            &["dst=2001:db8:20::77", "src=fc00:1::2", "count=40", "flows=4"],
            40,
        ),
    );
    compare(
        &t,
        &mk(
            "v6 default src, dst_n",
            &["dst=fc00:2::fe", "count=30", "dst_n=30", "tos=40"],
            30,
        ),
    );
    // the tag is cut off at 64 bytes: filter on its first bytes (the checksum does not cover the run id)
    let mut tiny = mk("v6 tiny", &["dst=fc00:2::1", "count=4", "size=64"], 4);
    tiny.tag = b"L3";
    tiny.mask_run_id = false;
    compare(&t, &tiny);
    compare(
        &t,
        &mk("smac", &["dst=10.2.0.1", "count=5", "smac=02:00:00:00:0a:02"], 5),
    );
    compare(&t, &mk("paced", &["dst=10.2.0.1", "count=20", "pps=500"], 20));
    compare(&t, &mk("count 0", &["dst=10.2.0.1", "count=0"], 0));
    compare(&t, &sc("usage", &["l3-frames", "tx=l3a"], "l3b", b"L3FRAMES", 0));
    compare(
        &t,
        &sc(
            "mixed family",
            &[
                "l3-frames",
                "tx=l3a",
                "rx=l3b",
                "dst=10.0.0.1",
                "src=fc00::1",
                "dmac=02:00:00:00:01:0a",
            ],
            "l3b",
            b"L3FRAMES",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "bad dmac",
            &["l3-frames", "tx=l3a", "rx=l3b", "dst=10.0.0.1", "dmac=xx"],
            "l3b",
            b"L3FRAMES",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "bad smac",
            &[
                "l3-frames",
                "tx=l3a",
                "rx=l3b",
                "dst=10.0.0.1",
                "dmac=02:00:00:00:01:0a",
                "smac=1",
            ],
            "l3b",
            b"L3FRAMES",
            0,
        ),
    );
    compare(
        &t,
        &sc(
            "no such tx",
            &[
                "l3-frames",
                "tx=nonesuch0",
                "rx=l3b",
                "dst=10.0.0.1",
                "dmac=02:00:00:00:01:0a",
            ],
            "l3b",
            b"L3FRAMES",
            0,
        ),
    );
}

// ------------------------------------------------------------------------------ sflow

/// A raw sample frame as the ipd driver hands it over: Ethernet / IPv4 / GRE 0x8818 / header / frame.
fn sample_frame(counter: u32, packets: u32, inner: &[u8]) -> Vec<u8> {
    let mut f = vec![0u8; 14 + 20 + 28];
    f[12] = 8;
    f[14] = 0x45;
    f[22] = 64;
    f[23] = 47;
    f[26..30].copy_from_slice(&[10, 0, 0, 1]);
    f[30..34].copy_from_slice(&[10, 0, 0, 2]);
    let g = 34;
    f[g + 2] = 0x88;
    f[g + 3] = 0x18;
    f[g + 4..g + 8].copy_from_slice(&[0, 0, 0, 1]);
    // ext_type 5, dir 1, sp 77 / sp2 99
    f[g + 8..g + 12].copy_from_slice(&(5u32 << 26 | 0x8000 | 0x4d).to_be_bytes());
    f[g + 12..g + 16].copy_from_slice(&99u32.to_be_bytes());
    f[g + 16..g + 20].copy_from_slice(&counter.to_be_bytes());
    f[g + 20..g + 24].copy_from_slice(&packets.to_be_bytes());
    f.extend_from_slice(inner);
    f
}

/// The chip emulator: samples every 10th frame (sequence 3 mod 10) of the sender, preceded by junk.
fn emulate_chip(seen: &str, out: &str, stop: &AtomicBool) {
    let ix_seen = if_nametoindex(seen);
    let sock = Sock::packet(3).unwrap();
    sock.bind_packet(ix_seen, 3).unwrap();
    sock.set_recv_timeout(Duration::from_millis(50)).unwrap();
    let tx = Sock::packet(0).unwrap();
    let ix_out = if_nametoindex(out);
    thread::sleep(Duration::from_millis(300));
    tx.send_to_packet(ix_out, [2, 0, 0, 0, 0, 9], &[0u8; 60]).unwrap();
    let mut buf = vec![0u8; 20000];
    let mut taken = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, t)) = sock.recv_from_packet(&mut buf, false) else {
            continue;
        };
        if n == 0 || t == PACKET_OUTGOING {
            continue;
        }
        let fr = &buf[..n];
        let Some(p) = find(fr, b"SFLOWPRB") else { continue };
        let seq = ne32(fr, p + 8);
        if seq % 10 == 3 {
            taken += 1;
            let s = sample_frame(1000 + taken, 5000 + seq + 1, fr);
            tx.send_to_packet(ix_out, [2, 0, 0, 0, 0, 9], &s).unwrap();
        }
    }
}

fn run_sflow(bin: &Path, args: &[String], env: &[(&str, &str)]) -> (i32, String, String) {
    let stop = AtomicBool::new(false);
    thread::scope(|sc| {
        let h = sc.spawn(|| emulate_chip("sfb", "sfd", &stop));
        let out = exec(bin, args, env, None);
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    })
}

fn sflow_netdev(label: &str, t: &Tool, args: &[&str], env: &[(&str, &str)]) {
    let a = s(args);
    let (cs, co, ce) = if t.live {
        run_sflow(&t.c, &a, env)
    } else {
        run_sflow(&t.rust, &a, env)
    };
    let (rs, ro, re) = run_sflow(&t.rust, &a, env);
    let cp = if t.live { &t.c } else { &t.rust };
    let (co, ro) = (mask(&co, &TIME_KEYS, cp), mask(&ro, &TIME_KEYS, &t.rust));
    let (ce, re) = (mask(&ce, &TIME_KEYS, cp), mask(&re, &TIME_KEYS, &t.rust));
    let key = format!("sflow-probe/{label}");
    if t.live {
        judge(&key, "C", record(cs, &co, &ce, &[]));
    }
    judge(&key, "Rust", record(rs, &ro, &re, &[]));
    println!("[sflow-probe] {label}: status {cs}/{rs}\n{ro}");
    assert_eq!(cs, rs, "{label}: status");
    assert_eq!(ce, re, "{label}: stderr");
    assert_eq!(co, ro, "{label}: stdout\n--- C\n{co}\n--- Rust\n{ro}");
}

fn sflow_scenarios() {
    let t = tool("sflow-probe");
    veth("sfa", "sfb");
    veth("sfc", "sfd");
    let tail = ["256", "02:99:99:99:99:99", "10.99.9.1", "10.99.9.2"];
    let mk = |count: &str, pps: &str, size: &str| {
        [
            "sflow-probe",
            "netdev",
            "sfa",
            "sfc",
            count,
            pps,
            size,
            tail[1],
            tail[2],
            tail[3],
        ]
        .map(str::to_owned)
    };
    for (label, count, pps, size, env) in [
        ("1000 frames", "1000", "0", "256", vec![]),
        ("paced, small", "200", "2000", "64", vec![]),
        ("hex dump", "100", "0", "256", vec![("HEX", "1")]),
        ("large frames", "100", "0", "1400", vec![]),
        ("hex count", "0x40", "0", "128", vec![]),
    ] {
        let a = mk(count, pps, size);
        let r: Vec<&str> = a.iter().map(String::as_str).collect();
        sflow_netdev(label, &t, &r, &env);
    }
    // listen only
    let a = mk("0", "0", "256");
    let r: Vec<&str> = a.iter().map(String::as_str).collect();
    sflow_netdev("listen", &t, &r, &[("LISTEN", "1")]);
    sflow_netdev("usage", &t, &["sflow-probe", "netdev"], &[]);
    sflow_netdev(
        "bad mode",
        &t,
        &[
            "sflow-probe",
            "udp",
            "a",
            "b",
            "1",
            "1",
            "64",
            tail[1],
            tail[2],
            tail[3],
        ],
        &[],
    );
    sflow_netdev(
        "bad mac",
        &t,
        &[
            "sflow-probe",
            "netdev",
            "sfa",
            "sfc",
            "1",
            "1",
            "64",
            "zz",
            tail[2],
            tail[3],
        ],
        &[],
    );
    sflow_netdev(
        "no such rx",
        &t,
        &[
            "sflow-probe",
            "netdev",
            "sfa",
            "nonesuch0",
            "1",
            "1",
            "64",
            tail[1],
            tail[2],
            tail[3],
        ],
        &[],
    );
    // no psample family in the kernel of the namespace (module not loaded) or the real one: equal either way
    sflow_netdev(
        "psample, kernel",
        &t,
        &[
            "sflow-probe",
            "psample",
            "sfa",
            "0",
            "0",
            "0",
            "64",
            tail[1],
            tail[2],
            tail[3],
        ],
        &[("LISTEN", "1")],
    );
}

/// `iif=5(sga)` -> `iif=*(sga)` (the kernel's interface index is not stable across runs).
fn mask_ifindex(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("iif=") {
        let (head, tail) = rest.split_at(p + 4);
        out.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && tail[digits..].starts_with('(') {
            out.push('*');
            rest = &tail[digits..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// psample mode against the canned family and messages of `tests/fixtures/psample_shim.c`.
fn psample_scenarios() {
    let t = tool("sflow-probe");
    let shim = std::env::var("TL_FRAMES_SHIM").unwrap();
    veth("sga", "sgb");
    let iif = if_nametoindex("sga").to_string();
    for (label, group, count, size) in [
        ("any group", "0", "300", "128"),
        ("group 1", "1", "300", "128"),
        ("group 2", "2", "200", "128"),
        ("hex, other size", "0x0", "120", "256"),
    ] {
        let args = s(&[
            "sflow-probe",
            "psample",
            "sga",
            group,
            count,
            "0",
            size,
            "02:99:99:99:99:99",
            "10.99.9.1",
            "10.99.9.2",
        ]);
        let env = [
            ("LD_PRELOAD", shim.as_str()),
            ("SHIM_COUNT", count),
            ("SHIM_IIF", iif.as_str()),
            ("SHIM_SIZE", size),
            ("SHIM_RATE", "1000"),
            ("HEX", "1"),
        ];
        let run = |bin: &Path| {
            let o = exec(bin, &args, &env, None);
            (
                o.status.code().unwrap_or(-1),
                mask(&String::from_utf8_lossy(&o.stdout), &TIME_KEYS, bin),
                mask(&String::from_utf8_lossy(&o.stderr), &TIME_KEYS, bin),
            )
        };
        let r = run(&t.rust);
        let c = if t.live { run(&t.c) } else { r.clone() };
        let key = format!("sflow-probe/psample {label}");
        // the ifindex of the veth depends on which group created its pair first: not part of the golden
        if t.live {
            judge(&key, "C", record(c.0, &mask_ifindex(&c.1), &c.2, &[]));
        }
        judge(&key, "Rust", record(r.0, &mask_ifindex(&r.1), &r.2, &[]));
        println!("[sflow-probe] psample {label}: status {}/{}\n{}", c.0, r.0, r.1);
        assert_eq!(c.0, r.0, "{label}: status");
        assert_eq!(c.2, r.2, "{label}: stderr");
        assert_eq!(c.1, r.1, "{label}: stdout\n--- C\n{}\n--- Rust\n{}", c.1, r.1);
        assert!(
            c.1.contains("PSAMPLE family=55 group_packets=101"),
            "{label}: the canned family was not used: {}",
            c.1
        );
        assert!(c.1.contains("META iif="), "{label}: no metadata: {}", c.1);
    }
}

#[test]
fn inner() {
    if std::env::var(INNER).is_err() {
        return;
    }
    // no kernel-generated noise on the veth ends
    for f in ["all", "default"] {
        let _ = std::fs::write(format!("/proc/sys/net/ipv6/conf/{f}/disable_ipv6"), "1");
    }
    ip(&["link", "set", "dev", "lo", "up"]);
    let jobs: Vec<(&str, fn())> = vec![
        ("trap", trap_scenarios as fn()),
        ("raw", raw_scenarios),
        ("qos", qos_scenarios),
        ("l3", l3_scenarios),
        ("sflow", sflow_scenarios),
        ("psample", psample_scenarios),
    ];
    // veth names are per namespace and distinct per tool: the groups run side by side
    let results: Vec<_> = jobs.into_iter().map(|(name, f)| (name, thread::spawn(f))).collect();
    let mut failed = Vec::new();
    for (name, h) in results {
        if h.join().is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "differential failed for {failed:?}");
    let seen = SEEN.lock().unwrap();
    if let Ok(path) = std::env::var("TL_FRAMES_CAPTURE") {
        // capture mode: the C tools' records, in key order (the golden's body)
        let body: String = seen
            .iter()
            .map(|(key, rec)| ["@@ ", key.as_str(), "\n", rec.as_str()].concat())
            .collect();
        std::fs::write(path, body).unwrap();
        return;
    }
    // every golden scenario ran, and nothing ran that the golden does not hold
    let want: Vec<&String> = golden().keys().collect();
    let got: Vec<&String> = seen.keys().collect();
    assert_eq!(want, got, "scenarios run vs golden scenarios");
}

/// The golden holds the 70 scenarios of the C tools' behaviour and names its sources.
#[test]
fn golden_shape() {
    let g = golden();
    assert_eq!(g.len(), 70, "scenarios in the golden");
    for (k, v) in g {
        assert!(v.starts_with("status "), "{k}: record shape");
    }
    for tool in ["trap-frames", "qos-frames", "raw-frames", "l3-frames", "sflow-probe"] {
        assert!(
            g.keys().any(|k| k.starts_with(&format!("{tool}/"))),
            "{tool}: no scenario"
        );
    }
    assert!(GOLDEN.contains("NOT to be regenerated from the Rust output"));
    // the header pins each C source by sha256
    for f in [
        "trap_frames.c",
        "qos_frames.c",
        "raw_frames.c",
        "l3_frames.c",
        "sflow_probe.c",
    ] {
        assert!(GOLDEN.contains(f), "header lacks {f}");
    }
}

/// Negative control: one changed byte (a frame, the status, the stdout) or a missing scenario makes
/// the comparison fail, so the golden cannot pass by being ignored.
#[test]
fn golden_negative_control() {
    let g = golden();
    let (key, rec) = g
        .iter()
        .find(|(_, v)| v.contains("\nf "))
        .expect("a scenario with frames");
    assert!(golden_check(g, key, rec).is_ok(), "an unchanged record matches");
    // flip one hex digit of the first frame
    let at = rec.find("\nf ").unwrap() + 3;
    let mut bad = rec.clone().into_bytes();
    bad[at] = if bad[at] == b'0' { b'1' } else { b'0' };
    let bad = String::from_utf8(bad).unwrap();
    assert!(
        golden_check(g, key, &bad).is_err(),
        "a changed frame byte must be reported"
    );
    // a dropped frame, a changed status, a changed stdout, an unknown scenario
    let dropped: String = rec.lines().take(4 + 1).map(|l| [l, "\n"].concat()).collect();
    assert!(
        golden_check(g, key, &dropped).is_err(),
        "a missing frame must be reported"
    );
    let status = rec.replacen("status 0", "status 9", 1);
    assert!(
        golden_check(g, key, &status).is_err(),
        "a changed status must be reported"
    );
    let out = rec.replacen("stdout \"", "stdout \"x", 1);
    assert!(golden_check(g, key, &out).is_err(), "a changed stdout must be reported");
    assert!(
        golden_check(g, "nonesuch/none", rec).is_err(),
        "an unknown scenario must be reported"
    );
    // the whole-text control: mutate the embedded golden itself in memory
    let mut text = GOLDEN.to_owned().into_bytes();
    let p = GOLDEN.rfind("\nf ").map_or(GOLDEN.len() - 2, |i| i + 3);
    text[p] = if text[p] == b'0' { b'1' } else { b'0' };
    let mutated = parse_golden(&String::from_utf8(text).unwrap());
    assert_ne!(&mutated, g, "the mutated golden differs");
    let k = mutated.keys().find(|k| mutated[*k] != g[*k]).unwrap();
    assert!(golden_check(g, k, &mutated[k]).is_err());
}

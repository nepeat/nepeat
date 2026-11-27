//! The Teralynx libsai end to end: the C++ `test_ifcs` (test_ifcs.cpp, t_<area>.cpp, harness.h of the C++
//! sai-teralynx, deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0) in Rust. The BUILT Rust `libsai.so` (with the Teralynx backend) is driven
//! through its C ABI by `sai-client` while the C mock libifcs (`rust/sai/tests/teralynx-mock`) stands
//! in for the vendor libifcs.so; the mock's hooks (call log, link and L2 events, state counters)
//! are called by name as the C++ test does with `dlsym`. Same profile files, same areas in the same
//! order on one switch, every `CHECK` / `CHECK_EQ` / `OK` of the C++ ported one to one:
//! `CHECK-MAP.txt` lists each C++ check (file:line) with its Rust assertion and its executions.
//!
//! The run is compared with `golden/test_ifcs-checks-cpp.txt`: the C++ test's own checks (its
//! macros instrumented to print each one's file:line and outcome) on the C++ libsai, in order, with
//! the per-section counts (frozen, captured before the C++ was deleted). Equal sequence and equal sections, all passing, is the gate.
//!
//! Inputs (environment): `IFCS_E2E_LIBSAI` (absolute path of the Rust libsai; the shim is loaded
//! from its directory), `IFCS_E2E_MOCK` (absolute path of the mock libifcs.so), `IFCS_E2E_DIR`
//! (scratch dir, removed first; the diag socket needs a path under 100 bytes; default: a directory
//! in the temp dir), `IFCS_E2E_FIXTURES` (the directory with `golden/` and `CHECK-MAP.txt`; default: this
//! one in the source tree; the cross-built test bundle ships them). Without the first two the test skips with a line, unless
//! `IFCS_E2E_REQUIRE=1` (the Nix checks): then it fails. `IFCS_E2E_WRITE_MAP=<file>` writes the
//! check map of this run instead of comparing it. `TEST_IFCS_DUMP=1` prints the mock's call log
//! before the switch is removed (as the C++ test does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::items_after_statements,
    clippy::doc_markdown,
    clippy::cognitive_complexity,
    clippy::needless_pass_by_value,
    clippy::unreadable_literal,
    clippy::type_complexity,
    clippy::struct_field_names,
    clippy::manual_assert,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::format_collect,
    unreachable_pub,
    missing_docs,
    reason = "test code, a port of the C++ test's style"
)]

#[macro_use]
mod support;
mod t_acl;
mod t_copp;
mod t_counters;
mod t_hooks;
mod t_hostif;
mod t_l2;
mod t_l3;
mod t_lag;
mod t_learn;
mod t_mirror;
mod t_port;
mod t_qos;
mod t_sflow;
mod t_switch;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use sai_client::{Notify, set_event_sink};
use support::{Api, Attr, Ck, Env, Event, Events, FdbEvent, Mock, SUCCESS, Sai, Value, put, sys};

/// The inputs from the environment.
struct Config {
    libsai: String,
    mock: String,
    dir: PathBuf,
}

fn required() -> bool {
    std::env::var("IFCS_E2E_REQUIRE").is_ok_and(|v| v == "1")
}

fn config() -> Option<Config> {
    let libsai = std::env::var("IFCS_E2E_LIBSAI").ok().filter(|v| !v.is_empty());
    let mock = std::env::var("IFCS_E2E_MOCK").ok().filter(|v| !v.is_empty());
    let (Some(libsai), Some(mock)) = (libsai, mock) else {
        assert!(
            !required(),
            "ifcs_e2e required but IFCS_E2E_LIBSAI / IFCS_E2E_MOCK are not set"
        );
        eprintln!("ifcs_e2e: SKIPPED (IFCS_E2E_LIBSAI and IFCS_E2E_MOCK name the built libsai and the mock libifcs)");
        return None;
    };
    assert!(
        Path::new(&libsai).is_absolute(),
        "IFCS_E2E_LIBSAI must be absolute: {libsai}"
    );
    assert!(
        Path::new(&mock).is_absolute(),
        "IFCS_E2E_MOCK must be absolute (the libsai opens it by this path): {mock}"
    );
    let dir = std::env::var_os("IFCS_E2E_DIR").map_or_else(
        || std::env::temp_dir().join(format!("ifcs-e2e-{}", std::process::id())),
        PathBuf::from,
    );
    Some(Config { libsai, mock, dir })
}

/// `g_profile` of test_ifcs.cpp (a sorted map: the order the C++'s `std::map` serves it in).
#[derive(Default)]
struct Profile(BTreeMap<String, String>);

impl Profile {
    fn set(&mut self, k: &str, v: &str) {
        self.0.insert(k.to_owned(), v.to_owned());
    }

    /// Serves this profile to the library from now on. Under the parity harness, `PARITY_PROFILE_ADD=<key>=<value>`
    /// is one more key, served after the others (the byte tap's `SAI_TERALYNX_IFCS_TRACE`: the profile is fixed
    /// otherwise), as the C++ test's `sai_api_initialize` wrapper did.
    fn serve(&self) {
        let mut pairs: Vec<(String, String)> = self.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some((k, v)) = std::env::var("PARITY_PROFILE_ADD")
            .ok()
            .as_deref()
            .and_then(|a| a.split_once('='))
        {
            if !self.0.contains_key(k) {
                pairs.push((k.to_owned(), v.to_owned()));
            }
        }
        sai_client::replace_profile(pairs);
    }
}

/// `main()` of test_ifcs.cpp up to the areas: profile files, refusals, the switch.
fn main_area(cfg: &Config) -> Env {
    let dir = cfg.dir.to_string_lossy().into_owned();
    let _ = std::fs::remove_dir_all(&cfg.dir);
    std::fs::create_dir_all(&cfg.dir).expect("scratch dir");
    let init = format!("{dir}/init.yaml");
    let isg = format!("{dir}/isg.yaml");
    let map = format!("{dir}/ports.map");
    let cmd1 = format!("{dir}/a.cmd");
    let cmd2 = format!("{dir}/b.cmd");
    let run = format!("{dir}/run");
    put(
        &init,
        "# generated init config\nifcs:\n  options:\n    log_level: \"info\"\n",
    );
    put(&isg, "  isg:\n  - id: \"0\"\n");
    put(
        &map,
        "# devport lanes speed fec\n\
         1 249,250,251,252,253,254,255,256 400000 rs\n\
         2 241,242,243,244,245,246,247,248 400000 rs   # cage 2\n\
         3 121 10000 none\n",
    );
    put(
        &cmd1,
        "node create 0\n\
         ifcs set devport 1 rx_eq_fine_tune 1\n\
         ifcs set devport 1 tx_eq_pre1 [2,2,2,2,2,2,2,2]\r\n\
         ifcs set devport 2 rx_ctle_lf [1, 2, 3, 4]\n\
         pci write led_cfg sck_rate_f 0x8\n\
         pci write led_refresh_cliff 0x020\n",
    );
    put(
        &cmd2,
        "ifcs set devport 3 rx_gainshape1 [5]\n\
         ifcs set devport 1 link_training 1\n\
         ifcs set devport 1 bogus_attr 3\n\
         port enable 1\n\
         # comment\n",
    );

    let events = Arc::new(Mutex::new(Events::default()));
    let sink = Arc::clone(&events);
    set_event_sink(move |ev| {
        let mut g = sink.lock().unwrap_or_else(PoisonError::into_inner);
        match ev {
            Event::PortState { port, state } => g.port.push((*port, *state)),
            Event::Fdb { event, mac, bv, .. } => g.fdb.push(FdbEvent {
                event: *event,
                bv: *bv,
                mac: *mac,
            }),
            Event::Shutdown => {}
        }
    });

    let sai = Sai::load(&cfg.libsai).expect("dlopen the libsai");
    let mut e = Env {
        sai,
        mock: None,
        ck: Ck::default(),
        events,
        s: 0,
        p1: 0,
        p2: 0,
        p3: 0,
        hif: 0,
        vlan: 0,
        bp1: 0,
        dir: dir.clone(),
        init: init.clone(),
        isg: isg.clone(),
        run: run.clone(),
    };
    e.ck.section("main");
    let mut prof = Profile::default();
    prof.serve();
    ok!(e, "test_ifcs.cpp:87", e.sai.initialize());
    ok!(e, "test_ifcs.cpp:93", e.sai.query(Api::Switch));
    ok!(e, "test_ifcs.cpp:94", e.sai.query(Api::Port));
    ok!(e, "test_ifcs.cpp:95", e.sai.query(Api::Hostif));
    ok!(e, "test_ifcs.cpp:96", e.sai.query(Api::Vlan));
    ok!(e, "test_ifcs.cpp:97", e.sai.query(Api::Bridge));
    ok!(e, "test_ifcs.cpp:98", e.sai.query(Api::Route));
    ok!(e, "test_ifcs.cpp:99", e.sai.query(Api::RouterInterface));
    ok!(e, "test_ifcs.cpp:100", e.sai.query(Api::Neighbor));
    ok!(e, "test_ifcs.cpp:101", e.sai.query(Api::NextHop));
    ok!(e, "test_ifcs.cpp:102", e.sai.query(Api::Fdb));
    ok!(e, "test_ifcs.cpp:103", e.sai.query(Api::Lag));
    ok!(e, "test_ifcs.cpp:104", e.sai.query(Api::Policer));
    ok!(e, "test_ifcs.cpp:105", e.sai.query(Api::Acl));

    let ca = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
            Value::Notify(Notify::PortState),
        ),
        Attr::new(sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY, Value::Notify(Notify::Fdb)),
    ];

    // ---- refusals before anything is loaded
    for (k, v) in [
        ("SAI_OPENBCM_BACKEND", "openbcm"),
        ("SAI_INIT_CONFIG_FILE", init.as_str()),
        ("SAI_TERALYNX_LIBIFCS", cfg.mock.as_str()),
        ("SAI_TERALYNX_RUN_DIR", run.as_str()),
    ] {
        prof.set(k, v);
    }
    prof.serve();
    let st = e.sai.create_switch(&mut e.s, &ca);
    check!(e, "test_ifcs.cpp:121", st != SUCCESS); // no port map
    prof.set("SAI_TERALYNX_PORT_MAP", &map);
    prof.set("SAI_TERALYNX_LIBIFCS", &format!("{dir}/does-not-exist.so"));
    prof.serve();
    let st = e.sai.create_switch(&mut e.s, &ca);
    check!(e, "test_ifcs.cpp:124", st != SUCCESS); // dlopen fails
    prof.set("SAI_TERALYNX_LIBIFCS", &cfg.mock);
    prof.set("SAI_INIT_CONFIG_FILE", &format!("{dir}/missing.yaml"));
    prof.serve();
    let st = e.sai.create_switch(&mut e.s, &ca);
    check!(e, "test_ifcs.cpp:127", st != SUCCESS); // no init yaml
    check!(e, "test_ifcs.cpp:128", !Mock::is_loaded(&cfg.mock)); // nothing loaded yet

    // ---- create_switch
    prof.set("SAI_INIT_CONFIG_FILE", &init);
    prof.set("SAI_TERALYNX_CONFIG_APPEND", &isg);
    prof.set("SAI_TERALYNX_INIT_CMDS", &format!("{cmd1}:{cmd2}"));
    prof.set("SAI_TERALYNX_LINK_STATUS", "cached"); // t_port tests the cache (the default reads IFCS)
    prof.set("SAI_TERALYNX_NETDEV", "create"); // t_hostif tests create mode (the default adopts)
    if format!("{dir}/diag.sock").len() < 100 {
        prof.set("SAI_OPENBCM_DIAG_SOCKET", &format!("{dir}/diag.sock")); // t_counters
    }
    prof.set(
        "SAI_TERALYNX_HASH_FIELDS",
        "src_ip,dst_ip,ip_protocol,l4_src_port,l4_dst_port,in_port",
    ); // t_lag
    prof.set("SAI_TERALYNX_HASH_SEED", "10");
    prof.set("SAI_TERALYNX_SFLOW_CPU_QUEUE", "2"); // t_sflow: the samples' queue
    prof.set("SAI_TERALYNX_SFLOW_METER", "on"); // t_sflow: the sflow group's policer as that queue's meter
    prof.serve();
    let st = e.sai.create_switch(&mut e.s, &ca);
    ok!(e, "test_ifcs.cpp:141", st);
    let mock = Mock::attach(&cfg.mock);
    check!(e, "test_ifcs.cpp:143", mock.is_ok());
    match mock {
        Ok(m) => e.mock = Some(m),
        Err(why) => panic!("the mock is not loaded after create_switch: {why}"),
    }
    e
}

fn parse_golden(text: &str) -> (Vec<(String, u32, u32)>, Vec<(String, bool)>) {
    let mut sections = Vec::new();
    let mut seq = Vec::new();
    for l in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w.as_slice() {
            ["section", name, checks, failed] => sections.push((
                (*name).to_owned(),
                checks.parse().expect("golden section checks"),
                failed.parse().expect("golden section failed"),
            )),
            ["CK", id, outcome] => seq.push(((*id).to_owned(), *outcome == "P")),
            _ => panic!("golden: {l}"),
        }
    }
    (sections, seq)
}

/// C++ check ids in order: by C++ file and line (not alphabetically).
fn id_key(id: &str) -> (String, u32) {
    let (file, line) = id.split_once(':').unwrap_or((id, "0"));
    (file.to_owned(), line.parse::<u32>().unwrap_or(0))
}

/// Every C++ id literal (`"t_port.cpp:50"`) of the Rust sources in `dir`: id -> (Rust file, line).
fn declared_ids(dir: &Path) -> Option<BTreeMap<String, (String, usize)>> {
    let mut out = BTreeMap::new();
    let mut files = 0;
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy().into_owned();
        if !(name == "main.rs" || (name.starts_with("t_") && name.ends_with(".rs"))) {
            continue;
        }
        files += 1;
        for (i, line) in std::fs::read_to_string(&path).ok()?.lines().enumerate() {
            for (k, seg) in line.split('"').enumerate() {
                let is_id = k % 2 == 1
                    && seg.split_once(".cpp:").is_some_and(|(f, n)| {
                        (f == "test_ifcs" || f.starts_with("t_"))
                            && !n.is_empty()
                            && n.bytes().all(|c| c.is_ascii_digit())
                    });
                if is_id {
                    out.insert(seg.to_owned(), (name.clone(), i + 1));
                }
            }
        }
    }
    (files > 1).then_some(out)
}

/// The check map: C++ id, the Rust assertion (file:line), executions; sorted by C++ file and line.
/// The sites that never run in this run (an `if (own_trap) OK(...)` that is not taken) have 0
/// executions and are found in the Rust sources (`declared`).
fn map_text(ck: &Ck, declared: &BTreeMap<String, (String, usize)>) -> String {
    let mut rows: BTreeMap<(String, u32), String> = BTreeMap::new();
    for (id, (rust, n)) in &ck.sites {
        let rust = rust.rsplit('/').next().unwrap_or(rust);
        rows.insert(id_key(id), format!("{id}\t{rust}\t{n}\n"));
    }
    for (id, (file, line)) in declared {
        if !ck.sites.contains_key(id) {
            rows.insert(id_key(id), format!("{id}\t{file}:{line}\t0\n"));
        }
    }
    rows.into_values().collect()
}

/// Panics with the first differing line of two texts (not the texts).
fn assert_same_lines(what: &str, left: &str, right: &str) {
    if left == right {
        return;
    }
    let (l, r): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
    let at = l
        .iter()
        .zip(&r)
        .position(|(a, b)| a != b)
        .unwrap_or(l.len().min(r.len()));
    panic!(
        "{what}: {} vs {} lines, first difference at line {}: {:?} vs {:?}",
        l.len(),
        r.len(),
        at + 1,
        l.get(at),
        r.get(at)
    );
}

#[test]
fn ifcs_e2e() {
    let Some(cfg) = config() else { return };
    let mut e = main_area(&cfg);

    t_switch::test_switch_init(&mut e);
    t_port::test_ports(&mut e);
    t_hostif::test_hostif(&mut e);
    t_l2::test_l2(&mut e);
    t_l3::test_l3(&mut e);
    t_lag::test_lag(&mut e);
    t_copp::test_copp(&mut e);
    t_acl::test_acl(&mut e);
    t_qos::test_qos(&mut e);
    t_mirror::test_mirror(&mut e);
    t_sflow::test_sflow(&mut e);
    t_counters::test_counters(&mut e);
    t_hooks::test_hooks(&mut e);
    t_learn::test_learn_guard(&mut e);
    t_switch::test_switch_misc(&mut e);

    // TEST_IFCS_DUMP=1: print every IFCS call the backend made (before removal)
    if std::env::var_os("TEST_IFCS_DUMP").is_some() {
        eprint!("{}", e.m().log());
    }

    t_switch::test_switch_remove(&mut e);
    e.ck.section("uninit");
    ok!(e, "test_ifcs.cpp:179", e.sai.uninitialize());

    e.ck.finish();
    println!("test_ifcs: {} checks, {} failed", e.ck.checks(), e.ck.failed());
    if e.ck.failed() > 0 {
        eprintln!("--- mock log at failure ---\n{}", e.m().log());
    }
    assert_eq!(e.ck.failed(), 0, "checks failed (FAIL lines above)");

    // the gate: the C++ test's own sequence of checks and outcomes, in order, with its sections
    let dir = std::env::var_os("IFCS_E2E_FIXTURES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ifcs_e2e"),
        PathBuf::from,
    );
    let golden = std::fs::read_to_string(dir.join("golden/test_ifcs-checks-cpp.txt")).expect("the golden");
    let (g_sections, g_seq) = parse_golden(&golden);
    assert_eq!(
        e.ck.sections, g_sections,
        "checks per section differ from the C++ test's"
    );
    if let Some((i, (got, want))) = e.ck.seq.iter().zip(&g_seq).enumerate().find(|(_, (a, b))| a != b) {
        panic!("check #{i} is {got:?}, the C++ test's is {want:?}");
    }
    assert_eq!(e.ck.seq.len(), g_seq.len(), "checks run vs the C++ test's");

    // the map: every C++ check, its Rust assertion, its executions (0: a site this run does not reach)
    let declared = declared_ids(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ifcs_e2e"));
    if let Some(path) = std::env::var_os("IFCS_E2E_WRITE_MAP") {
        let header = std::fs::read_to_string(dir.join("CHECK-MAP.txt"))
            .map(|t| {
                t.lines()
                    .take_while(|l| l.starts_with('#'))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let rows = map_text(&e.ck, &declared.expect("the Rust sources of the test"));
        std::fs::write(&path, format!("{header}\n{rows}")).expect("write the map");
    } else {
        let committed = std::fs::read_to_string(dir.join("CHECK-MAP.txt")).expect("CHECK-MAP.txt");
        // C++ id, Rust file, executions (the line numbers of the map are informational)
        let mut rows = Vec::new();
        for l in committed.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let w: Vec<&str> = l.split('\t').collect();
            let [id, rust, n] = w.as_slice() else {
                panic!("CHECK-MAP.txt: {l}")
            };
            rows.push((
                (*id).to_owned(),
                rust.split(':').next().unwrap_or(rust).to_owned(),
                n.parse::<u32>().expect("executions"),
            ));
        }
        let ran: String = rows
            .iter()
            .filter(|(_, _, n)| *n > 0)
            .map(|(id, file, n)| format!("{id}\t{file}\t{n}\n"))
            .collect();
        let actual: String = {
            let mut sites: Vec<(&String, &(String, u32))> = e.ck.sites.iter().collect();
            sites.sort_by_key(|(id, _)| id_key(id));
            sites
                .into_iter()
                .map(|(id, (rust, n))| {
                    let file = rust.rsplit('/').next().unwrap_or(rust);
                    format!("{id}\t{}\t{n}\n", file.split(':').next().unwrap_or(file))
                })
                .collect()
        };
        assert_same_lines(
            "CHECK-MAP.txt vs this run (IFCS_E2E_WRITE_MAP regenerates it)",
            &actual,
            &ran,
        );
        if let Some(declared) = &declared {
            // the sources agree with the map: every declared id is a row, every zero row is declared
            let in_map: BTreeSet<&String> = rows.iter().map(|(id, _, _)| id).collect();
            let decl: BTreeSet<&String> = declared.keys().collect();
            assert_eq!(in_map, decl, "the ids in the Rust sources and in CHECK-MAP.txt differ");
        }
        // the C++ sources are gone: the map's rows (frozen at the deletion) are the list of the C++ check sites
    }
}

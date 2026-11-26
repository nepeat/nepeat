//! The CLI end to end, without hardware: a fake Home Assistant HTTP server (the contract the
//! deleted Python tests checked) and a fake `ssh` on PATH that plays the recorded `nboutlet`
//! transcript and its error cases. Secrets used here must never show up in output or argv.
#![allow(missing_docs)]
// a test harness: failing loudly is the point
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

const HA_KEY: &str = "test-secret";
const NB_PASSWORD: &str = "nb-pass-Zq81";
static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lab-power-it-{}-{tag}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// --- fake Home Assistant -------------------------------------------------------

#[derive(Default)]
struct HaState {
    state: String,
    error: Option<u16>,
    stuck: bool,
    watts: String,
    calls: Vec<(String, String, Option<Value>, String)>,
}

struct FakeHa {
    port: u16,
    st: Arc<Mutex<HaState>>,
}

impl FakeHa {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let st = Arc::new(Mutex::new(HaState {
            state: "off".into(),
            watts: "41.5".into(),
            ..HaState::default()
        }));
        let shared = Arc::clone(&st);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let st = Arc::clone(&shared);
                thread::spawn(move || serve(stream, &st));
            }
        });
        Self { port, st }
    }

    fn calls(&self) -> Vec<(String, String, Option<Value>, String)> {
        self.st.lock().unwrap().calls.clone()
    }

    fn set(&self, f: impl FnOnce(&mut HaState)) {
        f(&mut self.st.lock().unwrap());
    }
}

fn serve(stream: std::net::TcpStream, st: &Mutex<HaState>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let (mut length, mut auth) = (0usize, String::new());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (k, v) = line.split_once(':').unwrap();
        match k.to_ascii_lowercase().as_str() {
            "content-length" => length = v.trim().parse().unwrap(),
            "authorization" => v.trim().clone_into(&mut auth),
            _ => {}
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let data: Option<Value> = (method == "POST").then(|| serde_json::from_slice(&body).unwrap());
    let mut s = st.lock().unwrap();
    s.calls.push((method.clone(), path.clone(), data.clone(), auth));
    let mut out = stream;
    if let Some(code) = s.error {
        let location = if code == 302 {
            "Location: /redirect-target\r\n"
        } else {
            ""
        };
        let msg = format!("HTTP/1.1 {code} X\r\n{location}Content-Length: 21\r\n\r\nprivate response body");
        let _ = out.write_all(msg.as_bytes());
        return;
    }
    let reply = if data.is_some() {
        if !s.stuck {
            path.rsplit("turn_").next().unwrap().clone_into(&mut s.state);
        }
        "[]".to_owned()
    } else {
        let entity = path.rsplit('/').next().unwrap().to_owned();
        if entity.starts_with("sensor.") {
            serde_json::json!({"entity_id": entity, "state": s.watts, "attributes": {"unit_of_measurement": "W"}})
                .to_string()
        } else {
            serde_json::json!({"entity_id": entity, "state": s.state}).to_string()
        }
    };
    // chunked, as aiohttp may send it
    let msg = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{reply}\r\n0\r\n\r\n",
        reply.len()
    );
    let _ = out.write_all(msg.as_bytes());
}

// --- fake NetBotz (ssh stub) ---------------------------------------------------

struct FakeNb {
    dir: PathBuf,
    bin: PathBuf,
    creds: PathBuf,
    known_hosts: PathBuf,
}

impl FakeNb {
    fn new(scenario: &str, state: &str) -> Self {
        let dir = scratch("nb");
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("ssh");
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-ssh.sh"), &stub).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let private = dir.join("private");
        fs::create_dir_all(&private).unwrap();
        let creds = private.join("credentials.json");
        fs::write(&creds, format!(r#"{{"username": "apc", "password": "{NB_PASSWORD}"}}"#)).unwrap();
        let known_hosts = private.join("known_hosts");
        fs::write(&known_hosts, "10.0.0.9 ssh-rsa AAAAfake\n").unwrap();
        for f in [&creds, &known_hosts] {
            fs::set_permissions(f, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(dir.join("scenario"), scenario).unwrap();
        fs::write(dir.join("state"), format!("{state}\n")).unwrap();
        fs::write(dir.join("password"), NB_PASSWORD).unwrap();
        Self {
            dir,
            bin,
            creds,
            known_hosts,
        }
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    fn commands(&self) -> Vec<String> {
        self.read("log")
            .lines()
            .filter_map(|l| l.strip_prefix("cmd "))
            .map(str::to_owned)
            .collect()
    }

    fn state(&self) -> String {
        self.read("state").trim().to_owned()
    }

    /// The fake's "Normal State" (Off when never set).
    fn normal(&self) -> String {
        let n = self.read("normal").trim().to_owned();
        if n.is_empty() { "Off".to_owned() } else { n }
    }

    fn clear_log(&self) {
        fs::write(self.dir.join("log"), "").unwrap();
    }
}

const TEST_CONFIG: &str = r#"{
  "targets": {
    "1": {"backend": "home-assistant", "entity": "switch.server_room_lab_plug", "feeds": "the test switch PSU2"},
    "m": {"backend": "home-assistant", "entity": "switch.metered", "power_sensor": "sensor.metered_power"},
    "nb": {"backend": "netbotz", "host": "10.0.0.9", "credentials_env": "FAKE_NB_CREDS",
           "known_hosts_env": "FAKE_NB_KH", "host_key_algorithms": "+ssh-rsa", "feeds": "the test switch PSU1"}
  },
  "devices": {"sw": {"description": "test switch", "feeds": ["1", "nb"]}}
}"#;

struct Env {
    ha: Option<u16>,
    config: bool,
    extra: Vec<(String, String)>,
}

fn run(env: &Env, nb: Option<&FakeNb>, args: &[&str]) -> (i32, String, String) {
    let home = scratch("home");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lab-power"));
    cmd.env_clear().args(args).env("HOME", &home);
    let path = std::env::var("PATH").unwrap_or_default();
    match nb {
        Some(nb) => {
            cmd.env("PATH", format!("{}:{path}", nb.bin.display()))
                .env("FAKE_SSH_DIR", &nb.dir)
                .env(
                    "FAKE_SSH_FIXTURES",
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
                )
                .env("FAKE_NB_CREDS", &nb.creds)
                .env("FAKE_NB_KH", &nb.known_hosts);
        }
        None => {
            cmd.env("PATH", path);
        }
    }
    if let Some(port) = env.ha {
        cmd.env("HA_BASE", format!("http://127.0.0.1:{port}"))
            .env("HA_KEY", HA_KEY);
    }
    if env.config {
        let cfg = home.join("targets.json");
        fs::write(&cfg, TEST_CONFIG).unwrap();
        cmd.env("LAB_POWER_CONFIG", cfg);
    }
    for (k, v) in &env.extra {
        cmd.env(k, v);
    }
    let Output { status, stdout, stderr } = cmd.output().unwrap();
    let (out, err) = (
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    );
    for secret in [HA_KEY, NB_PASSWORD] {
        assert!(
            !out.contains(secret) && !err.contains(secret),
            "secret leaked: {out}{err}"
        );
    }
    if let Some(nb) = nb {
        assert!(!nb.read("argv").contains(NB_PASSWORD), "password in ssh argv");
        assert!(!nb.read("env").contains(NB_PASSWORD), "password in ssh environment");
    }
    let _ = fs::remove_dir_all(&home);
    (status.code().unwrap_or(-1), out, err)
}

fn ha_env(ha: &FakeHa) -> Env {
    Env {
        ha: Some(ha.port),
        config: false,
        extra: vec![],
    }
}

fn all_env(ha: Option<&FakeHa>) -> Env {
    Env {
        ha: ha.map(|h| h.port),
        config: true,
        extra: vec![],
    }
}

fn short(calls: &[(String, String, Option<Value>, String)]) -> Vec<(String, String)> {
    calls.iter().map(|c| (c.0.clone(), c.1.clone())).collect()
}

// --- Home Assistant: the Python tests' cases ---------------------------------------

#[test]
fn ha_status_is_read_only_and_authenticated() {
    let ha = FakeHa::start();
    let (code, out, err) = run(&ha_env(&ha), None, &["1", "status", "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["state"], "off");
    assert_eq!(v["entity_id"], "switch.server_room_lab_plug");
    assert_eq!(v["power"], Value::Null);
    let calls = ha.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].0.as_str(), calls[0].1.as_str()),
        ("GET", "/api/states/switch.server_room_lab_plug")
    );
    assert_eq!(calls[0].3, format!("Bearer {HA_KEY}"));
    let (_, out, _) = run(&ha_env(&ha), None, &["1", "status"]);
    assert_eq!(
        out,
        "plug 1 (switch.server_room_lab_plug): off\n  power: no power meter on this target\n"
    );
}

#[test]
fn ha_on_off_target_and_verification() {
    let ha = FakeHa::start();
    let entity = "switch.server_room_lab_plug_2";
    for action in ["on", "off"] {
        ha.set(|s| s.calls.clear());
        let (code, out, err) = run(&ha_env(&ha), None, &["2", action]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains(&format!("plug 2 ({entity}): {action}")), "{out}");
        let calls = ha.calls();
        assert_eq!(
            short(&calls),
            [
                ("GET".into(), format!("/api/states/{entity}")),
                ("POST".into(), format!("/api/services/switch/turn_{action}")),
                ("GET".into(), format!("/api/states/{entity}")),
            ]
        );
        assert_eq!(calls[1].2, Some(serde_json::json!({ "entity_id": entity })));
    }
    // plug 2 feeds the console server: off says so
    let (_, _, err) = run(&ha_env(&ha), None, &["2", "off"]);
    assert!(err.contains("feeds dmc-12-48r-as0"), "{err}");
}

#[test]
fn ha_http_errors_hide_private_details() {
    let ha = FakeHa::start();
    for status in [401u16, 404, 500, 302] {
        ha.set(|s| s.error = Some(status));
        let (code, _, err) = run(&ha_env(&ha), None, &["1", "on"]);
        assert_eq!(code, 1);
        assert!(err.contains(&status.to_string()), "{err}");
        assert!(
            !err.contains("private response") && !err.contains("redirect-target"),
            "{err}"
        );
    }
    let calls = ha.calls();
    assert!(calls.iter().all(|c| c.0 == "GET"));
    assert_eq!(calls.len(), 4);
}

#[test]
fn ha_unavailable_prevents_switching() {
    let ha = FakeHa::start();
    ha.set(|s| s.state = "unavailable".into());
    assert_eq!(run(&ha_env(&ha), None, &["1", "status"]).0, 0);
    let (code, _, err) = run(&ha_env(&ha), None, &["1", "on"]);
    assert_eq!(code, 1);
    assert!(err.contains("cannot confirm control"), "{err}");
    assert!(ha.calls().iter().all(|c| c.0 == "GET"));
}

#[test]
fn ha_unconfirmed_switch_fails() {
    let ha = FakeHa::start();
    ha.set(|s| s.stuck = true);
    let (code, _, err) = run(&ha_env(&ha), None, &["1", "on", "--wait", "0.01"]);
    assert_eq!(code, 1);
    assert!(err.contains("did not report on"), "{err}");
}

#[test]
fn ha_missing_configuration() {
    let ha = FakeHa::start();
    let env = Env {
        ha: None,
        config: false,
        extra: vec![("HA_BASE".into(), format!("http://127.0.0.1:{}", ha.port))],
    };
    let (code, _, err) = run(&env, None, &["1", "status"]);
    assert_eq!(code, 1);
    assert!(err.contains("HA_BASE and HA_KEY"), "{err}");
    assert_eq!(ha.calls().len(), 0);
}

#[test]
fn target_is_required_and_validated() {
    // Old `lab-power off` and bogus targets must never reach a backend.
    let ha = FakeHa::start();
    for argv in [
        &["off"][..],
        &["on"],
        &["status"],
        &["cycle"],
        &["3", "off"],
        &["off", "2"],
        &["2", "off", "--entity", "switch.server_room_lab_plug"],
        &[],
    ] {
        let (code, _, err) = run(&ha_env(&ha), None, argv);
        assert_eq!(code, 2, "{argv:?}: {err}");
    }
    assert!(run(&ha_env(&ha), None, &["off"]).2.contains("no default target"));
    assert_eq!(ha.calls().len(), 0);
}

#[test]
fn ha_entity_env_is_ignored() {
    let ha = FakeHa::start();
    let env = Env {
        extra: vec![("HA_ENTITY".into(), "switch.server_room_lab_plug".into())],
        ..ha_env(&ha)
    };
    assert_eq!(run(&env, None, &["2", "status"]).0, 0);
    assert_eq!(ha.calls()[0].1, "/api/states/switch.server_room_lab_plug_2");
}

#[test]
fn ha_https_refused_without_a_request() {
    let env = Env {
        ha: None,
        config: false,
        extra: vec![
            ("HA_BASE".into(), "https://ha.example".into()),
            ("HA_KEY".into(), HA_KEY.into()),
        ],
    };
    let (code, _, err) = run(&env, None, &["1", "status"]);
    assert_eq!(code, 1);
    assert!(err.contains("https"), "{err}");
}

#[test]
fn ha_cycle_and_power_meter() {
    let ha = FakeHa::start();
    ha.set(|s| s.state = "on".into());
    let (code, out, err) = run(
        &all_env(Some(&ha)),
        None,
        &["1", "cycle", "--off-time", "0.1", "--json"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("feeds the test switch PSU2"), "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["action"].as_str(), v["state"].as_str()), (Some("cycle"), Some("on")));
    let posts: Vec<String> = ha
        .calls()
        .iter()
        .filter(|c| c.0 == "POST")
        .map(|c| c.1.clone())
        .collect();
    assert_eq!(posts, ["/api/services/switch/turn_off", "/api/services/switch/turn_on"]);

    let (code, out, _) = run(&all_env(Some(&ha)), None, &["m", "status"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("power: 41.5 W (Home Assistant sensor.metered_power)"),
        "{out}"
    );
    let (code, out, _) = run(
        &all_env(Some(&ha)),
        None,
        &["m", "power", "--watch", "0.05", "--count", "2", "--json"],
    );
    assert_eq!(code, 0);
    let samples: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0]["power"]["watts"], 41.5);
    ha.set(|s| s.watts = "unavailable".into());
    let (_, out, _) = run(&all_env(Some(&ha)), None, &["m", "power"]);
    assert!(out.contains("meter sensor.metered_power is unavailable"), "{out}");
    let (code, out, _) = run(&all_env(Some(&ha)), None, &["1", "power"]);
    assert_eq!(code, 0);
    assert!(out.contains("no power meter on this target"), "{out}");
    let (code, _, err) = run(&all_env(Some(&ha)), None, &["1", "power", "--watch", "1"]);
    assert_eq!(code, 1);
    assert!(err.contains("nothing to watch"), "{err}");
}

// --- NetBotz over the fake ssh ----------------------------------------------------

#[test]
fn nb_status_reads_the_recorded_table_and_logs_out() {
    let nb = FakeNb::new("ok", "Off");
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "status"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("nb (NetBotz 10.0.0.9 switched outlet \"Outlet\"): off, normal state off\n"),
        "{out}"
    );
    assert!(out.contains("no power meter on this target"));
    assert_eq!(nb.commands(), ["nboutlet", "bye"]);
    let argv = nb.read("argv");
    for want in [
        "-tt",
        "StrictHostKeyChecking=yes",
        "PreferredAuthentications=password",
        "PubkeyAuthentication=no",
        "HostKeyAlgorithms=+ssh-rsa",
        "-F /dev/null",
        "GlobalKnownHostsFile=/dev/null",
        "-l apc -- 10.0.0.9",
    ] {
        assert!(argv.contains(want), "{want}: {argv}");
    }
    assert!(argv.contains(&format!("UserKnownHostsFile={}", nb.known_hosts.display())));
    let (_, out, _) = run(&all_env(None), Some(&nb), &["nb", "status", "--json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (v["state"].as_str(), v["normal_state"].as_str(), v["backend"].as_str()),
        (Some("off"), Some("off"), Some("netbotz"))
    );
}

#[test]
fn nb_on_off_verified_by_read_back() {
    let nb = FakeNb::new("ok", "Off");
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "on"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("): on, normal state off"), "{out}");
    assert_eq!(nb.commands(), ["nboutlet", "nboutlet -s On", "nboutlet", "bye"]);
    assert_eq!(nb.state(), "On");
    fs::write(nb.dir.join("log"), "").unwrap();
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "off"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("it feeds the test switch PSU1"), "{err}");
    assert_eq!(nb.commands(), ["nboutlet", "nboutlet -s Off", "nboutlet", "bye"]);
    // already off: nothing is set; the normal state is never touched
    fs::write(nb.dir.join("log"), "").unwrap();
    assert_eq!(run(&all_env(None), Some(&nb), &["nb", "off"]).0, 0);
    assert_eq!(nb.commands(), ["nboutlet", "bye"]);
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "cycle", "--off-time", "0.1"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(nb.state(), "On");
    assert!(!nb.read("log").contains("-ns"));
}

#[test]
fn nb_failures_are_named_without_ssh_text() {
    for (scenario, want, code) in [
        ("unreachable", "unreachable over SSH", 1),
        ("hostkey-changed", "HOST KEY CHANGED", 1),
        ("hostkey-unknown", "host key not verified", 1),
        ("session-limit", "session limit", 1),
        ("garbage", "unexpected nboutlet output", 1),
    ] {
        let nb = FakeNb::new(scenario, "Off");
        let (got, _, err) = run(&all_env(None), Some(&nb), &["nb", "status"]);
        assert_eq!(got, code, "{scenario}: {err}");
        assert!(err.contains(want), "{scenario}: {err}");
        assert!(!err.contains("SHA256") && !err.contains("FAKEFINGERPRINT"), "{err}");
        if scenario == "garbage" {
            assert_eq!(nb.commands(), ["nboutlet", "bye"], "logs out after bad output");
        }
    }
    let nb = FakeNb::new("ok", "Off");
    fs::write(nb.dir.join("password"), "something-else").unwrap();
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "status"]);
    assert_eq!(code, 1);
    assert!(err.contains("authentication failed"), "{err}");

    let nb = FakeNb::new("cli-error", "Off");
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "on"]);
    assert_eq!(code, 1);
    assert!(err.contains("E102"), "{err}");
    assert_eq!(nb.commands().last().map(String::as_str), Some("bye"));

    let nb = FakeNb::new("stuck", "Off");
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "on", "--wait", "1"]);
    assert_eq!(code, 1);
    assert!(err.contains("accepted but the outlet still reads off"), "{err}");
    assert_eq!(nb.commands().last().map(String::as_str), Some("bye"));
}

#[test]
fn nb_normal_state_set_and_read_back() {
    let nb = FakeNb::new("ok", "Off");
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "on"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "nb (NetBotz 10.0.0.9 switched outlet \"Outlet\"): off, normal state on\n  power: no power meter on this \
         target (the NetBotz 250 outlet does not meter)\n"
    );
    assert!(err.contains("normal state set to on"), "{err}");
    assert_eq!(nb.commands(), ["nboutlet", "nboutlet -ns On", "nboutlet", "bye"]);
    assert_eq!(
        (nb.normal().as_str(), nb.state().as_str()),
        ("On", "Off"),
        "current state untouched"
    );
    // the fake persists it: status now shows it, with the current state still read separately
    nb.clear_log();
    let (_, out, _) = run(&all_env(None), Some(&nb), &["nb", "status"]);
    assert!(out.contains("): off, normal state on\n"), "{out}");
    fs::write(nb.dir.join("state"), "On\n").unwrap();
    let (_, out, _) = run(&all_env(None), Some(&nb), &["nb", "status"]);
    assert!(out.contains("): on, normal state on\n"), "{out}");
    // and back off, as JSON
    nb.clear_log();
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "off", "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (
            v["action"].as_str(),
            v["state"].as_str(),
            v["normal_state"].as_str(),
            v["changed"].as_bool()
        ),
        (Some("normal-state"), Some("on"), Some("off"), Some(true))
    );
    assert_eq!(nb.commands(), ["nboutlet", "nboutlet -ns Off", "nboutlet", "bye"]);
    assert_eq!((nb.normal().as_str(), nb.state().as_str()), ("Off", "On"));
}

#[test]
fn nb_normal_state_already_set_sends_nothing() {
    let nb = FakeNb::new("ok", "On");
    fs::write(nb.dir.join("normal"), "On\n").unwrap();
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "on"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("nb (NetBotz 10.0.0.9 switched outlet \"Outlet\"): on, normal state on\n"),
        "{out}"
    );
    assert!(err.contains("normal state is already on; nothing set"), "{err}");
    assert_eq!(nb.commands(), ["nboutlet", "bye"]);
    let (code, out, _) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "on", "--json"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["changed"], false);
    assert!(!nb.read("log").contains("-ns"));
}

#[test]
fn nb_normal_state_refused_or_not_followed() {
    let nb = FakeNb::new("ns-stuck", "Off");
    let (code, out, err) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "on", "--wait", "1"]);
    assert_eq!(code, 1);
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("nboutlet -ns On was accepted but the normal state still reads off"),
        "{err}"
    );
    let cmds = nb.commands();
    assert_eq!(cmds[..2], ["nboutlet", "nboutlet -ns On"]);
    assert_eq!(cmds.last().map(String::as_str), Some("bye"), "logs out after the error");
    assert!(cmds[2..cmds.len() - 1].iter().all(|c| c == "nboutlet"), "{cmds:?}");

    let nb = FakeNb::new("cli-error", "Off");
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "normal-state", "on"]);
    assert_eq!(code, 1);
    assert!(err.contains("nboutlet -ns On: CLI error E102"), "{err}");
    assert_eq!(nb.commands(), ["nboutlet", "nboutlet -ns On", "bye"]);
    assert_eq!(nb.normal(), "Off");
}

#[test]
fn normal_state_only_on_netbotz_targets() {
    let ha = FakeHa::start();
    let nb = FakeNb::new("ok", "Off");
    let (code, out, err) = run(&all_env(Some(&ha)), Some(&nb), &["1", "normal-state", "on"]);
    assert_eq!(code, 1);
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("normal-state is for NetBotz outlets; 1 is a Home Assistant plug"),
        "{err}"
    );
    let (code, _, err) = run(&all_env(Some(&ha)), Some(&nb), &["sw", "normal-state", "off"]);
    assert_eq!(code, 1);
    assert!(err.contains("not a device"), "{err}");
    assert!(err.contains("name its NetBotz feed instead (nb)"), "{err}");
    // nothing contacted: no HTTP request, no ssh
    assert_eq!(ha.calls().len(), 0);
    assert!(nb.read("argv").is_empty(), "ssh must not run");
    // a bad value is a usage error before anything
    for argv in [&["nb", "normal-state"][..], &["nb", "normal-state", "maybe"]] {
        let (code, _, err) = run(&all_env(Some(&ha)), Some(&nb), argv);
        assert_eq!(code, 2, "{argv:?}: {err}");
    }
    assert!(nb.read("argv").is_empty(), "ssh must not run");
}

#[test]
fn nb_login_timeout_kills_ssh() {
    let nb = FakeNb::new("hang", "Off");
    let start = std::time::Instant::now();
    let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "status", "--timeout", "1"]);
    assert_eq!(code, 1);
    assert!(err.contains("no CLI prompt"), "{err}");
    assert!(start.elapsed().as_secs() < 15);
}

#[test]
fn nb_private_files_must_be_private() {
    for (which, mode) in [("creds", 0o640), ("kh", 0o644), ("creds", 0o604)] {
        let nb = FakeNb::new("ok", "Off");
        let f = if which == "creds" { &nb.creds } else { &nb.known_hosts };
        fs::set_permissions(f, fs::Permissions::from_mode(mode)).unwrap();
        let (code, _, err) = run(&all_env(None), Some(&nb), &["nb", "status"]);
        assert_eq!(code, 1);
        assert!(err.contains("refusing"), "{err}");
        assert!(nb.read("argv").is_empty(), "ssh must not run");
    }
    // the variable naming the file must be set (extra is applied after FakeNb's)
    let nb = FakeNb::new("ok", "Off");
    let env = Env {
        extra: vec![("FAKE_NB_CREDS".into(), String::new())],
        ..all_env(None)
    };
    let (code, _, err) = run(&env, Some(&nb), &["nb", "status"]);
    assert_eq!(code, 1);
    assert!(err.contains("set FAKE_NB_CREDS"), "{err}");
    assert!(nb.read("argv").is_empty(), "ssh must not run");
}

#[test]
fn askpass_answers_only_password_prompts() {
    let nb = FakeNb::new("ok", "Off");
    let ask = |prompt: &str| {
        Command::new(env!("CARGO_BIN_EXE_lab-power"))
            .env_clear()
            .env("LAB_POWER_ASKPASS_CREDENTIALS", &nb.creds)
            .arg(prompt)
            .output()
            .unwrap()
    };
    let ok = ask("apc@10.0.0.9's password: ");
    assert!(ok.status.success());
    assert_eq!(String::from_utf8_lossy(&ok.stdout), format!("{NB_PASSWORD}\n"));
    let no = ask("Are you sure you want to continue connecting (yes/no/[fingerprint])?");
    assert!(!no.status.success() && no.stdout.is_empty());
    fs::set_permissions(&nb.creds, fs::Permissions::from_mode(0o644)).unwrap();
    let open = ask("password:");
    assert!(!open.status.success() && open.stdout.is_empty());
}

// --- devices: all feeds -------------------------------------------------------------

#[test]
fn device_status_and_cold_cycle() {
    let ha = FakeHa::start();
    ha.set(|s| s.state = "on".into());
    let nb = FakeNb::new("ok", "On");
    let (code, out, err) = run(&all_env(Some(&ha)), Some(&nb), &["sw", "status"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("sw (test switch): all feeds on\n"), "{out}");
    assert!(
        out.contains("feed 1: plug 1 (switch.server_room_lab_plug): on"),
        "{out}"
    );
    assert!(out.contains("feed nb: nb (NetBotz 10.0.0.9"), "{out}");

    let (code, out, err) = run(
        &all_env(Some(&ha)),
        Some(&nb),
        &["sw", "cycle", "--off-time", "0.1", "--json"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("all feeds of test switch"), "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["state"], "all feeds on");
    assert_eq!(v["feeds"].as_array().unwrap().len(), 2);
    let posts: Vec<String> = ha
        .calls()
        .iter()
        .filter(|c| c.0 == "POST")
        .map(|c| c.1.clone())
        .collect();
    assert_eq!(posts, ["/api/services/switch/turn_off", "/api/services/switch/turn_on"]);
    let sets: Vec<String> = nb.commands().into_iter().filter(|c| c.contains("-s")).collect();
    assert_eq!(sets, ["nboutlet -s Off", "nboutlet -s On"]);
    assert_eq!(nb.state(), "On");
}

#[test]
fn device_cycle_aborts_and_restores_when_a_feed_does_not_follow() {
    let ha = FakeHa::start();
    ha.set(|s| s.state = "on".into());
    let nb = FakeNb::new("stuck", "On");
    let (code, out, err) = run(
        &all_env(Some(&ha)),
        Some(&nb),
        &["sw", "cycle", "--off-time", "0.1", "--wait", "1"],
    );
    assert_eq!(code, 1);
    assert!(err.contains("feed nb did not turn off"), "{err}");
    assert!(err.contains("cycle aborted"), "{err}");
    assert!(out.contains("all feeds on"), "restored: {out}");
    assert_eq!(ha.st.lock().unwrap().state, "on");
}

#[test]
fn device_refuses_before_switching_when_a_feed_is_unreadable() {
    let ha = FakeHa::start();
    ha.set(|s| s.state = "on".into());
    let nb = FakeNb::new("unreachable", "On");
    let (code, _, err) = run(&all_env(Some(&ha)), Some(&nb), &["sw", "off"]);
    assert_eq!(code, 1);
    assert!(err.contains("feed nb cannot be read, nothing switched"), "{err}");
    assert!(ha.calls().iter().all(|c| c.0 == "GET"));
}

#[test]
fn list_shows_every_target() {
    let (code, out, _) = run(
        &Env {
            ha: None,
            config: false,
            extra: vec![],
        },
        None,
        &["--list"],
    );
    assert_eq!(code, 0);
    for want in [
        "switch.server_room_lab_plug",
        "switch.server_room_lab_plug_2",
        "cs8210-psu1",
        "10.60.30.45",
        "cs8210",
    ] {
        assert!(out.contains(want), "{want}: {out}");
    }
}

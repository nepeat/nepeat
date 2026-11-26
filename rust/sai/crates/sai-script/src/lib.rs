//! The oracle's JSON SAI script runner in Rust: `ai_re/nix/packages/sai-teralynx/oracle/sai_script.cpp`
//! with the same command line, script format (its README "Script format"), output (`STEP` / `EXEC` /
//! `DIAG` / `FILL` / `event` lines), trace markers (`IFCS_TRACE_FILE`) and exit codes, as a SAI C API
//! client of any `libsai.so` (crate `sai-client`), so the `hw:*` scripts run on either Teralynx libsai.
//!
//! Deliberate differences (the default; `--cpp-names` turns all of them off, so that mode must equal
//! the C++ runner exactly). Each says `sai-script: FIX <step>: ...` on stderr where it applies:
//!
//! - **null object of a failed create**: a step on `$x` where `x`'s `create` failed (id 0) gets the
//!   type the create asked for; the C++ asks `sai_object_type_query(0)` = NULL, finds no attribute
//!   table and stops the script (`unknown value ACTION_PACKET_ACTION` in `acl`, `MAP_TO_VALUE_LIST` in
//!   `qos`, `IN_PACKETS` in `phasec-recreate`). The call then goes to the library (which answers for
//!   the null id) instead of the C++'s `NOT_SUPPORTED` dispatch.
//! - **enumerator names by the attribute's enum**: a value name of an enum attribute (`s32`,
//!   `s32list` elements, ACL field / action `s32`) is looked up in that attribute's enum (SAI
//!   metadata) before the C++'s one flat table, where the first match wins across enums (`GENETLINK`
//!   of a host interface's `TYPE` was `SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK` = 5, not
//!   `SAI_HOSTIF_TYPE_GENETLINK` = 2).
//! - **names the C++ tables lack**: an attribute name of the object type's header enum, a counter name
//!   of its stat enum, where the C++ stops with "unknown value".
//!
//! Other differences: the library is `dlopen`ed (`--libsai`, default `sai_client::default_libsai`),
//! not linked (a missing one: exit 1 with the loader's message); exec steps can run other tools
//! (`--tool NAME=CMD`: `$ORACLE/bin/NAME` becomes CMD, the printed `EXEC` line keeps the script's
//! text); the JSON parse error text of a malformed script is not nlohmann's (the other accessor errors
//! are); only SAI 1.19 headers (the C++ also builds against isai's 1.16).
#![forbid(unsafe_code)]

pub mod json;
pub mod names;
pub mod script;
pub mod text;

use std::collections::BTreeMap;
use std::io::Write as _;

pub use script::{Options, Runner, say};

/// The profile file as `sai_script.cpp` reads it: `KEY=VALUE` lines (the first `=`), empty and `#`
/// lines skipped, a repeated key: the last; a missing file: an empty profile.
pub fn read_profile(path: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let Ok(text) = std::fs::read(path) else {
        return m;
    };
    for line in String::from_utf8_lossy(&text).split('\n') {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            m.insert(k.to_owned(), v.to_owned());
        }
    }
    m
}

/// The notification lines (`on_port_state`, `on_shutdown`, `on_fdb`).
pub fn event_line(e: &sai_client::Event) -> String {
    match e {
        sai_client::Event::PortState { port, state } => format!("event port 0x{port:x} oper {state}"),
        sai_client::Event::Shutdown => "event shutdown request".to_owned(),
        sai_client::Event::Fdb {
            event,
            mac: m,
            bv,
            bridge_port,
        } => format!(
            "event fdb {event} mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} bv 0x{bv:x} bp 0x{bridge_port:x}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
    }
}

fn usage(argv0: &str) -> i32 {
    let _ = writeln!(
        std::io::stderr(),
        "usage: {argv0} [--libsai LIBSAI] [--cpp-names] [--tool NAME=CMD ...] PROFILE SCRIPT.json"
    );
    2
}

/// The program: `main(argc, argv)`'s exit code.
pub fn main(args: &[String]) -> i32 {
    let argv0 = args.first().map_or("sai-script", String::as_str);
    let mut opt = Options::default();
    let mut libsai = None;
    let mut pos = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cpp-names" => opt.cpp_names = true,
            "--libsai" => match it.next() {
                Some(v) => libsai = Some(v.clone()),
                None => return usage(argv0),
            },
            "--tool" => match it.next().and_then(|v| v.split_once('=')) {
                Some((k, v)) if !k.is_empty() => opt.tools.push((k.to_owned(), v.to_owned())),
                _ => return usage(argv0),
            },
            _ => pos.push(a.clone()),
        }
    }
    if let Ok(env) = std::env::var("SAI_SCRIPT_TOOLS") {
        for t in env.split(';').filter(|t| !t.is_empty()) {
            if let Some((k, v)) = t.split_once('=') {
                opt.tools.push((k.to_owned(), v.to_owned()));
            }
        }
    }
    let (Some(profile_path), Some(script_path)) = (pos.first(), pos.get(1)) else {
        return usage(argv0);
    };
    let profile = read_profile(profile_path);
    let text = std::fs::read(script_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let script = match json::parse(&text) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{script_path}: {e}");
            return 2;
        }
    };
    let lib = libsai.unwrap_or_else(sai_client::default_libsai);
    let mut sai = match sai_client::Sai::load(&lib) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "sai-script: {e}");
            return 1;
        }
    };
    let mut mark = std::env::var_os("IFCS_TRACE_FILE")
        .and_then(|f| std::fs::OpenOptions::new().append(true).create(true).open(f).ok());
    let _ = sai_client::set_profile(profile.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    sai_client::set_event_sink(|e| say(&event_line(e)));
    let name = match script.member("name") {
        Ok(Some(n)) => n.str().map(str::to_owned).unwrap_or_default(),
        _ => script_path.clone(),
    };
    if let Some(f) = &mut mark {
        let _ = f.write_all(format!("M script {name}\n").as_bytes());
    }
    let r = sai.initialize();
    if r != sai_client::SUCCESS {
        let _ = writeln!(std::io::stderr(), "sai_api_initialize {r}");
        return 1;
    }
    for api in sai_client::Api::ALL {
        let _ = sai.query(api);
    }
    let mut run = Runner::new(&sai, &profile, opt, mark);
    let mut rc = 0;
    let steps = script
        .at("steps")
        .map_err(script::Error::Other)
        .and_then(|s| run.run(s, 0));
    if let Err(e) = steps {
        let _ = writeln!(std::io::stderr(), "script error: {}", e.text());
        rc = 2;
    }
    let failed = run.failed;
    run.mark(&format!("done failed={failed}"));
    drop(run);
    let _ = sai.uninitialize();
    say(&format!("sai-script: {failed} failed steps"));
    if rc != 0 { rc } else { i32::from(failed != 0) }
}

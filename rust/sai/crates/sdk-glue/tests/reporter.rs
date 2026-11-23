//! The callback reporter (`Switch::set_callback_reporter`): every call a trampoline refuses and
//! every closure that panics is counted and reported, on the thread of the call; nothing else is.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use sdk_glue::rust_fake::Chip;
use sdk_glue::{Callback, CallbackIssue, CallbackStats, Switch};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn scenario(text: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
        .join(format!("sdk-glue-reporter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.cfg", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn refusals_and_panics_are_reported_and_counted() {
    let chip = Chip::new();
    let (mut sw, _) = Switch::init_on(&chip, 0, &scenario("ports 2 4 100000\n"), None).unwrap();
    let seen: Arc<Mutex<Vec<(CallbackIssue, std::thread::ThreadId)>>> = Arc::default();
    let sink = Arc::clone(&seen);
    sw.set_callback_reporter(move |issue| sink.lock().unwrap().push((issue, std::thread::current().id())));
    sw.set_callbacks(
        |port, _| assert!(port != 2, "link closure panics for port 2"),
        |_| panic!("l2"),
    )
    .unwrap();
    let key = chip.callback_key();

    // fine: nothing reported
    assert!(chip.link_event(1, true));
    sw.set_port_gate(1, || panic!("gate")).unwrap();
    // a panicking link closure, from another thread
    std::thread::spawn(move || Chip::stray_link(key, 2, 1)).join().unwrap();
    // refused: bad state, negative port
    Chip::stray_link(key, 1, 3);
    Chip::stray_link(key, -1, 0);
    // L2: a refused one (VLAN 0) and a panicking closure
    assert!(chip.l2_event(Some([2, 0, 0, 0, 0, 1]), 0, 1, 1));
    assert!(chip.l2_event(Some([2, 0, 0, 0, 0, 1]), 10, 1, 1));
    // the gate panics: answered down
    assert_eq!(chip.gate(1), Some(0));

    let got: Vec<CallbackIssue> = seen.lock().unwrap().iter().map(|(i, _)| *i).collect();
    assert_eq!(
        got,
        [
            CallbackIssue::Panicked {
                callback: Callback::Link,
                port: 2
            },
            CallbackIssue::Rejected {
                callback: Callback::Link,
                port: 1
            },
            CallbackIssue::Rejected {
                callback: Callback::Link,
                port: -1
            },
            CallbackIssue::Rejected {
                callback: Callback::L2,
                port: 1
            },
            CallbackIssue::Panicked {
                callback: Callback::L2,
                port: 1
            },
            CallbackIssue::Panicked {
                callback: Callback::Gate,
                port: 1
            },
        ]
    );
    // the first was reported on the thread that made the call
    assert_ne!(seen.lock().unwrap()[0].1, std::thread::current().id());
    assert_eq!(
        sw.callback_stats(),
        CallbackStats {
            link_panics: 1,
            l2_panics: 1,
            gate_panics: 1,
            rejected: 3,
        }
    );
    assert_eq!(got[5].to_string(), "link gate of port 1 panicked: answered down");
    assert_eq!(got[0].to_string(), "link callback of port 2 panicked: event lost");
    assert_eq!(
        got[3].to_string(),
        "L2 callback of port 1: arguments out of range, refused"
    );
}

#[test]
fn a_panicking_reporter_is_ignored_and_stray_calls_report_nothing() {
    let chip = Chip::new();
    let (mut sw, _) = Switch::init_on(&chip, 0, &scenario("ports 1 4 100000\n"), None).unwrap();
    sw.set_callback_reporter(|_| panic!("reporter"));
    sw.set_link_callback(|_, _| {}).unwrap();
    let key = chip.callback_key();
    Chip::stray_link(key, -1, 1); // refused; the reporter panics; nothing unwinds
    assert_eq!(sw.callback_stats().rejected, 1);
    // a key of no registration: no entry, no reporter, nothing counted
    Chip::stray_link(key + 1000, -1, 1);
    assert_eq!(sw.callback_stats().rejected, 1);
}

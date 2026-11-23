//! The full bring-up, ports, VLANs and the link callback through the plain bring-up
//! (`Switch::init`), i.e. on the process's Rust fake with its hooks (`sdk_glue::fake`), on any host
//! (and on big-endian ppc64 from the bundle). Once the C fake's tests; the per-chip versions are in
//! `rust_fake.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;

use sdk_glue::{Code, Fec, InitError, Mmd, PortInfo, Sdk, Switch, fake};

/// The glue's state is one per process: one test at a time.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn scratch() -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("sdk-glue-switch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scenario(name: &str, text: &str) -> PathBuf {
    let path = scratch().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn up(name: &str, text: &str) -> Switch {
    Switch::init(0, &scenario(name, text), None).unwrap().0
}

fn refused(name: &str, text: &str) -> (Code, String) {
    match Switch::init(0, &scenario(name, text), None) {
        Err(InitError::Sdk { error, message }) => (error.code(), message),
        other => panic!("expected the glue to refuse, got {other:?}"),
    }
}

#[test]
fn bring_up_enumerates_ports_and_shuts_down_on_drop() {
    let _g = serial();
    let trace = scratch().join("trace.txt");
    let _ = std::fs::remove_file(&trace);
    let cfg = scenario(
        "up.cfg",
        &format!(
            "name TEST56960_B1\nports 2 4 100000\nport 66 129 1 10000 xe0\ncpu 0\ntrace {}\n",
            trace.display()
        ),
    );
    let rc = scenario("led.soc", "# rc\n");
    let (mut sw, line) = Switch::init(0, &cfg, Some(&rc)).unwrap();
    assert_eq!(
        line,
        "ok: stale KNET netifs 0 filters 0 destroyed; IPv4/IPv6 L3 on 3 ports"
    );
    assert!(fake::bcm_layer_up());
    assert_eq!(sw.unit(), 0);
    assert_eq!(sw.hw_info().unwrap(), "TEST56960_B1");
    assert_eq!(sw.cpu_port(), 0);
    assert_eq!(
        sw.ports().unwrap(),
        vec![
            PortInfo {
                port: 1,
                first_lane: 1,
                lanes: 4,
                speed: 100_000,
                name: "ce0".into()
            },
            PortInfo {
                port: 2,
                first_lane: 5,
                lanes: 4,
                speed: 100_000,
                name: "ce1".into()
            },
            PortInfo {
                port: 66,
                first_lane: 129,
                lanes: 1,
                speed: 10_000,
                name: "xe0".into()
            },
        ]
    );
    // the Sdk underneath: MDIO works after the full bring-up too
    assert_eq!(sw.mdio_c45_write(0x24, Mmd::new(1).unwrap(), 0x8b00, 0x2391), Ok(()));
    assert_eq!(sw.mdio_c45_read(0x24, Mmd::new(1).unwrap(), 0x8b00), Ok(0x2391));
    // one handle per process, whichever bring-up
    assert_eq!(Switch::init(0, &cfg, None).unwrap_err(), InitError::AlreadyOpen);
    assert_eq!(Sdk::init_mdio(0, &cfg).unwrap_err(), InitError::AlreadyOpen);
    drop(sw);
    assert!(!fake::bcm_layer_up());
    assert_eq!(fake::shutdown_calls(), 1);
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        format!(
            "init-all unit 0 rc {}\nW 24 1 8b00 2391\nR 24 1 8b00 = 2391\nunregister unit 0\nshutdown unit 0\n",
            rc.display()
        )
    );
    // and again after the drop
    let sw = up("again.cfg", "ports 1 4 100000\n");
    assert_eq!(sw.ports().unwrap().len(), 1);
}

#[test]
fn bring_up_failures_leave_no_handle_and_nothing_registered() {
    let _g = serial();
    let missing = scratch().join("no-such.cfg");
    let _ = std::fs::remove_file(&missing);
    match Switch::init(0, &missing, None) {
        Err(InitError::Sdk { error, message }) => {
            assert_eq!(error.code(), Code::Config);
            assert_eq!(
                message,
                format!("SDK config file '{}' missing or unreadable (2)", missing.display())
            );
        }
        other => panic!("{other:?}"),
    }
    let ok = scenario("ok.cfg", "ports 1 4 100000\n");
    match Switch::init(0, &ok, Some(&missing)) {
        Err(InitError::Sdk { error, message }) => {
            assert_eq!(error.code(), Code::Config);
            assert_eq!(
                message,
                format!("rc file '{}' missing or unreadable (2)", missing.display())
            );
        }
        other => panic!("{other:?}"),
    }
    let (code, message) = refused("busy.cfg", "init busy\n");
    assert_eq!(code, Code::Busy);
    assert!(
        message.starts_with("another SDK process owns the switch (lock /run/openbcm-unit0.lock held"),
        "{message}"
    );
    assert_eq!(
        refused("all.cfg", "init all\n"),
        (Code::Fail, "init all failed: -11".to_owned())
    );
    assert_eq!(refused("rc.cfg", "init rc\n").0, Code::Fail);
    assert_eq!(refused("unit.cfg", "init unit\n").0, Code::Unit);
    assert_eq!(refused("bogus.cfg", "ports 0 4 1\n").0, Code::Config);
    assert_eq!(refused("bogus2.cfg", "fail enable 0 -11\n").0, Code::Config);
    assert_eq!(
        Switch::init(1, &ok, None).map(|_| ()).unwrap_err().to_string(),
        "no attached switch unit (soc_ndev 1) (Invalid unit)"
    );
    assert_eq!(
        Switch::init(0, Path::new("a\0b"), None).unwrap_err(),
        InitError::BadPath
    );
    assert_eq!(
        Switch::init(0, &ok, Some(Path::new("r\0c"))).unwrap_err(),
        InitError::BadPath
    );
    assert!(!fake::bcm_layer_up());
    assert!(!fake::link_callback_registered());
    assert_eq!(fake::register_calls(), 0);
    // every failure released the claim
    let sw = up("after.cfg", "ports 1 4 100000\n");
    drop(sw);
}

#[test]
fn port_enable_link_speed_and_board_settings() {
    let _g = serial();
    let mut sw = up(
        "ports.cfg",
        "ports 2 4 100000\nport 9 33 1 25000 xe0\nlink admin\nfail enable 4 -9\nfail speed-get 2 -11\nfail speeds 2 -16\n",
    );
    // as "init all" leaves them: enabled, no link
    assert_eq!(fake::port_enabled(1), Some(true));
    assert_eq!(fake::port_enabled(7), None);
    assert_eq!(sw.port_link_get(1), Ok(false));
    sw.port_enable_set(1, false).unwrap();
    assert_eq!(fake::port_enabled(1), Some(false));
    sw.port_enable_set(1, true).unwrap();
    assert_eq!(sw.port_link_get(1), Ok(true));
    sw.port_enable_set(1, false).unwrap();
    assert_eq!(sw.port_link_get(1), Ok(false));
    assert_eq!(sw.port_enable_set(1, true).unwrap_err().code(), Code::Timeout); // the 4th call
    assert_eq!(fake::port_enabled(1), Some(false));
    assert_eq!(sw.port_enable_set(7, true).unwrap_err().code(), Code::Port);
    assert_eq!(sw.port_link_get(7).unwrap_err().code(), Code::Port);

    assert_eq!(sw.port_speed_get(1), Ok(100_000));
    assert_eq!(sw.port_speed_get(1).unwrap_err().code(), Code::Fail);
    assert_eq!(sw.port_speeds(1).unwrap(), vec![40_000, 100_000]);
    assert_eq!(sw.port_speeds(1).unwrap_err().code(), Code::Unavail);
    assert_eq!(sw.port_speeds(9).unwrap(), vec![1000, 10_000, 25_000]);
    sw.port_speed_set(1, 40_000).unwrap();
    assert_eq!(sw.port_speed_get(1), Ok(40_000));
    assert_eq!(sw.port_speed_set(1, 25_000).unwrap_err().code(), Code::Param);
    assert_eq!(sw.port_speed_set(1, u32::MAX).unwrap_err().code(), Code::Param);
    assert_eq!(sw.port_speed_get(1), Ok(40_000));
    assert_eq!(sw.port_speed_set(7, 40_000).unwrap_err().code(), Code::Port);

    assert_eq!(fake::port_settings(2), Some((-1, -1, -1, false)));
    sw.port_autoneg_set(2, false).unwrap();
    sw.port_interface_set(2, c"caui").unwrap();
    sw.port_fec_set(2, Fec::None).unwrap();
    sw.port_ifg_set(2, 128).unwrap();
    assert_eq!(fake::port_settings(2), Some((0, 0, 128, true)));
    sw.port_autoneg_set(2, true).unwrap();
    sw.port_fec_set(2, Fec::Rs).unwrap();
    assert_eq!(fake::port_settings(2), Some((1, 1, 128, true)));
    sw.port_fec_set(2, Fec::Fc).unwrap();
    assert_eq!(fake::port_settings(2).unwrap().1, 2);
    assert_eq!(sw.port_interface_set(2, c"bogus").unwrap_err().code(), Code::Param);
    assert_eq!(sw.port_ifg_set(2, u32::MAX).unwrap_err().code(), Code::Param);
    assert_eq!(sw.port_autoneg_set(7, true).unwrap_err().code(), Code::Port);
    for name in sdk_glue::port::INTERFACE_NAMES {
        let c = std::ffi::CString::new(name).unwrap();
        let c: &CStr = &c;
        sw.port_interface_set(1, c).unwrap();
    }
}

#[test]
fn vlans_and_members() {
    let _g = serial();
    let mut sw = up(
        "vlan.cfg",
        "ports 3 4 100000\nfail vlan-create 3 -6\nfail vlan-members 3 -11\n",
    );
    // the chip's default VLAN: every front-panel port, untagged
    assert_eq!(sw.vlan_members(1).unwrap(), vec![(1, true), (2, true), (3, true)]);
    assert_eq!(sw.vlan_members(100).unwrap_err().code(), Code::NotFound);
    sw.vlan_create(100).unwrap();
    assert_eq!(sw.vlan_create(100).unwrap_err().code(), Code::Exists);
    assert_eq!(sw.vlan_create(101).unwrap_err().code(), Code::Full); // the 3rd call
    assert_eq!(sw.vlan_members(100).unwrap_err().code(), Code::Fail); // the 3rd call
    assert_eq!(sw.vlan_members(100).unwrap(), vec![]);
    sw.vlan_port_add(100, 2, false).unwrap();
    sw.vlan_port_add(100, 3, true).unwrap();
    assert_eq!(sw.vlan_members(100).unwrap(), vec![(2, false), (3, true)]);
    sw.vlan_port_add(100, 2, true).unwrap(); // a member's tagging changes
    assert_eq!(sw.vlan_members(100).unwrap(), vec![(2, true), (3, true)]);
    assert_eq!(sw.vlan_port_add(100, 9, true).unwrap_err().code(), Code::Port);
    assert_eq!(sw.vlan_port_add(200, 2, true).unwrap_err().code(), Code::NotFound);
    sw.vlan_port_remove(100, 2).unwrap();
    assert_eq!(sw.vlan_members(100).unwrap(), vec![(3, true)]);
    assert_eq!(sw.vlan_port_remove(200, 2).unwrap_err().code(), Code::NotFound);
    assert_eq!(sw.vlan_destroy(200).unwrap_err().code(), Code::NotFound);
    assert_eq!(sw.vlan_destroy(1).unwrap_err().code(), Code::BadId);
    assert_eq!(sw.vlan_create(0).unwrap_err().code(), Code::Param);
    assert_eq!(sw.vlan_create(4095).unwrap_err().code(), Code::Param);
    sw.vlan_destroy(100).unwrap();
    assert_eq!(sw.vlan_members(100).unwrap_err().code(), Code::NotFound);
    assert_eq!(sw.vlan_members(1).unwrap().len(), 3);
}

#[test]
fn link_events_from_another_thread_reach_the_closure() {
    let _g = serial();
    let mut sw = up("link.cfg", "ports 2 4 100000\n");
    assert!(!fake::link_event(1, true)); // nobody registered
    assert_eq!(sw.port_link_get(1), Ok(true));
    let (tx, rx) = mpsc::channel();
    sw.set_link_callback(move |port, up| {
        let _ = tx.send((port, up, std::thread::current().id()));
    })
    .unwrap();
    assert!(fake::link_callback_registered());
    let cable = std::thread::spawn(|| {
        assert!(fake::link_event(1, false));
        assert!(!fake::link_event(1, false)); // no change, no call
        assert!(fake::link_event(2, true));
        assert!(!fake::link_event(9, true)); // no such port
        std::thread::current().id()
    });
    let cable_thread = cable.join().unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        (1, false, cable_thread)
    );
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        (2, true, cable_thread)
    );
    assert!(rx.try_recv().is_err());
    // a port call that changes the link calls back on the calling thread
    sw.port_enable_set(2, false).unwrap();
    assert_eq!(rx.try_recv().unwrap(), (2, false, std::thread::current().id()));

    // replacing the handler: the old one is dropped (its sender with it), the new one is called
    let (tx2, rx2) = mpsc::channel();
    sw.set_link_callback(move |port, up| {
        let _ = tx2.send((port, up));
    })
    .unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
    assert!(fake::link_event(1, true));
    assert_eq!(rx2.try_recv().unwrap(), (1, true));

    // unregistered: the SDK is told, the closure is gone, later events go nowhere
    sw.clear_link_callback();
    assert!(!fake::link_callback_registered());
    assert_eq!(rx2.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert!(!fake::link_event(1, false));
    sw.clear_link_callback(); // no effect, no second call into the glue
    assert_eq!(fake::unregister_calls(), 2);
}

#[test]
fn dropping_the_switch_unregisters_first_and_drops_the_closure() {
    let _g = serial();
    let trace = scratch().join("drop-trace.txt");
    let _ = std::fs::remove_file(&trace);
    let mut sw = up("drop.cfg", &format!("ports 1 4 100000\ntrace {}\n", trace.display()));
    let (tx, rx) = mpsc::channel();
    sw.set_link_callback(move |port, up| {
        let _ = tx.send((port, up));
    })
    .unwrap();
    drop(sw);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
    assert!(!fake::link_callback_registered());
    assert!(!fake::link_event(1, true));
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        // ob_sdk_shutdown alone, which unregisters inside the glue (as the C++ backend's shutdown)
        "init-all unit 0 rc -\nregister unit 0\nunregister unit 0\nshutdown unit 0\n"
    );
}

#[test]
fn a_refused_registration_leaves_nothing_registered() {
    let _g = serial();
    let mut sw = up("reg.cfg", "ports 1 4 100000\nfail register 1 -14\n");
    let (tx, rx) = mpsc::channel();
    let error = sw
        .set_link_callback(move |port, up| {
            let _ = tx.send((port, up));
        })
        .unwrap_err();
    assert_eq!(error.code(), Code::Resource);
    // the glue had stored the pointer before the SDK refused: it was taken back
    assert!(!fake::link_callback_registered());
    assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    assert!(!fake::link_event(1, true));
    // the second attempt works
    let (tx, rx) = mpsc::channel();
    sw.set_link_callback(move |port, up| {
        let _ = tx.send((port, up));
    })
    .unwrap();
    assert!(fake::link_event(1, false));
    assert_eq!(rx.try_recv().unwrap(), (1, false));
}

#[test]
fn a_panicking_closure_does_not_cross_into_c_and_the_next_event_still_arrives() {
    let _g = serial();
    let mut sw = up("panic.cfg", "ports 2 4 100000\n");
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    sw.set_link_callback(move |port, _up| {
        seen.fetch_add(1, Ordering::SeqCst);
        assert!(port != 1, "test panic inside the link callback");
    })
    .unwrap();
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let fired = std::thread::spawn(|| fake::link_event(1, true)).join();
    std::panic::set_hook(quiet);
    // the C frame returned normally: the thread did not die
    assert!(fired.unwrap());
    assert!(fake::link_event(2, true));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    sw.clear_link_callback();
}

#[test]
fn events_race_with_unregistering_without_harm() {
    let _g = serial();
    for round in 0..20 {
        let mut sw = up("race.cfg", "ports 4 4 100000\n");
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        sw.set_link_callback(move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cables: Vec<_> = (1..=4)
            .map(|port| {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut up = true;
                    while !stop.load(Ordering::SeqCst) {
                        fake::link_event(port, up);
                        up = !up;
                    }
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(2));
        if round % 2 == 0 {
            sw.clear_link_callback();
        } else {
            drop(sw);
        }
        // once unregistering returned no call is counted any more
        let after = count.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(count.load(Ordering::SeqCst), after);
        stop.store(true, Ordering::SeqCst);
        for cable in cables {
            cable.join().unwrap();
        }
    }
}

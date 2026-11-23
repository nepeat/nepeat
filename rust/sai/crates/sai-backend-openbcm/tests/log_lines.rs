//! The hardware backend's log lines (`sai_core::log`: syslog, per-API levels; captured here) with
//! the API, level and text of `openbcm_backend.cpp` at commit 50a6aa7 (`OB_NOTICE` / `OB_ERR` /
//! `OB_WARN`, `RV()`: `SAI_API_UNSPECIFIED`, error, info for `OB_E_UNAVAIL`), on Rust fake chips;
//! and what `sdk-glue`'s callback trampolines report. Every test captures (captures take turns).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod common;

use common::{own_profile, unique};
use sai_backend_openbcm::OpenbcmBackend;
use sai_core::backend::types::{FpQual, MacAddr};
use sai_core::backend::{Backend, EventSink, Field, Hostif, HwPort, L3, Profile};
use sai_core::error::Error;
use sai_core::log::{self, Line};
use sai_sys as sys;
use sdk_glue::error::sdk_text;
use sdk_glue::rust_fake::Chip;

const NOTICE: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_NOTICE;
const INFO: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_INFO;
const WARN: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_WARN;
const ERROR: sys::sai_log_level_t = sys::SAI_LOG_LEVEL_ERROR;

fn all_at(level: sys::sai_log_level_t) {
    for api in 0..=sys::SAI_API_MAX {
        assert!(log::set_level(api, level));
    }
}

#[track_caller]
fn has(lines: &[Line], api: sys::sai_api_t, level: sys::sai_log_level_t, want: &str) {
    assert!(
        lines
            .iter()
            .any(|l| l.api == api && l.level == level && l.message() == want),
        "no line (api {api}, level {level}) {want:?} in:\n{}",
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    );
}

/// Runs `f` with every API at INFO; its lines.
fn lines_of(f: impl FnOnce()) -> Vec<Line> {
    let ((), lines) = log::capture(|| {
        all_at(INFO);
        f();
        all_at(WARN);
    });
    lines
}

#[test]
fn init_logs_as_the_cpp_backend() {
    let lines = lines_of(|| {
        let chip = Chip::new();
        let mut b = OpenbcmBackend::on_chip(chip);
        let profile = own_profile("ports 2 4 100000\nknet 3 1\n", &[]);
        let (sink, _rx) = EventSink::channel();
        b.init(&profile, sink).unwrap();
        // a second init on a chip that is up (this backend's own refusal)
        let (sink, _rx2) = EventSink::channel();
        assert_eq!(b.init(&profile, sink), Err(Error::Failure));
        b.shutdown();
    });
    let config = lines
        .iter()
        .find_map(|l| l.message().strip_prefix("SDK init: unit 0, config "))
        .unwrap()
        .to_owned();
    assert!(config.ends_with(", rc (none)"), "{config}");
    let started = lines.iter().find(|l| l.message().starts_with("SDK init: ok")).unwrap();
    assert_eq!((started.api, started.level), (sys::SAI_API_SWITCH, NOTICE));
    assert!(
        lines.iter().any(|l| l.api == sys::SAI_API_SWITCH
            && l.level == NOTICE
            && l.message().starts_with("SDK tables: host ")
            && l.message().contains("; VRF ids up to ")),
        "{lines:?}"
    );
    has(&lines, sys::SAI_API_SWITCH, ERROR, "init: the chip is already up");
}

#[test]
fn refused_profiles_and_a_failed_bring_up() {
    let lines = lines_of(|| {
        let (sink, _rx) = EventSink::channel();
        let mut b = OpenbcmBackend::on_chip(Chip::new());
        assert_eq!(b.init(&Profile::default(), sink), Err(Error::Failure));
        let missing = unique("no-such.bcm");
        let (sink, _rx) = EventSink::channel();
        let profile = Profile::from_pairs([("SAI_INIT_CONFIG_FILE", missing.display().to_string())]);
        assert_eq!(b.init(&profile, sink), Err(Error::Failure));
        let (sink, _rx) = EventSink::channel();
        assert_eq!(
            b.init(&own_profile("ports 1 4 100000\n", &[("SAI_OPENBCM_UNIT", "x")]), sink),
            Err(Error::InvalidParameter)
        );
        let (sink, _rx) = EventSink::channel();
        assert_eq!(b.init(&own_profile("init busy\n", &[]), sink), Err(Error::ObjectInUse));
    });
    has(
        &lines,
        sys::SAI_API_SWITCH,
        ERROR,
        "SAI_INIT_CONFIG_FILE not set in the profile; refusing to start the SDK with compiled-in defaults",
    );
    has(
        &lines,
        sys::SAI_API_SWITCH,
        ERROR,
        &format!(
            "SDK config file {} missing or unreadable; refusing to start",
            unique_path_shown(&lines, "SDK config file ")
        ),
    );
    has(
        &lines,
        sys::SAI_API_SWITCH,
        ERROR,
        "SAI_OPENBCM_UNIT 'x': not a unit number (0..255)",
    );
    let failed = lines
        .iter()
        .find(|l| l.message().starts_with("SDK init failed: "))
        .unwrap();
    assert_eq!((failed.api, failed.level), (sys::SAI_API_SWITCH, ERROR));
    assert!(
        failed.message().ends_with(&format!("({})", sdk_text(-10))),
        "{}",
        failed.text
    );
}

/// The path a line starting with `prefix` names (up to the next blank).
fn unique_path_shown(lines: &[Line], prefix: &str) -> String {
    lines
        .iter()
        .find_map(|l| l.message().strip_prefix(prefix))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_owned()
}

#[test]
fn failed_glue_calls_are_logged_as_rv_logs_them() {
    let chip = Chip::new();
    let mut b = OpenbcmBackend::on_chip(chip.clone());
    let (sink, _rx) = EventSink::channel();
    b.init(&own_profile("ports 2 4 100000\n", &[]), sink).unwrap();
    let lines = lines_of(|| {
        assert!(chip.fail("enable", 1, -11));
        assert_eq!(b.port_admin_set(HwPort(1), true), Err(Error::Failure));
        // OB_E_UNAVAIL: info, not error (the C++ RV())
        assert!(chip.fail("enable", 1, -16));
        assert_eq!(b.port_admin_set(HwPort(1), true), Err(Error::NotSupported));
        // the C++ backend's notices of its own
        let gid = b.fp_group_create(&[FpQual::InPorts, FpQual::EtherType], 7).unwrap();
        let mac = MacAddr([2, 0, 0, 0, 0, 9]);
        let station = b.l3_station_add(mac).unwrap();
        log::info(sys::SAI_API_ACL, format_args!("ids {} {}", gid.0, station.0));
    });
    has(
        &lines,
        sys::SAI_API_UNSPECIFIED,
        ERROR,
        &format!("ob_sdk_port_enable_set(unit_, hw, up): {}", sdk_text(-11)),
    );
    has(
        &lines,
        sys::SAI_API_UNSPECIFIED,
        INFO,
        &format!("ob_sdk_port_enable_set(unit_, hw, up): {}", sdk_text(-16)),
    );
    let ids = lines
        .iter()
        .find_map(|l| l.message().strip_prefix("ids "))
        .unwrap()
        .to_owned();
    let (gid, station) = ids.split_once(' ').unwrap();
    let group = lines
        .iter()
        .find(|l| {
            l.message()
                .starts_with(&format!("field group {gid}: priority 7, qualifiers 0x"))
        })
        .unwrap();
    assert_eq!((group.api, group.level), (sys::SAI_API_ACL, NOTICE));
    has(
        &lines,
        sys::SAI_API_ROUTER_INTERFACE,
        NOTICE,
        &format!("my-station {station}: 02:00:00:00:00:09 (IPv4, IPv6, ARP)"),
    );
    b.shutdown();
}

#[test]
fn a_missing_knet_link_file_is_logged_for_the_host_interface_api() {
    let path = unique("no-dir").join("link");
    let lines = lines_of(|| {
        let mut b = OpenbcmBackend::on_chip(Chip::new()).with_knet_link(&path);
        assert_eq!(b.netif_carrier_set("Ethernet0", true), Err(Error::Failure));
    });
    has(
        &lines,
        sys::SAI_API_HOSTIF,
        ERROR,
        &format!("cannot open {} (is linux-bcm-knet loaded?)", path.display()),
    );
}

#[test]
fn what_the_callback_trampolines_refuse_reaches_the_log() {
    let chip = Chip::new();
    let mut b = OpenbcmBackend::on_chip(chip.clone());
    let (sink, rx) = EventSink::channel();
    b.init(&own_profile("ports 2 4 100000\n", &[]), sink).unwrap();
    let key = chip.callback_key();
    let lines = lines_of(|| {
        // refused by sdk-glue (a negative port, a bad link state, a NULL MAC): reported
        Chip::stray_link(key, -5, 1);
        Chip::stray_link(key, 1, 7);
        assert!(chip.l2_event(None, 10, 1, 1));
        // passed by sdk-glue, out of the SAI's 16 bits: the backend's own line
        Chip::stray_link(key, 70_000, 1);
        // from an SDK thread, as the link scan calls it
        let k = key;
        std::thread::spawn(move || Chip::stray_link(k, -9, 0)).join().unwrap();
    });
    has(
        &lines,
        sys::SAI_API_PORT,
        ERROR,
        "SDK callback: link callback of port -5: arguments out of range, refused",
    );
    has(
        &lines,
        sys::SAI_API_PORT,
        ERROR,
        "SDK callback: link callback of port 1: arguments out of range, refused",
    );
    has(
        &lines,
        sys::SAI_API_PORT,
        ERROR,
        "SDK callback: link callback of port -9: arguments out of range, refused",
    );
    has(
        &lines,
        sys::SAI_API_FDB,
        ERROR,
        "SDK callback: L2 callback of port 1: arguments out of range, refused",
    );
    has(
        &lines,
        sys::SAI_API_PORT,
        ERROR,
        "link event of port 70000: out of the SAI's range, dropped",
    );
    assert!(rx.try_recv().is_err(), "no event got through");
    b.shutdown();
}

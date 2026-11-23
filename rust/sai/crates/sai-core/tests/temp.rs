//! The switch temperature attributes and the publisher of fan control's files (`sai_temp.cpp`,
//! `temp_files.c`) on the virtual backend: the C++ `test_temperature` values and the publisher
//! part of `test_dump_and_teardown`.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
use sai_core::backend::{Backend, EventSink, Mode, Profile, Recording, Temp};
use sai_core::error::{AttrError, Error};
use sai_core::temp::{self, Publisher, Unwritten};
use sai_core::value::Value;
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::path::{Path, PathBuf};
use std::time::Duration;

const COUNT: sys::sai_attr_id_t = sys::SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS;
const LIST: sys::sai_attr_id_t = sys::SAI_SWITCH_ATTR_TEMP_LIST;
const MAX: sys::sai_attr_id_t = sys::SAI_SWITCH_ATTR_MAX_TEMP;
const AVG: sys::sai_attr_id_t = sys::SAI_SWITCH_ATTR_AVERAGE_TEMP;

fn backend() -> VirtualBackend {
    let mut b = VirtualBackend::new();
    let profile = Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "4")]);
    b.init(&profile, EventSink::channel().0).unwrap();
    b
}

/// A fresh directory of this test (not created: the publisher makes a missing parent).
fn test_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("sai-core-temp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn poll(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn slurp(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn profile(pairs: &[(&str, &str)]) -> Profile {
    Profile::from_pairs(pairs.iter().copied())
}

#[test]
fn the_attributes_answer_the_cpp_values() {
    let mut b = backend();
    // fake: 8 monitors at 40.0, 41.5, ... 50.5 C
    assert_eq!(temp::switch_temp_get(&b, COUNT), Ok(Value::U8(8)));
    assert_eq!(
        temp::switch_temp_get(&b, LIST),
        Ok(Value::I32List(vec![40, 42, 43, 45, 46, 48, 49, 51])) // 41.5 rounds away from zero
    );
    assert_eq!(temp::switch_temp_get(&b, MAX), Ok(Value::I32(51))); // 50.5
    assert_eq!(temp::switch_temp_get(&b, AVG), Ok(Value::I32(45))); // 45.25
    b.set_temps(vec![-55, 1205]); // below zero rounds away from zero too
    assert_eq!(temp::switch_temp_get(&b, MAX), Ok(Value::I32(121)));
    assert_eq!(temp::switch_temp_get(&b, AVG), Ok(Value::I32(58))); // 57.5
    assert_eq!(temp::switch_temp_get(&b, LIST), Ok(Value::I32List(vec![-6, 121])));
    assert_eq!(temp::switch_temp_get(&b, COUNT), Ok(Value::U8(2)));
    // sensor read fails: the error, no stale values; the read comes before the attribute check
    b.set_temp_fail(true);
    assert_eq!(temp::switch_temp_get(&b, MAX), Err(Error::Failure));
    assert_eq!(
        temp::switch_temp_get(&b, sys::SAI_SWITCH_ATTR_PORT_NUMBER),
        Err(Error::Failure)
    );
    b.set_temp_fail(false);
    assert_eq!(
        temp::switch_temp_get(&b, sys::SAI_SWITCH_ATTR_PORT_NUMBER),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    // no monitor: FAILURE
    b.set_temps(Vec::new());
    assert_eq!(temp::switch_temp_get(&b, COUNT), Err(Error::Failure));
    // a re-init brings the eight monitors back
    b.init(&Profile::default(), EventSink::channel().0).unwrap();
    assert_eq!(temp::switch_temp_get(&b, MAX), Ok(Value::I32(51)));
    for attr in [COUNT, LIST, MAX, AVG] {
        assert!(temp::is_temp_attr(attr));
    }
    assert!(!temp::is_temp_attr(sys::SAI_SWITCH_ATTR_PORT_NUMBER));
}

#[test]
fn backend_errors_are_mapped_as_attr_backend_status() {
    let mut b = backend();
    for (error, expected) in [
        (Error::InvalidParameter, Error::attr(AttrError::InvalidValue, 0)),
        (Error::NotSupported, Error::attr(AttrError::NotSupported, 0)),
        (Error::NotImplemented, Error::attr(AttrError::NotImplemented, 0)),
        (Error::NoMemory, Error::NoMemory),
        (Error::Failure, Error::Failure),
    ] {
        b.fail_nth("asic_temperatures", 1, error).unwrap();
        assert_eq!(temp::switch_temp_get(&b, MAX), Err(expected), "{error:?}");
        assert_eq!(temp::switch_temp_get(&b, MAX), Ok(Value::I32(51)));
    }
    // a boxed backend answers the same
    let boxed: Box<dyn Backend> = Box::new(backend());
    assert_eq!(temp::switch_temp_get(&*boxed, AVG), Ok(Value::I32(45)));
    assert!(boxed.temperature_reader().is_some());
}

#[test]
fn the_publisher_writes_and_removes_fan_controls_files() {
    let mut b = backend();
    let dir = test_dir("files");
    let (out, prom) = (dir.join("temperature"), dir.join("asic.prom"));
    let p = profile(&[
        ("SAI_OPENBCM_TEMP_FILE", out.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_PROM", prom.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", "1"),
    ]);
    let publisher = Publisher::start(&p, b.temperature_reader().unwrap()).unwrap();
    assert!(poll(|| out.exists() && prom.exists()));
    assert_eq!(slurp(&out), "50.5\n");
    let text = slurp(&prom);
    assert!(text.contains("switch_asic_temperature_max_celsius{unit=\"0\"} 50.5\n"));
    assert!(text.contains("switch_asic_temperature_celsius{unit=\"0\",sensor=\"pvtmon7\"} 50.5\n"));
    assert!(text.contains("switch_asic_temperature_peak_celsius{unit=\"0\",sensor=\"pvtmon0\"} 40.5\n"));
    assert!(text.contains("switch_asic_temperature_read_timestamp_seconds{unit=\"0\"} "));
    // the whole text is ob_temp_write's, with the read's time
    let stamp: i64 = text
        .lines()
        .last()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let cur: Vec<i32> = (0..8).map(|i| 400 + 15 * i).collect();
    let peak: Vec<i32> = cur.iter().map(|t| t + 5).collect();
    assert_eq!(text, temp::prom_text(0, &cur, &peak, stamp).unwrap());
    assert!(!dir.join("temperature.tmp").exists());

    b.set_temp_fail(true); // failed read: both files go
    assert!(poll(|| !out.exists() && !prom.exists()));
    b.set_temps(vec![612, 2000]); // 200 C: implausible, still no file (set before the
    b.set_temp_fail(false); // failure ends: no read sees the old values)
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!out.exists());
    assert!(!prom.exists());
    b.set_temps(vec![612, 433]);
    assert!(poll(|| slurp(&out) == "61.2\n"));
    assert!(poll(
        || slurp(&prom).contains("switch_asic_temperature_max_celsius{unit=\"0\"} 61.2\n")
    ));
    publisher.stop(); // removes the files
    assert!(!out.exists() && !prom.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_file_keys_no_publisher_and_drop_stops_it() {
    let b = backend();
    let reader = b.temperature_reader().unwrap();
    assert!(Publisher::start(&Profile::default(), reader.clone()).is_none());
    assert!(
        Publisher::start(
            &profile(&[("SAI_OPENBCM_TEMP_FILE", ""), ("SAI_OPENBCM_TEMP_PROM", "")]),
            reader.clone()
        )
        .is_none()
    );
    // only the Prometheus file, unit 3; dropped instead of stopped
    let dir = test_dir("prom-only");
    let prom = dir.join("asic.prom");
    let p = profile(&[
        ("SAI_OPENBCM_TEMP_PROM", prom.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", "1"),
        ("SAI_OPENBCM_UNIT", "3"),
    ]);
    let publisher = Publisher::start(&p, reader.clone()).unwrap();
    assert!(poll(
        || slurp(&prom).contains("switch_asic_temperature_max_celsius{unit=\"3\"} 50.5\n")
    ));
    assert!(!dir.join("temperature").exists());
    drop(publisher);
    assert!(!prom.exists());
    // an interval out of 1..3600 is 5 (C++: a warning, then 5)
    for bad in ["0", "3601", "-1", "x"] {
        let p = profile(&[
            ("SAI_OPENBCM_TEMP_PROM", prom.to_str().unwrap()),
            ("SAI_OPENBCM_TEMP_INTERVAL", bad),
        ]);
        let publisher = Publisher::start(&p, reader.clone()).unwrap();
        assert!(format!("{publisher:?}").contains("interval: 5"), "{bad}");
        publisher.stop();
        assert!(!prom.exists());
    }
    let p = profile(&[
        ("SAI_OPENBCM_TEMP_PROM", prom.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", " 7s"),
    ]);
    let publisher = Publisher::start(&p, reader).unwrap();
    assert!(format!("{publisher:?}").contains("interval: 7"));
    drop(publisher);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_implausible_first_read_writes_nothing() {
    let mut b = backend();
    b.set_temps(vec![-401]);
    let dir = test_dir("implausible");
    let out = dir.join("temperature");
    let p = profile(&[
        ("SAI_OPENBCM_TEMP_FILE", out.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", "1"),
    ]);
    let publisher = Publisher::start(&p, b.temperature_reader().unwrap()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!out.exists());
    b.set_temps(vec![-400, 1500]);
    assert!(poll(|| slurp(&out) == "150.0\n"));
    publisher.stop();
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_failures_name_the_file_and_the_error() {
    let dir = test_dir("write");
    // the parent's parent is missing: one level is made, not two
    let deep = dir.join("a").join("temperature");
    let failure = temp::write_files(Some(&deep), None, 0, &[400], &[405], 0).unwrap_err();
    assert_eq!(failure.unwritten, Unwritten::Out);
    assert_eq!(temp::strerror(&failure.error), "No such file or directory");
    // one missing level is made (mode 0755)
    let out = dir.join("temperature");
    temp::write_files(Some(&out), None, 0, &[-55, -5], &[0, 0], 0).unwrap();
    assert_eq!(slurp(&out), "-0.5\n");
    // a Prometheus text longer than the C++ buffer: ENOSPC, nothing written; the plain file is
    let prom = dir.join("asic.prom");
    let many = vec![400; 100];
    let failure = temp::write_files(Some(&out), Some(&prom), 0, &many, &many, 0).unwrap_err();
    assert_eq!(failure.unwritten, Unwritten::Prom);
    assert_eq!(temp::strerror(&failure.error), "No space left on device");
    assert!(!prom.exists());
    assert_eq!(slurp(&out), "40.0\n");
    // both failing: both named, the last error kept (`dir` exists now: two levels missing)
    let deeper = dir.join("b").join("c").join("temperature");
    let failure = temp::write_files(Some(&deeper), Some(&prom), 0, &many, &many, 0).unwrap_err();
    assert_eq!(failure.unwritten, Unwritten::Both);
    assert_eq!(failure.error.raw_os_error(), Some(28));
    // no monitor: EINVAL for each file asked
    let failure = temp::write_files(Some(&out), None, 0, &[], &[], 0).unwrap_err();
    assert_eq!(failure.unwritten, Unwritten::Out);
    assert_eq!(temp::strerror(&failure.error), "Invalid argument");
    assert!(temp::write_files(None, None, 0, &[], &[], 0).is_ok());
    temp::remove_files(Some(&out), Some(&prom));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recorded_reader_logs_the_asic_temperatures_line() {
    let mut rec = Recording::new(VirtualBackend::new(), Mode::Normal);
    rec.init(&Profile::default(), EventSink::channel().0).unwrap();
    let log = rec.log();
    log.take();
    // the line of the same call made through the backend
    let direct = rec.asic_temperatures().unwrap();
    let expected = log.take();
    assert_eq!(
        expected,
        ["asic_temperatures() -> SUCCESS cur=[400,415,430,445,460,475,490,505] peak=[405,420,435,450,465,480,495,510]"]
    );
    let reader = rec.temperature_reader().unwrap();
    assert_eq!(reader().unwrap(), direct);
    assert_eq!(log.take(), expected);
    rec.inner_mut().set_temp_fail(true);
    assert_eq!(reader(), Err(Error::Failure));
    assert_eq!(log.take(), ["asic_temperatures() -> FAILURE"]);
    // a publisher's reads are logged the same way
    rec.inner_mut().set_temp_fail(false);
    let dir = test_dir("recording");
    let out = dir.join("temperature");
    let p = profile(&[
        ("SAI_OPENBCM_TEMP_FILE", out.to_str().unwrap()),
        ("SAI_OPENBCM_TEMP_INTERVAL", "1"),
    ]);
    let publisher = Publisher::start(&p, reader).unwrap();
    assert!(poll(|| out.exists()));
    publisher.stop();
    assert_eq!(log.take().first().map(String::as_str), Some(expected[0].as_str()));
    let _ = std::fs::remove_dir_all(&dir);
}

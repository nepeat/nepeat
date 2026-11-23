//! The virtual backend's port map (`SAI_RS_VIRTUAL_PORT_MAP`, [`parse_port_map`]): the language,
//! its refusals, and a switch laid out by it (mixed lane counts, logical numbers with gaps).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod common;

use common::*;
use sai_core::backend::{HwPort, Profile};
use sai_core::log;
use sai_core::oid::TypedId;
use sai_core::sai::Sai;
use sai_core::value::Value;
use sai_core::virt::{MappedPort, VirtualBackend, parse_port_map};
use sai_sys as sys;
use std::fmt::Write as _;
use std::path::PathBuf;

fn file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
        .join(format!("sai-core-port-map-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn the_language_and_its_refusals() {
    let ok = "# a comment\n\nport 50 49 1 25000 xe0\n  port 1 1 4 100000 ce0  \nport 54 53 4 100000 ce1\n";
    assert_eq!(
        parse_port_map(ok).unwrap(),
        vec![
            MappedPort {
                hw: HwPort(1),
                lanes: vec![1, 2, 3, 4],
                speed: 100_000,
                name: "ce0".into()
            },
            MappedPort {
                hw: HwPort(50),
                lanes: vec![49],
                speed: 25_000,
                name: "xe0".into()
            },
            MappedPort {
                hw: HwPort(54),
                lanes: vec![53, 54, 55, 56],
                speed: 100_000,
                name: "ce1".into()
            },
        ]
    );
    for (text, line) in [
        ("", 0),
        ("ports 2 4 100000\n", 1),
        ("port 1 1 4 100000\n", 1),
        ("port 0 1 4 100000 cpu\n", 1),
        ("port 70000 1 4 100000 x\n", 1),
        ("port 1 0 4 100000 x\n", 1),
        ("port 1 1 9 100000 x\n", 1),
        ("port 1 1 4 0 x\n", 1),
        ("port 1 1 4 100000 abcdefghijklmnop\n", 1),
        ("port 1 1 4 100000 a\nport 5 4 1 25000 b\n", 2),
        ("port 1 1 4 100000 a\nport 1 5 4 100000 b\n", 2),
        ("port 1 1 4 100000 a\nport 5 5 4 100000 a\n", 2),
    ] {
        assert_eq!(parse_port_map(text).map_err(|e| e.0), Err(line), "{text:?}");
    }
    let many: String = (1..=257).fold(String::new(), |mut s, i| {
        let _ = writeln!(s, "port {i} {i} 1 25000 p{i}");
        s
    });
    assert_eq!(parse_port_map(&many).map_err(|e| e.0), Err(257));
}

#[test]
fn a_switch_laid_out_by_a_map_and_a_refused_map() {
    let map = file(
        "mixed.ports",
        "port 5 5 4 100000 ce1\nport 1 1 4 100000 ce0\nport 9 9 1 25000 xe0\nport 10 10 1 25000 xe1\n",
    );
    let mut sai = Sai::new(VirtualBackend::new());
    let profile = Profile::from_pairs([
        ("SAI_RS_VIRTUAL_PORT_MAP", map.display().to_string()),
        // not read with a map
        ("SAI_RS_VIRTUAL_PORTS", "32".to_owned()),
    ]);
    let switch = sai.create_switch(&init_attrs(), &profile).unwrap();
    let ports = oid_list(get1(&sai, SWITCH, switch.raw(), sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
    let laid: Vec<(Value, Value)> = ports
        .iter()
        .map(|p| {
            let mut v = sai
                .get(PORT, *p, &[sys::SAI_PORT_ATTR_HW_LANE_LIST, sys::SAI_PORT_ATTR_SPEED])
                .unwrap();
            (v.remove(0), v.remove(0))
        })
        .collect();
    assert_eq!(
        laid,
        [
            (Value::U32List(vec![1, 2, 3, 4]), Value::U32(100_000)),
            (Value::U32List(vec![5, 6, 7, 8]), Value::U32(100_000)),
            (Value::U32List(vec![9]), Value::U32(25_000)),
            (Value::U32List(vec![10]), Value::U32(25_000)),
        ]
    );
    sai.remove(SWITCH, switch.raw()).unwrap();

    let bad = file("bad.ports", "port 1 1 4 100000 ce0\nport 2 4 1 25000 xe0\n");
    let (result, lines) = log::capture(|| {
        let mut sai = Sai::new(VirtualBackend::new());
        let profile = Profile::from_pairs([("SAI_RS_VIRTUAL_PORT_MAP", bad.display().to_string())]);
        sai.create_switch(&init_attrs(), &profile).map(|_| ())
    });
    assert!(result.is_err());
    assert!(
        lines.iter().any(|l| l.api == sys::SAI_API_SWITCH
            && l.level == sys::SAI_LOG_LEVEL_ERROR
            && l.message()
                == format!(
                    "SAI_RS_VIRTUAL_PORT_MAP {} line 2: a lane of another port",
                    bad.display()
                )),
        "{lines:?}"
    );
}

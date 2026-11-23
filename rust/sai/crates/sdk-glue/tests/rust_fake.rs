//! Every wrapper of this crate against the pure-Rust fake ([`sdk_glue::rust_fake`]): success,
//! scripted faults of every operation, argument checks, the callback trampolines and the type
//! state. No C runs here: this file is what Miri runs. Each test has its own chip, so they run in parallel.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use sdk_glue::rust_fake::{Chip, OPS};
use sdk_glue::{
    Attached, Code, Dest, Error, Fec, FpEntry, FpQual, FpRule, Gate, InitError, IpType, L2Event, Learn, Loopback, Mmd,
    PolicerCfg, PortStat, RouteTarget, Sdk, Switch, TrapKind, TrunkMember,
};

fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!(
        "sdk-glue-rust-fake-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scenario(text: &str) -> PathBuf {
    let path = scratch().join("scenario.cfg");
    std::fs::write(&path, text).unwrap();
    path
}

const D1: Mmd = Mmd::new(1).unwrap();
const MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01];

fn code(e: Error) -> Code {
    e.code()
}

fn switch(chip: &Chip, text: &str) -> Switch {
    Switch::init_on(chip, 0, &scenario(text), None).unwrap().0
}

fn refused<T: std::fmt::Debug>(r: Result<T, InitError>) -> (Code, String) {
    match r {
        Err(InitError::Sdk { error, message }) => (error.code(), message),
        other => panic!("expected the fake to refuse, got {other:?}"),
    }
}

#[test]
fn mdio_bring_up_registers_and_trace() {
    let chip = Chip::new();
    let dir = scratch();
    let trace = dir.join("trace.txt");
    let cfg = scenario(&format!(
        "# scenario\nname TEST56960_B1\nreg 24 1 8b00 2391\ntrace {}\n",
        trace.display()
    ));
    let (mut sdk, line) = Sdk::init_mdio_on(&chip, 0, &cfg).unwrap();
    assert_eq!(
        line,
        "TEST56960_B1: init soc + init misc done; external MDIO rate register 0x000d0001, hold delay 13 (as configured)"
    );
    assert_eq!(sdk.unit(), 0);
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b00), Ok(0x2391));
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b01), Ok(0));
    let d31 = Mmd::new(31).unwrap();
    assert_eq!(sdk.mdio_c45_write(0x17c, d31, 0xffff, 0xfffe), Ok(()));
    assert_eq!(sdk.mdio_c45_read(0x17c, d31, 0xffff), Ok(0xfffe));
    assert_eq!(chip.mdio_reg(0x17c, 31, 0xffff), Some(0xfffe));
    // the address word beyond the glue's range: refused here, nothing reaches the fake
    assert_eq!(sdk.mdio_c45_read(0x800, D1, 0).map_err(code), Err(Code::Param));
    assert_eq!(
        sdk.mdio_c45_write(0xffff_ffff, D1, 0, 0).map_err(code),
        Err(Code::Param)
    );
    drop(sdk);
    // the C fake's trace lines, byte for byte; no shutdown after init_mdio
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        "init unit 0\nR 24 1 8b00 = 2391\nR 24 1 8b01 = 0000\nW 17c 1f ffff fffe\nR 17c 1f ffff = fffe\n"
    );
}

#[test]
fn one_handle_per_chip_chips_are_independent() {
    let (a, b) = (Chip::new(), Chip::new());
    let cfg = scenario("");
    let (sdk_a, _) = Sdk::init_mdio_on(&a, 0, &cfg).unwrap();
    let (sdk_b, _) = Sdk::init_mdio_on(&b, 0, &cfg).unwrap();
    // twice: a refused attempt must not release the live handle's claim
    assert_eq!(Sdk::init_mdio_on(&a, 0, &cfg).unwrap_err(), InitError::AlreadyOpen);
    assert_eq!(Switch::init_on(&a, 0, &cfg, None).unwrap_err(), InitError::AlreadyOpen);
    assert_eq!(Attached::attach_on(&a, 0).unwrap_err(), InitError::AlreadyOpen);
    drop(sdk_a);
    let (mut again, _) = Sdk::init_mdio_on(&a, 0, &cfg).unwrap();
    assert_eq!(again.mdio_c45_read(1, D1, 2), Ok(0));
    drop(sdk_b);
}

#[test]
fn bring_up_failures_carry_code_and_reason() {
    let chip = Chip::new();
    let missing = scratch().join("no-such.cfg");
    let (c, m) = refused(Sdk::init_mdio_on(&chip, 0, &missing));
    assert_eq!(c, Code::Config);
    assert_eq!(
        m,
        format!("SDK config file '{}' missing or unreadable (2)", missing.display())
    );
    let (c, m) = refused(Sdk::init_mdio_on(&chip, 0, &scenario("init busy\n")));
    assert_eq!(c, Code::Busy);
    assert!(
        m.starts_with("another SDK process owns the switch (lock /run/openbcm-unit0.lock held"),
        "{m}"
    );
    let (c, m) = refused(Sdk::init_mdio_on(&chip, 0, &scenario("init delay\n")));
    assert_eq!(c, Code::MdioClock);
    assert!(m.contains("refusing MDIO access"), "{m}");
    assert_eq!(
        refused(Sdk::init_mdio_on(&chip, 0, &scenario("init divisor\n"))).0,
        Code::MdioClock
    );
    assert_eq!(
        refused(Sdk::init_mdio_on(&chip, 0, &scenario("init soc\n"))),
        (Code::Fail, "init soc failed (-1)".to_owned())
    );
    assert_eq!(
        refused(Sdk::init_mdio_on(&chip, 0, &scenario("init unit\n"))).0,
        Code::Unit
    );
    assert_eq!(refused(Sdk::init_mdio_on(&chip, 1, &scenario(""))).0, Code::Unit);
    let (c, m) = refused(Sdk::init_mdio_on(&chip, 0, &scenario("bogus line\n")));
    assert_eq!(c, Code::Config);
    assert!(m.ends_with(":1: not a scenario line"), "{m}");
    // only known op names in a fail directive
    assert_eq!(
        refused(Sdk::init_mdio_on(&chip, 0, &scenario("fail no-such-op 1 -11\n"))).0,
        Code::Config
    );
    assert_eq!(
        Sdk::init_mdio_on(&chip, 0, std::path::Path::new("a\0b")).unwrap_err(),
        InitError::BadPath
    );
    // the full bring-up's own failures
    let (c, m) = refused(Switch::init_on(&chip, 0, &scenario(""), Some(&missing)));
    assert_eq!(c, Code::Config);
    assert_eq!(m, format!("rc file '{}' missing or unreadable (2)", missing.display()));
    assert_eq!(
        refused(Switch::init_on(&chip, 0, &scenario("init all\n"), None)).0,
        Code::Fail
    );
    let rc = scenario("# rc\n");
    let (c, m) = refused(Switch::init_on(&chip, 0, &scenario("init rc\n"), Some(&rc)));
    assert_eq!(c, Code::Fail);
    assert_eq!(m, format!("rc file '{}' failed (-1)", rc.display()));
    // a failed bring-up leaves no claim and nothing up
    assert!(!chip.bcm_layer_up());
    let (sw, line) = Switch::init_on(&chip, 0, &scenario("ports 2 4 100000\n"), Some(&rc)).unwrap();
    assert_eq!(
        line,
        "ok: stale KNET netifs 0 filters 0 destroyed; IPv4/IPv6 L3 on 2 ports"
    );
    drop(sw);
}

#[test]
fn scripted_mdio_faults_reach_the_caller() {
    let chip = Chip::new();
    let cfg = scenario("const 24 1900\ndead 26\nafter 2 1 1 8221 0303\nhang 2 1 1900\nfail-read 5\nfail-write 3\n");
    let (mut sdk, _) = Sdk::init_mdio_on(&chip, 0, &cfg).unwrap();
    assert_eq!(sdk.mdio_c45_read(0x24, D1, 0x8b00), Ok(0x1900));
    assert_eq!(sdk.mdio_c45_read(0x26, D1, 0x8b00).map_err(code), Err(Code::Timeout));
    assert_eq!(
        sdk.mdio_c45_write(0x26, D1, 0x8b00, 1).map_err(code),
        Err(Code::Timeout)
    );
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0));
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0));
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221).map_err(code), Err(Code::Fail)); // the 5th read
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0x0303));
    assert_eq!(sdk.mdio_c45_write(0, D1, 1, 7), Ok(())); // 2nd write: phy 1 hangs from here
    assert_eq!(sdk.mdio_c45_read(1, D1, 0x8221), Ok(0x1900));
    assert_eq!(sdk.mdio_c45_write(0, D1, 1, 8).map_err(code), Err(Code::Fail)); // the 3rd write
    assert_eq!(sdk.mdio_c45_read(0, D1, 1), Ok(7));
}

#[test]
fn temperatures_and_the_reader_handle() {
    let chip = Chip::new();
    let (mut sdk, _) = Sdk::init_mdio_on(&chip, 0, &scenario("")).unwrap();
    assert_eq!(sdk.temperatures().map_err(code), Err(Code::Unavail));
    drop(sdk);
    let (mut sdk, _) = Sdk::init_mdio_on(&chip, 0, &scenario("temp 413 398 -20\ntemp-fail 4\n")).unwrap();
    let t = sdk.temperatures().unwrap();
    assert_eq!(t.current(), &[413, 398, -20]);
    assert_eq!(t.peak(), &[428, 413, -5]);
    let reader = sdk.temp_reader();
    let other = reader.clone();
    let from_thread = std::thread::spawn(move || other.read().map(|t| t.current().to_vec()))
        .join()
        .unwrap();
    assert_eq!(from_thread, Ok(vec![413, 398, -20]));
    assert_eq!(reader.read().map(|t| t.current().len()), Ok(3));
    assert_eq!(reader.read().map_err(code), Err(Code::Timeout)); // the 4th read
    drop(sdk);
    // the owner is gone: the handle refuses without reaching the fake
    assert_eq!(reader.read().map_err(code), Err(Code::Init));
    let many = format!(
        "temp {}\n",
        (0..16).map(|i| (400 + i).to_string()).collect::<Vec<_>>().join(" ")
    );
    let (mut sdk, _) = Sdk::init_mdio_on(&chip, 0, &scenario(&many)).unwrap();
    assert_eq!(sdk.temperatures().unwrap().current()[15], 415);
    drop(sdk);
    let seventeen = format!(
        "temp {}\n",
        (0..17).map(|i| (400 + i).to_string()).collect::<Vec<_>>().join(" ")
    );
    assert_eq!(
        refused(Sdk::init_mdio_on(&chip, 0, &scenario(&seventeen))).0,
        Code::Config
    );
}

#[test]
fn attach_only_reads_temperatures_and_powers_the_pll_down() {
    let chip = Chip::new();
    chip.load("name ATTACH56960\ntemp 500 510\n").unwrap();
    assert_eq!(chip.load("nonsense\n"), Err(1));
    let (mut a, line) = Attached::attach_on(&chip, 0).unwrap();
    assert_eq!(line, "attached ATTACH56960");
    assert_eq!(a.unit(), 0);
    assert_eq!(a.temperatures().unwrap().current(), &[500, 510]);
    let reader = a.temp_reader();
    assert!(reader.read().is_ok());
    // one owner per chip, whatever kind
    assert_eq!(
        Sdk::init_mdio_on(&chip, 0, &scenario("")).unwrap_err(),
        InitError::AlreadyOpen
    );
    assert_eq!(a.overtemp_shutdown(), Ok(()));
    assert!(chip.pll_down());
    // the S-channel is dead afterwards
    assert_eq!(a.temperatures().map_err(code), Err(Code::Timeout));
    drop(a);
    assert_eq!(reader.read().map_err(code), Err(Code::Init));
    assert_eq!(refused(Attached::attach_on(&chip, 3)).0, Code::Unit);
    chip.load("init busy\n").unwrap();
    assert_eq!(refused(Attached::attach_on(&chip, 0)).0, Code::Busy);
}

#[test]
fn the_mdio_bus_handle_works_from_other_threads_until_its_owner_goes() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 1 4 100000\nreg 24 1 2 abcd\n");
    let bus = sw.mdio_bus();
    assert!(bus.is_live());
    let handles: Vec<_> = (0..4u16)
        .map(|i| {
            let bus = bus.clone();
            std::thread::spawn(move || {
                bus.write(0x30, D1, i, 0x100 + i).unwrap();
                bus.read(0x24, D1, 2).unwrap()
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), 0xabcd);
    }
    assert_eq!(sw.mdio_c45_read(0x30, D1, 3), Ok(0x103));
    assert_eq!(bus.read(0x800, D1, 0).map_err(code), Err(Code::Param));
    drop(sw);
    assert!(!bus.is_live());
    assert_eq!(bus.read(0x24, D1, 2).map_err(code), Err(Code::Init));
    assert_eq!(bus.write(0x24, D1, 2, 1).map_err(code), Err(Code::Init));
}

#[test]
fn ports_enumerate_and_take_their_settings() {
    let chip = Chip::new();
    let mut sw = switch(
        &chip,
        "name TEST56960_B1\nports 2 4 100000\nport 66 129 1 10000 xe0\ncpu 0\n",
    );
    assert_eq!(sw.hw_info().unwrap(), "TEST56960_B1");
    assert_eq!(sw.cpu_port(), 0);
    let ports = sw.ports().unwrap();
    assert_eq!(ports.len(), 3);
    assert_eq!(
        (ports[2].port, ports[2].first_lane, ports[2].lanes, ports[2].speed),
        (66, 129, 1, 10_000)
    );
    assert_eq!(ports[0].name, "ce0");
    assert_eq!(chip.port_enabled(1), Some(true)); // as "init all" leaves them
    sw.port_enable_set(1, false).unwrap();
    assert_eq!(chip.port_enabled(1), Some(false));
    assert_eq!(sw.port_link_get(1), Ok(false));
    assert_eq!(sw.port_speeds(1).unwrap(), vec![40_000, 100_000]);
    assert_eq!(sw.port_speeds(66).unwrap(), vec![1000, 10_000, 25_000]);
    sw.port_speed_set(1, 40_000).unwrap();
    assert_eq!(sw.port_speed_get(1), Ok(40_000));
    assert_eq!(sw.port_speed_set(1, 25_000).map_err(code), Err(Code::Param));
    assert_eq!(sw.port_speed_set(1, u32::MAX).map_err(code), Err(Code::Param)); // not an int
    sw.port_frame_max_set(1, 9100).unwrap();
    assert_eq!(sw.port_frame_max_set(1, 0).map_err(code), Err(Code::Param));
    for (fec, mode) in [(Fec::None, 0), (Fec::Rs, 1), (Fec::Fc, 2)] {
        sw.port_fec_set(2, fec).unwrap();
        assert_eq!(chip.port(2).unwrap().fec, mode);
    }
    sw.port_autoneg_set(2, true).unwrap();
    assert_eq!(sw.port_autoneg_get(2), Ok(true));
    sw.port_interface_set_name(2, "caui4").unwrap();
    assert_eq!(chip.port(2).unwrap().interface.as_deref(), Some("caui4"));
    assert_eq!(sw.port_interface_set_name(2, "bogus").map_err(code), Err(Code::Param));
    assert_eq!(sw.port_interface_set(2, c"bogus").map_err(code), Err(Code::Param)); // the fake's check
    sw.port_ifg_set(2, 128).unwrap();
    assert_eq!(chip.port_settings(2), Some((1, 2, 128, true)));
    for (lb, raw) in [(Loopback::None, 0), (Loopback::Mac, 1), (Loopback::Phy, 2)] {
        sw.port_loopback_set(2, lb).unwrap();
        assert_eq!(chip.port(2).unwrap().loopback, raw);
    }
    sw.port_pvid_set(2, 4094).unwrap();
    assert_eq!(sw.port_pvid_set(2, 4095).map_err(code), Err(Code::Param));
    assert_eq!(sw.port_pvid_set(2, 0).map_err(code), Err(Code::Param));
    for learn in Learn::ALL {
        sw.port_learn_set(2, learn).unwrap();
        assert_eq!(chip.port(2).unwrap().learn, learn.raw());
    }
    for stat in PortStat::ALL {
        assert!(chip.set_counter(66, stat, 1000 + u64::try_from(stat.raw()).unwrap()));
    }
    assert_eq!(sw.port_stat_get(66, PortStat::InOctets), Ok(1000));
    assert_eq!(sw.port_stat_get(66, PortStat::OutMulticastPkts), Ok(1014));
    sw.port_stat_clear(66).unwrap();
    assert_eq!(sw.port_stat_get(66, PortStat::OutMulticastPkts), Ok(0));
    // an unknown port: the glue's OB_E_PORT, from every port call
    assert_eq!(sw.port_enable_set(7, true).map_err(code), Err(Code::Port));
    assert_eq!(sw.port_link_get(7).map_err(code), Err(Code::Port));
    assert_eq!(sw.port_stat_get(-1, PortStat::InOctets).map_err(code), Err(Code::Port));
}

#[test]
fn vlans_and_members() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 3 4 100000\n");
    assert_eq!(sw.vlan_members(1).unwrap(), vec![(1, true), (2, true), (3, true)]);
    sw.vlan_create(100).unwrap();
    assert_eq!(sw.vlan_create(100).map_err(code), Err(Code::Exists));
    sw.vlan_port_add(100, 2, false).unwrap();
    sw.vlan_port_add(100, 3, true).unwrap();
    sw.vlan_port_add(100, 2, true).unwrap(); // tagging changes
    assert_eq!(sw.vlan_members(100).unwrap(), vec![(2, true), (3, true)]);
    sw.vlan_port_remove(100, 3).unwrap();
    sw.vlan_port_remove(100, 3).unwrap(); // no member: no error
    assert_eq!(chip.vlan(100), Some(vec![(2, true)]));
    assert_eq!(sw.vlan_port_add(100, 9, true).map_err(code), Err(Code::Port));
    assert_eq!(sw.vlan_create(0).map_err(code), Err(Code::Param));
    assert_eq!(sw.vlan_create(4095).map_err(code), Err(Code::Param));
    assert_eq!(sw.vlan_destroy(1).map_err(code), Err(Code::BadId));
    sw.vlan_destroy(100).unwrap();
    assert_eq!(sw.vlan_destroy(100).map_err(code), Err(Code::NotFound));
    assert_eq!(sw.vlan_members(100).map_err(code), Err(Code::NotFound));
}

#[test]
fn l2_entries_flushes_and_aging() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 3 4 100000\n");
    let tid = sw.trunk_create().unwrap();
    sw.l2_add(MAC, 10, Dest::Port(1), false, false).unwrap();
    sw.l2_add([2, 0, 0, 0, 0, 2], 10, Dest::Lag(tid), true, false).unwrap();
    sw.l2_add([2, 0, 0, 0, 0, 3], 20, Dest::Port(2), false, true).unwrap();
    let entries = chip.l2_entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].2.dest, 0x4000_0000 | i32::try_from(tid).unwrap());
    assert!(entries[1].2.is_static && entries[2].2.discard);
    // refused before the glue: a VLAN or destination out of range
    assert_eq!(
        sw.l2_add(MAC, 0, Dest::Port(1), false, false).map_err(code),
        Err(Code::Param)
    );
    assert_eq!(
        sw.l2_add(MAC, 10, Dest::Port(-1), false, false).map_err(code),
        Err(Code::Port)
    );
    assert_eq!(
        sw.l2_add(MAC, 10, Dest::Port(0x4000_0001), false, false).map_err(code),
        Err(Code::Port)
    );
    assert_eq!(
        sw.l2_add(MAC, 10, Dest::Lag(0x4000_0000), false, false).map_err(code),
        Err(Code::Port)
    );
    assert_eq!(
        sw.l2_flush(None, Some(4095), false, true).map_err(code),
        Err(Code::Param)
    );
    // the fake's: a port it does not have
    assert_eq!(
        sw.l2_add(MAC, 10, Dest::Port(9), false, false).map_err(code),
        Err(Code::Port)
    );
    assert_eq!(sw.l2_flush(None, None, true, false).map_err(code), Err(Code::Unavail));
    sw.l2_flush(None, None, false, false).unwrap(); // nothing
    sw.l2_flush(Some(Dest::Port(1)), None, false, true).unwrap();
    assert_eq!(chip.l2_entries().len(), 2);
    sw.l2_flush(None, Some(10), false, true).unwrap(); // the static one stays
    assert_eq!(chip.l2_entries().len(), 2);
    sw.l2_flush(Some(Dest::Lag(tid)), Some(10), true, true).unwrap();
    assert_eq!(chip.l2_entries().len(), 1);
    sw.l2_delete([2, 0, 0, 0, 0, 3], 20).unwrap();
    assert_eq!(sw.l2_delete([2, 0, 0, 0, 0, 3], 20).map_err(code), Err(Code::NotFound));
    sw.l2_age_set(300).unwrap();
    assert_eq!(chip.l2_age(), 300);
    assert_eq!(sw.l2_age_set(u32::MAX).map_err(code), Err(Code::Param));
}

#[test]
fn trunks_take_their_members() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 4 4 100000\n");
    let a = sw.trunk_create().unwrap();
    let b = sw.trunk_create().unwrap();
    assert_ne!(a, b);
    let m = |port, e, i| TrunkMember {
        port,
        egress_disable: e,
        ingress_disable: i,
    };
    sw.trunk_set(a, &[m(1, false, false), m(2, true, false), m(3, true, true)])
        .unwrap();
    assert_eq!(chip.trunk(a), Some(vec![(1, 0), (2, 1), (3, 3)]));
    sw.trunk_set(a, &[]).unwrap();
    assert_eq!(chip.trunk(a), Some(vec![]));
    let too_many = vec![m(1, false, false); 65];
    assert_eq!(sw.trunk_set(a, &too_many).map_err(code), Err(Code::Param));
    assert_eq!(sw.trunk_set(a, &[m(9, false, false)]).map_err(code), Err(Code::Port));
    sw.trunk_destroy(b).unwrap();
    assert_eq!(sw.trunk_destroy(b).map_err(code), Err(Code::NotFound));
    assert_eq!(sw.trunk_destroy(u32::MAX).map_err(code), Err(Code::Param));
    assert_eq!(chip.counts().trunks, 1);
}

#[test]
fn l3_interfaces_next_hops_hosts_routes_ecmp() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\n");
    let intf = sw.l3_intf_create(MAC, 100, 9100, 0).unwrap();
    assert_eq!(sw.l3_intf_create(MAC, 100, 9100, 0).map_err(code), Err(Code::Exists));
    sw.l3_intf_update(intf, MAC, 100, 1500, 1).unwrap();
    assert_eq!(sw.l3_intf_create(MAC, 0, 1500, 0).map_err(code), Err(Code::Param));
    let station = sw.l3_station_add(MAC).unwrap();
    let nh = sw
        .l3_egress_create(intf, [2, 0, 0, 0, 0, 9], Dest::Port(1), 100)
        .unwrap();
    let nh2 = sw
        .l3_egress_create(intf, [2, 0, 0, 0, 0, 8], Dest::Port(2), 100)
        .unwrap();
    sw.l3_egress_update(nh2, intf, [2, 0, 0, 0, 0, 7], Dest::Port(2), 100)
        .unwrap();
    assert_eq!(
        sw.l3_egress_create(intf, MAC, Dest::Port(9), 100).map_err(code),
        Err(Code::Port)
    );
    let (cpu, drop) = (sw.l3_egress_cpu().unwrap(), sw.l3_egress_drop().unwrap());
    assert_ne!(cpu, drop);
    assert_eq!(sw.l3_egress_cpu(), Ok(cpu)); // cached
    let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
    let v6 = IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2));
    sw.l3_host_add(0, v4, nh).unwrap();
    sw.l3_host_add(0, v6, nh).unwrap();
    let ecmp = sw.l3_ecmp_create(&[nh, nh2]).unwrap();
    sw.l3_ecmp_update(ecmp, &[nh]).unwrap();
    assert_eq!(sw.l3_ecmp_create(&[]).map_err(code), Err(Code::Param));
    assert_eq!(sw.l3_ecmp_create(&vec![nh; 257]).map_err(code), Err(Code::Param));
    let net = IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0));
    let mask = IpAddr::V4(Ipv4Addr::new(255, 255, 0, 0));
    sw.l3_route_add(0, net, mask, RouteTarget::Egress(nh), false, false)
        .unwrap();
    assert_eq!(
        sw.l3_route_add(0, net, mask, RouteTarget::Egress(nh), false, false)
            .map_err(code),
        Err(Code::Exists)
    );
    sw.l3_route_add(0, net, mask, RouteTarget::Ecmp(ecmp), false, true)
        .unwrap();
    sw.l3_route_add(
        0,
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        RouteTarget::Egress(drop),
        true,
        false,
    )
    .unwrap();
    // mixed families: refused here
    assert_eq!(
        sw.l3_route_add(
            0,
            net,
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            RouteTarget::Egress(nh),
            false,
            false
        )
        .map_err(code),
        Err(Code::Param)
    );
    let c = chip.counts();
    assert_eq!(
        (c.intfs, c.stations, c.egress, c.hosts, c.routes, c.ecmp),
        (1, 1, 4, 2, 2, 1)
    );
    let usage = sw.table_usage().unwrap();
    assert_eq!(usage.hosts_free, 8192 - 3); // an IPv6 host takes 2
    assert_eq!(usage.egress_free, 32768 - 4);
    assert_eq!(sw.chip_limits().unwrap().vrf_max, 1023);
    sw.l3_route_delete(0, net, mask).unwrap();
    assert_eq!(sw.l3_route_delete(0, net, mask).map_err(code), Err(Code::NotFound));
    sw.l3_host_delete(0, v6).unwrap();
    sw.l3_ecmp_destroy(ecmp).unwrap();
    sw.l3_egress_destroy(nh2).unwrap();
    sw.l3_station_delete(station).unwrap();
    sw.l3_intf_destroy(intf).unwrap();
    assert_eq!(sw.l3_intf_destroy(intf).map_err(code), Err(Code::NotFound));
}

#[test]
fn field_groups_entries_counters_and_meters() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\n");
    for (i, q) in FpQual::ALL.iter().enumerate() {
        assert_eq!(q.bit(), 1 << i, "{q:?}");
    }
    let qset = [
        FpQual::InPorts,
        FpQual::EtherType,
        FpQual::IpType,
        FpQual::DstIp6,
        FpQual::SrcIp,
    ];
    let gid = sw.fp_group_create(&qset, 10).unwrap();
    assert_eq!(sw.fp_group_create(&qset, 10).map_err(code), Err(Code::Exists));
    let pid = sw
        .policer_create(&PolicerCfg {
            packets: true,
            cir: 600,
            cbs: 600,
            ..PolicerCfg::default()
        })
        .unwrap();
    let rule = FpRule {
        in_ports: Some(vec![1, 2]),
        ether_type: Some((0x86dd, 0xffff)),
        ip_type: Some(IpType::Ipv6),
        dst_ip6: Some(([0xfd; 16], [0xff; 16])),
        src_ip: Some(([10, 0, 0, 1], [255, 255, 255, 255])),
        priority: 7,
        copy_cpu: true,
        counter: true,
        policer: Some(pid),
        red_drop: true,
        cpu_copy_only: true,
        ..FpRule::default()
    };
    let entry = sw.fp_entry_create(gid, &rule).unwrap();
    let stat = entry.stat.unwrap();
    // what crossed the struct: bits, priority, the in-ports through the pointer
    let seen = chip.fp_entry(entry.id).unwrap();
    let want = qset.iter().fold(0, |b, q| b | q.bit());
    assert_eq!(
        (seen.present, seen.priority, seen.ports.clone(), seen.policer),
        (want, 7, vec![1, 2], i32::try_from(pid).unwrap())
    );
    assert!(chip.fp_hit(stat, 3, 300));
    assert_eq!(sw.fp_stat_get(stat), Ok((3, 300)));
    // a qualifier the group lacks: refused by the fake as by the SDK
    let wrong = FpRule {
        ttl: Some((1, 0xff)),
        ..FpRule::default()
    };
    assert_eq!(sw.fp_entry_create(gid, &wrong).map_err(code), Err(Code::Param));
    // IPv4 and IPv6 on one side share the glue's field: refused here
    let both = FpRule {
        src_ip: Some(([1; 4], [0xff; 4])),
        src_ip6: Some(([1; 16], [0xff; 16])),
        ..FpRule::default()
    };
    assert_eq!(sw.fp_entry_create(gid, &both).map_err(code), Err(Code::Param));
    assert_eq!(sw.policer_destroy(pid).map_err(code), Err(Code::Busy)); // in use
    assert_eq!(sw.fp_group_destroy(gid).map_err(code), Err(Code::Busy));
    sw.policer_set(
        pid,
        &PolicerCfg {
            cir: 1200,
            ..PolicerCfg::default()
        },
    )
    .unwrap();
    assert_eq!(chip.policer(pid).unwrap().cir, 1200);
    sw.fp_entry_destroy(entry).unwrap();
    assert_eq!(
        sw.fp_entry_destroy(FpEntry {
            id: entry.id,
            stat: None
        })
        .map_err(code),
        Err(Code::NotFound)
    );
    sw.policer_destroy(pid).unwrap();
    sw.fp_group_destroy(gid).unwrap();
    let c = chip.counts();
    assert_eq!((c.fp_groups, c.fp_entries, c.fp_stats, c.policers), (0, 0, 0, 0));
}

#[test]
fn knet_netifs_filters_and_cleanup() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\nknet 3 4\n");
    let n = sw.netif_create("Ethernet0", 1, 1, MAC, false).unwrap();
    assert_eq!(sw.netif_create("Ethernet0123456", 1, 1, MAC, false).map(|_| ()), Ok(())); // 15 bytes
    assert_eq!(
        sw.netif_create("Ethernet01234567", 1, 1, MAC, false).map_err(code),
        Err(Code::Param)
    );
    assert_eq!(
        sw.netif_create("a\0b", 1, 1, MAC, false).map_err(code),
        Err(Code::Param)
    );
    assert_eq!(sw.netif_create("x", 9, 1, MAC, false).map_err(code), Err(Code::Port));
    let f = sw.netif_filter_set(n.netif, 1, true, Some(n.filter)).unwrap();
    assert_ne!(f, n.filter);
    sw.netif_destroy(Some(n.netif), Some(f)).unwrap();
    assert_eq!(sw.netif_destroy(Some(n.netif), None).map_err(code), Err(Code::NotFound));
    sw.netif_destroy(None, None).unwrap();
    // one netif + filter left, the bring-up already took the scenario's stale ones
    assert_eq!(sw.knet_cleanup(), Ok((1, 1)));
    assert_eq!(chip.counts().netifs, 0);
}

#[test]
fn traps_shell_and_switch_wide_reads() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 1 4 100000\n");
    for kind in TrapKind::ALL {
        let r = sw.trap_set(kind, true);
        if matches!(kind, TrapKind::Dhcpv6 | TrapKind::Stp | TrapKind::Other) {
            assert_eq!(r.map_err(code), Err(Code::Unavail), "{kind:?}");
        } else {
            assert_eq!(r, Ok(()), "{kind:?}");
            assert_eq!(chip.trap(kind), Some(true));
        }
    }
    sw.trap_set(TrapKind::Lldp, false).unwrap();
    assert_eq!(chip.trap(TrapKind::Lldp), Some(false));
    sw.run_shell("ps").unwrap();
    assert_eq!(sw.run_shell(&"x".repeat(1024)).map_err(code), Err(Code::Param)); // the glue would cut it
    assert_eq!(sw.run_shell("a\0b").map_err(code), Err(Code::Param));
    let path = scratch().join("shell.txt");
    let out = std::fs::File::create(&path).unwrap();
    sw.run_shell_to("show c", std::os::fd::AsFd::as_fd(&out)).unwrap();
    drop(out);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fake: show c\n");
    assert_eq!(chip.shell_log(), vec!["ps".to_owned(), "show c".to_owned()]);
}

#[test]
fn link_events_from_other_threads_reach_the_closure() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\nlink admin\n");
    assert!(!chip.link_event(1, true)); // nobody registered
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    sw.set_link_callback(move |port, up| {
        let _ = tx.lock().unwrap().send((port, up));
    })
    .unwrap();
    assert!(chip.link_callback_registered());
    let remote = chip.clone();
    std::thread::spawn(move || {
        assert!(remote.link_event(1, false)); // up since the unregistered event above
        assert!(!remote.link_event(1, false)); // no change, no call
        assert!(remote.link_event(2, true));
        assert!(!remote.link_event(9, true)); // no such port
    })
    .join()
    .unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (1, false));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (2, true));
    // disabling takes the link down; with "link admin" enabling brings it up, from the call
    sw.port_enable_set(2, false).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (2, false));
    sw.port_enable_set(1, true).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (1, true));
    assert_eq!(sw.port_link_get(1), Ok(true));
    sw.clear_link_callback();
    assert!(!chip.link_callback_registered());
    assert!(!chip.link_event(1, false));
    assert_eq!(sw.callback_stats(), sdk_glue::CallbackStats::default());
}

#[test]
fn l2_events_are_checked_and_typed() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\n");
    let seen: Arc<Mutex<Vec<L2Event>>> = Arc::default();
    let sink = Arc::clone(&seen);
    sw.set_l2_callback(move |e| sink.lock().unwrap().push(e)).unwrap();
    assert!(chip.l2_callback_registered() && !chip.link_callback_registered());
    assert!(chip.l2_event(Some(MAC), 10, 2, 1));
    assert!(chip.l2_event(Some(MAC), 10, 0x4000_0005, 0));
    // out of range: refused by the trampoline, counted, the closure not called
    for (mac, vid, port, learned) in [
        (None, 10, 2, 1),
        (Some(MAC), 0, 2, 1),
        (Some(MAC), 4095, 2, 1),
        (Some(MAC), 10, -1, 1),
        (Some(MAC), 10, 2, 2),
    ] {
        assert!(chip.l2_event(mac, vid, port, learned));
    }
    let events = seen.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![
            L2Event {
                mac: MAC,
                vid: 10,
                dest: Dest::Port(2),
                learned: true
            },
            L2Event {
                mac: MAC,
                vid: 10,
                dest: Dest::Lag(5),
                learned: false
            },
        ]
    );
    assert_eq!(sw.callback_stats().rejected, 5);
    // a link handler next to it, then the link one cleared: the L2 one stays
    sw.set_link_callback(|_, _| {}).unwrap();
    assert!(chip.l2_callback_registered() && chip.link_callback_registered());
    sw.clear_link_callback();
    assert!(chip.l2_callback_registered() && !chip.link_callback_registered());
    sw.clear_callbacks();
    assert!(!chip.l2_event(Some(MAC), 10, 2, 1));
}

#[test]
fn a_panicking_closure_does_not_unwind_into_the_glue() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\n");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    sw.set_link_callback(move |port, _| {
        counter.fetch_add(1, Ordering::Relaxed);
        assert!(port != 1, "a closure panicking on an SDK thread");
    })
    .unwrap();
    sw.set_l2_callback(|_| panic!("an L2 closure panicking")).unwrap();
    sw.set_port_gate(2, || panic!("a gate panicking")).unwrap();
    // from another thread, as the SDK's: the event's call returns normally
    let remote = chip.clone();
    std::thread::spawn(move || {
        assert!(remote.link_event(1, true));
        assert!(remote.l2_event(Some(MAC), 1, 1, 1));
        assert_eq!(remote.gate(2), Some(0)); // a panicking gate answers "down"
    })
    .join()
    .unwrap();
    assert!(chip.link_event(1, false)); // still registered, still called
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let stats = sw.callback_stats();
    assert_eq!((stats.link_panics, stats.l2_panics, stats.gate_panics), (2, 1, 1));
}

#[test]
fn port_gates_answer_the_link_scan() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 3 4 100000\nlinkscan 3 hw\n");
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    sw.set_link_callback(move |port, up| {
        let _ = tx.lock().unwrap().send((port, up));
    })
    .unwrap();
    let verdict = Arc::new(Mutex::new(Gate::Down));
    let v = Arc::clone(&verdict);
    let bus = sw.mdio_bus();
    sw.set_port_gate(1, move || {
        // what the ext-PHY gate does: MDIO through its own handle, then its verdict
        let _ = bus.read(0x24, D1, 1);
        *v.lock().unwrap()
    })
    .unwrap();
    assert_eq!(sw.gated_ports(), vec![1]);
    assert_eq!(sw.set_port_gate(3, || Gate::Up).map_err(code), Err(Code::Config)); // not SW link scan
    assert_eq!(sw.set_port_gate(-1, || Gate::Up).map_err(code), Err(Code::Port));
    assert_eq!(chip.gate(3), None);
    // the cable comes up: with a gate the link scan decides
    assert!(!chip.link_event(1, true));
    assert_eq!(chip.linkscan(), 0); // gate: down
    *verdict.lock().unwrap() = Gate::NoOpinion;
    assert_eq!(chip.linkscan(), 1); // the ASIC's link: up
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (1, true));
    *verdict.lock().unwrap() = Gate::Down;
    assert_eq!(chip.gate(1), Some(0));
    assert_eq!(chip.linkscan(), 1);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), (1, false));
    *verdict.lock().unwrap() = Gate::Up;
    assert_eq!(chip.gate(1), Some(1));
    sw.clear_port_gate(1).unwrap();
    sw.clear_port_gate(1).unwrap(); // none: no effect
    assert_eq!(chip.gate(1), None);
    assert_eq!(sw.gated_ports(), Vec::<i32>::new());
}

#[test]
fn stray_and_late_calls_find_nothing() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 2 4 100000\n");
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    sw.set_link_callback(move |_, _| {
        c.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();
    sw.set_port_gate(1, || Gate::Up).unwrap();
    let key = chip.callback_key();
    assert_ne!(key, 0);
    // other keys, out-of-range arguments: nothing runs
    Chip::stray_link(0, 1, 1);
    Chip::stray_link(key + 1_000_000, 1, 1);
    Chip::stray_link(key, -5, 1);
    Chip::stray_link(key, 1, 7);
    assert_eq!(Chip::stray_gate(key + 1_000_000, 1), -1);
    assert_eq!(Chip::stray_gate(key, 2), -1); // no gate on that port
    assert_eq!(Chip::stray_gate(key, 1), 1);
    Chip::stray_l2(key, None, 1, 1, 1); // no L2 handler: nothing
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(sw.callback_stats().rejected, 2);
    Chip::stray_link(key, 1, 1);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(sw);
    // the switch is gone: its key finds no registration any more
    Chip::stray_link(key, 1, 0);
    assert_eq!(Chip::stray_gate(key, 1), -1);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn dropping_the_switch_unregisters_and_shuts_down_in_order() {
    let chip = Chip::new();
    let trace = scratch().join("trace.txt");
    let mut sw = switch(&chip, &format!("ports 2 4 100000\ntrace {}\n", trace.display()));
    let owned = Arc::new(());
    let held = Arc::clone(&owned);
    sw.set_link_callback(move |_, _| {
        let _ = &held;
    })
    .unwrap();
    sw.set_port_gate(2, || Gate::NoOpinion).unwrap();
    assert_eq!(Arc::strong_count(&owned), 2);
    drop(sw);
    assert_eq!(Arc::strong_count(&owned), 1, "the closure goes with the switch");
    assert!(!chip.bcm_layer_up() && !chip.link_callback_registered());
    assert_eq!((chip.shutdown_calls(), chip.register_calls()), (1, 1));
    assert_eq!(chip.unregister_calls(), 1); // the one inside the shutdown (the C++ backend's order)
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        "init-all unit 0 rc -\nregister unit 0\ngate 2 on\ngate 2 off\nunregister unit 0\nshutdown unit 0\n"
    );
    // and again on the same chip
    let sw = switch(&chip, "ports 1 4 100000\n");
    assert_eq!(sw.ports().unwrap().len(), 1);
}

#[test]
fn a_refused_registration_leaves_nothing_registered() {
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 1 4 100000\nfail register 1 -16\nfail gate 1 -15\n");
    let owned = Arc::new(());
    let held = Arc::clone(&owned);
    let r = sw.set_link_callback(move |_, _| {
        let _ = &held;
    });
    assert_eq!(r.map_err(code), Err(Code::Unavail));
    assert_eq!(Arc::strong_count(&owned), 1, "the closure was dropped");
    assert!(!chip.link_callback_registered());
    assert_eq!(sw.set_port_gate(1, || Gate::Up).map_err(code), Err(Code::Config));
    assert_eq!(chip.gate(1), None);
    // the second attempts are not scripted to fail
    sw.set_link_callback(|_, _| {}).unwrap();
    sw.set_port_gate(1, || Gate::Up).unwrap();
    assert_eq!(chip.gate(1), Some(1));
}

/// Every operation the fake can be scripted to fail, through its wrapper: the code arrives.
#[test]
fn every_scripted_fault_reaches_the_caller() {
    type Call = fn(&mut Switch, &Ctx) -> Result<(), Error>;
    struct Ctx {
        tid: u32,
        intf: u32,
        nh: u32,
        ecmp: u32,
        gid: u32,
        entry: FpEntry,
        pid: u32,
        netif: sdk_glue::Netif,
    }
    let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    let _ = v4;
    let calls: Vec<(&str, Call)> = vec![
        ("register", |sw, _| sw.set_link_callback(|_, _| {})),
        ("hw-info", |sw, _| sw.hw_info().map(|_| ())),
        ("ports", |sw, _| sw.ports().map(|_| ())),
        ("enable", |sw, _| sw.port_enable_set(1, true)),
        ("link-get", |sw, _| sw.port_link_get(1).map(|_| ())),
        ("speed-set", |sw, _| sw.port_speed_set(1, 100_000)),
        ("speed-get", |sw, _| sw.port_speed_get(1).map(|_| ())),
        ("speeds", |sw, _| sw.port_speeds(1).map(|_| ())),
        ("fec", |sw, _| sw.port_fec_set(1, Fec::Rs)),
        ("autoneg", |sw, _| sw.port_autoneg_set(1, true)),
        ("interface", |sw, _| sw.port_interface_set_name(1, "cr4")),
        ("ifg", |sw, _| sw.port_ifg_set(1, 96)),
        ("vlan-create", |sw, _| sw.vlan_create(30)),
        ("vlan-destroy", |sw, _| sw.vlan_destroy(20)),
        ("vlan-add", |sw, _| sw.vlan_port_add(20, 1, true)),
        ("vlan-remove", |sw, _| sw.vlan_port_remove(20, 1)),
        ("vlan-members", |sw, _| sw.vlan_members(20).map(|_| ())),
        ("frame-max", |sw, _| sw.port_frame_max_set(1, 9100)),
        ("autoneg-get", |sw, _| sw.port_autoneg_get(1).map(|_| ())),
        ("loopback", |sw, _| sw.port_loopback_set(1, Loopback::Mac)),
        ("pvid", |sw, _| sw.port_pvid_set(1, 20)),
        ("learn", |sw, _| sw.port_learn_set(1, Learn::Cpu)),
        ("stat-get", |sw, _| sw.port_stat_get(1, PortStat::InErrors).map(|_| ())),
        ("stat-clear", |sw, _| sw.port_stat_clear(1)),
        ("l2-add", |sw, _| sw.l2_add(MAC, 20, Dest::Port(1), false, false)),
        ("l2-delete", |sw, _| sw.l2_delete(MAC, 20)),
        ("l2-flush", |sw, _| sw.l2_flush(None, None, false, true)),
        ("l2-age", |sw, _| sw.l2_age_set(10)),
        ("knet-cleanup", |sw, _| sw.knet_cleanup().map(|_| ())),
        ("netif-create", |sw, _| {
            sw.netif_create("e1", 1, 1, MAC, false).map(|_| ())
        }),
        ("netif-destroy", |sw, c| sw.netif_destroy(Some(c.netif.netif), None)),
        ("netif-filter", |sw, c| {
            sw.netif_filter_set(c.netif.netif, 1, false, None).map(|_| ())
        }),
        ("trap", |sw, _| sw.trap_set(TrapKind::ArpRequest, true)),
        ("trunk-create", |sw, _| sw.trunk_create().map(|_| ())),
        ("trunk-destroy", |sw, c| sw.trunk_destroy(c.tid)),
        ("trunk-set", |sw, c| sw.trunk_set(c.tid, &[])),
        ("intf-create", |sw, _| sw.l3_intf_create(MAC, 30, 1500, 0).map(|_| ())),
        ("intf-update", |sw, c| sw.l3_intf_update(c.intf, MAC, 20, 1500, 0)),
        ("intf-destroy", |sw, c| sw.l3_intf_destroy(c.intf)),
        ("station-add", |sw, _| sw.l3_station_add(MAC).map(|_| ())),
        ("station-delete", |sw, _| sw.l3_station_delete(1)),
        ("egress-create", |sw, c| {
            sw.l3_egress_create(c.intf, MAC, Dest::Port(1), 20).map(|_| ())
        }),
        ("egress-update", |sw, c| {
            sw.l3_egress_update(c.nh, c.intf, MAC, Dest::Port(1), 20)
        }),
        ("egress-special", |sw, _| sw.l3_egress_cpu().map(|_| ())),
        ("egress-destroy", |sw, c| sw.l3_egress_destroy(c.nh)),
        ("host-add", |sw, c| {
            sw.l3_host_add(0, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), c.nh)
        }),
        ("host-delete", |sw, _| {
            sw.l3_host_delete(0, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
        }),
        ("route-add", |sw, c| {
            sw.l3_route_add(
                0,
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                RouteTarget::Egress(c.nh),
                false,
                false,
            )
        }),
        ("route-delete", |sw, _| {
            sw.l3_route_delete(0, IpAddr::V4(Ipv4Addr::UNSPECIFIED), IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        }),
        ("ecmp-set", |sw, c| sw.l3_ecmp_update(c.ecmp, &[c.nh])),
        ("ecmp-destroy", |sw, c| sw.l3_ecmp_destroy(c.ecmp)),
        ("fp-group-create", |sw, _| {
            sw.fp_group_create(&[FpQual::Ttl], 99).map(|_| ())
        }),
        ("fp-group-destroy", |sw, c| sw.fp_group_destroy(c.gid)),
        ("fp-entry-create", |sw, c| {
            sw.fp_entry_create(c.gid, &FpRule::default()).map(|_| ())
        }),
        ("fp-entry-destroy", |sw, c| sw.fp_entry_destroy(c.entry)),
        ("fp-stat", |sw, c| sw.fp_stat_get(c.entry.stat.unwrap_or(0)).map(|_| ())),
        ("policer-create", |sw, _| {
            sw.policer_create(&PolicerCfg::default()).map(|_| ())
        }),
        ("policer-set", |sw, c| sw.policer_set(c.pid, &PolicerCfg::default())),
        ("policer-destroy", |sw, c| sw.policer_destroy(c.pid)),
        ("table-usage", |sw, _| sw.table_usage().map(|_| ())),
        ("chip-limits", |sw, _| sw.chip_limits().map(|_| ())),
        ("shell", |sw, _| sw.run_shell("ps")),
        ("gate", |sw, _| sw.set_port_gate(1, || Gate::Up)),
    ];
    let named: Vec<&str> = calls.iter().map(|(op, _)| *op).collect();
    for op in OPS {
        // the attach-only power-down is Attached's (tested with it)
        assert!(named.contains(op) || *op == "overtemp", "no wrapper exercised for {op}");
    }
    for (op, call) in calls {
        let chip = Chip::new();
        let mut sw = switch(&chip, "ports 2 4 100000\n");
        sw.vlan_create(20).unwrap();
        let tid = sw.trunk_create().unwrap();
        let intf = sw.l3_intf_create(MAC, 20, 1500, 0).unwrap();
        let nh = sw.l3_egress_create(intf, MAC, Dest::Port(1), 20).unwrap();
        let ecmp = sw.l3_ecmp_create(&[nh]).unwrap();
        let gid = sw.fp_group_create(&[FpQual::InPorts], 1).unwrap();
        let busy = sw.fp_group_create(&[FpQual::InPorts], 2).unwrap();
        let entry = sw
            .fp_entry_create(
                busy,
                &FpRule {
                    counter: true,
                    ..FpRule::default()
                },
            )
            .unwrap();
        let pid = sw.policer_create(&PolicerCfg::default()).unwrap();
        let netif = sw.netif_create("e0", 1, 1, MAC, false).unwrap();
        let ctx = Ctx {
            tid,
            intf,
            nh,
            ecmp,
            gid,
            entry,
            pid,
            netif,
        };
        assert!(chip.fail(op, 1, -11), "{op}");
        assert_eq!(call(&mut sw, &ctx).map_err(code), Err(Code::Fail), "{op}");
    }
    // the scripted n-th call, not the first
    let chip = Chip::new();
    let mut sw = switch(&chip, "ports 1 4 100000\nfail enable 2 -9\n");
    sw.port_enable_set(1, true).unwrap();
    assert_eq!(sw.port_enable_set(1, true).map_err(code), Err(Code::Timeout));
    sw.port_enable_set(1, true).unwrap();
    assert!(!chip.fail("bogus", 1, -11) && !chip.fail("enable", 0, -11) && !chip.fail("enable", 1, 0));
}

#[test]
fn the_message_table_and_codes() {
    assert_eq!(Error::from_raw(0), None);
    for (raw, c, text) in [
        (-1, Code::Internal, "Internal error"),
        (-4, Code::Param, "Invalid parameter"),
        (-16, Code::Unavail, "Feature unavailable"),
        (-18, Code::Port, "Invalid port"),
        (-100, Code::MdioClock, "MDIO clock setup not as configured"),
        (-57, Code::Other, "Unknown error"),
    ] {
        let e = Error::from_raw(raw).unwrap();
        assert_eq!((e.code(), sdk_glue::error::sdk_text(raw)), (c, text));
    }
}

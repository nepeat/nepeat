//! The external PHY runtime through the SAI core (`sai-openbcm/tests/test_extphy.inc`, every
//! check that goes through the SAI API), over the virtual backend with the C++ fake's port
//! numbering, on a manual clock; the backend call log of the C++ oracle (`frontend_driver` on the
//! same script) for switch creation, admin up / down, link scan and removal; failure injection.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::doc_markdown,
    missing_docs
)]
use sai_core::Error;
use sai_core::backend::types::{MdioDev, PhyAddr};
use sai_core::backend::{BackendEvent, HwPort, Mode, Profile, Recording, Shell};
use sai_core::extphy::{self, ManualClock};
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

const PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_PORT;
const SWITCH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SWITCH;

/// `kExtPhyGood` of `tests/test_tables.h`.
const GOOD: &str = "ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\n\
    port 1\nlink\n  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n\
    C 10 1 a101 3 3\nadmin-down\n  W 10 1 d139 351\nadmin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\n\
    port 5\nlink\n  C 11 1 a101 3 3\n";

fn table_file(text: &str) -> String {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, Into::into);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("sai-core-extphy-{}-{n}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

fn profile(table: Option<&str>) -> Profile {
    let mut pairs = vec![
        ("SAI_OPENBCM_BACKEND".to_owned(), "fake".to_owned()),
        ("SAI_OPENBCM_FAKE_PORTS".to_owned(), "4".to_owned()),
        ("SAI_OPENBCM_FAKE_LANES".to_owned(), "4".to_owned()),
        ("SAI_OPENBCM_FAKE_SPEED".to_owned(), "100000".to_owned()),
    ];
    if let Some(t) = table {
        pairs.push(("SAI_OPENBCM_EXT_PHY_FILE".to_owned(), t.to_owned()));
    }
    Profile::from_pairs(pairs)
}

type Core = Sai<Recording<VirtualBackend>>;

struct Rig {
    sai: Core,
    events: Receiver<BackendEvent>,
    clock: ManualClock,
    ports: Vec<RawOid>,
}

fn create(sai: &mut Core, profile: &Profile) -> sai_core::Result<RawOid> {
    sai.create_switch(
        &[Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))],
        profile,
    )
    .map(TypedId::raw)
}

fn rig(table: &str) -> Rig {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    let switch = create(&mut sai, &profile(Some(table))).unwrap();
    let clock = ManualClock::new();
    extphy::set_clock(&sai, Arc::new(clock.clone()));
    let events = sai.take_events().unwrap();
    let ports = match sai
        .get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
        .unwrap()
        .remove(0)
    {
        Value::OidList(l) => l,
        other => panic!("{other:?}"),
    };
    Rig {
        sai,
        events,
        clock,
        ports,
    }
}

impl Rig {
    /// The notifications of the events the backend raised so far (the notifier's work).
    fn notes(&mut self) -> Vec<Notification> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.extend(self.sai.handle_event(event));
        }
        out
    }
    fn up_down(&mut self, port: RawOid) -> (usize, usize) {
        let notes = self.notes();
        let n = |want: bool| {
            notes
                .iter()
                .filter(|n| matches!(n, Notification::PortOperStatus { port: p, up } if p.raw() == port && *up == want))
                .count()
        };
        (n(true), n(false))
    }
    fn quiet(&mut self) -> bool {
        self.notes().is_empty()
    }
    fn oper(&self, port: RawOid) -> bool {
        self.sai.get(PORT, port, &[sys::SAI_PORT_ATTR_OPER_STATUS]).unwrap()[0]
            == Value::Enum(sys::SAI_PORT_OPER_STATUS_UP.cast_signed())
    }
    fn admin(&mut self, port: RawOid, up: bool) {
        self.sai
            .set(PORT, port, &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up)))
            .unwrap();
    }
    fn reg(&self, phy: u32, reg: u16, v: u16) {
        let mdio = self.sai.backend().inner().mdio();
        mdio.lock()
            .unwrap()
            .regs
            .insert((PhyAddr(phy), MdioDev::new(1).unwrap(), reg), v);
    }
    fn dead(&self, phy: u32, dead: bool) {
        let mdio = self.sai.backend().inner().mdio();
        let mut m = mdio.lock().unwrap();
        if dead {
            m.dead.insert(PhyAddr(phy));
        } else {
            m.dead.remove(&PhyAddr(phy));
        }
    }
    fn writes(&self, phy: u32, reg: u16, val: u16) -> usize {
        let mdio = self.sai.backend().inner().mdio();
        let m = mdio.lock().unwrap();
        m.log
            .iter()
            .filter(|o| o.write && o.phy == PhyAddr(phy) && o.reg == reg && o.val == val)
            .count()
    }
    fn mdio_ops(&self) -> usize {
        self.sai.backend().inner().mdio().lock().unwrap().log.len()
    }
    fn linkscan(&mut self) {
        self.sai.backend_mut().inner_mut().linkscan();
    }
    fn status(&self) -> String {
        extphy::ext_phy_status(&self.sai)
    }
}

#[test]
fn a_bad_table_refuses_the_switch_before_the_backend_starts() {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    assert_eq!(
        create(&mut sai, &profile(Some("/nonexistent/ext-phy.tbl"))),
        Err(Error::Failure)
    );
    assert!(sai.switch_id().is_none());
    let bad = table_file("ext-phy 1\nport 1\n");
    assert_eq!(create(&mut sai, &profile(Some(&bad))), Err(Error::InvalidParameter));
    // lane 3 is inside port 0, not its first
    let lane = table_file("ext-phy 1\nport 3\nlink\nC 10 1 a101 3 3\n");
    assert_eq!(create(&mut sai, &profile(Some(&lane))), Err(Error::InvalidParameter));
    assert!(sai.switch_id().is_none() && !extphy::ext_phy_active(&sai));
    // the two load failures never reached the backend; the lane one shut it down again
    let log = sai.backend().log().lines();
    assert_eq!(log.iter().filter(|l| l.starts_with("init(")).count(), 1, "{log:#?}");
    assert_eq!(log.last().map(String::as_str), Some("shutdown() -> void"));
    // a backend without MDIO for the gates: the switch is refused (NOT_SUPPORTED), gates removed
    let good = table_file(GOOD);
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    sai.backend_mut()
        .inner_mut()
        .fail_nth("mdio_c45_read", 1, Error::NotSupported)
        .unwrap();
    assert_eq!(create(&mut sai, &profile(Some(&good))), Err(Error::NotSupported));
    let log = sai.backend().log().lines();
    assert!(
        log.contains(&"mdio_c45_read(phy=0, dev=1, reg=0) -> NOT_SUPPORTED".to_owned()),
        "{log:#?}"
    );
    assert!(log.contains(&"port_link_gate(hw=1, gate=empty) -> SUCCESS".to_owned()));
    // a backend that cannot gate the second port: refused with its status, every gate removed
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    sai.backend_mut()
        .inner_mut()
        .fail_nth("port_link_gate", 2, Error::NotSupported)
        .unwrap();
    assert_eq!(create(&mut sai, &profile(Some(&good))), Err(Error::NotSupported));
    assert!(sai.backend().inner().gated_ports().is_empty());
    for f in [bad, lane, good] {
        std::fs::remove_file(f).unwrap();
    }
}

#[test]
fn link_is_the_asic_link_and_the_line_side() {
    let table = table_file(GOOD);
    let mut r = rig(&table);
    std::fs::remove_file(&table).unwrap();
    assert!(extphy::ext_phy_active(&r.sai));
    assert_eq!(r.ports.len(), 4);
    let (p0, p1, p2) = (r.ports[0], r.ports[1], r.ports[2]);
    // creation disables every port: port 0's admin-down sequence ran once
    assert_eq!(r.writes(0x10, 0xd139, 0x351), 1);
    assert_eq!(r.sai.backend().inner().gated_ports(), [HwPort(1), HwPort(5)]);

    // a port the table does not list: link follows the ASIC
    r.admin(p2, true);
    assert_eq!(r.up_down(p2), (1, 0));
    assert!(r.oper(p2));

    // port 0, line side down: admin-up ran, the ASIC link is up, the port stays down
    r.reg(0x10, 0x8221, 0x11e3);
    r.reg(0x10, 0xa100, 2);
    r.reg(0x10, 0xa101, 0);
    r.reg(0x10, 0xd0e1, 0xa088); // the E masks bit 7: passes
    r.admin(p0, true);
    assert_eq!(r.writes(0x10, 0xd139, 0x350), 1);
    assert!(r.quiet());
    assert!(!r.oper(p0));
    for _ in 0..5 {
        r.linkscan();
    }
    assert!(r.quiet() && !r.oper(p0));
    // one good poll is not enough (up-polls 2), the second is
    r.reg(0x10, 0xa101, 3);
    r.linkscan();
    assert!(r.quiet() && !r.oper(p0));
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));
    assert!(r.oper(p0));
    r.linkscan();
    assert!(r.quiet());
    // one bad poll takes it down at once; one bit of the condition is enough
    r.reg(0x10, 0xa101, 1);
    r.linkscan();
    assert_eq!(r.up_down(p0), (0, 1));
    assert!(!r.oper(p0));
    // a flapping line never reaches two good polls in a row
    for i in 0..4 {
        r.reg(0x10, 0xa101, if i % 2 == 1 { 0 } else { 3 });
        r.linkscan();
    }
    assert!(r.quiet());
    r.reg(0x10, 0xa101, 3);
    r.linkscan();
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));
    // AND: line side up, ASIC link down -> down
    r.sai.backend_mut().inner_mut().inject_link(HwPort(1), false).unwrap();
    assert_eq!(r.up_down(p0), (0, 1));
    r.linkscan(); // the ASIC link is back (the fake follows admin), the line side still good
    assert_eq!(r.up_down(p0), (1, 0));
    // fault: the PHY stops answering -> down at once, back after two good polls
    r.dead(0x10, true);
    r.linkscan();
    assert_eq!(r.up_down(p0), (0, 1));
    assert!(r.status().contains("FAULT"));
    r.dead(0x10, false);
    r.linkscan();
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));
    assert!(!r.status().contains("FAULT"));
    // fault: the P never sees its value, or the E check fails
    r.reg(0x10, 0x8221, 0);
    r.linkscan();
    assert_eq!(r.up_down(p0), (0, 1));
    r.reg(0x10, 0x8221, 0x11e3);
    r.reg(0x10, 0xa100, 7);
    r.linkscan();
    r.linkscan();
    assert!(r.quiet() && !r.oper(p0));
    r.reg(0x10, 0xa100, 2);
    r.linkscan();
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));

    // hold-off: raw MDIO of the diag socket runs under the runtime's lock and pauses the polls
    let shells = r.sai.backend().inner().shell_log().len();
    extphy::ext_phy_foreign_mdio(&mut r.sai, |b| b.run_shell("phy raw c45 0x10 1 0x8b00")).unwrap();
    assert_eq!(r.sai.backend().inner().shell_log().len(), shells + 1);
    r.reg(0x10, 0xa101, 0);
    r.linkscan();
    assert!(r.oper(p0) && r.quiet()); // not polled: still up
    assert!(r.status().contains("PAUSED"));
    r.clock.advance_ms(80);
    r.linkscan();
    assert_eq!(r.up_down(p0), (0, 1));
    let text = r.status();
    assert!(text.starts_with("extphy: 2 ports"), "{text}");
    assert!(text.contains("ce0") && text.contains("ce1"));

    // admin down: the ASIC port first, then the admin-down sequence; no poll while down
    r.reg(0x10, 0xa101, 3);
    r.linkscan();
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));
    r.admin(p0, false);
    assert_eq!(r.writes(0x10, 0xd139, 0x351), 2);
    assert_eq!(r.up_down(p0), (0, 1));
    let ops = r.mdio_ops();
    r.linkscan();
    assert_eq!(r.mdio_ops(), ops);
    // admin up again starts from "down": two good polls (the first is the enable's own)
    r.admin(p0, true);
    assert!(r.quiet());
    r.linkscan();
    assert_eq!(r.up_down(p0), (1, 0));
    // an admin sequence that stops is logged and counted; the SAI call still succeeds
    r.admin(p0, false);
    assert_eq!(r.up_down(p0), (0, 1));
    r.reg(0x10, 0xd0e1, 0x1234);
    r.admin(p0, true);
    assert_eq!(r.writes(0x10, 0xd139, 0x350), 2); // the write after the failed E did not run
    assert_eq!(
        r.sai.get(PORT, p0, &[sys::SAI_PORT_ATTR_ADMIN_STATE]).unwrap()[0],
        Value::Bool(true)
    );
    let phy = extphy::ext_phy(&r.sai).unwrap();
    assert_eq!(phy.port_counters(sai_core::extphy::ChipPortNo(1)).unwrap().hook_fail, 1);

    // port 1: only a link condition
    r.reg(0x11, 0xa101, 3);
    r.admin(p1, true);
    assert!(r.quiet());
    r.linkscan();
    assert_eq!(r.up_down(p1), (1, 0));
    let switch = r.sai.switch_id().unwrap().raw();
    r.sai.remove(SWITCH, switch).unwrap();
    assert!(!extphy::ext_phy_active(&r.sai));
    assert!(r.sai.backend().inner().gated_ports().is_empty());
}

#[test]
fn between_polls_the_cached_state_answers() {
    let table = table_file("ext-phy 1\npoll-ms 60000\nup-polls 1\nport 1\nlink\nC 10 1 a101 3 3\n");
    let mut r = rig(&table);
    std::fs::remove_file(&table).unwrap();
    let p0 = r.ports[0];
    r.notes();
    r.reg(0x10, 0xa101, 3);
    r.admin(p0, true); // up-polls 1: the enable's own poll is enough
    assert_eq!(r.up_down(p0), (1, 0));
    r.reg(0x10, 0xa101, 0);
    r.linkscan();
    assert!(r.quiet() && r.oper(p0));
    r.clock.advance_ms(60_000);
    r.linkscan();
    assert_eq!(r.up_down(p0), (0, 1));
}

#[test]
fn without_a_table_nothing_is_registered() {
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    create(&mut sai, &profile(None)).unwrap();
    assert!(!extphy::ext_phy_active(&sai));
    assert_eq!(sai.backend().inner().mdio().lock().unwrap().log.len(), 0);
    assert!(sai.backend().inner().gated_ports().is_empty());
    assert!(extphy::ext_phy_status(&sai).contains("no external PHY table"));
}

/// The C++ oracle (`frontend_driver` on the same script: `profile` 4 ports + the good table,
/// `switch`, the registers, admin up, two link scans, OPER_STATUS, admin down, `remove_switch`)
/// wrote these call-log lines. The Rust core must write the same, line for line.
#[test]
fn the_call_log_is_the_cpp_oracles() {
    let table = table_file(GOOD);
    let mut sai = Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal));
    let switch = create(&mut sai, &profile(Some(&table))).unwrap();
    std::fs::remove_file(&table).unwrap();
    let events = sai.take_events().unwrap();
    let log = sai.backend().log();
    let create_lines = log.take();
    let ports = "[{hw=1,lanes=[1,2,3,4],speed=100000,name=\"ce0\",autoneg=false},{hw=5,lanes=[5,6,7,8],speed=100000,name=\"ce1\",autoneg=false},{hw=9,lanes=[9,10,11,12],speed=100000,name=\"ce2\",autoneg=false},{hw=13,lanes=[13,14,15,16],speed=100000,name=\"ce3\",autoneg=false}]";
    // after init: the ports (ext_phy_attach asks first), the MDIO probe, the two gates, then the
    // switch's own ports() and the admin-down of every port with port 0's sequence after its disable
    let after_init: Vec<&str> = create_lines
        .iter()
        .skip_while(|l| !l.starts_with("init("))
        .skip(1)
        .take(8)
        .map(String::as_str)
        .collect();
    assert_eq!(
        after_init,
        [
            &format!("ports() -> {ports}"),
            "mdio_c45_read(phy=0, dev=1, reg=0) -> SUCCESS val=0",
            "port_link_gate(hw=1, gate=set) -> SUCCESS",
            "port_link_gate(hw=5, gate=set) -> SUCCESS",
            &format!("ports() -> {ports}"),
            "port_admin_set(hw=1, up=false) -> SUCCESS",
            "mdio_c45_write(phy=16, dev=1, reg=53561, val=849) -> SUCCESS",
            "port_link_get(hw=1) -> SUCCESS up=false",
        ]
        .map(|s| s as &str),
        "{create_lines:#?}"
    );
    let mdio = sai.backend().inner().mdio();
    for (reg, v) in [(0x8221, 0x11e3), (0xa100, 2), (0xa101, 3), (0xd0e1, 0xa088)] {
        mdio.lock()
            .unwrap()
            .regs
            .insert((PhyAddr(0x10), MdioDev::new(1).unwrap(), reg), v);
    }
    let ports_list = match sai
        .get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
        .unwrap()
        .remove(0)
    {
        Value::OidList(l) => l,
        other => panic!("{other:?}"),
    };
    let poll = [
        "mdio_c45_read(phy=16, dev=1, reg=33313) -> SUCCESS val=4579",
        "mdio_c45_write(phy=16, dev=1, reg=40962, val=49505) -> SUCCESS",
        "mdio_c45_write(phy=16, dev=1, reg=33314, val=4369) -> SUCCESS",
        "mdio_c45_read(phy=16, dev=1, reg=33313) -> SUCCESS val=4579",
        "mdio_c45_read(phy=16, dev=1, reg=41216) -> SUCCESS val=2",
        "mdio_c45_read(phy=16, dev=1, reg=41217) -> SUCCESS val=3",
    ];
    log.take();
    sai.set(
        PORT,
        ports_list[0],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)),
    )
    .unwrap();
    let mut want: Vec<&str> = vec![
        "mdio_c45_read(phy=16, dev=1, reg=53473) -> SUCCESS val=41096",
        "mdio_c45_write(phy=16, dev=1, reg=53561, val=848) -> SUCCESS",
    ];
    want.extend(poll);
    want.push("port_admin_set(hw=1, up=true) -> SUCCESS");
    assert_eq!(log.take(), want);
    sai.backend_mut().inner_mut().linkscan();
    let mut want: Vec<&str> = poll.to_vec();
    want.push("event link(hw=1, up=true)");
    assert_eq!(log.take(), want);
    sai.backend_mut().inner_mut().linkscan();
    assert_eq!(log.take(), poll);
    while let Ok(e) = events.try_recv() {
        sai.handle_event(e);
    }
    sai.set(
        PORT,
        ports_list[0],
        &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(false)),
    )
    .unwrap();
    assert_eq!(
        log.take(),
        [
            "event link(hw=1, up=false)",
            "port_admin_set(hw=1, up=false) -> SUCCESS",
            "mdio_c45_write(phy=16, dev=1, reg=53561, val=849) -> SUCCESS",
        ]
    );
    sai.remove(SWITCH, switch).unwrap();
    let removal = log.take();
    let gates: Vec<&String> = removal.iter().filter(|l| l.starts_with("port_link_gate")).collect();
    assert_eq!(
        gates,
        [
            "port_link_gate(hw=1, gate=empty) -> SUCCESS",
            "port_link_gate(hw=5, gate=empty) -> SUCCESS"
        ]
    );
    assert_eq!(removal.last().map(String::as_str), Some("shutdown() -> void"));
}

/// Failure rule: the admin sequence runs around a failed port_admin_set as the C++ does (admin-up
/// before it; after a failed enable the line side goes off again), and the model keeps its state.
#[test]
fn a_failed_enable_leaves_model_and_line_side_down() {
    let table = table_file(GOOD);
    let mut r = rig(&table);
    std::fs::remove_file(&table).unwrap();
    let p0 = r.ports[0];
    r.reg(0x10, 0xd0e1, 0xa008);
    r.sai
        .backend_mut()
        .inner_mut()
        .fail_nth("port_admin_set", 1, Error::Failure)
        .unwrap();
    let st = r
        .sai
        .set(PORT, p0, &Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true)));
    assert!(st.is_err());
    // admin-up ran (0x350) and, the enable having failed, admin-down after it (0x351, the second)
    assert_eq!(r.writes(0x10, 0xd139, 0x350), 1);
    assert_eq!(r.writes(0x10, 0xd139, 0x351), 2);
    assert_eq!(
        r.sai.get(PORT, p0, &[sys::SAI_PORT_ATTR_ADMIN_STATE]).unwrap()[0],
        Value::Bool(false)
    );
    let phy = extphy::ext_phy(&r.sai).unwrap();
    assert!(!phy.port_state(sai_core::extphy::ChipPortNo(1)).unwrap().admin);
    // and no poll for a port that is down
    let ops = r.mdio_ops();
    r.linkscan();
    assert_eq!(r.mdio_ops(), ops);
}

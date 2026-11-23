//! The runtime against a register file on a manual clock: the checks of the C++
//! `tests/test_extphy.inc` that are about the runtime itself (link = ASIC AND line side is the
//! backend's: `sai-core/tests/extphy.rs`), the status text and the counters.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

use sai_extphy::{
    Api, BusStatus, ChipPort, Dev, ExtPhy, Level, LoadError, Logger, MACSEC_POLL_DEFER_MAX_MS, MacsecPollCounts,
    ManualClock, NoPortAtLane, PhyBus, PortRef, Verdict,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

/// `kExtPhyGood` of `tests/test_tables.h`.
const GOOD: &str = "ext-phy 1\n# comment\npoll-ms 0\nup-polls 2\nholdoff-ms 50\n\
    port 1\nlink\n  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n\
    C 10 1 a101 3 3\nadmin-down\n  W 10 1 d139 351\nadmin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\n\
    port 5\nlink\n  C 11 1 a101 3 3\n";

#[derive(Default)]
struct Chip {
    regs: BTreeMap<(u16, u16), u16>,
    dead: BTreeSet<u16>,
    log: Vec<(char, u16, u16, u16)>,
}

#[derive(Clone, Default)]
struct Bus(Arc<Mutex<Chip>>);

impl Bus {
    fn set(&self, phy: u16, reg: u16, v: u16) {
        self.0.lock().unwrap().regs.insert((phy, reg), v);
    }
    fn dead(&self, phy: u16, dead: bool) {
        let mut c = self.0.lock().unwrap();
        if dead {
            c.dead.insert(phy);
        } else {
            c.dead.remove(&phy);
        }
    }
    fn writes(&self, phy: u16, reg: u16, v: u16) -> usize {
        let c = self.0.lock().unwrap();
        c.log.iter().filter(|o| **o == ('W', phy, reg, v)).count()
    }
    fn ops(&self) -> usize {
        self.0.lock().unwrap().log.len()
    }
}

impl PhyBus for Bus {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        assert_eq!(dev.get(), 1);
        let mut c = self.0.lock().unwrap();
        if c.dead.contains(&phy) {
            return Err(BusStatus(-1));
        }
        let v = c.regs.get(&(phy, reg)).copied().unwrap_or(0);
        c.log.push(('R', phy, reg, v));
        Ok(v)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        assert_eq!(dev.get(), 1);
        let mut c = self.0.lock().unwrap();
        if c.dead.contains(&phy) {
            return Err(BusStatus(-1));
        }
        c.regs.insert((phy, reg), value);
        c.log.push(('W', phy, reg, value));
        Ok(())
    }
}

type Lines = Arc<Mutex<Vec<(Api, Level, String)>>>;

fn logger() -> (Logger, Lines) {
    let lines: Lines = Arc::default();
    let l = Arc::clone(&lines);
    (
        Arc::new(move |api, level, text: &str| l.lock().unwrap().push((api, level, text.to_owned()))),
        lines,
    )
}

/// The C++ fake's four 100G ports: hw 1, 5, 9, 13 with four lanes each, `ce0`..
fn ports() -> Vec<PortRef> {
    (0..4u16)
        .map(|i| PortRef {
            hw: ChipPort(1 + 4 * i),
            first_lane: Some(1 + 4 * u32::from(i)),
            name: format!("ce{i}"),
        })
        .collect()
}

fn table_file(text: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, Into::into);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("sai-extphy-{}-{n}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

struct Rig {
    ext: ExtPhy,
    bus: Bus,
    clock: ManualClock,
    lines: Lines,
}

fn rig(text: &str) -> Rig {
    let clock = ManualClock::new();
    let (log, lines) = logger();
    let path = table_file(text);
    let ext = ExtPhy::load(path.to_str(), Arc::new(clock.clone()), log).unwrap();
    std::fs::remove_file(path).unwrap();
    let bus = Bus::default();
    Rig { ext, bus, clock, lines }
}

const P0: ChipPort = ChipPort(1);
const P1: ChipPort = ChipPort(5);

#[test]
fn a_table_that_cannot_be_read_or_parsed_or_placed_refuses_the_switch() {
    let (log, lines) = logger();
    let clock = Arc::new(ManualClock::new());
    let e = ExtPhy::load(Some("/nonexistent/ext-phy.tbl"), clock.clone(), log.clone()).unwrap_err();
    assert_eq!(e, LoadError::CannotOpen);
    assert_eq!(
        lines.lock().unwrap().last().unwrap().2,
        "SAI_OPENBCM_EXT_PHY_FILE /nonexistent/ext-phy.tbl: cannot open; refusing to create the switch"
    );
    let bad = table_file("ext-phy 1\nport 1\n");
    let e = ExtPhy::load(bad.to_str(), clock.clone(), log.clone()).unwrap_err();
    assert_eq!(e, LoadError::Parse("line 2: port 1 has no sequence".into()));
    std::fs::remove_file(bad).unwrap();
    // lane 3 is inside port 0 (lanes 1-4), not its first
    let r = rig("ext-phy 1\nport 3\nlink\nC 10 1 a101 3 3\n");
    assert_eq!(
        r.ext.attach(&ports(), Box::new(r.bus.clone())),
        Err(NoPortAtLane { line: 2, lane: 3 })
    );
    assert!(!r.ext.active());
    assert_eq!(
        r.lines.lock().unwrap().last().unwrap().2,
        "external PHY table line 2: no port starts at lane 3"
    );
    // no key: nothing
    let none = ExtPhy::load(None, clock, log).unwrap();
    assert!(!none.active() && !none.has_ports());
    assert_eq!(none.gate(P0), Verdict::NoOpinion);
    assert!(none.status().contains("no external PHY table"));
}

#[test]
#[allow(clippy::too_many_lines, reason = "one story, as the C++ test tells it")]
fn line_side_debounce_faults_admin_sequences_and_hold_off() {
    let Rig { ext, bus, clock, lines } = rig(GOOD);
    assert_eq!(ext.attach(&ports(), Box::new(bus.clone())), Ok(vec![P0, P1]));
    assert!(ext.active());
    assert_eq!(ext.gated_ports(), [P0, P1]);
    for p in ports() {
        ext.admin(p.hw, false); // switch creation disables every port
    }
    assert_eq!(bus.writes(0x10, 0xd139, 0x351), 1);
    // admin down: no opinion, no poll
    let ops = bus.ops();
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(bus.ops(), ops);
    // a port the table does not list
    assert_eq!(ext.gate(ChipPort(9)), Verdict::NoOpinion);

    // line side down: admin-up ran (its E masks bit 7), the port stays down
    bus.set(0x10, 0x8221, 0x11e3);
    bus.set(0x10, 0xa100, 2);
    bus.set(0x10, 0xa101, 0);
    bus.set(0x10, 0xd0e1, 0xa088);
    ext.admin(P0, true);
    assert_eq!(bus.writes(0x10, 0xd139, 0x350), 1);
    for _ in 0..5 {
        assert_eq!(ext.gate(P0), Verdict::Down);
    }
    // two good polls in a row
    bus.set(0x10, 0xa101, 3);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert!(
        lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.2 == "port ce0: line side up (external PHY, value 0x0003, 2 good polls)")
    );
    // one bad poll: down at once; one bit of the condition is enough
    bus.set(0x10, 0xa101, 1);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert!(
        lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.2 == "port ce0: line side down (external PHY, value 0x0001)")
    );
    // flapping never reaches two good polls
    for i in 0..4 {
        bus.set(0x10, 0xa101, if i % 2 == 1 { 0 } else { 3 });
        assert_eq!(ext.gate(P0), Verdict::Down);
    }
    bus.set(0x10, 0xa101, 3);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    // fault: an MDIO error
    bus.dead(0x10, true);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert!(ext.status().contains("FAULT"));
    assert!(lines.lock().unwrap().iter().any(|l| l.1 == Level::Warn
        && l.2 == "port ce0: external PHY link poll fault: line 8: R phy 0x10 1.8221: MDIO error -1"));
    bus.dead(0x10, false);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert!(!ext.status().contains("FAULT"));
    // fault: the P never sees its value
    bus.set(0x10, 0x8221, 0);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert!(lines.lock().unwrap().iter().any(|l| {
        l.2.ends_with("line 11: P phy 0x10 1.8221 never read 0x11e3 (mask 0xffff) in 3 reads, last 0x0000")
    }));
    // fault: the E check
    bus.set(0x10, 0x8221, 0x11e3);
    bus.set(0x10, 0xa100, 7);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::Down);
    bus.set(0x10, 0xa100, 2);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);

    // hold-off: the polls pause (state frozen) for holdoff-ms after the last raw MDIO
    let ran = ext.foreign_mdio(|| 7);
    assert_eq!(ran, 7);
    assert!(lines.lock().unwrap().iter().any(
        |l| l.2 == "external PHY link polls paused: raw MDIO on the diag socket (resume 50 ms after the last one)"
    ));
    bus.set(0x10, 0xa101, 0);
    let ops = bus.ops();
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(bus.ops(), ops);
    assert!(ext.status().contains("PAUSED now, 1 so far"));
    clock.advance_ms(49);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    clock.advance_ms(1);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert!(ext.status().contains("not paused, 1 so far"));
    // a new hold-off is counted; a second raw command inside it extends it without a second count
    ext.foreign_mdio(|| ());
    clock.advance_ms(40);
    ext.foreign_mdio(|| ());
    clock.advance_ms(40);
    assert!(ext.status().contains("PAUSED now, 2 so far"));
    clock.advance_ms(50);

    // admin down: then nothing is polled
    bus.set(0x10, 0xa101, 3);
    ext.admin(P0, false);
    assert_eq!(bus.writes(0x10, 0xd139, 0x351), 2);
    let ops = bus.ops();
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(bus.ops(), ops);
    // admin up starts from down again
    ext.admin(P0, true);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    // an admin sequence that stops is counted; the write after the failed E does not run
    ext.admin(P0, false);
    bus.set(0x10, 0xd0e1, 0x1234);
    let fails = ext.port_counters(P0).unwrap().hook_fail;
    ext.admin(P0, true);
    assert_eq!(bus.writes(0x10, 0xd139, 0x350), 2);
    assert_eq!(ext.port_counters(P0).unwrap().hook_fail, fails + 1);
    assert!(ext.port_state(P0).unwrap().admin);
    assert!(lines.lock().unwrap().iter().any(|l| l.1 == Level::Error
        && l.2
            == "port ce0: external PHY admin-up sequence stopped: line 17: E phy 0x10 1.d0e1 read 0x1234, expected 0xa008 (mask 0xff7f)"));

    // port 1: only a link condition
    bus.set(0x11, 0xa101, 3);
    ext.admin(P1, true);
    assert_eq!(ext.gate(P1), Verdict::Down);
    assert_eq!(ext.gate(P1), Verdict::NoOpinion);

    // detach: the gates to unregister, then nothing
    assert_eq!(ext.gated_ports(), [P0, P1]);
    ext.clear();
    assert!(!ext.active());
    assert_eq!(ext.gated_ports(), []);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
}

#[test]
fn between_polls_the_cached_state_answers() {
    let Rig { ext, bus, clock, .. } = rig("ext-phy 1\npoll-ms 60000\nup-polls 1\nport 1\nlink\nC 10 1 a101 3 3\n");
    assert_eq!(ext.attach(&ports(), Box::new(bus.clone())), Ok(vec![P0]));
    bus.set(0x10, 0xa101, 3);
    ext.admin(P0, true);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion); // up-polls 1
    bus.set(0x10, 0xa101, 0);
    let ops = bus.ops();
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(bus.ops(), ops);
    clock.advance_ms(59_999);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    clock.advance_ms(1);
    assert_eq!(ext.gate(P0), Verdict::Down);
    // an admin change polls at once again
    bus.set(0x10, 0xa101, 3);
    ext.admin(P0, false);
    ext.admin(P0, true);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
}

#[test]
fn the_status_text_is_the_cpp_table() {
    let Rig { ext, bus, clock, .. } = rig(GOOD);
    assert_eq!(ext.attach(&ports(), Box::new(bus.clone())), Ok(vec![P0, P1]));
    bus.set(0x10, 0x8221, 0x11e3);
    bus.set(0x10, 0xa100, 2);
    bus.set(0x10, 0xa101, 3);
    bus.set(0x10, 0xd0e1, 0xa008);
    ext.admin(P0, true);
    // a P with waits: the durations come from the clock
    bus.set(0x10, 0x8221, 0);
    clock.advance_ms(1);
    assert_eq!(ext.gate(P0), Verdict::Down);
    bus.set(0x10, 0x8221, 0x11e3);
    assert_eq!(ext.gate(P0), Verdict::Down);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    let c = ext.port_counters(P0).unwrap();
    assert_eq!(
        (c.polls, c.faults, c.ups, c.downs, c.hooks, c.hook_fail),
        (3, 1, 1, 0, 1, 0)
    );
    assert_eq!(
        ext.status(),
        "extphy: 2 ports, poll 0 ms, up after 2 good polls, hold-off 50 ms (not paused, 0 so far)\n\
         extphy: table parser and sequence executor: rust (phy-seq)\n\
         port      lane   hw admin line  fault  value    polls   faults  ups downs last_us  avg_us  max_us hooks fail hook_max_us\n\
         ce0          1    1 up    up    no     0x0003        3        1    1     0       0       0       0     1    0           0\n\
         ce1          5    5 down  down  no     0x0000        0        0    0     0       0       0       0     0    0           0\n"
    );
}

#[test]
fn the_gate_takes_only_the_runtimes_lock() {
    // a link-scan thread asks while another thread runs admin sequences and raw MDIO
    let Rig { ext, bus, .. } = rig(GOOD);
    ext.attach(&ports(), Box::new(bus.clone())).unwrap();
    bus.set(0x10, 0x8221, 0x11e3);
    bus.set(0x10, 0xa100, 2);
    bus.set(0x10, 0xa101, 3);
    bus.set(0x10, 0xd0e1, 0xa008);
    let scan = {
        let ext = ext.clone();
        std::thread::spawn(move || {
            for _ in 0..2000 {
                let _ = ext.gate(P0);
            }
        })
    };
    for i in 0..500 {
        ext.admin(P0, i % 2 == 0);
        ext.locked(|| ());
    }
    scan.join().unwrap();
    assert!(ext.port_counters(P0).unwrap().hooks >= 500);
}

/// A deliberate improvement on the C++: the raw-MDIO hold-off also defers the external `MACsec`
/// table's poll sequences (PN, counters), per sequence at most `MACSEC_POLL_DEFER_MAX_MS` in a
/// row, so a tool polling the PHY for minutes cannot hide a PN nearing exhaustion.
#[test]
fn the_hold_off_defers_macsec_polls_and_bounds_the_deferral() {
    const PN: &str = "pn.egress.0";
    const STATS: &str = "stats-sa.egress.0";
    let Rig { ext, bus, clock, lines } = rig(&GOOD.replace("holdoff-ms 50", "holdoff-ms 3000"));
    // not attached: no hold-off, nothing deferred
    assert!(!ext.macsec_poll_deferred(P0, PN));
    ext.foreign_mdio(|| ());
    assert!(!ext.macsec_poll_deferred(P0, PN));
    assert_eq!(ext.attach(&ports(), Box::new(bus.clone())), Ok(vec![P0, P1]));
    // no raw MDIO: polls run
    assert!(!ext.macsec_poll_deferred(P0, PN));
    // raw MDIO at t; a poll due at t+1 s is deferred (one notice per hold-off), runs after it
    ext.foreign_mdio(|| ());
    clock.advance_ms(1000);
    assert!(ext.macsec_poll_deferred(P0, PN));
    assert!(ext.macsec_poll_deferred(P0, STATS));
    assert!(ext.macsec_poll_deferred(P1, PN)); // global, as the link polls' hold-off
    let notice = "external MACsec PN / counter polls deferred: raw MDIO on the diag socket (last values answer until the hold-off ends, at most 10000 ms per poll)";
    let count = |text: &str| lines.lock().unwrap().iter().filter(|l| l.2.starts_with(text)).count();
    assert_eq!(count(notice), 1);
    clock.advance_ms(1999);
    assert!(ext.macsec_poll_deferred(P0, PN));
    clock.advance_ms(1); // t+3 s: the hold-off is over
    assert!(!ext.macsec_poll_deferred(P0, PN));
    assert!(!ext.macsec_poll_deferred(P0, STATS));
    assert_eq!(ext.macsec_poll_counts(), MacsecPollCounts { deferred: 4, forced: 0 });
    // a tool that polls continuously (raw MDIO every 500 ms) for five minutes, syncd asking
    // for the PN every second: the PN is still read at least every MACSEC_POLL_DEFER_MAX_MS
    let mut runs = Vec::new();
    for ms in (0..300_000u64).step_by(500) {
        ext.foreign_mdio(|| ());
        if ms % 1000 == 0 && !ext.macsec_poll_deferred(P0, PN) {
            runs.push(ms);
        }
        clock.advance_ms(500);
    }
    assert_eq!(runs.len(), 29, "{runs:?}");
    assert_eq!(runs[0], MACSEC_POLL_DEFER_MAX_MS);
    assert!(
        runs.windows(2).all(|w| w[1] - w[0] == MACSEC_POLL_DEFER_MAX_MS),
        "{runs:?}"
    );
    assert_eq!(count(notice), 2);
    let forced = "external MACsec poll pn.egress.0 on chip port 1 runs inside the raw-MDIO hold-off: deferred 10000 ms, the most allowed";
    assert_eq!(count(forced), 29);
    assert_eq!(ext.macsec_poll_counts().forced, 29);
    // the status text (the C++'s) does not change
    assert!(ext.status().contains("hold-off 3000 ms (PAUSED now, 2 so far)"));
}

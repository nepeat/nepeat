//! The MDIO arbitration (crate documentation, "Serialisation") against a stateful fake of the
//! PHYs' firmware-message interface that **detects interleaving** (`sai_extphy::fwphy`: a
//! foreign access between a conversation's request and its result breaks it, the ack reads 0 as
//! on the AS7772). A diag client's transaction is interleaved with every library user of the
//! bus (link poll, admin sequence, `MACsec` change, periodic PN / stats read, the start-up
//! clean-up and a counter clear are all `MacsecUse::Op` or `Poll` runs here), on the same PHY and
//! on another, with lease expiry, a client that dies mid-transaction, back-to-back transactions
//! for five minutes of clock, and real threads (link scan, SAI calls, diag socket) under a
//! watchdog for deadlock freedom. The lock-order rules (the runtime's mutexes are leaves) are
//! checked by debug assertions in every test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    missing_docs
)]

use sai_extphy::fwphy::{FwBus, FwPhy, as_actor};
use sai_extphy::{
    Api, ChipPort, Clock, DiagTarget, DiagWho, ExtPhy, LEASE_MAX_MS, LeaseScope, Level, Logger,
    MACSEC_POLL_DEFER_MAX_MS, MacsecUse, ManualClock, PortRef, Turn, TxnRefused, Verdict,
};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Port 1 polled through PHY 0x10's message interface, port 5 through PHY 0x11's; the admin
/// sequences are conversations too. Every poll due (`poll-ms 0`), up after one good poll.
fn table(holdoff: u32) -> String {
    let conv = |phy: &str, cond: bool| {
        format!(
            "  R {phy} 1 8221\n  W {phy} 1 a000 c161\n  W {phy} 1 8222 1111\n  P {phy} 1 8221 11e3 ffff 3 0\n  E {phy} 1 a100 2\n  {} {phy} 1 a101 3 3\n",
            if cond { "C" } else { "E" }
        )
    };
    format!(
        "ext-phy 1\npoll-ms 0\nup-polls 1\nholdoff-ms {holdoff}\nport 1\nlink\n{}admin-down\n{}admin-up\n{}port 5\nlink\n{}",
        conv("10", true),
        conv("10", false),
        conv("10", false),
        conv("11", true)
    )
}

type Lines = Arc<Mutex<Vec<(Api, Level, String)>>>;

struct Rig {
    ext: ExtPhy,
    fw: Arc<FwPhy>,
    lines: Lines,
}

const P0: ChipPort = ChipPort(1);
const P1: ChipPort = ChipPort(5);

fn ports() -> Vec<PortRef> {
    (0..2u16)
        .map(|i| PortRef {
            hw: ChipPort(1 + 4 * i),
            first_lane: Some(1 + 4 * u32::from(i)),
            name: format!("ce{i}"),
        })
        .collect()
}

fn rig_on(clock: Arc<dyn Clock>, holdoff: u32) -> Rig {
    let lines: Lines = Arc::default();
    let l = Arc::clone(&lines);
    let log: Logger = Arc::new(move |api, level, text: &str| l.lock().unwrap().push((api, level, text.to_owned())));
    let table = sai_extphy::ExtPhyTable::parse(table(holdoff).as_bytes()).unwrap();
    let ext = ExtPhy::with_table(table, clock, log);
    let fw = FwPhy::new();
    for phy in [0x10, 0x11] {
        fw.set(phy, 0xa100, 2);
        fw.set(phy, 0xa101, 3);
    }
    assert_eq!(ext.attach(&ports(), Box::new(FwBus(Arc::clone(&fw)))), Ok(vec![P0, P1]));
    ext.admin(P0, true);
    ext.admin(P1, true);
    assert_eq!(ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(ext.gate(P1), Verdict::NoOpinion);
    Rig { ext, fw, lines }
}

fn rig(holdoff: u32) -> (Rig, ManualClock) {
    let clock = ManualClock::new();
    (rig_on(Arc::new(clock.clone()), holdoff), clock)
}

/// One firmware-message conversation through `access` (register, value to write): what the
/// tables and the vendor tools do. Ok(true): the ack came.
fn conversation(
    access: &mut dyn FnMut(u16, Option<u16>) -> Result<u16, TxnRefused>,
    cmd: u16,
) -> Result<bool, TxnRefused> {
    access(0x8221, None)?; // a stale ack away
    access(0xa000, Some(0xc161))?;
    access(0x8222, Some(cmd))?;
    let want = (cmd & 0xff00) | 0x00e3;
    let mut acked = false;
    for _ in 0..3 {
        if access(0x8221, None)? == want {
            acked = true;
            break;
        }
    }
    access(0xa100, None)?;
    access(0xa101, None)?;
    Ok(acked)
}

/// The diag client's access to `phy` as `who` (one socket connection each).
fn diag(r: &Rig, who: DiagWho, phy: u16) -> impl FnMut(u16, Option<u16>) -> Result<u16, TxnRefused> + '_ {
    move |reg, value| {
        r.ext.diag_access(who, DiagTarget::Phy(phy), || {
            as_actor("diag", || r.fw.access(phy, 1, reg, value))
        })
    }
}

/// A library sequence (a `MACsec` table run) on `phy`: one conversation inside the run. Checks
/// the arbitration's invariant on the way: while a library run holds a PHY, no transaction does.
fn lib_run(r: &Rig, port: ChipPort, phy: u16, use_: MacsecUse<'_>) -> Turn<bool> {
    r.ext.macsec_run(port, &[phy], use_, || {
        let held = r.ext.leases().into_iter().any(|l| {
            l.txn.is_some()
                && match &l.scope {
                    LeaseScope::All => true,
                    LeaseScope::Phys(set) => set.contains(&phy),
                }
        });
        assert!(!held, "a library run on phy 0x{phy:x} inside a transaction");
        let mut access = |reg, value| Ok(r.fw.access(phy, 1, reg, value));
        conversation(&mut access, 0x1222).unwrap()
    })
}

fn begin(r: &Rig, phy: u16) -> u64 {
    r.ext
        .txn_begin(LeaseScope::Phys(BTreeSet::from([phy])), None)
        .unwrap()
        .txn
}

/// Waits until `cond` holds: a handshake on the runtime's own state (no fixed sleep); the
/// 60 s watchdog only turns a hang into a failure.
fn until(what: &str, cond: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(60), "never happened: {what}");
        std::thread::yield_now();
    }
}

fn count(lines: &Lines, starts: &str) -> usize {
    lines.lock().unwrap().iter().filter(|l| l.2.starts_with(starts)).count()
}

#[test]
fn a_transaction_holds_the_periodic_users_off_its_phy_and_only_its_phy() {
    let (r, _clock) = rig(3000);
    let t = begin(&r, 0x10);
    // the tool's conversation, half-way: request and doorbell written, ack not read yet
    let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
    tool(0x8221, None).unwrap();
    tool(0xa000, Some(0xc161)).unwrap();
    tool(0x8222, Some(0x1333)).unwrap();
    assert!(r.fw.open(0x10));
    let before = r.fw.accesses();
    // the link poll of that PHY's port: its last verdict, no bus access, counted
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    // the periodic MACsec reads of that PHY: skipped (the model answers its last read)
    assert_eq!(lib_run(&r, P0, 0x10, MacsecUse::Poll("pn.egress.0")), Turn::Skipped);
    assert_eq!(
        lib_run(&r, P0, 0x10, MacsecUse::Poll("stats-sa.egress.0")),
        Turn::Skipped
    );
    assert_eq!(
        lib_run(&r, P0, 0x10, MacsecUse::Poll("stats-flow.egress")),
        Turn::Skipped
    );
    // a diag-socket extmacsec run on it: refused, the transaction's client must end it first
    match lib_run(&r, P0, 0x10, MacsecUse::DiagRun) {
        Turn::Refused(why) => assert!(why.contains("diag transaction 1"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(r.fw.accesses(), before);
    // another PHY: everything runs, in the middle of the transaction
    assert_eq!(r.ext.gate(P1), Verdict::NoOpinion);
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Poll("pn.egress.0")), Turn::Ran(true));
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Op), Turn::Ran(true));
    r.ext.admin(P1, false); // its admin-down sequence runs at once (it has none: the state resets)
    r.ext.admin(P1, true);
    assert_eq!(r.ext.gate(P1), Verdict::NoOpinion);
    // the tool's conversation completes undisturbed; then the transaction ends
    assert_eq!(tool(0x8221, None).unwrap(), 0x13e3);
    tool(0xa100, None).unwrap();
    tool(0xa101, None).unwrap();
    let ended = r.ext.txn_end(t).unwrap();
    assert_eq!(ended.accesses, 6);
    // and the PHY's users run again
    assert_eq!(lib_run(&r, P0, 0x10, MacsecUse::Poll("pn.egress.0")), Turn::Ran(true));
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(r.fw.violations(), Vec::<String>::new());
    assert_eq!(r.fw.completed("diag"), 1);
    let c = r.ext.counts();
    assert_eq!(
        (
            c.txn_begun,
            c.txn_ended,
            c.gate_cached,
            c.polls_skipped,
            c.diag_run_refused
        ),
        (1, 1, 1, 3, 1)
    );
    let text = r.ext.status();
    assert!(text.contains("transactions begun 1, ended 1, expired 0"), "{text}");
    assert!(text.contains("MACsec polls skipped during a lease 3"), "{text}");
}

#[test]
fn sonic_changes_admin_sequences_and_clears_wait_for_the_transaction_never_dropped() {
    let (r, clock) = rig(3000);
    let r = Arc::new(r);
    let t = begin(&r, 0x10);
    let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
    tool(0xa000, Some(0xc161)).unwrap();
    let lib_before = r.fw.completed("lib");
    // the SAI thread: an SA install (Op); when told, a clear (Op) and the port's admin down, all on
    // PHY 0x10
    let (tx, rx) = mpsc::channel();
    let (go, gate) = mpsc::channel::<()>();
    let sai = {
        let r = Arc::clone(&r);
        std::thread::spawn(move || {
            tx.send(("install", lib_run(&r, P0, 0x10, MacsecUse::Op))).unwrap();
            gate.recv().unwrap();
            tx.send(("clear", lib_run(&r, P0, 0x10, MacsecUse::Op))).unwrap();
            r.ext.admin(P0, false);
            tx.send(("admin", Turn::Ran(true))).unwrap();
        })
    };
    // the install is queued behind the transaction (the arbiter counts it as waiting: it has not
    // run), and stays queued while the clock moves 900 ms (the lease is 1000)
    until("the install waits", || r.ext.waiting() == 1);
    assert!(rx.try_recv().is_err());
    clock.advance_ms(900);
    tool(0x8222, Some(0x1444)).unwrap();
    assert_eq!(r.ext.waiting(), 1);
    assert!(rx.try_recv().is_err());
    assert_eq!(tool(0x8221, None).unwrap(), 0x14e3);
    tool(0xa100, None).unwrap();
    tool(0xa101, None).unwrap();
    // a second transaction cannot slip in before the queued install: its begin returns only
    // once the install's conversation has run
    r.ext.txn_end(t).unwrap();
    let t2 = begin(&r, 0x10);
    assert_eq!(r.fw.completed("lib"), lib_before + 1);
    assert_eq!(rx.recv().unwrap(), ("install", Turn::Ran(true)));
    // the clear, asked for inside the second transaction, waits for its end in turn
    go.send(()).unwrap();
    until("the clear waits", || r.ext.waiting() == 1);
    assert!(rx.try_recv().is_err());
    clock.advance_ms(500);
    assert!(rx.try_recv().is_err());
    r.ext.txn_end(t2).unwrap();
    let got: Vec<_> = rx.iter().take(2).collect();
    assert_eq!(got, [("clear", Turn::Ran(true)), ("admin", Turn::Ran(true))]);
    sai.join().unwrap();
    assert_eq!(r.ext.waiting(), 0);
    assert_eq!(r.fw.violations(), Vec::<String>::new());
    let c = r.ext.counts();
    assert_eq!(c.op_waits, 2, "{c:?}");
    assert!(c.op_wait_us_max >= 900_000, "{c:?}");
    assert_eq!(count(&r.lines, "external MACsec sequence on chip port 1 waited "), 2);
}

#[test]
fn a_transaction_that_outlives_its_lease_is_void_and_says_so() {
    let (r, clock) = rig(3000);
    let info = r
        .ext
        .txn_begin(LeaseScope::Phys(BTreeSet::from([0x10])), Some(100))
        .unwrap();
    assert_eq!((info.txn, info.step_ms, info.max_left_ms), (1, 100, LEASE_MAX_MS));
    let mut tool = diag(&r, DiagWho::Txn(1), 0x10);
    tool(0x8221, None).unwrap();
    clock.advance_ms(99);
    tool(0x8221, None).unwrap(); // renews: 100 ms from now
    clock.advance_ms(100);
    let err = tool(0x8221, None).unwrap_err();
    assert_eq!(
        err,
        TxnRefused::Expired {
            txn: 1,
            lived_ms: 199,
            accesses: 2
        }
    );
    assert_eq!(
        err.to_string(),
        "extphy txn 1: expired (its lease ran out after 199 ms, 2 accesses): what the PHY did since is unknown; \
         begin a new transaction"
    );
    assert!(matches!(r.ext.txn_end(1), Err(TxnRefused::Expired { .. })));
    assert!(matches!(r.ext.txn_renew(1, None), Err(TxnRefused::Expired { .. })));
    assert_eq!(r.ext.txn_end(77), Err(TxnRefused::Unknown(77)));
    assert_eq!(
        count(
            &r.lines,
            "external PHY diag transaction 1 (phy 0x10) expired after 199 ms (2 accesses) without an end"
        ),
        1
    );
    // the hard maximum: renewed every 500 ms, the lease still ends LEASE_MAX_MS after its begin
    let t = begin(&r, 0x10);
    let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
    let mut at = 0;
    while at < LEASE_MAX_MS - 500 {
        tool(0x8221, None).unwrap();
        r.ext.txn_renew(t, Some(5000)).unwrap(); // asks for more than the most: clamped
        clock.advance_ms(500);
        at += 500;
    }
    tool(0x8221, None).unwrap();
    clock.advance_ms(500);
    assert!(matches!(tool(0x8221, None), Err(TxnRefused::Expired { lived_ms, .. }) if lived_ms == LEASE_MAX_MS));
    let c = r.ext.counts();
    assert_eq!((c.txn_begun, c.txn_expired, c.txn_ended), (2, 2, 0));
    assert_eq!(c.void_refused, 5);
}

#[test]
fn a_client_that_dies_mid_transaction_blocks_nobody_for_longer_than_its_lease() {
    let (r, clock) = rig(3000);
    let r = Arc::new(r);
    // (a) it dies between two conversations: the library waits at most the lease, then runs clean
    let t = begin(&r, 0x10);
    {
        let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
        assert!(conversation(&mut tool, 0x1555).unwrap());
    }
    let (tx, rx) = mpsc::channel();
    let sai = {
        let r = Arc::clone(&r);
        std::thread::spawn(move || {
            tx.send(lib_run(&r, P0, 0x10, MacsecUse::Op)).unwrap();
            r.ext.admin(P0, true);
            tx.send(Turn::Ran(true)).unwrap();
        })
    };
    until("the SAI call waits", || r.ext.waiting() == 1);
    assert!(rx.try_recv().is_err());
    // the gate never waits for it
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    clock.advance_ms(1000); // the default lease after its last access
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), Turn::Ran(true));
    // its expiry is in the log (whoever noticed it first wrote it)
    let _ = r.ext.gate(P1);
    assert_eq!(
        count(
            &r.lines,
            "external PHY diag transaction 1 (phy 0x10) expired after 1000 ms"
        ),
        1
    );
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), Turn::Ran(true));
    sai.join().unwrap();
    assert_eq!(r.fw.violations(), Vec::<String>::new());
    assert_eq!(r.ext.counts().txn_expired, 1);
    // (b) it dies in the middle of a conversation: the library still runs once the lease is
    // over; the PHY's message interface is then in an unknown state (here the fake records the
    // break): the hazard a transaction cannot remove, only bound
    let t = begin(&r, 0x10);
    diag(&r, DiagWho::Txn(t), 0x10)(0xa000, Some(0xc161)).unwrap();
    clock.advance_ms(1000);
    assert_eq!(lib_run(&r, P0, 0x10, MacsecUse::Op), Turn::Ran(false));
    assert!(
        r.fw.violations()
            .iter()
            .all(|v| v.contains("inside the conversation of diag"))
    );
    assert!(!r.fw.violations().is_empty());
}

#[test]
fn bare_accesses_and_begins_inside_another_clients_transaction_are_refused() {
    let (r, clock) = rig(3000);
    let t = begin(&r, 0x10);
    // a tool without a transaction
    let err = diag(&r, DiagWho::Bare, 0x10)(0x8221, None).unwrap_err();
    assert_eq!(
        err.to_string(),
        "phy 0x10 is in diag transaction 1 (for 1000 ms more unless it is renewed): send 'extphy txn 1 <command>' \
         as its holder, or wait"
    );
    // ... on another PHY it is served (and holds that PHY's polls off)
    diag(&r, DiagWho::Bare, 0x11)(0xa100, None).unwrap();
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Poll("pn.egress.0")), Turn::Skipped);
    // a second transaction on an overlapping set: busy; on all PHYs: busy; elsewhere: fine
    let busy = r
        .ext
        .txn_begin(LeaseScope::Phys(BTreeSet::from([0x10, 0x12])), None)
        .unwrap_err();
    assert_eq!(
        busy.to_string(),
        "extphy txn: phy 0x10,0x12: held by transaction 1 for at most 1000 ms more; try again"
    );
    assert!(matches!(
        r.ext.txn_begin(LeaseScope::All, None),
        Err(TxnRefused::Busy { txn: Some(1), .. })
    ));
    let t2 = begin(&r, 0x12);
    // a transaction's access outside its PHYs, and raw MDIO that names no single PHY
    assert!(matches!(
        diag(&r, DiagWho::Txn(t2), 0x10)(0x8221, None),
        Err(TxnRefused::OutOfScope { txn: 2, .. })
    ));
    let global = r
        .ext
        .diag_access(DiagWho::Txn(t2), DiagTarget::Global, || ())
        .unwrap_err();
    assert!(
        global.to_string().ends_with("it needs a transaction on all"),
        "{global}"
    );
    assert!(matches!(
        r.ext.diag_access(DiagWho::Bare, DiagTarget::Global, || ()),
        Err(TxnRefused::InTxn { txn: 1, .. })
    ));
    assert_eq!(
        r.ext.txn_begin(LeaseScope::Phys(BTreeSet::new()), None),
        Err(TxnRefused::NoPhys)
    );
    r.ext.txn_end(t).unwrap();
    r.ext.txn_end(t2).unwrap();
    // a transaction on all PHYs: raw MDIO without a single PHY is served inside it
    let all = r.ext.txn_begin(LeaseScope::All, Some(300)).unwrap().txn;
    assert_eq!(r.ext.diag_access(DiagWho::Txn(all), DiagTarget::Global, || 9), Ok(9));
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(r.ext.gate(P1), Verdict::NoOpinion);
    let text = r.ext.status();
    assert!(
        text.contains("extphy:   txn 3 all PHYs: 300 ms left (at most 2000 ms), 1 accesses"),
        "{text}"
    );
    assert!(text.contains("extphy:   raw MDIO hold-off phy 0x11: "), "{text}");
    clock.advance_ms(3000);
    assert!(r.ext.leases().is_empty());
    let c = r.ext.counts();
    assert_eq!((c.busy_refused, c.txn_expired), (2, 1));
    assert_eq!(r.fw.violations(), Vec::<String>::new());
}

#[test]
fn raw_mdio_naming_no_single_phy_holds_every_phy() {
    let (r, clock) = rig(3000);
    r.ext.diag_access(DiagWho::Bare, DiagTarget::Global, || ()).unwrap();
    assert_eq!(
        count(&r.lines, "external PHY all PHYs: link polls and MACsec polls paused"),
        1
    );
    let before = r.fw.accesses();
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(r.ext.gate(P1), Verdict::NoOpinion);
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Poll("pn.ingress.1")), Turn::Skipped);
    assert_eq!(r.fw.accesses(), before);
    // SONiC's changes pass an implicit lease (as in the C++)
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Op), Turn::Ran(true));
    clock.advance_ms(3000);
    assert_eq!(lib_run(&r, P1, 0x11, MacsecUse::Poll("pn.ingress.1")), Turn::Ran(true));
}

#[test]
fn back_to_back_transactions_for_five_minutes_the_pn_is_read_within_the_bound_never_inside_one() {
    const RUN_MS: u64 = 300_000;
    let (r, clock) = rig(3000);
    let r = Arc::new(r);
    // syncd's counter thread: one PN get per second of the injected clock; `decided`: the second
    // whose get has returned
    let decided = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(Mutex::new(Vec::<u64>::new()));
    let polls = Arc::new(AtomicU64::new(0));
    let sai = {
        let (r, clock, decided, done, reads, polls) = (
            Arc::clone(&r),
            clock.clone(),
            Arc::clone(&decided),
            Arc::clone(&done),
            Arc::clone(&reads),
            Arc::clone(&polls),
        );
        std::thread::spawn(move || {
            let mut second = 1u64;
            while !done.load(Ordering::SeqCst) {
                if clock.now_us() / 1000 < second * 1000 {
                    std::thread::yield_now();
                    continue;
                }
                polls.fetch_add(1, Ordering::SeqCst);
                let turn = r.ext.macsec_run(P0, &[0x10], MacsecUse::Poll("pn.egress.0"), || {
                    // never inside a transaction
                    assert!(
                        r.ext.leases().iter().all(|l| l.txn.is_none()),
                        "a PN read inside a transaction"
                    );
                    let mut access = |reg, value| Ok(r.fw.access(0x10, 1, reg, value));
                    assert!(conversation(&mut access, 0x1222).unwrap());
                    reads.lock().unwrap().push(clock.now_us() / 1000);
                });
                assert!(matches!(turn, Turn::Ran(()) | Turn::Skipped));
                decided.store(second, Ordering::SeqCst);
                second += 1;
            }
        })
    };
    // the tool: 200 ms transactions (four conversations, 50 ms each), the next one right after,
    // laid out so that every second of the clock begins in the middle of one; after each second
    // begins, the tool goes on only once syncd's get for it has returned or waits for the
    // transaction (lockstep on the runtime's state, no sleeps)
    clock.advance_ms(100);
    let mut txns = 0u64;
    let step = |ms: u64| {
        let before = clock.now_us() / 1000;
        clock.advance_ms(ms);
        let second = (before + ms) / 1000;
        if second > before / 1000 {
            until("syncd's get of this second", || {
                decided.load(Ordering::SeqCst) >= second || r.ext.waiting() == 1
            });
        }
    };
    while clock.now_us() / 1000 < RUN_MS {
        let t = begin(&r, 0x10);
        for _ in 0..4 {
            let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
            assert!(
                conversation(&mut tool, 0x1666).unwrap(),
                "the tool's conversation broke"
            );
            step(50);
        }
        r.ext.txn_end(t).unwrap();
        txns += 1;
    }
    let last = RUN_MS / 1000;
    until("the last get", || decided.load(Ordering::SeqCst) >= last);
    done.store(true, Ordering::SeqCst);
    sai.join().unwrap();
    let reads = reads.lock().unwrap().clone();
    let polls = polls.load(Ordering::SeqCst);
    assert_eq!(txns, (RUN_MS - 100) / 200 + 1);
    assert_eq!(r.fw.violations(), Vec::<String>::new());
    assert_eq!(r.fw.completed("diag"), txns * 4);
    // every get landed in a transaction: each read is a forced one, every other get skipped
    let c = r.ext.counts();
    assert_eq!(c.forced_reads, reads.len() as u64, "{c:?}");
    assert_eq!(c.polls_skipped + c.forced_reads, polls, "{c:?}");
    // the first read 10 s after the first skip (second 1), each next one within the bound (plus
    // syncd's one-second cadence and the rest of the transaction it waited for) of the one before
    let bound = MACSEC_POLL_DEFER_MAX_MS + 1000 + 200;
    assert!(reads.len() >= 25, "{reads:?}");
    assert!(reads[0] <= 1000 + bound, "{reads:?}");
    assert!(reads.windows(2).all(|w| w[1] - w[0] <= bound), "{reads:?}");
    assert!(c.forced_wait_us_max <= 200_000, "{c:?}");
    assert_eq!(c.txn_expired, 0);
}

/// A library run on PHY 0x10 that holds it until told to finish: 10 ms of its sequence's waits
/// before, 70 ms after (an SA install's 80 ms on the AS7772), on the injected clock.
fn slow_run(r: &Arc<Rig>, clock: &ManualClock) -> (std::thread::JoinHandle<Turn<()>>, mpsc::Sender<()>) {
    let (held, holding) = mpsc::channel();
    let (finish, finished) = mpsc::channel::<()>();
    let holder = {
        let (r, clock) = (Arc::clone(r), clock.clone());
        std::thread::spawn(move || {
            r.ext.macsec_run(P0, &[0x10], MacsecUse::Op, || {
                clock.wait_us(10_000);
                held.send(()).unwrap();
                finished.recv().unwrap();
                clock.wait_us(70_000);
            })
        })
    };
    holding.recv().unwrap();
    (holder, finish)
}

/// The SDK's link scan on a thread of its own: `passes` gate calls of port 0, their verdicts.
fn scan(r: &Arc<Rig>, passes: usize) -> std::thread::JoinHandle<Vec<Verdict>> {
    let r = Arc::clone(r);
    std::thread::spawn(move || (0..passes).map(|_| r.ext.gate(P0)).collect())
}

#[test]
fn the_gate_never_waits_behind_a_library_run_and_is_polled_after_the_bound() {
    use sai_extphy::GATE_BUSY_MAX;
    let clock = ManualClock::new();
    let r = Arc::new(rig_on(Arc::new(clock.clone()), 3000));
    let polls = || r.ext.port_counters(P0).unwrap().polls;
    let polls0 = polls();
    let (holder, finish) = slow_run(&r, &clock);
    // GATE_BUSY_MAX passes while the run holds the PHY: each returns at once with the cached
    // verdict (the run cannot progress until the test lets it: a gate that waited would never
    // return), the poll stays due, no bus access
    let before = r.fw.accesses();
    let passes = usize::try_from(GATE_BUSY_MAX).unwrap();
    let pass = scan(&r, passes);
    until("the passes return", || pass.is_finished());
    assert_eq!(pass.join().unwrap(), vec![Verdict::NoOpinion; passes]);
    assert_eq!((polls(), r.fw.accesses()), (polls0, before));
    let c = r.ext.counts();
    assert_eq!(
        (c.gate_busy, c.gate_waits, c.gate_cached),
        (u64::from(GATE_BUSY_MAX), 0, 0),
        "{c:?}"
    );
    // the next pass: starved, it waits for this one run, before every other library user
    let starved = scan(&r, 1);
    until("the starved gate waits", || r.ext.waiting() == 1);
    let other = {
        let r = Arc::clone(&r);
        std::thread::spawn(move || {
            r.ext.macsec_run(P1, &[0x10], MacsecUse::Op, || {
                // the gate went first
                assert_eq!(r.ext.port_counters(P0).unwrap().polls, polls0 + 1);
            })
        })
    };
    until("the other user waits too", || r.ext.waiting() == 2);
    assert!(!starved.is_finished());
    finish.send(()).unwrap();
    assert_eq!(holder.join().unwrap(), Turn::Ran(()));
    assert_eq!(starved.join().unwrap(), vec![Verdict::NoOpinion]);
    assert_eq!(other.join().unwrap(), Turn::Ran(()));
    assert_eq!(polls(), polls0 + 1);
    let c = r.ext.counts();
    assert_eq!(c.gate_waits, 1, "{c:?}");
    // it waited for the rest of the run: 70 ms of the injected clock
    assert!(
        c.gate_wait_us_max >= 70_000 && c.gate_wait_us_total == c.gate_wait_us_max,
        "{c:?}"
    );
    assert_eq!(c.gate_time_hist[3], 1, "{c:?}");
    assert!(c.gate_time_us_max >= 70_000, "{c:?}");
    // and the count starts again: the next busy pass answers from the cache
    let (holder, finish) = slow_run(&r, &clock);
    assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    assert_eq!(r.ext.counts().gate_busy, u64::from(GATE_BUSY_MAX) + 1);
    finish.send(()).unwrap();
    holder.join().unwrap();
    // a transaction: the gate answers its last verdict without waiting and without the bus
    let t = begin(&r, 0x10);
    let before = r.fw.accesses();
    for _ in 0..100 {
        assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
    }
    assert_eq!(r.fw.accesses(), before);
    assert_eq!(r.ext.waiting(), 0);
    r.ext.txn_end(t).unwrap();
    let c = r.ext.counts();
    assert_eq!((c.gate_cached, c.gate_waits), (100, 1));
    let text = r.ext.status();
    assert!(
        text.contains("behind a library run 4; waits for a library run before a poll 1 (after 3 busy in a row"),
        "{text}"
    );
    assert!(text.contains("extphy: link gate time per poll (us): <=500 "), "{text}");
    assert_eq!(r.fw.violations(), Vec::<String>::new());
}

#[test]
fn link_down_behind_a_library_run_is_seen_one_pass_later() {
    let clock = ManualClock::new();
    let r = Arc::new(rig_on(Arc::new(clock.clone()), 3000));
    // the far end goes dark: the link sequence's condition fails from now on
    r.fw.set(0x10, 0xa101, 0);
    let (holder, finish) = slow_run(&r, &clock);
    // the pass while the run holds the PHY: the last verdict (up), at once
    let pass = scan(&r, 1);
    until("the pass returns", || pass.is_finished());
    assert_eq!(pass.join().unwrap(), vec![Verdict::NoOpinion]);
    finish.send(()).unwrap();
    holder.join().unwrap();
    // the next pass polls (still due) and sees it
    assert_eq!(r.ext.gate(P0), Verdict::Down);
    let c = r.ext.port_counters(P0).unwrap();
    assert_eq!(c.downs, 1, "{c:?}");
    assert_eq!(r.ext.counts().gate_busy, 1);
}

/// Real threads: the SDK's link scan calls the gate of both ports every pass while the SAI
/// thread runs slow library sequences (80 ms of waits each, on the injected clock that only it
/// moves) back to back on PHY 0x10. Every pass returns, the gate never waits for longer than one
/// run (and only after [`sai_extphy::GATE_BUSY_MAX`] busy answers), port 1's poll on PHY 0x11 is
/// never held up, and port 0 is still polled.
#[test]
fn a_link_scan_beside_back_to_back_slow_runs_never_stalls() {
    const PASSES: u64 = 200;
    let clock = ManualClock::new();
    let r = Arc::new(rig_on(Arc::new(clock.clone()), 3000));
    let stop = Arc::new(AtomicBool::new(false));
    let runs = Arc::new(AtomicU64::new(0));
    let sai = {
        let (r, clock, stop, runs) = (Arc::clone(&r), clock.clone(), Arc::clone(&stop), Arc::clone(&runs));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let turn = r.ext.macsec_run(P0, &[0x10], MacsecUse::Op, || {
                    for _ in 0..8 {
                        clock.wait_us(10_000);
                        std::thread::yield_now();
                    }
                });
                assert_eq!(turn, Turn::Ran(()));
                runs.fetch_add(1, Ordering::SeqCst);
            }
        })
    };
    let p1_before = r.ext.port_counters(P1).unwrap().polls;
    let scan = {
        let r = Arc::clone(&r);
        std::thread::spawn(move || {
            for _ in 0..PASSES {
                assert_eq!(r.ext.gate(P0), Verdict::NoOpinion);
                assert_eq!(r.ext.gate(P1), Verdict::NoOpinion);
            }
        })
    };
    until("every pass returned", || scan.is_finished());
    scan.join().unwrap();
    stop.store(true, Ordering::SeqCst);
    until("the SAI thread stops", || sai.is_finished());
    sai.join().unwrap();
    let c = r.ext.counts();
    // port 1 (another PHY) polled on every pass
    assert_eq!(r.ext.port_counters(P1).unwrap().polls - p1_before, PASSES);
    // port 0: every pass either polled or answered from the cache because of a run; a wait only
    // after GATE_BUSY_MAX busy ones, each at most one run (80 ms of the clock)
    let p0 = r.ext.port_counters(P0).unwrap();
    assert!(p0.polls >= 1, "{p0:?}");
    assert!(
        c.gate_waits <= c.gate_busy / u64::from(sai_extphy::GATE_BUSY_MAX),
        "{c:?}"
    );
    assert!(c.gate_wait_us_max <= 80_000, "{c:?}");
    assert_eq!(r.fw.violations(), Vec::<String>::new());
}

/// The rim's SAI call ([`sai_extphy::call`]) on a stand-in SAI lock: the change claims its PHY
/// first; the transaction in the way makes the call let go of the lock and wait outside it; a
/// begin cannot slip in while the call holds its claim; the change runs once, after the
/// transaction, never under the lock behind it. A get whose PN read is overdue does the same.
#[test]
fn inside_a_sai_call_nothing_waits_for_a_transaction_under_the_sai_lock() {
    let (r, clock) = rig(3000);
    let r = Arc::new(r);
    let sai_lock = Arc::new(Mutex::new(()));
    let t = begin(&r, 0x10);
    let mut tool = diag(&r, DiagWho::Txn(t), 0x10);
    tool(0xa000, Some(0xc161)).unwrap();
    let runs = Arc::new(AtomicU64::new(0));
    let sai = {
        let (r, sai_lock, runs) = (Arc::clone(&r), Arc::clone(&sai_lock), Arc::clone(&runs));
        std::thread::spawn(move || {
            sai_extphy::call(|| {
                let _lock = sai_lock.lock().unwrap();
                sai_extphy::under_sai_lock(|| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    // before the first effect
                    let _claim = match r.ext.claim(&[0x10]) {
                        Ok(claim) => claim,
                        Err(deferred) => {
                            assert_eq!(deferred.txn, 1);
                            return None;
                        }
                    };
                    Some(lib_run(&r, P0, 0x10, MacsecUse::Op))
                })
            })
        })
    };
    until("the call waits outside the SAI lock", || r.ext.waiting() == 1);
    drop(sai_lock.try_lock().expect("the SAI lock is free"));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    tool(0x8222, Some(0x1444)).unwrap();
    assert_eq!(tool(0x8221, None).unwrap(), 0x14e3);
    tool(0xa100, None).unwrap();
    tool(0xa101, None).unwrap();
    r.ext.txn_end(t).unwrap();
    assert_eq!(sai.join().unwrap(), Some(Turn::Ran(true)));
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(r.ext.claimed(), 0);
    let c = r.ext.counts();
    assert_eq!((c.calls_deferred, c.op_waits, c.locked_waits), (1, 1, 0), "{c:?}");
    assert_eq!(count(&r.lines, "external PHY phy 0x10: a SAI call waited "), 1);
    // a held claim keeps a begin off its PHY: it gives up after LEASE_MAX_MS (the clock moved
    // here until it does), busy, with no transaction in the way
    let claim = r.ext.claim(&[0x10]).unwrap();
    assert_eq!(r.ext.claimed(), 1);
    let beginner = {
        let r = Arc::clone(&r);
        std::thread::spawn(move || r.ext.txn_begin(LeaseScope::Phys(BTreeSet::from([0x10])), None))
    };
    until("the begin gives up", || {
        clock.advance_ms(100);
        beginner.is_finished()
    });
    assert!(matches!(
        beginner.join().unwrap(),
        Err(TxnRefused::Busy { txn: None, .. })
    ));
    drop(claim);
    assert_eq!(r.ext.claimed(), 0);
    // a PN get whose read is overdue: skipped inside the transaction, its call waits outside the
    // lock and reads once it is over (never inside it)
    let _expires = begin(&r, 0x10);
    assert_eq!(lib_run(&r, P0, 0x10, MacsecUse::Poll("pn.egress.0")), Turn::Skipped);
    clock.advance_ms(MACSEC_POLL_DEFER_MAX_MS); // that lease (1000 ms) runs out meanwhile
    let t = begin(&r, 0x10);
    let get = {
        let (r, sai_lock) = (Arc::clone(&r), Arc::clone(&sai_lock));
        std::thread::spawn(move || {
            sai_extphy::call(|| {
                let _lock = sai_lock.lock().unwrap();
                sai_extphy::under_sai_lock(|| lib_run(&r, P0, 0x10, MacsecUse::Poll("pn.egress.0")))
            })
        })
    };
    until("the get waits outside the SAI lock", || r.ext.waiting() == 1);
    drop(sai_lock.try_lock().expect("the SAI lock is free"));
    r.ext.txn_end(t).unwrap();
    assert_eq!(get.join().unwrap(), Turn::Ran(true));
    let c = r.ext.counts();
    assert_eq!((c.forced_reads, c.calls_deferred, c.locked_waits), (1, 2, 0), "{c:?}");
    assert_eq!(r.ext.claimed(), 0);
    assert_eq!(r.fw.violations(), Vec::<String>::new());
}

/// Re-validation: a SAI call's object moves to another PHY while the call waits outside the SAI
/// lock (here: the PHY it wants changes between its runs). The second run claims the new PHY,
/// meets the transaction there, waits for it too; the third run holds both claims and changes the
/// new PHY, never inside a transaction; every claim is given back when the call returns.
#[test]
fn a_call_whose_object_moved_while_it_waited_claims_again() {
    let (r, _clock) = rig(3000);
    let r = Arc::new(r);
    let target = Arc::new(AtomicU64::new(0x10));
    let runs = Arc::new(AtomicU64::new(0));
    let t1 = begin(&r, 0x10);
    let sai = {
        let (r, target, runs) = (Arc::clone(&r), Arc::clone(&target), Arc::clone(&runs));
        std::thread::spawn(move || {
            sai_extphy::call(|| {
                sai_extphy::under_sai_lock(|| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    let phy = u16::try_from(target.load(Ordering::SeqCst)).unwrap();
                    let _claim = r.ext.claim(&[phy]).ok()?;
                    let port = if phy == 0x10 { P0 } else { P1 };
                    Some((phy, lib_run(&r, port, phy, MacsecUse::Op)))
                })
            })
        })
    };
    until("the first run waits for transaction 1", || r.ext.waiting() == 1);
    // the object moves to PHY 0x11, where another transaction begins
    target.store(0x11, Ordering::SeqCst);
    let t2 = begin(&r, 0x11);
    r.ext.txn_end(t1).unwrap();
    until("the second run waits for transaction 2", || {
        runs.load(Ordering::SeqCst) == 2 && r.ext.waiting() == 1
    });
    assert_eq!(r.ext.claimed(), 1); // 0x10, taken after transaction 1
    r.ext.txn_end(t2).unwrap();
    assert_eq!(sai.join().unwrap(), Some((0x11, Turn::Ran(true))));
    assert_eq!(runs.load(Ordering::SeqCst), 3);
    assert_eq!(r.ext.claimed(), 0);
    let c = r.ext.counts();
    assert_eq!((c.calls_deferred, c.locked_waits), (2, 0), "{c:?}");
    assert_eq!(r.fw.violations(), Vec::<String>::new());
}

/// A change under the SAI lock on PHYs its call did not claim, with a transaction in the way:
/// the gap in the claims is a bug, caught by the debug assertion (release builds count and log
/// it, and wait).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "under the SAI lock")]
fn an_unclaimed_change_under_the_sai_lock_is_asserted() {
    let (r, _clock) = rig(3000);
    let _t = begin(&r, 0x10);
    sai_extphy::under_sai_lock(|| lib_run(&r, P0, 0x10, MacsecUse::Op));
}

#[test]
fn a_logger_may_call_back_into_the_runtime() {
    // log lines are written with no lock of the runtime held: a logger that asks for the
    // status text (as a careless one might) does not deadlock and trips no lock-order assertion
    let me: Arc<Mutex<Option<ExtPhy>>> = Arc::default();
    let seen = Arc::new(AtomicU64::new(0));
    let log: Logger = {
        let (me, seen) = (Arc::clone(&me), Arc::clone(&seen));
        Arc::new(move |_, _, _: &str| {
            let ext = me.lock().unwrap().clone();
            if let Some(ext) = ext {
                assert!(ext.status().starts_with("extphy: "));
                seen.fetch_add(1, Ordering::SeqCst);
            }
        })
    };
    let clock = ManualClock::new();
    let table = sai_extphy::ExtPhyTable::parse(table(50).as_bytes()).unwrap();
    let ext = ExtPhy::with_table(table, Arc::new(clock.clone()), log);
    let fw = FwPhy::new();
    fw.set(0x10, 0xa100, 2);
    fw.set(0x10, 0xa101, 3);
    ext.attach(&ports(), Box::new(FwBus(Arc::clone(&fw)))).unwrap();
    *me.lock().unwrap() = Some(ext.clone());
    ext.admin(P0, true); // "sequence done" (info)
    assert_eq!(ext.gate(P0), Verdict::NoOpinion); // "line side up"
    ext.diag_access(DiagWho::Bare, DiagTarget::Phy(0x10), || ()).unwrap(); // "paused"
    let t = ext.txn_begin(LeaseScope::All, Some(10)).unwrap().txn;
    clock.advance_ms(20);
    assert!(ext.txn_end(t).is_err()); // "expired"
    assert!(seen.load(Ordering::SeqCst) >= 4);
    *me.lock().unwrap() = None;
}

/// Real threads: the SDK link scan (gates of both ports), the SAI thread (`MACsec` changes and
/// polls on both PHYs, admin flaps), the diag socket (transactions with conversations on PHY
/// 0x11, single bare reads on PHY 0x10, transactions abandoned between conversations), under a
/// watchdog. On the injected clock, which only the diag thread moves (to let an abandoned lease
/// run out), so no lease runs out by load. They run until each has done 50 rounds. No
/// interleaving, no deadlock, every thread finishes.
#[test]
fn three_threads_on_two_phys_no_interleaving_no_deadlock() {
    const ROUNDS: u64 = 50;
    let clock = ManualClock::new();
    let r = Arc::new(rig_on(Arc::new(clock.clone()), 5));
    let stop = Arc::new(AtomicBool::new(false));
    let rounds: Arc<[AtomicU64; 3]> = Arc::new([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)]);
    let mut threads = Vec::new();
    {
        let (r, stop, rounds) = (Arc::clone(&r), Arc::clone(&stop), Arc::clone(&rounds));
        threads.push(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                r.ext.gate(P0);
                r.ext.gate(P1);
                rounds[0].fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    {
        let (r, stop, rounds) = (Arc::clone(&r), Arc::clone(&stop), Arc::clone(&rounds));
        threads.push(std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::SeqCst) {
                for (port, phy) in [(P0, 0x10), (P1, 0x11)] {
                    assert_ne!(lib_run(&r, port, phy, MacsecUse::Op), Turn::Ran(false));
                    assert_ne!(lib_run(&r, port, phy, MacsecUse::Poll("pn.egress.0")), Turn::Ran(false));
                }
                if n.is_multiple_of(10) {
                    r.ext.admin(P0, n.is_multiple_of(20));
                    r.ext.admin(P0, true);
                }
                n += 1;
                rounds[1].fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    {
        let (r, stop, rounds, clock) = (Arc::clone(&r), Arc::clone(&stop), Arc::clone(&rounds), clock.clone());
        threads.push(std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::SeqCst) {
                let t = r
                    .ext
                    .txn_begin(LeaseScope::Phys(BTreeSet::from([0x11])), Some(30))
                    .unwrap()
                    .txn;
                let mut tool = diag(&r, DiagWho::Txn(t), 0x11);
                assert!(conversation(&mut tool, 0x1777).unwrap());
                if n % 7 == 3 {
                    // the client dies: its lease runs out by itself
                    clock.advance_ms(31);
                    assert!(matches!(r.ext.txn_end(t), Err(TxnRefused::Expired { .. })));
                } else {
                    r.ext.txn_end(t).unwrap();
                }
                diag(&r, DiagWho::Bare, 0x10)(0x8b00, None).unwrap();
                n += 1;
                rounds[2].fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    until("every thread did its rounds", || {
        rounds.iter().all(|n| n.load(Ordering::SeqCst) >= ROUNDS)
    });
    stop.store(true, Ordering::SeqCst);
    let watchdog = Instant::now();
    for t in threads {
        while !t.is_finished() {
            assert!(
                watchdog.elapsed() < Duration::from_secs(60),
                "a thread did not finish: deadlock"
            );
            std::thread::yield_now();
        }
        t.join().unwrap();
    }
    assert_eq!(r.fw.violations(), Vec::<String>::new());
    let c = r.ext.counts();
    assert!(c.txn_begun >= ROUNDS && c.txn_expired >= ROUNDS / 7, "{c:?}");
}

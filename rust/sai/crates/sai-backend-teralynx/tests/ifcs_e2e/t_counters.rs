//! t_counters.cpp: IF_OUT_OCTETS on TX_BYTES_OK (isai's choice), every mapped SAI port counter through
//! the diag shell ("counters <devport>"), CPU queue counters ("copp"), the ACL report ("acl").
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

use crate::support::*;

/// `diag()` of the C++: one command to the diag shell over its unix socket, the whole answer.
fn diag(path: &str, cmd: &str) -> String {
    let Ok(mut s) = UnixStream::connect(path) else {
        return "(no diag socket)".to_owned();
    };
    let c = format!("{cmd}\n");
    let _ = s.write_all(c.as_bytes());
    let _ = s.shutdown(Shutdown::Write);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

pub fn test_counters(e: &mut Env) {
    e.ck.section("counters");
    let p1 = e.p1;
    // ---- IF_OUT_OCTETS = TX_BYTES_OK (107), not TX_BYTES_ALL (106); IN stays RX_BYTES_ALL
    e.m().set_stat(1, 106, 9999);
    e.m().set_stat(1, 107, 7777);
    e.m().set_stat(1, 47, 5555);
    ok!(e, "t_counters.cpp:40", e.sai.clear_port_all_stats(p1));
    e.m().set_stat(1, 106, 9999);
    e.m().set_stat(1, 107, 7777);
    e.m().set_stat(1, 47, 5555);
    let ids = [sys::SAI_PORT_STAT_IF_OUT_OCTETS, sys::SAI_PORT_STAT_IF_IN_OCTETS];
    let (st, v) = e.sai.stats(sys::SAI_OBJECT_TYPE_PORT, p1, &ids);
    ok!(e, "t_counters.cpp:46", st);
    check_eq!(e, "t_counters.cpp:47", v[0], 7777u64);
    check_eq!(e, "t_counters.cpp:48", v[1], 5555u64);

    // ---- every mapped SAI port counter (diag shell): FEC, PFC, size buckets, pause
    e.m().set_stat(1, 52, 11); // RX_KRPFEC_CORRECTED_CW_COUNT
    e.m().set_stat(1, 53, 12); // UNCORRECTED
    e.m().set_stat(1, 54, 13); // CW_SYMBOL_ERR
    e.m().set_stat(1, 24, 3); // RX_FRAMES_PFC3
    e.m().set_stat(1, 87, 4); // TX_FRAMES_PFC3
    e.m().set_stat(1, 10, 20); // RX_FRAMES_LEN_64
    e.m().set_stat(1, 9, 1); // RX_FRAMES_LEN_LT_64 (runts)
    e.m().set_stat(1, 73, 30); // TX_FRAMES_LEN_64
    e.m().set_stat(1, 18, 5); // RX 4096-8191
    e.m().set_stat(1, 19, 6); // RX 8192-9215
    e.m().set_stat(1, 6, 7); // RX pause
    let sock = format!("{}/diag.sock", e.dir);
    if sock.len() >= 100 {
        // test_ifcs.cpp sets the key only when the path fits a unix socket
        eprintln!("t_counters: scratch dir too long for the diag socket: diag checks SKIPPED");
        return;
    }
    let out = diag(&sock, "counters 1");
    let want = |sai: u32, val: u64| {
        let b = format!("{sai}={val}");
        let ok = out.contains(&format!(" {b}")) || out.contains(&format!("\n{b}"));
        if !ok {
            eprintln!("counters: want {b}");
        }
        ok
    };
    check!(e, "t_counters.cpp:75", has(&out, "devport 1:"));
    check!(
        e,
        "t_counters.cpp:76",
        want(sys::SAI_PORT_STAT_IF_IN_FEC_CORRECTABLE_FRAMES, 11)
    );
    check!(
        e,
        "t_counters.cpp:77",
        want(sys::SAI_PORT_STAT_IF_IN_FEC_NOT_CORRECTABLE_FRAMES, 12)
    );
    check!(
        e,
        "t_counters.cpp:78",
        want(sys::SAI_PORT_STAT_IF_IN_FEC_SYMBOL_ERRORS, 13)
    );
    check!(e, "t_counters.cpp:79", want(sys::SAI_PORT_STAT_PFC_3_RX_PKTS, 3));
    check!(e, "t_counters.cpp:80", want(sys::SAI_PORT_STAT_PFC_3_TX_PKTS, 4));
    check!(
        e,
        "t_counters.cpp:81",
        want(sys::SAI_PORT_STAT_ETHER_IN_PKTS_64_OCTETS, 21)
    ); // 64 B + runts, as isai
    check!(
        e,
        "t_counters.cpp:82",
        want(sys::SAI_PORT_STAT_ETHER_OUT_PKTS_64_OCTETS, 30)
    );
    check!(
        e,
        "t_counters.cpp:83",
        want(sys::SAI_PORT_STAT_ETHER_STATS_PKTS_64_OCTETS, 50)
    ); // rx + tx
    check!(
        e,
        "t_counters.cpp:84",
        want(sys::SAI_PORT_STAT_ETHER_IN_PKTS_4096_TO_9216_OCTETS, 11)
    );
    check!(e, "t_counters.cpp:85", want(sys::SAI_PORT_STAT_PAUSE_RX_PKTS, 7));
    check!(e, "t_counters.cpp:86", want(sys::SAI_PORT_STAT_IF_OUT_OCTETS, 7777));
    check!(
        e,
        "t_counters.cpp:87",
        want(sys::SAI_PORT_STAT_ETHER_STATS_OCTETS, 5555 + 9999)
    );
    check!(
        e,
        "t_counters.cpp:88",
        has(&out, "cpu queue 3: tx 103 bytes 6592 drops 3")
    );
    check!(e, "t_counters.cpp:89", has(&diag(&sock, "counters 999"), "unreadable"));

    // ---- CPU queue counters in the CoPP report, ACL report
    check!(e, "t_counters.cpp:92", has(&diag(&sock, "copp"), "cpu queue 0: tx 100"));
    check!(
        e,
        "t_counters.cpp:93",
        has(&diag(&sock, "acl"), "ACL: 0 tables, 0 entries")
    );
    check!(e, "t_counters.cpp:94", has(&diag(&sock, "help"), "counters <devport>"));
}

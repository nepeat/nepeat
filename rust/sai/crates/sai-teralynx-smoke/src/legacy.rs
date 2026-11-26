//! The C++ program's form, line for line (`tools/sai_smoke.cpp`): `PROFILE SECONDS
//! [FIRST_LANE[:IFNAME] ...]`. Brings the ports whose first lane is `FIRST_LANE` admin up (MTU 9100),
//! optionally with a host interface `IFNAME`, prints oper state, counters and the ASIC temperature
//! every 2 s for SECONDS (SIGINT / SIGTERM end it early), then admin down, host interfaces removed,
//! `remove_switch`, `sai_api_uninitialize`. Exit 0, 1 (a call failed), 2 (usage).
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sai_client::{Api, Attr, Event, Notify, Sai, Status, Value, sys};

use crate::{pad, read_profile, say, warn};

/// C `atoi`: leading blanks, a sign, digits; anything else ends it (0 for none).
pub fn atoi(s: &str) -> i32 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, d) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or_default()),
        Some(b'+') => (false, t.get(1..).unwrap_or_default()),
        _ => (false, t),
    };
    let mut v: i64 = 0;
    for c in d.bytes().take_while(u8::is_ascii_digit) {
        v = v.saturating_mul(10).saturating_add(i64::from(c - b'0'));
    }
    let v = if neg { -v } else { v };
    i32::try_from(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

/// C `(uint32_t)strtoul(s, nullptr, 0)` (0 when nothing parses).
#[allow(clippy::cast_possible_truncation, reason = "(uint32_t)")]
pub fn strtoul0(s: &str) -> u32 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, d) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or_default()),
        Some(b'+') => (false, t.get(1..).unwrap_or_default()),
        _ => (false, t),
    };
    let (base, digits) = if (d.starts_with("0x") || d.starts_with("0X"))
        && d.get(2..3).is_some_and(|c| c.chars().all(|c| c.is_ascii_hexdigit()))
    {
        (16, d.get(2..).unwrap_or_default())
    } else if d.starts_with('0') {
        (8, d)
    } else {
        (10, d)
    };
    let mut v: u64 = 0;
    for c in digits.chars().map_while(|c| c.to_digit(base)) {
        v = v
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(c)))
            .unwrap_or(u64::MAX);
    }
    (if neg { v.wrapping_neg() } else { v }) as u32
}

/// The C++'s `CHECK`: "<what padded to 28> <status>", a failure remembered.
fn check(fail: &mut bool, what: &str, st: Status) {
    say(&format!("{} {st}", pad(what, 28)));
    if st != sai_client::SUCCESS {
        *fail = true;
    }
}

struct Want {
    lane: u32,
    ifname: String,
    port: sys::sai_object_id_t,
    hostif: sys::sai_object_id_t,
}

/// The program (the C++'s `main`).
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "the C++ main, in its order and with its names"
)]
pub fn main(args: &[String]) -> i32 {
    let argv0 = args.first().map_or("sai-teralynx-smoke", String::as_str);
    let (Some(profile_path), Some(secs)) = (args.get(1), args.get(2)) else {
        warn(&format!("usage: {argv0} PROFILE SECONDS [FIRST_LANE[:IFNAME] ...]"));
        return 2;
    };
    let profile = read_profile(profile_path).unwrap_or_default();
    let seconds = atoi(secs);
    let mut want: Vec<Want> = args
        .iter()
        .skip(3)
        .map(|a| {
            let (l, n) = a.split_once(':').map_or((a.as_str(), ""), |(l, n)| (l, n));
            Want {
                lane: strtoul0(l),
                ifname: n.to_owned(),
                port: 0,
                hostif: 0,
            }
        })
        .collect();
    let stop = Arc::new(AtomicBool::new(false));
    for s in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(s, Arc::clone(&stop));
    }
    let mut fail = false;
    let mut sai = match Sai::load(&sai_client::default_libsai()) {
        Ok(s) => s,
        Err(e) => {
            warn(&format!("sai-teralynx-smoke: {e}"));
            return 1;
        }
    };
    let _ = sai_client::set_profile(profile.into_iter().collect());
    sai_client::set_event_sink(|e| match e {
        Event::PortState { port, state } => say(&format!("event port 0x{port:x} oper {state}")),
        Event::Shutdown => say("event shutdown request"),
        Event::Fdb { .. } => {}
    });
    check(&mut fail, "sai_api_initialize", sai.initialize());
    check(&mut fail, "query switch", sai.query(Api::Switch));
    check(&mut fail, "query port", sai.query(Api::Port));
    check(&mut fail, "query hostif", sai.query(Api::Hostif));
    if fail {
        return 1;
    }
    let at = [
        Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
        Attr::new(
            sys::SAI_SWITCH_ATTR_SRC_MAC_ADDRESS,
            Value::Mac([0x0e, 0x48, 0xc6, 0x7a, 0x55, 0xce]),
        ),
        Attr::new(
            sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
            Value::Notify(Notify::PortState),
        ),
        Attr::new(
            sys::SAI_SWITCH_ATTR_SWITCH_SHUTDOWN_REQUEST_NOTIFY,
            Value::Notify(Notify::Shutdown),
        ),
    ];
    let mut swid = 0;
    check(&mut fail, "create_switch", sai.create_switch(&mut swid, &at));
    if fail {
        let _ = sai.uninitialize();
        return 1;
    }
    let mut a = [Attr::new(sys::SAI_SWITCH_ATTR_PORT_LIST, Value::OidList(vec![0; 512]))];
    check(
        &mut fail,
        "get port list",
        sai.get(sys::SAI_OBJECT_TYPE_SWITCH, swid, &mut a),
    );
    let mut ports = match &a[0].value {
        Value::OidList(l) => l.clone(),
        _ => Vec::new(),
    };
    // std::vector::resize(count)
    ports.resize(usize::try_from(a[0].count).unwrap_or(0), 0);
    say(&format!("ports {}", ports.len()));
    for p in &ports {
        let mut q = [
            Attr::new(sys::SAI_PORT_ATTR_HW_LANE_LIST, Value::U32List(vec![0; 8])),
            Attr::new(sys::SAI_PORT_ATTR_SPEED, Value::U32(0)),
        ];
        let st = sai.get(sys::SAI_OBJECT_TYPE_PORT, *p, &mut q);
        let lanes = match &q[0].value {
            Value::U32List(l) => l.clone(),
            _ => Vec::new(),
        };
        let speed = match q[1].value {
            Value::U32(v) => v,
            _ => 0,
        };
        let mut line = format!("port 0x{p:x} st {st} speed {speed} lanes");
        for l in lanes.iter().take(8) {
            let _ = write!(line, " {l}");
        }
        say(&line);
        for w in &mut want {
            if st == sai_client::SUCCESS && q[0].count != 0 && lanes.first() == Some(&w.lane) {
                w.port = *p;
            }
        }
    }
    for w in &mut want {
        if w.port == 0 {
            say(&format!("no port with first lane {}", w.lane));
            fail = true;
            continue;
        }
        let mtu = Attr::new(sys::SAI_PORT_ATTR_MTU, Value::U32(9100));
        check(
            &mut fail,
            "set mtu 9100",
            sai.set(sys::SAI_OBJECT_TYPE_PORT, w.port, Some(&mtu)),
        );
        if !w.ifname.is_empty() {
            let name: Vec<u8> = w
                .ifname
                .bytes()
                .take(usize::try_from(sys::SAI_HOSTIF_NAME_SIZE - 1).unwrap_or(15))
                .collect();
            #[allow(clippy::cast_possible_wrap, reason = "an enumerator as s32")]
            let h = [
                Attr::new(
                    sys::SAI_HOSTIF_ATTR_TYPE,
                    Value::S32(sys::SAI_HOSTIF_TYPE_NETDEV as i32),
                ),
                Attr::new(sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(w.port)),
                Attr::new(sys::SAI_HOSTIF_ATTR_NAME, Value::Chardata(name)),
            ];
            let (st, id) = sai.create(sys::SAI_OBJECT_TYPE_HOSTIF, swid, &h);
            w.hostif = id;
            check(&mut fail, &format!("hostif {}", w.ifname), st);
        }
        let up = Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(true));
        check(
            &mut fail,
            "admin up",
            sai.set(sys::SAI_OBJECT_TYPE_PORT, w.port, Some(&up)),
        );
    }
    let mut t = 0;
    while t < seconds && !stop.load(Ordering::Relaxed) {
        let mut a = [Attr::new(sys::SAI_SWITCH_ATTR_MAX_TEMP, Value::S32(0))];
        let ts = sai.get(sys::SAI_OBJECT_TYPE_SWITCH, swid, &mut a);
        let temp = if let Value::S32(v) = a[0].value { v } else { 0 };
        let mut line = format!("t={t} temp st {ts} {temp} C");
        for w in want.iter().filter(|w| w.port != 0) {
            let mut o = [Attr::new(sys::SAI_PORT_ATTR_OPER_STATUS, Value::S32(0))];
            let _ = sai.get(sys::SAI_OBJECT_TYPE_PORT, w.port, &mut o);
            let oper = if let Value::S32(v) = o[0].value { v } else { 0 };
            let ids = [
                sys::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
                sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
                sys::SAI_PORT_STAT_IF_IN_ERRORS,
                sys::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
            ];
            let (ss, v) = sai.stats(sys::SAI_OBJECT_TYPE_PORT, w.port, &ids);
            let g = |i: usize| v.get(i).copied().unwrap_or(0);
            let _ = write!(
                line,
                " | lane {} oper {oper} stats({ss}) in {} out {} err {} nonuc {}",
                w.lane,
                g(0),
                g(1),
                g(2),
                g(3)
            );
        }
        say(&line);
        for _ in 0..20 {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        t += 2;
    }
    for w in want.iter().filter(|w| w.port != 0) {
        let down = Attr::new(sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(false));
        check(
            &mut fail,
            "admin down",
            sai.set(sys::SAI_OBJECT_TYPE_PORT, w.port, Some(&down)),
        );
        if w.hostif != 0 {
            check(
                &mut fail,
                "remove hostif",
                sai.remove(sys::SAI_OBJECT_TYPE_HOSTIF, w.hostif),
            );
        }
    }
    check(&mut fail, "remove_switch", sai.remove_switch(swid));
    check(&mut fail, "sai_api_uninitialize", sai.uninitialize());
    i32::from(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c_number_readers() {
        assert_eq!(atoi(" 30s"), 30);
        assert_eq!(atoi("x"), 0);
        assert_eq!(atoi("-2"), -2);
        assert_eq!(strtoul0("0x41"), 0x41);
        assert_eq!(strtoul0("017"), 15);
        assert_eq!(strtoul0("9"), 9);
        assert_eq!(strtoul0("abc"), 0);
    }
}

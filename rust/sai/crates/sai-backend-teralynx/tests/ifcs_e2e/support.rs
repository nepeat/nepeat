//! What the area files share: the check recorder and its macros, the environment (the loaded
//! libsai, the mock's hooks, the notifications, the objects an area hands to the next), and small
//! constructors for attributes. `harness.h` of the C++ test, in Rust.
#![allow(dead_code, reason = "each area uses its own part")]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub use sai_client::sys;
pub use sai_client::{Api, Attr, Entry, EntryOp, Event, Mock, Prefix, SUCCESS, Sai, Status, Value};

/// A marker line `@@ <text>` in the parity harness's stream (`PARITY_TAP_FILE`, which its tap appends the
/// mock's call lines to; one write per line, as the tap's): nothing without it.
fn tap_mark(text: &str) {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("PARITY_TAP_FILE") else {
        return;
    };
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .expect("PARITY_TAP_FILE");
    f.write_all(format!("@@ {text}\n").as_bytes()).expect("PARITY_TAP_FILE");
}

/// One check of the test: the C++ id it ports (`t_port.cpp:50`, the line of the `CHECK` /
/// `CHECK_EQ` / `OK`), where in Rust, what it did.
#[derive(Debug, Default)]
pub struct Ck {
    /// every check execution, in order: the C++ id and whether it passed
    pub seq: Vec<(String, bool)>,
    /// per C++ id: the Rust file and line of the assertion, and how often it ran
    pub sites: BTreeMap<String, (String, u32)>,
    /// per section (the areas of `main()`): name, checks, failed
    pub sections: Vec<(String, u32, u32)>,
}

impl Ck {
    /// A new section: the checks that follow count there. Under the parity harness (crate
    /// sai-teralynx-parity, `PARITY_TAP_FILE`) the stream gets the markers the C++ test's section
    /// wrappers wrote (its `c/sections.cpp`, deleted in H5): `@@ end <previous>`, then `@@ begin <name>`
    /// (none for the first, `main`: the stream's start, and none for `uninit`, what follows the last area;
    /// [`Ck::finish`] ends it).
    pub fn section(&mut self, name: &str) {
        if let Some((prev, _, _)) = self.sections.last() {
            tap_mark(&format!("end {prev}"));
            if name != "uninit" {
                tap_mark(&format!("begin {name}"));
            }
        }
        self.sections.push((name.to_owned(), 0, 0));
    }

    /// The end of the run: the last section's end marker, and the checks per section into
    /// `PARITY_COUNTS_FILE` when it is set (`section <name> checks <n> failed <n>`, the parity harness's
    /// counts, as the C++ test's section wrappers wrote them).
    pub fn finish(&self) {
        if let Some((last, _, _)) = self.sections.last() {
            tap_mark(&format!("end {last}"));
        }
        if let Some(path) = std::env::var_os("PARITY_COUNTS_FILE") {
            let text: String = self
                .sections
                .iter()
                .map(|(name, checks, failed)| format!("section {name} checks {checks} failed {failed}\n"))
                .collect();
            std::fs::write(&path, text).expect("PARITY_COUNTS_FILE");
        }
    }

    /// Records one check execution (`rust`: `file:line` of the assertion); a failure is printed as
    /// the C++ test prints it.
    pub fn record(&mut self, id: &str, rust: &str, ok: bool, detail: impl FnOnce() -> String) {
        if let Some(s) = self.sections.last_mut() {
            s.1 += 1;
            s.2 += u32::from(!ok);
        }
        self.seq.push((id.to_owned(), ok));
        let site = self.sites.entry(id.to_owned()).or_insert_with(|| (rust.to_owned(), 0));
        site.1 += 1;
        if !ok {
            // the C++ test's form (the parity harness's FAIL lines: "FAIL <file>:<line>: ..."), then the Rust site
            eprintln!("FAIL {id}: {} (Rust {rust})", detail());
        }
    }

    /// Checks run.
    pub fn checks(&self) -> usize {
        self.seq.len()
    }

    /// Checks failed.
    pub fn failed(&self) -> usize {
        self.seq.iter().filter(|(_, ok)| !*ok).count()
    }
}

/// `CHECK(cond)` of the C++ test: `check!(e, "t_port.cpp:50", cond)`.
macro_rules! check {
    ($e:expr, $id:literal, $cond:expr) => {{
        let ok: bool = $cond;
        $e.ck
            .record($id, concat!(file!(), ":", line!()), ok, || stringify!($cond).to_owned());
    }};
}

/// `CHECK_EQ(a, b)`: both sides evaluated once, compared with `==`.
macro_rules! check_eq {
    ($e:expr, $id:literal, $a:expr, $b:expr) => {{
        let (a, b) = ($a, $b);
        let ok = a == b;
        $e.ck.record($id, concat!(file!(), ":", line!()), ok, || {
            format!("{} == {} ({:?} vs {:?})", stringify!($a), stringify!($b), a, b)
        });
    }};
}

/// `OK(x)`: the status is `SAI_STATUS_SUCCESS`.
macro_rules! ok {
    ($e:expr, $id:literal, $st:expr) => {{
        let st: sai_client::Status = $st;
        $e.ck
            .record($id, concat!(file!(), ":", line!()), st == sai_client::SUCCESS, || {
                format!("{} = {st}", stringify!($st))
            });
    }};
}

/// A notification the library delivered.
#[derive(Clone, Debug)]
pub struct FdbEvent {
    /// `event_type`
    pub event: i32,
    /// the entry's `bv_id`
    pub bv: u64,
    /// the entry's MAC
    pub mac: [u8; 6],
}

/// The notifications so far (`g_port_events`, `g_fdb_events`).
#[derive(Debug, Default)]
pub struct Events {
    /// port state changes: port, `sai_port_oper_status_t`
    pub port: Vec<(u64, i32)>,
    /// FDB events
    pub fdb: Vec<FdbEvent>,
}

/// The switch under test: the library, the mock's hooks, and what one area creates for a later one.
pub struct Env {
    /// the loaded libsai
    pub sai: Sai,
    /// the mock libifcs, once the switch exists
    pub mock: Option<Mock>,
    /// the checks
    pub ck: Ck,
    /// notifications
    pub events: Arc<Mutex<Events>>,
    /// the switch
    pub s: u64,
    /// ports 1, 2, 3 (devports 1, 2, 3)
    pub p1: u64,
    /// port 2
    pub p2: u64,
    /// port 3
    pub p3: u64,
    /// the host interface of port 1
    pub hif: u64,
    /// VLAN 100
    pub vlan: u64,
    /// port 1's bridge port
    pub bp1: u64,
    /// scratch dir, the init yaml, the ISG yaml, the run dir (`main()` wrote them)
    pub dir: String,
    /// the init config
    pub init: String,
    /// the ISG config
    pub isg: String,
    /// the library's run dir
    pub run: String,
}

impl Env {
    /// The mock's hooks.
    pub fn m(&self) -> &Mock {
        self.mock.as_ref().expect("the mock is attached after create_switch")
    }

    /// `create_<type>(&id, s, attrs)`: status and id.
    pub fn create(&self, ty: sys::sai_object_type_t, attrs: &[Attr]) -> (Status, u64) {
        self.sai.create(ty, self.s, attrs)
    }

    /// `remove_<type>(oid)`.
    pub fn remove(&self, ty: sys::sai_object_type_t, oid: u64) -> Status {
        self.sai.remove(ty, oid)
    }

    /// `set_<type>_attribute(oid, &a)`.
    pub fn set(&self, ty: sys::sai_object_type_t, oid: u64, a: Attr) -> Status {
        self.sai.set(ty, oid, Some(&a))
    }

    /// `get_<type>_attribute(oid, n, attrs)`.
    pub fn get(&self, ty: sys::sai_object_type_t, oid: u64, attrs: &mut [Attr]) -> Status {
        self.sai.get(ty, oid, attrs)
    }

    /// A get of one attribute of the kind of `template` (a list's room is its length): status
    /// and the attribute as the library wrote it.
    pub fn get1(&self, ty: sys::sai_object_type_t, oid: u64, id: u32, template: Value) -> (Status, Attr) {
        let mut a = [Attr::new(id, template)];
        let st = self.sai.get(ty, oid, &mut a);
        let [a] = a;
        (st, a)
    }

    /// Waits up to 3 s (300 x 10 ms) for `f` over the notifications (`wait_for`).
    pub fn wait_for(&self, f: impl Fn(&Events) -> bool) -> bool {
        for _ in 0..300 {
            if f(&self.events.lock().unwrap_or_else(PoisonError::into_inner)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// The notifications now (a copy of the counts and entries).
    pub fn with_events<R>(&self, f: impl FnOnce(&mut Events) -> R) -> R {
        f(&mut self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Accessors of an attribute's value (what the C++ reads from the union; a kind the library did
/// not write reads 0 / empty).
pub trait AttrExt {
    /// `value.oid`
    fn oid(&self) -> u64;
    /// `value.u32`
    fn u32(&self) -> u32;
    /// `value.s32`
    fn s32(&self) -> i32;
    /// `value.u64`
    fn u64(&self) -> u64;
    /// `value.u16`
    fn u16(&self) -> u16;
    /// `value.u8`
    fn u8(&self) -> u8;
    /// `value.booldata`
    fn boolean(&self) -> bool;
    /// `value.objlist` entries (as many as fit the room and the count)
    fn oids(&self) -> Vec<u64>;
    /// `value.u32list`
    fn u32s(&self) -> Vec<u32>;
    /// `value.s32list`
    fn s32s(&self) -> Vec<i32>;
    /// the list count the library wrote
    fn count(&self) -> u32;
    /// `value.chardata` up to the NUL
    fn text(&self) -> String;
}

impl AttrExt for Attr {
    fn oid(&self) -> u64 {
        if let Value::Oid(v) = self.value { v } else { 0 }
    }
    fn u32(&self) -> u32 {
        if let Value::U32(v) = self.value { v } else { 0 }
    }
    fn s32(&self) -> i32 {
        if let Value::S32(v) = self.value { v } else { 0 }
    }
    fn u64(&self) -> u64 {
        if let Value::U64(v) = self.value { v } else { 0 }
    }
    fn u16(&self) -> u16 {
        if let Value::U16(v) = self.value { v } else { 0 }
    }
    fn u8(&self) -> u8 {
        if let Value::U8(v) = self.value { v } else { 0 }
    }
    fn boolean(&self) -> bool {
        matches!(self.value, Value::Bool(true))
    }
    fn oids(&self) -> Vec<u64> {
        match &self.value {
            Value::OidList(v)
            | Value::AclField(sai_client::AclField {
                data: sai_client::AclData::Oids(v),
                ..
            }) => v.clone(),
            _ => Vec::new(),
        }
    }
    fn u32s(&self) -> Vec<u32> {
        if let Value::U32List(v) = &self.value {
            v.clone()
        } else {
            Vec::new()
        }
    }
    fn s32s(&self) -> Vec<i32> {
        if let Value::S32List(v) = &self.value {
            v.clone()
        } else {
            Vec::new()
        }
    }
    fn count(&self) -> u32 {
        self.count
    }
    fn text(&self) -> String {
        if let Value::Chardata(v) = &self.value {
            String::from_utf8_lossy(v).into_owned()
        } else {
            String::new()
        }
    }
}

/// `booldata` attribute.
pub fn a_bool(id: u32, v: bool) -> Attr {
    Attr::new(id, Value::Bool(v))
}
/// `s32` attribute.
pub fn a_s32(id: u32, v: i32) -> Attr {
    Attr::new(id, Value::S32(v))
}
/// `u32` attribute.
pub fn a_u32(id: u32, v: u32) -> Attr {
    Attr::new(id, Value::U32(v))
}
/// `u64` attribute.
pub fn a_u64(id: u32, v: u64) -> Attr {
    Attr::new(id, Value::U64(v))
}
/// `u16` attribute.
pub fn a_u16(id: u32, v: u16) -> Attr {
    Attr::new(id, Value::U16(v))
}
/// `u8` attribute.
pub fn a_u8(id: u32, v: u8) -> Attr {
    Attr::new(id, Value::U8(v))
}
/// `oid` attribute.
pub fn a_oid(id: u32, v: u64) -> Attr {
    Attr::new(id, Value::Oid(v))
}
/// `mac` attribute.
pub fn a_mac(id: u32, v: [u8; 6]) -> Attr {
    Attr::new(id, Value::Mac(v))
}
/// `chardata` attribute (`snprintf` into the 32-byte array).
pub fn a_text(id: u32, v: &str) -> Attr {
    Attr::new(id, Value::Chardata(v.as_bytes().to_vec()))
}
/// An object list attribute.
pub fn a_oids(id: u32, v: &[u64]) -> Attr {
    Attr::new(id, Value::OidList(v.to_vec()))
}
/// An `s32list` attribute.
pub fn a_s32s(id: u32, v: &[i32]) -> Attr {
    Attr::new(id, Value::S32List(v.to_vec()))
}

/// `find_bridge_port` of t_l2.cpp / t_lag.cpp: the bridge port of the default 1Q bridge that has
/// `port` as its `PORT_ID`, 0 if none.
pub fn find_bridge_port(e: &Env, port: u64) -> u64 {
    let (st, a) = e.get1(
        sys::SAI_OBJECT_TYPE_SWITCH,
        e.s,
        sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID,
        Value::Oid(0),
    );
    if st != SUCCESS {
        return 0;
    }
    let (st, l) = e.get1(
        sys::SAI_OBJECT_TYPE_BRIDGE,
        a.oid(),
        sys::SAI_BRIDGE_ATTR_PORT_LIST,
        Value::OidList(vec![0; 64]),
    );
    if st != SUCCESS {
        return 0;
    }
    for bp in l.oids() {
        let (st, q) = e.get1(
            sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
            bp,
            sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
            Value::Oid(0),
        );
        if st == SUCCESS && q.oid() == port {
            return bp;
        }
    }
    0
}

/// `make_vlan_member` of t_l2.cpp / t_lag.cpp: a VLAN member, 0 if the create is refused.
pub fn make_vlan_member(e: &Env, vlan: u64, bp: u64, tagged: bool) -> u64 {
    let mode = if tagged {
        sys::SAI_VLAN_TAGGING_MODE_TAGGED
    } else {
        sys::SAI_VLAN_TAGGING_MODE_UNTAGGED
    };
    let (st, m) = e.create(
        sys::SAI_OBJECT_TYPE_VLAN_MEMBER,
        &[
            a_oid(sys::SAI_VLAN_MEMBER_ATTR_VLAN_ID, vlan),
            a_oid(sys::SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID, bp),
            a_s32(sys::SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE, mode as i32),
        ],
    );
    if st == SUCCESS { m } else { 0 }
}

/// `log.find(needle) != npos`.
pub fn has(log: &str, needle: &str) -> bool {
    log.contains(needle)
}

/// How often `needle` is in `log`, overlapping occurrences too (the C++ `count` advances by one).
pub fn count(log: &str, needle: &str) -> usize {
    let mut n = 0;
    let mut from = 0;
    while let Some(p) = log.get(from..).and_then(|s| s.find(needle)) {
        n += 1;
        from += p + log[from + p..].chars().next().map_or(1, char::len_utf8);
    }
    n
}

/// `std::getline` over `s`: the lines split at `\n`, no empty last one.
pub fn lines(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.split('\n').map(str::to_owned).collect();
    if v.last().is_some_and(String::is_empty) {
        v.pop();
    }
    v
}

/// Writes a file (`put`).
pub fn put(path: &str, s: &str) {
    std::fs::write(path, s).expect("write a test file");
}

/// Reads a file; empty when it is missing (`get`).
pub fn get(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The text of a number for a failure message list (kept for the area files' diagnostics).
pub fn hex_list(v: &[u32]) -> String {
    let mut s = String::new();
    for x in v {
        let _ = write!(s, "{x:#x} ");
    }
    s
}

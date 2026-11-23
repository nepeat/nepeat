//! End to end against the C++: the whole Rust path (the `sai-macsec` model, this crate's runner,
//! the `phy-seq` executor on a recording fake bus) and the whole C++ path (`sai_macsec.cpp` +
//! `sai_extmacsec.cpp` on the fake backend, through the SAI API: `sai-macsec/tests/cpp/driver.cpp`)
//! run the same scripts of SAI object operations over the same table. Per operation the SAI
//! status, the values read (packet numbers, counters, lists, capabilities, the inter-frame gap),
//! the diag status text (failure messages included), and every bus event in order must be the
//! same text: each read with the value read, each write with the value written (key words
//! included: this is where a wrong byte order of a key or a packet number would show), each
//! delay, each access the bus refused.
//!
//! Tables: a synthetic one made here (always), and the board's real one when
//! `SAI_MACSEC_E2E_TABLE` names it (the AS7772 table as generated for the system,
//! `sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_MACSEC_FILE`; read where it is,
//! never copied). The C++ side: `$SAI_RS_TEST_BIN/sai-macsec-cpp-chip-driver` (prebuilt by sai-macsec's
//! build.rs for a cross build), else compiled
//! here from `SAI_OPENBCM_SRC` (sai-openbcm's `src`) and `SAI_INC`; neither: SKIPPED (said on stderr).
//!
//! Scripts never move a flow between ports (variables of one parity only meet the port of that
//! parity): there the two differ on purpose (sai-macsec's D1).

#![allow(
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::format_collect,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    reason = "test code: a failed unwrap is a failed test"
)]

use phy_seq::{Bus, Dev, MacsecTable};
use proptest::prelude::*;
use sai_core::oid::{PortId, RawOid, TypedId};
use sai_macsec::{
    AclEntryId, AclField, AclParameter, AclTableId, Attr, Dir, FlowId, Macsec, MacsecId, MacsecPortId, Options,
    PortKey, SaId, ScId, Secret, SeqError, SwitchPort, Value,
};
use sai_macsec_phy::{BusStatus, IfgSink, PhySeqRunner, model_table};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// sai-macsec's driver.cpp built with -DSAI_MACSEC_DRIVER_CHIP (read-only overlay registers).
const DRIVER_NAME: &str = "sai-macsec-cpp-chip-driver";

/// A line for whoever runs the suite: on stderr, and appended to the file `$SAI_RS_TEST_SUMMARY`
/// names (the target-test bundle collects these; captured stderr of a passing test is not shown).
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}
/// The fake switch of the driver: 32 ports of 4 lanes (the AS7772 table names 32).
const PORTS: u32 = 32;
/// SAI_STATUS_FAILURE: what the C++ fake bus returns for a PHY marked dead.
const DEAD: i32 = -1;

// ------------------------------------------------------------------ the C++ side

const CPP: &[&str] = &[
    "sai_api",
    "sai_switch",
    "sai_port",
    "sai_l2",
    "sai_hostif",
    "sai_l3",
    "sai_lag",
    "sai_temp",
    "sai_caps",
    "sai_diag",
    "sai_acl",
    "sai_qos",
    "sai_mirror",
    "sai_hash",
    "sai_extphy",
    "sai_extmacsec",
    "sai_macsec",
    "fake_backend",
];

struct Driver {
    exe: PathBuf,
    dir: PathBuf,
}

fn scratch_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("sai-macsec-phy-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn driver() -> Option<&'static Driver> {
    static DRIVER: OnceLock<Option<Result<Driver, String>>> = OnceLock::new();
    let built = DRIVER.get_or_init(|| {
        if let Some(bin) = std::env::var_os("SAI_RS_TEST_BIN") {
            let exe = Path::new(&bin).join(DRIVER_NAME);
            if exe.is_file() {
                return Some(scratch_dir().map(|dir| Driver { exe, dir }));
            }
        }
        let src = ["SAI_OPENBCM_SRC", "MDIO_SCRIPT_C_DIR"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .find(|d| d.join("sai_macsec.cpp").is_file());
        let inc = std::env::var_os("SAI_INC").map(PathBuf::from);
        let driver_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sai-macsec/tests/cpp/driver.cpp");
        match (src, inc) {
            (Some(src), Some(inc)) if driver_src.is_file() => Some(build(&src, &inc, &driver_src)),
            _ => {
                summary(&format!(
                    "e2e: no $SAI_RS_TEST_BIN/{DRIVER_NAME}, and SAI_OPENBCM_SRC + SAI_INC + sai-macsec's driver.cpp are not all \
                     there: the end-to-end comparison with the C++ is SKIPPED"
                ));
                None
            }
        }
    });
    built.as_ref().map(|b| b.as_ref().unwrap_or_else(|e| panic!("{e}")))
}

fn build(src: &Path, inc: &Path, driver_src: &Path) -> Result<Driver, String> {
    let dir = scratch_dir()?;
    let cxx = std::env::var_os("CXX").unwrap_or_else(|| "c++".into());
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let includes = |c: &mut Command| {
        c.arg("-I")
            .arg(inc)
            .arg("-I")
            .arg(inc.join("../experimental"))
            .arg("-I")
            .arg(src);
    };
    let mut jobs = Vec::new();
    for unit in CPP {
        let mut c = Command::new(&cxx);
        c.args(["-std=c++17", "-O1", "-w", "-c"]);
        includes(&mut c);
        c.arg(src.join(format!("{unit}.cpp")))
            .arg("-o")
            .arg(dir.join(format!("{unit}.o")));
        jobs.push((
            unit.to_string(),
            c.spawn().map_err(|e| format!("{}: {e}", cxx.display()))?,
        ));
    }
    let mut c = Command::new(&cc);
    c.args(["-std=gnu11", "-O1", "-w", "-c", "-I"]).arg(src);
    c.arg(src.join("temp_files.c")).arg("-o").arg(dir.join("temp_files.o"));
    jobs.push((
        "temp_files".to_owned(),
        c.spawn().map_err(|e| format!("{}: {e}", cc.display()))?,
    ));
    for (unit, job) in jobs {
        if !job
            .wait_with_output()
            .map_err(|e| format!("{unit}: {e}"))?
            .status
            .success()
        {
            return Err(format!("{unit} does not compile"));
        }
    }
    let exe = dir.join(DRIVER_NAME);
    let mut link = Command::new(&cxx);
    link.args(["-std=c++17", "-O1", "-Wall", "-DSAI_MACSEC_DRIVER_CHIP"]);
    includes(&mut link);
    link.arg(driver_src);
    for unit in CPP.iter().chain(&["temp_files"]) {
        link.arg(dir.join(format!("{unit}.o")));
    }
    let out = link
        .arg("-o")
        .arg(&exe)
        .arg("-lpthread")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "the C++ driver does not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(Driver { exe, dir })
}

/// A second C++ driver (SAI_MACSEC_E2E_RUST_DRIVER, else `$SAI_RS_TEST_BIN/sai-macsec-rs-chip-driver`):
/// driver.cpp's chip variant built on the C++
/// library with the Rust model inside (sai_macsec_rust.cpp + libsai_macsec_capi.a): the rim
/// (attribute unions, statuses, lists, callbacks) against the C++ model through the SAI API.
fn rust_model_driver() -> Option<Driver> {
    let in_bin = std::env::var_os("SAI_RS_TEST_BIN")
        .map(|b| Path::new(&b).join("sai-macsec-rs-chip-driver"))
        .filter(|p| p.is_file());
    let exe = std::env::var_os("SAI_MACSEC_E2E_RUST_DRIVER")
        .map(PathBuf::from)
        .or(in_bin)?;
    let dir = scratch_dir().ok()?;
    Some(Driver { exe, dir })
}

impl Driver {
    fn output(&self, table: &Path, script: &str) -> String {
        static N: AtomicU64 = AtomicU64::new(0);
        let s = self.dir.join(format!("script-{}", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&s, script).unwrap();
        let out = Command::new(&self.exe)
            .arg(table)
            .arg(&s)
            .env("SAI_MACSEC_DRIVER_PORTS", PORTS.to_string())
            .env("SAI_MACSEC_DRIVER_DELAYS", "1")
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&s);
        assert!(
            out.status.success(),
            "driver failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

// ------------------------------------------------------------------ the Rust side

/// The C++ fake backend's MDIO bus (the driver's "chip" variant): a register file (a write is
/// read back, a register never written reads 0), registers whose reads see a set value and not
/// the writes (a FIFO), PHYs that can be marked dead (every access fails with FAILURE and is
/// not logged by the fake), plus the log in the driver's format.
#[derive(Default)]
struct RecBus {
    regs: BTreeMap<(u16, u8, u16), u16>,
    rd_only: BTreeMap<(u16, u8, u16), u16>,
    dead: BTreeSet<u16>,
    log: String,
}

impl Bus for RecBus {
    type Error = BusStatus;
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        if self.dead.contains(&phy) {
            write!(self.log, " !R{phy:x}.{:x}.{reg:x}", dev.get()).unwrap();
            return Err(BusStatus(DEAD));
        }
        let key = (phy, dev.get(), reg);
        let v = self
            .rd_only
            .get(&key)
            .or_else(|| self.regs.get(&key))
            .copied()
            .unwrap_or(0);
        write!(self.log, " R{phy:x}.{:x}.{reg:x}={v:x}", dev.get()).unwrap();
        Ok(v)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        if self.dead.contains(&phy) {
            write!(self.log, " !W{phy:x}.{:x}.{reg:x}", dev.get()).unwrap();
            return Err(BusStatus(DEAD));
        }
        self.regs.insert((phy, dev.get(), reg), value);
        write!(self.log, " W{phy:x}.{:x}.{reg:x}={value:x}", dev.get()).unwrap();
        Ok(())
    }
    fn delay_us(&mut self, us: u32) {
        write!(self.log, " D{us}").unwrap();
    }
}

/// The C++ `port_ifg_apply` on the fake backend without a board gap quirk: the MACsec gap,
/// else 96 bit times if the port had the MACsec gap, else nothing.
#[derive(Default)]
struct FakeIfg {
    ifg: BTreeMap<PortKey, u32>,
    macsec: BTreeMap<PortKey, bool>,
}

impl IfgSink for FakeIfg {
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError> {
        let had = self.macsec.insert(port, macsec_bits != 0).unwrap_or(false);
        let want = if macsec_bits != 0 {
            macsec_bits
        } else if had {
            96
        } else {
            0
        };
        if want != 0 {
            self.ifg.insert(port, want);
        }
        Ok(())
    }
}

fn switch_ports() -> Vec<SwitchPort> {
    (0..PORTS)
        .map(|i| SwitchPort {
            id: PortId::try_from(RawOid((u64::from(sys::SAI_OBJECT_TYPE_PORT) << 56) | u64::from(i + 1))).unwrap(),
            key: PortKey(i + 1),
            name: format!("ce{i}"),
            lanes: (0..4).map(|l| i * 4 + l + 1).collect(),
        })
        .collect()
}

struct World {
    m: Macsec,
    r: PhySeqRunner<RecBus, FakeIfg>,
    ports: Vec<SwitchPort>,
}

fn take(w: &mut World) -> String {
    std::mem::take(&mut w.r.bus_mut().log)
}

fn num(s: &str) -> u64 {
    u64::from_str_radix(s, 16).unwrap()
}
fn dir_of(s: &str) -> i32 {
    (if s == "1" {
        sys::SAI_MACSEC_DIRECTION_INGRESS
    } else {
        sys::SAI_MACSEC_DIRECTION_EGRESS
    }) as i32
}
fn a(id: sys::sai_attr_id_t, value: Value) -> Attr {
    Attr::new(id, value)
}
fn e(v: u32) -> Value {
    Value::Enum(v as i32)
}

struct Vars(BTreeMap<String, RawOid>);

impl Vars {
    fn get<T: TryFrom<RawOid>>(&self, name: &str) -> Option<T> {
        self.0
            .get(name)
            .map(|raw| T::try_from(*raw).ok().unwrap_or_else(|| panic!("{name}: another type")))
    }
    fn raw(&self, name: &str) -> Option<RawOid> {
        self.0.get(name).copied()
    }
    fn name_of(&self, raw: RawOid) -> String {
        if raw.is_null() {
            return "-".to_owned();
        }
        self.0
            .iter()
            .find(|(_, v)| **v == raw)
            .map_or_else(|| "?".to_owned(), |(k, _)| k.clone())
    }
    fn names(&self, v: &Value) -> String {
        let Value::OidList(list) = v else {
            panic!("not a list: {v:?}")
        };
        if list.is_empty() {
            return "none".to_owned();
        }
        list.iter().map(|o| self.name_of(*o)).collect::<Vec<_>>().join(",")
    }
}

/// The driver's key material for seed `seed` (driver.cpp, op "sa").
fn sa_attrs(dir: i32, sc: RawOid, an: u8, pn: u64, seed: u8) -> Vec<Attr> {
    let bytes = |base: u8, n: usize| -> Vec<u8> { (0..n).map(|i| base.wrapping_add(i as u8)).collect() };
    let pn_attr = if dir == sys::SAI_MACSEC_DIRECTION_EGRESS as i32 {
        sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN
    } else {
        sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
    };
    vec![
        a(sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, Value::Enum(dir)),
        a(sys::SAI_MACSEC_SA_ATTR_SC_ID, Value::Oid(sc)),
        a(sys::SAI_MACSEC_SA_ATTR_AN, Value::U8(an)),
        a(pn_attr, Value::U64(pn)),
        a(
            sys::SAI_MACSEC_SA_ATTR_SAK,
            Value::Sak(Secret::from_slice(&bytes(seed.wrapping_mul(37), 32)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::AuthKey(Secret::from_slice(&bytes(0x80u8.wrapping_add(seed), 16)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_SALT,
            Value::Salt(Secret::from_slice(&bytes(0x50u8.wrapping_add(seed), 12)).unwrap()),
        ),
        a(
            sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI,
            Value::U32(0x0102_0304 + u32::from(seed)),
        ),
    ]
}

fn action(id: sys::sai_attr_id_t, enable: bool, parameter: AclParameter) -> Attr {
    a(id, Value::AclAction { enable, parameter })
}

/// What the driver prints for the same table and script, from the Rust path.
fn rust_output(table_text: &[u8], script: &str) -> String {
    let table = MacsecTable::parse(table_text).unwrap();
    let ports = switch_ports();
    let mut r = PhySeqRunner::new(table.clone(), &ports, RecBus::default(), FakeIfg::default());
    let m = match Macsec::attach(&model_table(&table), &ports, Options::default(), &mut r) {
        Ok(m) => m,
        Err(err) => return format!("attach => {}\n", err.status()),
    };
    let mut w = World { m, r, ports };
    let mut out = String::new();
    writeln!(out, "attach => 0{}", take(&mut w)).unwrap();
    let mut vars = Vars(BTreeMap::new());
    for line in script.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let Some(op) = t.first().copied() else { continue };
        let mut extra = String::new();
        let status: Option<sai_core::Result<()>> = (|| {
            let creates = matches!(op, "macsec" | "mport" | "flow" | "table" | "entry" | "sc" | "sa");
            if creates && vars.0.contains_key(t[1]) {
                return None;
            }
            let created = |vars: &mut Vars, r: sai_core::Result<RawOid>| {
                r.map(|raw| {
                    vars.0.insert(t[1].to_owned(), raw);
                })
            };
            let port = |w: &World, i: &str| w.ports[usize::try_from(num(i)).unwrap()].clone();
            Some(match op {
                "macsec" => {
                    let r =
                        w.m.create_macsec(&[a(sys::SAI_MACSEC_ATTR_DIRECTION, Value::Enum(dir_of(t[2])))]);
                    created(&mut vars, r.map(Into::into))
                }
                "mport" => {
                    let p = port(&w, t[3]);
                    let attrs = [
                        a(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2]))),
                        a(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, Value::Oid(p.id.raw())),
                    ];
                    let r = w.m.create_macsec_port(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "flow" => {
                    let r =
                        w.m.create_flow(&[a(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2])))]);
                    created(&mut vars, r.map(Into::into))
                }
                "table" => {
                    let stage = if t[2] == "1" {
                        sys::SAI_ACL_STAGE_INGRESS_MACSEC
                    } else {
                        sys::SAI_ACL_STAGE_EGRESS_MACSEC
                    };
                    let r = w.m.create_acl_table(&[
                        a(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage)),
                        a(
                            sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                            Value::I32List(vec![sys::SAI_ACL_BIND_POINT_TYPE_PORT as i32]),
                        ),
                        a(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, Value::Bool(true)),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "bind" => {
                    let table = if t[3] == "-" { RawOid::NULL } else { vars.raw(t[3])? };
                    let p = port(&w, t[1]);
                    let dir = if t[2] == "1" { Dir::Ingress } else { Dir::Egress };
                    w.m.set_port_acl(&mut w.r, p.id, dir, table)
                }
                "entry" => {
                    let table = vars.raw(t[2])?;
                    let flow = if t[4] == "-" { None } else { Some(vars.raw(t[4])?) };
                    let mut attrs = vec![
                        a(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, Value::Oid(table)),
                        a(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, Value::U32(1)),
                        a(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(t[3] == "1")),
                    ];
                    if t[6] == "1" {
                        attrs.push(a(
                            sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE,
                            Value::AclField(Some(AclField::U16 {
                                data: 0x888e,
                                mask: 0xffff,
                            })),
                        ));
                    }
                    if t[5] != "0" {
                        let act = if t[5] == "1" {
                            sys::SAI_PACKET_ACTION_DROP
                        } else {
                            sys::SAI_PACKET_ACTION_FORWARD
                        };
                        attrs.push(action(
                            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                            true,
                            AclParameter::S32(act as i32),
                        ));
                    }
                    if let Some(flow) = flow {
                        attrs.push(action(
                            sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                            true,
                            AclParameter::Oid(flow),
                        ));
                    }
                    let r = w.m.create_acl_entry(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "eflow" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    let flow = if t[2] == "-" { RawOid::NULL } else { vars.raw(t[2])? };
                    let x = action(
                        sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
                        t[2] != "-",
                        AclParameter::Oid(flow),
                    );
                    w.m.set_acl_entry(&mut w.r, entry, &x)
                }
                "eadmin" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    w.m.set_acl_entry(
                        &mut w.r,
                        entry,
                        &a(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, Value::Bool(t[2] == "1")),
                    )
                }
                "sc" => {
                    let flow = vars.raw(t[3])?;
                    // the driver's SCI: these eight bytes in memory order, as a u64
                    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, num(t[7]) as u8]);
                    let r = w.m.create_sc(&[
                        a(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, Value::Enum(dir_of(t[2]))),
                        a(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, Value::Oid(flow)),
                        a(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, Value::U64(sci)),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                            Value::Enum(num(t[4]) as i32),
                        ),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE,
                            Value::Bool(t[5] == "1"),
                        ),
                        a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            Value::U32(num(t[6]) as u32),
                        ),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "scset" => {
                    let sc = vars.get::<ScId>(t[1])?;
                    let x = match t[2] {
                        "cipher" => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                            Value::Enum(num(t[3]) as i32),
                        ),
                        "replay" => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE,
                            Value::Bool(t[3] == "1"),
                        ),
                        _ => a(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            Value::U32(num(t[3]) as u32),
                        ),
                    };
                    w.m.set_sc(sc, &x)
                }
                "sa" => {
                    let sc = vars.raw(t[3])?;
                    let attrs = sa_attrs(dir_of(t[2]), sc, num(t[4]) as u8, num(t[5]), num(t[6]) as u8);
                    let r = w.m.create_sa(&mut w.r, &attrs);
                    created(&mut vars, r.map(Into::into))
                }
                "saminpn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.set_sa(
                        sa,
                        &a(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, Value::U64(num(t[2]))),
                    )
                }
                "sapn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.get_sa(&mut w.r, sa, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
                        .map(|v| {
                            let Value::U64(pn) = v[0] else { panic!() };
                            write!(extra, " pn={pn:x}").unwrap();
                        })
                }
                "sastat" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED,
                        sys::SAI_MACSEC_SA_STAT_IN_PKTS_OK,
                    ];
                    w.m.get_sa_stats(&mut w.r, sa, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "flowstat" => {
                    let flow = vars.get::<FlowId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS,
                        sys::SAI_MACSEC_FLOW_STAT_OTHER_ERR,
                    ];
                    w.m.get_flow_stats(&mut w.r, flow, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "saclr" => w.m.clear_sa_stats(&mut w.r, vars.get::<SaId>(t[1])?),
                "flowclr" => w.m.clear_flow_stats(&mut w.r, vars.get::<FlowId>(t[1])?),
                "active" => {
                    let sc = vars.get::<ScId>(t[1])?;
                    w.m.get_sc(sc, &[sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID]).map(|v| {
                        let Value::Oid(sa) = v[0] else { panic!() };
                        write!(extra, " active={}", vars.name_of(sa)).unwrap();
                    })
                }
                "lists" => {
                    let flow = vars.get::<FlowId>(t[1])?;
                    w.m.get_flow(
                        flow,
                        &[
                            sys::SAI_MACSEC_FLOW_ATTR_SC_LIST,
                            sys::SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST,
                        ],
                    )
                    .map(|v| write!(extra, " scs={} entries={}", vars.names(&v[0]), vars.names(&v[1])).unwrap())
                }
                "caps" => {
                    let m = vars.get::<MacsecId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED,
                        sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA,
                    ];
                    let sa =
                        w.m.stats_capability(&w.r, sys::SAI_OBJECT_TYPE_MACSEC_SA)
                            .unwrap()
                            .len();
                    let flow =
                        w.m.stats_capability(&w.r, sys::SAI_OBJECT_TYPE_MACSEC_FLOW)
                            .unwrap()
                            .len();
                    w.m.get_macsec(&w.r, m, &ids).map(|v| {
                        extra.push_str(" caps=");
                        for b in &v[..4] {
                            let Value::Bool(b) = b else { panic!() };
                            extra.push(if *b { '1' } else { '0' });
                        }
                        let Value::U32(n) = v[4] else { panic!() };
                        write!(extra, " sas={n} stats={sa},{flow}").unwrap();
                    })
                }
                "rm" => {
                    let r = match t[1] {
                        "macsec" => w.m.remove_macsec(vars.get::<MacsecId>(t[2])?),
                        "mport" => w.m.remove_macsec_port(&mut w.r, vars.get::<MacsecPortId>(t[2])?),
                        "flow" => w.m.remove_flow(vars.get::<FlowId>(t[2])?),
                        "sc" => w.m.remove_sc(vars.get::<ScId>(t[2])?),
                        "sa" => w.m.remove_sa(&mut w.r, vars.get::<SaId>(t[2])?),
                        "table" => w.m.remove_acl_table(vars.get::<AclTableId>(t[2])?),
                        _ => w.m.remove_acl_entry(&mut w.r, vars.get::<AclEntryId>(t[2])?),
                    };
                    if r != Err(sai_core::Error::ObjectInUse) {
                        vars.0.remove(t[2]);
                    }
                    r
                }
                "reg" => {
                    let key = (num(t[1]) as u16, 1u8, num(t[2]) as u16);
                    w.r.bus_mut().regs.insert(key, num(t[3]) as u16);
                    Ok(())
                }
                "rdreg" => {
                    let key = (num(t[1]) as u16, 1u8, num(t[2]) as u16);
                    if t[3] == "-" {
                        w.r.bus_mut().rd_only.remove(&key);
                    } else {
                        w.r.bus_mut().rd_only.insert(key, num(t[3]) as u16);
                    }
                    Ok(())
                }
                "dead" => {
                    let phy = num(t[1]) as u16;
                    if t[2] == "1" {
                        w.r.bus_mut().dead.insert(phy);
                    } else {
                        w.r.bus_mut().dead.remove(&phy);
                    }
                    Ok(())
                }
                "ifg" => {
                    let key = port(&w, t[1]).key;
                    write!(extra, " ifg={}", w.r.ifg().ifg.get(&key).copied().unwrap_or(0)).unwrap();
                    Ok(())
                }
                "status" => {
                    for l in w.m.status(&w.r).lines() {
                        write!(extra, "\n| {l}").unwrap();
                    }
                    Ok(())
                }
                other => panic!("unknown operation {other}"),
            })
        })();
        match status {
            None => writeln!(out, "{line} => skip").unwrap(),
            Some(st) => {
                let code = st.err().map_or(0, sai_core::Error::status);
                let extra = if code == 0 || op == "status" {
                    extra
                } else {
                    String::new()
                };
                writeln!(out, "{line} => {code}{extra}{}", take(&mut w)).unwrap();
            }
        }
        w.m.check_invariants()
            .unwrap_or_else(|why| panic!("after '{line}': {why}"));
    }
    out
}

/// The status text carries each engine's longest run time: wall clock, not comparable. It is
/// the 7 characters before column 76 of a port line (`%-8s %4x  %-7s %-6s %-6s %-3s x5 %5llu %5llu %7llu`).
fn normalise(text: &str) -> String {
    text.lines()
        .map(|l| {
            let body = l.strip_prefix("| ").unwrap_or("");
            let port_line =
                body.len() >= 76 && (body.get(15..22) == Some("egress ") || body.get(15..22) == Some("ingress"));
            if port_line && body.is_char_boundary(69) && body.is_char_boundary(76) {
                format!("| {}      ?{}\n", &body[..69], &body[76..])
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

static COMPARED: AtomicU64 = AtomicU64::new(0);
static EVENTS: AtomicU64 = AtomicU64::new(0);

fn compare(driver: &Driver, table: &Path, script: &str) -> Result<(), String> {
    let text = std::fs::read(table).unwrap();
    let (cpp, rust) = (
        normalise(&driver.output(table, script)),
        normalise(&rust_output(&text, script)),
    );
    if cpp == rust {
        COMPARED.fetch_add(
            cpp.lines().filter(|l| l.contains(" => ")).count() as u64,
            Ordering::Relaxed,
        );
        EVENTS.fetch_add(
            cpp.matches(" R").count() as u64 + cpp.matches(" W").count() as u64,
            Ordering::Relaxed,
        );
        return Ok(());
    }
    let at = cpp.lines().zip(rust.lines()).position(|(a, b)| a != b).unwrap_or(0);
    let show = |text: &str| -> String {
        text.lines()
            .skip(at.saturating_sub(3))
            .take(5)
            .map(|l| {
                if l.len() > 3000 {
                    format!("{}...\n", &l[..3000])
                } else {
                    format!("{l}\n")
                }
            })
            .collect()
    };
    Err(format!(
        "first difference at output line {}\n--- C++\n{}--- Rust\n{}--- script\n{script}",
        at + 1,
        show(&cpp),
        show(&rust)
    ))
}

// ------------------------------------------------------------------ tables and scripts

/// A synthetic table (no relation to any chip): every sequence the model runs, reads that
/// keep results and scratch values, polls, checks, delays, every parameter byte. Register
/// 1.20f / 1.21f must read 1 for an install to complete, 1.1fN = 0 for the other sequences.
fn synthetic_table() -> String {
    let mut t = String::from("ext-macsec 1\n# synthetic\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n");
    let ciphers = ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"];
    for (d, dn) in ["egress", "ingress"].iter().enumerate() {
        writeln!(t, "seq probe.{dn}\nX p 1 10{d} bypass[0]").unwrap();
        writeln!(t, "seq reset.{dn}\nE p 1 1f6 0\nW p 1 11{d} 1").unwrap();
        writeln!(t, "seq enable.{dn}\nE p 1 1f4 0\nW p 1 10{d} 0\nD 10").unwrap();
        writeln!(t, "seq disable.{dn}\nE p 1 1f1 0\nW p 1 10{d} 1").unwrap();
        writeln!(t, "seq gate-open.{dn}\nE p 1 1f3 0\nW p 1 12{d} 1").unwrap();
        writeln!(t, "seq gate-close.{dn}\nW p 1 12{d} 0").unwrap();
        writeln!(t, "seq stats-flow.{dn}\nE p 1 1f5 0\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 other_err[3]").unwrap();
        for an in 0..4 {
            for (c, cipher) in ciphers.iter().enumerate() {
                let (xpn, k256) = (c >= 2, c & 1 == 1);
                for first in ["", "-first"] {
                    writeln!(
                        t,
                        "seq sa-add{first}.{dn}.{an}.{cipher}\nR p 1 2{d}0\nW p 1 2{d}0 {:x}",
                        (c << 4) | an
                    )
                    .unwrap();
                    let klen = if k256 { 32 } else { 16 };
                    for i in (0..klen).step_by(2) {
                        writeln!(t, "W p 1 6{d}{:02x} key[{}].key[{i}]", i / 2, i + 1).unwrap();
                    }
                    for i in (0..16).step_by(2) {
                        writeln!(t, "W p 1 7{d}{:02x} hkey[{i}].hkey[{}]", i / 2, i + 1).unwrap();
                    }
                    writeln!(t, "W p 1 2{d}4 sci[0].sci[1]\nW p 1 2{d}5 sci[6].sci[7]\nW p 1 2{d}6 pn[1].pn[0]\nW p 1 2{d}7 pn[3].pn[2]").unwrap();
                    writeln!(t, "W p 1 2{d}8 window[1].window[0]\nW p 1 2{d}9 window[3].window[2]").unwrap();
                    if xpn {
                        writeln!(t, "W p 1 2{d}a pn[5].pn[4]\nW p 1 2{d}b pn[7].pn[6]").unwrap();
                        for i in (0..12).step_by(2) {
                            writeln!(t, "W p 1 8{d}{:02x} iv[{i}].salt[{}]", i / 2, i + 1).unwrap();
                        }
                        writeln!(t, "W p 1 2{d}c ssci[0].ssci[1]\nW p 1 2{d}d ssci[2].ssci[3]").unwrap();
                    }
                    writeln!(t, "X p 1 310 $0\nW p 1 310 $0&ff00|{:x}\nP p 1 2{d}f 1 1 3 7", an + 1).unwrap();
                }
            }
            writeln!(t, "seq sa-add-first.{dn}.{an}.gcm-aes-128.replay\nW p 1 2{d}0 f00\nW p 1 2{d}8 window[1].window[0]\nP p 1 2{d}f 1 1 2 0").unwrap();
            writeln!(
                t,
                "seq sa-del.{dn}.{an}\nE p 1 1f2 0\nW p 1 3{d}{an} 1\nseq sa-del-last.{dn}.{an}\nW p 1 3{d}{an} 8001"
            )
            .unwrap();
            writeln!(t, "seq pn.{dn}.{an}\nE p 1 1f5 0").unwrap();
            for k in 0..4 {
                writeln!(t, "X p 1 5{d}{} pn[{k}]", an * 4 + k).unwrap();
            }
            let stat = if d == 1 { "in_pkts_ok" } else { "out_pkts_encrypted" };
            writeln!(
                t,
                "seq stats-sa.{dn}.{an}\nX p 1 4{d}{} {stat}[0]\nX p 1 4{d}{} {stat}[1]",
                an * 2,
                an * 2 + 1
            )
            .unwrap();
        }
    }
    t
}

/// Board-specific knowledge the scripts need: the PHY address words of ports 0 and 1, and the
/// registers (and values) whose preset decides whether a sequence completes.
struct Board {
    phys: [&'static str; 2],
    head: String,
    regs: Vec<&'static str>,
    values: Vec<u16>,
    /// Groups of script lines a random operation may be (the read-back words of an install).
    groups: Vec<String>,
}

fn synthetic_board() -> Board {
    let head = "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\n".to_owned();
    Board {
        phys: ["10", "11"],
        head,
        regs: vec![
            "20f", "21f", "1f1", "1f2", "1f3", "1f4", "1f5", "1f6", "310", "400", "410", "4f0", "4f1", "4f2", "500",
            "501", "504", "510",
        ],
        values: vec![0, 1, 5, 0xab00, 0xffff],
        groups: vec!["status".to_owned()],
    }
}

/// The AS7772 table: ports 0 and 1 are cages with PHYs 0x17c / 0x17d (first lanes 1 and 5).
/// The LMI status (1.900a) and the message acknowledge (1.8221) are preset so that most
/// sequences complete, and the LMI data FIFO (1.9005) reads 0, not the last command word; the
/// scripts change these and the read-back words (1.a101, 1.a102) so that sequences also fail
/// at their checks.
fn as7772_board(table: &str) -> Board {
    let mut head = String::new();
    for phy in ["17c", "17d"] {
        writeln!(
            head,
            "reg {phy} 900a 7\nreg {phy} 8221 12e3\nreg {phy} a101 e066\nrdreg {phy} 9005 0"
        )
        .unwrap();
    }
    head.push_str("macsec m0 0\nmacsec m1 1\n");
    Board {
        phys: ["17c", "17d"],
        head,
        regs: vec![
            "900a", "8221", "a101", "a102", "a101", "a102", "9005", "a103", "a104", "a000",
        ],
        values: readback_values(table),
        groups: (0..2usize)
            .flat_map(|q| {
                (0..2usize).flat_map(move |d| (0..4usize).flat_map(move |an| (0..4usize).map(move |c| (q, d, an, c))))
            })
            .map(|(q, d, an, c)| readback(table, ["17c", "17d"][q], d, an, c).trim_end().to_owned())
            .collect(),
    }
}

/// The values the table's sequences check read-back words against (`E p 1 a101 / a102 ...`: the
/// PHY's answer to a message), plus a few others.
fn readback_values(table: &str) -> Vec<u16> {
    let mut v: BTreeSet<u16> = [0, 1, 7, 0x12e3, 0xffff].into_iter().collect();
    for l in table.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        if let ["E", "p", "1", "a101" | "a102", val, ..] = w.as_slice() {
            v.insert(u16::from_str_radix(val, 16).unwrap());
        }
    }
    v.into_iter().collect()
}

/// `reg` lines that make the read-back checks of the install sequence of (dir, an, cipher) hold
/// on PHY `phy` (what the chip would answer).
fn readback(table: &str, phy: &str, d: usize, an: usize, cipher: usize) -> String {
    let name = format!(
        "seq sa-add.{}.{an}.{}",
        ["egress", "ingress"][d],
        ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"][cipher]
    );
    let mut out = String::new();
    for l in table
        .lines()
        .skip_while(|l| *l != name)
        .skip(1)
        .take_while(|l| !l.starts_with("seq "))
    {
        let w: Vec<&str> = l.split_whitespace().collect();
        if let ["E", "p", "1", reg @ ("a101" | "a102"), val, ..] = w.as_slice() {
            writeln!(out, "reg {phy} {reg} {val}").unwrap();
        }
    }
    out
}

/// The backbones with the read-back words of each install set just before it.
fn as7772_backbones(table: &str, board: &Board) -> Vec<String> {
    let mut out = Vec::new();
    for cipher in 0..4 {
        let mut lines = Vec::new();
        for (q, d) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let x = q + 2 * d;
            for l in skeleton(q, d, cipher, d == 1) {
                let an = if l.starts_with(&format!("eflow e{x} ")) {
                    Some(0)
                } else if l.starts_with(&format!("sa a{} ", x + 4)) {
                    Some(1)
                } else if l.starts_with(&format!("sa a{} ", x + 8)) {
                    Some(2)
                } else {
                    None
                };
                if let Some(an) = an {
                    lines.push(readback(table, board.phys[q], d, an, cipher).trim_end().to_owned());
                }
                lines.push(l);
            }
        }
        lines.extend(teardown());
        out.push(format!("{}{}\n", board.head, lines.join("\n")));
    }
    out
}

/// MACsecOrch's order for port `q` (0, 1), direction `d`.
fn skeleton(q: usize, d: usize, cipher: usize, replay: bool) -> Vec<String> {
    let x = q + 2 * d;
    vec![
        format!("mport p{x} {d} {q}"),
        format!("flow f{x} {d}"),
        format!("table t{x} {d}"),
        format!("bind {q} {d} t{x}"),
        format!("entry e{x} t{x} 1 - 1 0"),
        format!("sc c{x} {d} f{x} {cipher} {} 1234 {x}", u8::from(replay)),
        format!("sa a{x} {d} c{x} 0 7 {x}"),
        format!("eflow e{x} f{x}"),
        format!("sa a{} {d} c{x} 1 1 {}", x + 4, x + 4),
        format!("sapn a{x}"),
        format!("sastat a{}", x + 4),
        format!("flowstat f{x}"),
        "status".to_owned(),
        format!("rm sa a{x}"),
        format!("sa a{} {d} c{x} 2 100000002 {}", x + 8, x + 8),
        format!("active c{x}"),
    ]
}

fn teardown() -> Vec<String> {
    let mut lines = Vec::new();
    for x in 0..4 {
        lines.push(format!("rm mport p{x}"));
        lines.push("status".to_owned());
        for v in [x, x + 4, x + 8] {
            lines.push(format!("rm sa a{v}"));
        }
        lines.extend([
            format!("rm sc c{x}"),
            format!("rm entry e{x}"),
            format!("bind {} {} -", x % 2, x / 2),
        ]);
        lines.extend([
            format!("rm table t{x}"),
            format!("rm flow f{x}"),
            format!("ifg {}", x % 2),
        ]);
    }
    lines.extend(["rm macsec m0", "rm macsec m1", "status"].map(str::to_owned));
    lines
}

/// One random operation; variables of index parity p only ever meet port p.
fn random_op(
    phys: [&'static str; 2],
    regs: Vec<&'static str>,
    values: Vec<u16>,
    groups: Vec<String>,
) -> impl Strategy<Value = String> {
    let d = 0..2usize;
    let reg = proptest::sample::select(regs);
    let value = prop_oneof![proptest::sample::select(values), any::<u16>()];
    prop_oneof![
        2 => (0..2usize, d.clone()).prop_map(|(v, d)| format!("macsec m{v} {d}")),
        4 => (0..5usize, d.clone(), 0..4usize).prop_map(|(v, d, q)| format!("mport p{v} {d} {q}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("flow f{v} {d}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("table t{v} {d}")),
        4 => (0..6usize, d.clone(), any::<bool>()).prop_map(|(k, d, unbind)| {
            if unbind { format!("bind {} {d} -", k % 2) } else { format!("bind {} {d} t{k}", k % 2) }
        }),
        4 => (0..6usize, 0..3usize, any::<bool>(), 0..4usize, 0..3usize, any::<bool>()).prop_map(
            |(i, k, admin, j, action, eapol)| {
                let (table, flow) = (2 * k + i % 2, 2 * (j % 3) + i % 2);
                let flow = if j == 3 { "-".to_owned() } else { format!("f{flow}") };
                format!("entry e{i} t{table} {} {flow} {action} {}", u8::from(admin), u8::from(eapol))
            }
        ),
        5 => (0..6usize, 0..4usize).prop_map(|(i, j)| {
            if j == 3 { format!("eflow e{i} -") } else { format!("eflow e{i} f{}", 2 * j + i % 2) }
        }),
        2 => (0..6usize, any::<bool>()).prop_map(|(i, b)| format!("eadmin e{i} {}", u8::from(b))),
        4 => (0..5usize, d.clone(), 0..6usize, 0..5usize, any::<bool>(), 0..3usize).prop_map(
            |(v, d, f, cipher, replay, win)| format!("sc c{v} {d} f{f} {cipher} {} {:x} {v}", u8::from(replay), win * 0x1234)
        ),
        3 => (0..5usize, 0..3usize, 0..5usize).prop_map(|(v, what, val)| match what {
            0 => format!("scset c{v} cipher {val}"),
            1 => format!("scset c{v} replay {}", val % 2),
            _ => format!("scset c{v} window {:x}", val * 0x1234),
        }),
        8 => (0..12usize, d.clone(), 0..5usize, 0..5usize, 0..4usize).prop_map(|(v, d, sc, an, pn)| {
            let pn = ["1", "7", "100000002", "fedcba9876543210"][pn];
            format!("sa a{v} {d} c{sc} {an} {pn} {v}")
        }),
        2 => (0..12usize, 0..3usize).prop_map(|(v, pn)| format!("saminpn a{v} {}", ["1", "63", "100000002"][pn])),
        3 => (0..12usize).prop_map(|v| format!("sapn a{v}")),
        3 => (0..12usize).prop_map(|v| format!("sastat a{v}")),
        1 => (0..12usize).prop_map(|v| format!("saclr a{v}")),
        3 => (0..6usize).prop_map(|v| format!("flowstat f{v}")),
        1 => (0..6usize).prop_map(|v| format!("flowclr f{v}")),
        2 => (0..5usize).prop_map(|v| format!("active c{v}")),
        2 => (0..6usize).prop_map(|v| format!("lists f{v}")),
        1 => (0..2usize).prop_map(|v| format!("caps m{v}")),
        3 => Just("status".to_owned()),
        1 => (0..4usize).prop_map(|q| format!("ifg {q}")),
        8 => (0..12usize).prop_map(|v| format!("rm sa a{v}")),
        3 => (0..5usize).prop_map(|v| format!("rm sc c{v}")),
        3 => (0..6usize).prop_map(|v| format!("rm flow f{v}")),
        3 => (0..6usize).prop_map(|v| format!("rm entry e{v}")),
        2 => (0..6usize).prop_map(|v| format!("rm table t{v}")),
        3 => (0..5usize).prop_map(|v| format!("rm mport p{v}")),
        1 => (0..2usize).prop_map(|v| format!("rm macsec m{v}")),
        8 => proptest::sample::select(groups),
        10 => (0..2usize, reg, value).prop_map(move |(p, reg, val)| format!("reg {} {reg} {val:x}", phys[p])),
        1 => (0..2usize, any::<bool>()).prop_map(move |(p, on)| format!("dead {} {}", phys[p], u8::from(on))),
        2 => (0..2usize, proptest::sample::select(vec!["0", "8000", "-"])).prop_map(move |(p, v)| format!("rdreg {} 9005 {v}", phys[p])),
    ]
}

fn script(board: &Board, max_random: usize) -> impl Strategy<Value = String> {
    let head = board.head.clone();
    (
        proptest::collection::vec((0..2usize, 0..2usize, 0..4usize, any::<bool>()), 1..4),
        proptest::collection::vec(
            (
                any::<prop::sample::Index>(),
                random_op(
                    board.phys,
                    board.regs.clone(),
                    board.values.clone(),
                    board.groups.clone(),
                ),
            ),
            0..max_random,
        ),
    )
        .prop_map(move |(backbones, random)| {
            let mut lines: Vec<String> = Vec::new();
            for (q, d, cipher, replay) in backbones {
                lines.extend(skeleton(q, d, cipher, replay));
            }
            for (at, op) in random {
                let at = at.index(lines.len() + 1);
                lines.insert(at, op);
            }
            lines.extend(["status".to_owned()]);
            format!("{head}{}\n", lines.join("\n"))
        })
}

fn cases(default: u32) -> u32 {
    std::env::var("SAI_MACSEC_E2E_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn backbones(board: &Board) -> Vec<String> {
    let mut out = Vec::new();
    for cipher in 0..4 {
        let mut lines = Vec::new();
        for (q, d) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            lines.extend(skeleton(q, d, cipher, d == 1));
        }
        lines.extend(teardown());
        out.push(format!("{}{}\n", board.head, lines.join("\n")));
    }
    out
}

fn synthetic_path(driver: &Driver) -> PathBuf {
    // one file per call: the tests run in parallel
    static N: AtomicU64 = AtomicU64::new(0);
    let p = driver
        .dir
        .join(format!("synthetic-{}.tbl", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&p, synthetic_table()).unwrap();
    p
}

fn report(what: &str) {
    summary(&format!(
        "e2e: {what}; so far in this run {} operations and {} bus accesses agree with the C++ (comparisons)",
        COMPARED.load(Ordering::Relaxed),
        EVENTS.load(Ordering::Relaxed)
    ));
}

fn run_random(driver: &Driver, table: &Path, board: &Board, n: u32, max_random: usize) {
    let config = ProptestConfig {
        cases: n,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    proptest!(config, |(s in script(board, max_random))| {
        compare(driver, table, &s).map_err(TestCaseError::fail)?;
    });
}

// ------------------------------------------------------------------ tests

fn compare_drivers(cpp: &Driver, rs: &Driver, table: &Path, script: &str) -> Result<(), String> {
    let (a, b) = (
        normalise(&cpp.output(table, script)),
        normalise(&rs.output(table, script)),
    );
    if a == b {
        COMPARED.fetch_add(
            a.lines().filter(|l| l.contains(" => ")).count() as u64,
            Ordering::Relaxed,
        );
        EVENTS.fetch_add(
            a.matches(" R").count() as u64 + a.matches(" W").count() as u64,
            Ordering::Relaxed,
        );
        return Ok(());
    }
    let at = a.lines().zip(b.lines()).position(|(x, y)| x != y).unwrap_or(0);
    let show = |t: &str| -> String {
        t.lines()
            .skip(at.saturating_sub(3))
            .take(5)
            .map(|l| format!("{}\n", l.get(..3000).unwrap_or(l)))
            .collect()
    };
    Err(format!(
        "first difference at output line {}\n--- C++\n{}--- C++ with the Rust model\n{}--- script\n{script}",
        at + 1,
        show(&a),
        show(&b)
    ))
}

/// The C++ library with the Rust model inside, against the C++ one, through the SAI API: the
/// same scripts on the synthetic table and (SAI_MACSEC_E2E_TABLE) the AS7772 one.
#[test]
fn the_library_with_the_rust_model_agrees_with_the_cpp() {
    let Some(rs) = rust_model_driver() else {
        summary(
            "e2e: SAI_MACSEC_E2E_RUST_DRIVER is not set: the C++ library with the Rust model is not compared (SKIPPED)",
        );
        return;
    };
    let Some(cpp) = driver() else { return };
    let mut tables = vec![(synthetic_path(cpp), synthetic_board())];
    if let Some(t) = std::env::var_os("SAI_MACSEC_E2E_TABLE").map(PathBuf::from) {
        let text = std::fs::read_to_string(&t).unwrap();
        tables.push((t, as7772_board(&text)));
    }
    let n = cases(32);
    for (table, board) in &tables {
        let text = std::fs::read_to_string(table).unwrap();
        let scripts = if board.phys[0] == "17c" {
            as7772_backbones(&text, board)
        } else {
            backbones(board)
        };
        for s in scripts {
            compare_drivers(cpp, &rs, table, &s).unwrap_or_else(|why| panic!("{why}"));
        }
        let config = ProptestConfig {
            cases: n,
            failure_persistence: None,
            ..ProptestConfig::default()
        };
        proptest!(config, |(s in script(board, 60))| {
            compare_drivers(cpp, &rs, table, &s).map_err(TestCaseError::fail)?;
        });
    }
    report(&format!(
        "the C++ library with the Rust model, {} tables x (4 backbones + {n} random scripts)",
        tables.len()
    ));
}

/// The Rust path alone, always: the backbone completes on the synthetic table, the key words
/// reach the bus in the table's byte order, and the model's invariants hold after each line.
#[test]
fn the_rust_path_runs_the_backbone() {
    let board = synthetic_board();
    let script = format!("{}{}\nstatus\n", board.head, skeleton(0, 0, 3, false).join("\n"));
    let out = rust_output(synthetic_table().as_bytes(), &script);
    assert!(!out.contains("skip"), "{out}");
    // sa a0 (seed 0): SAK bytes 0, 1, ... (seed * 37 + i); 256-bit XPN suite: key[1].key[0] = 0x0100
    let install = out.lines().find(|l| l.starts_with("eflow e0 f0 => 0")).unwrap();
    assert!(install.contains(" W10.1.6000=100 W10.1.6001=302"), "{install}");
    // pn 7: pn[1].pn[0] = 0x0007, salt 0x50.. with the SSCI 01 02 03 04 in iv[0..3]: iv[0] = 0x51
    assert!(install.contains(" W10.1.206=7 W10.1.207=0"), "{install}");
    assert!(install.contains(" W10.1.8000=5151"), "{install}");
    assert!(out.contains("active c0 => 0 active=a8"), "{out}");
}

#[test]
fn synthetic_backbones_agree_with_the_cpp() {
    let Some(driver) = driver() else { return };
    let (table, board) = (synthetic_path(driver), synthetic_board());
    for s in backbones(&board) {
        compare(driver, &table, &s).unwrap_or_else(|why| panic!("{why}"));
    }
    report("synthetic table, 4 backbone scripts");
}

#[test]
fn synthetic_random_scripts_agree_with_the_cpp() {
    let Some(driver) = driver() else { return };
    let (table, board) = (synthetic_path(driver), synthetic_board());
    let n = cases(64);
    run_random(driver, &table, &board, n, 80);
    report(&format!("synthetic table, {n} random scripts"));
}

/// The comparison is not vacuous: one key byte of difference is seen.
#[test]
fn the_comparison_notices_a_key_difference() {
    let Some(driver) = driver() else { return };
    let (table, board) = (synthetic_path(driver), synthetic_board());
    let script = format!("{}{}\n", board.head, skeleton(0, 0, 0, false).join("\n"));
    let other = script.replace("sa a4 0 c0 1 1 4", "sa a4 0 c0 1 1 5");
    let text = std::fs::read(&table).unwrap();
    assert_ne!(
        normalise(&driver.output(&table, &script)),
        normalise(&rust_output(&text, &other))
    );
}

/// The board's real table (SAI_MACSEC_E2E_TABLE): the backbones in every cipher suite.
#[test]
fn as7772_backbones_agree_with_the_cpp() {
    let Some(table) = std::env::var_os("SAI_MACSEC_E2E_TABLE").map(PathBuf::from) else {
        summary("e2e: SAI_MACSEC_E2E_TABLE is not set: the AS7772 table comparison is SKIPPED");
        return;
    };
    let Some(driver) = driver() else { return };
    let text = std::fs::read_to_string(&table).unwrap();
    let board = as7772_board(&text);
    let mut installs = 0;
    for s in as7772_backbones(&text, &board) {
        compare(driver, &table, &s).unwrap_or_else(|why| panic!("{why}"));
        // not vacuous: engines on, SAs installed (their key words on the bus), gates open
        let out = driver.output(&table, &s);
        installs += out.matches(" SA AN ").count() + out.lines().filter(|l| l.contains(" on     open   sa")).count();
    }
    assert!(installs >= 16, "the AS7772 backbones installed too little ({installs})");
    report(&format!(
        "AS7772 table, 4 backbone scripts ({installs} engine lines with an installed SA and an open gate)"
    ));
}

/// The board's real table: random scripts.
#[test]
fn as7772_random_scripts_agree_with_the_cpp() {
    let Some(table) = std::env::var_os("SAI_MACSEC_E2E_TABLE").map(PathBuf::from) else {
        summary("e2e: SAI_MACSEC_E2E_TABLE is not set: the AS7772 table comparison is SKIPPED");
        return;
    };
    let Some(driver) = driver() else { return };
    let board = as7772_board(&std::fs::read_to_string(&table).unwrap());
    let n = cases(48);
    run_random(driver, &table, &board, n, 60);
    report(&format!("AS7772 table, {n} random scripts"));
}

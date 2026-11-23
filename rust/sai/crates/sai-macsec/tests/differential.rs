//! Differential test against the C++ frontend (`sai_macsec.cpp` on its fake backend; frozen since
//! 2026-11-23 in `ai_re/nix/packages/sai-teralynx/frontend/src`, which the CS8210's libsai still
//! compiles: a live comparison, no golden).
//!
//! A script of MACsec object operations runs on both implementations over the same made-up
//! PHY table; per operation the SAI status, the values read and every access of the MDIO bus
//! (reads included, with the value read) must be the same text. The C++ side is
//! `tests/cpp/driver.cpp`, built from the unmodified C++ sources.
//!
//! - `$SAI_RS_TEST_BIN/sai-macsec-cpp-driver` and `...-fixed`: drivers built beforehand (a
//!   machine without a compiler; `build.rs`); else
//! - `SAI_OPENBCM_SRC` = the frozen frontend's
//!   `src` directory and `SAI_INC` = SAI's `inc`: the driver is compiled here (`$CXX`, `$CC`);
//! - neither: the comparison is SKIPPED, said on stderr, and the tests pass.
//!
//! Scripts are generated so that a flow never moves between ports: there the port differs on
//! purpose (crate documentation, D1; `tests/model.rs` pins the Rust behaviour).
//!
//! D9 and D10 (the owner's fixes of the C++): the Rust output must equal, to the end of every
//! script, that of a second driver built from the C++ with exactly those two edits
//! (`tests/cpp/listed_fixes.rs`, also `$SAI_RS_TEST_BIN/sai-macsec-cpp-driver-fixed`); where
//! that one differs from the unmodified C++, the script reached D9 / D10. No other difference
//! passes, and neither does a Rust that lacks D9 / D10 where the fixed C++ has them.
//!
//! The random scripts are deterministic per seed (`tests/common/seeded.rs`): `SAI_MACSEC_SEED`
//! (default random, printed), `SAI_MACSEC_DIFF_CASES` (default 300).

#![allow(
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::assert_is_empty,
    clippy::format_collect,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "test code: a failed unwrap is a failed test (clippy.toml exempts only #[test] functions, not their helpers)"
)]

mod common;
#[path = "cpp/listed_fixes.rs"]
mod listed_fixes;
#[path = "common/seeded.rs"]
mod seeded;

use common::{BusOp, World, action_oid, action_s32, boolean, en, ether_type, ms_table, oid, u8v, u32v, u64v};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use sai_core::oid::{RawOid, TypedId};
use sai_macsec::{AclEntryId, AclTableId, Attr, Dir, FlowId, MacsecId, MacsecPortId, SaId, ScId, Secret, Value};
use sai_sys as sys;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

const DRIVER_NAME: &str = "sai-macsec-cpp-driver";
/// The same driver on `sai_macsec.cpp` with D9 and D10 (`tests/cpp/listed_fixes.rs`).
const FIXED_NAME: &str = "sai-macsec-cpp-driver-fixed";

/// A line for whoever runs the suite: on stderr, and appended to the file `$SAI_RS_TEST_SUMMARY`
/// names (the target-test bundle collects these; captured stderr of a passing test is not shown).
fn summary_line(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

macro_rules! summary {
    ($($t:tt)*) => {
        summary_line(&format!($($t)*))
    };
}
/// The C++ sources of the library on its fake backend (the frontend's CORE; driver.cpp defines
/// `make_openbcm_backend`).
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
    /// The C++ with the owner's fixes (D9, D10).
    fixed: PathBuf,
    dir: PathBuf,
}

fn scratch_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(format!("sai-macsec-differential-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// The C++ driver, or `None` (said once on stderr) when there is neither a prebuilt one nor
/// the sources to build it from.
fn driver() -> Option<&'static Driver> {
    static DRIVER: OnceLock<Option<Result<Driver, String>>> = OnceLock::new();
    let built = DRIVER.get_or_init(|| {
        if let Some(bin) = std::env::var_os("SAI_RS_TEST_BIN") {
            let (exe, fixed) = (Path::new(&bin).join(DRIVER_NAME), Path::new(&bin).join(FIXED_NAME));
            if exe.is_file() && fixed.is_file() {
                return Some(scratch_dir().map(|dir| Driver { exe, fixed, dir }));
            }
        }
        let src = std::env::var_os("SAI_OPENBCM_SRC")
            .map(PathBuf::from)
            .filter(|d| d.join("sai_macsec.cpp").is_file());
        let inc = std::env::var_os("SAI_INC").map(PathBuf::from);
        let driver_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cpp/driver.cpp");
        match (src, inc) {
            (Some(src), Some(inc)) if driver_src.is_file() => Some(build(&src, &inc, &driver_src)),
            _ => {
                summary!(
                    "differential: no $SAI_RS_TEST_BIN/{DRIVER_NAME} + {FIXED_NAME}, and SAI_OPENBCM_SRC (sai-teralynx/frontend/src) + SAI_INC \
                     + tests/cpp/driver.cpp are not all there: the comparison with the C++ sai_macsec.cpp is SKIPPED"
                );
                None
            }
        }
    });
    built
        .as_ref()
        .map(|built| built.as_ref().unwrap_or_else(|e| panic!("{e}")))
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
    // every translation unit at once: the library is 18 files
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
            c.spawn()
                .map_err(|e| format!("cannot run the C++ compiler {}: {e}", cxx.display()))?,
        ));
    }
    // the owner's fixes: an edited copy of sai_macsec.cpp next to the objects
    let fixed_src = listed_fixes::write_fixed(src, &dir)?;
    let mut c = Command::new(&cxx);
    c.args(["-std=c++17", "-O1", "-w", "-c"]);
    includes(&mut c);
    c.arg(&fixed_src).arg("-o").arg(dir.join("sai_macsec_fixed.o"));
    jobs.push((
        "sai_macsec_fixed".to_owned(),
        c.spawn()
            .map_err(|e| format!("cannot run the C++ compiler {}: {e}", cxx.display()))?,
    ));
    let mut c = Command::new(&cc);
    c.args(["-std=gnu11", "-O1", "-w", "-c", "-I"]).arg(src);
    c.arg(src.join("temp_files.c")).arg("-o").arg(dir.join("temp_files.o"));
    jobs.push((
        "temp_files".to_owned(),
        c.spawn()
            .map_err(|e| format!("cannot run the C compiler {}: {e}", cc.display()))?,
    ));
    for (unit, job) in jobs {
        let out = job.wait_with_output().map_err(|e| format!("{unit}: {e}"))?;
        if !out.status.success() {
            return Err(format!("{unit} does not compile"));
        }
    }
    let (exe, fixed) = (dir.join(DRIVER_NAME), dir.join(FIXED_NAME));
    for (bin, macsec) in [(&exe, "sai_macsec"), (&fixed, "sai_macsec_fixed")] {
        let mut link = Command::new(&cxx);
        link.args(["-std=c++17", "-O1", "-Wall"]);
        includes(&mut link);
        link.arg(driver_src);
        for unit in CPP.iter().chain(&["temp_files"]) {
            let unit = if *unit == "sai_macsec" { macsec } else { unit };
            link.arg(dir.join(format!("{unit}.o")));
        }
        let out = link
            .arg("-o")
            .arg(bin)
            .arg("-lpthread")
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "the C++ driver does not compile:\n{}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    Ok(Driver { exe, fixed, dir })
}

impl Driver {
    fn cpp_output(&self, table: &str, script: &str) -> String {
        self.output(&self.exe, table, script)
    }

    /// The C++ with the owner's fixes (D9, D10).
    fn fixed_output(&self, table: &str, script: &str) -> String {
        self.output(&self.fixed, table, script)
    }

    fn output(&self, exe: &Path, table: &str, script: &str) -> String {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let (t, s) = (
            self.dir.join(format!("table-{n}")),
            self.dir.join(format!("script-{n}")),
        );
        std::fs::write(&t, table).unwrap();
        std::fs::write(&s, script).unwrap();
        let out = Command::new(exe).arg(&t).arg(&s).output().unwrap();
        let _ = (std::fs::remove_file(&t), std::fs::remove_file(&s));
        assert!(
            out.status.success(),
            "driver failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

// ------------------------------------------------------------------ the Rust side of the script

fn bus_take(w: &mut World) -> String {
    let mut s = String::new();
    for BusOp {
        write,
        phy,
        dev,
        reg,
        val,
    } in std::mem::take(&mut w.phy.bus)
    {
        write!(s, " {}{phy:x}.{dev:x}.{reg:x}={val:x}", if write { 'W' } else { 'R' }).unwrap();
    }
    w.phy.runs.clear();
    s
}

fn num(s: &str) -> u64 {
    u64::from_str_radix(s, 16).unwrap()
}

fn dir_of(s: &str) -> i32 {
    if s == "1" { common::ING } else { common::EGR }
}

fn e(v: u32) -> i32 {
    i32::try_from(v).unwrap()
}

struct Vars(BTreeMap<String, RawOid>);

impl Vars {
    fn get<T: TryFrom<RawOid>>(&self, name: &str) -> Option<T> {
        self.0.get(name).map(|raw| {
            T::try_from(*raw)
                .ok()
                .unwrap_or_else(|| panic!("{name} is of another type"))
        })
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

/// The driver's key material for seed `seed`.
fn sa_attrs(dir: i32, sc: RawOid, an: u8, pn: u64, seed: u8) -> Vec<Attr> {
    let bytes = |base: u8, n: usize| -> Vec<u8> { (0..n).map(|i| base.wrapping_add(i as u8)).collect() };
    let pn_attr = if dir == common::EGR {
        sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN
    } else {
        sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
    };
    vec![
        en(sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, dir),
        oid(sys::SAI_MACSEC_SA_ATTR_SC_ID, sc),
        u8v(sys::SAI_MACSEC_SA_ATTR_AN, an),
        u64v(pn_attr, pn),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SAK,
            Value::Sak(Secret::from_slice(&bytes(seed.wrapping_mul(37), 32)).unwrap()),
        ),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::AuthKey(Secret::from_slice(&bytes(0x80u8.wrapping_add(seed), 16)).unwrap()),
        ),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SALT,
            Value::Salt(Secret::from_slice(&bytes(0x50u8.wrapping_add(seed), 12)).unwrap()),
        ),
        u32v(sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI, 0x0102_0304 + u32::from(seed)),
    ]
}

/// What the C++ driver prints for the same table and script, from the Rust model.
fn rust_output(table: &str, script: &str) -> String {
    let mut out = String::new();
    let mut w = match World::new(table) {
        Ok(w) => w,
        Err(err) => return format!("attach => {}\n", err.status()),
    };
    writeln!(out, "attach => 0{}", bus_take(&mut w)).unwrap();
    let mut vars = Vars(BTreeMap::new());
    for line in script.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let Some(op) = t.first().copied() else { continue };
        let mut extra = String::new();
        // None: skipped (a variable the operation needs is not set, or the one it creates is)
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
            Some(match op {
                "macsec" => {
                    let r = w.m.create_macsec(&[en(sys::SAI_MACSEC_ATTR_DIRECTION, dir_of(t[2]))]);
                    created(&mut vars, r.map(Into::into))
                }
                "mport" => {
                    let port = w.port(usize::try_from(num(t[3])).unwrap());
                    let r = w.m.create_macsec_port(
                        &mut w.phy,
                        &[
                            en(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir_of(t[2])),
                            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port.raw()),
                        ],
                    );
                    created(&mut vars, r.map(Into::into))
                }
                "flow" => {
                    let r =
                        w.m.create_flow(&[en(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, dir_of(t[2]))]);
                    created(&mut vars, r.map(Into::into))
                }
                "table" => {
                    let stage = if t[2] == "1" {
                        sys::SAI_ACL_STAGE_INGRESS_MACSEC
                    } else {
                        sys::SAI_ACL_STAGE_EGRESS_MACSEC
                    };
                    let r = w.m.create_acl_table(&[
                        en(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage)),
                        Attr::new(
                            sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                            Value::I32List(vec![e(sys::SAI_ACL_BIND_POINT_TYPE_PORT)]),
                        ),
                        boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, true),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "bind" => {
                    let table = if t[3] == "-" { RawOid::NULL } else { vars.raw(t[3])? };
                    let port = w.port(usize::try_from(num(t[1])).unwrap());
                    let dir = if t[2] == "1" { Dir::Ingress } else { Dir::Egress };
                    w.m.set_port_acl(&mut w.phy, port, dir, table)
                }
                "entry" => {
                    let table = vars.raw(t[2])?;
                    let flow = if t[4] == "-" { None } else { Some(vars.raw(t[4])?) };
                    let mut a = vec![
                        oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, table),
                        u32v(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, 1),
                        boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, t[3] == "1"),
                    ];
                    if t[6] == "1" {
                        a.push(ether_type(0x888e));
                    }
                    if t[5] != "0" {
                        let action = if t[5] == "1" {
                            sys::SAI_PACKET_ACTION_DROP
                        } else {
                            sys::SAI_PACKET_ACTION_FORWARD
                        };
                        a.push(action_s32(
                            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
                            true,
                            e(action),
                        ));
                    }
                    if let Some(flow) = flow {
                        a.push(action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, true, flow));
                    }
                    let r = w.m.create_acl_entry(&mut w.phy, &a);
                    created(&mut vars, r.map(Into::into))
                }
                "eflow" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    let flow = if t[2] == "-" { RawOid::NULL } else { vars.raw(t[2])? };
                    let a = action_oid(sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW, t[2] != "-", flow);
                    w.m.set_acl_entry(&mut w.phy, entry, &a)
                }
                "eadmin" => {
                    let entry = vars.get::<AclEntryId>(t[1])?;
                    w.m.set_acl_entry(
                        &mut w.phy,
                        entry,
                        &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, t[2] == "1"),
                    )
                }
                "sc" => {
                    let flow = vars.raw(t[3])?;
                    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, num(t[7]) as u8]);
                    let r = w.m.create_sc(&[
                        en(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir_of(t[2])),
                        oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, flow),
                        u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, sci),
                        en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, num(t[4]) as i32),
                        boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, t[5] == "1"),
                        u32v(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            num(t[6]) as u32,
                        ),
                    ]);
                    created(&mut vars, r.map(Into::into))
                }
                "scset" => {
                    let sc = vars.get::<ScId>(t[1])?;
                    let a = match t[2] {
                        "cipher" => en(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, num(t[3]) as i32),
                        "replay" => boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, t[3] == "1"),
                        _ => u32v(
                            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW,
                            num(t[3]) as u32,
                        ),
                    };
                    w.m.set_sc(sc, &a)
                }
                "sa" => {
                    let sc = vars.raw(t[3])?;
                    let a = sa_attrs(dir_of(t[2]), sc, num(t[4]) as u8, num(t[5]), num(t[6]) as u8);
                    let r = w.m.create_sa(&mut w.phy, &a);
                    created(&mut vars, r.map(Into::into))
                }
                "saminpn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.set_sa(sa, &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, num(t[2])))
                }
                "sapn" => {
                    let sa = vars.get::<SaId>(t[1])?;
                    w.m.get_sa(&mut w.phy, sa, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
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
                    w.m.get_sa_stats(&mut w.phy, sa, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "flowstat" => {
                    let flow = vars.get::<FlowId>(t[1])?;
                    let ids = [
                        sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS,
                        sys::SAI_MACSEC_FLOW_STAT_OTHER_ERR,
                    ];
                    w.m.get_flow_stats(&mut w.phy, flow, &ids)
                        .map(|v| write!(extra, " stats={:x},{:x}", v[0], v[1]).unwrap())
                }
                "saclr" => w.m.clear_sa_stats(&mut w.phy, vars.get::<SaId>(t[1])?),
                "flowclr" => w.m.clear_flow_stats(&mut w.phy, vars.get::<FlowId>(t[1])?),
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
                        w.m.stats_capability(&w.phy, sys::SAI_OBJECT_TYPE_MACSEC_SA)
                            .unwrap()
                            .len();
                    let flow =
                        w.m.stats_capability(&w.phy, sys::SAI_OBJECT_TYPE_MACSEC_FLOW)
                            .unwrap()
                            .len();
                    w.m.get_macsec(&w.phy, m, &ids).map(|v| {
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
                        "macsec" => w.m.remove_macsec(vars.get(t[2])?),
                        "mport" => w.m.remove_macsec_port(&mut w.phy, vars.get::<MacsecPortId>(t[2])?),
                        "flow" => w.m.remove_flow(vars.get(t[2])?),
                        "sc" => w.m.remove_sc(vars.get(t[2])?),
                        "sa" => w.m.remove_sa(&mut w.phy, vars.get::<SaId>(t[2])?),
                        "table" => w.m.remove_acl_table(vars.get::<AclTableId>(t[2])?),
                        _ => w.m.remove_acl_entry(&mut w.phy, vars.get::<AclEntryId>(t[2])?),
                    };
                    if r != Err(sai_core::Error::ObjectInUse) {
                        vars.0.remove(t[2]);
                    }
                    r
                }
                "reg" => {
                    w.phy.reg(num(t[1]) as u32, num(t[2]) as u32, num(t[3]) as u16);
                    Ok(())
                }
                "ifg" => {
                    let key = w.key(usize::try_from(num(t[1])).unwrap());
                    write!(extra, " ifg={}", w.phy.ifg.get(&key).copied().unwrap_or(0)).unwrap();
                    Ok(())
                }
                "status" => {
                    for l in w.m.status(&w.phy).lines() {
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
                writeln!(out, "{line} => {code}{extra}{}", bus_take(&mut w)).unwrap();
            }
        }
        w.m.check_invariants()
            .unwrap_or_else(|why| panic!("after '{line}': {why}"));
    }
    out
}

/// The status text carries the longest run time of an engine: not comparable. The field has
/// a fixed place in a port line (`%-8s %4x  %-7s %-6s %-6s %-3s %-3s %-3s %-3s %-3s %5llu %5llu %7llu`).
fn normalise(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.starts_with("| ce") && l.len() >= 2 + 77 && l.is_char_boundary(2 + 70) && l.is_char_boundary(2 + 77) {
                format!("{}      ?{}\n", &l[..2 + 70], &l[2 + 77..])
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

/// Output lines of the C++ driver held against the Rust model's so far (one per operation).
static COMPARISONS: AtomicU64 = AtomicU64::new(0);
/// Scripts whose C++ with the owner's fixes differs from the unmodified C++ (and the Rust equals
/// the fixed one), by the first operation where the two C++ differ: a cipher change (D10) or any
/// other (D9: the flow operations, `reconcile`).
static D9_SEEN: AtomicU64 = AtomicU64::new(0);
static D10_SEEN: AtomicU64 = AtomicU64::new(0);

/// A difference the crate documents as a fix of the C++.
#[derive(Debug, PartialEq, Eq)]
enum Listed {
    /// Q7 fixed: a flow switch takes the old SC's SAs out.
    D9,
    /// Q10 fixed: no cipher suite change under waiting SAs.
    D10,
}

/// The first output line where `a` and `b` differ (0-based), if they do.
fn first_difference(a: &str, b: &str) -> Option<usize> {
    if a == b {
        return None;
    }
    Some(
        a.lines()
            .zip(b.lines())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.lines().count().min(b.lines().count())),
    )
}

/// Which listed fix the C++ with the fixes reaches first (its first line that differs from the
/// unmodified C++), or `None` when the two C++ agree.
fn listed_at(cpp: &str, fixed: &str) -> Option<(usize, Listed)> {
    let at = first_difference(cpp, fixed)?;
    let line = fixed.lines().nth(at).unwrap_or_default();
    let which = if line.starts_with("scset ") && line.contains(" cipher ") {
        Listed::D10
    } else {
        Listed::D9
    };
    Some((at, which))
}

/// The Rust model against the C++ (the owner's rule: every behaviour but D1, D9 and D10 is the
/// C++'s; scripts never move a flow, so D1 is not reached): the Rust output must equal, to the end
/// of the script, that of the C++ with exactly the D9 / D10 edits. Where that one differs from the
/// unmodified C++, the script reached a listed fix (returned). So a Rust output that differs from
/// the C++ in any other way fails, and so does one that equals the unmodified C++ where the fixed
/// C++ does not (the Rust would lack D9 / D10 there).
fn compare_listed(driver: &Driver, table: &str, script: &str) -> Result<Option<Listed>, String> {
    let (cpp, fixed, rust) = (
        normalise(&driver.cpp_output(table, script)),
        normalise(&driver.fixed_output(table, script)),
        normalise(&rust_output(table, script)),
    );
    classify(&cpp, &fixed, &rust).map_err(|why| format!("{why}\n--- script\n{script}"))
}

/// `compare_listed` on the normalised outputs of the C++, the C++ with D9 / D10, the Rust.
fn classify(cpp: &str, fixed: &str, rust: &str) -> Result<Option<Listed>, String> {
    let Some(at) = first_difference(fixed, rust) else {
        COMPARISONS.fetch_add(
            rust.lines().filter(|l| l.contains(" => ")).count() as u64,
            Ordering::Relaxed,
        );
        let listed = listed_at(cpp, fixed).map(|(_, which)| which);
        match listed {
            Some(Listed::D9) => D9_SEEN.fetch_add(1, Ordering::Relaxed),
            Some(Listed::D10) => D10_SEEN.fetch_add(1, Ordering::Relaxed),
            None => 0,
        };
        return Ok(listed);
    };
    let show = |text: &str, at: usize| {
        text.lines()
            .skip(at.saturating_sub(6))
            .take(9)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let plain = match first_difference(cpp, rust) {
        None => "it equals the unmodified C++: D9 / D10 missing in the Rust".to_owned(),
        Some(c) => format!("from the unmodified C++ at line {}", c + 1),
    };
    let fixes = listed_at(cpp, fixed).map_or_else(
        || "the two C++ agree".to_owned(),
        |(l, which)| format!("the two C++ first differ at line {} ({which:?})", l + 1),
    );
    Err(format!(
        "not a listed difference: the Rust differs from the C++ with D9 / D10 at output line {}, {plain}; {fixes}\n\
         --- C++ with D9 / D10\n{}\n--- Rust\n{}\n--- unmodified C++\n{}",
        at + 1,
        show(fixed, at),
        show(rust, at),
        show(cpp, at),
    ))
}

fn compare(driver: &Driver, table: &str, script: &str) -> Result<(), String> {
    compare_listed(driver, table, script).map(|_| ())
}

// ------------------------------------------------------------------ tables and scripts

/// The test table with a way to fail every kind of sequence from a script: an `E` on a
/// register that is 0 unless the script sets it (sa-add fails through its poll of 1.2?f).
fn fault_table(gates: bool, probe: bool, absolute: bool) -> String {
    let mut out = String::new();
    for line in ms_table(gates, probe).lines() {
        let line = if absolute && line == "counters clear-on-read" {
            "counters absolute"
        } else {
            line
        };
        out.push_str(line);
        out.push('\n');
        let Some(name) = line.strip_prefix("seq ") else {
            continue;
        };
        let reg = [
            ("enable.", "1f4"),
            ("disable.", "1f1"),
            ("sa-del", "1f2"),
            ("gate-", "1f3"),
            ("pn.", "1f5"),
            ("stats-", "1f5"),
            ("reset.", "1f6"),
        ]
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix));
        if let Some((_, reg)) = reg {
            writeln!(out, "E p 1 {reg} 0").unwrap();
        }
    }
    out
}

const HEAD: &str = "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\n";

/// MACsecOrch's order for port `q` (0, 1), direction `d`: the backbone random operations are
/// mixed into.
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

/// One random operation. Variables of index parity p only ever meet port p: a flow never
/// moves between ports (D1).
fn random_op() -> impl Strategy<Value = String> {
    let d = 0..2usize;
    let regs = prop_oneof![
        Just("20f"),
        Just("21f"),
        Just("1f1"),
        Just("1f2"),
        Just("1f3"),
        Just("1f4"),
        Just("1f5"),
        Just("1f6"),
        Just("310"),
        Just("400"),
        Just("401"),
        Just("402"),
        Just("403"),
        Just("410"),
        Just("412"),
        Just("4f0"),
        Just("4f1"),
        Just("4f2"),
        Just("500"),
        Just("501"),
        Just("502"),
        Just("503"),
        Just("504"),
        Just("505"),
        Just("510"),
        Just("511"),
        Just("512"),
        Just("514"),
        Just("508"),
    ];
    prop_oneof![
        2 => (0..2usize, d.clone()).prop_map(|(v, d)| format!("macsec m{v} {d}")),
        4 => (0..5usize, d.clone(), 0..4usize).prop_map(|(v, d, q)| format!("mport p{v} {d} {q}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("flow f{v} {d}")),
        3 => (0..6usize, d.clone()).prop_map(|(v, d)| format!("table t{v} {d}")),
        4 => (0..6usize, d.clone(), 0..3usize).prop_map(|(k, d, how)| match how {
            0 => format!("bind {} {d} -", k % 2),
            1 => format!("bind 2 {d} t{k}"), // a port without a MACsec PHY
            _ => format!("bind {} {d} t{k}", k % 2),
        }),
        4 => (0..6usize, 0..3usize, any::<bool>(), 0..4usize, 0..3usize, any::<bool>()).prop_map(
            |(i, k, admin, j, action, eapol)| {
                // table, entry and flow of one parity
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
            |(v, d, f, cipher, replay, win)| format!("sc c{v} {d} f{f} {cipher} {} {} {v}", u8::from(replay), win * 0x1234)
        ),
        3 => (0..5usize, 0..3usize, 0..5usize).prop_map(|(v, what, val)| match what {
            0 => format!("scset c{v} cipher {val}"),
            1 => format!("scset c{v} replay {}", val % 2),
            _ => format!("scset c{v} window {:x}", val * 0x1234),
        }),
        8 => (0..12usize, d.clone(), 0..5usize, 0..5usize, 0..3usize).prop_map(|(v, d, sc, an, pn)| {
            let pn = ["1", "7", "100000002"][pn];
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
        8 => (0..2usize, regs, prop_oneof![Just(0u16), Just(1), Just(5), Just(0xab00), any::<u16>()])
            .prop_map(|(phy, reg, val)| format!("reg 1{phy} {reg} {val:x}")),
    ]
}

/// A script: the backbones of some (port, direction) pairs in order, random operations in between.
fn script() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec((0..2usize, 0..2usize, 0..4usize, any::<bool>()), 1..4),
        proptest::collection::vec((any::<prop::sample::Index>(), random_op()), 0..70),
    )
        .prop_map(|(backbones, random)| {
            let mut lines: Vec<String> = Vec::new();
            for (q, d, cipher, replay) in backbones {
                lines.extend(skeleton(q, d, cipher, replay));
            }
            for (at, op) in random {
                let at = at.index(lines.len() + 1);
                lines.insert(at, op);
            }
            format!("{HEAD}{}\n", lines.join("\n"))
        })
}

// ------------------------------------------------------------------ tests

/// The Rust side alone, always: the interpreter of this file runs the backbone and the
/// model's invariants hold after every line (so the script format cannot rot unnoticed
/// where the C++ is not around).
#[test]
fn the_script_interpreter_runs_the_backbone() {
    let script = format!("{HEAD}{}\nstatus\n", skeleton(0, 0, 3, false).join("\n"));
    let out = rust_output(&fault_table(true, true, false), &script);
    assert!(out.contains("sa a0 0 c0 0 7 0 => 0\n"), "{out}"); // kept: its flow is not bound yet
    assert!(out.contains("eflow e0 f0 => 0 W10.1.200=130"), "{out}"); // installed as the first, then the gate
    assert!(out.contains("active c0 => 0 active=a8"), "{out}");
    assert!(!out.contains("skip"), "{out}");
}

/// MACsecOrch's order on every table variant, every (port, direction), every cipher.
#[test]
fn backbones_agree_with_the_cpp() {
    let Some(driver) = driver() else { return };
    let mut compared = 0;
    for (gates, probe, absolute) in [
        (true, true, false),
        (false, true, false),
        (true, false, true),
        (false, false, true),
    ] {
        let table = fault_table(gates, probe, absolute);
        for cipher in 0..4 {
            let mut lines = Vec::new();
            for (q, d) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                lines.extend(skeleton(q, d, cipher, d == 1));
            }
            // teardown, out of order on purpose, then in order
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
            lines.extend([
                "rm macsec m0".to_owned(),
                "rm macsec m1".to_owned(),
                "status".to_owned(),
            ]);
            let script = format!("{HEAD}{}\n", lines.join("\n"));
            compare(driver, &table, &script).unwrap_or_else(|why| panic!("{why}"));
            compared += 1;
        }
    }
    summary!(
        "differential: {compared} backbone scripts agree with the C++ ({} comparisons so far in this run)",
        COMPARISONS.load(Ordering::Relaxed)
    );
}

/// Sequences that fail, one kind at a time, at every point of the backbone.
#[test]
fn failing_sequences_agree_with_the_cpp() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, true, false);
    let mut compared = 0;
    for d in 0..2 {
        let backbone = skeleton(0, d, 3, true);
        for fault in ["20f", "21f", "1f1", "1f2", "1f3", "1f4", "1f5", "1f6"] {
            for at in 0..=backbone.len() {
                let mut lines = backbone.clone();
                lines.insert(at, format!("reg 10 {fault} 1"));
                lines.extend(
                    [
                        "status",
                        "rm sa a0",
                        "rm sa a4",
                        "rm sa a8",
                        "status",
                        "rm mport p0",
                        "status",
                    ]
                    .map(str::to_owned),
                );
                lines.push(format!("reg 10 {fault} {}", u8::from(fault.starts_with('2'))));
                lines.extend(skeleton(0, d, 1, false));
                lines.push("status".to_owned());
                let script = format!("{HEAD}{}\n", lines.join("\n"));
                compare(driver, &table, &script).unwrap_or_else(|why| panic!("{why}"));
                compared += 1;
            }
        }
    }
    summary!(
        "differential: {compared} scripts with a failing sequence agree with the C++ ({} comparisons so far in this run)",
        COMPARISONS.load(Ordering::Relaxed)
    );
}

/// The comparison is not vacuous: one association number changed on one side is noticed.
#[test]
fn the_comparison_notices_a_difference() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, true, false);
    let script = format!("{HEAD}{}\n", skeleton(0, 0, 3, false).join("\n"));
    let other = script.replace("sa a4 0 c0 1 1 4", "sa a4 0 c0 3 1 4");
    assert_ne!(script, other);
    assert_ne!(
        normalise(&driver.cpp_output(&table, &script)),
        normalise(&rust_output(&table, &other))
    );
    compare(driver, &table, &other).unwrap();
}

/// D9 of the crate documentation (Q7 fixed), against the real C++: the entry sends the port's
/// data to another flow while the first flow's SC has an SA in the PHY. The C++ leaves it in (and
/// the gate open); the Rust model closes the gate and removes it, then the new SC's SA goes in
/// on the same AN as the first of its SC. Up to that operation the two agree.
#[test]
fn d9_a_flow_switch_against_the_cpp() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, true, false);
    let script = format!(
        "{HEAD}mport p0 0 0
flow f0 0
flow f2 0
table t0 0
bind 0 0 t0
entry e0 t0 1 f0 1 0
         sc c0 0 f0 0 0 0 0
sc c2 0 f2 0 0 0 2
sa a0 0 c0 0 1 0
eflow e0 f2
sa a2 0 c2 0 1 2
status
"
    );
    let (cpp, rust) = (driver.cpp_output(&table, &script), rust_output(&table, &script));
    let line = |text: &str, start: &str| text.lines().find(|l| l.starts_with(start)).unwrap().to_owned();
    let until = |text: &str| {
        text.lines()
            .take_while(|l| !l.starts_with("eflow"))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    assert_eq!(until(&cpp), until(&rust));
    assert_eq!(line(&cpp, "eflow e0 f2"), "eflow e0 f2 => 0");
    assert_eq!(
        line(&rust, "eflow e0 f2"),
        "eflow e0 f2 => 0 R10.1.1f3=0 W10.1.120=0 R10.1.1f2=0 W10.1.300=8001"
    );
    // C++: AN 0 is the old SC's: INSUFFICIENT_RESOURCES; Rust: installed as the SC's first
    assert!(line(&cpp, "sa a2").starts_with(&format!(
        "sa a2 0 c2 0 1 2 => {}",
        sai_core::Error::InsufficientResources.status()
    )));
    assert!(
        line(&rust, "sa a2").contains("W10.1.200=100"),
        "{}",
        line(&rust, "sa a2")
    );
    // the C++ with the owner's fixes does what the Rust does, line for line
    let fixed = driver.fixed_output(&table, &script);
    assert_eq!(line(&fixed, "eflow e0 f2"), line(&rust, "eflow e0 f2"));
    assert_eq!(compare_listed(driver, &table, &script).unwrap(), Some(Listed::D9));
    summary!("differential: D9 shown against the C++ (flow switch: the old SC's SA taken out)");
}

/// D10 (Q10 fixed), against the real C++: an SC's cipher suite changes while an SA waits for its
/// flow. The C++ takes the change (and would install key bytes cut for the other suite); the
/// Rust model refuses it with OBJECT_IN_USE and later installs the SA with its own suite.
#[test]
fn d10_cipher_change_under_waiting_sas_against_the_cpp() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, true, false);
    let script = format!(
        "{HEAD}mport p0 0 0
flow f0 0
table t0 0
bind 0 0 t0
entry e0 t0 0 f0 1 0
sc c0 0 f0 1 0 0 0
         sa a0 0 c0 0 1 0
scset c0 cipher 0
eadmin e0 1
status
"
    );
    let (cpp, rust) = (driver.cpp_output(&table, &script), rust_output(&table, &script));
    let line = |text: &str, start: &str| text.lines().find(|l| l.starts_with(start)).unwrap().to_owned();
    assert_eq!(line(&cpp, "scset"), "scset c0 cipher 0 => 0");
    assert_eq!(
        line(&rust, "scset"),
        format!("scset c0 cipher 0 => {}", sai_core::Error::ObjectInUse.status())
    );
    // the C++ then installs the 256-bit SA as gcm-aes-128 (marker 100: cipher 0), Rust as gcm-aes-256 (110)
    assert!(
        line(&cpp, "eadmin").contains("W10.1.200=100"),
        "{}",
        line(&cpp, "eadmin")
    );
    assert!(
        line(&rust, "eadmin").contains("W10.1.200=110"),
        "{}",
        line(&rust, "eadmin")
    );
    assert_eq!(
        line(&driver.fixed_output(&table, &script), "scset"),
        line(&rust, "scset")
    );
    assert_eq!(compare_listed(driver, &table, &script).unwrap(), Some(Listed::D10));
    summary!("differential: D10 shown against the C++ (cipher change under a waiting SA refused)");
}

/// D1 of the crate documentation, shown against the real C++: once a flow moved to another
/// port, the C++ looks for an SA on "the port an SA of its SC was last installed on". Removing
/// the SA that is still in the first port's PHY then runs nothing and reports success: the key
/// stays in that PHY. The Rust model runs the removal on the port that holds the SA.
#[test]
fn where_a_flow_moved_the_cpp_leaves_the_key_in_the_phy() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, true, false);
    let script = format!(
        "{HEAD}mport p0 0 0\nmport p1 0 1\nflow f0 0\ntable t0 0\ntable t1 0\nbind 0 0 t0\nbind 1 0 t1\n\
         entry e0 t0 1 f0 1 0\nsc c0 0 f0 0 0 0 0\nsa a0 0 c0 0 1 0\neadmin e0 0\nentry e1 t1 1 f0 1 0\n\
         sa a1 0 c0 1 1 1\nrm sa a0\nstatus\n"
    );
    let (cpp, rust) = (driver.cpp_output(&table, &script), rust_output(&table, &script));
    let line = |text: &str, start: &str| text.lines().find(|l| l.starts_with(start)).unwrap().to_owned();
    // both installed a0 on PHY 0x10 and a1 on PHY 0x11
    assert_eq!(line(&cpp, "sa a0"), line(&rust, "sa a0"));
    assert!(line(&cpp, "sa a0").contains("W10.1.200=100"));
    assert_eq!(line(&cpp, "sa a1"), line(&rust, "sa a1"));
    assert!(line(&cpp, "sa a1").contains("W11.1.200=101"));
    // the removal
    assert_eq!(line(&cpp, "rm sa a0"), "rm sa a0 => 0");
    assert_eq!(line(&rust, "rm sa a0"), "rm sa a0 => 0 R10.1.1f2=0 W10.1.300=8001");
    // and what each believes PHY 0x10 holds afterwards (AN0 column)
    assert!(line(&cpp, "| ce0").contains("closed sa  -   -   -"), "{cpp}");
    assert!(line(&rust, "| ce0").contains("closed -   -   -   -"), "{rust}");
}

/// A table that names a lane no port starts at: both refuse to start with the same status.
#[test]
fn an_unknown_lane_agrees_with_the_cpp() {
    let Some(driver) = driver() else { return };
    compare(driver, "ext-macsec 1\nport 3 10\n", "status\n").unwrap_or_else(|why| panic!("{why}"));
}

/// Random scripts, every table variant: `SAI_MACSEC_DIFF_CASES` seeds (default 300; the soak:
/// 50000) from `SAI_MACSEC_SEED` (default: a random one, printed), each its own script
/// (`tests/common/seeded.rs`). Every seed runs; the failing ones are reported with their
/// minimal case and how to replay them.
#[test]
fn random_scripts_agree_with_the_cpp() {
    let Some(driver) = driver() else { return };
    let strategy = (script(), (any::<bool>(), any::<bool>(), any::<bool>()));
    let run = seeded::run(
        seeded::first_seed(),
        seeded::cases("SAI_MACSEC_DIFF_CASES", 300),
        &strategy,
        |(script, (gates, probe, absolute))| {
            compare(driver, &fault_table(gates, probe, absolute), &script).map_err(TestCaseError::fail)
        },
    );
    summary!(
        "differential: random scripts, {}: {} failing; D9 reached {}, D10 {} (equal to the C++ with those fixes); {} operations compared",
        run.seeds(),
        run.failed.len(),
        D9_SEEN.load(Ordering::Relaxed),
        D10_SEEN.load(Ordering::Relaxed),
        COMPARISONS.load(Ordering::Relaxed)
    );
    run.assert_ok("random_scripts_agree_with_the_cpp", "SAI_MACSEC_DIFF_CASES");
}

/// The same scripts on the Rust model alone (this runs where the C++ is not around): no panic,
/// the invariants hold after every line.
#[test]
fn random_scripts_keep_the_invariants() {
    let strategy = (script(), (any::<bool>(), any::<bool>(), any::<bool>()));
    let run = seeded::run(
        seeded::first_seed(),
        seeded::cases("SAI_MACSEC_DIFF_CASES", 300),
        &strategy,
        |(script, (gates, probe, absolute))| {
            let out = rust_output(&fault_table(gates, probe, absolute), &script);
            prop_assert!(out.starts_with("attach => 0"));
            Ok(())
        },
    );
    run.assert_ok("random_scripts_keep_the_invariants", "SAI_MACSEC_DIFF_CASES");
}

/// The seeds are what they say: one seed, one script, whatever ran before.
#[test]
fn a_seed_replays_its_script() {
    let strategy = (script(), (any::<bool>(), any::<bool>(), any::<bool>()));
    let (a, b, other) = (
        seeded::value(&strategy, 7),
        seeded::value(&strategy, 7),
        seeded::value(&strategy, 8),
    );
    assert_eq!(a, b);
    assert_ne!(a, other);
}

/// Found by `random_scripts_agree_with_the_cpp` (2026-11-23): a D9 flow switch whose SA removal
/// fails at its first step (the fault table's check of `sa-del`, register 1f2 set to 1): Rust's
/// only extra bus operation is that failing read, which is D9 too.
#[test]
fn a_d9_flow_switch_whose_sa_removal_fails_at_once() {
    let Some(driver) = driver() else { return };
    let before = D9_SEEN.load(Ordering::Relaxed);
    compare(driver, &fault_table(false, false, false), "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\nmport p1 0 1\nflow f1 0\ntable t1 0\nbind 1 0 t1\nentry e1 t1 1 - 1 0\nsc c1 0 f1 0 0 1234 1\nsa a1 0 c1 0 7 1\neflow e1 f1\nsa a5 0 c1 1 1 5\nrm entry e1\nflow f3 0\nentry e3 t1 1 - 0 0\nreg 11 1f2 1\nsapn a1\nsastat a5\nflowstat f1\nstatus\nrm sa a1\nsa a9 0 c1 2 100000002 9\nactive c1\nmport p0 0 0\nflow f0 0\ntable t0 0\nbind 0 0 t0\nentry e0 t0 1 - 1 0\neflow e3 f3\nsc c0 0 f0 0 0 1234 0\nsa a0 0 c0 0 7 0\neflow e0 f0\nsa a4 0 c0 1 1 4\nsapn a0\nsastat a4\nflowstat f0\nstatus\nrm sa a0\nsa a8 0 c0 2 100000002 8\nactive c0\n").unwrap();
    assert!(D9_SEEN.load(Ordering::Relaxed) > before, "the script reaches D9");
}

/// Found by `random_scripts_agree_with_the_cpp` (G2 tier run, 2026-11-23; proptest's minimal
/// case, table `fault_table(true, false, false)`). Port 1 ingress: SA a3 of SC c3 (flow f3) is
/// installed through entry e3, e3 is disabled (no bound flow: the gate closes, a3 stays, as in
/// the C++), then a second entry e1 of the same table sends the port's data to flow f5, which
/// has no SC. The C++ (`reconcile`, :287-312) installs nothing for f5 and opens the gate
/// because a3 is still installed (:311 counts every installed SA): the port accepts data
/// through an SC no enabled entry selects (Q7). The Rust model takes a3 out (D9: the bound flow
/// is another flow than a3's SC's) and the gate stays closed. D9 as listed, not a model defect:
/// the C++ with the D9 edit does the same, to the end of the script.
#[test]
fn d9_another_entry_takes_over_after_the_entry_was_disabled() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, false, false);
    let script = "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\nflow f5 1\ntable t0 0\n\
                  mport p3 1 1\nflow f3 1\ntable t3 1\nbind 1 1 t3\nentry e3 t3 1 - 1 0\nsc c3 1 f3 0 0 1234 3\n\
                  sa a3 1 c3 0 7 3\neflow e3 f3\neadmin e3 0\nsa a7 1 c3 1 1 7\nentry e1 t3 1 f5 0 0\nsapn a3\nsastat a7\n\
                  flowstat f3\nstatus\nrm sa a3\nsa a11 1 c3 2 100000002 11\nactive c3\n";
    let (cpp, rust) = (driver.cpp_output(&table, script), rust_output(&table, script));
    let line = |text: &str, start: &str| text.lines().find(|l| l.starts_with(start)).unwrap().to_owned();
    assert_eq!(line(&cpp, "eadmin e3 0"), "eadmin e3 0 => 0 R11.1.1f3=0 W11.1.121=0");
    assert_eq!(line(&rust, "eadmin e3 0"), line(&cpp, "eadmin e3 0"));
    // the C++ opens the gate on the old SC's SA; the Rust takes that SA out (the last: sa-del-last)
    assert_eq!(
        line(&cpp, "entry e1"),
        "entry e1 t3 1 f5 0 0 => 0 R11.1.1f3=0 W11.1.121=1"
    );
    assert_eq!(
        line(&rust, "entry e1"),
        "entry e1 t3 1 f5 0 0 => 0 R11.1.1f2=0 W11.1.310=8001"
    );
    assert_eq!(compare_listed(driver, &table, script).unwrap(), Some(Listed::D9));
}

/// The classifier is not a blanket skip: the Rust output of the D9 script above with one
/// association number changed after the D9 operation is neither the C++ nor the C++ with the
/// fixes, and is refused; so is a difference at a line before it.
#[test]
fn a_difference_beside_a_listed_one_is_refused() {
    let Some(driver) = driver() else { return };
    let table = fault_table(true, false, false);
    let script = "reg 10 20f 1\nreg 10 21f 1\nreg 11 20f 1\nreg 11 21f 1\nmacsec m0 0\nmacsec m1 1\nflow f5 1\nmport p3 1 1\n\
                  flow f3 1\ntable t3 1\nbind 1 1 t3\nentry e3 t3 1 - 1 0\nsc c3 1 f3 0 0 1234 3\nsa a3 1 c3 0 7 3\n\
                  eflow e3 f3\neadmin e3 0\nsa a7 1 c3 1 1 7\nentry e1 t3 1 f5 0 0\nstatus\nrm entry e1\neadmin e3 1\nstatus\n";
    let (cpp, fixed, rust) = (
        normalise(&driver.cpp_output(&table, script)),
        normalise(&driver.fixed_output(&table, script)),
        normalise(&rust_output(&table, script)),
    );
    assert_eq!(classify(&cpp, &fixed, &rust), Ok(Some(Listed::D9)));
    // after the D9 operation: a7 installed on another AN (one bus word changed)
    let after = rust.replacen("W11.1.210=101", "W11.1.210=102", 1);
    assert_ne!(after, rust, "{rust}");
    assert!(classify(&cpp, &fixed, &after).is_err());
    // before it: the gate of eflow e3 f3
    let before = rust.replacen("W11.1.121=1", "W11.1.121=0", 1);
    assert!(classify(&cpp, &fixed, &before).is_err());
    // and the Rust without D9 there (the unmodified C++'s output) is refused as well
    assert!(classify(&cpp, &fixed, &cpp).is_err());
}

//! The `MACsec` objects through the SAI core (`sai-openbcm/tests/test_macsec.inc`, the checks that
//! go through the SAI API), natively: the core's seam with `sai-macsec-phy`'s model registered as
//! the rim registers it, over the virtual backend (the C++ fake's numbering), with the made-up
//! PHY table of `tests/test_tables.h`. Plus: the fixes D9 / D10 at SAI level, the two designed
//! clear-window keys both ways, failure injection, the call log.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::doc_markdown,
    clippy::write_with_newline,
    missing_docs
)]
use sai_core::Error;
use sai_core::backend::types::{MdioDev, PhyAddr};
use sai_core::backend::{HwPort, Mode, Profile, Recording};
use sai_core::error::AttrError;
use sai_core::extphy::{self, ManualClock};
use sai_core::macsec::{self, MacsecObject, Secret, StatsMode};
use sai_core::oid::{RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{AclAction, AclField, AclFieldData, AclKind, AclParam, Attr, FieldMatch, Value};
use sai_core::virt::VirtualBackend;
use sai_sys as sys;
use std::fmt::Write as _;
use std::sync::Arc;

const PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_PORT;
const SWITCH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SWITCH;
const ACL_TABLE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_TABLE;
const ACL_ENTRY: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ACL_ENTRY;
const EGR: i32 = sys::SAI_MACSEC_DIRECTION_EGRESS.cast_signed();
const ING: i32 = sys::SAI_MACSEC_DIRECTION_INGRESS.cast_signed();

/// `ms_table()` of `tests/test_tables.h`.
fn ms_table(gates: bool, probe: bool) -> String {
    let mut t =
        String::from("ext-macsec 1\n# test table\ncounters clear-on-read\nifg-bits 384\nport 1 10\nport 5 11\n");
    let ciphers = ["gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256"];
    for (d, dn) in ["egress", "ingress"].iter().enumerate() {
        if probe {
            write!(t, "seq probe.{dn}\nX p 1 10{d} bypass[0]\n").unwrap();
        }
        write!(
            t,
            "seq reset.{dn}\nW p 1 11{d} 1\nseq enable.{dn}\nW p 1 10{d} 0\nD 10\nseq disable.{dn}\nW p 1 10{d} 1\n"
        )
        .unwrap();
        if gates {
            write!(
                t,
                "seq gate-open.{dn}\nW p 1 12{d} 1\nseq gate-close.{dn}\nW p 1 12{d} 0\n"
            )
            .unwrap();
        }
        write!(
            t,
            "seq stats-flow.{dn}\nX p 1 4f0 control_pkts[0]\nX p 1 4f1 control_pkts[1]\nX p 1 4f2 x_unknown[0]\n"
        )
        .unwrap();
        for an in 0..4usize {
            for (c, cipher) in ciphers.iter().enumerate() {
                let (xpn, k256) = (c >= 2, c & 1 == 1);
                for first in 0..2usize {
                    let marker = (first << 8) | (c << 4) | an;
                    write!(
                        t,
                        "seq sa-add{}.{dn}.{an}.{cipher}\nW p 1 2{d}0 {marker:x}\n",
                        if first == 1 { "-first" } else { "" }
                    )
                    .unwrap();
                    write!(
                        t,
                        "W p 1 2{d}1 key[1].key[0]\nW p 1 2{d}2 key[{}].key[{}]\nW p 1 2{d}3 hkey[15].hkey[0]\n",
                        if k256 { 31 } else { 15 },
                        if k256 { 30 } else { 14 }
                    )
                    .unwrap();
                    write!(
                        t,
                        "W p 1 2{d}4 sci[0].sci[7]\nW p 1 2{d}5 pn[1].pn[0]\nW p 1 2{d}6 {}\nW p 1 2{d}7 window[1].window[0]\n",
                        if xpn { "pn[5].pn[4]" } else { "0" }
                    )
                    .unwrap();
                    if xpn {
                        write!(
                            t,
                            "W p 1 2{d}8 iv[0].iv[3]\nW p 1 2{d}9 iv[4].iv[11]\nW p 1 2{d}a ssci[0].salt[0]\n"
                        )
                        .unwrap();
                    }
                    if d == 1 {
                        write!(t, "X p 1 310 $0\nW p 1 310 $0&ff00|{}\n", an + 1).unwrap();
                    }
                    write!(t, "P p 1 2{d}f 1 1 3 0\n").unwrap();
                }
            }
            if d == 1 && an == 0 {
                t.push_str("seq sa-add-first.ingress.0.gcm-aes-128.replay\nW p 1 210 f00\nW p 1 217 window[1].window[0]\nP p 1 21f 1 1 3 0\n");
            }
            write!(
                t,
                "seq sa-del.{dn}.{an}\nW p 1 3{d}{an} 1\nseq sa-del-last.{dn}.{an}\nW p 1 3{d}{an} 8001\n"
            )
            .unwrap();
            write!(
                t,
                "seq pn.{dn}.{an}\nX p 1 5{d}{} pn[0]\nX p 1 5{d}{} pn[1]\nX p 1 5{d}{} pn[2]\nX p 1 5{d}{} pn[3]\n",
                an * 4,
                an * 4 + 1,
                an * 4 + 2,
                an * 4 + 3
            )
            .unwrap();
            let stat = if d == 1 { "in_pkts_ok" } else { "out_pkts_encrypted" };
            write!(
                t,
                "seq stats-sa.{dn}.{an}\nX p 1 4{d}{} {stat}[0]\nX p 1 4{d}{} {stat}[1]\n",
                an * 2,
                an * 2 + 1
            )
            .unwrap();
        }
    }
    t
}

fn table_file(text: &str) -> String {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, Into::into);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("sai-core-macsec-{}-{n}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

fn profile(table: Option<&str>, extra: &[(&str, &str)]) -> Profile {
    let mut pairs: Vec<(String, String)> = [
        ("SAI_OPENBCM_BACKEND", "fake"),
        ("SAI_OPENBCM_FAKE_PORTS", "4"),
        ("SAI_OPENBCM_FAKE_LANES", "4"),
        ("SAI_OPENBCM_FAKE_SPEED", "100000"),
    ]
    .iter()
    .chain(extra)
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    if let Some(t) = table {
        pairs.push(("SAI_OPENBCM_EXT_MACSEC_FILE".to_owned(), t.to_owned()));
    }
    Profile::from_pairs(pairs)
}

type Core = Sai<Recording<VirtualBackend>>;

fn create(sai: &mut Core, p: &Profile) -> sai_core::Result<RawOid> {
    macsec::register_loader(&sai_macsec_phy::native::LOADER);
    sai.create_switch(&[Attr::new(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))], p)
        .map(TypedId::raw)
}

fn core() -> Core {
    Sai::new(Recording::new(VirtualBackend::new(), Mode::Normal))
}

struct Rig {
    sai: Core,
    ports: Vec<RawOid>,
}

fn rig(text: &str, extra: &[(&str, &str)]) -> Rig {
    let table = table_file(text);
    let mut sai = core();
    let switch = create(&mut sai, &profile(Some(&table), extra)).unwrap();
    std::fs::remove_file(&table).unwrap();
    let ports = match sai
        .get(SWITCH, switch, &[sys::SAI_SWITCH_ATTR_PORT_LIST])
        .unwrap()
        .remove(0)
    {
        Value::OidList(l) => l,
        other => panic!("{other:?}"),
    };
    Rig { sai, ports }
}

fn s32(id: sys::sai_attr_id_t, v: i32) -> Attr {
    Attr::new(id, Value::Enum(v))
}
fn oid(id: sys::sai_attr_id_t, v: RawOid) -> Attr {
    Attr::new(id, Value::Oid(v))
}
fn boolean(id: sys::sai_attr_id_t, v: bool) -> Attr {
    Attr::new(id, Value::Bool(v))
}
fn u64v(id: sys::sai_attr_id_t, v: u64) -> Attr {
    Attr::new(id, Value::U64(v))
}
fn u32v(id: sys::sai_attr_id_t, v: u32) -> Attr {
    Attr::new(id, Value::U32(v))
}
fn e(v: u32) -> i32 {
    v.cast_signed()
}

/// The test key: bytes 0xc0.. so they are easy to find where they must not be.
fn sa_attrs(dir: i32, sc: RawOid, an: u8, k256: bool, xpn: bool, pn: u64) -> Vec<Attr> {
    let mut sak = [0u8; 32];
    for (i, b) in sak.iter_mut().enumerate() {
        if k256 || i >= 16 {
            *b = 0xc0 + (if k256 { i } else { i - 16 }) as u8 + an;
        }
    }
    let mut auth = [0u8; 16];
    for (i, b) in auth.iter_mut().enumerate() {
        *b = 0x80 + i as u8;
    }
    let mut v = vec![
        s32(sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION, dir),
        oid(sys::SAI_MACSEC_SA_ATTR_SC_ID, sc),
        Attr::new(sys::SAI_MACSEC_SA_ATTR_AN, Value::U8(an)),
        u64v(
            if dir == EGR {
                sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN
            } else {
                sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
            },
            pn,
        ),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SAK,
            Value::MacsecSak(Secret::from_slice(&sak).unwrap()),
        ),
        Attr::new(
            sys::SAI_MACSEC_SA_ATTR_AUTH_KEY,
            Value::MacsecAuthKey(Secret::from_slice(&auth).unwrap()),
        ),
    ];
    if xpn {
        let salt: Vec<u8> = (0..12u8).map(|i| 0x50 + i).collect();
        v.push(Attr::new(
            sys::SAI_MACSEC_SA_ATTR_SALT,
            Value::MacsecSalt(Secret::from_slice(&salt).unwrap()),
        ));
        v.push(u32v(sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI, 0x0102_0304));
    }
    v
}

fn action_flow(flow: Option<RawOid>) -> Attr {
    Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW,
        Value::AclAction(flow.map_or(AclAction::Disabled(AclKind::Oid), |f| {
            AclAction::Enabled(AclParam::Oid(f))
        })),
    )
}
fn action_packet(enable: bool, action: u32) -> Attr {
    Attr::new(
        sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION,
        Value::AclAction(if enable {
            AclAction::Enabled(AclParam::Enum(e(action)))
        } else {
            AclAction::Disabled(AclKind::Enum)
        }),
    )
}

impl Rig {
    fn sw(&self) -> RawOid {
        self.sai.switch_id().unwrap().raw()
    }
    fn reg(&self, phy: u32, reg: u16, v: u16) {
        let mdio = self.sai.backend().inner().mdio();
        mdio.lock()
            .unwrap()
            .regs
            .insert((PhyAddr(phy), MdioDev::new(1).unwrap(), reg), v);
    }
    /// `reg=val` of the writes since the last call (the C++ `MS_BUS`).
    fn bus(&self) -> String {
        let mdio = self.sai.backend().inner().mdio();
        let mut m = mdio.lock().unwrap();
        let out: Vec<String> = m
            .log
            .iter()
            .filter(|o| o.write)
            .map(|o| format!("{:03x}={:x}", o.reg, o.val))
            .collect();
        m.log.clear();
        out.join(" ")
    }
    fn ifg(&self, hw: u16) -> u32 {
        self.sai.backend().inner().port_state(HwPort(hw)).unwrap().ifg
    }
    fn quirk_calls(&self, hw: u16) -> u32 {
        self.sai.backend().inner().port_state(HwPort(hw)).unwrap().quirk_calls
    }
    fn mk(&mut self, object: MacsecObject, attrs: &[Attr]) -> sai_core::Result<RawOid> {
        self.sai.macsec_create(object, attrs)
    }
    fn status(&self) -> String {
        macsec::ext_macsec_status(&self.sai)
    }
    fn acl_table(&mut self, ingress: bool) -> RawOid {
        let stage = if ingress {
            sys::SAI_ACL_STAGE_INGRESS_MACSEC
        } else {
            sys::SAI_ACL_STAGE_EGRESS_MACSEC
        };
        let sw = self.sw();
        self.sai
            .create(
                ACL_TABLE,
                sw,
                &[
                    s32(sys::SAI_ACL_TABLE_ATTR_ACL_STAGE, e(stage)),
                    Attr::new(
                        sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST,
                        Value::I32List(vec![e(sys::SAI_ACL_BIND_POINT_TYPE_PORT)]),
                    ),
                    boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC, true),
                    boolean(sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE, true),
                ],
            )
            .unwrap()
    }
    fn acl_entry(&mut self, table: RawOid, priority: u32, more: Vec<Attr>) -> RawOid {
        let mut attrs = vec![
            oid(sys::SAI_ACL_ENTRY_ATTR_TABLE_ID, table),
            u32v(sys::SAI_ACL_ENTRY_ATTR_PRIORITY, priority),
            boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, true),
        ];
        attrs.extend(more);
        let sw = self.sw();
        self.sai.create(ACL_ENTRY, sw, &attrs).unwrap()
    }
    fn stat(&mut self, object: MacsecObject, id: RawOid, stat: u32) -> u64 {
        self.sai.macsec_stats(object, id, &[stat], StatsMode::Read).unwrap()[0]
    }
}

#[test]
fn a_bad_table_refuses_the_switch_and_no_table_says_not_supported() {
    let mut sai = core();
    assert_eq!(
        create(&mut sai, &profile(Some("/nonexistent/ext-macsec.tbl"), &[])),
        Err(Error::Failure)
    );
    let bad = table_file("ext-macsec 1\nseq a\nW p 0 1 1\n");
    assert_eq!(
        create(&mut sai, &profile(Some(&bad), &[])),
        Err(Error::InvalidParameter)
    );
    let lane = table_file("ext-macsec 1\nport 3 10\n");
    assert_eq!(
        create(&mut sai, &profile(Some(&lane), &[])),
        Err(Error::InvalidParameter)
    );
    assert!(sai.switch_id().is_none() && !sai.ext_macsec_active());
    for f in [bad, lane] {
        std::fs::remove_file(f).unwrap();
    }
    // no table: the API is there and says so
    create(&mut sai, &profile(None, &[])).unwrap();
    assert!(!sai.ext_macsec_active());
    assert_eq!(
        sai.macsec_create(MacsecObject::Macsec, &[s32(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)]),
        Err(Error::NotSupported)
    );
    assert!(macsec::ext_macsec_status(&sai).contains("no external MACsec table"));
}

#[test]
fn macsec_as_macsecorch_drives_it() {
    let mut r = rig(&ms_table(true, true), &[("SAI_OPENBCM_PORT_IFG", "128")]);
    assert!(r.sai.ext_macsec_active());
    // every engine read 0 = "not in bypass": all four were reset and disabled
    assert_eq!(r.bus(), "110=1 100=1 111=1 101=1 110=1 100=1 111=1 101=1");
    let (p0, p2) = (r.ports[0], r.ports[2]);
    assert_eq!(r.ifg(1), 128);
    for phy in [0x10, 0x11] {
        for reg in [0x20f, 0x21f] {
            r.reg(phy, reg, 1);
        }
    }
    // MACSEC objects
    let m_e = r
        .mk(
            MacsecObject::Macsec,
            &[
                s32(sys::SAI_MACSEC_ATTR_DIRECTION, EGR),
                boolean(sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true),
            ],
        )
        .unwrap();
    let m_i = r
        .mk(
            MacsecObject::Macsec,
            &[
                s32(sys::SAI_MACSEC_ATTR_DIRECTION, ING),
                boolean(sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, true),
            ],
        )
        .unwrap();
    assert_eq!(
        r.mk(MacsecObject::Macsec, &[s32(sys::SAI_MACSEC_ATTR_DIRECTION, ING)]),
        Err(Error::ItemAlreadyExists)
    );
    assert_eq!(
        r.mk(
            MacsecObject::Macsec,
            &[
                s32(sys::SAI_MACSEC_ATTR_DIRECTION, ING),
                boolean(sys::SAI_MACSEC_ATTR_ENABLE_POST, true)
            ]
        ),
        Err(Error::attr(AttrError::NotSupported, 1))
    );
    let get = |r: &mut Rig, o, id, a| r.sai.macsec_get(o, id, &[a]).unwrap().remove(0);
    assert_eq!(
        get(
            &mut r,
            MacsecObject::Macsec,
            m_i,
            sys::SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL
        ),
        Value::Bool(false)
    );
    assert_eq!(
        get(
            &mut r,
            MacsecObject::Macsec,
            m_e,
            sys::SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC
        ),
        Value::Enum(e(sys::SAI_MACSEC_MAX_SECURE_ASSOCIATIONS_PER_SC_FOUR))
    );
    assert_eq!(
        get(
            &mut r,
            MacsecObject::Macsec,
            m_e,
            sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED
        ),
        Value::Bool(true)
    );

    // MACSEC_PORT: the engine comes out of bypass (after a reset), the inter-frame gap widens
    let mport = |dir, port| {
        vec![
            s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, dir),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, port),
        ]
    };
    assert_eq!(r.mk(MacsecObject::Port, &mport(EGR, p2)), Err(Error::NotSupported)); // not in the table
    let qc0 = r.quirk_calls(1);
    let mp_e = r.mk(MacsecObject::Port, &mport(EGR, p0)).unwrap();
    assert_eq!(r.bus(), "110=1 100=0");
    assert_eq!(r.ifg(1), 384);
    assert_eq!(r.quirk_calls(1), qc0 + 1); // the gap alone
    let mp_i = r.mk(MacsecObject::Port, &mport(ING, p0)).unwrap();
    assert_eq!(r.bus(), "111=1 101=0");
    assert_eq!(r.mk(MacsecObject::Port, &mport(ING, p0)), Err(Error::ItemAlreadyExists));
    // the gap survives what re-applies the port quirks (a speed set)
    r.sai.set(PORT, p0, &u32v(sys::SAI_PORT_ATTR_SPEED, 100_000)).unwrap();
    assert_eq!(r.ifg(1), 384);
    r.bus();

    // flows, the MACsec ACL tables with MACsecOrch's entries, bound to the port: nothing reaches the PHY
    let f_e = r
        .mk(
            MacsecObject::Flow,
            &[s32(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)],
        )
        .unwrap();
    let f_i = r
        .mk(
            MacsecObject::Flow,
            &[s32(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, ING)],
        )
        .unwrap();
    let fp_before = r.sai.backend().inner().fp().groups.len();
    let mut tbl = [RawOid::NULL; 2];
    let mut eapol = [RawOid::NULL; 2];
    let mut data = [RawOid::NULL; 2];
    for d in 0..2 {
        tbl[d] = r.acl_table(d == 1);
        let et = Attr::new(
            sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE,
            Value::AclField(AclField::Enabled(AclFieldData::U16(FieldMatch {
                data: 0x888e,
                mask: 0xffff,
            }))),
        );
        eapol[d] = r.acl_entry(
            tbl[d],
            1000,
            vec![et, action_packet(true, sys::SAI_PACKET_ACTION_FORWARD)],
        );
        let attr = if d == 1 {
            sys::SAI_PORT_ATTR_INGRESS_MACSEC_ACL
        } else {
            sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL
        };
        r.sai.set(PORT, p0, &oid(attr, tbl[d])).unwrap();
        assert_eq!(r.sai.get(PORT, p0, &[attr]).unwrap()[0], Value::Oid(tbl[d]));
    }
    assert_eq!(r.sai.backend().inner().fp().groups.len(), fp_before);
    assert_eq!(r.bus(), "");
    assert_eq!(
        r.sai.set(PORT, p0, &oid(sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL, tbl[1])),
        Err(Error::attr(AttrError::InvalidValue, 0))
    );
    // SCs (the SCI as SONiC passes it: bytes in memory order)
    let sci = u64::from_ne_bytes([0x1c, 0xea, 0x0b, 0x38, 0x2b, 0xd5, 0x00, 0x01]);
    let sc_attrs = |dir: i32, flow: RawOid, cipher: u32| {
        vec![
            s32(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, dir),
            oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, flow),
            u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, sci),
            boolean(sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, true),
            boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE, true),
            s32(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(cipher)),
        ]
    };
    let mut integrity = sc_attrs(EGR, f_e, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128);
    integrity[3] = boolean(sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE, false);
    assert_eq!(
        r.mk(MacsecObject::Sc, &integrity),
        Err(Error::attr(AttrError::NotSupported, 3))
    );
    assert_eq!(
        r.mk(
            MacsecObject::Sc,
            &sc_attrs(EGR, f_i, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)
        ),
        Err(Error::InvalidParameter)
    );
    let sc_e = r
        .mk(
            MacsecObject::Sc,
            &sc_attrs(EGR, f_e, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128),
        )
        .unwrap();
    assert_eq!(
        r.mk(
            MacsecObject::Sc,
            &sc_attrs(EGR, f_e, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)
        ),
        Err(Error::InsufficientResources)
    );
    let mut vi = sc_attrs(ING, f_i, sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_XPN_256);
    vi.push(boolean(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE, true));
    vi.push(u32v(sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW, 0x1234));
    let sc_i = r.mk(MacsecObject::Sc, &vi).unwrap();
    for d in 0..2 {
        data[d] = r.acl_entry(tbl[d], 1, vec![action_packet(true, sys::SAI_PACKET_ACTION_DROP)]);
    }
    assert_eq!(r.bus(), "");

    // an SA before its flow is bound: kept, nothing on the bus
    let sa_e0 = r
        .mk(MacsecObject::Sa, &sa_attrs(EGR, sc_e, 0, false, false, 7))
        .unwrap();
    assert_eq!(r.bus(), "");
    assert!(r.status().contains("waiting for their flow"));
    assert_eq!(
        get(&mut r, MacsecObject::Sa, sa_e0, sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN),
        Value::U64(7)
    );
    // the flow becomes active: the SA goes in as the SC's first, then the gate opens
    r.sai.set(ACL_ENTRY, data[0], &action_flow(Some(f_e))).unwrap();
    r.sai
        .set(ACL_ENTRY, data[0], &action_packet(false, sys::SAI_PACKET_ACTION_DROP))
        .unwrap();
    assert_eq!(
        r.bus(),
        "200=100 201=c1c0 202=cfce 203=8f80 204=1c01 205=7 206=0 207=0 120=1"
    );
    assert!(!r.status().contains("waiting"));
    // rekey
    let sa_e1 = r
        .mk(MacsecObject::Sa, &sa_attrs(EGR, sc_e, 1, false, false, 1))
        .unwrap();
    assert_eq!(r.bus(), "200=1 201=c2c1 202=d0cf 203=8f80 204=1c01 205=1 206=0 207=0");
    assert_eq!(
        get(
            &mut r,
            MacsecObject::Sc,
            sc_e,
            sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID
        ),
        Value::Oid(sa_e1)
    );
    assert_eq!(
        r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc_e, 1, false, false, 1)),
        Err(Error::ItemAlreadyExists)
    );
    r.sai.macsec_remove(MacsecObject::Sa, sa_e0).unwrap();
    assert_eq!(r.bus(), "300=1");
    // counters: clear-on-read accumulated; PN from the PHY (32 bit for a non-XPN suite)
    r.reg(0x10, 0x402, 5);
    assert_eq!(
        r.stat(MacsecObject::Sa, sa_e1, sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED),
        5
    );
    r.reg(0x10, 0x402, 3);
    r.reg(0x10, 0x403, 1);
    assert_eq!(
        r.stat(MacsecObject::Sa, sa_e1, sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED),
        5 + 3 + 0x10000
    );
    assert_eq!(
        r.stat(MacsecObject::Sa, sa_e1, sys::SAI_MACSEC_SA_STAT_OCTETS_ENCRYPTED),
        0
    );
    r.reg(0x10, 0x4f0, 9);
    assert_eq!(
        r.stat(MacsecObject::Flow, f_e, sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS),
        9
    );
    assert_eq!(
        r.stat(MacsecObject::Flow, f_e, sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS),
        18
    );
    r.reg(0x10, 0x504, 0x2222);
    r.reg(0x10, 0x505, 0x1111);
    r.reg(0x10, 0x506, 0xffff);
    assert_eq!(
        get(&mut r, MacsecObject::Sa, sa_e1, sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN),
        Value::U64(0x1111_2222)
    );
    assert_eq!(
        r.sai
            .macsec_get(MacsecObject::Sa, sa_e1, &[sys::SAI_MACSEC_SA_ATTR_SAK]),
        Err(Error::attr(AttrError::NotSupported, 0))
    );
    let caps =
        sai_core::caps::stats_capability(Some(r.sai.backend()), r.sai.store(), sys::SAI_OBJECT_TYPE_MACSEC_SA).unwrap();
    assert_eq!(caps.len(), 2); // out_pkts_encrypted, in_pkts_ok: what this table reads
    r.bus();

    // ingress, flow active first: XPN-256 with replay protection, the SA goes in at once
    r.sai.set(ACL_ENTRY, data[1], &action_flow(Some(f_i))).unwrap();
    assert_eq!(r.bus(), "");
    r.reg(0x10, 0x310, 0xab00);
    let sa_i2 = r
        .mk(MacsecObject::Sa, &sa_attrs(ING, sc_i, 2, true, true, 0x1_0000_0002))
        .unwrap();
    assert_eq!(
        r.bus(),
        "210=132 211=c3c2 212=e1e0 213=8f80 214=1c01 215=2 216=1 217=1234 218=5157 219=545b 21a=150 310=ab03 121=1"
    );
    let sa_i3 = r.mk(MacsecObject::Sa, &sa_attrs(ING, sc_i, 3, true, true, 1)).unwrap();
    assert_eq!(
        r.bus(),
        "210=33 211=c4c3 212=e2e1 213=8f80 214=1c01 215=1 216=0 217=1234 218=5157 219=545b 21a=150 310=ab04"
    );
    assert_eq!(
        r.sai.macsec_set(
            MacsecObject::Sc,
            sc_i,
            &s32(
                sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                e(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128)
            )
        ),
        Err(Error::ObjectInUse)
    );
    r.sai
        .macsec_set(
            MacsecObject::Sa,
            sa_i3,
            &u64v(sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN, 99),
        )
        .unwrap();
    assert_eq!(r.bus(), "");
    // a sequence that fails: the SA is not created
    r.reg(0x10, 0x21f, 0);
    assert_eq!(
        r.mk(MacsecObject::Sa, &sa_attrs(ING, sc_i, 0, true, true, 1)),
        Err(Error::Failure)
    );
    assert!(r.status().contains("sa-add.ingress.0.gcm-aes-xpn-256"));
    r.reg(0x10, 0x21f, 1);
    r.bus();
    // the key reached the bus and nothing else: not the status text
    let st = r.status();
    for k in [
        "c0c1", "c1c0", "c2c3", "c3c2", "8081", "8f80", "5051", "5157", "c0 c1", "c0:c1", "0xc0",
    ] {
        assert!(!st.contains(k), "{k} in the status");
    }

    // removal: the transmitting egress SA is also the last: gate closed first, then the "last" sequence
    r.sai.macsec_remove(MacsecObject::Sa, sa_e1).unwrap();
    assert_eq!(r.bus(), "120=0 301=8001");
    r.sai.macsec_remove(MacsecObject::Sa, sa_i2).unwrap();
    assert_eq!(r.bus(), "312=1");
    r.sai.macsec_remove(MacsecObject::Sa, sa_i3).unwrap();
    assert_eq!(r.bus(), "121=0 313=8001");
    let sa_e2 = r
        .mk(MacsecObject::Sa, &sa_attrs(EGR, sc_e, 2, false, false, 1))
        .unwrap();
    assert_eq!(
        r.bus(),
        "200=102 201=c3c2 202=d1d0 203=8f80 204=1c01 205=1 206=0 207=0 120=1"
    );
    // the flow goes inactive: the gate closes, the SA stays
    r.sai.set(ACL_ENTRY, data[0], &action_flow(None)).unwrap();
    assert_eq!(r.bus(), "120=0");
    assert_eq!(r.sai.macsec_remove(MacsecObject::Sc, sc_e), Err(Error::ObjectInUse));
    assert_eq!(r.sai.macsec_remove(MacsecObject::Flow, f_e), Err(Error::ObjectInUse));
    assert_eq!(r.sai.remove(ACL_TABLE, tbl[0]), Err(Error::ObjectInUse));
    r.sai.macsec_remove(MacsecObject::Sa, sa_e2).unwrap();
    assert_eq!(r.bus(), "302=8001");

    // teardown in MACsecOrch's order
    for d in 0..2 {
        r.sai.remove(ACL_ENTRY, data[d]).unwrap();
        r.sai.remove(ACL_ENTRY, eapol[d]).unwrap();
        let attr = if d == 1 {
            sys::SAI_PORT_ATTR_INGRESS_MACSEC_ACL
        } else {
            sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL
        };
        r.sai.set(PORT, p0, &oid(attr, RawOid::NULL)).unwrap();
        r.sai.remove(ACL_TABLE, tbl[d]).unwrap();
    }
    r.sai.macsec_remove(MacsecObject::Sc, sc_e).unwrap();
    r.sai.macsec_remove(MacsecObject::Sc, sc_i).unwrap();
    r.sai.macsec_remove(MacsecObject::Flow, f_e).unwrap();
    r.sai.macsec_remove(MacsecObject::Flow, f_i).unwrap();
    assert_eq!(r.sai.macsec_remove(MacsecObject::Macsec, m_e), Err(Error::ObjectInUse));
    r.sai.macsec_remove(MacsecObject::Port, mp_e).unwrap();
    assert_eq!(r.bus(), "100=1");
    assert_eq!(r.ifg(1), 384); // the ingress MACsec port is still there
    r.sai.macsec_remove(MacsecObject::Port, mp_i).unwrap();
    assert_eq!(r.bus(), "101=1");
    assert_eq!(r.ifg(1), 128);
    r.sai.macsec_remove(MacsecObject::Macsec, m_e).unwrap();
    r.sai.macsec_remove(MacsecObject::Macsec, m_i).unwrap();
    assert!(
        r.status()
            .contains("0 macsec, 0 ports, 0 flows, 0 SCs, 0 SAs, 0 ACL tables, 0 ACL entries"),
        "{}",
        r.status()
    );
}

#[test]
fn a_table_without_gates_or_probe() {
    let mut r = rig(&ms_table(false, false), &[]);
    assert_eq!(r.bus(), ""); // nothing at creation
    let p0 = r.ports[0];
    r.reg(0x10, 0x20f, 1);
    let m_e = r
        .mk(MacsecObject::Macsec, &[s32(sys::SAI_MACSEC_ATTR_DIRECTION, EGR)])
        .unwrap();
    let mp_e = r
        .mk(
            MacsecObject::Port,
            &[
                s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
                oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
            ],
        )
        .unwrap();
    assert_eq!(r.ifg(1), 384);
    r.sai.macsec_remove(MacsecObject::Port, mp_e).unwrap();
    assert_eq!(r.ifg(1), 96); // no quirk: the Ethernet default
    r.sai.macsec_remove(MacsecObject::Macsec, m_e).unwrap();
}

/// Port, MACSEC_PORT and a bound flow with an SA of the first SC on AN 0, ready for D9.
fn bound(r: &mut Rig) -> (RawOid, RawOid, RawOid, RawOid) {
    let p0 = r.ports[0];
    r.reg(0x10, 0x20f, 1);
    r.mk(
        MacsecObject::Port,
        &[
            s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
        ],
    )
    .unwrap();
    let f = r
        .mk(
            MacsecObject::Flow,
            &[s32(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)],
        )
        .unwrap();
    let t = r.acl_table(false);
    r.sai
        .set(PORT, p0, &oid(sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL, t))
        .unwrap();
    let entry = r.acl_entry(t, 1, vec![action_flow(Some(f))]);
    let sc = r
        .mk(
            MacsecObject::Sc,
            &[
                s32(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, EGR),
                oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, f),
                u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, 1),
                s32(
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                    e(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128),
                ),
            ],
        )
        .unwrap();
    (p0, f, entry, sc)
}

/// D9 (sai-macsec: Q7 fixed) through the SAI API: the ACL entry sends the port's data to another
/// flow; the old SC's SA comes out of the PHY (gate closed first), the new SC's goes in on AN 0.
#[test]
fn d9_a_flow_switch_takes_the_old_scs_sas_out() {
    let mut r = rig(&ms_table(true, true), &[]);
    let (_, _, entry, sc) = bound(&mut r);
    r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc, 0, false, false, 1)).unwrap();
    let f2 = r
        .mk(
            MacsecObject::Flow,
            &[s32(sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION, EGR)],
        )
        .unwrap();
    let sc2 = r
        .mk(
            MacsecObject::Sc,
            &[
                s32(sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION, EGR),
                oid(sys::SAI_MACSEC_SC_ATTR_FLOW_ID, f2),
                u64v(sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI, 2),
                s32(
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE,
                    e(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_128),
                ),
            ],
        )
        .unwrap();
    r.bus();
    r.sai.set(ACL_ENTRY, entry, &action_flow(Some(f2))).unwrap();
    assert_eq!(r.bus(), "120=0 300=8001"); // the C++ writes nothing here and keeps the gate open
    r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc2, 0, false, false, 1)).unwrap(); // the C++: INSUFFICIENT_RESOURCES
    assert!(r.bus().starts_with("200=100 "));
}

/// D10 (sai-macsec: Q10 fixed) through the SAI API: no cipher suite change under a waiting SA.
#[test]
fn d10_no_cipher_suite_change_under_a_waiting_sa() {
    let mut r = rig(&ms_table(true, true), &[]);
    let (_, _, entry, sc) = bound(&mut r);
    r.sai
        .set(ACL_ENTRY, entry, &boolean(sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE, false))
        .unwrap();
    r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc, 0, true, false, 1)).unwrap(); // waiting
    let suite = |c: u32| s32(sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE, e(c));
    assert_eq!(
        r.sai
            .macsec_set(MacsecObject::Sc, sc, &suite(sys::SAI_MACSEC_CIPHER_SUITE_GCM_AES_256)),
        Err(Error::ObjectInUse)
    );
}

/// `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP` (designed, off): off = the C++ (removing the
/// MACSEC_PORT runs `disable`); on = the engine stays on (no `disable`), and the start-up
/// clean-up only resets an engine left on.
#[test]
fn engines_on_at_stop_both_ways() {
    for on in [false, true] {
        let extra: &[(&str, &str)] = if on {
            &[("SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP", "1")]
        } else {
            &[]
        };
        let mut r = rig(&ms_table(true, true), extra);
        let start = r.bus();
        assert_eq!(
            start,
            if on {
                "110=1 111=1 110=1 111=1"
            } else {
                "110=1 100=1 111=1 101=1 110=1 100=1 111=1 101=1"
            }
        );
        let p0 = r.ports[0];
        let mp = r
            .mk(
                MacsecObject::Port,
                &[
                    s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
                    oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
                ],
            )
            .unwrap();
        r.bus();
        r.sai.macsec_remove(MacsecObject::Port, mp).unwrap();
        assert_eq!(r.bus(), if on { "" } else { "100=1" });
        assert_eq!(r.ifg(1), 96); // the gap goes back either way
    }
    let mut sai = core();
    let t = table_file(&ms_table(true, true));
    assert_eq!(
        create(
            &mut sai,
            &profile(Some(&t), &[("SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP", "yes")])
        ),
        Err(Error::InvalidParameter)
    );
    std::fs::remove_file(t).unwrap();
}

/// `SAI_OPENBCM_MACSEC_HOLD_CLOSED` (designed, off): a listed port's link stays down (its gate)
/// until it has a MACSEC_PORT; off, no gate is registered (the C++).
#[test]
fn hold_closed_both_ways() {
    let r = rig(&ms_table(true, true), &[]);
    assert!(r.sai.backend().inner().gated_ports().is_empty());
    let mut r = rig(&ms_table(true, true), &[("SAI_OPENBCM_MACSEC_HOLD_CLOSED", "1")]);
    assert_eq!(r.sai.backend().inner().gated_ports(), [HwPort(1)]);
    let p0 = r.ports[0];
    let events = r.sai.take_events().unwrap();
    r.sai
        .set(PORT, p0, &boolean(sys::SAI_PORT_ATTR_ADMIN_STATE, true))
        .unwrap();
    r.sai.backend_mut().inner_mut().linkscan();
    assert!(events.try_recv().is_err()); // held: no link
    r.mk(
        MacsecObject::Port,
        &[
            s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
        ],
    )
    .unwrap();
    r.sai.backend_mut().inner_mut().linkscan();
    assert!(matches!(
        events.try_recv(),
        Ok(sai_core::backend::BackendEvent::Link {
            hw: HwPort(1),
            up: true
        })
    ));
    // "all" names every engine port; a lane that is not a MACsec port is refused
    let r = rig(&ms_table(true, true), &[("SAI_OPENBCM_MACSEC_HOLD_CLOSED", "all")]);
    assert_eq!(r.sai.backend().inner().gated_ports(), [HwPort(1), HwPort(5)]);
    let t = table_file(&ms_table(true, true));
    let mut sai = core();
    assert_eq!(
        create(&mut sai, &profile(Some(&t), &[("SAI_OPENBCM_MACSEC_HOLD_CLOSED", "9")])),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        create(&mut sai, &profile(Some(&t), &[("SAI_OPENBCM_MACSEC_HOLD_CLOSED", "x")])),
        Err(Error::InvalidParameter)
    );
    std::fs::remove_file(t).unwrap();
}

/// Backend first, model second: a MACSEC_PORT whose `enable` write fails is not created; the
/// model and the chip's gap are as before.
#[test]
fn a_failed_sequence_leaves_the_model_as_it_was() {
    let mut r = rig(&ms_table(true, true), &[]);
    let p0 = r.ports[0];
    r.bus();
    // reset.egress is the first write, enable.egress the second
    r.sai
        .backend_mut()
        .inner_mut()
        .fail_nth("mdio_c45_write", 2, Error::Failure)
        .unwrap();
    let attrs = [
        s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
        oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
    ];
    assert_eq!(r.mk(MacsecObject::Port, &attrs), Err(Error::Failure));
    assert_eq!(r.ifg(1), 0);
    assert!(r.status().contains("0 macsec, 0 ports"), "{}", r.status());
    assert_eq!(
        r.sai.get(PORT, p0, &[sys::SAI_PORT_ATTR_MACSEC_PORT_LIST]).unwrap()[0],
        Value::OidList(vec![])
    );
    // and it works once the bus does
    r.mk(MacsecObject::Port, &attrs).unwrap();
    assert_eq!(r.ifg(1), 384);
}

/// The sequences' MDIO goes through the backend (logged as the C++ recording backend logs it) and
/// the gap through `port_ifg_set`, in the model's order.
#[test]
fn the_call_log_shows_the_sequences_and_the_gap() {
    let mut r = rig(&ms_table(true, true), &[]);
    let p0 = r.ports[0];
    let log = r.sai.backend().log();
    log.take();
    r.mk(
        MacsecObject::Port,
        &[
            s32(sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION, EGR),
            oid(sys::SAI_MACSEC_PORT_ATTR_PORT_ID, p0),
        ],
    )
    .unwrap();
    assert_eq!(
        log.take(),
        [
            "mdio_c45_write(phy=16, dev=1, reg=272, val=1) -> SUCCESS",
            "mdio_c45_write(phy=16, dev=1, reg=256, val=0) -> SUCCESS",
            "port_ifg_set(hw=1, bits=384) -> SUCCESS",
        ]
    );
}

/// `extmacsec trace`: off by default; on, a sequence that writes is kept with its key words
/// hidden; the usage line for anything else.
#[test]
fn the_trace_hides_key_words() {
    let mut r = rig(&ms_table(true, true), &[]);
    assert!(
        macsec::ext_macsec_trace(&mut r.sai, "").starts_with("extmacsec trace: off, engine rust (phy-seq), 0 runs")
    );
    assert!(macsec::ext_macsec_trace(&mut r.sai, "bogus").starts_with("usage:"));
    macsec::ext_macsec_trace(&mut r.sai, "on");
    let (_, _, _, sc) = bound(&mut r);
    r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc, 0, false, false, 1)).unwrap();
    let dump = macsec::ext_macsec_trace(&mut r.sai, "dump");
    assert!(dump.contains("W 010 1 0201 ****"), "{dump}");
    assert!(dump.contains("W 010 1 0200 0100"), "{dump}");
    assert!(dump.contains("# run ") && dump.contains("= 0"));
    for k in ["c1c0", "cfce", "8f80"] {
        assert!(!dump.contains(k), "{k} in the trace");
    }
    assert!(macsec::ext_macsec_trace(&mut r.sai, "clear").contains("0 runs, 0 lines kept"));
}

/// A deliberate improvement on the C++ (whose hold-off held the link polls only): raw MDIO of the
/// diag socket (a tool's PHY firmware-message conversation) defers the PN and counter polls
/// syncd asks for while the hold-off lasts; the last read answers, the next read catches up;
/// SA installs and counter clears are not deferred; a tool that polls for minutes cannot keep
/// the PN from being read every `MACSEC_POLL_DEFER_MAX_MS`.
#[test]
fn the_raw_mdio_hold_off_defers_the_pn_and_counter_polls() {
    let phy_table = table_file(
        "ext-phy 1
poll-ms 0
holdoff-ms 3000
port 5
link
  C 11 1 a101 3 3
",
    );
    let mut r = rig(
        &ms_table(true, true),
        &[("SAI_OPENBCM_EXT_PHY_FILE", phy_table.as_str())],
    );
    std::fs::remove_file(&phy_table).unwrap();
    assert!(extphy::ext_phy_active(&r.sai));
    let clock = ManualClock::new();
    extphy::set_clock(&r.sai, Arc::new(clock.clone()));
    let (_, flow, _, sc) = bound(&mut r);
    let sa = r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc, 0, false, false, 1)).unwrap();
    let pn = |r: &mut Rig| {
        r.sai
            .macsec_get(MacsecObject::Sa, sa, &[sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN])
            .unwrap()
            .remove(0)
    };
    let pkts = |r: &mut Rig| r.stat(MacsecObject::Sa, sa, sys::SAI_MACSEC_SA_STAT_OUT_PKTS_ENCRYPTED);
    let ops = |r: &Rig| r.sai.backend().inner().mdio().lock().unwrap().log.len();
    let tool = |r: &mut Rig| extphy::ext_phy_foreign_mdio(&mut r.sai, |_| ());
    r.reg(0x10, 0x500, 0x10);
    r.reg(0x10, 0x400, 5);
    assert_eq!(pn(&mut r), Value::U64(0x10));
    assert_eq!(pkts(&mut r), 5);

    // raw MDIO at t; the polls at t+1 s are deferred: nothing on the bus, the last read answers
    tool(&mut r);
    r.reg(0x10, 0x500, 0x20);
    clock.advance_ms(1000);
    let before = ops(&r);
    assert_eq!(pn(&mut r), Value::U64(0x10));
    assert_eq!(pkts(&mut r), 5);
    assert_eq!(
        r.stat(MacsecObject::Flow, flow, sys::SAI_MACSEC_FLOW_STAT_CONTROL_PKTS),
        0
    );
    assert_eq!(ops(&r), before);
    // what SONiC asks for is not deferred: an SA install runs at once
    r.bus();
    r.mk(MacsecObject::Sa, &sa_attrs(EGR, sc, 1, false, false, 1)).unwrap();
    assert!(!r.bus().is_empty());

    // t+3 s: the hold-off is over, the reads catch up (the PHY's counter cleared on read)
    clock.advance_ms(2000);
    assert_eq!(pn(&mut r), Value::U64(0x20));
    assert_eq!(pkts(&mut r), 5 + 5);
    let counts = extphy::ext_phy(&r.sai).unwrap().macsec_poll_counts();
    assert_eq!((counts.deferred, counts.forced), (3, 0));

    // a clear inside the hold-off still reads the PHY first
    tool(&mut r);
    let before = ops(&r);
    r.sai.macsec_clear_stats(MacsecObject::Sa, sa).unwrap();
    assert!(ops(&r) > before);
    assert_eq!(pkts(&mut r), 0); // deferred: the cleared value
    clock.advance_ms(3000);

    // a tool polling for five minutes (raw MDIO every 500 ms), syncd asking for the PN every
    // second: the PN is read every 10 s, so its exhaustion cannot hide behind the tool
    let mut reads = Vec::new();
    for ms in (0..300_000u64).step_by(500) {
        tool(&mut r);
        if ms % 1000 == 0 {
            r.reg(0x10, 0x500, u16::try_from(ms / 1000).unwrap());
            let at = ops(&r);
            let got = pn(&mut r);
            if ops(&r) > at {
                assert_eq!(got, Value::U64(ms / 1000));
                reads.push(ms);
            }
        }
        clock.advance_ms(500);
    }
    assert_eq!(reads.len(), 29, "{reads:?}");
    assert!(reads.windows(2).all(|w| w[1] - w[0] == 10_000), "{reads:?}");
}

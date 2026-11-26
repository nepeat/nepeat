//! Field processor (the SAI ACL tables: fp groups / entries), policers, storm control and the
//! `CoPP` ACL (`tl_acl.{h,cpp}`), line by line: the same IFCS calls in the same order with the
//! same attributes, the same bookkeeping, refusals and log lines.
//!
//! Policers are backend objects; one used by a `CoPP` trap ([`crate::hostif`]) becomes one IFCS
//! `acl_meter` per inno block, created on first use and updated by `policer_set`; one used by a SAI
//! ACL entry is one node-scope meter in that entry's table.
//!
//! `CoPP` table, as isai 6.2.0 builds it on this chip (oracle traces 2026-11-16): an ingress match
//! profile (DMAC, ether type, L4 protocol, L4 ports, L4 byte 1, hostif trap), an action profile
//! (drop, copy to CPU, copy override, meter, counter), one control table with scope `PER_IB` and one
//! ACL per inno block added to it (the number of IBs is probed: ACL create with IB n until the SDK
//! refuses). A trap rule is one ACE per IB. Meter units as isai: packet policers at 84 B per packet
//! (CIR = pps x 672 bit/s, CBS = packets x 84 B), byte policers at CIR x 8 bit/s, CBS bytes.
//!
//! SAI ACL tables (isai 6.2.0, oracle acl): per table a match profile (`PORT_SHARING`), an action
//! profile, a node-scope table and one ACL in it, bound to the union of its entries' ports with
//! `acl_port_add`. An entry is one ACE, or one ACE per port with an `INGRESS_L2_INTERFACE` match when it
//! covers only part of the table's ports. Values in network order with a mask. ACE priority
//! `0xffffffff - 2 x priority` (lower wins); table priority 12..=15 (`CoPP`'s is 11), egress 6.
//!
//! Cross-area: REDIRECT asks the L3 area (`L3Area::nexthop_handle`) and the LAG area
//! (`LagArea::dest_handle`), MIRROR the mirror area's collector sets ([`MirrorSets`]).
#![forbid(unsafe_code)]

use core::mem::{offset_of, size_of};
use std::collections::{BTreeMap, BTreeSet};

use ifcs::{Attrs, Handle, sys};
use sai_core::backend::HwPort;
use sai_core::backend::types::{
    ChipLimits, FpCounter, FpEntryId, FpGroupExtra, FpGroupId, FpQual, FpRule, MacAddr, Masked, MirrorId, PolicerCfg,
    PolicerId, Redirect, StormKind,
};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};
use crate::l3::L3Area;
use crate::lag::LagArea;

/// Inno blocks probed at most (`MAX_IBS`).
const MAX_IBS: u32 = 8;
/// Entries per IB of the `CoPP` table (isai: 128).
const COPP_TABLE_SIZE: u32 = 128;
/// The `CoPP` table's priority.
const COPP_TABLE_PRIORITY: u32 = 11;
/// Entries of a SAI ACL table (`UACL_SIZE`).
const UACL_SIZE: u32 = 256;
/// The frontend's own `CoPP` field group (not used with `trap_program`).
const COPP_FP_PRIORITY: i32 = 1000;

/// One match of a `CoPP` rule or a SAI ACL entry (`CoppMatch`): VALUE bytes (network order, with
/// a MASK for SAI ACL entries) or an OBJECT handle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoppMatch {
    /// `IFCS_ACL_MATCH_TYPE_*`.
    pub kind: u32,
    /// The value.
    pub value: Vec<u8>,
    /// Its mask (`CoPP` rules: none, exact).
    pub mask: Vec<u8>,
    /// The object (0: a value match).
    pub object: Handle,
}

/// A `CoPP` rule: all must match (one ACE per rule and IB).
pub type CoppRule = Vec<CoppMatch>;

/// What a `CoPP` install does with matched frames (`CoppActions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoppActions {
    /// Copy to the CPU with this hostif trap (null: no copy).
    pub trap: Handle,
    /// Drop the original (SAI TRAP, DROP).
    pub drop: bool,
    /// The policer: meter + its colours.
    pub policer: Option<PolicerId>,
    /// Policed yellow frames lose their CPU copy.
    pub yellow_drop: bool,
    /// Policed red frames lose their CPU copy.
    pub red_drop: bool,
}

/// The mirror area's collector sets for ACL mirror actions (C++ `MirrorArea::collector_set` /
/// `collector_set_release`: the set of a session and direction, created on first use and counted).
/// TODO(G2 package E, mirror): `MirrorArea` implements it when the mirror area lands; until then
/// [`NoMirror`] answers as the C++ does for a session it does not have.
pub trait MirrorSets {
    /// The collector set of session `id` in a direction (held until released).
    fn collector_set(&mut self, core: &Core, id: MirrorId, ingress: bool) -> Result<Handle>;
    /// Gives one hold back.
    fn collector_set_release(&mut self, core: &Core, id: MirrorId, ingress: bool);
}

/// No mirror session exists (the mirror area is not ported yet: package E): `collector_set` answers
/// `ITEM_NOT_FOUND` and a release does nothing, as the C++ for an unknown session.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMirror;

impl MirrorSets for NoMirror {
    fn collector_set(&mut self, _core: &Core, _id: MirrorId, _ingress: bool) -> Result<Handle> {
        Err(Error::ItemNotFound)
    }
    fn collector_set_release(&mut self, _core: &Core, _id: MirrorId, _ingress: bool) {}
}

/// What an ACL entry asks of the other areas (C++ `c_.l3`, `c_.lag`, `c_.mirror`).
pub struct Peers<'a> {
    /// Next hops (REDIRECT to an egress object).
    pub l3: &'a L3Area,
    /// Ports and LAGs (REDIRECT to a destination).
    pub lag: &'a LagArea,
    /// Collector sets (MIRROR).
    pub mirror: &'a mut dyn MirrorSets,
}

impl core::fmt::Debug for Peers<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Peers").finish_non_exhaustive()
    }
}

/// An install's objects in one IB (`Install::PerIb`).
#[derive(Clone, Debug, Default)]
struct PerIb {
    ib: u32,
    counter: Handle,
    aces: Vec<Handle>,
    matches: Vec<Handle>,
    actions: Vec<Handle>,
}

/// A `CoPP` install (`Install`).
#[derive(Clone, Debug, Default)]
struct Install {
    ibs: Vec<PerIb>,
    policer: Option<PolicerId>,
}

/// A SAI ACL table (`UGroup`).
#[derive(Clone, Debug, Default)]
struct UGroup {
    qset: Vec<FpQual>,
    v6: bool,
    meters: bool,
    /// Egress, ranges, `OUT_PORTS`, extra actions.
    x: FpGroupExtra,
    size: u32,
    mprof: Handle,
    aprof: Handle,
    table: Handle,
    acl: Handle,
    /// Devports the ACL is bound to (`acl_port_add`).
    bound: BTreeSet<u32>,
}

/// A SAI ACL entry (`UEntry`).
#[derive(Clone, Debug)]
struct UEntry {
    gid: u32,
    rule: FpRule,
    /// One ACE per port (the entry covers part of the table's ports).
    per_port: bool,
    counter: Handle,
    aces: Vec<Handle>,
    matches: Vec<Handle>,
    actions: Vec<Handle>,
    /// L4 port range comparators.
    cmps: Vec<Handle>,
    cmp_group: Handle,
    /// (session, ingress) collector sets held.
    mirrors: Vec<(MirrorId, bool)>,
    enabled: bool,
    /// Storm control: `Some(0)` = L2 miss only (C++ `dmac_hit`, -1 = none).
    dmac_hit: Option<u8>,
}

impl UEntry {
    fn new(gid: u32, rule: FpRule, dmac_hit: Option<u8>) -> Self {
        Self {
            gid,
            rule,
            per_port: false,
            counter: Handle::NULL,
            aces: Vec::new(),
            matches: Vec::new(),
            actions: Vec::new(),
            cmps: Vec::new(),
            cmp_group: Handle::NULL,
            mirrors: Vec::new(),
            enabled: true,
            dmac_hit,
        }
    }
}

/// Policers and their meters (`policers_`, `meters_`, `umeters_`).
#[derive(Debug, Default)]
struct Meters {
    policers: BTreeMap<u32, PolicerCfg>,
    next_policer: u32,
    /// (policer, IB) -> `CoPP` meter.
    copp: BTreeMap<(u32, u32), Handle>,
    /// (policer, table) -> meter in a SAI ACL table.
    user: BTreeMap<(u32, u32), Handle>,
}

/// The `CoPP` table (`copp_up_`, ...).
#[derive(Debug, Default)]
struct Copp {
    up: bool,
    node_scope: bool,
    mprof: Handle,
    aprof: Handle,
    table: Handle,
    /// Index = inno block.
    acls: Vec<Handle>,
    installs: BTreeMap<u32, Install>,
    next_install: u32,
}

/// The ACL area's state (`AclArea`).
#[derive(Debug)]
pub struct AclArea {
    meters: Meters,
    copp: Copp,
    ugroups: BTreeMap<u32, UGroup>,
    uentries: BTreeMap<u32, UEntry>,
    next_ugroup: u32,
    next_uentry: u32,
    /// Hostif trap of ACL COPY / TRAP actions (queue 0).
    acl_trap: Handle,
    storm_gid: Option<u32>,
    /// (devport, kind) -> entry.
    storm: BTreeMap<(u32, u8), u32>,
}

impl Default for AclArea {
    fn default() -> Self {
        Self {
            meters: Meters {
                next_policer: 1,
                ..Meters::default()
            },
            copp: Copp {
                next_install: 1,
                ..Copp::default()
            },
            ugroups: BTreeMap::new(),
            uentries: BTreeMap::new(),
            next_ugroup: 1,
            next_uentry: 1,
            acl_trap: Handle::NULL,
            storm_gid: None,
            storm: BTreeMap::new(),
        }
    }
}

/// `map_reason` of a failed reason as a result (a success never reaches it: `FAILURE` then).
fn reason_err(r: ifcs::status::Reason) -> Error {
    if r == sys::IFCS_SUCCESS {
        Error::Failure
    } else {
        ifcs::status::sai_error(r)
    }
}

/// A value struct's bytes (`memset` 0, then the members): `size` bytes with each `(offset, bytes)`.
fn value_of(size: usize, members: &[(usize, &[u8])]) -> Vec<u8> {
    let mut v = vec![0u8; size];
    for (off, b) in members {
        if let Some(dst) = v.get_mut(*off..*off + b.len()) {
            dst.copy_from_slice(b);
        }
    }
    v
}

/// `ifcs_acl_ctc_action_t` (copy to the CPU).
fn ctc_value(hostif: Handle, green: u32, yellow: u32, red: u32, dirty: u8) -> Vec<u8> {
    type T = sys::ifcs_acl_ctc_action_t;
    value_of(
        size_of::<T>(),
        &[
            (offset_of!(T, hostif), &hostif.0.to_ne_bytes()),
            (offset_of!(T, green), &green.to_ne_bytes()),
            (offset_of!(T, yellow), &yellow.to_ne_bytes()),
            (offset_of!(T, red), &red.to_ne_bytes()),
            (offset_of!(T, dirty), &[dirty]),
        ],
    )
}

/// `ifcs_acl_ctc_override_action_t`.
fn ctc_override_value(green: u32, yellow: u32, red: u32, dirty: u8) -> Vec<u8> {
    type T = sys::ifcs_acl_ctc_override_action_t;
    value_of(
        size_of::<T>(),
        &[
            (offset_of!(T, green), &green.to_ne_bytes()),
            (offset_of!(T, yellow), &yellow.to_ne_bytes()),
            (offset_of!(T, red), &red.to_ne_bytes()),
            (offset_of!(T, dirty), &[dirty]),
        ],
    )
}

/// `ifcs_acl_drop_action_t` (no drop reason).
fn drop_value(green: u32, yellow: u32, red: u32, dirty: u8) -> Vec<u8> {
    type T = sys::ifcs_acl_drop_action_t;
    value_of(
        size_of::<T>(),
        &[
            (offset_of!(T, green), &green.to_ne_bytes()),
            (offset_of!(T, yellow), &yellow.to_ne_bytes()),
            (offset_of!(T, red), &red.to_ne_bytes()),
            (offset_of!(T, dirty), &[dirty]),
        ],
    )
}

/// `ifcs_acl_set_tc_action_t`: every colour on, the same class, dirty 0x3f.
fn set_tc_value(tc: u32) -> Vec<u8> {
    type T = sys::ifcs_acl_set_tc_action_t;
    let on = sys::IFCS_ACL_ACTION_ENABLE.to_ne_bytes();
    let v = tc.to_ne_bytes();
    value_of(
        size_of::<T>(),
        &[
            (offset_of!(T, green), &on),
            (offset_of!(T, yellow), &on),
            (offset_of!(T, red), &on),
            (offset_of!(T, green_value), &v),
            (offset_of!(T, yellow_value), &v),
            (offset_of!(T, red_value), &v),
            (offset_of!(T, dirty), &[0x3f]),
        ],
    )
}

/// `ifcs_acl_set_dscp_action_t`: address family, value, dirty 3.
fn set_dscp_value(v6: bool, dscp: u32) -> Vec<u8> {
    type T = sys::ifcs_acl_set_dscp_action_t;
    let af = if v6 {
        sys::IFCS_IP_ADDR_FAMILY_IPV6
    } else {
        sys::IFCS_IP_ADDR_FAMILY_IPV4
    };
    value_of(
        size_of::<T>(),
        &[
            (offset_of!(T, addr_family), &af.to_ne_bytes()),
            (offset_of!(T, value), &dscp.to_ne_bytes()),
            (offset_of!(T, dirty), &[0x03]),
        ],
    )
}

/// `AclArea::meter_attrs`: (attribute, `value.u64`) of a policer's rates and bursts, packets at 84 B
/// (64 B frame + preamble + IPG) per packet as isai converts; srTCM without PBS: EBS 1 (isai).
pub fn meter_attrs(p: &PolicerCfg) -> Vec<(u32, u64)> {
    let rate = |r: u64| -> u64 {
        if p.packets {
            r.wrapping_mul(84 * 8)
        } else {
            r.wrapping_mul(8)
        }
    };
    let burst = |b: u64| -> u64 { if p.packets { b.wrapping_mul(84) } else { b } };
    let mut a = vec![
        (sys::IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE, rate(p.cir)),
        (
            sys::IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE,
            burst(if p.cbs != 0 { p.cbs } else { 1 }),
        ),
    ];
    if p.tr_tcm {
        a.push((sys::IFCS_ACL_METER_ATTR_EXCESS_INFORMATION_RATE, rate(p.pir)));
    }
    a.push((
        sys::IFCS_ACL_METER_ATTR_EXCESS_BURST_SIZE,
        if p.pbs != 0 { burst(p.pbs) } else { 1 },
    ));
    a
}

/// The meter's create attributes before its table / scope (type bytes, mode, colour awareness,
/// the rates).
fn meter_create_attrs(p: &PolicerCfg) -> Attrs {
    let mode = if p.tr_tcm {
        sys::IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2698
    } else {
        sys::IFCS_ACL_METER_MODE_DUAL_TOKEN_RFC_2697
    };
    let mut a = Attrs::new()
        .u32(sys::IFCS_ACL_METER_ATTR_TYPE, sys::IFCS_ACL_METER_TYPE_BYTE)
        .u32(sys::IFCS_ACL_METER_ATTR_MODE, mode)
        .data(sys::IFCS_ACL_METER_ATTR_COLOR_AWARE, !p.color_blind);
    for (id, v) in meter_attrs(p) {
        a = a.u64(id, v);
    }
    a
}

/// A SAI ACL match: value and mask bytes (`mbytes`).
fn mbytes(kind: u32, v: &[u8], m: &[u8]) -> CoppMatch {
    CoppMatch {
        kind,
        value: v.to_vec(),
        mask: m.to_vec(),
        object: Handle::NULL,
    }
}
fn mu8(kind: u32, v: u8, m: u8) -> CoppMatch {
    mbytes(kind, &[v], &[m])
}
fn mu16(kind: u32, v: u16, m: u16) -> CoppMatch {
    mbytes(kind, &v.to_be_bytes(), &m.to_be_bytes())
}
fn mobj(kind: u32, object: Handle) -> CoppMatch {
    CoppMatch {
        kind,
        object,
        ..CoppMatch::default()
    }
}

/// `push_types`: appends the types not in the list yet.
fn push_types(v: &mut Vec<u32>, t: &[u32]) {
    for x in t {
        if !v.contains(x) {
            v.push(*x);
        }
    }
}

/// The C++ `(int)FpQual`.
const fn qual_num(q: FpQual) -> i32 {
    q as i32
}

/// The IFCS match types of a table's qualifiers (`group_make`); `Err(q)` for one without a match.
pub fn match_types(qset: &[FpQual], egress: bool, dmac_hit: bool) -> core::result::Result<Vec<u32>, FpQual> {
    let v6 = qset.iter().any(|q| matches!(q, FpQual::SrcIp6 | FpQual::DstIp6));
    let mut mt = Vec::new();
    for q in qset {
        match q {
            // egress tables: no port match (isai's egress profile has none; ours with one: PARAM)
            FpQual::InPorts => {
                if !egress {
                    push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_INGRESS_L2_INTERFACE]);
                }
            }
            FpQual::EtherType => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE]),
            FpQual::IpType => push_types(
                &mut mt,
                &[
                    sys::IFCS_ACL_MATCH_TYPE_L3_TYPE,
                    sys::IFCS_ACL_MATCH_TYPE_ARP_OPERATION,
                    sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE,
                ],
            ),
            FpQual::IpProtocol => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL]),
            FpQual::SrcIp => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS]),
            FpQual::DstIp => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS]),
            FpQual::SrcIp6 => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS]),
            FpQual::DstIp6 => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS]),
            FpQual::L4SrcPort => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT]),
            FpQual::L4DstPort => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT]),
            FpQual::TcpFlags => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_TCP_FLAGS]),
            FpQual::Tos => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DSCP, sys::IFCS_ACL_MATCH_TYPE_ECN]),
            FpQual::DstMac => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS]),
            FpQual::SrcMac => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS]),
            FpQual::OuterVlan => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_VLAN_ID]),
            FpQual::Ttl => push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL]),
            FpQual::IcmpTypeCode => {
                // isai: ICMP type/code in the L4 source port field (v4 tables), L4 bytes 1 and 2
                // (v6 tables); the protocol is matched too
                if v6 {
                    push_types(
                        &mut mt,
                        &[sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1, sys::IFCS_ACL_MATCH_TYPE_L4_BYTE2],
                    );
                } else {
                    push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT]);
                }
                push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL]);
            }
            FpQual::DstCpu => return Err(*q),
        }
    }
    if dmac_hit {
        push_types(&mut mt, &[sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT]);
    }
    Ok(mt)
}

/// 0.12.41 needs `MAX_WIDTH` (isai's 0.16 does not): the key bits of the match types by our
/// estimate, plus 8 per comparator result (I).
pub fn key_bits(mt: &[u32], x: &FpGroupExtra) -> u32 {
    let mut bits = 0u32;
    for &t in mt {
        bits += match t {
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS | sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS => 128,
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS | sys::IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS => 48,
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS | sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS => 32,
            sys::IFCS_ACL_MATCH_TYPE_L3_TYPE | sys::IFCS_ACL_MATCH_TYPE_ECN => 4,
            sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL
            | sys::IFCS_ACL_MATCH_TYPE_TCP_FLAGS
            | sys::IFCS_ACL_MATCH_TYPE_DSCP
            | sys::IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL
            | sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1
            | sys::IFCS_ACL_MATCH_TYPE_L4_BYTE2 => 8,
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT => 1,
            _ => 16, // ports, ether type, VLAN, ARP operation, the in-port
        };
    }
    bits + if x.src_range { 8 } else { 0 } + if x.dst_range { 8 } else { 0 }
}

/// The match profile widths tried, smallest first (480 if none is wide enough).
pub fn widths(bits: u32) -> Vec<u32> {
    let w: Vec<u32> = [80, 160, 320, 480].into_iter().filter(|w| *w >= bits).collect();
    if w.is_empty() { vec![480] } else { w }
}

/// The IFCS table priority: egress 6 (isai), ingress `16 - priority` within 12..=15.
pub fn table_priority(priority: i32, egress: bool) -> u32 {
    if egress {
        6
    } else {
        let p = (16 - i64::from(priority)).clamp(12, 15);
        u32::try_from(p).unwrap_or(12)
    }
}

/// The ACE priority of a SAI priority: `0xffffffff - 2 x min(priority, 0x7ffffffe)` (lower wins).
pub const fn ace_priority(priority: u32) -> u32 {
    let sp = if priority > 0x7fff_fffe { 0x7fff_fffe } else { priority };
    0xffff_ffff - 2 * sp
}

/// The matches of a rule (without the port), validated against the table (`uentry_matches`).
#[allow(clippy::too_many_lines, reason = "the C++ function, field for field")]
fn uentry_matches(g: &UGroup, r: &FpRule) -> Result<Vec<CoppMatch>> {
    let m = &r.m;
    let mut out = Vec::new();
    for q in FpQual::ALL {
        if !m.has(q) || q == FpQual::InPorts {
            continue;
        }
        if !g.qset.contains(&q) {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL entry: qualifier {} is not in its table", qual_num(q)),
            );
            return Err(Error::InvalidParameter);
        }
    }
    let ether_set = m.ether_type.is_some();
    let proto_set = m.ip_protocol.is_some();
    if let Some(e) = m.ether_type {
        out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE, e.value, e.mask));
    }
    if let Some(t) = m.ip_type {
        match t.cast_unsigned() {
            ssys::SAI_ACL_IP_TYPE_IP => out.push(mu8(
                sys::IFCS_ACL_MATCH_TYPE_L3_TYPE,
                l3_type(sys::IFCS_ACL_MATCH_L3_TYPE_IPV4),
                0x04,
            )),
            ssys::SAI_ACL_IP_TYPE_NON_IP => out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_L3_TYPE, 0x00, 0x04)),
            ssys::SAI_ACL_IP_TYPE_IPV4ANY => out.push(mu8(
                sys::IFCS_ACL_MATCH_TYPE_L3_TYPE,
                l3_type(sys::IFCS_ACL_MATCH_L3_TYPE_IPV4),
                0x06,
            )),
            ssys::SAI_ACL_IP_TYPE_IPV6ANY => out.push(mu8(
                sys::IFCS_ACL_MATCH_TYPE_L3_TYPE,
                l3_type(sys::IFCS_ACL_MATCH_L3_TYPE_IPV6),
                0x06,
            )),
            v @ (ssys::SAI_ACL_IP_TYPE_ARP | ssys::SAI_ACL_IP_TYPE_ARP_REQUEST | ssys::SAI_ACL_IP_TYPE_ARP_REPLY) => {
                if !ether_set {
                    out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE, 0x0806, 0xffff));
                }
                if v != ssys::SAI_ACL_IP_TYPE_ARP {
                    let op = if v == ssys::SAI_ACL_IP_TYPE_ARP_REQUEST { 1 } else { 2 };
                    out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_ARP_OPERATION, op, 0xffff));
                }
            }
            _ => {
                log::error(
                    ssys::SAI_API_ACL,
                    format_args!("ACL entry: ACL_IP_TYPE {t} has no IFCS L3 type match (NON_IPV4 / NON_IPV6)"),
                );
                return Err(Error::NotSupported);
            }
        }
    }
    if let Some(p) = m.ip_protocol {
        out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, p.value, p.mask));
    }
    if let Some(a) = m.src_ip {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV4_ADDRESS,
            &a.value.octets(),
            &a.mask.octets(),
        ));
    }
    if let Some(a) = m.dst_ip {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS,
            &a.value.octets(),
            &a.mask.octets(),
        ));
    }
    if let Some(a) = m.src_ip6 {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_IPV6_ADDRESS,
            &a.value.octets(),
            &a.mask.octets(),
        ));
    }
    if let Some(a) = m.dst_ip6 {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS,
            &a.value.octets(),
            &a.mask.octets(),
        ));
    }
    if let Some(p) = m.l4_src_port {
        out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, p.value, p.mask));
    }
    if let Some(p) = m.l4_dst_port {
        out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT, p.value, p.mask));
    }
    if let Some(t) = m.tcp_flags {
        out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_TCP_FLAGS, t.value, t.mask));
    }
    if let Some(i) = m.icmp_type_code {
        if m.l4_src_port.is_some() && !g.v6 {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL entry: ICMP type/code and L4 source port share one IFCS field"),
            );
            return Err(Error::NotSupported);
        }
        if !proto_set {
            out.push(mu8(
                sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL,
                if g.v6 { 58 } else { 1 },
                0xff,
            ));
        }
        if g.v6 {
            let [vt, vc] = i.value.to_be_bytes();
            let [mt, mc] = i.mask.to_be_bytes();
            if mt != 0 {
                out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1, vt, mt));
            }
            if mc != 0 {
                out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_L4_BYTE2, vc, mc));
            }
        } else {
            out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, i.value, i.mask));
        }
    }
    if let Some(t) = m.tos {
        if t.mask >> 2 != 0 {
            out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_DSCP, t.value >> 2, t.mask >> 2));
        }
        if t.mask & 3 != 0 {
            out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_ECN, t.value & 3, t.mask & 3));
        }
    }
    if let Some(a) = m.dst_mac {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS,
            &a.value.0,
            &a.mask.0,
        ));
    }
    if let Some(a) = m.src_mac {
        out.push(mbytes(
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_MAC_ADDRESS,
            &a.value.0,
            &a.mask.0,
        ));
    }
    if let Some(v) = m.outer_vlan {
        out.push(mu16(sys::IFCS_ACL_MATCH_TYPE_VLAN_ID, v.value & 0xfff, v.mask & 0xfff));
    }
    if let Some(t) = m.ttl {
        out.push(mu8(sys::IFCS_ACL_MATCH_TYPE_INGRESS_L3_TTL, t.value, t.mask));
    }
    Ok(out)
}

/// An L3 type value as the C++'s `(uint8_t)` of the enumerator.
fn l3_type(v: u32) -> u8 {
    u8::try_from(v).unwrap_or(0)
}

/// The ports of an entry (C++ `m.ports`: the frontend's `InPorts`, always set for an entry).
fn entry_ports(r: &FpRule) -> Vec<u32> {
    r.m.in_ports
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|p| u32::from(p.0))
        .collect()
}

/// The table's ports an entry of group `gid` uses: egress entries with `OUT_PORTS` the bound ports they
/// name, the others their ports (`ugroup_ports`).
fn ugroup_ports(gid: u32, egress: bool, entries: &BTreeMap<u32, UEntry>) -> BTreeSet<u32> {
    let mut s = BTreeSet::new();
    for e in entries.values().filter(|e| e.gid == gid) {
        let ports = entry_ports(&e.rule);
        if egress && !e.rule.m.out_ports.is_empty() {
            for hw in e.rule.m.out_ports.iter().map(|p| u32::from(p.0)) {
                if ports.contains(&hw) {
                    s.insert(hw);
                }
            }
        } else {
            s.extend(ports);
        }
    }
    s
}

/// Binds the table's ACL to exactly the ports its entries use (`ugroup_bind`).
fn ugroup_bind(core: &Core, gid: u32, g: &mut UGroup, entries: &BTreeMap<u32, UEntry>) -> Result<()> {
    let want = ugroup_ports(gid, g.x.egress, entries);
    let (mut add, mut addp, mut del, mut delp) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for hw in &want {
        if let (false, Some(sp)) = (g.bound.contains(hw), core.sysport.get(hw)) {
            add.push(*sp);
            addp.push(*hw);
        }
    }
    for hw in &g.bound {
        if let (false, Some(sp)) = (want.contains(hw), core.sysport.get(hw)) {
            del.push(*sp);
            delp.push(*hw);
        }
    }
    if !add.is_empty() {
        core.call(
            core.lib.ifcs_acl_port_add(NODE, g.acl, &mut add, &mut Attrs::null()),
            "acl_port_add",
        )?;
        g.bound.extend(addp);
    }
    if !del.is_empty() {
        let _ = core.call(core.lib.ifcs_acl_port_remove(NODE, g.acl, &mut del), "acl_port_remove");
        for hw in delp {
            g.bound.remove(&hw);
        }
    }
    Ok(())
}

/// The shared hostif trap ACL COPY / TRAP actions copy with (queue 0, as isai's): `acl_trap`.
fn acl_trap(core: &Core, cached: &mut Handle) -> Result<Handle> {
    if cached.is_null() {
        let mut q = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
        let mut t = Handle::NULL;
        core.call(
            core.lib.ifcs_hostif_trap_create(NODE, &mut t, &mut q),
            "hostif_trap_create (acl)",
        )?;
        *cached = t;
    }
    Ok(*cached)
}

/// A SAI ACL table's meter of a policer (`umeter_for`).
fn umeter_for(core: &Core, meters: &mut Meters, pid: PolicerId, gid: u32, g: &UGroup) -> Result<Handle> {
    let key = (pid.0, gid);
    if let Some(meter) = meters.user.get(&key) {
        return Ok(*meter);
    }
    let Some(cfg) = meters.policers.get(&pid.0) else {
        return Err(Error::InvalidParameter);
    };
    if !g.meters {
        return Err(Error::NotSupported);
    }
    let mut attrs = meter_create_attrs(cfg)
        .handle(sys::IFCS_ACL_METER_ATTR_ACL_TABLE, g.table)
        .u32(sys::IFCS_ACL_METER_ATTR_SCOPE, sys::IFCS_ACL_SCOPE_NODE);
    let mut meter = Handle::NULL;
    core.call(
        core.lib.ifcs_acl_meter_create(NODE, &mut meter, &mut attrs),
        "acl_meter_create (acl)",
    )?;
    meters.user.insert(key, meter);
    Ok(meter)
}

/// A SAI ACL action (the `action` lambda of `uentry_install`): TYPE, then OBJECT (a non-null
/// object) or VALUE (the struct, or zero), then `ACTION_PROFILE`.
fn uaction(
    core: &Core,
    g: &UGroup,
    e: &mut UEntry,
    set: &mut Vec<Handle>,
    kind: u32,
    value: Option<Vec<u8>>,
    obj: Handle,
) -> Result<()> {
    let mut a = Attrs::new().u32(sys::IFCS_ACL_ACTION_ATTR_TYPE, kind);
    a = if obj.is_null() {
        match value {
            Some(v) => a.bytes(sys::IFCS_ACL_ACTION_ATTR_VALUE, v),
            None => a.of(sys::IFCS_ACL_ACTION_ATTR_VALUE),
        }
    } else {
        a.handle(sys::IFCS_ACL_ACTION_ATTR_OBJECT, obj)
    };
    a = a.handle(sys::IFCS_ACL_ACTION_ATTR_ACTION_PROFILE, g.aprof);
    let mut h = Handle::NULL;
    core.call(
        core.lib.ifcs_acl_action_create(NODE, &mut h, &mut a),
        "acl_action_create (acl)",
    )?;
    set.push(h);
    e.actions.push(h);
    Ok(())
}

/// Installs an entry's ACEs (`uentry_install`).
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "the C++ function, step for step"
)]
fn uentry_install(
    core: &Core,
    g: &UGroup,
    e: &mut UEntry,
    meters: &mut Meters,
    trap_cache: &mut Handle,
    storm_gid: Option<u32>,
    peers: &mut Peers<'_>,
) -> Result<()> {
    let r = e.rule.clone();
    let mut base = uentry_matches(g, &r)?;
    if let Some(hit) = e.dmac_hit {
        base.push(mu8(sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT, hit, 0x01));
    }
    if r.m.l4_src_range.is_some() || r.m.l4_dst_range.is_some() {
        // One comparator per range (OPERATION RANGE, VALUE / HI_VALUE inclusive) in a group
        // matched as COMPARATOR (isai, oracle acl).
        if (r.m.l4_src_range.is_some() && !g.x.src_range) || (r.m.l4_dst_range.is_some() && !g.x.dst_range) {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL entry: L4 port range on a table without that range type"),
            );
            return Err(Error::InvalidParameter);
        }
        let dir = if g.x.egress {
            sys::IFCS_ACL_DIRECTION_EGRESS
        } else {
            sys::IFCS_ACL_DIRECTION_INGRESS
        };
        let mut cmp = |kind: u32, (lo, hi): (u16, u16)| -> Result<()> {
            let mut ca = Attrs::new()
                .u32(sys::IFCS_ACL_COMPARATOR_ATTR_TYPE, kind)
                .u32(sys::IFCS_ACL_COMPARATOR_ATTR_VALUE, u32::from(lo))
                .u32(sys::IFCS_ACL_COMPARATOR_ATTR_HI_VALUE, u32::from(hi))
                .u32(
                    sys::IFCS_ACL_COMPARATOR_ATTR_OPERATION,
                    sys::IFCS_ACL_COMPARATOR_OP_TYPE_RANGE,
                )
                .u32(sys::IFCS_ACL_COMPARATOR_ATTR_DIRECTION, dir);
            let mut made = Handle::NULL;
            core.call(
                core.lib.ifcs_acl_comparator_create(NODE, &mut made, &mut ca),
                "acl_comparator_create",
            )?;
            e.cmps.push(made);
            Ok(())
        };
        let mut st = Ok(());
        if let Some(rg) = r.m.l4_src_range {
            st = cmp(sys::IFCS_ACL_COMPARATOR_TYPE_SOURCE_L4_PORT, rg);
        }
        if let (Ok(()), Some(rg)) = (&st, r.m.l4_dst_range) {
            st = cmp(sys::IFCS_ACL_COMPARATOR_TYPE_DESTINATION_L4_PORT, rg);
        }
        st?;
        let mut ga = Attrs::new().handle_list(sys::IFCS_ACL_COMPARATOR_GROUP_ATTR_COMPARATORS, e.cmps.clone());
        let mut made = Handle::NULL;
        if let Err(err) = core.call(
            core.lib.ifcs_acl_comparator_group_create(NODE, &mut made, &mut ga),
            "acl_comparator_group_create",
        ) {
            e.cmp_group = Handle::NULL;
            return Err(err);
        }
        e.cmp_group = made;
        base.push(mobj(sys::IFCS_ACL_MATCH_TYPE_COMPARATOR, made));
    }
    let mut ports: BTreeSet<u32> = entry_ports(&r).into_iter().collect();
    if g.x.egress && !r.m.out_ports.is_empty() {
        // OUT_PORTS: those of the bound ports
        let o: BTreeSet<u32> =
            r.m.out_ports
                .iter()
                .map(|p| u32::from(p.0))
                .filter(|hw| ports.contains(hw))
                .collect();
        ports = o;
        if ports.is_empty() {
            return Ok(()); // matches nothing here
        }
    }
    e.per_port = ports != g.bound;
    if e.per_port && g.x.egress {
        // egress ACEs have no port match here: an entry applies to every port the table is
        // bound to (the union of its entries' OUT_PORTS / bindings)
        log::error(
            ssys::SAI_API_ACL,
            format_args!("egress ACL entry: covers part of the table's ports (needs a per-port match IFCS refuses)"),
        );
        return Err(Error::NotSupported);
    }
    // one ACE per port when per_port, else one
    let aces_for: Vec<Option<u32>> = if e.per_port {
        ports.iter().copied().map(Some).collect()
    } else {
        vec![None]
    };
    if r.counter && e.counter.is_null() {
        // storm control entries count per colour (diag "acl": what the meter let through)
        let colours = Some(e.gid) == storm_gid;
        let counting = if colours {
            sys::IFCS_ACL_COUNTER_MODE_COLOR_AWARE
        } else {
            sys::IFCS_ACL_COUNTER_MODE_COLOR_BLIND
        };
        let mut ca = Attrs::new()
            .handle(sys::IFCS_ACL_COUNTER_ATTR_ACL_TABLE, g.table)
            .u32(sys::IFCS_ACL_COUNTER_ATTR_MODE, counting);
        let mut made = Handle::NULL;
        if let Err(err) = core.call(
            core.lib.ifcs_acl_counter_create(NODE, &mut made, &mut ca),
            "acl_counter_create (acl)",
        ) {
            e.counter = Handle::NULL;
            return Err(err);
        }
        e.counter = made;
    }
    let mut meter = Handle::NULL;
    let mut trap = Handle::NULL;
    if let Some(pid) = r.policer {
        meter = umeter_for(core, meters, pid, e.gid, g)?;
    }
    if r.copy_cpu {
        trap = acl_trap(core, trap_cache)?;
    }
    let prio = ace_priority(r.priority);
    let first = aces_for.first().copied().flatten();
    for hw in &aces_for {
        let (mut ms, mut set) = (Vec::new(), Vec::new());
        let mut mm = base.clone();
        if let Some(hw) = hw {
            let kind = if g.x.egress {
                sys::IFCS_ACL_MATCH_TYPE_EGRESS_SYSPORT
            } else {
                sys::IFCS_ACL_MATCH_TYPE_INGRESS_L2_INTERFACE
            };
            let object = core.sysport.get(hw).copied().unwrap_or(Handle::NULL);
            if object.is_null() {
                return Err(Error::InvalidPortNumber);
            }
            mm.push(mobj(kind, object));
        }
        for m in &mm {
            let mut attrs = Attrs::new().u32(sys::IFCS_ACL_MATCH_ATTR_TYPE, m.kind);
            attrs = if m.object.is_null() {
                attrs
                    .u8_list(sys::IFCS_ACL_MATCH_ATTR_MASK, m.mask.clone())
                    .u8_list(sys::IFCS_ACL_MATCH_ATTR_VALUE, m.value.clone())
            } else {
                attrs.handle(sys::IFCS_ACL_MATCH_ATTR_OBJECT, m.object)
            };
            attrs = attrs.handle(sys::IFCS_ACL_MATCH_ATTR_MATCH_PROFILE, g.mprof);
            let mut made = Handle::NULL;
            core.call(
                core.lib.ifcs_acl_match_create(NODE, &mut made, &mut attrs),
                "acl_match_create (acl)",
            )?;
            ms.push(made);
            e.matches.push(made);
        }
        let on = sys::IFCS_ACL_ACTION_ENABLE;
        let off = sys::IFCS_ACL_ACTION_DISABLE;
        if r.copy_cpu {
            let v = ctc_value(trap, on, on, on, 0x0f);
            uaction(
                core,
                g,
                e,
                &mut set,
                sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
                Some(v),
                Handle::NULL,
            )?;
        }
        // DROP: all colours (SAI DROP / TRAP), the policer's dropped colours, or none at all
        // (SAI FORWARD: a zero drop action, as isai programs it)
        let (mut dg, mut dy, mut dr) = (0u32, 0u32, 0u32);
        let mut any = r.drop || r.drop_cancel;
        if r.drop {
            (dg, dy, dr) = (on, on, on);
        } else if r.policer.is_some() && (r.green_drop || r.yellow_drop || r.red_drop) {
            dg = if r.green_drop { on } else { off };
            dy = if r.yellow_drop { on } else { off };
            dr = if r.red_drop { on } else { off };
            any = true;
        }
        if any || (!r.copy_cpu && !r.counter && r.policer.is_none()) {
            let dirty = if dg != 0 || dy != 0 || dr != 0 { 0x07 } else { 0 };
            let v = (dirty != 0).then(|| drop_value(dg, dy, dr, dirty));
            uaction(core, g, e, &mut set, sys::IFCS_ACL_ACTION_TYPE_DROP, v, Handle::NULL)?;
        }
        if let Some(target) = r.redirect {
            let (dh, raw) = match target {
                Redirect::Egress(id) => (peers.l3.nexthop_handle(id), i64::from(id.0)),
                Redirect::Dest(d) => (peers.lag.dest_handle(core, d), i64::from(d.raw())),
            };
            let Some(dh) = dh else {
                log::error(
                    ssys::SAI_API_ACL,
                    format_args!("ACL entry: redirect target {raw} has no IFCS destination (unresolved next hop?)"),
                );
                return Err(Error::InvalidParameter);
            };
            uaction(core, g, e, &mut set, sys::IFCS_ACL_ACTION_TYPE_REDIRECT, None, dh)?;
        }
        if let Some(tc) = r.set_tc {
            let v = set_tc_value(u32::from(tc));
            uaction(
                core,
                g,
                e,
                &mut set,
                sys::IFCS_ACL_ACTION_TYPE_SET_TC,
                Some(v),
                Handle::NULL,
            )?;
        }
        if let Some(dscp) = r.set_dscp {
            let v = set_dscp_value(g.v6, u32::from(dscp));
            uaction(
                core,
                g,
                e,
                &mut set,
                sys::IFCS_ACL_ACTION_TYPE_SET_DSCP,
                Some(v),
                Handle::NULL,
            )?;
        }
        // mirror: the session's collector set (MirrorArea)
        for (ids, ing) in [(&r.mirror_ingress, true), (&r.mirror_egress, false)] {
            let Some(&id) = ids.first() else { continue };
            if ids.len() > 1 {
                core.log_once(
                    "acl:mirror-multi",
                    format_args!("ACL entry with several mirror sessions: the first one is used"),
                );
            }
            let cs = peers.mirror.collector_set(core, id, ing)?;
            if hw.is_none() || *hw == first {
                // one hold per entry and direction
                e.mirrors.push((id, ing));
            } else {
                // held already: the same set
                peers.mirror.collector_set_release(core, id, ing);
            }
            uaction(core, g, e, &mut set, sys::IFCS_ACL_ACTION_TYPE_MIRROR, None, cs)?;
        }
        if !meter.is_null() {
            uaction(core, g, e, &mut set, sys::IFCS_ACL_ACTION_TYPE_METER, None, meter)?;
        }
        if !e.counter.is_null() {
            let c = e.counter;
            uaction(core, g, e, &mut set, sys::IFCS_ACL_ACTION_TYPE_ACL_COUNTER, None, c)?;
        }
        let mut ea = Attrs::new()
            .u32(sys::IFCS_ACE_ATTR_PRIORITY, prio)
            .data(sys::IFCS_ACE_ATTR_ENABLE, e.enabled)
            .handle_list(sys::IFCS_ACE_ATTR_MATCH_SET, ms)
            .handle_list(sys::IFCS_ACE_ATTR_ACTION_SET, set);
        let mut ace = Handle::NULL;
        core.call(core.lib.ifcs_ace_create(NODE, &mut ace, &mut ea), "ace_create (acl)")?;
        if let Err(err) = core.call(
            core.lib.ifcs_acl_ace_add(NODE, g.acl, &mut [ace], &mut Attrs::null()),
            "acl_ace_add (acl)",
        ) {
            let _ = core.call(core.lib.ifcs_ace_delete(NODE, ace), "ace_delete");
            return Err(err);
        }
        e.aces.push(ace);
    }
    Ok(())
}

/// Takes an entry's ACEs and objects away (`uentry_uninstall`); the counter stays with
/// `keep_counter`.
fn uentry_uninstall(core: &Core, g: &UGroup, e: &mut UEntry, keep_counter: bool, mirror: &mut dyn MirrorSets) {
    if !e.aces.is_empty() {
        let _ = core.call(core.lib.ifcs_acl_ace_remove(NODE, g.acl, &mut e.aces), "acl_ace_remove");
    }
    for h in &e.aces {
        let _ = core.call(core.lib.ifcs_ace_delete(NODE, *h), "ace_delete");
    }
    for h in &e.actions {
        let _ = core.call(core.lib.ifcs_acl_action_delete(NODE, *h), "acl_action_delete");
    }
    for h in &e.matches {
        let _ = core.call(core.lib.ifcs_acl_match_delete(NODE, *h), "acl_match_delete");
    }
    e.aces.clear();
    e.actions.clear();
    e.matches.clear();
    if !e.cmp_group.is_null() {
        let _ = core.call(
            core.lib.ifcs_acl_comparator_group_delete(NODE, e.cmp_group),
            "acl_comparator_group_delete",
        );
    }
    for h in &e.cmps {
        let _ = core.call(core.lib.ifcs_acl_comparator_delete(NODE, *h), "acl_comparator_delete");
    }
    e.cmp_group = Handle::NULL;
    e.cmps.clear();
    for (id, ing) in e.mirrors.drain(..) {
        mirror.collector_set_release(core, id, ing);
    }
    if !keep_counter && !e.counter.is_null() {
        let _ = core.call(core.lib.ifcs_acl_counter_delete(NODE, e.counter), "acl_counter_delete");
        e.counter = Handle::NULL;
    }
}

/// `snprintf(b, 256, ...)` of the diag reports: a line cut to 255 bytes (its newline then lost).
pub fn line256(s: &str) -> &str {
    if s.len() <= 255 {
        return s;
    }
    let mut end = 255;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or(s)
}

impl AclArea {
    // ---------------------------------------------------------------- policers

    /// `policer_create`: a backend object (no IFCS call until a trap or entry uses it).
    pub fn policer_create(&mut self, cfg: &PolicerCfg) -> PolicerId {
        let pid = self.meters.next_policer;
        self.meters.next_policer += 1;
        self.meters.policers.insert(pid, *cfg);
        PolicerId(pid)
    }

    /// Whether the policer exists.
    pub fn has_policer(&self, pid: PolicerId) -> bool {
        self.meters.policers.contains_key(&pid.0)
    }

    /// `policer_set`: the new config, every meter of it updated one attribute per call (as isai
    /// updates a meter): the `CoPP` meters, then the ACL tables' ones; the last failure is answered.
    /// The caller then tells the `sFlow` area (`SflowArea::policer_changed`, as the C++ does here).
    pub fn policer_set(&mut self, core: &Core, pid: PolicerId, cfg: &PolicerCfg) -> Result<()> {
        let Some(p) = self.meters.policers.get_mut(&pid.0) else {
            return Err(Error::ItemNotFound);
        };
        *p = *cfg;
        let a = meter_attrs(cfg);
        let mut worst = Ok(());
        let all: Vec<(u32, Handle)> = self
            .meters
            .copp
            .iter()
            .map(|(k, h)| (k.0, *h))
            .chain(self.meters.user.iter().map(|(k, h)| (k.0, *h)))
            .collect();
        for (p, h) in all {
            if p != pid.0 {
                continue;
            }
            for (id, v) in &a {
                let mut x = Attrs::new().u64(*id, *v);
                if let Err(e) = core.call(
                    core.lib.ifcs_acl_meter_attr_set(NODE, h.0, &mut x),
                    "acl_meter_attr_set",
                ) {
                    worst = Err(e);
                }
            }
        }
        worst
    }

    /// `policer_get`: a policer's config (the `sFlow` area's sample policer).
    pub fn policer_get(&self, pid: PolicerId) -> Option<PolicerCfg> {
        self.meters.policers.get(&pid.0).copied()
    }

    /// `policer_destroy`: refused while a `CoPP` install or an entry uses it; its meters deleted.
    pub fn policer_destroy(&mut self, core: &Core, pid: PolicerId) -> Result<()> {
        if self.copp.installs.values().any(|i| i.policer == Some(pid))
            || self.uentries.values().any(|e| e.rule.policer == Some(pid))
        {
            return Err(Error::ObjectInUse);
        }
        let users: Vec<(u32, u32)> = self.meters.user.keys().filter(|k| k.0 == pid.0).copied().collect();
        for k in users {
            if let Some(h) = self.meters.user.remove(&k) {
                let _ = core.call(core.lib.ifcs_acl_meter_delete(NODE, h), "acl_meter_delete");
            }
        }
        let copps: Vec<(u32, u32)> = self.meters.copp.keys().filter(|k| k.0 == pid.0).copied().collect();
        for k in copps {
            if let Some(h) = self.meters.copp.remove(&k) {
                let _ = core.call(core.lib.ifcs_acl_meter_delete(NODE, h), "acl_meter_delete");
            }
        }
        if self.meters.policers.remove(&pid.0).is_some() {
            Ok(())
        } else {
            Err(Error::ItemNotFound)
        }
    }

    // ------------------------------------------------------------------- CoPP

    /// `copp_init`: the `CoPP` table on first use (match and action profile, the table in the first
    /// form the SDK takes, one ACL per inno block probed).
    #[allow(clippy::too_many_lines, reason = "the C++ function, step for step")]
    fn copp_init(&mut self, core: &Core) -> Result<()> {
        if self.copp.up {
            return Ok(());
        }
        let mt = vec![
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS,
            sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE,
            sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL,
            sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT,
            sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT,
            sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1,
            sys::IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP,
        ];
        let at = vec![
            sys::IFCS_ACL_ACTION_TYPE_DROP,
            sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
            sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE,
            sys::IFCS_ACL_ACTION_TYPE_METER,
            sys::IFCS_ACL_ACTION_TYPE_ACL_COUNTER,
        ];
        let mut mp = Attrs::new()
            .u32_list(sys::IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES, mt)
            .u32(
                sys::IFCS_ACL_MATCH_PROFILE_ATTR_DIRECTION,
                sys::IFCS_ACL_DIRECTION_INGRESS,
            )
            .u32(sys::IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH, 320);
        // IFCS creates need a null handle to allocate one
        self.copp.mprof = Handle::NULL;
        self.copp.aprof = Handle::NULL;
        self.copp.table = Handle::NULL;
        if let Err(e) = core.call(
            core.lib
                .ifcs_acl_match_profile_create(NODE, &mut self.copp.mprof, &mut mp),
            "acl_match_profile_create (copp)",
        ) {
            self.copp.mprof = Handle::NULL;
            return Err(e);
        }
        let mut ap = Attrs::new()
            .u32_list(sys::IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES, at)
            .u32(
                sys::IFCS_ACL_ACTION_PROFILE_ATTR_DIRECTION,
                sys::IFCS_ACL_DIRECTION_INGRESS,
            );
        if let Err(e) = core.call(
            core.lib
                .ifcs_acl_action_profile_create(NODE, &mut self.copp.aprof, &mut ap),
            "acl_action_profile_create (copp)",
        ) {
            self.copp.aprof = Handle::NULL;
            self.copp_rollback(core);
            return Err(e);
        }
        // The table as isai creates it, then simpler forms (this SDK refuses some attributes isai
        // 6.2 uses: each refused form is logged, I).
        let forms: [(&str, bool, bool, bool); 4] = [
            (
                "per IB, isai form (type control, meter/counter maxima)",
                true,
                true,
                false,
            ),
            ("per IB, without type", false, true, false),
            ("node scope, isai form", true, true, true),
            ("node scope, without type", false, true, true),
        ];
        let mut r = sys::IFCS_PARAM;
        for (what, kind, maxes, node) in forms {
            let scope = if node {
                sys::IFCS_ACL_SCOPE_NODE
            } else {
                sys::IFCS_ACL_SCOPE_PER_IB
            };
            let mut tv = Attrs::new()
                .handle(sys::IFCS_ACL_TABLE_ATTR_MATCH_PROFILE, self.copp.mprof)
                .handle(sys::IFCS_ACL_TABLE_ATTR_ACTION_PROFILE, self.copp.aprof)
                .u32(sys::IFCS_ACL_TABLE_ATTR_DIRECTION, sys::IFCS_ACL_DIRECTION_INGRESS)
                .u32(sys::IFCS_ACL_TABLE_ATTR_SCOPE, scope)
                .u32(sys::IFCS_ACL_TABLE_ATTR_SIZE, COPP_TABLE_SIZE);
            if maxes {
                // own counters per entry need the table's default counters off
                tv = tv
                    .u32(sys::IFCS_ACL_TABLE_ATTR_MAX_METERS, COPP_TABLE_SIZE)
                    .u32(sys::IFCS_ACL_TABLE_ATTR_MAX_COUNTERS, COPP_TABLE_SIZE)
                    .data(sys::IFCS_ACL_TABLE_ATTR_DEFAULT_STATS, false);
            }
            if kind {
                tv = tv.u32(sys::IFCS_ACL_TABLE_ATTR_TYPE, sys::IFCS_ACL_TABLE_TYPE_CONTROL);
            }
            tv = tv.u32(sys::IFCS_ACL_TABLE_ATTR_PRIORITY, COPP_TABLE_PRIORITY);
            self.copp.table = Handle::NULL;
            r = core
                .lib
                .ifcs_acl_table_create(NODE, &mut self.copp.table, &mut tv)
                .reason();
            if r == sys::IFCS_SUCCESS {
                log::notice(ssys::SAI_API_HOSTIF, format_args!("CoPP ACL table: {what}"));
                self.copp.node_scope = node;
                break;
            }
            log::warn(
                ssys::SAI_API_HOSTIF,
                format_args!("CoPP ACL table, {what}: {}", ifcs::status::reason_name(r)),
            );
            self.copp.table = Handle::NULL;
        }
        if r != sys::IFCS_SUCCESS {
            self.copp_rollback(core);
            return Err(reason_err(r));
        }
        // One ACL per inno block: probe until the SDK refuses an IB number (node scope: one ACL).
        self.copp.acls.clear();
        let scope = if self.copp.node_scope {
            sys::IFCS_ACL_SCOPE_NODE
        } else {
            sys::IFCS_ACL_SCOPE_PER_IB
        };
        for ib in 0..(if self.copp.node_scope { 1 } else { MAX_IBS }) {
            let mut a = Attrs::new().u32(sys::IFCS_ACL_ATTR_SCOPE, scope);
            if !self.copp.node_scope {
                a = a.u32(sys::IFCS_ACL_ATTR_IB, ib);
            }
            let mut h = Handle::NULL;
            let rr = core.lib.ifcs_acl_create(NODE, &mut h, &mut a).reason();
            if rr != sys::IFCS_SUCCESS {
                if ib == 0 {
                    log::error(
                        ssys::SAI_API_HOSTIF,
                        format_args!("CoPP: acl_create (IB 0): {}", ifcs::status::reason_name(rr)),
                    );
                }
                break;
            }
            self.copp.acls.push(h);
        }
        if self.copp.acls.is_empty() {
            log::error(
                ssys::SAI_API_HOSTIF,
                format_args!("CoPP: no ACL could be created (IB 0 refused)"),
            );
            self.copp_rollback(core);
            return Err(Error::Failure);
        }
        let mut acls = self.copp.acls.clone();
        if let Err(e) = core.call(
            core.lib
                .ifcs_acl_table_acl_add(NODE, self.copp.table, &mut acls, &mut Attrs::null()),
            "acl_table_acl_add (copp)",
        ) {
            self.copp_rollback(core);
            return Err(e);
        }
        self.copp.up = true;
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!("CoPP ACL table ready: {} inno blocks", self.copp.acls.len()),
        );
        Ok(())
    }

    /// `copp_rollback`: a failed `CoPP` table setup takes back what it created (the next trap tries
    /// again).
    fn copp_rollback(&mut self, core: &Core) {
        let c = &mut self.copp;
        for h in &c.acls {
            let _ = core.call(core.lib.ifcs_acl_delete(NODE, *h), "acl_delete (copp rollback)");
        }
        c.acls.clear();
        if !c.table.is_null() {
            let _ = core.call(
                core.lib.ifcs_acl_table_delete(NODE, c.table),
                "acl_table_delete (copp rollback)",
            );
        }
        if !c.aprof.is_null() {
            let _ = core.call(
                core.lib.ifcs_acl_action_profile_delete(NODE, c.aprof),
                "acl_action_profile_delete (copp rollback)",
            );
        }
        if !c.mprof.is_null() {
            let _ = core.call(
                core.lib.ifcs_acl_match_profile_delete(NODE, c.mprof),
                "acl_match_profile_delete (copp rollback)",
            );
        }
        c.mprof = Handle::NULL;
        c.aprof = Handle::NULL;
        c.table = Handle::NULL;
    }

    /// `copp_prio`: an offset into the chip's ACL entry priority range (capped at its maximum).
    pub fn copp_prio(limits: &ChipLimits, offset: u32) -> u32 {
        let p = limits.acl_prio_min.wrapping_add(offset);
        if limits.acl_prio_max > limits.acl_prio_min && p > limits.acl_prio_max {
            limits.acl_prio_max
        } else {
            p
        }
    }

    /// The number of inno blocks the `CoPP` table has an ACL in (`copp_ibs`).
    pub fn copp_ibs(&self) -> usize {
        self.copp.acls.len()
    }

    /// `meter_for`: the `CoPP` meter of a policer in an IB, created on first use.
    fn meter_for(&mut self, core: &Core, pid: PolicerId, ib: u32) -> Result<Handle> {
        let key = (pid.0, ib);
        if let Some(meter) = self.meters.copp.get(&key) {
            return Ok(*meter);
        }
        let Some(cfg) = self.meters.policers.get(&pid.0) else {
            return Err(Error::InvalidParameter);
        };
        let copp = &self.copp;
        let scope = if copp.node_scope {
            sys::IFCS_ACL_SCOPE_NODE
        } else {
            sys::IFCS_ACL_SCOPE_PER_IB
        };
        let mut attrs = meter_create_attrs(cfg)
            .handle(sys::IFCS_ACL_METER_ATTR_ACL_TABLE, copp.table)
            .u32(sys::IFCS_ACL_METER_ATTR_SCOPE, scope);
        if !copp.node_scope {
            attrs = attrs.u32(sys::IFCS_ACL_METER_ATTR_IB, ib);
        }
        let mut meter = Handle::NULL;
        core.call(
            core.lib.ifcs_acl_meter_create(NODE, &mut meter, &mut attrs),
            "acl_meter_create",
        )?;
        self.meters.copp.insert(key, meter);
        Ok(meter)
    }

    /// A `CoPP` action (the `action` lambda of `install_ib`): `ACTION_PROFILE`, TYPE, then VALUE
    /// (the struct) or OBJECT.
    fn copp_action(
        &self,
        core: &Core,
        out: &mut PerIb,
        set: &mut Vec<Handle>,
        kind: u32,
        value: Option<Vec<u8>>,
        obj: Handle,
    ) -> Result<()> {
        let mut a = Attrs::new()
            .handle(sys::IFCS_ACL_ACTION_ATTR_ACTION_PROFILE, self.copp.aprof)
            .u32(sys::IFCS_ACL_ACTION_ATTR_TYPE, kind);
        a = match value {
            Some(v) => a.bytes(sys::IFCS_ACL_ACTION_ATTR_VALUE, v),
            None => a.handle(sys::IFCS_ACL_ACTION_ATTR_OBJECT, obj),
        };
        let mut h = Handle::NULL;
        core.call(
            core.lib.ifcs_acl_action_create(NODE, &mut h, &mut a),
            "acl_action_create",
        )?;
        set.push(h);
        out.actions.push(h);
        Ok(())
    }

    /// `install_ib`: one install's counter (colour aware), meter, matches, actions and ACEs in one IB,
    /// the ACEs added to its ACL. `out` holds what was made, also on an error.
    #[allow(clippy::too_many_lines, reason = "the C++ function, step for step")]
    fn install_ib(
        &mut self,
        core: &Core,
        rules: &[CoppRule],
        prio: u32,
        act: &CoppActions,
        ib: u32,
        out: &mut PerIb,
    ) -> Result<()> {
        out.ib = ib;
        let node_scope = self.copp.node_scope;
        // counter (colour-aware), shared by this install's ACEs in this IB
        let scope = if node_scope {
            sys::IFCS_ACL_SCOPE_NODE
        } else {
            sys::IFCS_ACL_SCOPE_PER_IB
        };
        let mut ca = Attrs::new()
            .handle(sys::IFCS_ACL_COUNTER_ATTR_ACL_TABLE, self.copp.table)
            .u32(sys::IFCS_ACL_COUNTER_ATTR_SCOPE, scope)
            .u32(sys::IFCS_ACL_COUNTER_ATTR_MODE, sys::IFCS_ACL_COUNTER_MODE_COLOR_AWARE);
        if !node_scope {
            ca = ca.u32(sys::IFCS_ACL_COUNTER_ATTR_IB, ib);
        }
        core.call(
            core.lib.ifcs_acl_counter_create(NODE, &mut out.counter, &mut ca),
            "acl_counter_create",
        )?;
        let mut meter = Handle::NULL;
        if let Some(pid) = act.policer {
            meter = self.meter_for(core, pid, ib)?;
        }
        let on = sys::IFCS_ACL_ACTION_ENABLE;
        let off = sys::IFCS_ACL_ACTION_DISABLE;
        for rule in rules {
            let (mut ms, mut set) = (Vec::new(), Vec::new());
            for m in rule {
                let mut a = Attrs::new().u32(sys::IFCS_ACL_MATCH_ATTR_TYPE, m.kind);
                a = if m.object.is_null() {
                    a.u8_list(sys::IFCS_ACL_MATCH_ATTR_VALUE, m.value.clone())
                } else {
                    a.handle(sys::IFCS_ACL_MATCH_ATTR_OBJECT, m.object)
                };
                a = a.handle(sys::IFCS_ACL_MATCH_ATTR_MATCH_PROFILE, self.copp.mprof);
                let mut h = Handle::NULL;
                core.call(core.lib.ifcs_acl_match_create(NODE, &mut h, &mut a), "acl_match_create")?;
                ms.push(h);
                out.matches.push(h);
            }
            let policed = act.policer.is_some();
            if !act.trap.is_null() {
                let yellow = if policed && act.yellow_drop { off } else { on };
                let red = if policed && act.red_drop { off } else { on };
                let v = ctc_value(act.trap, on, yellow, red, 0x0f);
                self.copp_action(
                    core,
                    out,
                    &mut set,
                    sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
                    Some(v),
                    Handle::NULL,
                )?;
                if policed && (act.yellow_drop || act.red_drop) {
                    let v = ctc_override_value(
                        0,
                        if act.yellow_drop { on } else { off },
                        if act.red_drop { on } else { off },
                        0x07,
                    );
                    self.copp_action(
                        core,
                        out,
                        &mut set,
                        sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE,
                        Some(v),
                        Handle::NULL,
                    )?;
                }
            }
            if act.drop {
                let v = drop_value(on, on, on, 0x07);
                self.copp_action(
                    core,
                    out,
                    &mut set,
                    sys::IFCS_ACL_ACTION_TYPE_DROP,
                    Some(v),
                    Handle::NULL,
                )?;
            }
            if !meter.is_null() {
                self.copp_action(core, out, &mut set, sys::IFCS_ACL_ACTION_TYPE_METER, None, meter)?;
            }
            let counter = out.counter;
            self.copp_action(
                core,
                out,
                &mut set,
                sys::IFCS_ACL_ACTION_TYPE_ACL_COUNTER,
                None,
                counter,
            )?;
            let mut e = Attrs::new()
                .u32(sys::IFCS_ACE_ATTR_SCOPE, scope)
                .u32(sys::IFCS_ACE_ATTR_PRIORITY, prio)
                .data(sys::IFCS_ACE_ATTR_ENABLE, true)
                .handle_list(sys::IFCS_ACE_ATTR_MATCH_SET, ms)
                .handle_list(sys::IFCS_ACE_ATTR_ACTION_SET, set);
            if !node_scope {
                e = e.u32(sys::IFCS_ACE_ATTR_IB, ib);
            }
            let mut ace = Handle::NULL;
            core.call(core.lib.ifcs_ace_create(NODE, &mut ace, &mut e), "ace_create")?;
            out.aces.push(ace);
        }
        let mut st = Ok(());
        if !out.aces.is_empty() {
            let acl = self
                .copp
                .acls
                .get(usize::try_from(ib).unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(Handle::NULL);
            let mut aces = out.aces.clone();
            st = core.call(
                core.lib.ifcs_acl_ace_add(NODE, acl, &mut aces, &mut Attrs::null()),
                "acl_ace_add",
            );
        }
        if st.is_err() {
            out.aces.clear(); // not in the ACL: remove_ib must not acl_ace_remove them
        }
        st
    }

    /// `remove_ib`.
    fn remove_ib(&self, core: &Core, p: &mut PerIb) {
        let acl = usize::try_from(p.ib).ok().and_then(|i| self.copp.acls.get(i)).copied();
        if let (false, Some(acl)) = (p.aces.is_empty(), acl) {
            let _ = core.call(core.lib.ifcs_acl_ace_remove(NODE, acl, &mut p.aces), "acl_ace_remove");
        }
        for h in &p.aces {
            let _ = core.call(core.lib.ifcs_ace_delete(NODE, *h), "ace_delete");
        }
        for h in &p.matches {
            let _ = core.call(core.lib.ifcs_acl_match_delete(NODE, *h), "acl_match_delete");
        }
        for h in &p.actions {
            let _ = core.call(core.lib.ifcs_acl_action_delete(NODE, *h), "acl_action_delete");
        }
        if !p.counter.is_null() {
            let _ = core.call(core.lib.ifcs_acl_counter_delete(NODE, p.counter), "acl_counter_delete");
        }
        *p = PerIb::default();
    }

    /// `copp_install`: the rules (one ACE per rule in every inno block's ACL) with the actions, at ACE
    /// priority `prio` (lower wins, I); the install's id for removal.
    pub fn copp_install(&mut self, core: &Core, rules: &[CoppRule], prio: u32, a: &CoppActions) -> Result<u32> {
        self.copp_init(core)?;
        if let Some(pid) = a.policer {
            if !self.meters.policers.contains_key(&pid.0) {
                return Err(Error::InvalidParameter);
            }
        }
        let mut inst = Install {
            ibs: Vec::new(),
            policer: a.policer,
        };
        for ib in 0..self.copp.acls.len() {
            let ib = u32::try_from(ib).unwrap_or(u32::MAX);
            let mut p = PerIb::default();
            let st = self.install_ib(core, rules, prio, a, ib, &mut p);
            inst.ibs.push(p);
            if let Err(e) = st {
                for x in &mut inst.ibs {
                    self.remove_ib(core, x);
                }
                return Err(e);
            }
        }
        let id = self.copp.next_install;
        self.copp.next_install += 1;
        self.copp.installs.insert(id, inst);
        Ok(id)
    }

    /// `copp_remove`.
    pub fn copp_remove(&mut self, core: &Core, id: u32) -> Result<()> {
        let Some(mut inst) = self.copp.installs.remove(&id) else {
            return Err(Error::ItemNotFound);
        };
        for p in &mut inst.ibs {
            self.remove_ib(core, p);
        }
        Ok(())
    }

    /// `copp_counters`: packets the install's entries matched (all IBs): total, green, yellow, red; on a
    /// failed read the sums of the IBs before it (the C++'s out parameters), with the error.
    pub fn copp_counters(&self, core: &Core, id: u32) -> ([u64; 4], Result<()>) {
        let Some(inst) = self.copp.installs.get(&id) else {
            return ([0; 4], Err(Error::ItemNotFound));
        };
        let mut sum = [0u64; 4];
        for p in &inst.ibs {
            let mut ids = [
                sys::IFCS_ACL_COUNTER_STATS_ID_PACKETS,
                sys::IFCS_ACL_COUNTER_STATS_ID_GREEN_PACKETS,
                sys::IFCS_ACL_COUNTER_STATS_ID_YELLOW_PACKETS,
                sys::IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS,
            ]
            .map(|i| i32::try_from(i).unwrap_or(i32::MAX));
            let mut v = [0u64; 4];
            let (rc, _) = core.lib.ifcs_acl_counter_stats_get(NODE, p.counter, &mut ids, &mut v);
            if let Err(e) = core.call(rc, "acl_counter_stats_get") {
                return (sum, Err(e)); // the IBs before it summed, as the C++ leaves them
            }
            for (s, x) in sum.iter_mut().zip(v) {
                *s = s.wrapping_add(x);
            }
        }
        (sum, Ok(()))
    }

    // ------------------------------------------------------- SAI ACL tables

    /// `group_create`.
    pub fn group_create(&mut self, core: &Core, qset: &[FpQual], priority: i32) -> Result<FpGroupId> {
        self.group_make(core, qset, priority, FpGroupExtra::default(), false)
            .map(FpGroupId)
    }

    /// `group_create_ex`: egress tables, L4 port ranges, `OUT_PORTS`, REDIRECT / `SET_TC` / `SET_DSCP` /
    /// MIRROR actions.
    pub fn group_create_ex(
        &mut self,
        core: &Core,
        qset: &[FpQual],
        priority: i32,
        x: FpGroupExtra,
    ) -> Result<FpGroupId> {
        if x.out_ports && !x.egress {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL table: OUT_PORTS only in egress tables here"),
            );
            return Err(Error::NotSupported);
        }
        if x.egress && (x.redirect || x.set_tc || x.set_dscp || x.mirror) {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL table: egress tables take drop / forward / counter actions only"),
            );
            return Err(Error::NotSupported);
        }
        self.group_make(core, qset, priority, x, false).map(FpGroupId)
    }

    /// `group_make`: the match profile in the smallest width the SDK takes, the action profile and
    /// table in the first form it takes (with meters, without, smaller), one ACL in it.
    #[allow(clippy::too_many_lines, reason = "the C++ function, step for step")]
    fn group_make(
        &mut self,
        core: &Core,
        qset: &[FpQual],
        priority: i32,
        x: FpGroupExtra,
        dmac_hit: bool,
    ) -> Result<u32> {
        let dir = if x.egress {
            sys::IFCS_ACL_DIRECTION_EGRESS
        } else {
            sys::IFCS_ACL_DIRECTION_INGRESS
        };
        if priority >= COPP_FP_PRIORITY {
            // CoPP is programmed by HostifArea::trap_program; the frontend's field entry CoPP
            // (traps without a mapping) would police without a copy.
            core.log_once(
                "acl:copp-fp",
                format_args!("CoPP field group refused: trap types without an IFCS mapping stay unpoliced"),
            );
            return Err(Error::NotSupported);
        }
        let mut u = UGroup {
            qset: qset.to_vec(),
            x,
            v6: qset.iter().any(|q| matches!(q, FpQual::SrcIp6 | FpQual::DstIp6)),
            ..UGroup::default()
        };
        let mt = match match_types(qset, x.egress, dmac_hit) {
            Ok(mt) => mt,
            Err(q) => {
                log::error(
                    ssys::SAI_API_ACL,
                    format_args!("ACL table: qualifier {} has no IFCS match here", qual_num(q)),
                );
                return Err(Error::NotSupported);
            }
        };
        let try_widths = widths(key_bits(&mt, &x));
        // With a meter (SET_POLICER) first, then without it, then smaller; per key width.
        let forms: [(bool, u32); 3] = [(true, UACL_SIZE), (false, UACL_SIZE), (false, 128)];
        let tprio = table_priority(priority, x.egress);
        let mut r = sys::IFCS_PARAM;
        let mut width = 0;
        for w in try_widths {
            let mut mp = Attrs::new()
                .u32(sys::IFCS_ACL_MATCH_PROFILE_ATTR_DIRECTION, dir)
                .u32(sys::IFCS_ACL_MATCH_PROFILE_ATTR_PORT_SHARING, 1)
                .u32_list(sys::IFCS_ACL_MATCH_PROFILE_ATTR_MATCH_TYPES, mt.clone())
                .u32(sys::IFCS_ACL_MATCH_PROFILE_ATTR_MAX_WIDTH, w);
            if x.src_range {
                mp = mp.data(sys::IFCS_ACL_MATCH_PROFILE_ATTR_SRC_L4_PORT_COMPARATOR, true);
            }
            if x.dst_range {
                mp = mp.data(sys::IFCS_ACL_MATCH_PROFILE_ATTR_DST_L4_PORT_COMPARATOR, true);
            }
            u.mprof = Handle::NULL;
            r = core
                .lib
                .ifcs_acl_match_profile_create(NODE, &mut u.mprof, &mut mp)
                .reason();
            if r != sys::IFCS_SUCCESS {
                log::warn(
                    ssys::SAI_API_ACL,
                    format_args!(
                        "ACL match profile, {} types, width {w}: {}",
                        mt.len(),
                        ifcs::status::reason_name(r)
                    ),
                );
                u.mprof = Handle::NULL;
                continue;
            }
            for (meters, size) in forms {
                let mut at = vec![
                    sys::IFCS_ACL_ACTION_TYPE_DROP,
                    sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU,
                    sys::IFCS_ACL_ACTION_TYPE_COPY_TO_CPU_OVERRIDE,
                    sys::IFCS_ACL_ACTION_TYPE_ACL_COUNTER,
                ];
                if meters {
                    at.push(sys::IFCS_ACL_ACTION_TYPE_METER);
                }
                if x.egress {
                    at = vec![sys::IFCS_ACL_ACTION_TYPE_DROP, sys::IFCS_ACL_ACTION_TYPE_ACL_COUNTER];
                }
                if x.redirect {
                    at.push(sys::IFCS_ACL_ACTION_TYPE_REDIRECT);
                }
                if x.set_tc {
                    at.push(sys::IFCS_ACL_ACTION_TYPE_SET_TC);
                }
                if x.set_dscp {
                    at.push(sys::IFCS_ACL_ACTION_TYPE_SET_DSCP);
                }
                if x.mirror {
                    at.push(sys::IFCS_ACL_ACTION_TYPE_MIRROR);
                }
                if x.egress && meters {
                    continue; // no meters on egress tables
                }
                let mut ap = Attrs::new()
                    .u32(sys::IFCS_ACL_ACTION_PROFILE_ATTR_DIRECTION, dir)
                    .u32_list(sys::IFCS_ACL_ACTION_PROFILE_ATTR_ACTION_TYPES, at);
                u.aprof = Handle::NULL;
                r = core
                    .lib
                    .ifcs_acl_action_profile_create(NODE, &mut u.aprof, &mut ap)
                    .reason();
                if r != sys::IFCS_SUCCESS {
                    u.aprof = Handle::NULL;
                    continue;
                }
                let mut tv = Attrs::new()
                    .u32(sys::IFCS_ACL_TABLE_ATTR_DIRECTION, dir)
                    .u32(sys::IFCS_ACL_TABLE_ATTR_SIZE, size)
                    .data(sys::IFCS_ACL_TABLE_ATTR_DEFAULT_STATS, false)
                    .u32(sys::IFCS_ACL_TABLE_ATTR_MAX_COUNTERS, size)
                    .handle(sys::IFCS_ACL_TABLE_ATTR_MATCH_PROFILE, u.mprof)
                    .handle(sys::IFCS_ACL_TABLE_ATTR_ACTION_PROFILE, u.aprof)
                    .u32(sys::IFCS_ACL_TABLE_ATTR_SCOPE, sys::IFCS_ACL_SCOPE_NODE)
                    .u32(sys::IFCS_ACL_TABLE_ATTR_PRIORITY, tprio);
                if meters {
                    tv = tv.u32(sys::IFCS_ACL_TABLE_ATTR_MAX_METERS, size);
                }
                u.table = Handle::NULL;
                r = core.lib.ifcs_acl_table_create(NODE, &mut u.table, &mut tv).reason();
                if r == sys::IFCS_SUCCESS {
                    u.meters = meters;
                    u.size = size;
                    break;
                }
                log::warn(
                    ssys::SAI_API_ACL,
                    format_args!(
                        "ACL table (width {w}, size {size}{}): {}",
                        if meters { ", meters" } else { "" },
                        ifcs::status::reason_name(r)
                    ),
                );
                u.table = Handle::NULL;
                let _ = core.call(
                    core.lib.ifcs_acl_action_profile_delete(NODE, u.aprof),
                    "acl_action_profile_delete",
                );
                u.aprof = Handle::NULL;
            }
            if r == sys::IFCS_SUCCESS {
                width = w;
                break;
            }
            let _ = core.call(
                core.lib.ifcs_acl_match_profile_delete(NODE, u.mprof),
                "acl_match_profile_delete",
            );
            u.mprof = Handle::NULL;
        }
        if u.mprof.is_null() {
            return Err(reason_err(r));
        }
        let undo = |u: &UGroup| {
            if !u.acl.is_null() {
                let _ = core.call(core.lib.ifcs_acl_delete(NODE, u.acl), "acl_delete");
            }
            if !u.table.is_null() {
                let _ = core.call(core.lib.ifcs_acl_table_delete(NODE, u.table), "acl_table_delete");
            }
            if !u.aprof.is_null() {
                let _ = core.call(
                    core.lib.ifcs_acl_action_profile_delete(NODE, u.aprof),
                    "acl_action_profile_delete",
                );
            }
            if !u.mprof.is_null() {
                let _ = core.call(
                    core.lib.ifcs_acl_match_profile_delete(NODE, u.mprof),
                    "acl_match_profile_delete",
                );
            }
        };
        if r != sys::IFCS_SUCCESS {
            undo(&u);
            return Err(reason_err(r));
        }
        let mut aa = Attrs::new().u32(sys::IFCS_ACL_ATTR_SCOPE, sys::IFCS_ACL_SCOPE_NODE);
        u.acl = Handle::NULL;
        let mut st = core.call(core.lib.ifcs_acl_create(NODE, &mut u.acl, &mut aa), "acl_create (acl)");
        if st.is_ok() {
            st = core.call(
                core.lib
                    .ifcs_acl_table_acl_add(NODE, u.table, &mut [u.acl], &mut Attrs::null()),
                "acl_table_acl_add (acl)",
            );
        } else {
            u.acl = Handle::NULL;
        }
        if let Err(e) = st {
            undo(&u);
            return Err(e);
        }
        let gid = self.next_ugroup;
        self.next_ugroup += 1;
        log::notice(
            ssys::SAI_API_ACL,
            format_args!(
                "ACL table {gid}: IFCS {} table 0x{:x}, {} match types, width {width}, size {}, priority {tprio}{}{}{}",
                if x.egress { "egress" } else { "ingress" },
                u.table.0,
                mt.len(),
                u.size,
                if u.meters || x.egress {
                    ""
                } else {
                    ", no meters (SET_POLICER refused)"
                },
                if x.src_range || x.dst_range { ", ranges" } else { "" },
                if x.redirect || x.set_tc || x.set_dscp || x.mirror {
                    ", redirect/tc/dscp/mirror actions"
                } else {
                    ""
                }
            ),
        );
        self.ugroups.insert(gid, u);
        Ok(gid)
    }

    /// `group_destroy`: refused while it has entries; ports unbound, its meters, ACL, table and
    /// profiles deleted.
    pub fn group_destroy(&mut self, core: &Core, gid: FpGroupId) -> Result<()> {
        let Some(u) = self.ugroups.get(&gid.0) else {
            return Err(Error::ItemNotFound);
        };
        if self.uentries.values().any(|e| e.gid == gid.0) {
            return Err(Error::ObjectInUse);
        }
        let mut sp: Vec<Handle> = u.bound.iter().filter_map(|hw| core.sysport.get(hw).copied()).collect();
        if !sp.is_empty() {
            let _ = core.call(core.lib.ifcs_acl_port_remove(NODE, u.acl, &mut sp), "acl_port_remove");
        }
        let ms: Vec<(u32, u32)> = self.meters.user.keys().filter(|k| k.1 == gid.0).copied().collect();
        for k in ms {
            if let Some(h) = self.meters.user.remove(&k) {
                let _ = core.call(core.lib.ifcs_acl_meter_delete(NODE, h), "acl_meter_delete");
            }
        }
        let _ = core.call(
            core.lib.ifcs_acl_table_acl_remove(NODE, u.table, &mut [u.acl]),
            "acl_table_acl_remove",
        );
        let _ = core.call(core.lib.ifcs_acl_delete(NODE, u.acl), "acl_delete");
        let _ = core.call(core.lib.ifcs_acl_table_delete(NODE, u.table), "acl_table_delete");
        let _ = core.call(
            core.lib.ifcs_acl_match_profile_delete(NODE, u.mprof),
            "acl_match_profile_delete",
        );
        let _ = core.call(
            core.lib.ifcs_acl_action_profile_delete(NODE, u.aprof),
            "acl_action_profile_delete",
        );
        self.ugroups.remove(&gid.0);
        Ok(())
    }

    /// The checks of `entry_make` / `entry_update`: an action the table was not made with.
    fn action_refused(x: FpGroupExtra, r: &FpRule) -> bool {
        ((!r.mirror_ingress.is_empty() || !r.mirror_egress.is_empty()) && !x.mirror)
            || (r.redirect.is_some() && !x.redirect)
            || (r.set_tc.is_some() && !x.set_tc)
            || (r.set_dscp.is_some() && !x.set_dscp)
    }

    /// Re-installs the entries of `gid` other than `skip` that covered all of the table's ports (the
    /// table now spans more or fewer ports); a failure is logged by `log`.
    fn reinstall_others(&mut self, core: &Core, gid: u32, skip: u32, peers: &mut Peers<'_>, log_status: bool) {
        let Some(u) = self.ugroups.get(&gid) else { return };
        for (oid, o) in &mut self.uentries {
            if *oid == skip || o.gid != gid || o.per_port {
                continue;
            }
            uentry_uninstall(core, u, o, true, &mut *peers.mirror);
            if let Err(s2) = uentry_install(core, u, o, &mut self.meters, &mut self.acl_trap, self.storm_gid, peers) {
                if log_status {
                    log::error(
                        ssys::SAI_API_ACL,
                        format_args!("ACL entry {oid}: re-install for the new ports failed ({})", s2.status()),
                    );
                } else {
                    log::error(
                        ssys::SAI_API_ACL,
                        format_args!("ACL entry {oid}: re-install for the new ports failed"),
                    );
                }
            }
        }
    }

    /// `entry_create`.
    pub fn entry_create(
        &mut self,
        core: &Core,
        peers: &mut Peers<'_>,
        gid: FpGroupId,
        r: &FpRule,
    ) -> Result<FpEntryId> {
        self.entry_make(core, peers, gid.0, r, None).map(FpEntryId)
    }

    /// `entry_make`: checks, the ports bound, the ACEs installed; on a failure what was made goes again.
    /// The other entries are re-installed when the table now spans more ports.
    fn entry_make(
        &mut self,
        core: &Core,
        peers: &mut Peers<'_>,
        gid: u32,
        r: &FpRule,
        dmac_hit: Option<u8>,
    ) -> Result<u32> {
        let Some(u) = self.ugroups.get(&gid) else {
            return Err(Error::ItemNotFound);
        };
        if Self::action_refused(u.x, r) {
            log::error(
                ssys::SAI_API_ACL,
                format_args!("ACL entry: an action its table was not created with (ACTION_TYPE_LIST)"),
            );
            return Err(Error::NotSupported);
        }
        if r.cpu_copy_only || r.m.dst_cpu {
            return Err(Error::NotSupported);
        }
        let ports = entry_ports(r);
        if ports.is_empty() {
            return Err(Error::InvalidParameter);
        }
        if ports.iter().any(|hw| core.front(*hw).is_none()) {
            return Err(Error::InvalidPortNumber);
        }
        if let Some(pid) = r.policer {
            if !self.meters.policers.contains_key(&pid.0) {
                return Err(Error::InvalidParameter);
            }
        }
        let id = self.next_uentry;
        self.next_uentry += 1;
        self.uentries.insert(id, UEntry::new(gid, r.clone(), dmac_hit));
        let Some(u) = self.ugroups.get_mut(&gid) else {
            return Err(Error::ItemNotFound);
        };
        let before = u.bound.clone();
        let mut st = ugroup_bind(core, gid, u, &self.uentries);
        if st.is_ok() {
            if let Some(e) = self.uentries.get_mut(&id) {
                st = uentry_install(core, u, e, &mut self.meters, &mut self.acl_trap, self.storm_gid, peers);
            }
        }
        if let Err(err) = st {
            if let Some(mut e) = self.uentries.remove(&id) {
                uentry_uninstall(core, u, &mut e, false, &mut *peers.mirror);
            }
            let _ = ugroup_bind(core, gid, u, &self.uentries);
            return Err(err);
        }
        // The table now spans more ports: entries that covered all of them before must not match
        // on the new ones (re-installed with per-port ACEs).
        if u.bound != before {
            self.reinstall_others(core, gid, id, peers, true);
        }
        Ok(id)
    }

    /// `entry_destroy`: its ACEs and counter go, the table's ports follow the remaining entries.
    pub fn entry_destroy(&mut self, core: &Core, mirror: &mut dyn MirrorSets, eid: FpEntryId) -> Result<()> {
        let Some(mut e) = self.uentries.remove(&eid.0) else {
            return Err(Error::ItemNotFound);
        };
        let Some(u) = self.ugroups.get_mut(&e.gid) else {
            return Ok(());
        };
        uentry_uninstall(core, u, &mut e, false, mirror);
        // fewer ports: the others' ACEs stay correct
        let _ = ugroup_bind(core, e.gid, u, &self.uentries);
        Ok(())
    }

    /// `entry_counter`: packets and bytes (0 without a counter).
    pub fn entry_counter(&self, core: &Core, eid: FpEntryId) -> Result<FpCounter> {
        let Some(e) = self.uentries.get(&eid.0) else {
            return Err(Error::ItemNotFound);
        };
        if e.counter.is_null() {
            return Ok(FpCounter::default());
        }
        let mut ids = [
            sys::IFCS_ACL_COUNTER_STATS_ID_PACKETS,
            sys::IFCS_ACL_COUNTER_STATS_ID_BYTES,
        ]
        .map(|i| i32::try_from(i).unwrap_or(i32::MAX));
        let mut v = [0u64; 2];
        let (rc, _) = core.lib.ifcs_acl_counter_stats_get(NODE, e.counter, &mut ids, &mut v);
        core.call(rc, "acl_counter_stats_get (acl)")?;
        Ok(FpCounter {
            packets: v[0],
            bytes: v[1],
        })
    }

    /// `entry_update`: in place: the entry's ACEs are rebuilt, its IFCS counter is kept (no counts
    /// lost to the ~1 s counter read lag of a re-created entry); on a failure the old rule goes back.
    pub fn entry_update(&mut self, core: &Core, peers: &mut Peers<'_>, eid: FpEntryId, r: &FpRule) -> Result<()> {
        let eid = eid.0;
        let Some(gid) = self.uentries.get(&eid).map(|e| e.gid) else {
            return Err(Error::ItemNotFound);
        };
        let Some(u) = self.ugroups.get(&gid) else {
            return Err(Error::ItemNotFound);
        };
        if Self::action_refused(u.x, r) {
            return Err(Error::NotSupported);
        }
        let ports = entry_ports(r);
        if r.cpu_copy_only || r.m.dst_cpu || ports.is_empty() {
            return Err(Error::InvalidParameter);
        }
        if ports.iter().any(|hw| core.front(*hw).is_none()) {
            return Err(Error::InvalidPortNumber);
        }
        let (Some(u), Some(e)) = (self.ugroups.get_mut(&gid), self.uentries.get_mut(&eid)) else {
            return Err(Error::ItemNotFound);
        };
        let old = e.rule.clone();
        let before = u.bound.clone();
        uentry_uninstall(core, u, e, true, &mut *peers.mirror); // the counter stays (and keeps its counts)
        if r.counter && !old.counter {
            e.counter = Handle::NULL;
        }
        e.rule = r.clone();
        let mut st = ugroup_bind(core, gid, u, &self.uentries);
        if st.is_ok() {
            if let Some(e) = self.uentries.get_mut(&eid) {
                st = uentry_install(core, u, e, &mut self.meters, &mut self.acl_trap, self.storm_gid, peers);
            }
        }
        if let Err(err) = st {
            // put the old rule back
            if let Some(e) = self.uentries.get_mut(&eid) {
                uentry_uninstall(core, u, e, true, &mut *peers.mirror);
                e.rule = old;
            }
            let _ = ugroup_bind(core, gid, u, &self.uentries);
            if let Some(e) = self.uentries.get_mut(&eid) {
                if uentry_install(core, u, e, &mut self.meters, &mut self.acl_trap, self.storm_gid, peers).is_err() {
                    log::error(
                        ssys::SAI_API_ACL,
                        format_args!("ACL entry {eid}: restoring the old rule failed"),
                    );
                }
            }
            return Err(err);
        }
        if let Some(e) = self.uentries.get_mut(&eid) {
            if !r.counter && !e.counter.is_null() {
                let _ = core.call(core.lib.ifcs_acl_counter_delete(NODE, e.counter), "acl_counter_delete");
                e.counter = Handle::NULL;
            }
        }
        if u.bound != before {
            self.reinstall_others(core, gid, eid, peers, false);
        }
        Ok(())
    }

    /// `entry_enable`: ACE ENABLE (isai: `ace_attr_set`), admin state without re-creation.
    pub fn entry_enable(&mut self, core: &Core, eid: FpEntryId, on: bool) -> Result<()> {
        let Some(e) = self.uentries.get_mut(&eid.0) else {
            return Err(Error::ItemNotFound);
        };
        for h in &e.aces {
            let mut a = Attrs::new().data(sys::IFCS_ACE_ATTR_ENABLE, on);
            core.call(core.lib.ifcs_ace_attr_set(NODE, h.0, &mut a), "ace_attr_set ENABLE")?;
        }
        e.enabled = on;
        Ok(())
    }

    /// `group_free`: the table's size minus the ACEs of its entries.
    pub fn group_free(&self, gid: FpGroupId) -> Result<u32> {
        let Some(g) = self.ugroups.get(&gid.0) else {
            return Err(Error::ItemNotFound);
        };
        let used: usize = self
            .uentries
            .values()
            .filter(|e| e.gid == gid.0)
            .map(|e| e.aces.len())
            .sum();
        let used = u32::try_from(used).unwrap_or(u32::MAX);
        Ok(g.size.saturating_sub(used))
    }

    /// `storm_set` (`Backend::port_storm_set`): storm control as ACL entries (0.12.41 has no sysport storm
    /// attribute): a table {in-port, DMAC, DMAC hit} of its own, one entry per (port, kind), the
    /// policer's meter with yellow and red dropped (green passes). Unknown unicast: a unicast DMAC
    /// that missed the L2 table (`DESTINATION_MAC_HIT` 0, I).
    pub fn storm_set(
        &mut self,
        core: &Core,
        peers: &mut Peers<'_>,
        hw: u32,
        kind: StormKind,
        policer: Option<PolicerId>,
    ) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        let key = (hw, kind as u8);
        let existing = self.storm.get(&key).copied();
        let Some(pid) = policer else {
            let Some(eid) = existing else { return Ok(()) };
            let st = self.entry_destroy(core, &mut *peers.mirror, FpEntryId(eid));
            self.storm.remove(&key);
            return st;
        };
        if !self.meters.policers.contains_key(&pid.0) {
            return Err(Error::InvalidParameter);
        }
        let gid = if let Some(g) = self.storm_gid {
            g
        } else {
            let g = self.group_make(
                core,
                &[FpQual::InPorts, FpQual::DstMac],
                1,
                FpGroupExtra::default(),
                true,
            )?;
            self.storm_gid = Some(g);
            g
        };
        let (dmac, mask, hit) = match kind {
            StormKind::Broadcast => ([0xff; 6], [0xff; 6], None),
            // includes broadcast (IEEE group bit)
            StormKind::Multicast => ([0x01, 0, 0, 0, 0, 0], [0x01, 0, 0, 0, 0, 0], None),
            StormKind::UnknownUnicast => ([0; 6], [0x01, 0, 0, 0, 0, 0], Some(0)),
        };
        let mut r = FpRule {
            // broadcast is multicast too: its own entry wins
            priority: if kind == StormKind::Broadcast { 200 } else { 100 },
            policer: Some(pid),
            yellow_drop: true,
            red_drop: true,
            counter: true,
            ..FpRule::default()
        };
        r.m.in_ports = Some(vec![HwPort(u16::try_from(hw).unwrap_or(u16::MAX))]);
        r.m.dst_mac = Some(Masked {
            value: MacAddr(dmac),
            mask: MacAddr(mask),
        });
        if let Some(eid) = existing {
            // (C++: `uentries_[id]`; a missing entry would compare as a default one, hit -1)
            if self.uentries.get(&eid).map_or(hit.is_none(), |e| e.dmac_hit == hit) {
                return self.entry_update(core, peers, FpEntryId(eid), &r);
            }
        }
        let eid = self.entry_make(core, peers, gid, &r, hit)?;
        if let Some(old) = existing {
            let _ = self.entry_destroy(core, &mut *peers.mirror, FpEntryId(old));
        }
        self.storm.insert(key, eid);
        log::notice(
            ssys::SAI_API_PORT,
            format_args!(
                "storm control: devport {hw} kind {} policer {} (ACL entry {eid})",
                kind as u8, pid.0
            ),
        );
        Ok(())
    }

    /// `report` (the diag shell's `acl`): SAI ACL tables, entries, counters; the storm table's colour
    /// counters.
    pub fn report(&self, core: &Core) -> String {
        let mut out = String::new();
        let mut put = |s: String| out.push_str(line256(&s));
        put(format!(
            "ACL: {} tables, {} entries (CoPP: {} installs)\n",
            self.ugroups.len(),
            self.uentries.len(),
            self.copp.installs.len()
        ));
        for (gid, g) in &self.ugroups {
            let ports = g.bound.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
            put(format!(
                "table {gid}: ifcs 0x{:x} acl 0x{:x} size {}{} ports [{ports}]\n",
                g.table.0,
                g.acl.0,
                g.size,
                if g.meters { " meters" } else { "" }
            ));
            for (eid, e) in self.uentries.iter().filter(|(_, e)| e.gid == *gid) {
                let c = self.entry_counter(core, FpEntryId(*eid)).unwrap_or_default();
                let r = &e.rule;
                put(format!(
                    "  entry {eid}: priority {}, {} ACEs{}, {}{}{}{} packets {} bytes {}\n",
                    r.priority,
                    e.aces.len(),
                    if e.per_port { " (per port)" } else { "" },
                    if r.drop { "drop " } else { "" },
                    if r.drop_cancel { "forward " } else { "" },
                    if r.copy_cpu { "copy " } else { "" },
                    if r.policer.is_some() { "policer " } else { "" },
                    c.packets,
                    c.bytes
                ));
                if Some(*gid) == self.storm_gid && !e.counter.is_null() {
                    let mut ids = [
                        sys::IFCS_ACL_COUNTER_STATS_ID_GREEN_PACKETS,
                        sys::IFCS_ACL_COUNTER_STATS_ID_YELLOW_PACKETS,
                        sys::IFCS_ACL_COUNTER_STATS_ID_RED_PACKETS,
                    ]
                    .map(|i| i32::try_from(i).unwrap_or(i32::MAX));
                    let mut v = [0u64; 3];
                    let _ = core.lib.ifcs_acl_counter_stats_get(NODE, e.counter, &mut ids, &mut v);
                    put(format!("    storm: green {} yellow {} red {}\n", v[0], v[1], v[2]));
                }
            }
        }
        out
    }

    /// `AclArea::forget`: the shutdown drops the state (the IFCS objects go with `ifcs_deinit`).
    pub fn forget(&mut self) {
        self.copp.installs.clear();
        self.meters.copp.clear();
        self.copp.acls.clear();
        self.copp.up = false;
        self.copp.mprof = Handle::NULL;
        self.copp.aprof = Handle::NULL;
        self.copp.table = Handle::NULL;
        self.ugroups.clear();
        self.uentries.clear();
        self.meters.user.clear();
        self.acl_trap = Handle::NULL;
        self.storm_gid = None;
        self.storm.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn group(qset: &[FpQual], x: FpGroupExtra) -> UGroup {
        UGroup {
            qset: qset.to_vec(),
            x,
            v6: qset.iter().any(|q| matches!(q, FpQual::SrcIp6 | FpQual::DstIp6)),
            ..UGroup::default()
        }
    }

    #[test]
    fn meters_at_84_bytes_per_packet_as_isai() {
        let p = PolicerCfg {
            cir: 600,
            cbs: 600,
            ..PolicerCfg::default()
        };
        // t_copp: 600 pps -> cir=403200 cbs=50400; srTCM without PBS: EBS 1
        assert_eq!(
            meter_attrs(&p),
            vec![
                (sys::IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE, 403_200),
                (sys::IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE, 50_400),
                (sys::IFCS_ACL_METER_ATTR_EXCESS_BURST_SIZE, 1),
            ]
        );
        let bytes = PolicerCfg {
            tr_tcm: true,
            packets: false,
            cir: 1000,
            pir: 2000,
            pbs: 300,
            ..PolicerCfg::default()
        };
        assert_eq!(
            meter_attrs(&bytes),
            vec![
                (sys::IFCS_ACL_METER_ATTR_COMMITTED_INFORMATION_RATE, 8000),
                (sys::IFCS_ACL_METER_ATTR_COMMITTED_BURST_SIZE, 1), // CBS 0: 1
                (sys::IFCS_ACL_METER_ATTR_EXCESS_INFORMATION_RATE, 16_000),
                (sys::IFCS_ACL_METER_ATTR_EXCESS_BURST_SIZE, 300),
            ]
        );
    }

    #[test]
    fn the_l3_table_of_aclorch_has_isais_match_types() {
        // t_acl: list=4,18,27,25,56,28,29,59,60,68 (in-port, ether, L3 type, ARP op, proto, IPs, ports, flags)
        let q = [
            FpQual::InPorts,
            FpQual::EtherType,
            FpQual::IpType,
            FpQual::IpProtocol,
            FpQual::SrcIp,
            FpQual::DstIp,
            FpQual::L4SrcPort,
            FpQual::L4DstPort,
            FpQual::TcpFlags,
            FpQual::IcmpTypeCode,
            FpQual::Tos,
            FpQual::OuterVlan,
        ];
        let mt = match_types(&q, false, false).unwrap();
        assert_eq!(&mt[..10], &[4, 18, 27, 25, 56, 28, 29, 59, 60, 68]);
        // egress: no in-port match; DstCpu has none at all; the storm table's DMAC hit
        assert!(!match_types(&[FpQual::InPorts], true, false).unwrap().contains(&4));
        assert_eq!(match_types(&[FpQual::DstCpu], false, false), Err(FpQual::DstCpu));
        assert_eq!(
            match_types(&[FpQual::InPorts, FpQual::DstMac], false, true).unwrap(),
            vec![
                sys::IFCS_ACL_MATCH_TYPE_INGRESS_L2_INTERFACE,
                sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS,
                sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_HIT
            ]
        );
        // v6 tables: ICMP type / code in L4 bytes 1 and 2
        let v6 = match_types(&[FpQual::DstIp6, FpQual::IcmpTypeCode], false, false).unwrap();
        assert!(v6.contains(&sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1) && v6.contains(&sys::IFCS_ACL_MATCH_TYPE_L4_BYTE2));
    }

    #[test]
    fn widths_priorities_and_ace_priorities() {
        let mt = match_types(&[FpQual::InPorts, FpQual::DstMac], false, true).unwrap();
        assert_eq!(key_bits(&mt, &FpGroupExtra::default()), 16 + 48 + 1);
        assert_eq!(widths(65), vec![80, 160, 320, 480]);
        assert_eq!(widths(200), vec![320, 480]);
        assert_eq!(widths(1000), vec![480]);
        let x = FpGroupExtra {
            src_range: true,
            dst_range: true,
            ..FpGroupExtra::default()
        };
        assert_eq!(key_bits(&[], &x), 16);
        assert_eq!(table_priority(1, false), 15);
        assert_eq!(table_priority(2, false), 14);
        assert_eq!(table_priority(100, false), 12);
        assert_eq!(table_priority(-5, false), 15);
        assert_eq!(table_priority(1, true), 6);
        assert_eq!(ace_priority(4000), 0xffff_ffff - 8000);
        assert_eq!(ace_priority(u32::MAX), 3); // capped at 0x7ffffffe
        let l = ChipLimits {
            acl_prio_min: 10,
            acl_prio_max: 100,
            ..ChipLimits::default()
        };
        assert_eq!(AclArea::copp_prio(&l, 50), 60);
        assert_eq!(AclArea::copp_prio(&l, 5000), 100);
        assert_eq!(AclArea::copp_prio(&ChipLimits::default(), 3009), 3009); // limits unread: no cap
    }

    #[test]
    fn entry_matches_in_network_order_with_masks() {
        let g = group(
            &[
                FpQual::DstIp,
                FpQual::IpProtocol,
                FpQual::L4DstPort,
                FpQual::IpType,
                FpQual::Tos,
                FpQual::OuterVlan,
            ],
            FpGroupExtra::default(),
        );
        let mut r = FpRule::default();
        r.m.dst_ip = Some(Masked {
            value: Ipv4Addr::new(10, 99, 0, 1),
            mask: Ipv4Addr::BROADCAST,
        });
        r.m.ip_protocol = Some(Masked { value: 6, mask: 0xff });
        r.m.l4_dst_port = Some(Masked {
            value: 179,
            mask: 0xffff,
        });
        r.m.ip_type = Some(ssys::SAI_ACL_IP_TYPE_IPV4ANY.cast_signed());
        r.m.tos = Some(Masked {
            value: 46 << 2 | 1,
            mask: 0xff,
        });
        r.m.outer_vlan = Some(Masked {
            value: 0x1064,
            mask: 0xffff,
        });
        let m = uentry_matches(&g, &r).unwrap();
        let v: Vec<(u32, Vec<u8>, Vec<u8>)> = m.into_iter().map(|c| (c.kind, c.value, c.mask)).collect();
        assert_eq!(
            v,
            vec![
                (sys::IFCS_ACL_MATCH_TYPE_L3_TYPE, vec![4], vec![0x06]),
                (sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, vec![6], vec![0xff]),
                (
                    sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV4_ADDRESS,
                    vec![10, 99, 0, 1],
                    vec![0xff; 4]
                ),
                (
                    sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT,
                    vec![0, 179],
                    vec![0xff, 0xff]
                ),
                (sys::IFCS_ACL_MATCH_TYPE_DSCP, vec![46], vec![0x3f]),
                (sys::IFCS_ACL_MATCH_TYPE_ECN, vec![1], vec![3]),
                (sys::IFCS_ACL_MATCH_TYPE_VLAN_ID, vec![0, 0x64], vec![0x0f, 0xff]),
            ]
        );
        // a field the table does not have; NON_IPV4 has no IFCS form
        let mut bad = FpRule::default();
        bad.m.src_mac = Some(Masked::default());
        assert_eq!(uentry_matches(&g, &bad), Err(Error::InvalidParameter));
        let mut non = FpRule::default();
        non.m.ip_type = Some(ssys::SAI_ACL_IP_TYPE_NON_IPV4.cast_signed());
        assert_eq!(uentry_matches(&g, &non), Err(Error::NotSupported));
    }

    #[test]
    fn arp_and_icmp_encodings() {
        let g = group(
            &[FpQual::IpType, FpQual::IcmpTypeCode, FpQual::L4SrcPort],
            FpGroupExtra::default(),
        );
        let mut r = FpRule::default();
        r.m.ip_type = Some(ssys::SAI_ACL_IP_TYPE_ARP_REQUEST.cast_signed());
        let m = uentry_matches(&g, &r).unwrap();
        assert_eq!(m[0].kind, sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE);
        assert_eq!(m[0].value, vec![0x08, 0x06]);
        assert_eq!(
            (m[1].kind, m[1].value.clone()),
            (sys::IFCS_ACL_MATCH_TYPE_ARP_OPERATION, vec![0, 1])
        );
        // v4 ICMP: protocol 1 + type/code in the L4 source port field; not with an L4 source port
        let mut i = FpRule::default();
        i.m.icmp_type_code = Some(Masked {
            value: 0x0800,
            mask: 0xffff,
        });
        let m = uentry_matches(&g, &i).unwrap();
        assert_eq!(
            (m[0].kind, m[0].value.clone()),
            (sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, vec![1])
        );
        assert_eq!(
            (m[1].kind, m[1].value.clone()),
            (sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT, vec![8, 0])
        );
        i.m.l4_src_port = Some(Masked::default());
        assert_eq!(uentry_matches(&g, &i), Err(Error::NotSupported));
        // v6: next header 58, L4 byte 1 only when the type is masked
        let g6 = group(&[FpQual::DstIp6, FpQual::IcmpTypeCode], FpGroupExtra::default());
        let mut i6 = FpRule::default();
        i6.m.icmp_type_code = Some(Masked {
            value: 128 << 8,
            mask: 0xff00,
        });
        i6.m.dst_ip6 = Some(Masked {
            value: Ipv6Addr::LOCALHOST,
            mask: Ipv6Addr::UNSPECIFIED,
        });
        let m = uentry_matches(&g6, &i6).unwrap();
        let kinds: Vec<u32> = m.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                sys::IFCS_ACL_MATCH_TYPE_DESTINATION_IPV6_ADDRESS,
                sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL,
                sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1
            ]
        );
        assert_eq!(m[1].value, vec![58]);
        assert_eq!(m[2].value, vec![128]);
    }

    #[test]
    fn action_values_are_the_structs() {
        type C = sys::ifcs_acl_ctc_action_t;
        type T = sys::ifcs_acl_set_tc_action_t;
        type S = sys::ifcs_acl_set_dscp_action_t;
        let v = ctc_value(Handle(0x55), 1, 1, 0, 0x0f);
        assert_eq!(v.len(), size_of::<C>());
        assert_eq!(v[offset_of!(C, hostif)..][..4], 0x55u32.to_ne_bytes());
        assert_eq!(v[offset_of!(C, red)..][..4], [0; 4]);
        assert_eq!(v[offset_of!(C, dirty)], 0x0f);
        let d = drop_value(1, 1, 1, 7);
        assert_eq!(d.len(), size_of::<sys::ifcs_acl_drop_action_t>());
        let t = set_tc_value(5);
        assert_eq!(t[offset_of!(T, red_value)..][..4], 5u32.to_ne_bytes());
        assert_eq!(t[offset_of!(T, dirty)], 0x3f);
        let s = set_dscp_value(true, 46);
        assert_eq!(
            s[offset_of!(S, addr_family)..][..4],
            sys::IFCS_IP_ADDR_FAMILY_IPV6.to_ne_bytes()
        );
        assert_eq!(
            ctc_override_value(0, 0, 1, 7).len(),
            size_of::<sys::ifcs_acl_ctc_override_action_t>()
        );
    }

    #[test]
    fn entries_cover_their_ports_and_out_ports() {
        let mut entries = BTreeMap::new();
        let mut r = FpRule::default();
        r.m.in_ports = Some(vec![HwPort(1), HwPort(2)]);
        entries.insert(1, UEntry::new(7, r.clone(), None));
        r.m.in_ports = Some(vec![HwPort(3)]);
        entries.insert(2, UEntry::new(8, r.clone(), None));
        assert_eq!(ugroup_ports(7, false, &entries), [1, 2].into_iter().collect());
        // egress: OUT_PORTS among the bound ports
        r.m.in_ports = Some(vec![HwPort(1), HwPort(2)]);
        r.m.out_ports = vec![HwPort(2), HwPort(9)];
        entries.insert(3, UEntry::new(9, r, None));
        assert_eq!(ugroup_ports(9, true, &entries), [2].into_iter().collect());
        assert_eq!(ugroup_ports(9, false, &entries), [1, 2].into_iter().collect());
        let refused = FpRule {
            set_dscp: Some(1),
            ..FpRule::default()
        };
        assert!(AclArea::action_refused(FpGroupExtra::default(), &refused));
        assert!(!AclArea::action_refused(
            FpGroupExtra {
                set_dscp: true,
                ..FpGroupExtra::default()
            },
            &refused
        ));
    }

    #[test]
    fn report_lines_are_cut_as_snprintf_256() {
        let long = "x".repeat(300) + "\n";
        assert_eq!(line256(&long).len(), 255);
        assert_eq!(line256("ab\n"), "ab\n");
        let multibyte = "é".repeat(200);
        assert!(line256(&multibyte).len() <= 255);
    }
}

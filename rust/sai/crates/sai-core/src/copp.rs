//! Control-plane policing: a trap group's `POLICER` meters the CPU-bound traffic of the traps
//! in that group with field entries in one `CoPP` field group (the trap's protocol as match,
//! the group's meter attached, red / yellow / green packets per the policer's actions).
//! Traps the CPU gets as a copy (switch controls: ARP, DHCP, ND, IGMP, MLD) lose only the copy;
//! traps whose packets go only to the CPU (LACP, LLDP, STP, EAPOL, and traffic routed to the
//! CPU: IP2ME, BGP, SSH, ...) are dropped. Ports the `CoPP` part of `sai_acl.cpp`
//! (lines 254..=453): rule priorities 100 (IP2ME) / 150 (a protocol to the CPU) / 200 (by
//! protocol), the group at priority 1000 with the `DstCpu` qualifier, else without it (then
//! the routed-to-CPU traps are not policed).
//!
//! Backend: [`crate::backend::Field`]; [`crate::backend::Hostif::trap_program`] for the traps
//! the backend owns ([`crate::hostif::Programming::Full`]).
//!
//! Registration: none of its own: it owns no object type. [`copp_refresh`] runs after every
//! change of a trap, a trap group ([`crate::hostif`]) or a policer's colour actions
//! ([`crate::policer`]). Its state ([`CoppState`]) is part of the switch's
//! [`crate::acl::AclState`].
use crate::acl::{apply_colors, switch_of};
use crate::backend::Backend;
use crate::backend::types::{FpEntryId, FpGroupId, FpQual, FpRule, MacAddr, Masked, Redirect};
use crate::hostif::{Programming, TrapGroup};
use crate::log;
use crate::model::Switch;
use crate::oid::RawOid;
use crate::store::Store;
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

const fn e(enumerator: u32) -> i32 {
    enumerator.cast_signed()
}

/// One installed `CoPP` rule (C++ `CoppEntry`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoppEntry {
    /// The backend's field entry.
    pub eid: FpEntryId,
    /// The SAI policer it meters with.
    pub policer: RawOid,
}

/// The `CoPP` field group and its rules (C++ `copp_gid`, `copp_dst_cpu`, `copp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoppState {
    /// The field group, while it has rules.
    pub gid: Option<FpGroupId>,
    /// The group has the `DstCpu` qualifier.
    pub dst_cpu: bool,
    /// The installed rules by their canonical key ([`rule_key`], the C++ `rule_key`): the
    /// order rules are created and destroyed in.
    pub entries: BTreeMap<Vec<u8>, CoppEntry>,
    /// The rule count last logged (C++: a function-static `last`).
    logged: Option<usize>,
}

// ------------------------------------------------------------------ keys

/// A canonical byte string of a rule: the C++ `rule_key` byte for byte (the fields' memory
/// images in declaration order, native byte order), so that the `CoPP` rules are created and
/// destroyed in the C++ order.
pub(crate) fn rule_key(r: &FpRule) -> Vec<u8> {
    let m = &r.m;
    let mut s = Vec::with_capacity(256);
    let masked16 = |s: &mut Vec<u8>, v: Option<Masked<u16>>| {
        let v = v.unwrap_or_default();
        s.extend_from_slice(&v.value.to_ne_bytes());
        s.extend_from_slice(&v.mask.to_ne_bytes());
    };
    let masked8 = |s: &mut Vec<u8>, v: Option<Masked<u8>>| {
        let v = v.unwrap_or_default();
        s.push(v.value);
        s.push(v.mask);
    };
    // C++ IpAddr { bool v6; uint8_t addr[16]; }: the v4 and v6 qualifiers share one field.
    let ip = |s: &mut Vec<u8>, v4: Option<Ipv4Addr>, v6: Option<Ipv6Addr>| {
        let mut addr = [0u8; 16];
        let is_v6 = match (v4, v6) {
            (_, Some(a)) => {
                addr = a.octets();
                true
            }
            (Some(a), None) => {
                for (dst, src) in addr.iter_mut().zip(a.octets()) {
                    *dst = src;
                }
                false
            }
            (None, None) => false,
        };
        s.push(u8::from(is_v6));
        s.extend_from_slice(&addr);
    };
    let mac = |s: &mut Vec<u8>, v: Option<Masked<MacAddr>>| {
        let v = v.unwrap_or_default();
        s.extend_from_slice(&v.value.0);
        s.extend_from_slice(&v.mask.0);
    };
    let int = |s: &mut Vec<u8>, v: i64| {
        // C++ `int`: 32 bits; every id and port number of the backend fits.
        s.extend_from_slice(&i32::try_from(v).unwrap_or(-1).to_ne_bytes());
    };
    s.extend_from_slice(&r.priority.to_ne_bytes());
    s.extend_from_slice(&m.present().to_ne_bytes());
    masked16(&mut s, m.ether_type);
    s.extend_from_slice(&m.ip_type.unwrap_or(0).to_ne_bytes());
    masked8(&mut s, m.ip_protocol);
    ip(&mut s, m.src_ip.map(|v| v.value), m.src_ip6.map(|v| v.value));
    ip(&mut s, m.src_ip.map(|v| v.mask), m.src_ip6.map(|v| v.mask));
    ip(&mut s, m.dst_ip.map(|v| v.value), m.dst_ip6.map(|v| v.value));
    ip(&mut s, m.dst_ip.map(|v| v.mask), m.dst_ip6.map(|v| v.mask));
    masked16(&mut s, m.l4_src_port);
    masked16(&mut s, m.l4_dst_port);
    masked8(&mut s, m.tcp_flags);
    masked16(&mut s, m.icmp_type_code);
    masked8(&mut s, m.tos);
    // dmac, dmac_mask, smac, smac_mask
    mac(&mut s, m.dst_mac);
    mac(&mut s, m.src_mac);
    masked16(&mut s, m.outer_vlan);
    masked8(&mut s, m.ttl);
    for p in m.in_ports.iter().flatten() {
        int(&mut s, i64::from(p.0));
    }
    for flag in [r.drop, r.drop_cancel, r.copy_cpu, r.counter] {
        s.push(u8::from(flag));
    }
    int(&mut s, r.policer.map_or(-1, |p| i64::from(p.0)));
    for flag in [r.green_drop, r.yellow_drop, r.red_drop, r.cpu_copy_only] {
        s.push(u8::from(flag));
    }
    for x in &r.mirror_ingress {
        int(&mut s, i64::from(x.0));
    }
    int(&mut s, -1);
    for x in &r.mirror_egress {
        int(&mut s, i64::from(x.0));
    }
    for range in [m.l4_src_range, m.l4_dst_range] {
        let (min, max) = range.unwrap_or_default();
        s.push(u8::from(range.is_some()));
        s.extend_from_slice(&min.to_ne_bytes());
        s.extend_from_slice(&max.to_ne_bytes());
    }
    int(&mut s, -2);
    for p in &m.out_ports {
        int(&mut s, i64::from(p.0));
    }
    let (redirect, redirect_egress) = match r.redirect {
        None => (-1, false),
        Some(Redirect::Dest(d)) => (i64::from(d.raw()), false),
        Some(Redirect::Egress(id)) => (i64::from(id.0), true),
    };
    int(&mut s, redirect);
    s.push(u8::from(redirect_egress));
    int(&mut s, r.set_tc.map_or(-1, i64::from));
    int(&mut s, r.set_dscp.map_or(-1, i64::from));
    s
}

// ------------------------------------------------------------------ rules

const fn exact16(value: u16) -> Masked<u16> {
    Masked { value, mask: 0xffff }
}

const fn exact8(value: u8) -> Masked<u8> {
    Masked { value, mask: 0xff }
}

/// C++ `l4`: `EtherType`, IP protocol and one L4 port (destination or source), exact.
fn l4(ether: u16, proto: u8, dst: bool, port: u16) -> FpRule {
    let mut r = FpRule::default();
    r.m.ether_type = Some(exact16(ether));
    r.m.ip_protocol = Some(exact8(proto));
    if dst {
        r.m.l4_dst_port = Some(exact16(port));
    } else {
        r.m.l4_src_port = Some(exact16(port));
    }
    r
}

fn ether_rule(ether: u16) -> FpRule {
    let mut r = FpRule::default();
    r.m.ether_type = Some(exact16(ether));
    r
}

fn icmp6_rule(icmp_type: u8) -> FpRule {
    let mut r = ether_rule(0x86dd);
    r.m.ip_protocol = Some(exact8(58));
    r.m.icmp_type_code = Some(Masked {
        value: u16::from(icmp_type) << 8,
        mask: 0xff00,
    });
    r
}

fn ip2me(mut r: FpRule) -> FpRule {
    r.m.dst_cpu = true;
    r
}

/// The matches of a trap type's CPU-bound packets (C++ `copp_rules`), and whether the CPU gets
/// them as a copy (then a colour drop cancels the copy only); priorities 100 / 150 / 200.
#[allow(clippy::too_many_lines, reason = "one arm per trap type, as the C++ switch")]
pub(crate) fn copp_rules(trap_type: i32) -> (Vec<FpRule>, bool) {
    let mut v = Vec::new();
    let mut copy = false;
    let both_ports = |v: &mut Vec<FpRule>, ether, proto, port| {
        v.push(ip2me(l4(ether, proto, true, port)));
        v.push(ip2me(l4(ether, proto, false, port)));
    };
    match u32::try_from(trap_type).unwrap_or(u32::MAX) {
        t @ (sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST | sys::SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE) => {
            let mut r = FpRule::default();
            r.m.ip_type = Some(if t == sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST {
                e(sys::SAI_ACL_IP_TYPE_ARP_REQUEST)
            } else {
                e(sys::SAI_ACL_IP_TYPE_ARP_REPLY)
            });
            v.push(r);
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY => {
            v.extend((133..=137).map(icmp6_rule));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION => {
            v.push(icmp6_rule(135));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_ADVERTISEMENT => {
            v.push(icmp6_rule(136));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2 => {
            v.extend([130, 131, 132, 143].map(icmp6_rule));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT => {
            v.push(icmp6_rule(131));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE => {
            v.push(icmp6_rule(132));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT => {
            v.push(icmp6_rule(143));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_DHCP | sys::SAI_HOSTIF_TRAP_TYPE_DHCP_L2 => {
            v.extend([67, 68].map(|p| l4(0x0800, 17, true, p)));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6 | sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2 => {
            v.extend([546, 547].map(|p| l4(0x86dd, 17, true, p)));
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT => {
            let mut r = ether_rule(0x0800);
            r.m.ip_protocol = Some(exact8(2));
            v.push(r);
            copy = true;
        }
        sys::SAI_HOSTIF_TRAP_TYPE_LACP => v.push(ether_rule(0x8809)), // slow protocols
        sys::SAI_HOSTIF_TRAP_TYPE_LLDP => v.push(ether_rule(0x88cc)),
        sys::SAI_HOSTIF_TRAP_TYPE_EAPOL => v.push(ether_rule(0x888e)),
        sys::SAI_HOSTIF_TRAP_TYPE_STP | sys::SAI_HOSTIF_TRAP_TYPE_PVRST => {
            let mut r = FpRule::default();
            r.m.dst_mac = Some(Masked {
                value: MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x00]),
                mask: MacAddr([0xff; 6]),
            });
            v.push(r);
        }
        sys::SAI_HOSTIF_TRAP_TYPE_BGP => both_ports(&mut v, 0x0800, 6, 179),
        sys::SAI_HOSTIF_TRAP_TYPE_BGPV6 => both_ports(&mut v, 0x86dd, 6, 179),
        sys::SAI_HOSTIF_TRAP_TYPE_SSH => {
            both_ports(&mut v, 0x0800, 6, 22);
            both_ports(&mut v, 0x86dd, 6, 22);
        }
        sys::SAI_HOSTIF_TRAP_TYPE_SNMP => {
            v.push(ip2me(l4(0x0800, 17, true, 161)));
            v.push(ip2me(l4(0x86dd, 17, true, 161)));
        }
        t @ (sys::SAI_HOSTIF_TRAP_TYPE_BFD | sys::SAI_HOSTIF_TRAP_TYPE_BFDV6) => {
            let ether = if t == sys::SAI_HOSTIF_TRAP_TYPE_BFD {
                0x0800
            } else {
                0x86dd
            };
            v.extend([3784, 4784].map(|p| ip2me(l4(ether, 17, true, p))));
        }
        sys::SAI_HOSTIF_TRAP_TYPE_IP2ME => v.push(ip2me(FpRule::default())),
        _ => {}
    }
    // Specific protocols before IP2ME (one CoPP group: the highest entry wins).
    for r in &mut v {
        r.priority = if r.m.present() == FpQual::DstCpu.bit() {
            100
        } else if r.m.dst_cpu {
            150
        } else {
            200
        };
    }
    (v, copy)
}

/// The `CoPP` group's qualifiers without `DstCpu` (C++ `copp_qset_base`).
const COPP_QSET_BASE: [FpQual; 7] = [
    FpQual::EtherType,
    FpQual::IpType,
    FpQual::IpProtocol,
    FpQual::L4SrcPort,
    FpQual::L4DstPort,
    FpQual::IcmpTypeCode,
    FpQual::DstMac,
];

/// A trap's packets reach the CPU: `TRAP`, `COPY` or `LOG` (C++ `to_cpu`).
const fn to_cpu(action: i32) -> bool {
    action == e(sys::SAI_PACKET_ACTION_TRAP)
        || action == e(sys::SAI_PACKET_ACTION_COPY)
        || action == e(sys::SAI_PACKET_ACTION_LOG)
}

/// C++ `copp_refresh`: hands the backend-owned traps their current state (`trap_program`),
/// then makes the `CoPP` rules what the traps, their groups and the groups' policers ask for:
/// the group is created on the first rule (with `DstCpu`, else without it and without the
/// routed-to-CPU rules), rules no longer wanted are destroyed, new ones created (a failure is
/// logged and that rule left out), the group destroyed with its last rule. Failures are logged,
/// never returned (the trap or policer change that called it stands, as in the C++).
#[allow(clippy::too_many_lines, reason = "the C++ function, step by step")]
pub(crate) fn copp_refresh<B: Backend>(backend: &mut B, store: &mut Store) {
    let Some((switch, mut state)) = switch_of(store).map(|(id, s)| (id, s.acl.copp.clone())) else {
        return;
    };
    let traps: Vec<crate::hostif::Trap> = crate::hostif::traps(store).map(|(_, t)| *t).collect();
    for t in traps.iter().filter(|t| t.programming == Programming::Full) {
        let spec = crate::hostif::trap_spec(store, t);
        if let Err(err) = backend.trap_program(&spec, false) {
            log::error(
                sys::SAI_API_HOSTIF,
                format_args!(
                    "trap type {}: backend reprogram failed ({})",
                    t.trap_type.0,
                    err.status()
                ),
            );
        }
    }
    let mut want: BTreeMap<Vec<u8>, (FpRule, RawOid)> = BTreeMap::new();
    let mut unpoliced = Vec::new();
    let mut need_cpu = false;
    for t in &traps {
        if t.programming == Programming::Full || !to_cpu(t.action.0) {
            continue;
        }
        let Some(policer_oid) = store.get::<TrapGroup>(t.group).ok().and_then(|g| g.policer) else {
            continue;
        };
        let Some(policer) = crate::policer::policer(store, policer_oid) else {
            continue;
        };
        let (rules, copy) = copp_rules(t.trap_type.0);
        if rules.is_empty() {
            unpoliced.push(t.trap_type.0);
        }
        for mut r in rules {
            apply_colors(&mut r, policer, copy);
            need_cpu |= r.m.dst_cpu;
            // the first trap wins a shared match
            want.entry(rule_key(&r)).or_insert((r, policer_oid));
        }
    }
    if !want.is_empty() && state.gid.is_none() {
        let mut q = COPP_QSET_BASE.to_vec();
        q.push(FpQual::DstCpu);
        let first = backend.fp_group_create(&q, 1000);
        state.dst_cpu = first.is_ok();
        let created = first.or_else(|_| {
            let base = backend.fp_group_create(&COPP_QSET_BASE, 1000);
            if base.is_ok() {
                log::warn(
                    sys::SAI_API_HOSTIF,
                    format_args!(
                        "CoPP: field group without the routed-to-CPU qualifier: IP2ME, BGP, SSH, SNMP, BFD are not policed"
                    ),
                );
            }
            base
        });
        match created {
            Ok(gid) => state.gid = Some(gid),
            Err(err) => {
                log::error(
                    sys::SAI_API_HOSTIF,
                    format_args!(
                        "CoPP: field group create failed ({}): traps run unpoliced",
                        err.status()
                    ),
                );
                save(store, switch, state);
                return;
            }
        }
    }
    if need_cpu && !state.dst_cpu {
        want.retain(|_, (r, _)| !r.m.dst_cpu);
    }
    let unwanted: Vec<Vec<u8>> = state
        .entries
        .keys()
        .filter(|k| !want.contains_key(*k))
        .cloned()
        .collect();
    for key in unwanted {
        if let Some(entry) = state.entries.remove(&key) {
            let _ = backend.fp_entry_destroy(entry.eid);
        }
    }
    if let Some(gid) = state.gid {
        for (key, (r, policer)) in &want {
            if state.entries.contains_key(key) {
                continue;
            }
            match backend.fp_entry_create(gid, r) {
                Ok(eid) => {
                    state.entries.insert(key.clone(), CoppEntry { eid, policer: *policer });
                }
                Err(err) => log::error(
                    sys::SAI_API_HOSTIF,
                    format_args!("CoPP: field entry create failed ({})", err.status()),
                ),
            }
        }
    }
    if state.entries.is_empty() {
        if let Some(gid) = state.gid.take() {
            let _ = backend.fp_group_destroy(gid);
        }
    }
    if state.logged != Some(state.entries.len()) {
        let policers: BTreeSet<RawOid> = state.entries.values().map(|e| e.policer).collect();
        log::notice(
            sys::SAI_API_HOSTIF,
            format_args!(
                "CoPP: {} field entries, {} policers (field group {})",
                state.entries.len(),
                policers.len(),
                state.gid.map_or(-1, |g| i64::from(g.0))
            ),
        );
        state.logged = Some(state.entries.len());
    }
    if !unpoliced.is_empty() {
        let list: Vec<String> = unpoliced.iter().map(ToString::to_string).collect();
        log::info(
            sys::SAI_API_HOSTIF,
            format_args!(
                "CoPP: trap types {} have a policer but no match here: not policed",
                list.join(",")
            ),
        );
    }
    save(store, switch, state);
}

fn save(store: &mut Store, switch: crate::oid::SwitchId, state: CoppState) {
    // The switch object exists (it was just read) and its references do not change.
    let _ = store.update::<Switch, _>(switch, |s| s.acl.copp = state);
}

/// The backend's `CoPP` field group (test port `copp_group`), `None` while it has no rules.
pub fn copp_group(store: &Store) -> Option<FpGroupId> {
    switch_of(store).and_then(|(_, s)| s.acl.copp.gid)
}

/// The `CoPP` rules installed (test port `copp_rule_count`).
pub fn copp_rule_count(store: &Store) -> usize {
    switch_of(store).map_or(0, |(_, s)| s.acl.copp.entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_their_priorities_are_the_cpp_ones() {
        let (arp, copy) = copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST));
        assert!(copy);
        assert_eq!(arp.len(), 1);
        assert_eq!(
            arp.first().map(|r| (r.priority, r.m.ip_type)),
            Some((200, Some(e(sys::SAI_ACL_IP_TYPE_ARP_REQUEST))))
        );
        let (bgp, copy) = copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_BGP));
        assert!(!copy);
        assert_eq!(bgp.iter().map(|r| r.priority).collect::<Vec<_>>(), [150, 150]);
        let (ip2me, _) = copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_IP2ME));
        assert_eq!(
            ip2me.iter().map(|r| (r.priority, r.m.present())).collect::<Vec<_>>(),
            [(100, FpQual::DstCpu.bit())]
        );
        assert_eq!(copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_SSH)).0.len(), 4);
        assert_eq!(
            copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY)).0.len(),
            5
        );
        assert_eq!(copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR)).0, []);
        // icmp type in the high byte
        let (ns, _) = copp_rules(e(sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION));
        assert_eq!(
            ns.first().and_then(|r| r.m.icmp_type_code),
            Some(Masked {
                value: 135 << 8,
                mask: 0xff00
            })
        );
    }

    #[test]
    fn keys_order_like_the_cpp_memory_images() {
        // The C++ key starts with the priority's bytes: on a little-endian host 100 < 150 < 200
        // byte-wise too, so IP2ME is created first.
        let key = |t| rule_key(copp_rules(e(t)).0.first().unwrap_or(&FpRule::default()));
        let ip2me = key(sys::SAI_HOSTIF_TRAP_TYPE_IP2ME);
        assert_eq!(ip2me.get(..4), Some(&100u32.to_ne_bytes()[..]));
        assert_ne!(key(sys::SAI_HOSTIF_TRAP_TYPE_LACP), key(sys::SAI_HOSTIF_TRAP_TYPE_LLDP));
        // C++ size: 4+4 +2+2 +4 +1+1 +4*17 +8 +1+1 +2+2 +1+1 +24 +2+2 +1+1 +4 +4 +4 +4 (-1) +5+5 +4 (-2) +4+1+4+4
        let r = FpRule::default();
        assert_eq!(
            rule_key(&r).len(),
            4 + 4 + 4 + 4 + 2 + 4 * 17 + 8 + 2 + 4 + 2 + 24 + 4 + 2 + 4 + 4 + 4 + 4 + 10 + 4 + 4 + 1 + 4 + 4
        );
    }
}

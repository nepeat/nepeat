//! The field processor and meters of `openbcm_backend.cpp`: groups, entries with the C++
//! policer-clone fallback and its bookkeeping (`entries_`, `policer_cfg_`, `policer_users_`),
//! entry counters, policers. The rule the glue gets is field for field what the C++ `to_c`
//! builds ([`to_c`]).
use crate::{Entry, OpenbcmBackend, log, rv, status};
use sai_core::backend::types::{
    EgressId, FpCounter, FpEntryId, FpGroupId, FpQual, FpRule, MacAddr, Masked, PolicerCfg, PolicerId,
};
use sai_core::backend::{Field, Policer};
use sai_core::error::{Error, Result};
use sai_sys as sys;
use sdk_glue::field::FpRuleRaw;
use sdk_glue::{Code, IpType};
use std::net::{Ipv4Addr, Ipv6Addr};

/// `sai_core`'s qualifier as the glue's (the same `ob::FpQual` order; `DstCpu` is the glue's
/// `DSTEGRESS` with the CPU egress object).
pub(crate) const fn glue_qual(q: FpQual) -> sdk_glue::FpQual {
    use sdk_glue::FpQual as G;
    match q {
        FpQual::InPorts => G::InPorts,
        FpQual::EtherType => G::EtherType,
        FpQual::IpType => G::IpType,
        FpQual::IpProtocol => G::IpProtocol,
        FpQual::SrcIp => G::SrcIp,
        FpQual::DstIp => G::DstIp,
        FpQual::SrcIp6 => G::SrcIp6,
        FpQual::DstIp6 => G::DstIp6,
        FpQual::L4SrcPort => G::L4SrcPort,
        FpQual::L4DstPort => G::L4DstPort,
        FpQual::TcpFlags => G::TcpFlags,
        FpQual::IcmpTypeCode => G::IcmpTypeCode,
        FpQual::Tos => G::Tos,
        FpQual::DstMac => G::DstMac,
        FpQual::SrcMac => G::SrcMac,
        FpQual::OuterVlan => G::OuterVlan,
        FpQual::Ttl => G::Ttl,
        FpQual::DstCpu => G::DstL3Egress,
    }
}

/// C++ `ip_type`: `sai_acl_ip_type_t` to `OB_FP_*`, -1 for anything else (`ANY`, the
/// fragment types, an absent qualifier: the C++ field is 0, `ANY`).
pub(crate) fn ip_type(t: i32) -> i32 {
    let table = [
        (sys::SAI_ACL_IP_TYPE_IP, IpType::Ip),
        (sys::SAI_ACL_IP_TYPE_NON_IP, IpType::NonIp),
        (sys::SAI_ACL_IP_TYPE_IPV4ANY, IpType::Ipv4),
        (sys::SAI_ACL_IP_TYPE_NON_IPV4, IpType::NonIpv4),
        (sys::SAI_ACL_IP_TYPE_IPV6ANY, IpType::Ipv6),
        (sys::SAI_ACL_IP_TYPE_NON_IPV6, IpType::NonIpv6),
        (sys::SAI_ACL_IP_TYPE_ARP, IpType::Arp),
        (sys::SAI_ACL_IP_TYPE_ARP_REQUEST, IpType::ArpRequest),
        (sys::SAI_ACL_IP_TYPE_ARP_REPLY, IpType::ArpReply),
    ];
    table
        .iter()
        .find(|(sai, _)| i64::from(*sai) == i64::from(t))
        .and_then(|(_, ob)| i32::try_from(ob.raw()).ok())
        .unwrap_or(-1)
}

fn pair<T: Copy + Default>(v: Option<Masked<T>>) -> (T, T) {
    v.map_or_else(|| (T::default(), T::default()), |m| (m.value, m.mask))
}

fn mac(v: Option<Masked<MacAddr>>) -> ([u8; 6], [u8; 6]) {
    v.map_or(([0; 6], [0; 6]), |m| (m.value.0, m.mask.0))
}

/// One side's address field of the C++ `FpMatch` (`IpAddr src` / `src_mask`: IPv4 in the
/// first 4 bytes): the IPv6 address, then the IPv4 one written over its first 4 bytes.
fn address(v4: Option<Masked<Ipv4Addr>>, v6: Option<Masked<Ipv6Addr>>) -> ([u8; 16], [u8; 16]) {
    let (mut addr, mut mask) = v6.map_or(([0; 16], [0; 16]), |m| (m.value.octets(), m.mask.octets()));
    if let Some(m) = v4 {
        for (o, b) in addr.iter_mut().zip(m.value.octets()) {
            *o = b;
        }
        for (o, b) in mask.iter_mut().zip(m.mask.octets()) {
            *o = b;
        }
    }
    (addr, mask)
}

/// An id the C++ holds in an `int`.
fn int(id: u32) -> Result<i32> {
    i32::try_from(id).map_err(|_| Error::InvalidParameter)
}

/// The C++ `to_c`: the glue's rule for `r`, every field copied whether its qualifier is
/// present or not (absent ones are the C++ defaults, 0), the IP type mapped ([`ip_type`]),
/// `dst_egress` the CPU egress object always (`DstCpu`: what the L3 lookup picked is the CPU),
/// the priority clamped to `0x7fffffff`, the policer -1 for none.
pub(crate) fn to_c(r: &FpRule, cpu_egress: EgressId) -> Result<FpRuleRaw> {
    let m = &r.m;
    let (ether, ether_mask) = pair(m.ether_type);
    let (proto, proto_mask) = pair(m.ip_protocol);
    let (src, src_mask) = address(m.src_ip, m.src_ip6);
    let (dst, dst_mask) = address(m.dst_ip, m.dst_ip6);
    let (l4_src, l4_src_mask) = pair(m.l4_src_port);
    let (l4_dst, l4_dst_mask) = pair(m.l4_dst_port);
    let (tcp, tcp_mask) = pair(m.tcp_flags);
    let (icmp, icmp_mask) = pair(m.icmp_type_code);
    let (tos, tos_mask) = pair(m.tos);
    let (dmac, dmac_mask) = mac(m.dst_mac);
    let (smac, smac_mask) = mac(m.src_mac);
    let (vlan, vlan_mask) = pair(m.outer_vlan);
    let (ttl, ttl_mask) = pair(m.ttl);
    Ok(FpRuleRaw {
        present: m.present(),
        ether,
        ether_mask,
        ip_type: ip_type(m.ip_type.unwrap_or(0)),
        proto,
        proto_mask,
        src,
        src_mask,
        dst,
        dst_mask,
        l4_src,
        l4_src_mask,
        l4_dst,
        l4_dst_mask,
        tcp,
        tcp_mask,
        icmp,
        icmp_mask,
        tos,
        tos_mask,
        dmac,
        dmac_mask,
        smac,
        smac_mask,
        vlan,
        vlan_mask,
        ttl,
        ttl_mask,
        dst_egress: int(cpu_egress.0)?,
        ports: m.in_ports.iter().flatten().map(|p| i32::from(p.0)).collect(),
        priority: i32::try_from(r.priority.min(0x7fff_ffff)).unwrap_or(i32::MAX),
        drop: r.drop,
        drop_cancel: r.drop_cancel,
        copy_cpu: r.copy_cpu,
        counter: r.counter,
        policer: r.policer.map_or(Ok(-1), |p| int(p.0))?,
        green_drop: r.green_drop,
        yellow_drop: r.yellow_drop,
        red_drop: r.red_drop,
        cpu_copy_only: r.cpu_copy_only,
    })
}

/// The C++ `to_c(PolicerCfg)`.
pub(crate) const fn policer_cfg(p: &PolicerCfg) -> sdk_glue::PolicerCfg {
    sdk_glue::PolicerCfg {
        tr_tcm: p.tr_tcm,
        packets: p.packets,
        color_blind: p.color_blind,
        cir: p.cir,
        cbs: p.cbs,
        pir: p.pir,
        pbs: p.pbs,
    }
}

impl Field for OpenbcmBackend {
    fn fp_group_create(&mut self, qset: &[FpQual], priority: i32) -> Result<FpGroupId> {
        let q: Vec<sdk_glue::FpQual> = qset.iter().copied().map(glue_qual).collect();
        let bits = qset.iter().fold(0u32, |b, x| b | x.bit());
        let gid = rv(
            "ob_sdk_fp_group_create(unit_, q, priority, &gid)",
            self.switch()?.fp_group_create(&q, priority),
        )?;
        log::notice(
            sys::SAI_API_ACL,
            format_args!("field group {gid}: priority {priority}, qualifiers 0x{bits:x}"),
        );
        Ok(FpGroupId(gid))
    }

    fn fp_group_destroy(&mut self, gid: FpGroupId) -> Result<()> {
        rv(
            "ob_sdk_fp_group_destroy(unit_, gid)",
            self.switch()?.fp_group_destroy(gid.0),
        )
    }

    fn fp_entry_create(&mut self, gid: FpGroupId, r: &FpRule) -> Result<FpEntryId> {
        let up = self.up_mut()?;
        let mut c = to_c(r, up.cpu_egress)?;
        let mut clone = None;
        let mut result = up.switch.fp_entry_create_raw(gid.0, &c);
        if let (Err(error), Some(policer)) = (result, r.policer) {
            // The SDK shares a meter only between entries of one slice: this entry gets a copy
            // of it (same rates, its own bucket).
            let shared = matches!(error.code(), Code::Config | Code::Param)
                && *up.policer_users.entry(policer.0).or_insert(0) > 0;
            if let Some(cfg) = up.policer_cfg.get(&policer.0).copied().filter(|_| shared) {
                // A glue id is an int (sdk-glue checked it): the filter never refuses one.
                if let Some((copy, raw)) = up
                    .switch
                    .policer_create(&cfg)
                    .ok()
                    .and_then(|p| Some((p, int(p).ok()?)))
                {
                    c.policer = raw;
                    result = up.switch.fp_entry_create_raw(gid.0, &c);
                    match result {
                        Err(_) => {
                            // As the C++: the copy goes, its answer is not looked at.
                            let _ = up.switch.policer_destroy(copy);
                        }
                        Ok(entry) => {
                            clone = Some(copy);
                            log::warn(
                                sys::SAI_API_POLICER,
                                format_args!(
                                    "policer {}: the SDK refused to share it; entry {} meters with copy {copy}",
                                    policer.0, entry.id
                                ),
                            );
                        }
                    }
                }
            }
        }
        let entry = result.map_err(|error| {
            log::error(
                sys::SAI_API_ACL,
                format_args!("field entry in group {}: {error}", gid.0),
            );
            status::sai_error(error)
        })?;
        up.entries.insert(
            entry.id,
            Entry {
                stat: entry.stat,
                policer: r.policer.map(|p| p.0),
                clone,
            },
        );
        if let Some(policer) = r.policer {
            let users = up.policer_users.entry(policer.0).or_insert(0);
            *users = users.saturating_add(1);
        }
        Ok(FpEntryId(entry.id))
    }

    fn fp_entry_destroy(&mut self, eid: FpEntryId) -> Result<()> {
        let up = self.up_mut()?;
        let Some(entry) = up.entries.get(&eid.0).copied() else {
            return Err(Error::ItemNotFound);
        };
        let result = up.switch.fp_entry_destroy(sdk_glue::FpEntry {
            id: eid.0,
            stat: entry.stat,
        });
        if let Some(clone) = entry.clone {
            let _ = up.switch.policer_destroy(clone);
        }
        if let Some(policer) = entry.policer {
            let users = up.policer_users.entry(policer).or_insert(0);
            *users = users.saturating_sub(1);
        }
        up.entries.remove(&eid.0);
        // C++ RV(rv): the call's text is "rv".
        rv("rv", result)
    }

    fn fp_entry_counter(&self, eid: FpEntryId) -> Result<FpCounter> {
        let up = self.up()?;
        let entry = up.entries.get(&eid.0).ok_or(Error::ItemNotFound)?;
        let stat = entry.stat.ok_or(Error::NotSupported)?;
        let (packets, bytes) = rv(
            "ob_sdk_fp_stat_get(unit_, it->second.stat, &packets, &bytes)",
            up.switch.fp_stat_get(stat),
        )?;
        Ok(FpCounter { packets, bytes })
    }
}

impl Policer for OpenbcmBackend {
    fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<PolicerId> {
        let c = policer_cfg(cfg);
        let up = self.up_mut()?;
        let pid = rv("ob_sdk_policer_create(unit_, &c, &pid)", up.switch.policer_create(&c))?;
        up.policer_cfg.insert(pid, c);
        Ok(PolicerId(pid))
    }

    fn policer_set(&mut self, pid: PolicerId, cfg: &PolicerCfg) -> Result<()> {
        let c = policer_cfg(cfg);
        let up = self.up_mut()?;
        rv("ob_sdk_policer_set(unit_, pid, &c)", up.switch.policer_set(pid.0, &c))?;
        up.policer_cfg.insert(pid.0, c);
        // Every copy of it (entries by id, as the C++ map iterates).
        let clones: Vec<u32> = up
            .entries
            .values()
            .filter(|e| e.policer == Some(pid.0))
            .filter_map(|e| e.clone)
            .collect();
        for clone in clones {
            rv(
                "ob_sdk_policer_set(unit_, e.second.clone, &c)",
                up.switch.policer_set(clone, &c),
            )?;
        }
        Ok(())
    }

    fn policer_destroy(&mut self, pid: PolicerId) -> Result<()> {
        let up = self.up_mut()?;
        if *up.policer_users.entry(pid.0).or_insert(0) > 0 {
            return Err(Error::ObjectInUse);
        }
        rv("ob_sdk_policer_destroy(unit_, pid)", up.switch.policer_destroy(pid.0))?;
        up.policer_cfg.remove(&pid.0);
        up.policer_users.remove(&pid.0);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unnecessary_wraps, clippy::too_many_lines, reason = "test code")]
mod tests {
    use super::{glue_qual, ip_type, policer_cfg, to_c};
    use sai_core::backend::types::{
        EgressId, FpMatch, FpQual, FpRule, MacAddr, Masked, MirrorId, PolicerCfg, PolicerId, Redirect,
    };
    use sai_core::backend::{HwPort, types::Dest};
    use sai_sys as sys;
    use sdk_glue::field::FpRuleRaw;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn qualifier_bits_are_the_cpp_ones() {
        for q in FpQual::ALL {
            assert_eq!(glue_qual(q).bit(), q.bit(), "{q:?}");
        }
    }

    #[test]
    fn ip_types_as_the_cpp_maps_them() {
        assert_eq!(ip_type(0), -1, "ANY / absent");
        assert_eq!(ip_type(i32::try_from(sys::SAI_ACL_IP_TYPE_IP).unwrap()), 0);
        assert_eq!(ip_type(i32::try_from(sys::SAI_ACL_IP_TYPE_ARP_REPLY).unwrap()), 8);
        assert_eq!(ip_type(i32::try_from(sys::SAI_ACL_IP_TYPE_IPV4ANY).unwrap()), 2);
        assert_eq!(ip_type(i32::try_from(sys::SAI_ACL_IP_TYPE_NON_IPV6).unwrap()), 5);
        assert_eq!(ip_type(i32::try_from(sys::SAI_ACL_IP_TYPE_ANY).unwrap()), -1);
        assert_eq!(ip_type(99), -1);
    }

    fn m<T>(value: T, mask: T) -> Option<Masked<T>> {
        Some(Masked { value, mask })
    }

    /// The C++ `to_c` field for field: every qualifier, the actions, and what it leaves out.
    #[test]
    fn the_rule_is_what_the_cpp_to_c_builds() {
        let rule = FpRule {
            priority: 0x8000_0000,
            m: FpMatch {
                in_ports: Some(vec![HwPort(5), HwPort(1)]),
                ether_type: m(0x0800, 0xffff),
                ip_type: Some(i32::try_from(sys::SAI_ACL_IP_TYPE_IPV6ANY).unwrap()),
                ip_protocol: m(6, 0xff),
                src_ip: m(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(255, 255, 255, 0)),
                dst_ip6: m(
                    Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 2),
                    Ipv6Addr::new(0xffff, 0xffff, 0, 0, 0, 0, 0, 0),
                ),
                l4_src_port: m(1234, 0xffff),
                l4_dst_port: m(179, 0xffff),
                tcp_flags: m(0x12, 0x3f),
                icmp_type_code: m(0x0800, 0xff00),
                tos: m(0xb8, 0xfc),
                dst_mac: m(MacAddr([1, 2, 3, 4, 5, 6]), MacAddr([0xff; 6])),
                src_mac: m(MacAddr([6, 5, 4, 3, 2, 1]), MacAddr([0xff, 0xff, 0xff, 0, 0, 0])),
                outer_vlan: m(100, 0xfff),
                ttl: m(1, 0xff),
                dst_cpu: true,
                // only with the optional features: the C++ to_c does not look at them
                l4_src_range: Some((1, 2)),
                l4_dst_range: Some((3, 4)),
                out_ports: vec![HwPort(9)],
                ..FpMatch::default()
            },
            drop: true,
            drop_cancel: true,
            copy_cpu: true,
            counter: true,
            policer: Some(PolicerId(7)),
            green_drop: true,
            yellow_drop: false,
            red_drop: true,
            cpu_copy_only: true,
            mirror_ingress: vec![MirrorId(1)],
            mirror_egress: vec![MirrorId(2)],
            redirect: Some(Redirect::Dest(Dest::Port(HwPort(3)))),
            set_tc: Some(3),
            set_dscp: Some(46),
        };
        let mut src = [0u8; 16];
        src[..4].copy_from_slice(&[10, 0, 0, 1]);
        let mut src_mask = [0u8; 16];
        src_mask[..4].copy_from_slice(&[255, 255, 255, 0]);
        let expected = FpRuleRaw {
            present: rule.m.present(),
            ether: 0x0800,
            ether_mask: 0xffff,
            ip_type: 4, // OB_FP_IPV6
            proto: 6,
            proto_mask: 0xff,
            src,
            src_mask,
            dst: Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 2).octets(),
            dst_mask: Ipv6Addr::new(0xffff, 0xffff, 0, 0, 0, 0, 0, 0).octets(),
            l4_src: 1234,
            l4_src_mask: 0xffff,
            l4_dst: 179,
            l4_dst_mask: 0xffff,
            tcp: 0x12,
            tcp_mask: 0x3f,
            icmp: 0x0800,
            icmp_mask: 0xff00,
            tos: 0xb8,
            tos_mask: 0xfc,
            dmac: [1, 2, 3, 4, 5, 6],
            dmac_mask: [0xff; 6],
            smac: [6, 5, 4, 3, 2, 1],
            smac_mask: [0xff, 0xff, 0xff, 0, 0, 0],
            vlan: 100,
            vlan_mask: 0xfff,
            ttl: 1,
            ttl_mask: 0xff,
            dst_egress: 100_007,
            ports: vec![5, 1],
            priority: 0x7fff_ffff,
            drop: true,
            drop_cancel: true,
            copy_cpu: true,
            counter: true,
            policer: 7,
            green_drop: true,
            yellow_drop: false,
            red_drop: true,
            cpu_copy_only: true,
        };
        assert_eq!(to_c(&rule, EgressId(100_007)), Ok(expected));
        // nothing present: the fields are the C++ defaults, the IP type -1, the CPU egress set
        let none = to_c(&FpRule::default(), EgressId(3)).unwrap();
        assert_eq!(
            none,
            FpRuleRaw {
                ip_type: -1,
                dst_egress: 3,
                policer: -1,
                ..FpRuleRaw::default()
            }
        );
        // both families on one side: the IPv6 address under the IPv4 one
        let both = FpRule {
            m: FpMatch {
                dst_ip: m(Ipv4Addr::new(1, 2, 3, 4), Ipv4Addr::BROADCAST),
                dst_ip6: m(Ipv6Addr::LOCALHOST, Ipv6Addr::from_bits(u128::MAX)),
                ..FpMatch::default()
            },
            ..FpRule::default()
        };
        let c = to_c(&both, EgressId(3)).unwrap();
        let mut dst = Ipv6Addr::LOCALHOST.octets();
        dst[..4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!((c.dst, c.dst_mask), (dst, [0xff; 16]));
        // ids that do not fit the C++ int
        let big = FpRule {
            policer: Some(PolicerId(u32::MAX)),
            ..FpRule::default()
        };
        assert!(to_c(&big, EgressId(3)).is_err());
    }

    #[test]
    fn the_policer_is_the_cpp_one() {
        let cfg = PolicerCfg {
            tr_tcm: true,
            packets: false,
            color_blind: true,
            cir: 1,
            cbs: 2,
            pir: 3,
            pbs: 4,
        };
        assert_eq!(
            policer_cfg(&cfg),
            sdk_glue::PolicerCfg {
                tr_tcm: true,
                packets: false,
                color_blind: true,
                cir: 1,
                cbs: 2,
                pir: 3,
                pbs: 4,
            }
        );
    }
}

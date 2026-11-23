//! The ingress field processor (ACL tables, CoPP): groups by qualifier set, entries (rules)
//! installed at create, their packet / byte counters. Data and masks in network order.

use std::ffi::c_int;
use std::marker::PhantomData;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::port::c_value;
use crate::switch::{Switch, count};

/// A qualifier: its bit in a group's qualifier set is the glue's `OB_FPQ_*` (`sdk_glue.h`; the
/// values come from there, the names are this crate's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FpQual {
    /// `OB_FPQ_INPORTS`
    InPorts,
    /// `OB_FPQ_ETHERTYPE`
    EtherType,
    /// `OB_FPQ_IPTYPE`
    IpType,
    /// `OB_FPQ_IPPROTO`
    IpProtocol,
    /// `OB_FPQ_SRCIP`
    SrcIp,
    /// `OB_FPQ_DSTIP`
    DstIp,
    /// `OB_FPQ_SRCIP6`
    SrcIp6,
    /// `OB_FPQ_DSTIP6`
    DstIp6,
    /// `OB_FPQ_L4SRC`
    L4SrcPort,
    /// `OB_FPQ_L4DST`
    L4DstPort,
    /// `OB_FPQ_TCP` (TCP flags)
    TcpFlags,
    /// `OB_FPQ_ICMP` (type << 8 | code)
    IcmpTypeCode,
    /// `OB_FPQ_TOS` (DSCP << 2 | ECN)
    Tos,
    /// `OB_FPQ_DSTMAC`
    DstMac,
    /// `OB_FPQ_SRCMAC`
    SrcMac,
    /// `OB_FPQ_VLAN` (outer VLAN id)
    OuterVlan,
    /// `OB_FPQ_TTL`
    Ttl,
    /// `OB_FPQ_DSTEGRESS` (the L3 egress object the lookup chose)
    DstL3Egress,
}

impl FpQual {
    /// Every qualifier, in the glue's bit order.
    pub const ALL: [Self; 18] = [
        Self::InPorts,
        Self::EtherType,
        Self::IpType,
        Self::IpProtocol,
        Self::SrcIp,
        Self::DstIp,
        Self::SrcIp6,
        Self::DstIp6,
        Self::L4SrcPort,
        Self::L4DstPort,
        Self::TcpFlags,
        Self::IcmpTypeCode,
        Self::Tos,
        Self::DstMac,
        Self::SrcMac,
        Self::OuterVlan,
        Self::Ttl,
        Self::DstL3Egress,
    ];

    /// The bit index (`OB_FPQ_*` of the header).
    pub const fn index(self) -> u32 {
        match self {
            Self::InPorts => sys::OB_FPQ_INPORTS,
            Self::EtherType => sys::OB_FPQ_ETHERTYPE,
            Self::IpType => sys::OB_FPQ_IPTYPE,
            Self::IpProtocol => sys::OB_FPQ_IPPROTO,
            Self::SrcIp => sys::OB_FPQ_SRCIP,
            Self::DstIp => sys::OB_FPQ_DSTIP,
            Self::SrcIp6 => sys::OB_FPQ_SRCIP6,
            Self::DstIp6 => sys::OB_FPQ_DSTIP6,
            Self::L4SrcPort => sys::OB_FPQ_L4SRC,
            Self::L4DstPort => sys::OB_FPQ_L4DST,
            Self::TcpFlags => sys::OB_FPQ_TCP,
            Self::IcmpTypeCode => sys::OB_FPQ_ICMP,
            Self::Tos => sys::OB_FPQ_TOS,
            Self::DstMac => sys::OB_FPQ_DSTMAC,
            Self::SrcMac => sys::OB_FPQ_SRCMAC,
            Self::OuterVlan => sys::OB_FPQ_VLAN,
            Self::Ttl => sys::OB_FPQ_TTL,
            Self::DstL3Egress => sys::OB_FPQ_DSTEGRESS,
        }
    }

    /// The qualifier's bit in a qualifier set.
    pub const fn bit(self) -> u32 {
        1 << self.index()
    }
}

// The header's qualifier list and this enum have the same length (each name maps to its own
// constant above; a new OB_FPQ_* without a variant stops the build here).
const _: () = assert!(FpQual::ALL.len() == sys::OB_FPQ_COUNT as usize);

/// The IP type a rule matches (`OB_FP_*` of the header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpType {
    /// `OB_FP_IP`
    Ip,
    /// `OB_FP_NON_IP`
    NonIp,
    /// `OB_FP_IPV4`
    Ipv4,
    /// `OB_FP_NON_IPV4`
    NonIpv4,
    /// `OB_FP_IPV6`
    Ipv6,
    /// `OB_FP_NON_IPV6`
    NonIpv6,
    /// `OB_FP_ARP`
    Arp,
    /// `OB_FP_ARP_REQUEST`
    ArpRequest,
    /// `OB_FP_ARP_REPLY`
    ArpReply,
}

impl IpType {
    /// The glue's number (`OB_FP_*`).
    pub const fn raw(self) -> u32 {
        match self {
            Self::Ip => sys::OB_FP_IP,
            Self::NonIp => sys::OB_FP_NON_IP,
            Self::Ipv4 => sys::OB_FP_IPV4,
            Self::NonIpv4 => sys::OB_FP_NON_IPV4,
            Self::Ipv6 => sys::OB_FP_IPV6,
            Self::NonIpv6 => sys::OB_FP_NON_IPV6,
            Self::Arp => sys::OB_FP_ARP,
            Self::ArpRequest => sys::OB_FP_ARP_REQUEST,
            Self::ArpReply => sys::OB_FP_ARP_REPLY,
        }
    }
}

/// A rule: what it matches (`Some`: the qualifier is present, value and mask) and what it does.
/// The IPv4 and IPv6 address of one side share the glue's field: at most one of them per side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "the glue's flags")]
pub struct FpRule {
    /// Ingress ports (any of them).
    pub in_ports: Option<Vec<i32>>,
    /// Ethertype.
    pub ether_type: Option<(u16, u16)>,
    /// IP type.
    pub ip_type: Option<IpType>,
    /// IP protocol.
    pub ip_protocol: Option<(u8, u8)>,
    /// IPv4 source.
    pub src_ip: Option<([u8; 4], [u8; 4])>,
    /// IPv4 destination.
    pub dst_ip: Option<([u8; 4], [u8; 4])>,
    /// IPv6 source.
    pub src_ip6: Option<([u8; 16], [u8; 16])>,
    /// IPv6 destination.
    pub dst_ip6: Option<([u8; 16], [u8; 16])>,
    /// L4 source port.
    pub l4_src_port: Option<(u16, u16)>,
    /// L4 destination port.
    pub l4_dst_port: Option<(u16, u16)>,
    /// TCP flags.
    pub tcp_flags: Option<(u8, u8)>,
    /// ICMP type << 8 | code.
    pub icmp_type_code: Option<(u16, u16)>,
    /// TOS byte.
    pub tos: Option<(u8, u8)>,
    /// Destination MAC.
    pub dst_mac: Option<([u8; 6], [u8; 6])>,
    /// Source MAC.
    pub src_mac: Option<([u8; 6], [u8; 6])>,
    /// Outer VLAN id.
    pub outer_vlan: Option<(u16, u16)>,
    /// TTL.
    pub ttl: Option<(u8, u8)>,
    /// The L3 egress object the lookup chose.
    pub dst_l3_egress: Option<u32>,
    /// Priority within the group (higher wins).
    pub priority: i32,
    /// Drop.
    pub drop: bool,
    /// Cancel a drop of a lower-priority rule.
    pub drop_cancel: bool,
    /// Copy to the CPU.
    pub copy_cpu: bool,
    /// A packet + byte counter.
    pub counter: bool,
    /// A policer (meter) attached.
    pub policer: Option<u32>,
    /// Policer colours: drop green / yellow / red, or with `cpu_copy_only` only cancel their copy
    /// to the CPU (`CoPP`).
    pub green_drop: bool,
    /// See `green_drop`.
    pub yellow_drop: bool,
    /// See `green_drop`.
    pub red_drop: bool,
    /// See `green_drop`.
    pub cpu_copy_only: bool,
}

/// A rule field for field as the glue's `ob_fp_rule_t` takes it (`ports` / `nports` as the
/// vector): for a caller that builds the struct itself, as the C++ backend's `to_c` does (fields
/// of absent qualifiers, the IP type as an `OB_FP_*` number or -1, `dst_egress` set without its
/// qualifier). The glue reads only what `present` names; nothing here is checked before the call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "the glue's flags")]
pub struct FpRuleRaw {
    /// `OB_FPQ_*` bits of the qualifiers present.
    pub present: u32,
    /// Ethertype and mask.
    pub ether: u16,
    /// See `ether`.
    pub ether_mask: u16,
    /// `OB_FP_*` ([`IpType::raw`]), or anything else.
    pub ip_type: i32,
    /// IP protocol and mask.
    pub proto: u8,
    /// See `proto`.
    pub proto_mask: u8,
    /// Source address (IPv4 in the first 4 bytes) and mask, network order.
    pub src: [u8; 16],
    /// See `src`.
    pub src_mask: [u8; 16],
    /// Destination address and mask.
    pub dst: [u8; 16],
    /// See `dst`.
    pub dst_mask: [u8; 16],
    /// L4 source port and mask.
    pub l4_src: u16,
    /// See `l4_src`.
    pub l4_src_mask: u16,
    /// L4 destination port and mask.
    pub l4_dst: u16,
    /// See `l4_dst`.
    pub l4_dst_mask: u16,
    /// TCP flags and mask.
    pub tcp: u8,
    /// See `tcp`.
    pub tcp_mask: u8,
    /// ICMP type << 8 | code, and mask.
    pub icmp: u16,
    /// See `icmp`.
    pub icmp_mask: u16,
    /// TOS byte and mask.
    pub tos: u8,
    /// See `tos`.
    pub tos_mask: u8,
    /// Destination MAC and mask.
    pub dmac: [u8; 6],
    /// See `dmac`.
    pub dmac_mask: [u8; 6],
    /// Source MAC and mask.
    pub smac: [u8; 6],
    /// See `smac`.
    pub smac_mask: [u8; 6],
    /// Outer VLAN id and mask.
    pub vlan: u16,
    /// See `vlan`.
    pub vlan_mask: u16,
    /// TTL and mask.
    pub ttl: u8,
    /// See `ttl`.
    pub ttl_mask: u8,
    /// The L3 egress object of `OB_FPQ_DSTEGRESS`.
    pub dst_egress: i32,
    /// In-ports (`OB_FPQ_INPORTS`).
    pub ports: Vec<i32>,
    /// Priority within the group.
    pub priority: i32,
    /// Drop.
    pub drop: bool,
    /// Cancel a drop of a lower-priority rule.
    pub drop_cancel: bool,
    /// Copy to the CPU.
    pub copy_cpu: bool,
    /// A packet + byte counter.
    pub counter: bool,
    /// The policer, -1 none.
    pub policer: i32,
    /// Policer colours, as [`FpRule::green_drop`].
    pub green_drop: bool,
    /// See `green_drop`.
    pub yellow_drop: bool,
    /// See `green_drop`.
    pub red_drop: bool,
    /// See `green_drop`.
    pub cpu_copy_only: bool,
}

impl FpRuleRaw {
    /// The glue's struct, its `ports` pointing into this rule. `OB_E_PARAM`: more ports than an
    /// `int` counts.
    pub(crate) fn raw(&self) -> Result<RawRule<'_>, Error> {
        let ports: &[c_int] = &self.ports;
        let raw = sys::ob_fp_rule_t {
            present: self.present,
            ether: self.ether,
            ether_mask: self.ether_mask,
            ip_type: self.ip_type,
            proto: self.proto,
            proto_mask: self.proto_mask,
            src: self.src,
            src_mask: self.src_mask,
            dst: self.dst,
            dst_mask: self.dst_mask,
            l4_src: self.l4_src,
            l4_src_mask: self.l4_src_mask,
            l4_dst: self.l4_dst,
            l4_dst_mask: self.l4_dst_mask,
            tcp: self.tcp,
            tcp_mask: self.tcp_mask,
            icmp: self.icmp,
            icmp_mask: self.icmp_mask,
            tos: self.tos,
            tos_mask: self.tos_mask,
            dmac: self.dmac,
            dmac_mask: self.dmac_mask,
            smac: self.smac,
            smac_mask: self.smac_mask,
            vlan: self.vlan,
            vlan_mask: self.vlan_mask,
            ttl: self.ttl,
            ttl_mask: self.ttl_mask,
            dst_egress: self.dst_egress,
            // Into the borrowed vector; only the glue reads through it, during the call.
            ports: ports.as_ptr(),
            nports: c_int::try_from(ports.len()).map_err(|_| Error::PARAM)?,
            priority: self.priority,
            drop: bool_int(self.drop),
            drop_cancel: bool_int(self.drop_cancel),
            copy_cpu: bool_int(self.copy_cpu),
            counter: bool_int(self.counter),
            policer: self.policer,
            green_drop: bool_int(self.green_drop),
            yellow_drop: bool_int(self.yellow_drop),
            red_drop: bool_int(self.red_drop),
            cpu_copy_only: bool_int(self.cpu_copy_only),
        };
        Ok(RawRule {
            raw,
            ports,
            _borrow: PhantomData,
        })
    }
}

/// An installed entry: its id and its counter's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpEntry {
    /// The entry.
    pub id: u32,
    /// The counter, with [`FpRule::counter`].
    pub stat: Option<u32>,
}

/// A rule as the glue takes it (`ob_fp_rule_t`), with the slice its `ports` pointer points into
/// borrowed for as long as it lives.
#[derive(Debug)]
pub(crate) struct RawRule<'a> {
    raw: sys::ob_fp_rule_t,
    #[cfg_attr(not(feature = "rust-fake"), allow(dead_code, reason = "read by the Rust fake"))]
    ports: &'a [c_int],
    _borrow: PhantomData<&'a [c_int]>,
}

impl RawRule<'_> {
    pub(crate) const fn raw(&self) -> &sys::ob_fp_rule_t {
        &self.raw
    }

    /// The in-ports, as `raw().ports` / `nports` describe them.
    #[cfg_attr(not(feature = "rust-fake"), allow(dead_code, reason = "read by the Rust fake"))]
    pub(crate) const fn ports(&self) -> &[c_int] {
        self.ports
    }
}

fn wide<const N: usize>(bytes: [u8; N]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (o, b) in out.iter_mut().zip(bytes) {
        *o = b;
    }
    out
}

fn bool_int(b: bool) -> c_int {
    c_int::from(b)
}

impl FpRule {
    /// The qualifier set this rule needs.
    pub fn qualifiers(&self) -> Vec<FpQual> {
        let has = [
            self.in_ports.is_some(),
            self.ether_type.is_some(),
            self.ip_type.is_some(),
            self.ip_protocol.is_some(),
            self.src_ip.is_some(),
            self.dst_ip.is_some(),
            self.src_ip6.is_some(),
            self.dst_ip6.is_some(),
            self.l4_src_port.is_some(),
            self.l4_dst_port.is_some(),
            self.tcp_flags.is_some(),
            self.icmp_type_code.is_some(),
            self.tos.is_some(),
            self.dst_mac.is_some(),
            self.src_mac.is_some(),
            self.outer_vlan.is_some(),
            self.ttl.is_some(),
            self.dst_l3_egress.is_some(),
        ];
        FpQual::ALL
            .into_iter()
            .zip(has)
            .filter(|(_, h)| *h)
            .map(|(q, _)| q)
            .collect()
    }

    /// The glue's struct. `OB_E_PARAM`: both an IPv4 and an IPv6 address on one side, or a value
    /// out of the glue's `int`.
    pub(crate) fn raw(&self) -> Result<RawRule<'_>, Error> {
        if (self.src_ip.is_some() && self.src_ip6.is_some()) || (self.dst_ip.is_some() && self.dst_ip6.is_some()) {
            return Err(Error::PARAM);
        }
        let present = self.qualifiers().iter().fold(0u32, |bits, q| bits | q.bit());
        let ports: &[c_int] = self.in_ports.as_deref().unwrap_or(&[]);
        let (src, src_mask) = match (self.src_ip, self.src_ip6) {
            (Some((a, m)), _) => (wide(a), wide(m)),
            (_, Some((a, m))) => (a, m),
            _ => ([0; 16], [0; 16]),
        };
        let (dst, dst_mask) = match (self.dst_ip, self.dst_ip6) {
            (Some((a, m)), _) => (wide(a), wide(m)),
            (_, Some((a, m))) => (a, m),
            _ => ([0; 16], [0; 16]),
        };
        let pair16 = |v: Option<(u16, u16)>| v.unwrap_or((0, 0));
        let pair8 = |v: Option<(u8, u8)>| v.unwrap_or((0, 0));
        let mac = |v: Option<([u8; 6], [u8; 6])>| v.unwrap_or(([0; 6], [0; 6]));
        let (ether, ether_mask) = pair16(self.ether_type);
        let (proto, proto_mask) = pair8(self.ip_protocol);
        let (l4_src, l4_src_mask) = pair16(self.l4_src_port);
        let (l4_dst, l4_dst_mask) = pair16(self.l4_dst_port);
        let (tcp, tcp_mask) = pair8(self.tcp_flags);
        let (icmp, icmp_mask) = pair16(self.icmp_type_code);
        let (tos, tos_mask) = pair8(self.tos);
        let (dmac, dmac_mask) = mac(self.dst_mac);
        let (smac, smac_mask) = mac(self.src_mac);
        let (vlan, vlan_mask) = pair16(self.outer_vlan);
        let (ttl, ttl_mask) = pair8(self.ttl);
        let raw = sys::ob_fp_rule_t {
            present,
            ether,
            ether_mask,
            ip_type: self
                .ip_type
                .map_or(Ok(0), |t| c_int::try_from(t.raw()).map_err(|_| Error::PARAM))?,
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
            dst_egress: self.dst_l3_egress.map_or(Ok(0), c_value)?,
            // Into the borrowed slice; only the glue reads through it, during the call.
            ports: ports.as_ptr(),
            nports: c_int::try_from(ports.len()).map_err(|_| Error::PARAM)?,
            priority: self.priority,
            drop: bool_int(self.drop),
            drop_cancel: bool_int(self.drop_cancel),
            copy_cpu: bool_int(self.copy_cpu),
            counter: bool_int(self.counter),
            policer: self.policer.map_or(Ok(-1), c_value)?,
            green_drop: bool_int(self.green_drop),
            yellow_drop: bool_int(self.yellow_drop),
            red_drop: bool_int(self.red_drop),
            cpu_copy_only: bool_int(self.cpu_copy_only),
        };
        Ok(RawRule {
            raw,
            ports,
            _borrow: PhantomData,
        })
    }
}

impl Switch {
    /// Creates an ingress group with the qualifier set `qset` (the SDK widens it as needed) at
    /// group priority `priority`; its id.
    pub fn fp_group_create(&mut self, qset: &[FpQual], priority: i32) -> Result<u32, Error> {
        let bits = qset.iter().fold(0u32, |b, q| b | q.bit());
        let mut gid: c_int = -1;
        check(self.glue().fp_group_create(self.raw_unit(), bits, priority, &mut gid))?;
        count(gid)
    }

    /// Destroys group `gid`.
    pub fn fp_group_destroy(&mut self, gid: u32) -> Result<(), Error> {
        let g = c_value(gid)?;
        check(self.glue().fp_group_destroy(self.raw_unit(), g)).map(|_| ())
    }

    /// Creates and installs an entry of group `gid` (on a failure the glue undoes it all).
    pub fn fp_entry_create(&mut self, gid: u32, rule: &FpRule) -> Result<FpEntry, Error> {
        let g = c_value(gid)?;
        let raw = rule.raw()?;
        let (mut eid, mut stat): (c_int, c_int) = (-1, -1);
        check(
            self.glue()
                .fp_entry_create(self.raw_unit(), g, &raw, &mut eid, &mut stat),
        )?;
        Ok(FpEntry {
            id: count(eid)?,
            stat: if stat < 0 { None } else { Some(count(stat)?) },
        })
    }

    /// [`Switch::fp_entry_create`] with the glue's struct as the caller built it ([`FpRuleRaw`]).
    pub fn fp_entry_create_raw(&mut self, gid: u32, rule: &FpRuleRaw) -> Result<FpEntry, Error> {
        let g = c_value(gid)?;
        let raw = rule.raw()?;
        let (mut eid, mut stat): (c_int, c_int) = (-1, -1);
        check(
            self.glue()
                .fp_entry_create(self.raw_unit(), g, &raw, &mut eid, &mut stat),
        )?;
        Ok(FpEntry {
            id: count(eid)?,
            stat: if stat < 0 { None } else { Some(count(stat)?) },
        })
    }

    /// Removes and destroys an entry and its counter.
    pub fn fp_entry_destroy(&mut self, entry: FpEntry) -> Result<(), Error> {
        let e = c_value(entry.id)?;
        let s = entry.stat.map_or(Ok(-1), c_value)?;
        check(self.glue().fp_entry_destroy(self.raw_unit(), e, s)).map(|_| ())
    }

    /// An entry counter's packets and bytes (synced from the hardware).
    pub fn fp_stat_get(&self, stat: u32) -> Result<(u64, u64), Error> {
        let s = c_value(stat)?;
        let (mut packets, mut bytes) = (0u64, 0u64);
        check(self.glue().fp_stat_get(self.raw_unit(), s, &mut packets, &mut bytes))?;
        Ok((packets, bytes))
    }
}

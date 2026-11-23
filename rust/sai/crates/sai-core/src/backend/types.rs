//! The value types of the typed backend operations ([`super::ops`]): one Rust type per kind of
//! id and per structure of the C++ `ob::Backend` (`sai-openbcm/src/backend.h`), so an id of the wrong
//! kind cannot be passed where another is expected. Fields keep the C++ names and order (the
//! call log prints them in that order, [`super::log`]); a C++ `-1` meaning "none" is an
//! [`Option`], a C++ `int` that is a port or a LAG is a [`Dest`].
use super::{HwPort, Vid};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

pub use crate::value::IpPrefix;

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident;)*) => {
        $(
            $(#[$doc])*
            ///
            /// The backend's own number for the object (decimal in the call log). Made by the
            /// backend that hands it out; the SAI layer only stores and passes it back.
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $name(pub u32);

            impl $name {
                /// The number.
                pub const fn get(self) -> u32 {
                    self.0
                }
            }
        )*
    };
}

ids! {
    /// An SDK trunk (LAG) id (`lag_create`).
    TrunkId;
    /// A KNET netif (`netif_create`).
    NetifId;
    /// A KNET RX filter (`netif_create`, `netif_set_keep_tag`).
    FilterId;
    /// An L3 interface (`l3_intf_create`).
    IntfId;
    /// A my-station entry (`l3_station_add`).
    StationId;
    /// An L3 egress object (`l3_egress_create`, the shared CPU / drop / glean ones).
    EgressId;
    /// An ECMP group (`l3_ecmp_create`).
    EcmpId;
    /// A field group (`fp_group_create`).
    FpGroupId;
    /// A field entry (`fp_entry_create`).
    FpEntryId;
    /// A meter (`policer_create`).
    PolicerId;
    /// A `QoS` map (`qos_map_create`).
    QosMapId;
    /// A WRED profile (`wred_create`).
    WredId;
    /// A mirror session (`mirror_create`).
    MirrorId;
    /// A sampler (`sample_create`).
    SampleId;
    /// A generic-netlink host interface (`genetlink_create`).
    GenlId;
}

/// An SDK VRF id: 0 is the default virtual router.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Vrf(pub u32);

impl Vrf {
    /// The default virtual router's VRF.
    pub const DEFAULT: Self = Self(0);

    /// The number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A MAC address, network order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddr(pub [u8; 6]);

impl core::fmt::Display for MacAddr {
    /// Lower-case hex, two digits, `:` between (the call log's form).
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Where an L2 entry, an L2 flush, an L3 egress object or an FDB event points: a port
/// (the CPU port included) or a LAG. The C++ passes an `int`: the port number, or
/// `lag_dest(tid)` = `0x40000000 | tid` ([`Dest::raw`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dest {
    /// A port.
    Port(HwPort),
    /// A LAG.
    Lag(TrunkId),
}

impl Dest {
    /// `OB_LAG_DEST_BIT` (`sdk_glue.h`).
    pub const LAG_BIT: u32 = 0x4000_0000;

    /// The C++ integer form (what the SDK glue takes and the call log prints).
    pub fn raw(self) -> u32 {
        match self {
            Self::Port(hw) => u32::from(hw.0),
            Self::Lag(tid) => Self::LAG_BIT | tid.0,
        }
    }

    /// The destination of a C++ integer: `None` for a negative one or a port number above
    /// `u16::MAX`.
    pub fn from_raw(raw: i64) -> Option<Self> {
        let raw = u32::try_from(raw).ok()?;
        if raw & Self::LAG_BIT != 0 {
            Some(Self::Lag(TrunkId(raw & !Self::LAG_BIT)))
        } else {
            u16::try_from(raw).ok().map(|hw| Self::Port(HwPort(hw)))
        }
    }
}

/// A clause-45 MDIO device (MMD) number, 1..=31 by construction: device 0 would be a
/// clause-22 access, which this interface cannot express. The same rule as
/// `mdio_script::Dev` (a local type: `sai-core` does not depend on `mdio-script`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MdioDev(u8);

impl MdioDev {
    /// The largest device number.
    pub const MAX: u8 = 31;

    /// `None` unless `1 <= dev <= 31`.
    pub const fn new(dev: u8) -> Option<Self> {
        if dev >= 1 && dev <= Self::MAX {
            Some(Self(dev))
        } else {
            None
        }
    }

    /// The device number.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The address word of a PHY on the switch chip's MDIO master (bus and PHY address as the
/// SDK packs them; `ext_phy.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhyAddr(pub u32);

/// What a link gate says about its port on one link scan pass ([`LinkGate`]). The C++ gate
/// returns an `int`: 0 = down, anything else = no opinion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// The port is down whatever the ASIC says.
    Down,
    /// No opinion: the ASIC's link state stands.
    NoOpinion,
}

/// A port's link gate ([`super::ops::PortOps::port_link_gate`]): asked by the backend on every
/// link scan pass of its port, before the ASIC's own link read.
///
/// **Rules for the closure.** It runs on a backend thread (the SDK's link scan thread, with the
/// SDK's link scan lock held [inferred from the C++]; on the virtual backend: inside the call
/// that changes the link, or [`crate::virt::VirtualBackend::linkscan`]). It must not take the SAI
/// lock, must not call back into the backend, and may only touch its own state behind its own
/// lock (the ext-PHY runtime keeps its per-port verdict there and does its MDIO through its own
/// bus handle). It should return quickly: the whole link scan waits for it.
pub type LinkGate = Arc<dyn Fn() -> GateVerdict + Send + Sync>;

/// FEC mode (`ob::Fec`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Fec {
    /// No FEC.
    #[default]
    None,
    /// Reed-Solomon (clause 91).
    Rs,
    /// Fire code / BASE-R (clause 74).
    Fc,
}

/// Port loopback (`ob::Loopback`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Loopback {
    /// None.
    #[default]
    None,
    /// MAC loopback.
    Mac,
    /// PHY loopback.
    Phy,
}

/// Address learning on a port (`ob::Learn`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Learn {
    /// Learn in hardware.
    #[default]
    Hw,
    /// Do not learn, forward.
    Disable,
    /// Do not learn, drop unknown sources.
    Drop,
    /// Send unknown sources to the CPU.
    Cpu,
}

macro_rules! named_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident = $text:literal,)* }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($(#[$vdoc])* $variant,)*
        }

        impl $name {
            /// Every value, in the C++ order.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The name the profile and the C++ use.
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $text,)* }
            }

            /// The value of a name.
            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.name() == name)
            }
        }
    };
}

named_enum! {
    /// The interface type a port's MAC / `SerDes` runs (`SAI_OPENBCM_PORT_INTERFACE`,
    /// `ob::port_interface_names`; the C++ passes the name as a string).
    PortInterface {
        /// `caui`
        Caui = "caui",
        /// `caui4`
        Caui4 = "caui4",
        /// `caui_c2c`
        CauiC2c = "caui_c2c",
        /// `caui_c2m`
        CauiC2m = "caui_c2m",
        /// `xlaui`
        Xlaui = "xlaui",
        /// `cr4`
        Cr4 = "cr4",
        /// `sr4`
        Sr4 = "sr4",
        /// `lr4`
        Lr4 = "lr4",
        /// `kr4`
        Kr4 = "kr4",
        /// `sfi`
        Sfi = "sfi",
        /// `xfi`
        Xfi = "xfi",
        /// `cr`
        Cr = "cr",
        /// `sr`
        Sr = "sr",
        /// `kr`
        Kr = "kr",
    }
}

impl PortInterface {
    /// Whether the interface type needs a four-lane port.
    pub const fn four_lanes(self) -> bool {
        matches!(
            self,
            Self::Caui
                | Self::Caui4
                | Self::CauiC2c
                | Self::CauiC2m
                | Self::Xlaui
                | Self::Cr4
                | Self::Sr4
                | Self::Lr4
                | Self::Kr4
        )
    }
}

/// The port counters the SAI layer reads through [`super::ops::PortOps::port_stat_get`]
/// (`ob::PortStat`, the same order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PortStat {
    /// `IF_IN_OCTETS`
    InOctets,
    /// `IF_IN_UCAST_PKTS`
    InUcastPkts,
    /// `IF_IN_NON_UCAST_PKTS`
    InNonUcastPkts,
    /// `IF_IN_DISCARDS`
    InDiscards,
    /// `IF_IN_ERRORS`
    InErrors,
    /// `IF_IN_UNKNOWN_PROTOS`
    InUnknownProtos,
    /// `IF_IN_BROADCAST_PKTS`
    InBroadcastPkts,
    /// `IF_IN_MULTICAST_PKTS`
    InMulticastPkts,
    /// `IF_OUT_OCTETS`
    OutOctets,
    /// `IF_OUT_UCAST_PKTS`
    OutUcastPkts,
    /// `IF_OUT_NON_UCAST_PKTS`
    OutNonUcastPkts,
    /// `IF_OUT_DISCARDS`
    OutDiscards,
    /// `IF_OUT_ERRORS`
    OutErrors,
    /// `IF_OUT_BROADCAST_PKTS`
    OutBroadcastPkts,
    /// `IF_OUT_MULTICAST_PKTS`
    OutMulticastPkts,
}

impl PortStat {
    /// Every counter, in the C++ order.
    pub const ALL: [Self; 15] = [
        Self::InOctets,
        Self::InUcastPkts,
        Self::InNonUcastPkts,
        Self::InDiscards,
        Self::InErrors,
        Self::InUnknownProtos,
        Self::InBroadcastPkts,
        Self::InMulticastPkts,
        Self::OutOctets,
        Self::OutUcastPkts,
        Self::OutNonUcastPkts,
        Self::OutDiscards,
        Self::OutErrors,
        Self::OutBroadcastPkts,
        Self::OutMulticastPkts,
    ];

    /// The `SAI_PORT_STAT_*` id of the counter.
    pub const fn sai_id(self) -> sai_sys::sai_stat_id_t {
        use sai_sys as sys;
        match self {
            Self::InOctets => sys::SAI_PORT_STAT_IF_IN_OCTETS,
            Self::InUcastPkts => sys::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
            Self::InNonUcastPkts => sys::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
            Self::InDiscards => sys::SAI_PORT_STAT_IF_IN_DISCARDS,
            Self::InErrors => sys::SAI_PORT_STAT_IF_IN_ERRORS,
            Self::InUnknownProtos => sys::SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS,
            Self::InBroadcastPkts => sys::SAI_PORT_STAT_IF_IN_BROADCAST_PKTS,
            Self::InMulticastPkts => sys::SAI_PORT_STAT_IF_IN_MULTICAST_PKTS,
            Self::OutOctets => sys::SAI_PORT_STAT_IF_OUT_OCTETS,
            Self::OutUcastPkts => sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
            Self::OutNonUcastPkts => sys::SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS,
            Self::OutDiscards => sys::SAI_PORT_STAT_IF_OUT_DISCARDS,
            Self::OutErrors => sys::SAI_PORT_STAT_IF_OUT_ERRORS,
            Self::OutBroadcastPkts => sys::SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS,
            Self::OutMulticastPkts => sys::SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS,
        }
    }

    /// The counter of a `SAI_PORT_STAT_*` id, if it is one of these.
    pub fn from_sai_id(id: sai_sys::sai_stat_id_t) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.sai_id() == id)
    }
}

/// The protocols a backend may know how to steer to the CPU (`ob::TrapKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrapKind {
    /// ARP requests.
    ArpRequest,
    /// ARP replies.
    ArpResponse,
    /// DHCP.
    Dhcp,
    /// `DHCPv6`.
    Dhcpv6,
    /// IGMP.
    Igmp,
    /// MLD.
    Mld,
    /// Neighbor discovery.
    Nd,
    /// LLDP.
    Lldp,
    /// LACP.
    Lacp,
    /// STP BPDUs.
    Stp,
    /// Anything else (no backend control: `NotSupported`).
    Other,
}

/// A LAG member (`ob::LagMember`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LagMember {
    /// The port.
    pub hw: HwPort,
    /// Not in the distribution (hash) set.
    pub egress_disable: bool,
    /// Received as a non-trunk port (not a drop).
    pub ingress_disable: bool,
}

/// What an FDB event says happened (`ob::FdbEvent::learned`). The SAI layer makes a SAI
/// `MOVE` of a learn for an address it already has on another port (`sai_switch.cpp`
/// `deliver_fdb`), as the C++ does: the backend reports only these two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FdbChange {
    /// The chip learned the address (`learned = true`).
    Learned,
    /// The chip aged it out or deleted it (`learned = false`).
    Aged,
}

/// An address the chip learned or aged on its own (`ob::FdbEvent`), reported as
/// [`super::BackendEvent::Fdb`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FdbEvent {
    /// Learned or aged.
    pub change: FdbChange,
    /// The address.
    pub mac: MacAddr,
    /// Its VLAN.
    pub vid: Vid,
    /// Where it was seen (C++ `hw_port`).
    pub hw_port: Dest,
}

/// Free entries of the forwarding tables (`ob::TableUsage`): what the switch's `AVAILABLE_*`
/// attributes answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names, reason = "the C++ names, printed in the call log")]
pub struct TableUsage {
    /// LPM, IPv4 prefixes.
    pub v4_routes_free: u32,
    /// LPM, IPv6 prefixes up to /64.
    pub v6_routes_free: u32,
    /// Host table in IPv4 entries (an IPv6 host takes 2).
    pub hosts_free: u32,
    /// L3 egress objects (one per neighbor).
    pub egress_free: u32,
    /// L2 table.
    pub fdb_free: u32,
    /// ECMP groups.
    pub ecmp_groups_free: u32,
    /// ECMP member table entries.
    pub ecmp_members_free: u32,
}

/// Fixed properties of the chip (`ob::ChipLimits`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChipLimits {
    /// Shared packet buffer (MMU) in bytes.
    pub buffer_bytes: u64,
    /// Lowest field entry priority.
    pub acl_prio_min: u32,
    /// Highest field entry priority (higher wins).
    pub acl_prio_max: u32,
    /// Highest VRF id (0: the default VRF only).
    pub vrf_max: u32,
    /// ECMP groups.
    pub ecmp_groups: u32,
    /// Members per group.
    pub ecmp_paths: u32,
    /// Member entries across all groups.
    pub ecmp_members: u32,
}

/// The ASIC's die temperature monitors, in tenths of a degree C
/// ([`super::ops::Temp::asic_temperatures`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsicTemps {
    /// Current, per monitor.
    pub cur: Vec<i32>,
    /// The peak since the previous read, per monitor.
    pub peak: Vec<i32>,
}

/// What [`super::ops::Hostif::netif_create`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetifCreated {
    /// The netif.
    pub netif_id: NetifId,
    /// Its RX filter.
    pub filter_id: FilterId,
}

/// A field entry's counter ([`super::ops::Field::fp_entry_counter`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FpCounter {
    /// Packets.
    pub packets: u64,
    /// Bytes.
    pub bytes: u64,
}

/// What a route forwards to (C++: `int target, bool ecmp` of `l3_route_add`; the call log prints
/// the two C++ arguments, `target=<n>, ecmp=<bool>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RouteTarget {
    /// An egress object (a next hop, the CPU, drop, glean).
    Egress(EgressId),
    /// An ECMP group.
    Ecmp(EcmpId),
}

/// Field-processor qualifiers (`ob::FpQual`, the same order: the bit of a qualifier is
/// `1 << (q as u32)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FpQual {
    /// Ingress ports.
    InPorts,
    /// Ethertype.
    EtherType,
    /// `sai_acl_ip_type_t`.
    IpType,
    /// IP protocol.
    IpProtocol,
    /// IPv4 source.
    SrcIp,
    /// IPv4 destination.
    DstIp,
    /// IPv6 source.
    SrcIp6,
    /// IPv6 destination.
    DstIp6,
    /// L4 source port.
    L4SrcPort,
    /// L4 destination port.
    L4DstPort,
    /// TCP flags.
    TcpFlags,
    /// ICMP type and code.
    IcmpTypeCode,
    /// TOS (DSCP and ECN).
    Tos,
    /// Destination MAC.
    DstMac,
    /// Source MAC.
    SrcMac,
    /// Outer VLAN.
    OuterVlan,
    /// TTL / hop limit.
    Ttl,
    /// Routed or host hit to the CPU egress.
    DstCpu,
}

impl FpQual {
    /// Every qualifier, in the C++ order.
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
        Self::DstCpu,
    ];

    /// The qualifier's bit in a C++ `present` / qualifier set mask.
    pub const fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// A value and its mask (data and masks in network order, as the C++).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Masked<T> {
    /// The value.
    pub value: T,
    /// The mask.
    pub mask: T,
}

/// What a field entry matches (`ob::FpMatch`). A qualifier is present when its field is
/// `Some` (the C++ `present` bit is [`FpMatch::present`]); IPv4 and IPv6 addresses are
/// separate fields, so a match cannot carry an address of the wrong family.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FpMatch {
    /// `InPorts`: never empty when present.
    pub in_ports: Option<Vec<HwPort>>,
    /// `EtherType`.
    pub ether_type: Option<Masked<u16>>,
    /// `IpType` (`sai_acl_ip_type_t`; the backend maps it).
    pub ip_type: Option<i32>,
    /// `IpProtocol`.
    pub ip_protocol: Option<Masked<u8>>,
    /// `SrcIp`.
    pub src_ip: Option<Masked<Ipv4Addr>>,
    /// `DstIp`.
    pub dst_ip: Option<Masked<Ipv4Addr>>,
    /// `SrcIp6`.
    pub src_ip6: Option<Masked<Ipv6Addr>>,
    /// `DstIp6`.
    pub dst_ip6: Option<Masked<Ipv6Addr>>,
    /// `L4SrcPort`.
    pub l4_src_port: Option<Masked<u16>>,
    /// `L4DstPort`.
    pub l4_dst_port: Option<Masked<u16>>,
    /// `TcpFlags`.
    pub tcp_flags: Option<Masked<u8>>,
    /// `IcmpTypeCode`: `type << 8 | code`.
    pub icmp_type_code: Option<Masked<u16>>,
    /// `Tos`: `DSCP << 2 | ECN`.
    pub tos: Option<Masked<u8>>,
    /// `DstMac`.
    pub dst_mac: Option<Masked<MacAddr>>,
    /// `SrcMac`.
    pub src_mac: Option<Masked<MacAddr>>,
    /// `OuterVlan`.
    pub outer_vlan: Option<Masked<u16>>,
    /// `Ttl`.
    pub ttl: Option<Masked<u8>>,
    /// `DstCpu`: a routed or host hit to the CPU egress.
    pub dst_cpu: bool,
    /// Only with [`FpCap::Ranges`]: an inclusive L4 source port range (not a `present` bit).
    pub l4_src_range: Option<(u16, u16)>,
    /// Only with [`FpCap::Ranges`]: an inclusive L4 destination port range.
    pub l4_dst_range: Option<(u16, u16)>,
    /// Only with [`FpCap::OutPorts`]: the egress ports of an egress table entry.
    pub out_ports: Vec<HwPort>,
}

impl FpMatch {
    /// The qualifiers present, in [`FpQual`] order.
    pub fn qualifiers(&self) -> Vec<FpQual> {
        FpQual::ALL.into_iter().filter(|q| self.has(*q)).collect()
    }

    /// Whether qualifier `q` is present.
    pub const fn has(&self, q: FpQual) -> bool {
        match q {
            FpQual::InPorts => self.in_ports.is_some(),
            FpQual::EtherType => self.ether_type.is_some(),
            FpQual::IpType => self.ip_type.is_some(),
            FpQual::IpProtocol => self.ip_protocol.is_some(),
            FpQual::SrcIp => self.src_ip.is_some(),
            FpQual::DstIp => self.dst_ip.is_some(),
            FpQual::SrcIp6 => self.src_ip6.is_some(),
            FpQual::DstIp6 => self.dst_ip6.is_some(),
            FpQual::L4SrcPort => self.l4_src_port.is_some(),
            FpQual::L4DstPort => self.l4_dst_port.is_some(),
            FpQual::TcpFlags => self.tcp_flags.is_some(),
            FpQual::IcmpTypeCode => self.icmp_type_code.is_some(),
            FpQual::Tos => self.tos.is_some(),
            FpQual::DstMac => self.dst_mac.is_some(),
            FpQual::SrcMac => self.src_mac.is_some(),
            FpQual::OuterVlan => self.outer_vlan.is_some(),
            FpQual::Ttl => self.ttl.is_some(),
            FpQual::DstCpu => self.dst_cpu,
        }
    }

    /// The C++ `present` mask.
    pub fn present(&self) -> u32 {
        self.qualifiers().into_iter().map(FpQual::bit).fold(0, |a, b| a | b)
    }
}

/// Where a field entry redirects to (only with [`FpCap::Redirect`]; C++ `redirect` +
/// `redirect_egress`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Redirect {
    /// A port or LAG.
    Dest(Dest),
    /// An L3 egress object (a next hop).
    Egress(EgressId),
}

/// A field entry: match and actions (`ob::FpRule`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools, reason = "the C++ structure, field for field")]
pub struct FpRule {
    /// Within the group, higher wins.
    pub priority: u32,
    /// The match.
    pub m: FpMatch,
    /// Drop.
    pub drop: bool,
    /// Cancel a drop of a lower-priority group.
    pub drop_cancel: bool,
    /// Copy to the CPU.
    pub copy_cpu: bool,
    /// Count packets and bytes.
    pub counter: bool,
    /// The meter.
    pub policer: Option<PolicerId>,
    /// Drop green packets.
    pub green_drop: bool,
    /// Drop yellow packets.
    pub yellow_drop: bool,
    /// Drop red packets.
    pub red_drop: bool,
    /// The colour drops cancel only the copy to the CPU (`CoPP`: forwarding untouched).
    pub cpu_copy_only: bool,
    /// Mirror sessions for matched packets, ingress (only with `mirror_supported`).
    pub mirror_ingress: Vec<MirrorId>,
    /// Mirror sessions for matched packets, egress.
    pub mirror_egress: Vec<MirrorId>,
    /// Only with [`FpCap::Redirect`].
    pub redirect: Option<Redirect>,
    /// Only with [`FpCap::SetTc`]: the traffic class to set.
    pub set_tc: Option<u8>,
    /// Only with [`FpCap::SetDscp`]: the DSCP to set.
    pub set_dscp: Option<u8>,
}

/// Optional field-processor features (`ob::FpCap`, [`super::ops::Field::fp_supported`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FpCap {
    /// L4 port ranges.
    Ranges,
    /// Redirect.
    Redirect,
    /// Set the traffic class.
    SetTc,
    /// Set the DSCP.
    SetDscp,
    /// Egress tables.
    Egress,
    /// `OUT_PORTS` of egress entries.
    OutPorts,
    /// [`super::ops::Field::fp_entry_enable`].
    EntryEnable,
    /// [`super::ops::Field::fp_entry_update`].
    EntryUpdate,
}

/// What a table needs beyond its qualifier set (`ob::FpGroupExtra`,
/// [`super::ops::Field::fp_group_create_ex`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools, reason = "the C++ structure, field for field")]
pub struct FpGroupExtra {
    /// An egress table.
    pub egress: bool,
    /// Source port ranges.
    pub src_range: bool,
    /// Destination port ranges.
    pub dst_range: bool,
    /// `OUT_PORTS`.
    pub out_ports: bool,
    /// Redirect.
    pub redirect: bool,
    /// Set the traffic class.
    pub set_tc: bool,
    /// Set the DSCP.
    pub set_dscp: bool,
    /// Mirror actions.
    pub mirror: bool,
}

impl FpGroupExtra {
    /// Whether anything is asked for.
    pub const fn any(self) -> bool {
        self.egress
            || self.src_range
            || self.dst_range
            || self.out_ports
            || self.redirect
            || self.set_tc
            || self.set_dscp
            || self.mirror
    }
}

/// An SDK meter (`ob::PolicerCfg`). With `packets`: rates in packets/s and bursts in
/// packets, else bytes/s and bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PolicerCfg {
    /// `trTCM` (RFC 2698), else `srTCM` (two colours when `pbs` is 0).
    pub tr_tcm: bool,
    /// Packet mode.
    pub packets: bool,
    /// Colour blind, else colour aware (packets arrive green).
    pub color_blind: bool,
    /// Committed rate.
    pub cir: u64,
    /// Committed burst.
    pub cbs: u64,
    /// Peak rate.
    pub pir: u64,
    /// Peak / excess burst.
    pub pbs: u64,
}

impl Default for PolicerCfg {
    /// The C++ defaults: `srTCM`, packet mode, colour aware, all zero.
    fn default() -> Self {
        Self {
            tr_tcm: false,
            packets: true,
            color_blind: false,
            cir: 0,
            cbs: 0,
            pir: 0,
            pbs: 0,
        }
    }
}

/// A host-interface trap as SAI configures it (`ob::TrapSpec`,
/// [`super::ops::Hostif::trap_program`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrapSpec {
    /// `sai_hostif_trap_type_t` (C++ `type`).
    pub trap_type: i32,
    /// `sai_packet_action_t`.
    pub action: i32,
    /// `TRAP_PRIORITY`.
    pub priority: u32,
    /// The group's CPU queue.
    pub queue: u32,
    /// The group's `ADMIN_STATE`.
    pub group_up: bool,
    /// The group's policer.
    pub policer: Option<PolicerId>,
    /// The policer's `YELLOW_PACKET_ACTION` is `DROP`.
    pub yellow_drop: bool,
    /// The policer's `RED_PACKET_ACTION` is `DROP`.
    pub red_drop: bool,
}

/// The kinds of `QoS` map (`ob::QosMapKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QosMapKind {
    /// DSCP to traffic class.
    DscpToTc,
    /// 802.1p to traffic class.
    Dot1pToTc,
    /// Traffic class to queue.
    TcToQueue,
    /// Traffic class to priority group.
    TcToPg,
    /// PFC priority to queue.
    PfcPrioToQueue,
    /// PFC priority to priority group.
    PfcPrioToPg,
    /// Traffic class and colour to DSCP.
    TcColorToDscp,
    /// Traffic class and colour to 802.1p.
    TcColorToDot1p,
    /// DSCP to colour.
    DscpToColor,
    /// 802.1p to colour.
    Dot1pToColor,
}

/// One `QoS` map entry (`ob::QosMapEntry`): key -> value; colours are `SAI_PACKET_COLOR_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QosMapEntry {
    /// The key.
    pub key: u32,
    /// The key's colour (the `TC_AND_COLOR` keys).
    pub key_color: u32,
    /// The value.
    pub value: u32,
    /// The value's colour (the `*_TO_COLOR` maps).
    pub value_color: u32,
}

/// A scheduler profile (`ob::SchedCfg`). Rates: bytes/s and bytes, or packets/s and packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SchedCfg {
    /// Strict priority, else DWRR / WRR with `weight`.
    pub strict: bool,
    /// The weight.
    pub weight: u32,
    /// Packet mode.
    pub packets: bool,
    /// Minimum rate (0: none).
    pub min_rate: u64,
    /// Minimum burst.
    pub min_burst: u64,
    /// Maximum rate (0: none).
    pub max_rate: u64,
    /// Maximum burst.
    pub max_burst: u64,
}

impl Default for SchedCfg {
    /// The C++ defaults: DWRR weight 1, no shaping.
    fn default() -> Self {
        Self {
            strict: false,
            weight: 1,
            packets: false,
            min_rate: 0,
            min_burst: 0,
            max_rate: 0,
            max_burst: 0,
        }
    }
}

/// A WRED profile (`ob::WredCfg`): index 0 green, 1 yellow, 2 red; thresholds in bytes,
/// probability in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WredCfg {
    /// Enabled per colour.
    pub enable: [bool; 3],
    /// Minimum threshold per colour.
    pub min: [u32; 3],
    /// Maximum threshold per colour.
    pub max: [u32; 3],
    /// Drop probability per colour.
    pub prob: [u32; 3],
    /// Queue size averaging weight.
    pub weight: u8,
    /// `sai_ecn_mark_mode_t`.
    pub ecn: i32,
}

impl Default for WredCfg {
    /// The C++ defaults: off, probability 100.
    fn default() -> Self {
        Self {
            enable: [false; 3],
            min: [0; 3],
            max: [0; 3],
            prob: [100; 3],
            weight: 0,
            ecn: 0,
        }
    }
}

/// A buffer profile with what its pool says (`ob::BufferCfg`); bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferCfg {
    /// The pool's `TYPE` is ingress.
    pub ingress: bool,
    /// `THRESHOLD_MODE` dynamic (the pool's when the profile has none).
    pub dynamic: bool,
    /// Alpha = `2^dynamic_th`.
    pub dynamic_th: i8,
    /// Static threshold.
    pub static_th: u64,
    /// Reserved size.
    pub reserved: u64,
    /// XOFF threshold.
    pub xoff: u64,
    /// XON threshold.
    pub xon: u64,
    /// XON offset.
    pub xon_offset: u64,
    /// The pool's size.
    pub pool_size: u64,
    /// The pool's shared headroom.
    pub pool_xoff: u64,
}

impl Default for BufferCfg {
    /// The C++ defaults: egress, dynamic, all zero.
    fn default() -> Self {
        Self {
            ingress: false,
            dynamic: true,
            dynamic_th: 0,
            static_th: 0,
            reserved: 0,
            xoff: 0,
            xon: 0,
            xon_offset: 0,
            pool_size: 0,
            pool_xoff: 0,
        }
    }
}

/// A mirror session (`ob::MirrorCfg`): SPAN, or ERSPAN with its encapsulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MirrorCfg {
    /// `ENHANCED_REMOTE` (L3 GRE), else `LOCAL`.
    pub erspan: bool,
    /// The monitor port.
    pub monitor: HwPort,
    /// Truncate to this many bytes (0: whole frames).
    pub truncate: u16,
    /// 1 in this many frames (0 / 1: every frame).
    pub sample_rate: u32,
    /// ERSPAN: TOS.
    pub tos: u8,
    /// ERSPAN: TTL.
    pub ttl: u8,
    /// ERSPAN: source address.
    pub src: IpAddr,
    /// ERSPAN: destination address.
    pub dst: IpAddr,
    /// ERSPAN: source MAC.
    pub smac: MacAddr,
    /// ERSPAN: destination MAC.
    pub dmac: MacAddr,
    /// ERSPAN: GRE protocol type.
    pub gre_type: u16,
    /// ERSPAN: `VLAN_HEADER_VALID`.
    pub vlan: bool,
    /// ERSPAN: VLAN TPID.
    pub vlan_tpid: u16,
    /// ERSPAN: VLAN id.
    pub vlan_id: u16,
    /// ERSPAN: VLAN priority.
    pub vlan_pri: u8,
    /// ERSPAN: VLAN CFI.
    pub vlan_cfi: u8,
}

impl MirrorCfg {
    /// A session to `monitor` with the C++ defaults (SPAN, whole frames, TTL 255, GRE type
    /// `0x88be`, TPID `0x8100`, addresses 0.0.0.0).
    pub const fn new(monitor: HwPort) -> Self {
        Self {
            erspan: false,
            monitor,
            truncate: 0,
            sample_rate: 0,
            tos: 0,
            ttl: 255,
            src: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            dst: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            smac: MacAddr([0; 6]),
            dmac: MacAddr([0; 6]),
            gre_type: 0x88be,
            vlan: false,
            vlan_tpid: 0x8100,
            vlan_id: 0,
            vlan_pri: 0,
            vlan_cfi: 0,
        }
    }
}

/// Storm control kinds (`ob::StormKind`, [`super::ops::Storm::port_storm_set`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StormKind {
    /// Broadcast.
    Broadcast,
    /// Multicast.
    Multicast,
    /// Unknown unicast.
    UnknownUnicast,
}

/// VLAN flood control kinds (`ob::FloodKind`, [`super::ops::Storm::vlan_flood_set`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FloodKind {
    /// Unknown unicast.
    UnknownUnicast,
    /// Unknown multicast.
    UnknownMulticast,
    /// Broadcast.
    Broadcast,
}

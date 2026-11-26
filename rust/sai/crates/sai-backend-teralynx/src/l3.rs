//! L3 (`tl_l3.{h,cpp}`): VRFs (L3VNIs), interfaces, my-station, next hops, host and route entries,
//! ECMP groups, RIF counters.
//!
//! VRF n = `IFCS_HANDLE_L3VNI(n + 1)`, created on first use (TTL check failures and IPv4 options
//! copied to the CPU, options dropped: isai's L3VNI attributes). An L3 interface sits on the
//! frontend's VLAN (a port RIF on its internal VLAN): intf `TYPE` = SVI on the L2VNI,
//! `FORWARDING_INSTANCE` = the VRF's L3VNI, `TRANSPORT_MAC`, IPv4 / IPv6 unicast on, MTU. An egress
//! object is an IFCS nexthop (`INTF`, `DEST_MAC`, `LOCAL_DESTINATION` = the port's sysport or the
//! LAG's handle), changed in place; the shared CPU / drop / glean egress objects have no nexthop and
//! turn into route policies. Hosts are /32 (/128) route entries. ECMP groups: one member list, a
//! weight is a repeated nexthop.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ifcs::{Attrs, Handle, RouteKey, sys};
use sai_core::backend::HwPort;
use sai_core::backend::types::{Dest, EcmpId, EgressId, IntfId, IpPrefix, RouteTarget, StationId, Vrf};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, ECMP_PATHS, NODE, VRF_MAX};
use crate::hostif::HostifArea;
use crate::l2::L2Area;
use crate::lag::LagArea;

/// What the table usage needs from the L3 area (`L3Area::Counts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// IPv4 prefixes.
    pub v4_prefixes: u32,
    /// IPv6 prefixes.
    pub v6_prefixes: u32,
    /// IPv4 hosts.
    pub v4_hosts: u32,
    /// IPv6 hosts.
    pub v6_hosts: u32,
    /// ECMP members in all groups.
    pub ecmp_members: u32,
}

/// An egress object's kind (`Egress::Kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An IFCS nexthop.
    Nh,
    /// The shared CPU egress (TRAP / ip2me routes).
    Cpu,
    /// The shared drop egress.
    Drop,
    /// The shared glean egress (connected subnets, unresolved next hops).
    Glean,
}

/// An L3 interface (`Intf`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intf {
    /// Its IFCS handle.
    pub handle: Handle,
    /// `TRANSPORT_MAC`.
    pub mac: [u8; 6],
    /// The frontend's VLAN.
    pub vid: u16,
    /// `MTU`.
    pub mtu: u32,
    /// The VRF.
    pub vrf: u32,
}

/// An egress object (`Egress`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Egress {
    /// Its kind.
    pub kind: Kind,
    /// The nexthop (`Kind::Nh` only).
    pub handle: Handle,
    /// Its interface (`Kind::Nh`).
    pub intf: Option<IntfId>,
    /// Its port or LAG (`Kind::Nh`).
    pub hw: Option<Dest>,
    /// `DEST_MAC`.
    pub dmac: [u8; 6],
    /// The VLAN the frontend gave.
    pub vid: u16,
}

impl Egress {
    const fn shared(kind: Kind) -> Self {
        Self {
            kind,
            handle: Handle::NULL,
            intf: None,
            hw: None,
            dmac: [0; 6],
            vid: 0,
        }
    }
}

/// An ECMP group (`Ecmp`): the members in order, a repeated nexthop = weight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ecmp {
    /// Its IFCS handle.
    pub handle: Handle,
    /// The nexthops.
    pub members: Vec<Handle>,
}

/// Whether `p` is a host prefix (every mask bit of its family set).
pub fn is_host(p: &IpPrefix) -> bool {
    match p.mask() {
        IpAddr::V4(m) => m == Ipv4Addr::BROADCAST,
        IpAddr::V6(m) => m.to_bits() == u128::MAX,
    }
}

/// The host prefix of an address (`host_prefix`).
pub fn host_prefix(ip: IpAddr) -> Option<IpPrefix> {
    let mask = match ip {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::BROADCAST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from_bits(u128::MAX)),
    };
    IpPrefix::new(ip, mask)
}

/// The `ifcs_ip_addr_t` bytes of an address (`route_key`): IPv4 as a host-order value in the
/// union's `ipv4` (`be32`), IPv6 as its 16 bytes.
pub fn ip_bytes(a: IpAddr) -> [u8; 16] {
    let mut out = [0u8; 16];
    match a {
        IpAddr::V4(v4) => {
            if let Some(dst) = out.get_mut(..4) {
                dst.copy_from_slice(&v4.to_bits().to_ne_bytes());
            }
        }
        IpAddr::V6(v6) => out = v6.octets(),
    }
    out
}

/// The IFCS interface counters of one RIF counter (`intf_stats`): ingress the counters of the
/// node's ingress mode (`pre_enable`: 4 = `IPV4_IPV6_ANY` v4 + v6, 0.12.41 takes it, chip
/// 2026-11-16; 3 = `UC_ONLY` unicast; else the total), egress the IPv4 + IPv6 ones (IFCS has no
/// combined one). `None`: no IFCS counter (the error counters).
pub fn intf_counters(id: ssys::sai_stat_id_t, in_mode: u32) -> Option<Vec<i32>> {
    let c = |ids: &[u32]| ids.iter().map(|&i| i.cast_signed()).collect::<Vec<i32>>();
    Some(match id {
        ssys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS => match in_mode {
            4 => c(&[
                sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV4_PKTS,
                sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV6_PKTS,
            ]),
            3 => c(&[sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_UCAST_PKTS]),
            _ => c(&[sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_PKTS]),
        },
        ssys::SAI_ROUTER_INTERFACE_STAT_IN_OCTETS => match in_mode {
            4 => c(&[
                sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV4_BYTES,
                sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV6_BYTES,
            ]),
            3 => c(&[sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_UCAST_BYTES]),
            _ => c(&[sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_BYTES]),
        },
        ssys::SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS => c(&[
            sys::IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV4_PKTS,
            sys::IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV6_PKTS,
        ]),
        ssys::SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS => c(&[
            sys::IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV4_BYTES,
            sys::IFCS_INTF_STATS_ID_L3INTF_EGRESS_IPV6_BYTES,
        ]),
        _ => return None,
    })
}

/// The L3 area's state.
#[derive(Debug)]
pub struct L3Area {
    /// The ingress L3INTF stats mode the node accepted (`in_mode_`, 1 until one is).
    pub in_mode: u32,
    /// VRF -> L3VNI (`l3vnis_`).
    pub l3vnis: BTreeMap<u32, Handle>,
    /// Interfaces.
    pub intfs: BTreeMap<u32, Intf>,
    /// Egress objects.
    pub egress: BTreeMap<u32, Egress>,
    /// ECMP groups.
    pub ecmps: BTreeMap<u32, Ecmp>,
    /// (VRF, address) -> egress object.
    pub hosts: BTreeMap<(u32, IpAddr), EgressId>,
    /// (VRF, prefix) -> target.
    pub routes: BTreeMap<(u32, IpPrefix), RouteTarget>,
    next_intf: u32,
    next_station: u32,
    next_egress: u32,
    next_ecmp: u32,
    cpu_egress: Option<u32>,
    drop_egress: Option<u32>,
    glean_egress: Option<u32>,
    /// Node `TTL_CHECK_PROFILE_1_THRESHOLD` = 0 done (`ttl_profile_set_`).
    pub ttl_profile_set: bool,
}

impl Default for L3Area {
    fn default() -> Self {
        Self {
            in_mode: 1,
            l3vnis: BTreeMap::new(),
            intfs: BTreeMap::new(),
            egress: BTreeMap::new(),
            ecmps: BTreeMap::new(),
            hosts: BTreeMap::new(),
            routes: BTreeMap::new(),
            next_intf: 1,
            next_station: 1,
            next_egress: 100_001,
            next_ecmp: 200_001,
            cpu_egress: None,
            drop_egress: None,
            glean_egress: None,
            ttl_profile_set: false,
        }
    }
}

fn next(n: &mut u32) -> u32 {
    let id = *n;
    *n = n.wrapping_add(1);
    id
}

impl L3Area {
    /// `L3Area::pre_enable`: per direction a flex counter block (`CONFIG`: mode 0, offset 0,
    /// `SAI_TERALYNX_L3INTF_STATS_COUNTERS`; skipped when 0), then the first mode of
    /// `SAI_TERALYNX_L3INTF_STATS_MODES` the node accepts. Refusals are warnings.
    pub fn pre_enable(&mut self, core: &Core) {
        if !core.settings.l3intf_stats {
            return;
        }
        let n = core.settings.l3intf_counters;
        let cfg_id = [
            sys::IFCS_NODE_ATTR_INGRESS_L3INTF_STATS_CONFIG,
            sys::IFCS_NODE_ATTR_EGRESS_L3INTF_STATS_CONFIG,
        ];
        let mode_id = [
            sys::IFCS_NODE_ATTR_INGRESS_L3INTF_STATS_MODE,
            sys::IFCS_NODE_ATTR_EGRESS_L3INTF_STATS_MODE,
        ];
        for (d, (cfg, mode)) in cfg_id.iter().zip(mode_id).enumerate() {
            let dir = if d == 0 { "ingress" } else { "egress" };
            if n != 0 {
                let mut c = Attrs::new().flex_stats(*cfg, 0, 0, n, 0x07);
                let rc = core.lib.ifcs_node_attr_set(NODE, &mut c);
                if !rc.is_success() {
                    log::warn(
                        ssys::SAI_API_ROUTER_INTERFACE,
                        format_args!("node L3INTF stats config (attr {cfg}, {n} counters): {}", rc.name()),
                    );
                }
            }
            let mut ok = false;
            for &m in &core.settings.l3intf_modes {
                let mut x = Attrs::new().u32(mode, m);
                if core.lib.ifcs_node_attr_set(NODE, &mut x).is_success() {
                    log::notice(
                        ssys::SAI_API_ROUTER_INTERFACE,
                        format_args!("node {dir} L3INTF stats mode {m}"),
                    );
                    if d == 0 {
                        self.in_mode = m;
                    }
                    ok = true;
                    break;
                }
            }
            if !ok {
                log::warn(
                    ssys::SAI_API_ROUTER_INTERFACE,
                    format_args!("node {dir} L3INTF stats: no mode accepted (RIF counters stay 0)"),
                );
            }
        }
    }

    /// `L3Area::counts` (our own counts for the switch's table usage: no trusted IFCS call). A
    /// host-length route lives in the host hash like a neighbor's host entry (chip 2026-11-16:
    /// 95,043 /32 routes fitted with the 24,576-entry TCAM full).
    pub fn counts(&self) -> Counts {
        let mut n = Counts::default();
        for (_, p) in self.routes.keys() {
            let v6 = p.addr().is_ipv6();
            let slot = match (is_host(p), v6) {
                (true, true) => &mut n.v6_hosts,
                (true, false) => &mut n.v4_hosts,
                (false, true) => &mut n.v6_prefixes,
                (false, false) => &mut n.v4_prefixes,
            };
            *slot = slot.wrapping_add(1);
        }
        for (_, ip) in self.hosts.keys() {
            let slot = if ip.is_ipv6() { &mut n.v6_hosts } else { &mut n.v4_hosts };
            *slot = slot.wrapping_add(1);
        }
        for e in self.ecmps.values() {
            n.ecmp_members = n
                .ecmp_members
                .wrapping_add(u32::try_from(e.members.len()).unwrap_or(u32::MAX));
        }
        n
    }

    /// `L3Area::intf_stat_supported`: the four RIF counters the C++ reads.
    pub const fn intf_stat_supported(id: ssys::sai_stat_id_t) -> bool {
        matches!(
            id,
            ssys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS
                | ssys::SAI_ROUTER_INTERFACE_STAT_IN_OCTETS
                | ssys::SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS
                | ssys::SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS
        )
    }

    /// `L3Area::l3vni`: the VRF's L3VNI, created on first use with isai's attributes: routed
    /// packets whose TTL check fails are copied to the CPU (the system trap `FWD_L3_TTL_CHECK_FAIL`
    /// then decides queue / whether: `TTL_ERROR`; without it TTL 1 is dropped silently, chip
    /// 2026-11-16), IPv4 with options copied and dropped (`FWD_IPV4_OPTIONS_DETECTED`). An L3VNI
    /// that exists is taken. The first one also sets node TTL check profile 1 to threshold 0:
    /// IP2ME routes use it, so TTL 1 packets to the switch itself reach the CPU (isai).
    fn l3vni(&mut self, core: &Core, vrf: u32) -> Result<Handle> {
        if vrf > VRF_MAX {
            return Err(Error::InvalidParameter);
        }
        if let Some(&h) = self.l3vnis.get(&vrf) {
            return Ok(h);
        }
        let mut v = Handle::l3vni(vrf + 1);
        let mut a = Attrs::new()
            .data(sys::IFCS_L3VNI_ATTR_IPV4_UNICAST_TTL_CHECK_FAIL_CTC, true)
            .data(sys::IFCS_L3VNI_ATTR_IPV6_UNICAST_TTL_CHECK_FAIL_CTC, true)
            .data(sys::IFCS_L3VNI_ATTR_IPV4_OPTIONS_CTC, true)
            .data(sys::IFCS_L3VNI_ATTR_IPV4_OPTIONS_DROP, true);
        let rc = core.lib.ifcs_l3vni_create(NODE, &mut v, &mut a);
        if !rc.is_success() && rc.reason() != sys::IFCS_EXIST {
            return Err(core.fail(rc.reason(), "l3vni_create"));
        }
        if !self.ttl_profile_set {
            let mut t = Attrs::new().u32(sys::IFCS_NODE_ATTR_TTL_CHECK_PROFILE_1_THRESHOLD, 0);
            if core
                .call(
                    core.lib.ifcs_node_attr_set(NODE, &mut t),
                    "node TTL_CHECK_PROFILE_1_THRESHOLD",
                )
                .is_ok()
            {
                self.ttl_profile_set = true;
            }
        }
        self.l3vnis.insert(vrf, v);
        Ok(v)
    }

    /// `L3Area::intf_create`: an SVI on the VLAN's L2VNI in the VRF's L3VNI (the vendor sample uses
    /// `L3PORT` on a sysport; inferred).
    pub fn intf_create(
        &mut self,
        core: &Core,
        l2: &L2Area,
        mac: [u8; 6],
        vid: u16,
        mtu: u32,
        vrf: Vrf,
    ) -> Result<IntfId> {
        let l2vni = l2.vlan_l2vni(vid).ok_or(Error::InvalidParameter)?;
        let vni = self.l3vni(core, vrf.get())?;
        let mut a = Attrs::new()
            .u32(sys::IFCS_INTF_ATTR_TYPE, sys::IFCS_INTF_TYPE_SVI)
            .handle(sys::IFCS_INTF_ATTR_FORWARDING_INSTANCE, vni)
            .handle(sys::IFCS_INTF_ATTR_TRANSPORT_INSTANCE, l2vni)
            .mac(sys::IFCS_INTF_ATTR_TRANSPORT_MAC, mac)
            .data(sys::IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE, true)
            .data(sys::IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE, true)
            .u32(sys::IFCS_INTF_ATTR_MTU, mtu);
        let mut h = Handle::NULL;
        core.call(core.lib.ifcs_intf_create(NODE, &mut h, &mut a), "intf_create")?;
        let id = next(&mut self.next_intf);
        self.intfs.insert(
            id,
            Intf {
                handle: h,
                mac,
                vid,
                mtu,
                vrf: vrf.get(),
            },
        );
        Ok(IntfId(id))
    }

    /// `L3Area::intf_update`: MAC and MTU in place; another VLAN or VRF is `NOT_IMPLEMENTED`.
    pub fn intf_update(&mut self, core: &Core, id: IntfId, mac: [u8; 6], vid: u16, mtu: u32, vrf: Vrf) -> Result<()> {
        let i = self.intfs.get_mut(&id.0).ok_or(Error::ItemNotFound)?;
        if vid != i.vid || vrf.get() != i.vrf {
            return Err(core.not_impl("moving an L3 interface to another VLAN/VRF"));
        }
        if mac != i.mac {
            let mut a = Attrs::new().mac(sys::IFCS_INTF_ATTR_TRANSPORT_MAC, mac);
            core.call(
                core.lib.ifcs_intf_attr_set(NODE, i.handle.0, &mut a),
                "intf transport_mac",
            )?;
            i.mac = mac;
        }
        if mtu != i.mtu {
            let mut a = Attrs::new().u32(sys::IFCS_INTF_ATTR_MTU, mtu);
            core.call(core.lib.ifcs_intf_attr_set(NODE, i.handle.0, &mut a), "intf mtu")?;
            i.mtu = mtu;
        }
        Ok(())
    }

    /// `L3Area::intf_destroy`.
    pub fn intf_destroy(&mut self, core: &Core, id: IntfId) -> Result<()> {
        let i = self.intfs.get(&id.0).ok_or(Error::ItemNotFound)?;
        core.call(core.lib.ifcs_intf_delete(NODE, i.handle), "intf_delete")?;
        self.intfs.remove(&id.0);
        Ok(())
    }

    /// `L3Area::station_add`: IFCS has no my-station table in the calls we use (an interface
    /// terminates its own `TRANSPORT_MAC`; inferred): bookkeeping only, said once.
    pub fn station_add(&mut self, core: &Core, mac: [u8; 6]) -> StationId {
        let id = next(&mut self.next_station);
        let m = sai_core::backend::types::MacAddr(mac);
        core.log_once(
            "station",
            format_args!("my-station {m}: no IFCS object (interfaces terminate their own MAC)"),
        );
        StationId(id)
    }

    /// `dest_of`: a port's sysport (unknown: `INVALID_PORT_NUMBER`) or a LAG's handle (unknown:
    /// `INVALID_PARAMETER`).
    fn dest_of(core: &Core, lag: &LagArea, hw: Dest) -> Result<Handle> {
        match hw {
            Dest::Lag(_) => lag.dest_handle(core, hw).ok_or(Error::InvalidParameter),
            Dest::Port(HwPort(p)) => core.sysport.get(&u32::from(p)).copied().ok_or(Error::InvalidPortNumber),
        }
    }

    /// `L3Area::egress_create`: an IFCS nexthop `INTF`, `DEST_MAC`, `LOCAL_DESTINATION` = the port's
    /// sysport (`l3snake_cel.py`) or the LAG's handle (a port RIF on a LAG, as isai).
    #[allow(clippy::too_many_arguments, reason = "the C++ operation's arguments")]
    pub fn egress_create(
        &mut self,
        core: &Core,
        lag: &LagArea,
        intf: IntfId,
        dmac: [u8; 6],
        hw: Dest,
        vid: u16,
    ) -> Result<EgressId> {
        let i = *self.intfs.get(&intf.0).ok_or(Error::InvalidParameter)?;
        let dest = Self::dest_of(core, lag, hw)?;
        let mut a = Attrs::new()
            .handle(sys::IFCS_NEXTHOP_ATTR_INTF, i.handle)
            .mac(sys::IFCS_NEXTHOP_ATTR_DEST_MAC, dmac)
            .handle(sys::IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, dest);
        let mut h = Handle::NULL;
        core.call(core.lib.ifcs_nexthop_create(NODE, &mut h, &mut a), "nexthop_create")?;
        let id = next(&mut self.next_egress);
        self.egress.insert(
            id,
            Egress {
                kind: Kind::Nh,
                handle: h,
                intf: Some(intf),
                hw: Some(hw),
                dmac,
                vid,
            },
        );
        Ok(EgressId(id))
    }

    /// `L3Area::egress_update`: an in-place `nexthop_attr_set` of what changed (inferred), so routes
    /// keep their handle.
    #[allow(clippy::too_many_arguments, reason = "the C++ operation's arguments")]
    pub fn egress_update(
        &mut self,
        core: &Core,
        lag: &LagArea,
        id: EgressId,
        intf: IntfId,
        dmac: [u8; 6],
        hw: Dest,
        vid: u16,
    ) -> Result<()> {
        let Some(e) = self.egress.get_mut(&id.0).filter(|e| e.kind == Kind::Nh) else {
            return Err(Error::InvalidParameter);
        };
        let i = *self.intfs.get(&intf.0).ok_or(Error::InvalidParameter)?;
        let dest = Self::dest_of(core, lag, hw)?;
        if Some(intf) != e.intf {
            let mut a = Attrs::new().handle(sys::IFCS_NEXTHOP_ATTR_INTF, i.handle);
            core.call(core.lib.ifcs_nexthop_attr_set(NODE, e.handle.0, &mut a), "nexthop intf")?;
            e.intf = Some(intf);
        }
        if dmac != e.dmac {
            let mut a = Attrs::new().mac(sys::IFCS_NEXTHOP_ATTR_DEST_MAC, dmac);
            core.call(
                core.lib.ifcs_nexthop_attr_set(NODE, e.handle.0, &mut a),
                "nexthop dest_mac",
            )?;
            e.dmac = dmac;
        }
        if Some(hw) != e.hw {
            let mut a = Attrs::new().handle(sys::IFCS_NEXTHOP_ATTR_LOCAL_DESTINATION, dest);
            core.call(
                core.lib.ifcs_nexthop_attr_set(NODE, e.handle.0, &mut a),
                "nexthop local_destination",
            )?;
            e.hw = Some(hw);
        }
        e.vid = vid;
        Ok(())
    }

    /// `L3Area::egress_destroy`: a shared one is `INVALID_PARAMETER`.
    pub fn egress_destroy(&mut self, core: &Core, id: EgressId) -> Result<()> {
        let e = self.egress.get(&id.0).ok_or(Error::ItemNotFound)?;
        if e.kind != Kind::Nh {
            return Err(Error::InvalidParameter);
        }
        core.call(core.lib.ifcs_nexthop_delete(NODE, e.handle), "nexthop_delete")?;
        self.egress.remove(&id.0);
        Ok(())
    }

    fn shared(&mut self, kind: Kind) -> EgressId {
        let slot = match kind {
            Kind::Cpu => &mut self.cpu_egress,
            Kind::Drop => &mut self.drop_egress,
            Kind::Glean | Kind::Nh => &mut self.glean_egress,
        };
        if let Some(id) = *slot {
            return EgressId(id);
        }
        let id = next(&mut self.next_egress);
        *slot = Some(id);
        self.egress.insert(id, Egress::shared(kind));
        EgressId(id)
    }

    /// `L3Area::egress_cpu`: the shared CPU egress (made on first use).
    pub fn egress_cpu(&mut self) -> EgressId {
        self.shared(Kind::Cpu)
    }

    /// `L3Area::egress_drop`.
    pub fn egress_drop(&mut self) -> EgressId {
        self.shared(Kind::Drop)
    }

    /// `L3Area::egress_glean`: copies to the hostif area's glean trap (`NEIGHBOR_MISS`'s queue /
    /// policer), as isai.
    pub fn egress_glean(&mut self) -> EgressId {
        self.shared(Kind::Glean)
    }

    /// `L3Area::nexthop_handle`: the IFCS nexthop of an egress object (the ACL area's redirect);
    /// `None` for a shared or unknown one.
    pub fn nexthop_handle(&self, id: EgressId) -> Option<Handle> {
        self.egress
            .get(&id.0)
            .filter(|e| e.kind == Kind::Nh && !e.handle.is_null())
            .map(|e| e.handle)
    }

    /// `L3Area::intf_admin_set`: RIF `ADMIN_V4` / `V6_STATE` = intf `IPV4` / `IPV6_UNICAST_ENABLE`.
    pub fn intf_admin_set(&self, core: &Core, id: IntfId, v4: bool, v6: bool) -> Result<()> {
        let i = self.intfs.get(&id.0).ok_or(Error::ItemNotFound)?;
        let mut a = Attrs::new()
            .data(sys::IFCS_INTF_ATTR_IPV4_UNICAST_ENABLE, v4)
            .data(sys::IFCS_INTF_ATTR_IPV6_UNICAST_ENABLE, v6);
        core.call(
            core.lib.ifcs_intf_attr_set(NODE, i.handle.0, &mut a),
            "intf ipv4/ipv6 unicast enable",
        )
    }

    /// `L3Area::intf_stats`: one `ifcs_intf_stats_get` of the counters [`intf_counters`] needs
    /// (each once, in first-use order), summed per RIF counter; `clear`: then
    /// `ifcs_intf_stats_clear` of the same. An unknown interface is `INVALID_OBJECT_ID`, a counter
    /// without an IFCS one `NOT_SUPPORTED` (before any call).
    pub fn intf_stats(&self, core: &Core, id: IntfId, ids: &[ssys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>> {
        let i = self.intfs.get(&id.0).ok_or(Error::InvalidObjectId)?;
        let mut want = Vec::with_capacity(ids.len());
        let mut q: Vec<i32> = Vec::new();
        for &s in ids {
            let w = intf_counters(s, self.in_mode).ok_or(Error::NotSupported)?;
            for &c in &w {
                if !q.contains(&c) {
                    q.push(c);
                }
            }
            want.push(w);
        }
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut v = vec![0u64; q.len()];
        let (rc, _) = core.lib.ifcs_intf_stats_get(NODE, i.handle, &mut q, &mut v);
        core.call(rc, "intf_stats_get")?;
        let out = want
            .iter()
            .map(|w| {
                w.iter()
                    .filter_map(|c| q.iter().position(|x| x == c).and_then(|p| v.get(p)))
                    .fold(0u64, |a, b| a.wrapping_add(*b))
            })
            .collect();
        if clear {
            core.call(
                core.lib.ifcs_intf_stats_clear(NODE, i.handle.0, &mut q),
                "intf_stats_clear",
            )?;
        }
        Ok(out)
    }

    /// `L3Area::route_key`: as `scripts/l3snake_cel.py` builds it (prefix accessors, then the key's).
    fn route_key(core: &Core, vni: Handle, p: &IpPrefix) -> RouteKey {
        let lib = &core.lib;
        let family = if p.addr().is_ipv6() {
            sys::IFCS_IP_ADDR_FAMILY_IPV6
        } else {
            sys::IFCS_IP_ADDR_FAMILY_IPV4
        };
        let mut pf = ifcs::IpPrefix::zeroed();
        let _ = lib.ifcs_ip_prefix_t_init(&mut pf);
        let _ = lib.ifcs_ip_prefix_t_addr_family_set(&mut pf, family.cast_signed());
        let _ = lib.ifcs_ip_prefix_t_addr_set(&mut pf, ip_bytes(p.addr()));
        let _ = lib.ifcs_ip_prefix_t_mask_set(&mut pf, ip_bytes(p.mask()));
        let mut k = RouteKey::zeroed();
        let _ = lib.ifcs_route_entry_key_t_init(&mut k);
        let _ =
            lib.ifcs_route_entry_key_t_key_type_set(&mut k, sys::IFCS_ROUTE_ENTRY_KEY_TYPE_IP_DEST_L3VNI.cast_signed());
        let member = k.ip_dest_l3vni();
        let _ = lib.ifcs_route_entry_key_t_ip_dest_l3vni_set(&mut k, member);
        let _ = lib.ifcs_route_entry_key_ip_dest_l3vni_t_l3vni_set(&mut k, vni);
        let _ = lib.ifcs_route_entry_key_ip_dest_l3vni_t_ip_dest_set(&mut k, &mut pf);
        k
    }

    /// `L3Area::route_attrs`: per target kind: nexthop / ECMP: `NEXTHOP` + `FWD_POLICY` forward
    /// (`l3snake_cel.py`; an ECMP handle as `NEXTHOP` is inferred); drop: `FWD_POLICY` drop, no
    /// `NEXTHOP`; CPU (TRAP / ip2me) and glean: `FWD_POLICY` drop + `CTC_POLICY` copy-to-cpu with the
    /// hostif area's IP2ME or glean trap (drop + copy as isai, oracle l3-basic); a host prefix to
    /// the CPU (IP2ME) also takes TTL check profile 1 ([`L3Area::l3vni`]).
    fn route_attrs(
        &self,
        core: &Core,
        hostif: &mut HostifArea,
        target: RouteTarget,
        host: bool,
    ) -> Result<(Attrs, Kind)> {
        let forward = |h: Handle| {
            Attrs::new().handle(sys::IFCS_ROUTE_ENTRY_ATTR_NEXTHOP, h).fwd(
                sys::IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY,
                sys::IFCS_FWD_ACTION_FORWARD.cast_signed(),
            )
        };
        let e = match target {
            RouteTarget::Ecmp(id) => {
                let g = self.ecmps.get(&id.0).ok_or(Error::InvalidParameter)?;
                return Ok((forward(g.handle), Kind::Nh));
            }
            RouteTarget::Egress(id) => *self.egress.get(&id.0).ok_or(Error::InvalidParameter)?,
        };
        let drop = || {
            Attrs::new().fwd(
                sys::IFCS_ROUTE_ENTRY_ATTR_FWD_POLICY,
                sys::IFCS_FWD_ACTION_DROP.cast_signed(),
            )
        };
        let a = match e.kind {
            Kind::Nh => forward(e.handle),
            Kind::Drop => drop(),
            Kind::Cpu | Kind::Glean => {
                let trap = if e.kind == Kind::Glean {
                    hostif.glean_trap(core)?
                } else {
                    hostif.ip2me_trap(core)?
                };
                let mut a = drop().ctc(sys::IFCS_ROUTE_ENTRY_ATTR_CTC_POLICY, true, trap);
                if host && self.ttl_profile_set && e.kind == Kind::Cpu {
                    a = a.u32(sys::IFCS_ROUTE_ENTRY_ATTR_TTL_CHECK_PROFILE, 1); // IP2ME
                }
                a
            }
        };
        Ok((a, e.kind))
    }

    /// `L3Area::route_program`: create, or (`old`) replace: the same kind (nexthop / ECMP to nexthop
    /// / ECMP) is an `attr_set` of `NEXTHOP` + `FWD_POLICY`; a change of kind deletes and re-creates
    /// the entry (no attribute "unset" in IFCS we know).
    fn route_program(
        &mut self,
        core: &Core,
        hostif: &mut HostifArea,
        vrf: u32,
        p: &IpPrefix,
        target: RouteTarget,
        old: Option<RouteTarget>,
    ) -> Result<()> {
        let vni = self.l3vni(core, vrf)?;
        let (mut a, kind) = self.route_attrs(core, hostif, target, is_host(p))?;
        let mut k = Self::route_key(core, vni, p);
        if let Some(old) = old {
            let okind = match old {
                RouteTarget::Ecmp(_) => Some(Kind::Nh),
                RouteTarget::Egress(id) => self.egress.get(&id.0).map(|e| e.kind),
            };
            if okind == Some(kind) && kind == Kind::Nh {
                return core.call(
                    core.lib.ifcs_route_entry_attr_set(NODE, &mut k, &mut a),
                    "route_entry_attr_set",
                );
            }
            core.call(
                core.lib.ifcs_route_entry_delete(NODE, &mut k),
                "route_entry_delete (replace)",
            )?;
        }
        core.call(
            core.lib.ifcs_route_entry_create(NODE, &mut k, &mut a),
            "route_entry_create",
        )
    }

    /// `L3Area::route_remove`.
    fn route_remove(&mut self, core: &Core, vrf: u32, p: &IpPrefix) -> Result<()> {
        let vni = self.l3vni(core, vrf)?;
        let mut k = Self::route_key(core, vni, p);
        core.call(core.lib.ifcs_route_entry_delete(NODE, &mut k), "route_entry_delete")
    }

    /// `L3Area::host_add`: a /32 (/128) route entry (IFCS picks the host table itself: route entry
    /// has a `HOST_RESOURCE_TYPE` attribute; inferred).
    pub fn host_add(
        &mut self,
        core: &Core,
        hostif: &mut HostifArea,
        vrf: Vrf,
        ip: IpAddr,
        egress: EgressId,
    ) -> Result<()> {
        let key = (vrf.get(), ip);
        if self.hosts.contains_key(&key) {
            return Err(Error::ItemAlreadyExists);
        }
        let p = host_prefix(ip).ok_or(Error::InvalidParameter)?;
        self.route_program(core, hostif, vrf.get(), &p, RouteTarget::Egress(egress), None)?;
        self.hosts.insert(key, egress);
        Ok(())
    }

    /// `L3Area::host_delete`.
    pub fn host_delete(&mut self, core: &Core, vrf: Vrf, ip: IpAddr) -> Result<()> {
        let key = (vrf.get(), ip);
        if !self.hosts.contains_key(&key) {
            return Err(Error::ItemNotFound);
        }
        let p = host_prefix(ip).ok_or(Error::InvalidParameter)?;
        self.route_remove(core, vrf.get(), &p)?;
        self.hosts.remove(&key);
        Ok(())
    }

    /// `L3Area::route_add` (`replace`: it exists and is changed).
    pub fn route_add(
        &mut self,
        core: &Core,
        hostif: &mut HostifArea,
        vrf: Vrf,
        p: IpPrefix,
        target: RouteTarget,
        replace: bool,
    ) -> Result<()> {
        let key = (vrf.get(), p);
        let old = self.routes.get(&key).copied();
        match (old, replace) {
            (Some(_), false) => return Err(Error::ItemAlreadyExists),
            (None, true) => return Err(Error::ItemNotFound),
            _ => {}
        }
        self.route_program(core, hostif, vrf.get(), &p, target, old)?;
        self.routes.insert(key, target);
        Ok(())
    }

    /// `L3Area::route_delete`.
    pub fn route_delete(&mut self, core: &Core, vrf: Vrf, p: IpPrefix) -> Result<()> {
        let key = (vrf.get(), p);
        if !self.routes.contains_key(&key) {
            return Err(Error::ItemNotFound);
        }
        self.route_remove(core, vrf.get(), &p)?;
        self.routes.remove(&key);
        Ok(())
    }

    /// `L3Area::ecmp_members`: the nexthops of the egress objects (1..=`ECMP_PATHS`, next hops
    /// only).
    fn ecmp_members(&self, egress: &[EgressId]) -> Result<Vec<Handle>> {
        if egress.is_empty() || egress.len() > ECMP_PATHS as usize {
            return Err(Error::InvalidParameter);
        }
        egress
            .iter()
            .map(|e| {
                self.egress
                    .get(&e.0)
                    .filter(|x| x.kind == Kind::Nh)
                    .map(|x| x.handle)
                    .ok_or(Error::InvalidParameter)
            })
            .collect()
    }

    /// `L3Area::ecmp_create`: `ifcs_ecmp_create` without attributes (`cmds/ecmp.py` with no options),
    /// then the members in one `ifcs_ecmp_member_add`; a member's weight is its nexthop repeated
    /// (the frontend repeats the egress object). All as isai does (oracle l3-ecmpw);
    /// `IFCS_ECMP_MEMBER_ATTR_WEIGHT` is accepted but has no effect (chip 2026-11-16: weight 3:1
    /// forwarded 1:1).
    pub fn ecmp_create(&mut self, core: &Core, egress: &[EgressId]) -> Result<EcmpId> {
        let want = self.ecmp_members(egress)?;
        let mut h = Handle::NULL;
        core.call(
            core.lib.ifcs_ecmp_create(NODE, &mut h, &mut Attrs::none()),
            "ecmp_create",
        )?;
        let mut m = want.clone();
        if let Err(e) = core.call(
            core.lib.ifcs_ecmp_member_add(NODE, h, &mut m, &mut Attrs::null()),
            "ecmp_member_add",
        ) {
            let _ = core.lib.ifcs_ecmp_delete(NODE, h);
            return Err(e);
        }
        let id = next(&mut self.next_ecmp);
        self.ecmps.insert(
            id,
            Ecmp {
                handle: h,
                members: want,
            },
        );
        Ok(EcmpId(id))
    }

    /// `L3Area::ecmp_update`: one `ifcs_ecmp_member_replace` of the whole list (nothing when it is
    /// the same).
    pub fn ecmp_update(&mut self, core: &Core, id: EcmpId, egress: &[EgressId]) -> Result<()> {
        if !self.ecmps.contains_key(&id.0) {
            return Err(Error::ItemNotFound);
        }
        let want = self.ecmp_members(egress)?;
        let g = self.ecmps.get_mut(&id.0).ok_or(Error::ItemNotFound)?;
        if want == g.members {
            return Ok(());
        }
        let mut m = want.clone();
        core.call(
            core.lib
                .ifcs_ecmp_member_replace(NODE, g.handle, &mut m, &mut Attrs::null()),
            "ecmp_member_replace",
        )?;
        g.members = want;
        Ok(())
    }

    /// `L3Area::ecmp_destroy`: its members out, then `ifcs_ecmp_delete`.
    pub fn ecmp_destroy(&mut self, core: &Core, id: EcmpId) -> Result<()> {
        let g = self.ecmps.get_mut(&id.0).ok_or(Error::ItemNotFound)?;
        if !g.members.is_empty() {
            let mut m = g.members.clone();
            core.call(
                core.lib.ifcs_ecmp_member_remove(NODE, g.handle, &mut m),
                "ecmp_member_remove",
            )?;
            g.members.clear();
        }
        core.call(core.lib.ifcs_ecmp_delete(NODE, g.handle), "ecmp_delete")?;
        self.ecmps.remove(&id.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pfx(a: &str, m: &str) -> IpPrefix {
        IpPrefix::new(a.parse().unwrap(), m.parse().unwrap()).unwrap()
    }

    #[test]
    fn host_prefixes_and_key_bytes_as_tl_l3() {
        assert!(is_host(&pfx("10.0.0.1", "255.255.255.255")));
        assert!(!is_host(&pfx("10.0.0.0", "255.255.255.0")));
        assert!(is_host(&pfx("fc00::2", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")));
        assert!(!is_host(&pfx("::", "::")));
        let h = host_prefix("10.0.1.2".parse().unwrap()).unwrap();
        assert!(is_host(&h));
        // IPv4: be32 of the address, stored as a host-order u32 (the mock prints 0a000102)
        let b = ip_bytes("10.0.1.2".parse().unwrap());
        assert_eq!(u32::from_ne_bytes(b[..4].try_into().unwrap()), 0x0a00_0102);
        assert_eq!(b[4..], [0; 12]);
        let b6 = ip_bytes("fc00:1::2".parse().unwrap());
        assert_eq!((b6[0], b6[1], b6[3], b6[15]), (0xfc, 0, 1, 2));
    }

    #[test]
    fn rif_counters_follow_the_ingress_mode() {
        let i = |x: u32| x.cast_signed();
        assert_eq!(
            intf_counters(ssys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS, 4),
            Some(vec![
                i(sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV4_PKTS),
                i(sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_IPV6_PKTS)
            ])
        );
        assert_eq!(
            intf_counters(ssys::SAI_ROUTER_INTERFACE_STAT_IN_OCTETS, 3),
            Some(vec![i(sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_UCAST_BYTES)])
        );
        assert_eq!(
            intf_counters(ssys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS, 1),
            Some(vec![i(sys::IFCS_INTF_STATS_ID_L3INTF_INGRESS_PKTS)])
        );
        assert_eq!(
            intf_counters(ssys::SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS, 1).map(|v| v.len()),
            Some(2)
        );
        assert_eq!(intf_counters(ssys::SAI_ROUTER_INTERFACE_STAT_IN_ERROR_PACKETS, 4), None);
        for id in [
            ssys::SAI_ROUTER_INTERFACE_STAT_IN_PACKETS,
            ssys::SAI_ROUTER_INTERFACE_STAT_IN_OCTETS,
            ssys::SAI_ROUTER_INTERFACE_STAT_OUT_PACKETS,
            ssys::SAI_ROUTER_INTERFACE_STAT_OUT_OCTETS,
        ] {
            assert!(L3Area::intf_stat_supported(id) && intf_counters(id, 4).is_some());
        }
    }

    #[test]
    fn shared_egress_objects_and_counts() {
        let mut l3 = L3Area::default();
        let cpu = l3.egress_cpu();
        assert_eq!(cpu, EgressId(100_001));
        assert_eq!(l3.egress_cpu(), cpu);
        let drop = l3.egress_drop();
        let glean = l3.egress_glean();
        assert_eq!((drop, glean), (EgressId(100_002), EgressId(100_003)));
        assert_eq!(l3.nexthop_handle(cpu), None);
        assert!(l3.ecmp_members(&[cpu]).is_err() && l3.ecmp_members(&[]).is_err());
        l3.routes
            .insert((0, pfx("10.0.0.0", "255.0.0.0")), RouteTarget::Egress(drop));
        l3.routes
            .insert((0, pfx("10.0.0.1", "255.255.255.255")), RouteTarget::Egress(cpu));
        l3.routes
            .insert((1, pfx("fc00::", "ffff::")), RouteTarget::Egress(drop));
        l3.hosts.insert((0, "fc00::1".parse().unwrap()), cpu);
        l3.ecmps.insert(
            1,
            Ecmp {
                handle: Handle(5),
                members: vec![Handle(1), Handle(2), Handle(2)],
            },
        );
        assert_eq!(
            l3.counts(),
            Counts {
                v4_prefixes: 1,
                v6_prefixes: 1,
                v4_hosts: 1,
                v6_hosts: 1,
                ecmp_members: 3
            }
        );
    }
}

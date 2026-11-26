//! Host interfaces (`tl_hostif.{h,cpp}`), line by line: one Linux netdev per port (`ifcs_netdev` or the
//! SDK's own, adopted), protocol traps (`trap_program`: the trap -> `CoPP` rule table, IGMP / MLD /
//! `NEIGHBOR_MISS`), the system traps as isai sets them, the shared IP2ME and glean traps of TRAP / glean
//! routes, the CPU queue counters and the diag shell's `copp` report.
//!
//! Netdevs: `create` mode (`SAI_TERALYNX_NETDEV=create`): `ifcs_netdev_create` with `INTF_NAME`,
//! `DEVPORTNUM`, `MACADDR`, `VLAN_TAG`; ipd then makes the Linux netdev. `adopt` (the default): the init yaml
//! has netdev `auto_create` (vendor `-k` mode); the SDK's `inno_0_<sysport>` netdev is renamed and
//! given the MAC, as `knet.py` does (the interface ioctls: `ifcs::netdev`). Carrier: libifcs drives
//! the netdev link state from its link scan; there is no IFCS call for it, so `netif_carrier_set`
//! only records the wish.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ifcs::netdev::{IFF_UP, NetdevSocket, strerror};
use ifcs::{Attrs, Handle, sys};
use sai_core::backend::types::{ChipLimits, FilterId, MacAddr, NetifCreated, NetifId, TrapKind, TrapSpec};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::acl::{AclArea, CoppActions, CoppMatch, CoppRule, line256};
use crate::core::{Core, NODE};
use crate::keys::NetdevMode;
use crate::sflow::SflowArea;

/// A netdev (`Netif`).
#[derive(Clone, Debug)]
struct Netif {
    name: String,
    /// The SDK's name of an adopted one.
    orig: String,
    adopted: bool,
    handle: Handle,
    keep_tag: bool,
    /// What `OPER_STATUS` asked (recorded only).
    carrier_wish: bool,
}

/// A programmed trap (`Trap`).
#[derive(Clone, Copy, Debug)]
struct Trap {
    spec: TrapSpec,
    /// Hostif trap (a user trap, the shared IP2ME / glean one, or a system trap).
    handle: Handle,
    /// Ours to delete.
    user: bool,
    /// The `CoPP` install.
    copp: Option<u32>,
}

/// The host interface area's state.
#[derive(Debug)]
pub struct HostifArea {
    /// `SAI_TERALYNX_NETDEV`.
    pub netdev: NetdevMode,
    /// The system traps were set (`sys_traps_done_`).
    pub sys_traps_done: bool,
    /// The shared IP2ME hostif trap (`trap_`).
    pub ip2me: Handle,
    /// The glean hostif trap (`glean_`).
    pub glean: Handle,
    /// By SAI trap type.
    traps: BTreeMap<i32, Trap>,
    netifs: BTreeMap<u32, Netif>,
    next_netif: u32,
}

impl Default for HostifArea {
    fn default() -> Self {
        Self {
            netdev: NetdevMode::Adopt,
            sys_traps_done: false,
            ip2me: Handle::NULL,
            glean: Handle::NULL,
            traps: BTreeMap::new(),
            netifs: BTreeMap::new(),
            next_netif: 1,
        }
    }
}

/// `tag_action`.
const fn tag_action(keep_tag: bool) -> u32 {
    if keep_tag {
        sys::IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_ORIGINAL
    } else {
        sys::IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_STRIP
    }
}

/// `rename_netdev`: down if up, renamed, the MAC set (a failed MAC is a warning only).
fn rename_netdev(from: &str, to: &str, mac: Option<MacAddr>) -> Result<()> {
    let Ok(s) = NetdevSocket::open() else {
        return Err(Error::Failure);
    };
    let flags = match s.flags(from) {
        Ok(f) => f,
        Err(e) => {
            log::error(
                ssys::SAI_API_HOSTIF,
                format_args!(
                    "netdev {from} not found ({}); SAI_TERALYNX_NETDEV=adopt needs the init yaml's netdev auto_create",
                    strerror(&e)
                ),
            );
            return Err(Error::ItemNotFound);
        }
    };
    if flags & IFF_UP != 0 {
        let _ = s.set_flags(from, flags & !IFF_UP);
    }
    if let Err(e) = s.rename(from, to) {
        log::error(
            ssys::SAI_API_HOSTIF,
            format_args!("rename {from} -> {to}: {}", strerror(&e)),
        );
        return Err(Error::Failure);
    }
    if let Some(mac) = mac {
        if let Err(e) = s.set_hwaddr(to, mac.0) {
            log::warn(ssys::SAI_API_HOSTIF, format_args!("{to}: set MAC: {}", strerror(&e)));
        }
    }
    Ok(())
}

/// What a SAI trap type is on this chip (`TrapDef`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrapDef {
    /// The `CoPP` rules.
    pub rules: Vec<CoppRule>,
    /// System trap reason (`ifcs_hostif_trap_e`), 0: a user trap.
    pub sys: u32,
    /// The trap is the shared IP2ME hostif trap of TRAP routes.
    pub ip2me: bool,
    /// The trap is the glean (neighbor miss) hostif trap.
    pub glean: bool,
    /// Offset into the ACL priority range (lower wins).
    pub prio: u32,
}

fn m_bytes(kind: u32, v: &[u8]) -> CoppMatch {
    CoppMatch {
        kind,
        value: v.to_vec(),
        ..CoppMatch::default()
    }
}
fn m_et(et: u16) -> CoppMatch {
    m_bytes(sys::IFCS_ACL_MATCH_TYPE_ETHER_TYPE, &et.to_be_bytes())
}
fn m_dmac(mac: [u8; 6]) -> CoppMatch {
    m_bytes(sys::IFCS_ACL_MATCH_TYPE_DESTINATION_MAC_ADDRESS, &mac)
}
fn m_proto(p: u8) -> CoppMatch {
    m_bytes(sys::IFCS_ACL_MATCH_TYPE_L4_PROTOCOL, &[p])
}
fn m_port(dst: bool, p: u16) -> CoppMatch {
    let kind = if dst {
        sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT
    } else {
        sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT
    };
    m_bytes(kind, &p.to_be_bytes())
}
fn m_byte1(t: u8) -> CoppMatch {
    m_bytes(sys::IFCS_ACL_MATCH_TYPE_L4_BYTE1, &[t])
}
fn m_trap(h: Handle) -> CoppMatch {
    CoppMatch {
        kind: sys::IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP,
        object: h,
        ..CoppMatch::default()
    }
}

/// `trap_def`: the mapping of SAI trap types to `CoPP` rules (what the CPU should get), in the shape
/// isai 6.2.0 programs them on this chip (oracle traces): L2 protocols by DMAC / ether type, IP
/// protocols by ether type + L4, the routed-to-me ones (BGP, SSH, SNMP, BFD) as "IP2ME trap +
/// protocol", IP2ME itself as "hostif trap = ip2me", TTL errors as the system trap
/// `FWD_L3_TTL_CHECK_FAIL` (queue + enable + an entry on it for the policer), MTU errors as the system
/// trap `L3MTU_CHECK_FAIL` (queue + enable, no entry: not policed, as isai). Priorities: lower wins
/// (I, isai's values). `None`: no mapping.
#[allow(clippy::too_many_lines, reason = "the C++ switch, case for case")]
pub fn trap_def(trap_type: i32, ip2me: Handle, glean: Handle) -> Option<TrapDef> {
    let mut d = TrapDef {
        prio: 300,
        ..TrapDef::default()
    };
    let l4 = |d: &mut TrapDef, et: u16, proto: u8, both: bool, port: u16, me: bool| {
        for dst in if both { &[true, false][..] } else { &[true][..] } {
            let mut r = vec![m_et(et), m_proto(proto), m_port(*dst, port)];
            if me {
                r.push(m_trap(ip2me));
            }
            d.rules.push(r);
        }
    };
    match trap_type.cast_unsigned() {
        ssys::SAI_HOSTIF_TRAP_TYPE_LLDP => {
            for last in [0x0e, 0x03, 0x00] {
                d.rules
                    .push(vec![m_dmac([0x01, 0x80, 0xc2, 0x00, 0x00, last]), m_et(0x88cc)]);
            }
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_LACP => d
            .rules
            .push(vec![m_dmac([0x01, 0x80, 0xc2, 0x00, 0x00, 0x02]), m_et(0x8809)]),
        ssys::SAI_HOSTIF_TRAP_TYPE_EAPOL => d.rules.push(vec![m_et(0x888e)]),
        ssys::SAI_HOSTIF_TRAP_TYPE_STP => {
            d.rules.push(vec![m_dmac([0x01, 0x80, 0xc2, 0x00, 0x00, 0x00])]);
            d.prio = 310;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_PVRST => d.rules.push(vec![m_dmac([0x01, 0x00, 0x0c, 0xcc, 0xcc, 0xcd])]),
        ssys::SAI_HOSTIF_TRAP_TYPE_UDLD => d.rules.push(vec![m_dmac([0x01, 0x00, 0x0c, 0xcc, 0xcc, 0xcc])]),
        ssys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST => d.rules.push(vec![m_dmac([0xff; 6]), m_et(0x0806)]),
        ssys::SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE => {
            d.rules.push(vec![m_et(0x0806)]);
            d.prio = 310; // after the request
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_DHCP | ssys::SAI_HOSTIF_TRAP_TYPE_DHCP_L2 => {
            for p in [67, 68] {
                d.rules.push(vec![m_et(0x0800), m_proto(17), m_port(true, p)]);
            }
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_DHCPV6 | ssys::SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2 => {
            for p in [546, 547] {
                d.rules.push(vec![m_et(0x86dd), m_proto(17), m_port(true, p)]);
            }
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY => {
            for t in 133..=137 {
                d.rules.push(vec![m_et(0x86dd), m_proto(58), m_byte1(t)]);
            }
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_BGP => {
            l4(&mut d, 0x0800, 6, true, 179, true);
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_BGPV6 => {
            l4(&mut d, 0x86dd, 6, true, 179, true);
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_SSH => {
            l4(&mut d, 0x0800, 6, true, 22, true);
            l4(&mut d, 0x86dd, 6, true, 22, true);
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_SNMP => {
            l4(&mut d, 0x0800, 17, false, 161, true);
            l4(&mut d, 0x86dd, 17, false, 161, true);
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_BFD => {
            for p in [3784, 4784] {
                l4(&mut d, 0x0800, 17, false, p, true);
            }
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_BFDV6 => {
            for p in [3784, 4784] {
                l4(&mut d, 0x86dd, 17, false, p, true);
            }
            d.prio = 200;
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_IP2ME => {
            d.ip2me = true;
            d.rules.push(vec![m_trap(ip2me)]);
            d.prio = 400;
        }
        // isai: a hostif trap of its own (the glean routes copy with it) and an ACE on it
        ssys::SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS => {
            d.glean = true;
            d.rules.push(vec![m_trap(glean)]);
            d.prio = 400;
        }
        // IGMP / MLD: ether type + protocol + the message type in L4 byte 1 (isai's ACEs)
        t @ (ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY
        | ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE
        | ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT
        | ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT
        | ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT) => {
            let m = match t {
                ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY => 0x11,
                ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE => 0x17,
                ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT => 0x12,
                ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT => 0x16,
                _ => 0x22,
            };
            d.rules.push(vec![m_et(0x0800), m_proto(2), m_byte1(m)]);
        }
        t @ (ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2
        | ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT
        | ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE
        | ssys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT) => {
            let m = match t {
                ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2 => 130,
                ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT => 131,
                ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE => 132,
                _ => 143,
            };
            d.rules.push(vec![m_et(0x86dd), m_proto(58), m_byte1(m)]);
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR => {
            d.sys = sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL;
            d.rules.push(vec![m_trap(Handle::hostif_trap(d.sys))]);
        }
        ssys::SAI_HOSTIF_TRAP_TYPE_L3_MTU_ERROR => d.sys = sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL,
        _ => return None,
    }
    Some(d)
}

/// `to_cpu`: TRAP, COPY and LOG send frames to the CPU.
fn to_cpu(a: i32) -> bool {
    matches!(
        a.cast_unsigned(),
        ssys::SAI_PACKET_ACTION_TRAP | ssys::SAI_PACKET_ACTION_COPY | ssys::SAI_PACKET_ACTION_LOG
    )
}

/// The C++ `policer` int of a trap (-1: none), for the report.
fn policer_num(p: Option<sai_core::backend::types::PolicerId>) -> i64 {
    p.map_or(-1, |p| i64::from(p.0))
}

/// The `CoPP` entry priority offset of a trap: SAI `TRAP_PRIORITY` higher wins, IFCS lower wins (I).
pub const fn prio_offset(def_prio: u32, priority: u32) -> u32 {
    def_prio * 10 + 9_u32.saturating_sub(priority)
}

impl HostifArea {
    /// `HostifArea::configure` (at init).
    pub const fn configure(&mut self, netdev: NetdevMode) {
        self.netdev = netdev;
    }

    /// `netif_create`: a netdev for front devport `hw` (created, or the SDK's adopted); the system traps
    /// first (once).
    pub fn netif_create(
        &mut self,
        core: &Core,
        initialized: bool,
        ifname: &str,
        hw: u32,
        mac: MacAddr,
        keep_tag: bool,
    ) -> Result<NetifCreated> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if ifname.is_empty() || ifname.len() > 15 {
            return Err(Error::InvalidParameter);
        }
        self.system_traps_init(core, initialized);
        if self.netifs.values().any(|n| n.name == ifname) {
            return Err(Error::ItemAlreadyExists);
        }
        let mut n = Netif {
            name: ifname.to_owned(),
            orig: String::new(),
            adopted: false,
            handle: Handle::NULL,
            keep_tag,
            carrier_wish: false,
        };
        if self.netdev == NetdevMode::Adopt {
            n.adopted = true;
            let sp = core.sysport.get(&hw).copied().unwrap_or(Handle::NULL);
            n.orig = format!("inno_0_{:04}", sp.value());
            rename_netdev(&n.orig, ifname, Some(mac))?;
            if keep_tag {
                log::warn(
                    ssys::SAI_API_HOSTIF,
                    format_args!("{ifname}: adopted netdev keeps the SDK's VLAN tag action"),
                );
            }
        } else {
            let mut a = Attrs::new()
                .string(sys::IFCS_NETDEV_ATTR_INTF_NAME, ifname)
                .devport(sys::IFCS_NETDEV_ATTR_DEVPORTNUM, hw)
                .mac(sys::IFCS_NETDEV_ATTR_MACADDR, mac.0)
                .u32(sys::IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag));
            let mut h = Handle::NULL;
            core.call(core.lib.ifcs_netdev_create(NODE, &mut h, &mut a), "netdev_create")?;
            n.handle = h;
        }
        let id = self.next_netif;
        self.next_netif += 1;
        let how = if n.adopted { "adopted" } else { "created" };
        self.netifs.insert(id, n);
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!("netdev {ifname} on devport {hw} ({how})"),
        );
        Ok(NetifCreated {
            netif_id: NetifId(id),
            filter_id: FilterId(id),
        })
    }

    /// `netif_destroy`: deleted or given back (forgotten either way).
    pub fn netif_destroy(&mut self, core: &Core, netif_id: NetifId) -> Result<()> {
        let Some(n) = self.netifs.remove(&netif_id.0) else {
            return Err(Error::ItemNotFound);
        };
        Self::netif_teardown(core, &n)
    }

    /// `netif_carrier_set`: no IFCS call (libifcs drives the carrier); the wish is recorded.
    pub fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()> {
        match self.netifs.values_mut().find(|n| n.name == ifname) {
            Some(n) => {
                n.carrier_wish = up;
                Ok(())
            }
            None => Err(Error::ItemNotFound),
        }
    }

    /// `netif_set_keep_tag`: netdev `VLAN_TAG`; `NOT_IMPLEMENTED` for an adopted one. The filter is the
    /// netif's id.
    pub fn netif_set_keep_tag(&mut self, core: &Core, netif_id: NetifId, keep_tag: bool) -> Result<FilterId> {
        let Some(n) = self.netifs.get_mut(&netif_id.0) else {
            return Err(Error::ItemNotFound);
        };
        let filter = FilterId(netif_id.0);
        if n.keep_tag == keep_tag {
            return Ok(filter);
        }
        if n.adopted {
            return Err(core.not_impl("VLAN tag action of an adopted netdev"));
        }
        let mut a = Attrs::new().u32(sys::IFCS_NETDEV_ATTR_VLAN_TAG, tag_action(keep_tag));
        core.call(
            core.lib.ifcs_netdev_attr_set(NODE, n.handle.0, &mut a),
            "netdev vlan_tag",
        )?;
        n.keep_tag = keep_tag;
        Ok(filter)
    }

    /// `trap_set`: IFCS hostif traps are drop / miss reasons, not protocol matches: no control
    /// (`NOT_SUPPORTED`, said once per kind; `trap_program` programs the traps).
    pub fn trap_set(core: &Core, kind: TrapKind) -> Result<()> {
        let name = match kind {
            TrapKind::ArpRequest => "arp-request",
            TrapKind::ArpResponse => "arp-response",
            TrapKind::Dhcp => "dhcp",
            TrapKind::Dhcpv6 => "dhcpv6",
            TrapKind::Igmp => "igmp",
            TrapKind::Mld => "mld",
            TrapKind::Nd => "nd",
            TrapKind::Lldp => "lldp",
            TrapKind::Lacp => "lacp",
            TrapKind::Stp => "stp",
            TrapKind::Other => "other",
        };
        let w = format!("trap {name}");
        core.log_once(
            &w,
            format_args!("{w}: no IFCS control; left as the SDK has it (NOT_SUPPORTED)"),
        );
        Err(Error::NotSupported)
    }

    /// `trap_handle_for`: the trap's hostif trap (the glean one, a system trap, the IP2ME one, or a
    /// user trap on the group's queue, ours to delete).
    fn trap_handle_for(&mut self, core: &Core, t: &mut Trap, sys_reason: u32, ip2me: bool) -> Result<()> {
        if !t.handle.is_null() {
            return Ok(());
        }
        if t.spec.trap_type.cast_unsigned() == ssys::SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS {
            t.handle = self.glean_trap(core)?;
            return Ok(());
        }
        if sys_reason != 0 {
            t.handle = Handle::hostif_trap(sys_reason);
            return Ok(());
        }
        if ip2me {
            t.handle = self.ip2me_trap(core)?;
            return Ok(());
        }
        let mut q = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, t.spec.queue);
        core.call(
            core.lib.ifcs_hostif_trap_create(NODE, &mut t.handle, &mut q),
            "hostif_trap_create",
        )?;
        t.user = true;
        Ok(())
    }

    /// `trap_program` (`Backend::trap_program`): the hostif trap with the group's queue (for system
    /// traps whether they fire at all), the `CoPP` entries (match, copy, drop, the group's policer),
    /// re-installed on any change. `NOT_SUPPORTED` for trap types without a mapping (the frontend keeps
    /// them unprogrammed); SAMPLEPACKET is the `sFlow` area's.
    #[allow(clippy::too_many_arguments, reason = "the areas the C++ reaches through its Core")]
    pub fn trap_program(
        &mut self,
        core: &Core,
        initialized: bool,
        acl: &mut AclArea,
        sflow: &mut SflowArea,
        limits: &dyn Fn() -> ChipLimits,
        s: &TrapSpec,
        remove: bool,
    ) -> Result<()> {
        self.system_traps_init(core, initialized);
        // sFlow samples: the MIRROR_TO_CPU system trap + the samples' queue meter (tl_sflow)
        if s.trap_type.cast_unsigned() == ssys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET {
            return sflow.trap_program(core, acl, s, remove);
        }
        let ip2me = self.ip2me_trap(core)?;
        let mut glean = Handle::NULL;
        if s.trap_type.cast_unsigned() == ssys::SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS {
            glean = self.glean_trap(core)?;
        }
        let Some(d) = trap_def(s.trap_type, ip2me, glean) else {
            core.log_once(
                &format!("trap-type:{}", s.trap_type),
                format_args!("trap type {}: no IFCS mapping (kept, not programmed)", s.trap_type),
            );
            return Err(Error::NotSupported);
        };
        if remove {
            let Some(t) = self.traps.remove(&s.trap_type) else {
                return Ok(());
            };
            if let Some(id) = t.copp {
                let _ = acl.copp_remove(core, id);
            }
            if t.user {
                let _ = core.call(core.lib.ifcs_hostif_trap_delete(NODE, t.handle), "hostif_trap_delete");
            } else if d.sys != 0 {
                let mut e = Attrs::new().data(sys::IFCS_HOSTIF_TRAP_ATTR_ENABLE, false);
                let _ = core.call(
                    core.lib.ifcs_hostif_trap_attr_set(NODE, t.handle.0, &mut e),
                    "hostif_trap disable",
                );
            } else if d.ip2me || d.glean {
                // TRAP routes keep it, on queue 0
                let mut q = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
                let _ = core.call(
                    core.lib.ifcs_hostif_trap_attr_set(NODE, t.handle.0, &mut q),
                    "hostif_trap queue (ip2me / glean)",
                );
            }
            return Ok(());
        }
        if self.traps.get(&s.trap_type).is_some_and(|t| t.spec == *s) {
            return Ok(());
        }
        let fresh = !self.traps.contains_key(&s.trap_type);
        let mut t = self.traps.remove(&s.trap_type).unwrap_or(Trap {
            spec: *s,
            handle: Handle::NULL,
            user: false,
            copp: None,
        });
        let old = t.spec;
        t.spec = *s;
        if let Err(e) = self.trap_handle_for(core, &mut t, d.sys, d.ip2me) {
            if !fresh {
                self.traps.insert(s.trap_type, t);
            }
            return Err(e);
        }
        let st = Self::trap_entries(core, acl, limits, &mut t, &d, fresh, &old, s);
        self.traps.insert(s.trap_type, t);
        st
    }

    /// The rest of `trap_program`: the hostif trap's queue (and enable), the entries re-installed.
    #[allow(
        clippy::too_many_arguments,
        reason = "trap_program's state, split for the borrow of the trap"
    )]
    fn trap_entries(
        core: &Core,
        acl: &mut AclArea,
        limits: &dyn Fn() -> ChipLimits,
        t: &mut Trap,
        d: &TrapDef,
        fresh: bool,
        old: &TrapSpec,
        s: &TrapSpec,
    ) -> Result<()> {
        let cpu = to_cpu(s.action) && s.group_up;
        // the hostif trap: queue (and for system traps, whether they fire at all)
        if !t.user || !fresh {
            let mut a = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, s.queue);
            if d.sys != 0 {
                a = a.data(sys::IFCS_HOSTIF_TRAP_ATTR_ENABLE, cpu);
            }
            if fresh || old.queue != s.queue || d.sys != 0 {
                core.call(
                    core.lib.ifcs_hostif_trap_attr_set(NODE, t.handle.0, &mut a),
                    "hostif_trap_attr_set",
                )?;
            }
        }
        // entries: re-installed on any change
        if let Some(id) = t.copp.take() {
            let _ = acl.copp_remove(core, id);
        }
        if d.rules.is_empty() {
            return Ok(());
        }
        let act = if cpu {
            CoppActions {
                trap: t.handle,
                drop: s.action.cast_unsigned() == ssys::SAI_PACKET_ACTION_TRAP,
                policer: s.policer,
                yellow_drop: s.yellow_drop,
                red_drop: s.red_drop,
            }
        } else if s.action.cast_unsigned() == ssys::SAI_PACKET_ACTION_DROP {
            CoppActions {
                drop: true,
                ..CoppActions::default()
            }
        } else {
            return Ok(()); // FORWARD (or a disabled group): no entry, normal forwarding
        };
        // SAI TRAP_PRIORITY: higher wins; IFCS: lower wins (I)
        let pr = AclArea::copp_prio(&limits(), prio_offset(d.prio, s.priority));
        match acl.copp_install(core, &d.rules, pr, &act) {
            Ok(id) => {
                t.copp = Some(id);
                Ok(())
            }
            Err(e) => {
                log::error(
                    ssys::SAI_API_HOSTIF,
                    format_args!("trap type {}: CoPP entries failed ({})", s.trap_type, e.status()),
                );
                Err(e)
            }
        }
    }

    /// `trap_report` (the diag shell's `copp`): traps, queues, `CoPP` counters, the system traps that
    /// delivered packets.
    pub fn trap_report(&self, core: &Core, acl: &AclArea) -> String {
        let mut out = String::new();
        let mut put = |s: String| out.push_str(line256(&s));
        put(format!(
            "CoPP: {} traps, {} inno blocks\n",
            self.traps.len(),
            acl.copp_ibs()
        ));
        let rx_id = [i32::try_from(sys::IFCS_HOSTIF_TRAP_STATS_ID_RX_PACKET_COUNT).unwrap_or(0)];
        for (ty, t) in &self.traps {
            let c = t.copp.map_or([0u64; 4], |id| acl.copp_counters(core, id).0);
            let mut ids = rx_id;
            let mut rx = [0u64];
            let (rc, _) = core.lib.ifcs_hostif_trap_stats_get(NODE, t.handle, &mut ids, &mut rx);
            put(format!(
                "trap {ty}: action {} queue {} policer {} hostif 0x{:x} rx {}{} | acl pkts {} green {} yellow {} red {}\n",
                t.spec.action,
                t.spec.queue,
                policer_num(t.spec.policer),
                t.handle.0,
                if rc.is_success() { "" } else { "?" },
                rx[0],
                c[0],
                c[1],
                c[2],
                c[3]
            ));
        }
        // CPU queues: what each one delivered (which queue a trap's frames use).
        for q in 0..8 {
            let Ok((pk, by, dr)) = cpu_queue_stats(core, q) else {
                continue;
            };
            put(format!("cpu queue {q}: tx {pk} bytes {by} drops {dr}\n"));
        }
        // System (drop / exception) traps that delivered packets: which reason sends what to the
        // CPU (handles IFCS_HANDLE(HOSTIF, TRAP, 1..63)).
        for r in 1..64 {
            let h = Handle::hostif_trap(r);
            let mut ids = rx_id;
            let mut rx = [0u64];
            let (rc, _) = core.lib.ifcs_hostif_trap_stats_get(NODE, h, &mut ids, &mut rx);
            if !rc.is_success() || rx[0] == 0 {
                continue;
            }
            put(format!("system trap {r} (0x{:x}): rx {}\n", h.0, rx[0]));
        }
        out
    }

    /// `HostifArea::teardown_all` (shutdown): every netdev deleted / given back, the traps forgotten
    /// (their IFCS objects go with `ifcs_deinit`).
    pub fn teardown_all(&mut self, core: &Core) {
        for n in self.netifs.values() {
            let _ = Self::netif_teardown(core, n);
        }
        self.netifs.clear();
        self.traps.clear();
        self.ip2me = Handle::NULL;
        self.glean = Handle::NULL;
        self.sys_traps_done = false;
    }

    /// `HostifArea::ip2me_trap`: the hostif trap TRAP routes (IP2ME) copy to, created on first use
    /// (queue 0, the trap create of `cmds/netdev_test.py`); the IP2ME SAI trap programs its queue.
    pub fn ip2me_trap(&mut self, core: &Core) -> Result<Handle> {
        if self.ip2me.is_null() {
            let mut q = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
            let mut t = Handle::NULL;
            core.call(
                core.lib.ifcs_hostif_trap_create(NODE, &mut t, &mut q),
                "hostif_trap_create",
            )?;
            self.ip2me = t;
        }
        Ok(self.ip2me)
    }

    /// `HostifArea::glean_trap`: a separate hostif trap for glean (neighbor miss) routes, as isai
    /// has (`NEIGHBOR_MISS` programs its queue / policer), created on first use (queue 0).
    pub fn glean_trap(&mut self, core: &Core) -> Result<Handle> {
        if self.glean.is_null() {
            let mut q = Attrs::new().u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
            let mut t = Handle::NULL;
            core.call(
                core.lib.ifcs_hostif_trap_create(NODE, &mut t, &mut q),
                "hostif_trap_create (glean)",
            )?;
            self.glean = t;
        }
        Ok(self.glean)
    }

    /// `HostifArea::system_traps_init`: isai 6.2.0's `create_switch` calls, in its order: system
    /// trap 23 (L3 MTU) queue 0 + enable, then disable; 17 (TTL) queue 0 + enable, enable; 16 (L2
    /// source miss / move) queue 0 + enable, then disable; 20 (IPv4 options) enable. Once;
    /// `SAI_TERALYNX_SYSTEM_TRAPS=sdk` leaves the SDK's.
    pub fn system_traps_init(&mut self, core: &Core, initialized: bool) {
        if self.sys_traps_done || !initialized {
            return;
        }
        self.sys_traps_done = true;
        if core.settings.system_traps_sdk {
            core.log_once(
                "systraps",
                format_args!("SAI_TERALYNX_SYSTEM_TRAPS=sdk: system traps left as the SDK has them"),
            );
            return;
        }
        let steps = [
            (sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, true, true),
            (sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, false, false),
            (sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, true, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, false, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, true, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, false, false),
            (sys::IFCS_HOSTIF_TRAP_FWD_IPV4_OPTIONS_DETECTED, false, true),
        ];
        for (reason, queue, enable) in steps {
            let mut a = Attrs::new();
            if queue {
                a = a.u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
            }
            a = a.data(sys::IFCS_HOSTIF_TRAP_ATTR_ENABLE, enable);
            let _ = core.call(
                core.lib
                    .ifcs_hostif_trap_attr_set(NODE, Handle::hostif_trap(reason).0, &mut a),
                "hostif_trap_attr_set (system trap)",
            );
        }
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!("system traps as isai: TTL and IPv4 options to the CPU, L2 source miss/move and L3 MTU off"),
        );
    }

    /// `netif_teardown`: an adopted netdev gets its SDK name back, a created one is deleted.
    fn netif_teardown(core: &Core, n: &Netif) -> Result<()> {
        if n.adopted {
            return rename_netdev(&n.name, &n.orig, None);
        }
        core.call(core.lib.ifcs_netdev_delete(NODE, n.handle), "netdev_delete")
    }
}

/// `HostifArea::cpu_queue_stats`: frames and bytes CPU queue `q` sent to the CPU, frames it
/// dropped (no log on failure, as the C++).
pub fn cpu_queue_stats(core: &Core, q: u32) -> Result<(u64, u64, u64)> {
    let mut ids = [
        sys::IFCS_CPU_QUEUE_STATS_ID_PACKET_TX_COUNT,
        sys::IFCS_CPU_QUEUE_STATS_ID_BYTE_TX_COUNT,
        sys::IFCS_CPU_QUEUE_STATS_ID_PACKET_DROP_COUNT,
    ]
    .map(|i| i32::try_from(i).unwrap_or(i32::MAX));
    let mut v = [0u64; 3];
    let (rc, _) = core.lib.ifcs_cpu_queue_stats_get(NODE, q, &mut ids, &mut v);
    rc.ok()?;
    Ok((v[0], v[1], v[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP2ME: Handle = Handle(0x101);
    const GLEAN: Handle = Handle(0x102);

    fn def(t: u32) -> Option<TrapDef> {
        trap_def(t.cast_signed(), IP2ME, GLEAN)
    }

    #[test]
    fn l2_protocols_by_dmac_and_ether_type() {
        let d = def(ssys::SAI_HOSTIF_TRAP_TYPE_LLDP).unwrap();
        assert_eq!(d.rules.len(), 3);
        assert_eq!(d.rules[0][0].value, vec![0x01, 0x80, 0xc2, 0, 0, 0x0e]);
        assert_eq!(d.rules[2][1].value, vec![0x88, 0xcc]);
        assert_eq!((d.sys, d.prio, d.ip2me, d.glean), (0, 300, false, false));
        assert_eq!(def(ssys::SAI_HOSTIF_TRAP_TYPE_STP).unwrap().prio, 310);
        assert_eq!(def(ssys::SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE).unwrap().prio, 310);
        assert_eq!(
            def(ssys::SAI_HOSTIF_TRAP_TYPE_DHCP_L2).unwrap(),
            def(ssys::SAI_HOSTIF_TRAP_TYPE_DHCP).unwrap()
        );
        assert_eq!(
            def(ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY)
                .unwrap()
                .rules
                .len(),
            5
        );
    }

    #[test]
    fn routed_to_me_protocols_match_the_ip2me_trap() {
        // t_copp: BGP = dst and src port 179, both with the IP2ME trap (type 98)
        let d = def(ssys::SAI_HOSTIF_TRAP_TYPE_BGP).unwrap();
        assert_eq!(d.rules.len(), 2);
        assert_eq!(d.prio, 200);
        assert_eq!(d.rules[0][2].kind, sys::IFCS_ACL_MATCH_TYPE_DESTINATION_L4_PORT);
        assert_eq!(d.rules[1][2].kind, sys::IFCS_ACL_MATCH_TYPE_SOURCE_L4_PORT);
        assert_eq!(d.rules[0][2].value, vec![0x00, 0xb3]);
        assert!(
            d.rules
                .iter()
                .all(|r| r[3].kind == sys::IFCS_ACL_MATCH_TYPE_HOSTIF_TRAP && r[3].object == IP2ME)
        );
        assert_eq!(def(ssys::SAI_HOSTIF_TRAP_TYPE_SSH).unwrap().rules.len(), 4);
        assert_eq!(def(ssys::SAI_HOSTIF_TRAP_TYPE_SNMP).unwrap().rules.len(), 2);
        assert_eq!(def(ssys::SAI_HOSTIF_TRAP_TYPE_BFDV6).unwrap().rules.len(), 2);
        let me = def(ssys::SAI_HOSTIF_TRAP_TYPE_IP2ME).unwrap();
        assert!(me.ip2me && me.prio == 400 && me.rules == vec![vec![m_trap(IP2ME)]]);
        let nm = def(ssys::SAI_HOSTIF_TRAP_TYPE_NEIGHBOR_MISS).unwrap();
        assert!(nm.glean && nm.rules == vec![vec![m_trap(GLEAN)]]);
    }

    #[test]
    fn igmp_mld_and_system_traps() {
        let q = def(ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY).unwrap();
        assert_eq!(q.rules, vec![vec![m_et(0x0800), m_proto(2), m_byte1(0x11)]]);
        assert_eq!(
            def(ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT).unwrap().rules[0][2].value,
            vec![0x22]
        );
        assert_eq!(
            def(ssys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE).unwrap().rules[0][2].value,
            vec![0x17]
        );
        let m = def(ssys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT).unwrap();
        assert_eq!(m.rules, vec![vec![m_et(0x86dd), m_proto(58), m_byte1(143)]]);
        assert_eq!(
            def(ssys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2).unwrap().rules[0][2].value,
            vec![130]
        );
        let ttl = def(ssys::SAI_HOSTIF_TRAP_TYPE_TTL_ERROR).unwrap();
        assert_eq!(ttl.sys, sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL);
        assert_eq!(ttl.rules[0][0].object, Handle(0x1820_0011));
        let mtu = def(ssys::SAI_HOSTIF_TRAP_TYPE_L3_MTU_ERROR).unwrap();
        assert!(mtu.sys == sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL && mtu.rules.is_empty());
        for t in [
            ssys::SAI_HOSTIF_TRAP_TYPE_OSPF,
            ssys::SAI_HOSTIF_TRAP_TYPE_PIM,
            ssys::SAI_HOSTIF_TRAP_TYPE_VRRP,
            ssys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET,
        ] {
            assert_eq!(def(t), None);
        }
    }

    #[test]
    fn priorities_actions_and_tags() {
        assert_eq!(prio_offset(300, 0), 3009);
        assert_eq!(prio_offset(200, 4), 2005);
        assert_eq!(prio_offset(400, 100), 4000);
        assert!(to_cpu(ssys::SAI_PACKET_ACTION_TRAP.cast_signed()));
        assert!(to_cpu(ssys::SAI_PACKET_ACTION_LOG.cast_signed()));
        assert!(!to_cpu(ssys::SAI_PACKET_ACTION_DROP.cast_signed()));
        assert!(!to_cpu(-1));
        assert_eq!(tag_action(false), sys::IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_STRIP);
        assert_eq!(tag_action(true), sys::IFCS_NETDEV_VLAN_TAG_ACTION_VLAN_TAG_ORIGINAL);
        assert_eq!(policer_num(None), -1);
        assert_eq!(policer_num(Some(sai_core::backend::types::PolicerId(3))), 3);
    }

    #[test]
    fn adopting_a_missing_netdev_is_item_not_found() {
        // no inno_0_9999 exists on a build host (sandbox without AF_INET: FAILURE before any ioctl)
        let r = rename_netdev("inno_0_9999", "tl-test0", None);
        assert!(matches!(r, Err(Error::ItemNotFound | Error::Failure)));
    }
}

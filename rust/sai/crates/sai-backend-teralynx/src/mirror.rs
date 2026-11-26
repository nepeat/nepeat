//! Mirror sessions and sampling (`tl_mirror.{h,cpp}`), line by line, the way isai 6.2.0 programs this
//! chip (oracle, `sai-oracle.md` "Mirror"):
//!
//! - a session = a `dup_copy_dest_port` (DCDP) on the monitor port's sysport (one per sysport, shared by
//!   the sessions on it: the SDK refuses a second one, "Sysport in use by DCDP", chip 2026-11-16), plus
//!   per direction in use a collector (SPAN: type SPAN; ERSPAN: `ERSPAN_TYPE_II` with MACs, IPs, DSCP =
//!   TOS >> 2, TTL) in a collector set of the same type; a session change re-creates the collector
//!   inside the same set (sysport policies and ACL actions keep pointing at the set);
//! - port mirroring: sysport `MIRROR_INGRESS` / `MIRROR_EGRESS` = traffic-monitor policy {ENABLE, the
//!   set, container 0, rate 1}, off = {DISABLE, the same set}; one session per port and direction;
//! - SAMPLEPACKET: one `sFlow` collector (type SFLOW, DCDP = the CPU sysport, isai's tag values) and set
//!   per direction for all ports; sysport `SFLOW_INGRESS` = {ENABLE, the set, container 1, rate}; egress
//!   sampling needs the node's `EGRESS_SFLOW_ENABLE` (off in this init config), else it is refused;
//!   every sampler change passes the port's rate to the `sFlow` area (the samples' metadata).
//!
//! [`MirrorArea::collector_set`] is for the ACL area (MIRROR actions, [`crate::acl::MirrorSets`]): the
//! set of a session and direction, created on first use and counted. Ids: one counter for sessions and
//! samplers, from 1 (`next_`).
#![forbid(unsafe_code)]

use core::mem::{offset_of, size_of};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::IpAddr;

use ifcs::{Attrs, Handle, MonitorPolicy, sys};
use sai_core::backend::types::{MirrorCfg, MirrorId, SampleId};
use sai_core::error::{Error, Result};

use crate::acl::MirrorSets;
use crate::core::{Core, NODE};
use crate::sflow::SflowArea;

/// A direction's collector and set and their users (`Dir`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Dir {
    collector: Handle,
    set: Handle,
    refs: u32,
}

/// A direction pair (the C++ `Dir dir[2]`: [0] ingress, [1] egress).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Dirs {
    ingress: Dir,
    egress: Dir,
}

impl Dirs {
    const fn at(&self, ingress: bool) -> &Dir {
        if ingress { &self.ingress } else { &self.egress }
    }

    const fn at_mut(&mut self, ingress: bool) -> &mut Dir {
        if ingress { &mut self.ingress } else { &mut self.egress }
    }

    const fn any_used(&self) -> bool {
        self.ingress.refs != 0 || self.egress.refs != 0
    }
}

/// A session (`Session`).
#[derive(Clone, Copy, Debug)]
struct Session {
    cfg: MirrorCfg,
    dcdp: Handle,
    dir: Dirs,
}

/// The `sFlow` collectors toward the CPU (`Sampler`).
#[derive(Clone, Copy, Debug, Default)]
struct Sampler {
    dir: Dirs,
    dcdp: Handle,
}

/// A DCDP's sysport: a front port's devport, or the CPU (the C++ -1).
type Monitor = Option<u32>;

/// The mirror area's state (`MirrorArea`).
#[derive(Debug)]
pub struct MirrorArea {
    next: u32,
    sessions: BTreeMap<u32, Session>,
    /// (devport, ingress) -> session.
    port_session: BTreeMap<(u32, bool), u32>,
    /// Sampler id -> rate.
    samples: BTreeMap<u32, u32>,
    /// (devport, ingress) -> sampler id.
    port_sample: BTreeMap<(u32, bool), u32>,
    sflow: Sampler,
    /// Monitor -> (DCDP, users).
    dcdps: BTreeMap<Monitor, (Handle, u32)>,
}

impl Default for MirrorArea {
    fn default() -> Self {
        Self {
            next: 1,
            sessions: BTreeMap::new(),
            port_session: BTreeMap::new(),
            samples: BTreeMap::new(),
            port_sample: BTreeMap::new(),
            sflow: Sampler::default(),
            dcdps: BTreeMap::new(),
        }
    }
}

const fn dir_val(ingress: bool) -> u32 {
    if ingress {
        sys::IFCS_TRAFFIC_MONITOR_DIRECTION_INGRESS
    } else {
        sys::IFCS_TRAFFIC_MONITOR_DIRECTION_EGRESS
    }
}

/// `ip_attr`: `value.ip_addr` (an IPv4 address as its host-order value).
fn ip_attr(a: Attrs, id: u32, ip: IpAddr) -> Attrs {
    a.bytes(id, ip_bytes(ip))
}

/// An `ifcs_ip_address_t`'s bytes: the family, the address (IPv4: its host-order value).
fn ip_bytes(ip: IpAddr) -> Vec<u8> {
    type A = sys::ifcs_ip_address_t;
    let mut b = vec![0u8; size_of::<A>()];
    let (family, addr) = match ip {
        IpAddr::V4(v4) => (sys::IFCS_IP_ADDR_FAMILY_IPV4, u32::from(v4).to_ne_bytes().to_vec()),
        IpAddr::V6(v6) => (sys::IFCS_IP_ADDR_FAMILY_IPV6, v6.octets().to_vec()),
    };
    let mut put = |off: usize, v: &[u8]| {
        if let Some(d) = b.get_mut(off..off + v.len()) {
            d.copy_from_slice(v);
        }
    };
    put(offset_of!(A, addr_family), &family.cast_signed().to_ne_bytes());
    put(offset_of!(A, addr), &addr);
    b
}

/// The type of a session's collectors and sets.
const fn collector_type(erspan: bool) -> u32 {
    if erspan {
        sys::IFCS_COLLECTOR_TYPE_ERSPAN_TYPE_II
    } else {
        sys::IFCS_COLLECTOR_TYPE_SPAN
    }
}

/// `collector_make`: isai: SPAN ingress {type, direction, dcdp, truncate}, egress without truncate;
/// ERSPAN {type II, direction, dcdp, dest / src MAC, src / dest IP, DSCP, TTL, (VLAN,) session 0,
/// truncate when asked}. `TRUNCATE`: 0.12.41 refuses it on egress and on ERSPAN collectors ("Invalid
/// attr check failed", chip 2026-11-16); egress copies come out truncated anyway (180 B on the wire for
/// 260 B frames).
fn collector_make(core: &Core, cfg: &MirrorCfg, dcdp: Handle, ingress: bool) -> Result<Handle> {
    let m = cfg;
    let mut a = Attrs::new()
        .u32(sys::IFCS_COLLECTOR_ATTR_COLLECTOR_TYPE, collector_type(m.erspan))
        .u32(sys::IFCS_COLLECTOR_ATTR_DIRECTION, dir_val(ingress))
        .handle(sys::IFCS_COLLECTOR_ATTR_DCDP, dcdp);
    if m.erspan {
        a = a
            .mac(sys::IFCS_COLLECTOR_ATTR_DEST_MAC, m.dmac.0)
            .mac(sys::IFCS_COLLECTOR_ATTR_SRC_MAC, m.smac.0);
        a = ip_attr(a, sys::IFCS_COLLECTOR_ATTR_SRC_IP, m.src);
        a = ip_attr(a, sys::IFCS_COLLECTOR_ATTR_DEST_IP, m.dst);
        a = a
            .u8(sys::IFCS_COLLECTOR_ATTR_DSCP, m.tos >> 2)
            .u8(sys::IFCS_COLLECTOR_ATTR_TTL, m.ttl);
        if m.vlan {
            a = a.u16(sys::IFCS_COLLECTOR_ATTR_DOT1Q_VLAN, m.vlan_id);
        }
        a = a.u16(sys::IFCS_COLLECTOR_ATTR_SESSION_ID, 0);
    }
    if if m.erspan { m.truncate != 0 } else { ingress } {
        a = a.u32(
            sys::IFCS_COLLECTOR_ATTR_TRUNCATE,
            if m.truncate != 0 {
                sys::IFCS_TRAFFIC_MONITOR_TRUNCATE_YES
            } else {
                sys::IFCS_TRAFFIC_MONITOR_TRUNCATE_NO
            },
        );
    }
    let mut h = Handle::NULL;
    let st = core.call(core.lib.ifcs_collector_create(NODE, &mut h, &mut a), "collector create");
    st.map(|()| h)
}

/// `dir_get`: a direction of a session in use once more; its collector and set made on first use.
fn dir_get(core: &Core, s: &mut Session, ingress: bool) -> Result<()> {
    let (cfg, dcdp) = (s.cfg, s.dcdp);
    let d = s.dir.at_mut(ingress);
    d.refs += 1;
    if d.refs > 1 {
        return Ok(());
    }
    match collector_make(core, &cfg, dcdp, ingress) {
        Ok(h) => d.collector = h,
        Err(e) => {
            d.refs = 0;
            return Err(e);
        }
    }
    let mut a = Attrs::new()
        .u32(
            sys::IFCS_COLLECTOR_SET_ATTR_COLLECTOR_SET_TYPE,
            collector_type(cfg.erspan),
        )
        .u32(sys::IFCS_COLLECTOR_SET_ATTR_DIRECTION, dir_val(ingress));
    let mut st = core.call(
        core.lib.ifcs_collector_set_create(NODE, &mut d.set, &mut a),
        "collector set create",
    );
    if st.is_ok() {
        st = core.call(
            core.lib
                .ifcs_collector_set_member_add(NODE, d.set, &mut [d.collector], &mut Attrs::null()),
            "collector set add",
        );
    }
    if st.is_err() {
        if !d.set.is_null() {
            let _ = core.lib.ifcs_collector_set_delete(NODE, d.set);
        }
        let _ = core.lib.ifcs_collector_delete(NODE, d.collector);
        *d = Dir::default();
    }
    st
}

/// `dir_put`: one user less; the last one removes the collector from its set and deletes both.
fn dir_put(core: &Core, s: &mut Session, ingress: bool) {
    let d = s.dir.at_mut(ingress);
    if d.refs == 0 {
        return;
    }
    d.refs -= 1;
    if d.refs > 0 {
        return;
    }
    let _ = core.call(
        core.lib
            .ifcs_collector_set_member_remove(NODE, d.set, &mut [d.collector]),
        "collector set remove",
    );
    let _ = core.call(core.lib.ifcs_collector_set_delete(NODE, d.set), "collector set delete");
    let _ = core.call(core.lib.ifcs_collector_delete(NODE, d.collector), "collector delete");
    *d = Dir::default();
}

/// `policy_set`: a sysport traffic-monitor policy (`MIRROR_*`, `SFLOW_*`).
fn policy_set(core: &Core, hw: u32, attr: u32, enable: bool, set: Handle, container: u32, rate: u32) -> Result<()> {
    let mut p = MonitorPolicy::zeroed();
    let _ = core.lib.ifcs_traffic_monitor_policy_t_init(&mut p);
    let action = if enable {
        sys::IFCS_TRAFFIC_MONITOR_ACTION_ENABLE
    } else {
        sys::IFCS_TRAFFIC_MONITOR_ACTION_DISABLE
    };
    p.set(action, set, container, rate);
    core.sp_set(hw, Attrs::new().bytes(attr, p.bytes()), "sysport traffic monitor")
}

impl MirrorArea {
    /// `dcdp_get`: the monitor's DCDP (made on first use), one more user.
    fn dcdp_get(&mut self, core: &Core, hw: Monitor) -> Result<Handle> {
        if let Some((h, users)) = self.dcdps.get_mut(&hw) {
            *users += 1;
            return Ok(*h);
        }
        let h = Self::dcdp_create(core, hw)?;
        self.dcdps.insert(hw, (h, 1));
        Ok(h)
    }

    /// `dcdp_put`: one user less; the last one deletes it.
    fn dcdp_put(&mut self, core: &Core, hw: Monitor) {
        let Some((h, users)) = self.dcdps.get_mut(&hw) else {
            return;
        };
        *users -= 1;
        if *users > 0 {
            return;
        }
        let h = *h;
        let _ = core.call(core.lib.ifcs_dup_copy_dest_port_delete(NODE, h), "dcdp delete");
        self.dcdps.remove(&hw);
    }

    /// `dcdp_create`: on the monitor's sysport (the CPU's: node `CPU_SYSPORT`, else sysport 1000).
    fn dcdp_create(core: &Core, hw: Monitor) -> Result<Handle> {
        let sp = match hw {
            None => {
                let mut a = Attrs::new().of(sys::IFCS_NODE_ATTR_CPU_SYSPORT);
                let (rc, _) = core.lib.ifcs_node_attr_get(NODE, &mut a);
                if rc.is_success() && !a.handle_at(0).is_null() {
                    a.handle_at(0)
                } else {
                    Handle::sysport(1000)
                }
            }
            Some(hw) => *core.sysport.get(&hw).ok_or(Error::InvalidPortNumber)?,
        };
        let mut a = Attrs::new().handle(sys::IFCS_DUP_COPY_DEST_PORT_ATTR_SYSPORT, sp);
        let mut h = Handle::NULL;
        core.call(
            core.lib.ifcs_dup_copy_dest_port_create(NODE, &mut h, &mut a),
            "dcdp create",
        )?;
        Ok(h)
    }

    /// `create`: a session (its DCDP; collectors when a direction is used).
    pub fn create(&mut self, core: &Core, m: &MirrorCfg) -> Result<MirrorId> {
        let monitor = u32::from(m.monitor.0);
        if core.front(monitor).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if m.erspan && m.gre_type != 0x88be {
            core.log_once(
                "mirror:gre",
                format_args!(
                    "ERSPAN: GRE protocol type 0x{:04x} requested; the chip sends type II (0x88be)",
                    m.gre_type
                ),
            );
        }
        if m.truncate != 0 && m.truncate != 192 {
            core.log_once(
                "mirror:trunc",
                format_args!(
                    "mirror: TRUNCATE_SIZE {} requested; the chip truncates at its own size (isai reads 192)",
                    m.truncate
                ),
            );
        }
        if m.sample_rate > 1 {
            core.log_once(
                "mirror:rate",
                format_args!(
                    "mirror: sampled mirroring (SAMPLE_RATE {}) is not supported",
                    m.sample_rate
                ),
            );
            return Err(Error::NotSupported);
        }
        let dcdp = self.dcdp_get(core, Some(monitor))?;
        let id = self.next;
        self.next += 1;
        self.sessions.insert(
            id,
            Session {
                cfg: *m,
                dcdp,
                dir: Dirs::default(),
            },
        );
        Ok(MirrorId(id))
    }

    /// `update`: isai: the collectors out of their sets, a new DCDP when the monitor port moved, new
    /// collectors into the same sets. A failure keeps the old settings (and collectors made with them).
    pub fn update(&mut self, core: &Core, id: MirrorId, m: &MirrorCfg) -> Result<()> {
        let Some(mut s) = self.sessions.get(&id.0).copied() else {
            return Err(Error::ItemNotFound);
        };
        let monitor = u32::from(m.monitor.0);
        if core.front(monitor).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if m.erspan != s.cfg.erspan {
            return Err(Error::InvalidParameter);
        }
        if m.sample_rate > 1 {
            return Err(Error::NotSupported);
        }
        for d in [&mut s.dir.ingress, &mut s.dir.egress] {
            if d.refs != 0 {
                let _ = core.call(
                    core.lib
                        .ifcs_collector_set_member_remove(NODE, d.set, &mut [d.collector]),
                    "collector set remove",
                );
                let _ = core.call(core.lib.ifcs_collector_delete(NODE, d.collector), "collector delete");
                d.collector = Handle::NULL;
            }
        }
        let mut st = Ok(());
        let old_monitor = u32::from(s.cfg.monitor.0);
        if monitor != old_monitor {
            match self.dcdp_get(core, Some(monitor)) {
                Ok(nd) => {
                    self.dcdp_put(core, Some(old_monitor));
                    s.dcdp = nd;
                }
                Err(e) => st = Err(e),
            }
        }
        let old = s.cfg;
        if st.is_ok() {
            s.cfg = *m;
        }
        for ingress in [true, false] {
            if s.dir.at(ingress).refs == 0 {
                continue;
            }
            let (cfg, dcdp) = (s.cfg, s.dcdp);
            let d = s.dir.at_mut(ingress);
            let r = collector_make(core, &cfg, dcdp, ingress).and_then(|h| {
                d.collector = h;
                core.call(
                    core.lib
                        .ifcs_collector_set_member_add(NODE, d.set, &mut [h], &mut Attrs::null()),
                    "collector set add",
                )
            });
            if r.is_err() && st.is_ok() {
                st = r;
            }
        }
        if st.is_err() {
            s.cfg = old;
        }
        self.sessions.insert(id.0, s);
        st
    }

    /// `destroy`: `OBJECT_IN_USE` while a port or an ACL entry uses it.
    pub fn destroy(&mut self, core: &Core, id: MirrorId) -> Result<()> {
        let Some(s) = self.sessions.get(&id.0) else {
            return Err(Error::ItemNotFound);
        };
        if s.dir.any_used() {
            return Err(Error::ObjectInUse);
        }
        let monitor = u32::from(s.cfg.monitor.0);
        self.dcdp_put(core, Some(monitor));
        self.sessions.remove(&id.0);
        Ok(())
    }

    /// `port_set`: the session a port mirrors to in one direction (one at most).
    pub fn port_set(&mut self, core: &Core, hw: u32, ingress: bool, ids: &[MirrorId]) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if ids.len() > 1 {
            core.log_once(
                "mirror:multi",
                format_args!("port mirroring: one session per port and direction"),
            );
            return Err(Error::NotSupported);
        }
        if ids.iter().any(|x| !self.sessions.contains_key(&x.0)) {
            return Err(Error::ItemNotFound);
        }
        let key = (hw, ingress);
        let old = self.port_session.get(&key).copied();
        let now = ids.first().map(|x| x.0);
        if old == now {
            return Ok(());
        }
        let attr = if ingress {
            sys::IFCS_SYSPORT_ATTR_MIRROR_INGRESS
        } else {
            sys::IFCS_SYSPORT_ATTR_MIRROR_EGRESS
        };
        if let Some(now) = now {
            let Some(s) = self.sessions.get_mut(&now) else {
                return Err(Error::ItemNotFound);
            };
            dir_get(core, s, ingress)?;
            let set = s.dir.at(ingress).set;
            if let Err(e) = policy_set(core, hw, attr, true, set, sys::IFCS_TRAFFIC_MONITOR_CONTAINER_0, 1) {
                dir_put(core, s, ingress);
                return Err(e);
            }
            self.port_session.insert(key, now);
        } else if let Some(o) = old {
            let set = self.sessions.get(&o).map_or(Handle::NULL, |s| s.dir.at(ingress).set);
            policy_set(core, hw, attr, false, set, sys::IFCS_TRAFFIC_MONITOR_CONTAINER_0, 1)?;
            self.port_session.remove(&key);
        }
        if let Some(o) = old {
            if let Some(s) = self.sessions.get_mut(&o) {
                dir_put(core, s, ingress);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- sampling

    /// `sflow_get`: the `sFlow` collector toward the CPU and its set for a direction (made on first use),
    /// one more user. isai's MACs / DSCP / TTL / session id: the tags in the samples' header.
    fn sflow_get(&mut self, core: &Core, ingress: bool) -> Result<Handle> {
        let d = self.sflow.dir.at_mut(ingress);
        d.refs += 1;
        if d.refs > 1 {
            return Ok(d.set);
        }
        if self.sflow.dcdp.is_null() {
            match self.dcdp_get(core, None) {
                Ok(h) => self.sflow.dcdp = h,
                Err(e) => {
                    self.sflow.dir.at_mut(ingress).refs = 0;
                    return Err(e);
                }
            }
        }
        let smac = [0x00, 0x02, 0x04, 0x06, 0x08, 0x0a];
        let dmac = [0x00, 0x01, 0x03, 0x05, 0x07, 0x09];
        let zero = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
        let mut a = Attrs::new()
            .u32(sys::IFCS_COLLECTOR_ATTR_COLLECTOR_TYPE, sys::IFCS_COLLECTOR_TYPE_SFLOW)
            .u32(sys::IFCS_COLLECTOR_ATTR_DIRECTION, dir_val(ingress))
            .handle(sys::IFCS_COLLECTOR_ATTR_DCDP, self.sflow.dcdp)
            .mac(sys::IFCS_COLLECTOR_ATTR_SRC_MAC, smac);
        a = ip_attr(a, sys::IFCS_COLLECTOR_ATTR_SRC_IP, zero).mac(sys::IFCS_COLLECTOR_ATTR_DEST_MAC, dmac);
        a = ip_attr(a, sys::IFCS_COLLECTOR_ATTR_DEST_IP, zero)
            .u8(sys::IFCS_COLLECTOR_ATTR_DSCP, 2)
            .u8(sys::IFCS_COLLECTOR_ATTR_TTL, 100)
            .u16(sys::IFCS_COLLECTOR_ATTR_SESSION_ID, 111);
        let d = self.sflow.dir.at_mut(ingress);
        let mut st = core.call(
            core.lib.ifcs_collector_create(NODE, &mut d.collector, &mut a),
            "sflow collector create",
        );
        if st.is_ok() {
            let mut s2 = Attrs::new()
                .u32(
                    sys::IFCS_COLLECTOR_SET_ATTR_COLLECTOR_SET_TYPE,
                    sys::IFCS_COLLECTOR_TYPE_SFLOW,
                )
                .u32(sys::IFCS_COLLECTOR_SET_ATTR_DIRECTION, dir_val(ingress));
            st = core.call(
                core.lib.ifcs_collector_set_create(NODE, &mut d.set, &mut s2),
                "sflow collector set create",
            );
            if st.is_ok() {
                st = core.call(
                    core.lib
                        .ifcs_collector_set_member_add(NODE, d.set, &mut [d.collector], &mut Attrs::null()),
                    "sflow collector set add",
                );
            }
        }
        if let Err(e) = st {
            if !d.set.is_null() {
                let _ = core.lib.ifcs_collector_set_delete(NODE, d.set);
            }
            if !d.collector.is_null() {
                let _ = core.lib.ifcs_collector_delete(NODE, d.collector);
            }
            *d = Dir::default();
            return Err(e);
        }
        Ok(d.set)
    }

    /// `sflow_put`: one user less; the last one of a direction deletes its collector and set, the last
    /// of both the CPU's DCDP use.
    fn sflow_put(&mut self, core: &Core, ingress: bool) {
        let d = self.sflow.dir.at_mut(ingress);
        if d.refs == 0 {
            return;
        }
        d.refs -= 1;
        if d.refs > 0 {
            return;
        }
        let _ = core.call(
            core.lib
                .ifcs_collector_set_member_remove(NODE, d.set, &mut [d.collector]),
            "sflow collector set remove",
        );
        let _ = core.call(
            core.lib.ifcs_collector_set_delete(NODE, d.set),
            "sflow collector set delete",
        );
        let _ = core.call(
            core.lib.ifcs_collector_delete(NODE, d.collector),
            "sflow collector delete",
        );
        *d = Dir::default();
        if !self.sflow.dir.any_used() && !self.sflow.dcdp.is_null() {
            self.dcdp_put(core, None);
            self.sflow.dcdp = Handle::NULL;
        }
    }

    /// `sample_create`: a sampler (bookkeeping; programmed per port).
    pub fn sample_create(&mut self, rate: u32) -> Result<SampleId> {
        if rate == 0 {
            return Err(Error::InvalidParameter);
        }
        let id = self.next;
        self.next += 1;
        self.samples.insert(id, rate);
        Ok(SampleId(id))
    }

    /// `sample_update`: the new rate on every port using it (policy, then the `sFlow` area's rate); a
    /// refusal keeps the old rate (ports already changed stay changed, as the C++).
    pub fn sample_update(&mut self, core: &Core, sflow: &mut SflowArea, id: SampleId, rate: u32) -> Result<()> {
        let Some(old) = self.samples.insert(id.0, rate) else {
            self.samples.remove(&id.0);
            return Err(Error::ItemNotFound);
        };
        let users: Vec<(u32, bool)> = self
            .port_sample
            .iter()
            .filter(|(_, s)| **s == id.0)
            .map(|(k, _)| *k)
            .collect();
        for (hw, ingress) in users {
            let set = self.sflow.dir.at(ingress).set;
            let attr = if ingress {
                sys::IFCS_SYSPORT_ATTR_SFLOW_INGRESS
            } else {
                sys::IFCS_SYSPORT_ATTR_SFLOW_EGRESS
            };
            if let Err(e) = policy_set(core, hw, attr, true, set, sys::IFCS_TRAFFIC_MONITOR_CONTAINER_1, rate) {
                self.samples.insert(id.0, old);
                return Err(e);
            }
            sflow.port_rate(core, hw, ingress, rate); // the samples' metadata
        }
        Ok(())
    }

    /// `sample_destroy`: `OBJECT_IN_USE` while a port uses it.
    pub fn sample_destroy(&mut self, id: SampleId) -> Result<()> {
        if self.port_sample.values().any(|s| *s == id.0) {
            return Err(Error::ObjectInUse);
        }
        self.samples.remove(&id.0).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    /// `port_sample_set`: a port's sampler in one direction (`None`: off). Egress needs the node's
    /// `EGRESS_SFLOW_ENABLE` (`SflowArea::egress_enabled`).
    pub fn port_sample_set(
        &mut self,
        core: &Core,
        sflow: &mut SflowArea,
        hw: u32,
        ingress: bool,
        id: Option<SampleId>,
    ) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        let rate = match id {
            Some(i) => Some(*self.samples.get(&i.0).ok_or(Error::ItemNotFound)?),
            None => None,
        };
        if !ingress && id.is_some() && !sflow.egress_enabled(core) {
            core.log_once(
                "sflow:egress",
                format_args!("egress sampling: not supported (node EGRESS_SFLOW_ENABLE is off in this init config)"),
            );
            return Err(Error::NotSupported);
        }
        let key = (hw, ingress);
        let had = self.port_sample.contains_key(&key);
        let attr = if ingress {
            sys::IFCS_SYSPORT_ATTR_SFLOW_INGRESS
        } else {
            sys::IFCS_SYSPORT_ATTR_SFLOW_EGRESS
        };
        if let (Some(id), Some(rate)) = (id, rate) {
            if !had {
                self.sflow_get(core, ingress)?;
            }
            let set = self.sflow.dir.at(ingress).set;
            if let Err(e) = policy_set(core, hw, attr, true, set, sys::IFCS_TRAFFIC_MONITOR_CONTAINER_1, rate) {
                if !had {
                    self.sflow_put(core, ingress);
                }
                return Err(e);
            }
            self.port_sample.insert(key, id.0);
            sflow.port_rate(core, hw, ingress, rate); // the samples' metadata
            return Ok(());
        }
        if !had {
            return Ok(());
        }
        let set = self.sflow.dir.at(ingress).set;
        policy_set(core, hw, attr, false, set, sys::IFCS_TRAFFIC_MONITOR_CONTAINER_1, 0)?;
        self.port_sample.remove(&key);
        self.sflow_put(core, ingress);
        sflow.port_rate(core, hw, ingress, 0);
        Ok(())
    }

    /// `show` (the C++ `diag "mirror"` text, the `sFlow` area's after it; no diag command calls it there
    /// either).
    pub fn show(&self, sflow: &SflowArea) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "mirror: {} sessions, {} port bindings, {} samplers on {} ports",
            self.sessions.len(),
            self.port_session.len(),
            self.samples.len(),
            self.port_sample.len()
        );
        for (id, x) in &self.sessions {
            let _ = writeln!(
                s,
                "  session {id}: {} monitor devport {} dcdp 0x{:x} in set 0x{:x} ({}) eg set 0x{:x} ({})",
                if x.cfg.erspan { "ERSPAN" } else { "SPAN" },
                x.cfg.monitor.0,
                x.dcdp.0,
                x.dir.ingress.set.0,
                x.dir.ingress.refs,
                x.dir.egress.set.0,
                x.dir.egress.refs
            );
        }
        for ((hw, ingress), id) in &self.port_sample {
            let _ = writeln!(
                s,
                "  devport {hw} {}: sample {id} rate {}",
                if *ingress { "in" } else { "eg" },
                self.samples.get(id).copied().unwrap_or(0)
            );
        }
        s.push_str(&sflow.show());
        s
    }
}

impl MirrorSets for MirrorArea {
    /// `collector_set`: the set of session `id` in a direction (created on first use, counted).
    fn collector_set(&mut self, core: &Core, id: MirrorId, ingress: bool) -> Result<Handle> {
        let Some(s) = self.sessions.get_mut(&id.0) else {
            return Err(Error::ItemNotFound);
        };
        dir_get(core, s, ingress)?;
        Ok(s.dir.at(ingress).set)
    }

    /// `collector_set_release`.
    fn collector_set_release(&mut self, core: &Core, id: MirrorId, ingress: bool) {
        if let Some(s) = self.sessions.get_mut(&id.0) {
            dir_put(core, s, ingress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_values_are_family_and_host_order_address() {
        let b = ip_bytes("10.99.1.1".parse().unwrap());
        assert_eq!(b.len(), size_of::<sys::ifcs_ip_address_t>());
        assert_eq!(b[0..4], sys::IFCS_IP_ADDR_FAMILY_IPV4.cast_signed().to_ne_bytes());
        assert_eq!(b[4..8], 0x0a63_0101u32.to_ne_bytes());
        let b6 = ip_bytes("2001:db8::1".parse().unwrap());
        assert_eq!(b6[0..4], sys::IFCS_IP_ADDR_FAMILY_IPV6.cast_signed().to_ne_bytes());
        assert_eq!(b6[4..6], [0x20, 0x01]);
        assert_eq!(b6[19], 1);
        assert_eq!(dir_val(true), sys::IFCS_TRAFFIC_MONITOR_DIRECTION_INGRESS);
        assert_eq!(collector_type(true), sys::IFCS_COLLECTOR_TYPE_ERSPAN_TYPE_II);
        assert_eq!(collector_type(false), sys::IFCS_COLLECTOR_TYPE_SPAN);
    }

    #[test]
    fn samplers_are_bookkeeping_until_a_port_uses_them() {
        let mut m = MirrorArea::default();
        assert_eq!(m.sample_create(0), Err(Error::InvalidParameter));
        let s = m.sample_create(1000).unwrap();
        assert_eq!(s, SampleId(1));
        m.port_sample.insert((1, true), s.0);
        assert_eq!(m.sample_destroy(s), Err(Error::ObjectInUse));
        m.port_sample.clear();
        assert_eq!(m.sample_destroy(s), Ok(()));
        assert_eq!(m.sample_destroy(s), Err(Error::ItemNotFound));
        assert_eq!(m.sample_create(5).unwrap(), SampleId(2)); // one counter, never reused
    }
}

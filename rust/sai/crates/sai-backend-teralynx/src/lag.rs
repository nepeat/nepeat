//! LAGs and the switch hash (`tl_lag.{h,cpp}`). Call forms as isai 6.2.0 makes them on this chip
//! (oracle lag-l2, sai-oracle.md):
//!
//! * create: `ifcs_lag_create(0, &h, 0, NULL)`;
//! * member: `ifcs_lag_member_add(0, lag, 1, [sysport], 0, NULL)` / `ifcs_lag_member_remove`; a member
//!   that is both egress- and ingress-disabled is not in the IFCS LAG (isai), its learning is off;
//!   egress-only disable keeps it (no IFCS member state);
//! * dest: L2 entries, L2VNI members, `mc_dest`s and next hops take the LAG handle where a port takes
//!   its sysport ([`LagArea::dest_handle`], the accessor the other areas use);
//! * hash: `ifcs_extended_field_list` (ELEMENTS) per packet type on the node `ENTROPY_SELECT_*`
//!   attributes, `ENTROPY_SEED` (one entropy for LAG and ECMP).
//!
//! libifcs refuses a member that is in an L2VNI, an `mc_dest` or an L2 entry: the L2 area keeps
//! ports out of all three while they are in no VLAN, and a joining member's learned entries are
//! flushed first.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use ifcs::{Attrs, Handle, sys};
use sai_core::backend::HwPort;
use sai_core::backend::types::{Dest, LagMember, TrunkId};
use sai_core::error::{AttrError, Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};
use crate::l2::L2Area;

/// LAG handle / member sysport -> `lag_dest(tid)`, read by the L2 callback (its own lock, never
/// held across an IFCS call: `map_mu_`).
pub type DestMap = Arc<Mutex<BTreeMap<Handle, i32>>>;

/// The map's entry for `h`.
pub fn lookup(map: &DestMap, h: Handle) -> Option<i32> {
    map.lock().unwrap_or_else(PoisonError::into_inner).get(&h).copied()
}

/// Hash field names (`Fld`), in the C++ order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// `src_ip`
    SrcIp,
    /// `dst_ip`
    DstIp,
    /// `ip_protocol`
    Proto,
    /// `l4_src_port`
    L4Src,
    /// `l4_dst_port`
    L4Dst,
    /// `src_mac`
    SrcMac,
    /// `dst_mac`
    DstMac,
    /// `ethertype`
    Ethertype,
    /// `vlan_id`
    Vlan,
    /// `in_port`
    InPort,
    /// `ipv6_flow_label`
    FlowLabel,
}

const NAMES: [(&str, Field); 11] = [
    ("src_ip", Field::SrcIp),
    ("dst_ip", Field::DstIp),
    ("ip_protocol", Field::Proto),
    ("l4_src_port", Field::L4Src),
    ("l4_dst_port", Field::L4Dst),
    ("src_mac", Field::SrcMac),
    ("dst_mac", Field::DstMac),
    ("ethertype", Field::Ethertype),
    ("vlan_id", Field::Vlan),
    ("in_port", Field::InPort),
    ("ipv6_flow_label", Field::FlowLabel),
];

/// `SAI_NATIVE_HASH_FIELD_*` with an IFCS entropy element (`hash_set`'s switch).
const SAI_FIELDS: [(u32, Field); 11] = [
    (ssys::SAI_NATIVE_HASH_FIELD_SRC_IP, Field::SrcIp),
    (ssys::SAI_NATIVE_HASH_FIELD_DST_IP, Field::DstIp),
    (ssys::SAI_NATIVE_HASH_FIELD_IP_PROTOCOL, Field::Proto),
    (ssys::SAI_NATIVE_HASH_FIELD_L4_SRC_PORT, Field::L4Src),
    (ssys::SAI_NATIVE_HASH_FIELD_L4_DST_PORT, Field::L4Dst),
    (ssys::SAI_NATIVE_HASH_FIELD_SRC_MAC, Field::SrcMac),
    (ssys::SAI_NATIVE_HASH_FIELD_DST_MAC, Field::DstMac),
    (ssys::SAI_NATIVE_HASH_FIELD_ETHERTYPE, Field::Ethertype),
    (ssys::SAI_NATIVE_HASH_FIELD_VLAN_ID, Field::Vlan),
    (ssys::SAI_NATIVE_HASH_FIELD_IN_PORT, Field::InPort),
    (ssys::SAI_NATIVE_HASH_FIELD_IPV6_FLOW_LABEL, Field::FlowLabel),
];

/// The field of a `SAI_NATIVE_HASH_FIELD_*` value, `None` for one without an IFCS element (inner
/// headers, ...).
pub fn sai_field(s: i32) -> Option<Field> {
    let s = u32::try_from(s).ok()?;
    SAI_FIELDS.iter().find(|(v, _)| *v == s).map(|(_, f)| *f)
}

/// The node attribute of each packet type's entropy list (IPv4 TCP/UDP, IPv4 other, IPv6
/// TCP/UDP, IPv6 other, non-IP).
const SELECT: [u32; 5] = [
    sys::IFCS_NODE_ATTR_ENTROPY_SELECT_IPV4_TCP_OR_UDP,
    sys::IFCS_NODE_ATTR_ENTROPY_SELECT_IPV4_NON_TCP_OR_UDP,
    sys::IFCS_NODE_ATTR_ENTROPY_SELECT_IPV6_TCP_OR_UDP,
    sys::IFCS_NODE_ATTR_ENTROPY_SELECT_IPV6_NON_TCP_OR_UDP,
    sys::IFCS_NODE_ATTR_ENTROPY_SELECT_NON_IP,
];

/// `LagArea::parse_fields`: comma-separated names (blanks around each ignored, empty items
/// skipped); `None` for an unknown name or none at all.
pub fn parse_fields(s: &str) -> Option<Vec<Field>> {
    let mut out = Vec::new();
    for t in s.split(',') {
        let t = t.trim_matches(' ');
        if t.is_empty() {
            continue;
        }
        out.push(NAMES.iter().find(|(n, _)| *n == t)?.1);
    }
    (!out.is_empty()).then_some(out)
}

/// The IFCS entropy elements of `fields` for packet type `t` (`hash_fields_set`: per family the
/// address / protocol fields, L4 ports for TCP/UDP only, the rest everywhere).
pub fn elements(fields: &[Field], t: usize) -> Vec<u32> {
    let (v6, l4, ip) = (t == 2 || t == 3, t == 0 || t == 2, t < 4);
    let mut el = Vec::new();
    for f in fields {
        let e = match f {
            Field::SrcIp if ip => Some(if v6 {
                sys::IFCS_EXTENDED_FIELD_IPV6_SOURCE_IP
            } else {
                sys::IFCS_EXTENDED_FIELD_IPV4_SOURCE_IP
            }),
            Field::DstIp if ip => Some(if v6 {
                sys::IFCS_EXTENDED_FIELD_IPV6_DESTINATION_IP
            } else {
                sys::IFCS_EXTENDED_FIELD_IPV4_DESTINATION_IP
            }),
            Field::Proto if ip => Some(if v6 {
                sys::IFCS_EXTENDED_FIELD_IPV6_NEXTHEADER
            } else {
                sys::IFCS_EXTENDED_FIELD_IPV4_PROTOCOL
            }),
            Field::FlowLabel if v6 => Some(sys::IFCS_EXTENDED_FIELD_IPV6_FLOW_LABEL),
            Field::L4Src if l4 => Some(sys::IFCS_EXTENDED_FIELD_TCP_OR_UDP_SOURCE_PORT),
            Field::L4Dst if l4 => Some(sys::IFCS_EXTENDED_FIELD_TCP_OR_UDP_DESTINATION_PORT),
            Field::SrcMac => Some(sys::IFCS_EXTENDED_FIELD_MAC_SA),
            Field::DstMac => Some(sys::IFCS_EXTENDED_FIELD_MAC_DA),
            Field::Ethertype => Some(sys::IFCS_EXTENDED_FIELD_ETHERTYPE),
            Field::Vlan => Some(sys::IFCS_EXTENDED_FIELD_CVID),
            Field::InPort => Some(sys::IFCS_EXTENDED_FIELD_SOURCE_SYSPORT),
            _ => None,
        };
        el.extend(e);
    }
    el
}

/// The C++ `lag_dest(tid)` integer of a trunk (what the callback map holds).
pub fn lag_dest(tid: u32) -> i32 {
    i32::try_from(Dest::Lag(TrunkId(tid)).raw()).unwrap_or(-1)
}

/// The members that are in the IFCS LAG (`set_members`' `want`: not both egress- and
/// ingress-disabled), as devports.
pub fn wanted(members: &[LagMember]) -> BTreeSet<u32> {
    members
        .iter()
        .filter(|m| !(m.egress_disable && m.ingress_disable))
        .map(|m| u32::from(m.hw.0))
        .collect()
}

/// A LAG (`LagArea::Lag`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lag {
    /// Its IFCS handle.
    pub h: Handle,
    /// The frontend's member list.
    pub members: Vec<LagMember>,
    /// The devports that are IFCS members now.
    pub in_ifcs: BTreeSet<u32>,
}

/// The LAG area's state.
#[derive(Debug)]
pub struct LagArea {
    /// The extended field lists this backend made, per packet type (`hash_lists_`).
    pub hash_lists: [Handle; 5],
    /// The next trunk id (`next_tid_`).
    pub next_tid: u32,
    /// Shared with the L2 callback.
    pub dest_map: DestMap,
    /// The LAGs by trunk id.
    pub lags: BTreeMap<u32, Lag>,
    /// What SAI set last: `[0]` ECMP, `[1]` LAG (`hash_sai_`).
    pub hash_sai: [Vec<i32>; 2],
    /// A seed was written (`seed_set_`).
    pub seed_set: bool,
}

impl Default for LagArea {
    fn default() -> Self {
        Self {
            hash_lists: [Handle::NULL; 5],
            next_tid: 1,
            dest_map: DestMap::default(),
            lags: BTreeMap::new(),
            hash_sai: [Vec::new(), Vec::new()],
            seed_set: false,
        }
    }
}

impl LagArea {
    /// `LagArea::reset` (a new switch): no LAGs, the old node's lists gone with it.
    pub fn reset(&mut self) {
        self.lags.clear();
        self.next_tid = 1;
        self.hash_lists = [Handle::NULL; 5];
        self.dest_map.lock().unwrap_or_else(PoisonError::into_inner).clear(); // publish() of no LAGs
    }

    /// `LagArea::create`: `ifcs_lag_create(0, &h, 0, NULL)`, the next trunk id.
    pub fn create(&mut self, core: &Core) -> Result<TrunkId> {
        let mut h = Handle::NULL;
        core.call(core.lib.ifcs_lag_create(NODE, &mut h, &mut Attrs::null()), "lag_create")?;
        let tid = self.next_tid;
        self.next_tid = self.next_tid.wrapping_add(1);
        self.lags.insert(tid, Lag { h, ..Lag::default() });
        self.publish(core);
        log::notice(ssys::SAI_API_LAG, format_args!("LAG {tid} = IFCS lag 0x{:x}", h.0));
        Ok(TrunkId(tid))
    }

    /// `LagArea::destroy`: its IFCS members out, then `ifcs_lag_delete` (a refusal keeps the LAG).
    pub fn destroy(&mut self, core: &Core, tid: TrunkId) -> Result<()> {
        let Some(l) = self.lags.get_mut(&tid.0) else {
            return Err(Error::ItemNotFound);
        };
        for hw in l.in_ifcs.clone() {
            let _ = Self::member_remove(core, l, hw);
        }
        core.call(core.lib.ifcs_lag_delete(NODE, l.h), "lag_delete")?;
        self.lags.remove(&tid.0);
        self.publish(core);
        Ok(())
    }

    /// `LagArea::set_members`: the frontend passes the whole list. In IFCS: every member that is
    /// not both egress- and ingress-disabled (isai's rule; teamd sets both while a member is not
    /// collecting / distributing). Leaving members go first; disabled ones (in the list, not in
    /// IFCS) do not learn: their frames would otherwise create entries on the member sysport,
    /// which then blocks its return to the LAG. The status: the last failed removal, else the first
    /// failed add (the C++'s).
    pub fn set_members(&mut self, core: &Core, tid: TrunkId, members: &[LagMember]) -> Result<()> {
        if !self.lags.contains_key(&tid.0) {
            return Err(Error::ItemNotFound);
        }
        for m in members {
            if core.front(u32::from(m.hw.0)).is_none() {
                return Err(Error::InvalidPortNumber);
            }
            let elsewhere = self
                .lags
                .iter()
                .any(|(&o, l)| o != tid.0 && l.members.iter().any(|x| x.hw == m.hw));
            if elsewhere {
                return Err(Error::ItemAlreadyExists);
            }
        }
        let want = wanted(members);
        let Some(l) = self.lags.get_mut(&tid.0) else {
            return Err(Error::ItemNotFound);
        };
        let mut res = Ok(());
        for hw in l.in_ifcs.clone() {
            if !want.contains(&hw) {
                if let Err(e) = Self::member_remove(core, l, hw) {
                    res = Err(e);
                }
            }
        }
        for m in members {
            let hw = u32::from(m.hw.0);
            if !want.contains(&hw) {
                let _ = core.sp_set(
                    hw,
                    Attrs::new().data(sys::IFCS_SYSPORT_ATTR_LEARN_ENABLE, false),
                    "lag member learn off",
                );
            }
        }
        for &hw in &want {
            if !l.in_ifcs.contains(&hw) {
                if let Err(e) = Self::member_add(core, l, hw) {
                    if res.is_ok() {
                        res = Err(e);
                    }
                }
            }
        }
        l.members = members.to_vec();
        self.publish(core);
        res
    }

    /// `LagArea::member_add`: what was learned on the port is flushed (libifcs refuses a member an
    /// L2 entry points at), `ifcs_lag_member_add`, learning on.
    fn member_add(core: &Core, l: &mut Lag, hw: u32) -> Result<()> {
        let Some(&sp) = core.sysport.get(&hw) else {
            return Err(Error::InvalidPortNumber);
        };
        let _ = L2Area::flush_handle(core, Some(sp), None, false, true);
        core.call(
            core.lib.ifcs_lag_member_add(NODE, l.h, &mut [sp], &mut Attrs::null()),
            "lag_member_add",
        )?;
        l.in_ifcs.insert(hw);
        let _ = core.sp_set(
            hw,
            Attrs::new().data(sys::IFCS_SYSPORT_ATTR_LEARN_ENABLE, true),
            "lag member learn on",
        );
        Ok(())
    }

    /// `LagArea::member_remove`.
    fn member_remove(core: &Core, l: &mut Lag, hw: u32) -> Result<()> {
        let sp = core.sysport.get(&hw).copied().unwrap_or_default();
        core.call(
            core.lib.ifcs_lag_member_remove(NODE, l.h, &mut [sp]),
            "lag_member_remove",
        )?;
        l.in_ifcs.remove(&hw);
        Ok(())
    }

    /// `LagArea::lag_of_port`: the LAG a front devport is a member of in the frontend's view.
    pub fn lag_of_port(&self, hw: u32) -> Option<u32> {
        self.lags
            .iter()
            .find(|(_, l)| l.members.iter().any(|m| u32::from(m.hw.0) == hw))
            .map(|(&tid, _)| tid)
    }

    /// `LagArea::dest_handle`: the IFCS destination of a backend dest: the sysport of a port, the
    /// LAG handle of a LAG; `None` for an unknown one. The accessor of the other areas (L2 entries
    /// and members, next hops, ACL redirects).
    pub fn dest_handle(&self, core: &Core, dest: Dest) -> Option<Handle> {
        match dest {
            Dest::Lag(tid) => self.lags.get(&tid.0).map(|l| l.h),
            Dest::Port(HwPort(p)) => core.sysport.get(&u32::from(p)).copied(),
        }
    }

    /// `LagArea::publish`: the callback map rebuilt (LAG handle and member sysports ->
    /// `lag_dest(tid)`), swapped in under its own lock.
    fn publish(&self, core: &Core) {
        let mut m = BTreeMap::new();
        for (&tid, l) in &self.lags {
            m.insert(l.h, lag_dest(tid));
            for hw in &l.in_ifcs {
                m.insert(core.sysport.get(hw).copied().unwrap_or_default(), lag_dest(tid));
            }
        }
        *self.dest_map.lock().unwrap_or_else(PoisonError::into_inner) = m;
    }

    /// `LagArea::apply_profile`: `SAI_TERALYNX_HASH_FIELDS` (unparsable: `INVALID_PARAMETER`) and
    /// `SAI_TERALYNX_HASH_SEED` (`strtoul`).
    pub fn apply_profile(&mut self, core: &Core) -> Result<()> {
        let f = core.settings.hash_fields.clone();
        if !f.is_empty() {
            let Some(fields) = parse_fields(&f) else {
                log::error(
                    ssys::SAI_API_SWITCH,
                    format_args!("SAI_TERALYNX_HASH_FIELDS: cannot parse '{f}'"),
                );
                return Err(Error::InvalidParameter);
            };
            self.hash_fields_set(core, &fields)?;
        }
        let s = &core.settings.hash_seed;
        if !s.is_empty() {
            return Self::hash_seed_set(core, crate::keys::strtoul_u32(s));
        }
        Ok(())
    }

    /// `LagArea::hash_fields_set`: per packet type with elements, a new extended field list on
    /// node `ENTROPY_SELECT_*`, the previous list of that type deleted.
    pub fn hash_fields_set(&mut self, core: &Core, fields: &[Field]) -> Result<()> {
        for (t, node_attr) in SELECT.iter().enumerate() {
            let el = elements(fields, t);
            if el.is_empty() {
                continue; // nothing for this packet type: the SDK's selection stays
            }
            let mut a = Attrs::new().u32_list(sys::IFCS_EXTENDED_FIELD_LIST_ATTR_ELEMENTS, el);
            let mut h = Handle::NULL;
            core.call(
                core.lib.ifcs_extended_field_list_create(NODE, &mut h, &mut a),
                "extended_field_list_create",
            )?;
            let mut n = Attrs::new().handle(*node_attr, h);
            if let Err(e) = core.call(core.lib.ifcs_node_attr_set(NODE, &mut n), "node entropy_select") {
                let _ = core.lib.ifcs_extended_field_list_delete(NODE, h);
                return Err(e);
            }
            if let Some(old) = self.hash_lists.get_mut(t) {
                if !old.is_null() {
                    let _ = core.call(
                        core.lib.ifcs_extended_field_list_delete(NODE, *old),
                        "extended_field_list_delete",
                    );
                }
                *old = h;
            }
        }
        Ok(())
    }

    /// `LagArea::hash_seed_set`: node `ENTROPY_SEED`.
    pub fn hash_seed_set(core: &Core, seed: u32) -> Result<()> {
        let mut a = Attrs::new().u32(sys::IFCS_NODE_ATTR_ENTROPY_SEED, seed);
        core.call(core.lib.ifcs_node_attr_set(NODE, &mut a), "node entropy_seed")
    }

    /// `LagArea::hash_set` (`Backend::hash_set`): `SAI_NATIVE_HASH_FIELD_*` of SWITCH `LAG_HASH` or
    /// `ECMP_HASH` and the seed. IFCS has one entropy for both: the last set wins (warned when LAG
    /// and ECMP disagree). A field without an IFCS element is refused (`INVALID_ATTR_VALUE_0`; not
    /// `NOT_SUPPORTED`, which means "no hash control" to the frontend); an empty list leaves the
    /// selection; seed 0 is written only once a seed was set.
    pub fn hash_set(&mut self, core: &Core, lag: bool, sai_fields: &[i32], seed: u32) -> Result<()> {
        let mut f = Vec::with_capacity(sai_fields.len());
        for &s in sai_fields {
            let Some(x) = sai_field(s) else {
                log::error(
                    ssys::SAI_API_HASH,
                    format_args!("hash field {s} has no IFCS entropy element here"),
                );
                return Err(Error::Attr(AttrError::InvalidValue, 0));
            };
            f.push(x);
        }
        let (mine, other) = if lag { (1, 0) } else { (0, 1) };
        if !f.is_empty() {
            if self
                .hash_sai
                .get(other)
                .is_some_and(|o| !o.is_empty() && o.as_slice() != sai_fields)
            {
                let (a, b) = if lag { ("LAG", "ECMP") } else { ("ECMP", "LAG") };
                log::warn(
                    ssys::SAI_API_HASH,
                    format_args!(
                        "{a} hash fields differ from the {b} ones: IFCS has one entropy for both, this set wins"
                    ),
                );
            }
            self.hash_fields_set(core, &f)?;
            if let Some(m) = self.hash_sai.get_mut(mine) {
                *m = sai_fields.to_vec();
            }
        }
        if seed != 0 || self.seed_set {
            Self::hash_seed_set(core, seed)?;
            self.seed_set = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_parse_and_map_as_tl_lag() {
        let f = parse_fields("src_ip,dst_ip,ip_protocol,l4_src_port,l4_dst_port,in_port").unwrap();
        assert_eq!(f.len(), 6);
        assert_eq!(
            parse_fields(" src_mac , ,dst_mac,").unwrap(),
            vec![Field::SrcMac, Field::DstMac]
        );
        assert!(parse_fields("src_ip,bogus").is_none());
        assert!(parse_fields(" , ").is_none());
        // IPv4 TCP/UDP: 6 elements; IPv4 other: no L4 ports; non-IP: in_port only
        assert_eq!(elements(&f, 0).len(), 6);
        assert_eq!(elements(&f, 1).len(), 4);
        assert_eq!(elements(&f, 2).first(), Some(&sys::IFCS_EXTENDED_FIELD_IPV6_SOURCE_IP));
        assert_eq!(elements(&f, 4), vec![sys::IFCS_EXTENDED_FIELD_SOURCE_SYSPORT]);
        assert_eq!(elements(&[Field::FlowLabel], 0), Vec::<u32>::new());
        assert_eq!(
            elements(&[Field::FlowLabel], 3),
            vec![sys::IFCS_EXTENDED_FIELD_IPV6_FLOW_LABEL]
        );
    }

    #[test]
    fn sai_hash_fields_as_hash_set() {
        let v = |x: u32| i32::try_from(x).unwrap();
        assert_eq!(sai_field(v(ssys::SAI_NATIVE_HASH_FIELD_SRC_IP)), Some(Field::SrcIp));
        assert_eq!(
            sai_field(v(ssys::SAI_NATIVE_HASH_FIELD_IPV6_FLOW_LABEL)),
            Some(Field::FlowLabel)
        );
        assert_eq!(sai_field(v(ssys::SAI_NATIVE_HASH_FIELD_IN_PORT)), Some(Field::InPort));
        assert_eq!(sai_field(v(ssys::SAI_NATIVE_HASH_FIELD_INNER_SRC_IP)), None);
        assert_eq!(sai_field(-1), None);
        // the IP fields of a non-IP packet type: nothing, so its selection stays
        assert_eq!(elements(&[Field::SrcIp, Field::DstIp], 4), Vec::<u32>::new());
        assert_eq!(elements(&[Field::SrcIp, Field::DstIp], 0).len(), 2);
    }

    #[test]
    fn members_in_ifcs_unless_both_disabled() {
        let m = |hw, e, i| LagMember {
            hw: HwPort(hw),
            egress_disable: e,
            ingress_disable: i,
        };
        let w = wanted(&[
            m(1, false, false),
            m(2, true, false),
            m(3, false, true),
            m(4, true, true),
        ]);
        assert_eq!(w.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(lag_dest(2), 0x4000_0002);
    }
}

//! LAGs and the switch hash (`tl_lag.{h,cpp}`). Ported in G1: what init does (`reset`, the
//! profile's hash fields and seed through the IFCS entropy: `apply_profile`, `hash_fields_set`,
//! `hash_seed_set`) and the destination map the L2 callback reads. LAG create / members and the
//! SAI hash objects are G2.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use ifcs::{Attrs, Handle, sys};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};

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

/// The LAG area's state (G1: the hash lists, the destination map).
#[derive(Debug, Default)]
pub struct LagArea {
    /// The extended field lists this backend made, per packet type (`hash_lists_`).
    pub hash_lists: [Handle; 5],
    /// The next trunk id (`next_tid_`).
    pub next_tid: i32,
    /// Shared with the L2 callback.
    pub dest_map: DestMap,
}

impl LagArea {
    /// `LagArea::reset` (a new switch): no LAGs, the old node's lists gone with it.
    pub fn reset(&mut self) {
        self.next_tid = 1;
        self.hash_lists = [Handle::NULL; 5];
        self.dest_map.lock().unwrap_or_else(PoisonError::into_inner).clear(); // publish() of no LAGs
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
}

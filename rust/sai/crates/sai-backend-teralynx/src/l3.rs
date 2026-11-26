//! L3 (`tl_l3.{h,cpp}`). Ported in G1: `pre_enable` (the RIF counter node setup before node
//! enable) and the counts the table usage reads (none while the area is not ported). Interfaces,
//! next hops, routes, hosts, ECMP and RIF counters are G2.
#![forbid(unsafe_code)]

use ifcs::{Attrs, sys};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};

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

/// The L3 area's state (G1: the ingress counter mode the node took).
#[derive(Debug)]
pub struct L3Area {
    /// The ingress L3INTF stats mode the node accepted (`in_mode_`, 1 until one is).
    pub in_mode: u32,
}

impl Default for L3Area {
    fn default() -> Self {
        Self { in_mode: 1 }
    }
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

    /// `L3Area::counts` (G1: no routes, hosts or ECMP groups exist).
    pub const fn counts(&self) -> Counts {
        Counts {
            v4_prefixes: 0,
            v6_prefixes: 0,
            v4_hosts: 0,
            v6_hosts: 0,
            ecmp_members: 0,
        }
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
}

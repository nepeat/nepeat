//! L3: virtual routers, router interfaces (a port RIF is an internal VLAN counting down from
//! `SAI_OPENBCM_RIF_VLAN_BASE`, default 4094), neighbors (an egress object + a host entry), next
//! hops (they reuse their neighbor's egress; the CPU egress until resolved), routes (CPU, drop,
//! egress or ECMP targets; VRF leaking), next hop groups (weights by repeating an egress; an
//! empty group has no SDK group and its routes drop), the router MAC as a my-station entry, RIF
//! counters and admin state. Ports `sai_l3.cpp` (1161 lines).
//!
//! Backend: [`crate::backend::L3`].
//!
//! Registration: [`crate::dispatch::Owner::L3`] for `VIRTUAL_ROUTER`, `ROUTER_INTERFACE`,
//! `NEXT_HOP`, `NEXT_HOP_GROUP`, `NEXT_HOP_GROUP_MEMBER` and the `NEIGHBOR_ENTRY` /
//! `ROUTE_ENTRY` entry types in [`crate::dispatch::OWNERS`] / `ENTRY_OWNERS`. The capacity
//! attributes then come from [`crate::backend::Base::table_usage`] (switch area).
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct L3Area;

impl Area for L3Area {}

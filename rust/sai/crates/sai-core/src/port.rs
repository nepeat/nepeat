//! Ports beyond the first slice: board quirks, counters, MTU, FEC / autoneg / loopback, the
//! ext-PHY and `MACsec` hooks around admin state. Ports `sai_port.cpp` (438 lines: `PortQuirks`
//! parse and apply at create and after a speed change, 15 counters through `PortStat` plus
//! `port_stat_by_id` and PFC, MTU 68..=9408 as `frame_max = MTU + 8`, the set / get fall-through
//! to the qos, storm, mirror and macsec handlers, 14 stubs).
//!
//! Backend: [`crate::backend::PortOps`] and the port operations still in [`crate::backend::Backend`]
//! (`port_admin_set`, `port_link_get`, `port_speed_*`, `port_supported_speeds`); profile keys
//! `SAI_OPENBCM_PORT_{INTERFACE,FEC,AUTONEG,IFG}`.
//!
//! Registration: [`crate::dispatch::Owner::Port`] has no type yet. `PORT` is
//! [`crate::dispatch::Owner::Core`] while its code is in `sai.rs` (`set` / `get_port`,
//! `PORT_KEPT`, `PORT_BACKEND`); the port area moves that code here and switches the `PORT`
//! line of [`crate::dispatch::OWNERS`] (and the `AnyId::Port` arms of `sai.rs` with it: an
//! orchestrator edit). Hooks: [`crate::extphy`] (admin up / down sequences), [`crate::macsec`]
//! (IFG follows `MACsec` state).
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct PortArea;

impl Area for PortArea {}

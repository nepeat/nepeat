//! The typed backend operations: one Rust method per C++ `ob::Backend` operation
//! (`sai-openbcm/src/backend.h`), grouped in sub-traits as there; [`super::Backend`] requires
//! them all. The map from each C++ operation to its method is the scratchpad's
//! `backend-op-map.md`.
//!
//! - **Defaults.** A default body answers `NotSupported` (or `false` for a probe) where the C++
//!   has one (an optional feature: a backend that lacks it says so, the SAI layer keeps the
//!   object and programs nothing, as with the C++). The operations the frontend calls that are
//!   pure virtual in the C++ are `required`: no default, so every backend says what it does
//!   (`sai-backend-openbcm` answers the ones it has not ported with an explicit `NotSupported`
//!   marked `G2`, `src/g2.rs`). The body written after a `required` one is the test recorder's.
//! - **Capability probes** as in the C++: [`Qos::qos_supported`], [`Mirror::mirror_supported`],
//!   [`Hash::hash_supported`], [`Field::fp_supported`], [`PortOps::port_stat_by_id`],
//!   [`L3::l3_intf_stat_supported`].
//! - **Typed arguments**: ids of each kind are their own type ([`super::types`]); a C++ `-1`
//!   ("none") is an `Option`; output parameters are return values.
//! - **Not here yet** (they live in [`super::Backend`] with their C++ meaning until its
//!   generic methods are removed, then move into [`Base`], [`PortOps`] and a `Vlan` trait):
//!   `name`, `init`, `shutdown`, `cpu_port`, `ports`, `port_admin_set`, `port_link_get`,
//!   `port_speed_set`, `port_speed_get`, `port_supported_speeds` and the five VLAN operations.
//!
//! Each trait is written once, in the macro below, which also writes its forwarding impl for
//! `Box<B>` and its logging impl for [`super::recording::Recording`]: an operation cannot be added
//! to one and forgotten in the others. Argument names are the C++ parameter names (the call log
//! prints them).
use super::types::{
    AsicTemps, BufferCfg, ChipLimits, Dest, EcmpId, EgressId, Fec, FilterId, FloodKind, FpCap, FpCounter, FpEntryId,
    FpGroupExtra, FpGroupId, FpQual, FpRule, GenlId, IntfId, IpPrefix, LagMember, Learn, LinkGate, Loopback, MacAddr,
    MdioDev, MirrorCfg, MirrorId, NetifCreated, NetifId, PhyAddr, PolicerCfg, PolicerId, PortInterface, PortStat,
    QosMapEntry, QosMapId, QosMapKind, RouteTarget, SampleId, SchedCfg, StationId, StormKind, TableUsage, TrapKind,
    TrapSpec, TrunkId, Vrf, WredCfg, WredId,
};
use super::{HwPort, Vid};
use crate::error::{Error, Result};
use sai_sys as sys;
use std::net::IpAddr;
use std::os::fd::BorrowedFd;

/// Writes a sub-trait, its `Box<B>` forwarding impl and its recording impl. Each operation:
/// `fn op(&self | &mut self, args) -> Ret, <log kind>(<C++ output names>) { default body }`.
macro_rules! backend_ops {
    (
        $(#[$tmeta:meta])*
        pub trait $tr:ident { $($items:tt)* }
    ) => {
        backend_ops!(@munch [$(#[$tmeta])*] $tr [] $($items)*);
    };
    (@munch [$($tm:tt)*] $tr:ident [$($done:tt)*]
        $(#[$m:meta])*
        fn $op:ident(&mut $slf:ident $(, $a:ident: $t:ty)* $(,)?) -> $ret:ty, $kind:ident($($outs:tt)*) $body:block
        $($rest:tt)*
    ) => {
        backend_ops!(@munch [$($tm)*] $tr [$($done)* {[$(#[$m])*] [mut] $slf $op [$($a: $t),*] $ret, $kind($($outs)*) $body $body}] $($rest)*);
    };
    (@munch [$($tm:tt)*] $tr:ident [$($done:tt)*]
        $(#[$m:meta])*
        fn $op:ident(& $slf:ident $(, $a:ident: $t:ty)* $(,)?) -> $ret:ty, $kind:ident($($outs:tt)*) $body:block
        $($rest:tt)*
    ) => {
        backend_ops!(@munch [$($tm)*] $tr [$($done)* {[$(#[$m])*] [] $slf $op [$($a: $t),*] $ret, $kind($($outs)*) $body $body}] $($rest)*);
    };
    // `required`: no default in the trait (the body is the test recorder's answer only)
    (@munch [$($tm:tt)*] $tr:ident [$($done:tt)*]
        $(#[$m:meta])*
        required fn $op:ident(&mut $slf:ident $(, $a:ident: $t:ty)* $(,)?) -> $ret:ty, $kind:ident($($outs:tt)*) $body:block
        $($rest:tt)*
    ) => {
        backend_ops!(@munch [$($tm)*] $tr [$($done)* {[$(#[$m])*] [mut] $slf $op [$($a: $t),*] $ret, $kind($($outs)*) ; $body}] $($rest)*);
    };
    (@munch [$($tm:tt)*] $tr:ident [$($done:tt)*]
        $(#[$m:meta])*
        required fn $op:ident(& $slf:ident $(, $a:ident: $t:ty)* $(,)?) -> $ret:ty, $kind:ident($($outs:tt)*) $body:block
        $($rest:tt)*
    ) => {
        backend_ops!(@munch [$($tm)*] $tr [$($done)* {[$(#[$m])*] [] $slf $op [$($a: $t),*] $ret, $kind($($outs)*) ; $body}] $($rest)*);
    };
    (@munch [$($tm:tt)*] $tr:ident [$({[$($m:tt)*] [$($mu:tt)?] $slf:ident $op:ident [$($a:ident: $t:ty),*] $ret:ty, $kind:ident($($outs:tt)*) $tail:tt $body:block})*]) => {
        $($tm)*
        pub trait $tr {
            $(
                $($m)*
                #[allow(unused_variables, reason = "a default body may ignore its arguments")]
                fn $op(& $($mu)? $slf $(, $a: $t)*) -> $ret $tail
            )*
        }

        impl<B: $tr + ?Sized> $tr for Box<B> {
            $(
                fn $op(& $($mu)? $slf $(, $a: $t)*) -> $ret {
                    (**$slf).$op($($a),*)
                }
            )*
        }

        impl<B: super::Backend> $tr for super::recording::Recording<B> {
            $(
                fn $op(& $($mu)? $slf $(, $a: $t)*) -> $ret {
                    record!($slf, $op, $kind($($outs)*), [$($a),*])
                }
            )*
        }

        impl<B: super::Backend> $tr for super::Refusing<B> {
            $(
                #[allow(unused_variables, reason = "the refusal ignores its arguments")]
                fn $op(& $($mu)? $slf $(, $a: $t)*) -> $ret $body
            )*
        }

        #[cfg(test)]
        impl $tr for super::OpNames {
            $(
                #[allow(unused_variables, reason = "the default body may ignore its arguments")]
                fn $op(& $($mu)? $slf $(, $a: $t)*) -> $ret {
                    $slf.0.borrow_mut().push(stringify!($op));
                    $body
                }
            )*
        }
    };
}

/// The names of the typed operations, in declaration order (for fault injection by name and
/// for the op map).
pub const OP_NAMES: &[&str] = &[
    "is_fake",
    "hardware_info",
    "table_usage",
    "chip_limits",
    "run_shell",
    "run_shell_to",
    "asic_temperatures",
    "mdio_c45_read",
    "mdio_c45_write",
    "port_link_gate",
    "port_frame_max_set",
    "port_fec_set",
    "port_autoneg_set",
    "port_interface_set",
    "port_ifg_set",
    "port_loopback_set",
    "port_pvid_set",
    "port_learn_set",
    "port_stat_get",
    "port_stat_clear",
    "port_stat_by_id",
    "port_stats_by_id",
    "port_pfc_set",
    "port_pfc_stat_get",
    "lag_create",
    "lag_destroy",
    "lag_set_members",
    "l2_add",
    "l2_delete",
    "l2_flush",
    "l2_age_set",
    "netif_create",
    "netif_destroy",
    "netif_carrier_set",
    "netif_set_keep_tag",
    "trap_set",
    "trap_program",
    "genetlink_create",
    "genetlink_destroy",
    "genetlink_trap_set",
    "l3_intf_create",
    "l3_intf_update",
    "l3_intf_destroy",
    "l3_station_add",
    "l3_station_delete",
    "l3_egress_create",
    "l3_egress_update",
    "l3_egress_destroy",
    "l3_egress_cpu",
    "l3_egress_drop",
    "l3_host_add",
    "l3_host_delete",
    "l3_route_add",
    "l3_route_delete",
    "l3_ecmp_create",
    "l3_ecmp_update",
    "l3_ecmp_destroy",
    "l3_intf_stat_supported",
    "l3_intf_stats",
    "l3_intf_admin_set",
    "l3_egress_glean",
    "fp_group_create",
    "fp_group_destroy",
    "fp_entry_create",
    "fp_entry_destroy",
    "fp_entry_counter",
    "fp_supported",
    "fp_group_create_ex",
    "fp_entry_update",
    "fp_entry_enable",
    "fp_group_free",
    "policer_create",
    "policer_set",
    "policer_destroy",
    "qos_supported",
    "qos_map_create",
    "qos_map_update",
    "qos_map_destroy",
    "port_qos_map_set",
    "port_default_tc_set",
    "queue_scheduler_set",
    "port_scheduler_set",
    "wred_create",
    "wred_update",
    "wred_destroy",
    "queue_wred_set",
    "queue_buffer_set",
    "pg_buffer_set",
    "queue_stats_get",
    "queue_stats_clear",
    "pg_stats_get",
    "pg_stats_clear",
    "pool_stats_get",
    "mirror_supported",
    "mirror_create",
    "mirror_update",
    "mirror_destroy",
    "port_mirror_set",
    "sample_create",
    "sample_update",
    "sample_destroy",
    "port_sample_set",
    "hash_supported",
    "hash_set",
    "port_storm_set",
    "vlan_flood_set",
];

backend_ops! {
    /// Lifecycle and chip facts (`ob::Backend` "lifecycle/misc"; `name`, `init`, `shutdown`,
    /// `cpu_port` and `ports` are still in [`super::Backend`]).
    pub trait Base {
        /// Whether this is the in-memory model (C++ `is_fake`; `sai_switch.cpp` refuses the fake
        /// next to a real chip). Required (the switch area uses it).
        required fn is_fake(&self) -> bool, query() {
            false
        }
        /// A description of the chip for logs (the frontend does not ask for it yet).
        fn hardware_info(&self) -> String, query() {
            String::new()
        }
        /// Free entries in the forwarding tables (the switch's `AVAILABLE_*` attributes). Required
        /// (the switch area uses it) (it replaced [`super::Backend::limits`]).
        required fn table_usage(&self) -> Result<TableUsage>, status(u) {
            Err(Error::NotSupported)
        }
        /// Fixed properties of the chip. Required (the switch area uses it).
        required fn chip_limits(&self) -> Result<ChipLimits>, query_status(l) {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// The SDK's diagnostic shell (the diag socket's `shell` commands, `sai_diag.cpp`).
    pub trait Shell {
        /// Runs a diag shell command; its output goes wherever the SDK prints (the diag
        /// socket uses [`Shell::run_shell_to`]).
        fn run_shell(&mut self, cmd: &str) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The same with the output written to `fd` (a diag socket connection). The SDK prints
        /// to the process's stdout: a backend redirects it for the call (the C glue does). Required
        /// (the diag area uses it).
        required fn run_shell_to(&mut self, cmd: &str, fd: BorrowedFd<'_>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// ASIC temperatures (`sai_temp.cpp`: the switch's temperature attributes and the publisher).
    pub trait Temp {
        /// The ASIC's die temperature monitors, tenths of a degree C: current, and the peak since
        /// the previous read. The C++ publisher calls it without the SAI lock (the backend is
        /// thread-safe for it); here it takes `&self`, and a publisher thread needs a handle
        /// the backend hands out (open for the temperature area). Required (the
        /// temperature area uses it).
        required fn asic_temperatures(&self) -> Result<AsicTemps>, outs(cur, peak) {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Raw clause-45 MDIO through the switch chip's MDIO master (the external PHY and `MACsec`
    /// tables). Clause 22 cannot be expressed: the device is an [`MdioDev`] (1..=31).
    pub trait Mdio {
        /// Reads a register.
        fn mdio_c45_read(&self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16>, status(val) {
            Err(Error::NotSupported)
        }
        /// Writes a register.
        fn mdio_c45_write(&self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Port operations beyond the five in [`super::Backend`] (`sai_port.cpp`, `sai_extphy.cpp`).
    pub trait PortOps {
        /// Sets (`Some`) or removes (`None`) the port's link gate, asked on every link scan pass
        /// before the ASIC's link read ([`LinkGate`]: its rules).
        fn port_link_gate(&mut self, hw: HwPort, gate: Option<LinkGate>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The largest frame, bytes (the SAI layer passes MTU + 8). Required (the port area
        /// uses it).
        required fn port_frame_max_set(&mut self, hw: HwPort, bytes: u32) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// FEC. Required (the port area uses it).
        required fn port_fec_set(&mut self, hw: HwPort, fec: Fec) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Autonegotiation. Required (the port area uses it).
        required fn port_autoneg_set(&mut self, hw: HwPort, on: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Board quirk: the interface type (`SAI_OPENBCM_PORT_INTERFACE`).
        fn port_interface_set(&mut self, hw: HwPort, name: PortInterface) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Board quirk: the average inter-frame gap in bit times (`SAI_OPENBCM_PORT_IFG`; the
        /// `MACsec` area changes it too).
        fn port_ifg_set(&mut self, hw: HwPort, bits: u32) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Loopback. Required (the port area uses it).
        required fn port_loopback_set(&mut self, hw: HwPort, lb: Loopback) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The port VLAN id. Required (the L2 area uses it).
        required fn port_pvid_set(&mut self, hw: HwPort, vid: Vid) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Address learning. Required (the L2 area uses it).
        required fn port_learn_set(&mut self, hw: HwPort, mode: Learn) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// One port counter. Required (the port area uses it) (it replaced
        /// [`super::Backend::stats_get`] for ports).
        required fn port_stat_get(&self, hw: HwPort, stat: PortStat) -> Result<u64>, status(v) {
            Err(Error::NotSupported)
        }
        /// Zeroes every counter of the port. Required (the port area uses it).
        required fn port_stat_clear(&mut self, hw: HwPort) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Probe: whether counter `id` (`SAI_PORT_STAT_*`) comes from
        /// [`PortOps::port_stats_by_id`] instead of [`PortOps::port_stat_get`].
        fn port_stat_by_id(&self, id: sys::sai_stat_id_t) -> bool, query() {
            false
        }
        /// Counters by SAI id (those [`PortOps::port_stat_by_id`] answers yes for).
        fn port_stats_by_id(&self, hw: HwPort, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>>, status(values) {
            Err(Error::NotSupported)
        }
        /// PFC per priority bit: combined mode passes `tx == rx` (qos only).
        fn port_pfc_set(&mut self, hw: HwPort, separate: bool, tx: u8, rx: u8) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// PFC frames of one priority (`SAI_PORT_STAT_PFC_<prio>_RX_PKTS` / `TX_PKTS`).
        fn port_pfc_stat_get(&self, hw: HwPort, prio: u8, tx: bool) -> Result<u64>, status(v) {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// LAGs as SDK trunks (`sai_lag.cpp`). The SAI layer applies VLAN membership, PVID and
    /// learning to every member port itself.
    pub trait Lag {
        /// A trunk without members. Required (the LAG area uses it).
        required fn lag_create(&mut self) -> Result<TrunkId>, status(tid) {
            Err(Error::NotSupported)
        }
        /// Destroys a trunk. Required (the LAG area uses it).
        required fn lag_destroy(&mut self, tid: TrunkId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Replaces the whole member list. Required (the LAG area uses it).
        required fn lag_set_members(&mut self, tid: TrunkId, members: &[LagMember]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// The L2 table (`sai_l2.cpp`: FDB entries, flush, aging). Learned and aged addresses come
    /// back as [`super::BackendEvent::Fdb`].
    pub trait L2 {
        /// Adds or replaces an entry. Required (the L2 area uses it).
        required fn l2_add(&mut self, mac: MacAddr, vid: Vid, hw: Dest, is_static: bool, discard: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Deletes an entry. Required (the L2 area uses it).
        required fn l2_delete(&mut self, mac: MacAddr, vid: Vid) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Deletes the entries of a destination and / or VLAN (`None`: any), static and / or
        /// dynamic ones. Required (the L2 area uses it).
        required fn l2_flush(&mut self, hw: Option<Dest>, vid: Option<Vid>, statics: bool, dynamics: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The aging time in seconds. Required (the L2 area uses it).
        required fn l2_age_set(&mut self, seconds: u32) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Host interfaces: KNET netdevs, traps, generic netlink (`sai_hostif.cpp`).
    pub trait Hostif {
        /// One Linux netdev for a port (TX straight to the port, RX through a filter on the
        /// ingress port). Required (the hostif area uses it).
        required fn netif_create(&mut self, ifname: &str, hw: HwPort, vid: Vid, mac: MacAddr, keep_tag: bool) -> Result<NetifCreated>, outs(netif_id, filter_id) {
            Err(Error::NotSupported)
        }
        /// Removes a netdev and its filter. Required (the hostif area uses it).
        required fn netif_destroy(&mut self, netif_id: NetifId, filter_id: FilterId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The netdev's carrier (the host interface's `OPER_STATUS`). Required (the
        /// hostif area uses it).
        required fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Re-creates the RX filter with / without VLAN tag stripping (the netdev stays) and
        /// returns the new filter; `filter_id` is the old one, which the backend destroys (C++:
        /// an in/out parameter, logged on both sides; the SAI layer passes `None`, as the C++
        /// passes -1). Required (the hostif area uses it).
        required fn netif_set_keep_tag(&mut self, netif_id: NetifId, hw: HwPort, keep_tag: bool, filter_id: Option<FilterId>) -> Result<FilterId>, status(filter_id) {
            Err(Error::NotSupported)
        }
        /// Steers a protocol to the CPU or stops doing so; `NotSupported` for a kind the
        /// backend has no control for. Required (the hostif area uses it).
        required fn trap_set(&mut self, kind: TrapKind, to_cpu: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Optional: a backend that steers, queues and polices a trap itself programs it here
        /// (`remove`: takes it away), for every change of the trap, its group or the group's
        /// policer. `NotSupported` keeps the `trap_set` + `CoPP` field entry path.
        fn trap_program(&mut self, t: &TrapSpec, remove: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A generic-netlink host interface (`sFlow` samples to `psample`).
        fn genetlink_create(&mut self, name: &str, mcgrp: &str) -> Result<GenlId>, status(id) {
            Err(Error::NotSupported)
        }
        /// Removes it.
        fn genetlink_destroy(&mut self, id: GenlId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Sends (`add`) or stops sending the packets of a trap type there (`HOSTIF_TABLE_ENTRY`
        /// channel `GENETLINK`).
        fn genetlink_trap_set(&mut self, id: GenlId, trap_type: i32, add: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// L3 in the SDK's egress mode (`sai_l3.cpp`): interfaces bind a VLAN to a VRF; host and
    /// route entries are per VRF, egress objects and ECMP groups are not.
    pub trait L3 {
        /// An L3 interface. Required (the L3 area uses it).
        required fn l3_intf_create(&mut self, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<IntfId>, status(intf_id) {
            Err(Error::NotSupported)
        }
        /// Changes one. Required (the L3 area uses it).
        required fn l3_intf_update(&mut self, intf_id: IntfId, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the L3 area uses it).
        required fn l3_intf_destroy(&mut self, intf_id: IntfId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A my-station entry (the router MAC). Required (the L3 area uses it).
        required fn l3_station_add(&mut self, mac: MacAddr) -> Result<StationId>, status(station_id) {
            Err(Error::NotSupported)
        }
        /// Deletes one. Required (the L3 area uses it).
        required fn l3_station_delete(&mut self, station_id: StationId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// An egress object (a neighbor). Required (the L3 area uses it).
        required fn l3_egress_create(&mut self, intf_id: IntfId, dmac: MacAddr, hw: Dest, vid: Vid) -> Result<EgressId>, status(egress_id) {
            Err(Error::NotSupported)
        }
        /// Changes one. Required (the L3 area uses it).
        required fn l3_egress_update(&mut self, egress_id: EgressId, intf_id: IntfId, dmac: MacAddr, hw: Dest, vid: Vid) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the L3 area uses it).
        required fn l3_egress_destroy(&mut self, egress_id: EgressId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The shared CPU egress (never destroyed). Required (the L3 area uses it).
        required fn l3_egress_cpu(&mut self) -> Result<EgressId>, status(egress_id) {
            Err(Error::NotSupported)
        }
        /// The shared drop egress (never destroyed). Required (the L3 area uses it).
        required fn l3_egress_drop(&mut self) -> Result<EgressId>, status(egress_id) {
            Err(Error::NotSupported)
        }
        /// A host entry. Required (the L3 area uses it).
        required fn l3_host_add(&mut self, vrf: Vrf, ip: IpAddr, egress_id: EgressId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Deletes one. Required (the L3 area uses it).
        required fn l3_host_delete(&mut self, vrf: Vrf, ip: IpAddr) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A route to an egress object or an ECMP group (`replace`: it exists and is changed).
        /// Required (the L3 area uses it).
        required fn l3_route_add(&mut self, vrf: Vrf, p: IpPrefix, target: RouteTarget, replace: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Deletes one. Required (the L3 area uses it).
        required fn l3_route_delete(&mut self, vrf: Vrf, p: IpPrefix) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// An ECMP group over egress objects (a repeated one weighs more; never empty). Required
        /// (the L3 area uses it).
        required fn l3_ecmp_create(&mut self, egress: &[EgressId]) -> Result<EcmpId>, status(ecmp_id) {
            Err(Error::NotSupported)
        }
        /// Replaces its members. Required (the L3 area uses it).
        required fn l3_ecmp_update(&mut self, ecmp_id: EcmpId, egress: &[EgressId]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the L3 area uses it).
        required fn l3_ecmp_destroy(&mut self, ecmp_id: EcmpId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Probe: whether router interface counter `id` (`SAI_ROUTER_INTERFACE_STAT_*`) exists.
        fn l3_intf_stat_supported(&self, id: sys::sai_stat_id_t) -> bool, query() {
            false
        }
        /// Router interface counters (`clear`: read and reset).
        fn l3_intf_stats(&mut self, intf_id: IntfId, ids: &[sys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>>, status(values) {
            Err(Error::NotSupported)
        }
        /// RIF `ADMIN_V4_STATE` / `ADMIN_V6_STATE` (`NotSupported`: the SAI layer only stores them).
        fn l3_intf_admin_set(&mut self, intf_id: IntfId, v4: bool, v6: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The shared CPU egress of glean (routes to a connected subnet, next hops without a
        /// neighbor). Default: the CPU egress, as the C++.
        fn l3_egress_glean(&mut self) -> Result<EgressId>, status(egress_id) {
            self.l3_egress_cpu()
        }
    }
}

backend_ops! {
    /// The ingress field processor (`sai_acl.cpp`: ACL tables and entries, `CoPP`). A group is
    /// one SDK field group with a qualifier set; an entry is installed at create (destroyed and
    /// re-created to change it, unless [`FpCap::EntryUpdate`]).
    pub trait Field {
        /// A group; `priority` orders groups (higher wins on conflicting actions). Required (the
        /// ACL area uses it).
        required fn fp_group_create(&mut self, qset: &[FpQual], priority: i32) -> Result<FpGroupId>, status(gid) {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the ACL area uses it).
        required fn fp_group_destroy(&mut self, gid: FpGroupId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// An entry. Required (the ACL area uses it).
        required fn fp_entry_create(&mut self, gid: FpGroupId, r: &FpRule) -> Result<FpEntryId>, status(eid) {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the ACL area uses it).
        required fn fp_entry_destroy(&mut self, eid: FpEntryId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Its counter. Required (the ACL area uses it).
        required fn fp_entry_counter(&self, eid: FpEntryId) -> Result<FpCounter>, outs(packets, bytes) {
            Err(Error::NotSupported)
        }
        /// Probe: an optional field-processor feature.
        fn fp_supported(&self, cap: FpCap) -> bool, query() {
            false
        }
        /// A group with what it needs beyond its qualifiers. Default: refused when anything is
        /// asked for, else [`Field::fp_group_create`], as the C++.
        fn fp_group_create_ex(&mut self, qset: &[FpQual], priority: i32, x: FpGroupExtra) -> Result<FpGroupId>, status(gid) {
            if x.any() { Err(Error::NotSupported) } else { self.fp_group_create(qset, priority) }
        }
        /// [`FpCap::EntryUpdate`]: changes an installed entry in place (its counter keeps counting).
        fn fp_entry_update(&mut self, eid: FpEntryId, r: &FpRule) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// [`FpCap::EntryEnable`]: switches an entry off / on without removing it.
        fn fp_entry_enable(&mut self, eid: FpEntryId, on: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Free entries in a group (`ACL_TABLE` `AVAILABLE_ACL_ENTRY`).
        fn fp_group_free(&self, gid: FpGroupId) -> Result<u32>, status(entries) {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Meters (`sai_acl.cpp` policers; one may serve several entries).
    pub trait Policer {
        /// A meter. Required (the ACL area uses it).
        required fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<PolicerId>, status(pid) {
            Err(Error::NotSupported)
        }
        /// Changes one. Required (the ACL area uses it).
        required fn policer_set(&mut self, pid: PolicerId, cfg: &PolicerCfg) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one. Required (the ACL area uses it).
        required fn policer_destroy(&mut self, pid: PolicerId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// `QoS` maps, schedulers, WRED, buffers and queue / priority group counters (`sai_qos.cpp`).
    /// Optional as a whole: with [`Qos::qos_supported`] false the SAI layer keeps these objects,
    /// programs nothing and makes none of these calls. Queues and priority groups: 0..=7 per port.
    pub trait Qos {
        /// Probe: the `QoS` model is programmed.
        fn qos_supported(&self) -> bool, query() {
            false
        }
        /// A map (the SAI layer passes complete tables).
        fn qos_map_create(&mut self, kind: QosMapKind, entries: &[QosMapEntry]) -> Result<QosMapId>, status(id) {
            Err(Error::NotSupported)
        }
        /// Replaces a map's entries.
        fn qos_map_update(&mut self, id: QosMapId, entries: &[QosMapEntry]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys a map.
        fn qos_map_destroy(&mut self, id: QosMapId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A port's map of one kind (`None`: the default).
        fn port_qos_map_set(&mut self, hw: HwPort, kind: QosMapKind, id: Option<QosMapId>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A port's default traffic class.
        fn port_default_tc_set(&mut self, hw: HwPort, tc: u8) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A queue's scheduler (`None`: back to DWRR weight 1, no shaping).
        fn queue_scheduler_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&SchedCfg>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A port's shaper (`None`: none).
        fn port_scheduler_set(&mut self, hw: HwPort, cfg: Option<&SchedCfg>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A WRED profile.
        fn wred_create(&mut self, cfg: &WredCfg) -> Result<WredId>, status(id) {
            Err(Error::NotSupported)
        }
        /// Changes one.
        fn wred_update(&mut self, id: WredId, cfg: &WredCfg) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one.
        fn wred_destroy(&mut self, id: WredId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A queue's WRED profile (`None`: off).
        fn queue_wred_set(&mut self, hw: HwPort, queue: u8, id: Option<WredId>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A queue's buffer profile (`None`: the default).
        fn queue_buffer_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&BufferCfg>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// An ingress priority group's buffer profile (`None`: the default).
        fn pg_buffer_set(&mut self, hw: HwPort, pg: u8, cfg: Option<&BufferCfg>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Queue counters by SAI id (`SAI_QUEUE_STAT_*`; `NotSupported` for one it lacks).
        fn queue_stats_get(&self, hw: HwPort, queue: u8, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>>, status(values) {
            Err(Error::NotSupported)
        }
        /// Zeroes queue counters.
        fn queue_stats_clear(&mut self, hw: HwPort, queue: u8, ids: &[sys::sai_stat_id_t]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Priority group counters by SAI id (`SAI_INGRESS_PRIORITY_GROUP_STAT_*`).
        fn pg_stats_get(&self, hw: HwPort, pg: u8, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>>, status(values) {
            Err(Error::NotSupported)
        }
        /// Zeroes priority group counters.
        fn pg_stats_clear(&mut self, hw: HwPort, pg: u8, ids: &[sys::sai_stat_id_t]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Buffer pool counters by SAI id (`SAI_BUFFER_POOL_STAT_*`), the ingress or egress pool.
        fn pool_stats_get(&self, ingress: bool, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>>, status(values) {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Mirror sessions and port mirroring (`sai_mirror.cpp`). Optional like `QoS`: with
    /// [`Mirror::mirror_supported`] false the SAI layer keeps the sessions.
    pub trait Mirror {
        /// Probe: mirroring is programmed.
        fn mirror_supported(&self) -> bool, query() {
            false
        }
        /// A session.
        fn mirror_create(&mut self, cfg: &MirrorCfg) -> Result<MirrorId>, status(id) {
            Err(Error::NotSupported)
        }
        /// Changes one.
        fn mirror_update(&mut self, id: MirrorId, cfg: &MirrorCfg) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one.
        fn mirror_destroy(&mut self, id: MirrorId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// The sessions a port mirrors to in one direction (empty: none).
        fn port_mirror_set(&mut self, hw: HwPort, ingress: bool, ids: &[MirrorId]) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// `SAMPLEPACKET`: 1 in `rate` frames to the CPU (`sai_mirror.cpp`).
    pub trait Sample {
        /// A sampler.
        fn sample_create(&mut self, rate: u32) -> Result<SampleId>, status(id) {
            Err(Error::NotSupported)
        }
        /// Changes its rate.
        fn sample_update(&mut self, id: SampleId, rate: u32) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// Destroys one.
        fn sample_destroy(&mut self, id: SampleId) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// A port's sampler in one direction (`None`: off).
        fn port_sample_set(&mut self, hw: HwPort, ingress: bool, id: Option<SampleId>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// The LAG and ECMP hash (`sai_hash.cpp`).
    pub trait Hash {
        /// Probe: the switch's `LAG_HASH` / `ECMP_HASH` objects are programmed (else those
        /// attributes stay unsupported and the seeds are kept only).
        fn hash_supported(&self) -> bool, query() {
            false
        }
        /// The LAG (`lag`) or ECMP hash: `SAI_NATIVE_HASH_FIELD_*` values and the seed.
        fn hash_set(&mut self, lag: bool, fields: &[i32], seed: u32) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

backend_ops! {
    /// Storm control and VLAN flood control (`sai_hash.cpp`; `NotSupported`: the SAI layer
    /// keeps the attribute without effect).
    pub trait Storm {
        /// A policer (`None`: off) on one kind of a port's traffic.
        fn port_storm_set(&mut self, hw: HwPort, kind: StormKind, policer: Option<PolicerId>) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
        /// VLAN flooding of one kind on / off.
        fn vlan_flood_set(&mut self, vid: Vid, kind: FloodKind, on: bool) -> Result<()>, status() {
            Err(Error::NotSupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::OpNames;

    /// Calls every typed operation once through the recording names and checks that
    /// [`OP_NAMES`] lists exactly them, in declaration order.
    #[test]
    #[allow(clippy::too_many_lines, reason = "one call per operation")]
    fn op_names_are_the_operations() {
        let mut n = OpNames::default();
        let fd = std::io::stdin();
        let fd = std::os::fd::AsFd::as_fd(&fd);
        let p = IpPrefix::new(
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
        )
        .unwrap();
        let hw = HwPort(1);
        let vid = Vid::DEFAULT;
        let mac = MacAddr::default();
        let dev = MdioDev::new(1).unwrap();
        let _ = n.is_fake();
        let _ = n.hardware_info();
        let _ = n.table_usage();
        let _ = n.chip_limits();
        let _ = n.run_shell("x");
        let _ = n.run_shell_to("x", fd);
        let _ = n.asic_temperatures();
        let _ = n.mdio_c45_read(PhyAddr(0), dev, 0);
        let _ = n.mdio_c45_write(PhyAddr(0), dev, 0, 0);
        let _ = n.port_link_gate(hw, None);
        let _ = n.port_frame_max_set(hw, 0);
        let _ = n.port_fec_set(hw, Fec::None);
        let _ = n.port_autoneg_set(hw, false);
        let _ = n.port_interface_set(hw, PortInterface::Kr);
        let _ = n.port_ifg_set(hw, 0);
        let _ = n.port_loopback_set(hw, Loopback::None);
        let _ = n.port_pvid_set(hw, vid);
        let _ = n.port_learn_set(hw, Learn::Hw);
        let _ = n.port_stat_get(hw, PortStat::InOctets);
        let _ = n.port_stat_clear(hw);
        let _ = n.port_stat_by_id(0);
        let _ = n.port_stats_by_id(hw, &[]);
        let _ = n.port_pfc_set(hw, false, 0, 0);
        let _ = n.port_pfc_stat_get(hw, 0, false);
        let _ = n.lag_create();
        let _ = n.lag_destroy(TrunkId(0));
        let _ = n.lag_set_members(TrunkId(0), &[]);
        let _ = n.l2_add(mac, vid, Dest::Port(hw), false, false);
        let _ = n.l2_delete(mac, vid);
        let _ = n.l2_flush(None, None, false, false);
        let _ = n.l2_age_set(0);
        let _ = n.netif_create("x", hw, vid, mac, false);
        let _ = n.netif_destroy(NetifId(0), FilterId(0));
        let _ = n.netif_carrier_set("x", false);
        let _ = n.netif_set_keep_tag(NetifId(0), hw, false, None);
        let _ = n.trap_set(TrapKind::Lldp, false);
        let _ = n.trap_program(
            &TrapSpec {
                trap_type: 0,
                action: 0,
                priority: 0,
                queue: 0,
                group_up: true,
                policer: None,
                yellow_drop: false,
                red_drop: false,
            },
            false,
        );
        let _ = n.genetlink_create("x", "y");
        let _ = n.genetlink_destroy(GenlId(0));
        let _ = n.genetlink_trap_set(GenlId(0), 0, false);
        let _ = n.l3_intf_create(mac, vid, 0, Vrf::DEFAULT);
        let _ = n.l3_intf_update(IntfId(0), mac, vid, 0, Vrf::DEFAULT);
        let _ = n.l3_intf_destroy(IntfId(0));
        let _ = n.l3_station_add(mac);
        let _ = n.l3_station_delete(StationId(0));
        let _ = n.l3_egress_create(IntfId(0), mac, Dest::Port(hw), vid);
        let _ = n.l3_egress_update(EgressId(0), IntfId(0), mac, Dest::Port(hw), vid);
        let _ = n.l3_egress_destroy(EgressId(0));
        let _ = n.l3_egress_cpu();
        let _ = n.l3_egress_drop();
        let _ = n.l3_host_add(Vrf::DEFAULT, p.addr(), EgressId(0));
        let _ = n.l3_host_delete(Vrf::DEFAULT, p.addr());
        let _ = n.l3_route_add(Vrf::DEFAULT, p, RouteTarget::Egress(EgressId(0)), false);
        let _ = n.l3_route_delete(Vrf::DEFAULT, p);
        let _ = n.l3_ecmp_create(&[]);
        let _ = n.l3_ecmp_update(EcmpId(0), &[]);
        let _ = n.l3_ecmp_destroy(EcmpId(0));
        let _ = n.l3_intf_stat_supported(0);
        let _ = n.l3_intf_stats(IntfId(0), &[], false);
        let _ = n.l3_intf_admin_set(IntfId(0), false, false);
        let glean_start = n.0.borrow().len();
        let _ = n.l3_egress_glean();
        assert_eq!(
            n.0.borrow().get(glean_start..),
            Some(&["l3_egress_glean", "l3_egress_cpu"][..]),
            "glean defaults to the CPU egress"
        );
        n.0.borrow_mut().pop();
        let _ = n.fp_group_create(&[], 0);
        let _ = n.fp_group_destroy(FpGroupId(0));
        let _ = n.fp_entry_create(FpGroupId(0), &FpRule::default());
        let _ = n.fp_entry_destroy(FpEntryId(0));
        let _ = n.fp_entry_counter(FpEntryId(0));
        let _ = n.fp_supported(FpCap::Ranges);
        let ex_start = n.0.borrow().len();
        let _ = n.fp_group_create_ex(&[], 0, FpGroupExtra::default());
        assert_eq!(
            n.0.borrow().get(ex_start..),
            Some(&["fp_group_create_ex", "fp_group_create"][..])
        );
        n.0.borrow_mut().pop();
        assert_eq!(
            n.fp_group_create_ex(
                &[],
                0,
                FpGroupExtra {
                    egress: true,
                    ..FpGroupExtra::default()
                }
            ),
            Err(Error::NotSupported)
        );
        n.0.borrow_mut().pop();
        let _ = n.fp_entry_update(FpEntryId(0), &FpRule::default());
        let _ = n.fp_entry_enable(FpEntryId(0), false);
        let _ = n.fp_group_free(FpGroupId(0));
        let _ = n.policer_create(&PolicerCfg::default());
        let _ = n.policer_set(PolicerId(0), &PolicerCfg::default());
        let _ = n.policer_destroy(PolicerId(0));
        let _ = n.qos_supported();
        let _ = n.qos_map_create(QosMapKind::DscpToTc, &[]);
        let _ = n.qos_map_update(QosMapId(0), &[]);
        let _ = n.qos_map_destroy(QosMapId(0));
        let _ = n.port_qos_map_set(hw, QosMapKind::DscpToTc, None);
        let _ = n.port_default_tc_set(hw, 0);
        let _ = n.queue_scheduler_set(hw, 0, None);
        let _ = n.port_scheduler_set(hw, None);
        let _ = n.wred_create(&WredCfg::default());
        let _ = n.wred_update(WredId(0), &WredCfg::default());
        let _ = n.wred_destroy(WredId(0));
        let _ = n.queue_wred_set(hw, 0, None);
        let _ = n.queue_buffer_set(hw, 0, None);
        let _ = n.pg_buffer_set(hw, 0, None);
        let _ = n.queue_stats_get(hw, 0, &[]);
        let _ = n.queue_stats_clear(hw, 0, &[]);
        let _ = n.pg_stats_get(hw, 0, &[]);
        let _ = n.pg_stats_clear(hw, 0, &[]);
        let _ = n.pool_stats_get(false, &[]);
        let _ = n.mirror_supported();
        let _ = n.mirror_create(&MirrorCfg::new(hw));
        let _ = n.mirror_update(MirrorId(0), &MirrorCfg::new(hw));
        let _ = n.mirror_destroy(MirrorId(0));
        let _ = n.port_mirror_set(hw, false, &[]);
        let _ = n.sample_create(0);
        let _ = n.sample_update(SampleId(0), 0);
        let _ = n.sample_destroy(SampleId(0));
        let _ = n.port_sample_set(hw, false, None);
        let _ = n.hash_supported();
        let _ = n.hash_set(false, &[], 0);
        let _ = n.port_storm_set(hw, StormKind::Broadcast, None);
        let _ = n.vlan_flood_set(vid, FloodKind::Broadcast, false);
        assert_eq!(*n.0.borrow(), OP_NAMES);
        assert_eq!(
            OP_NAMES.len() + super::super::LEGACY_CPP_OPS.len(),
            121,
            "every ob::Backend operation"
        );
    }
}

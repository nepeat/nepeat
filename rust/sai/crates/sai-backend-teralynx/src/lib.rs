//! The Teralynx 7 backend of the Rust SAI (Celestica CS8210-32X): [`sai_core::backend::Backend`]
//! over the vendor IFCS 0.12.41 SDK, `libifcs.so` `dlopen`ed at run time through the `ifcs` rim
//! (nothing vendor is linked). A port of the C++ IFCS backend of
//! `ai_re/nix/packages/sai-teralynx/src` (`ifcs_backend.cpp` + `tl_*.cpp`), area by area, with the
//! same IFCS calls in the same order and arguments. No `unsafe` here: all of it is in `ifcs`.
//!
//! # Areas (one module per C++ file, same names)
//!
//! | module | C++ | G1 (this gate) |
//! |---|---|---|
//! | [`core`] | `tl_core` | shared state, port map, sysport handles, status mapping, attribute helpers |
//! | [`config`] | `ifcs_config` | port map, init commands (accepted / skipped kinds), init yaml + append merge |
//! | [`keys`] | (`Core::key` calls) | every profile key, the C++ defaults and parsing |
//! | [`switch`] | `tl_switch` | init / shutdown sequence, init commands, callbacks, temperatures, table usage, chip limits, diag shell |
//! | [`port`] | `tl_port` | every port operation |
//! | [`counters`] | `tl_counters` | the 101-id map, counters by id, `counters <devport>` |
//! | [`l2`], [`lag`], [`l3`], [`hostif`], [`sflow`], [`acl`] | `tl_l2`, ... | what init / shutdown call; the rest is G2 ([`g2`]) |
//!
//! # Locking
//!
//! The C++ serialises the backend with `Core::mu` (recursive), taken by most area functions; the
//! SAI frontend calls the backend under its own API lock too. Here the SAI layer's API lock is the
//! one lock: every [`Backend`] method runs under it (`&self` / `&mut self` of the backend the
//! core owns). Outside it, as in the C++: the temperature reader of the publisher thread ([`switch::Temps`],
//! its own mutex = `temp_mu_`) and libifcs' callback threads (link scan, L2 events, log), which
//! make no IFCS call, read only immutable state (the port map's devports, the sysport map) or
//! atomics (the link cache) and the LAG destination map (its own mutex = `map_mu_`, never held
//! across an IFCS call), and send on the [`EventSink`]. libifcs serialises its API itself.
//!
//! # Differences from the C++ backend
//!
//! * Before init and after shutdown every operation answers `UNINITIALIZED` without an IFCS call
//!   (C++: the function table is empty before the first init; a call would jump through a null
//!   pointer), `ports()` is empty, `cpu_port()` is devport 0; a second init while up is refused.
//! * A failed init leaves no callback entry behind: libifcs keeps the trampoline it was given (the
//!   C++ keeps `this`), which then finds nothing and does nothing.
//! * The GOT redirects check each slot before writing it and restore a read-only page's protection
//!   (`elf-got`); a slot holding an unexpected address is left alone and said on stderr.
//! * An L2 event without a known destination (C++ `hw_port = -1`) is passed as the port `0xffff`
//!   that no switch has: the SAI layer then finds no bridge port for it, as the C++ frontend does.
//! * The diag shell's `copp` and `acl` answer "not ported yet" (G2).
#![forbid(unsafe_code)]
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]

pub mod acl;
pub mod config;
pub mod core;
pub mod counters;
pub mod g2;
pub mod hostif;
pub mod keys;
pub mod l2;
pub mod l3;
pub mod lag;
pub mod port;
pub mod sflow;
pub mod switch;

use std::io::Write as _;
use std::net::IpAddr;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

use sai_core::backend::types::{
    AsicTemps, BufferCfg, ChipLimits, Dest, EcmpId, EgressId, Fec, FilterId, FloodKind, FpCap, FpCounter, FpEntryId,
    FpGroupExtra, FpGroupId, FpQual, FpRule, GenlId, IntfId, IpPrefix, LagMember, Learn, Loopback, MacAddr, MirrorCfg,
    MirrorId, NetifCreated, NetifId, PolicerCfg, PolicerId, PortStat, QosMapEntry, QosMapId, QosMapKind, RouteTarget,
    SampleId, SchedCfg, StationId, StormKind, TableUsage, TrapKind, TrapSpec, TrunkId, Vrf, WredCfg, WredId,
};
use sai_core::backend::{Backend, EventSink, HwPort, PortInfo, Profile, Vid, ops};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::g2::not_ported;
use crate::port::PortArea;
use crate::switch::Up;

pub use crate::keys::PROFILE_KEYS;

/// The backend over libifcs. Creating one touches nothing; [`Backend::init`] loads libifcs and
/// brings the chip up.
#[derive(Debug, Default)]
pub struct TeralynxBackend {
    up: Option<Up>,
}

impl TeralynxBackend {
    /// A backend that is not initialised.
    pub const fn new() -> Self {
        Self { up: None }
    }

    fn up(&self) -> Result<&Up> {
        self.up.as_ref().ok_or(Error::Uninitialized)
    }

    fn up_mut(&mut self) -> Result<&mut Up> {
        self.up.as_mut().ok_or(Error::Uninitialized)
    }

    fn shell(&self, cmd: &str) -> (Result<()>, String) {
        match &self.up {
            Some(up) => up.shell(cmd),
            None if cmd.split_whitespace().next().is_none_or(|w| w == "help") => (
                Ok(()),
                "commands: ports | devport <id> | temp | copp | acl | counters <devport> | help (read-only)\n"
                    .to_owned(),
            ),
            None => (Err(Error::Uninitialized), "SDK not initialised\n".to_owned()),
        }
    }
}

impl Drop for TeralynxBackend {
    fn drop(&mut self) {
        // ~IfcsBackend: if initialised, shut down.
        if let Some(mut up) = self.up.take() {
            if up.initialized {
                up.shutdown();
            }
        }
    }
}

const fn dp(hw: HwPort) -> u32 {
    hw.0 as u32
}

impl Backend for TeralynxBackend {
    fn name(&self) -> &'static str {
        "teralynx"
    }

    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        if self.up.is_some() {
            log::error(ssys::SAI_API_SWITCH, format_args!("init: the chip is already up"));
            return Err(Error::Failure);
        }
        self.up = Some(Up::init(profile, events)?);
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(mut up) = self.up.take() {
            up.shutdown();
        }
    }

    fn ports(&self) -> Vec<PortInfo> {
        self.up.as_ref().map(|up| PortArea::infos(&up.core)).unwrap_or_default()
    }

    fn cpu_port(&self) -> HwPort {
        HwPort(0)
    }

    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()> {
        let u = self.up()?;
        u.port.admin_set(&u.core, dp(hw), up)
    }

    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        let u = self.up()?;
        u.port.link_get(&u.core, dp(hw))
    }

    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        PortArea::speed_set(&self.up()?.core, dp(hw), mbps)
    }

    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        PortArea::speed_get(&self.up()?.core, dp(hw))
    }

    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        PortArea::supported_speeds(&self.up()?.core, dp(hw))
    }

    fn vlan_create(&mut self, _vid: Vid) -> Result<()> {
        self.up()?;
        Err(not_ported("vlan_create"))
    }

    fn vlan_destroy(&mut self, _vid: Vid) -> Result<()> {
        self.up()?;
        Err(not_ported("vlan_destroy"))
    }

    fn vlan_port_add(&mut self, _vid: Vid, _hw: HwPort, _untagged: bool) -> Result<()> {
        self.up()?;
        Err(not_ported("vlan_port_add"))
    }

    fn vlan_port_remove(&mut self, _vid: Vid, _hw: HwPort) -> Result<()> {
        self.up()?;
        Err(not_ported("vlan_port_remove"))
    }

    fn vlan_members(&self, _vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        self.up()?;
        Err(not_ported("vlan_members"))
    }

    fn temperature_reader(&self) -> Option<sai_core::temp::TempReader> {
        let temps = Arc::clone(&self.up.as_ref()?.temps);
        Some(Arc::new(move || temps.read()))
    }
}

impl ops::Base for TeralynxBackend {
    fn is_fake(&self) -> bool {
        false
    }
    fn hardware_info(&self) -> String {
        "teralynx7 ifcs-0.12.41".to_owned()
    }
    fn table_usage(&self) -> Result<TableUsage> {
        self.up()?.table_usage()
    }
    fn chip_limits(&self) -> Result<ChipLimits> {
        self.up()?.chip_limits()
    }
}

impl ops::Shell for TeralynxBackend {
    fn run_shell(&mut self, cmd: &str) -> Result<()> {
        let (st, out) = self.shell(cmd);
        for line in out.lines() {
            log::notice(ssys::SAI_API_SWITCH, format_args!("shell: {line}"));
        }
        st
    }
    fn run_shell_to(&mut self, cmd: &str, fd: BorrowedFd<'_>) -> Result<()> {
        let (st, out) = self.shell(cmd);
        // C++: write(fd) until done or an error; a dup of the fd here (closed again at once)
        if let Ok(owned) = fd.try_clone_to_owned() {
            let _ = std::fs::File::from(owned).write_all(out.as_bytes());
        }
        st
    }
}

impl ops::Temp for TeralynxBackend {
    fn asic_temperatures(&self) -> Result<AsicTemps> {
        let up = self.up()?;
        if !up.initialized {
            return Err(Error::Uninitialized);
        }
        up.temps.read()
    }
}

// The C++ backend implements no MDIO (the Credo PHYs are not the switch chip's).
sai_core::typed_defaults!(TeralynxBackend: Mdio);

impl ops::PortOps for TeralynxBackend {
    fn port_frame_max_set(&mut self, hw: HwPort, bytes: u32) -> Result<()> {
        PortArea::frame_max_set(&self.up()?.core, dp(hw), bytes)
    }
    fn port_fec_set(&mut self, hw: HwPort, fec: Fec) -> Result<()> {
        PortArea::fec_set(&self.up()?.core, dp(hw), fec)
    }
    fn port_autoneg_set(&mut self, hw: HwPort, on: bool) -> Result<()> {
        PortArea::autoneg_set(&self.up()?.core, dp(hw), on)
    }
    fn port_loopback_set(&mut self, hw: HwPort, lb: Loopback) -> Result<()> {
        PortArea::loopback_set(&self.up()?.core, dp(hw), lb)
    }
    fn port_pvid_set(&mut self, hw: HwPort, vid: Vid) -> Result<()> {
        PortArea::pvid_set(&self.up()?.core, dp(hw), vid)
    }
    fn port_learn_set(&mut self, hw: HwPort, mode: Learn) -> Result<()> {
        PortArea::learn_set(&self.up()?.core, dp(hw), mode)
    }
    fn port_stat_get(&self, hw: HwPort, stat: PortStat) -> Result<u64> {
        let u = self.up()?;
        u.port.stat_get(&u.core, dp(hw), stat)
    }
    fn port_stat_clear(&mut self, hw: HwPort) -> Result<()> {
        let u = self.up_mut()?;
        u.port.stat_clear(&u.core, dp(hw))
    }
    fn port_stat_by_id(&self, id: ssys::sai_stat_id_t) -> bool {
        counters::port_stat_mapped(id)
    }
    fn port_stats_by_id(&self, hw: HwPort, ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        counters::port_stats_by_id(&self.up()?.core, dp(hw), ids)
    }
    fn port_pfc_set(&mut self, _hw: HwPort, _separate: bool, _tx: u8, _rx: u8) -> Result<()> {
        self.up()?;
        Err(not_ported("port_pfc_set"))
    }
    fn port_pfc_stat_get(&self, _hw: HwPort, _prio: u8, _tx: bool) -> Result<u64> {
        self.up()?;
        Err(not_ported("port_pfc_stat_get"))
    }
}

/// `fn op(..) -> Result<T> { G2 }` for each listed operation.
macro_rules! g2 {
    ($tr:ident { $(fn $op:ident(&$($m:ident)? self $(, $a:ident: $t:ty)*) -> $ret:ty;)* }) => {
        impl ops::$tr for TeralynxBackend {
            $(
                fn $op(&$($m)? self $(, $a: $t)*) -> $ret {
                    $(let _ = $a;)*
                    self.up()?;
                    Err(not_ported(stringify!($op)))
                }
            )*
        }
    };
}

g2!(Lag {
    fn lag_create(&mut self) -> Result<TrunkId>;
    fn lag_destroy(&mut self, tid: TrunkId) -> Result<()>;
    fn lag_set_members(&mut self, tid: TrunkId, members: &[LagMember]) -> Result<()>;
});

g2!(L2 {
    fn l2_add(&mut self, mac: MacAddr, vid: Vid, hw: Dest, is_static: bool, discard: bool) -> Result<()>;
    fn l2_delete(&mut self, mac: MacAddr, vid: Vid) -> Result<()>;
    fn l2_flush(&mut self, hw: Option<Dest>, vid: Option<Vid>, statics: bool, dynamics: bool) -> Result<()>;
    fn l2_age_set(&mut self, seconds: u32) -> Result<()>;
});

g2!(Hostif {
    fn netif_create(&mut self, ifname: &str, hw: HwPort, vid: Vid, mac: MacAddr, keep_tag: bool) -> Result<NetifCreated>;
    fn netif_destroy(&mut self, netif_id: NetifId, filter_id: FilterId) -> Result<()>;
    fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()>;
    fn netif_set_keep_tag(&mut self, netif_id: NetifId, hw: HwPort, keep_tag: bool, filter_id: Option<FilterId>) -> Result<FilterId>;
    fn trap_set(&mut self, kind: TrapKind, to_cpu: bool) -> Result<()>;
    fn trap_program(&mut self, t: &TrapSpec, remove: bool) -> Result<()>;
    fn genetlink_create(&mut self, name: &str, mcgrp: &str) -> Result<GenlId>;
    fn genetlink_destroy(&mut self, id: GenlId) -> Result<()>;
    fn genetlink_trap_set(&mut self, id: GenlId, trap_type: i32, add: bool) -> Result<()>;
});

impl ops::L3 for TeralynxBackend {
    fn l3_intf_create(&mut self, _mac: MacAddr, _vid: Vid, _mtu: u32, _vrf: Vrf) -> Result<IntfId> {
        self.up()?;
        Err(not_ported("l3_intf_create"))
    }
    fn l3_intf_update(&mut self, _intf_id: IntfId, _mac: MacAddr, _vid: Vid, _mtu: u32, _vrf: Vrf) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_intf_update"))
    }
    fn l3_intf_destroy(&mut self, _intf_id: IntfId) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_intf_destroy"))
    }
    fn l3_station_add(&mut self, _mac: MacAddr) -> Result<StationId> {
        self.up()?;
        Err(not_ported("l3_station_add"))
    }
    fn l3_station_delete(&mut self, _station_id: StationId) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_station_delete"))
    }
    fn l3_egress_create(&mut self, _intf_id: IntfId, _dmac: MacAddr, _hw: Dest, _vid: Vid) -> Result<EgressId> {
        self.up()?;
        Err(not_ported("l3_egress_create"))
    }
    fn l3_egress_update(
        &mut self,
        _egress_id: EgressId,
        _intf_id: IntfId,
        _dmac: MacAddr,
        _hw: Dest,
        _vid: Vid,
    ) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_egress_update"))
    }
    fn l3_egress_destroy(&mut self, _egress_id: EgressId) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_egress_destroy"))
    }
    fn l3_egress_cpu(&mut self) -> Result<EgressId> {
        self.up()?;
        Err(not_ported("l3_egress_cpu"))
    }
    fn l3_egress_drop(&mut self) -> Result<EgressId> {
        self.up()?;
        Err(not_ported("l3_egress_drop"))
    }
    fn l3_host_add(&mut self, _vrf: Vrf, _ip: IpAddr, _egress_id: EgressId) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_host_add"))
    }
    fn l3_host_delete(&mut self, _vrf: Vrf, _ip: IpAddr) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_host_delete"))
    }
    fn l3_route_add(&mut self, _vrf: Vrf, _p: IpPrefix, _target: RouteTarget, _replace: bool) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_route_add"))
    }
    fn l3_route_delete(&mut self, _vrf: Vrf, _p: IpPrefix) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_route_delete"))
    }
    fn l3_ecmp_create(&mut self, _egress: &[EgressId]) -> Result<EcmpId> {
        self.up()?;
        Err(not_ported("l3_ecmp_create"))
    }
    fn l3_ecmp_update(&mut self, _ecmp_id: EcmpId, _egress: &[EgressId]) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_ecmp_update"))
    }
    fn l3_ecmp_destroy(&mut self, _ecmp_id: EcmpId) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_ecmp_destroy"))
    }
    fn l3_intf_stat_supported(&self, id: ssys::sai_stat_id_t) -> bool {
        l3::L3Area::intf_stat_supported(id)
    }
    fn l3_intf_stats(&mut self, _intf_id: IntfId, _ids: &[ssys::sai_stat_id_t], _clear: bool) -> Result<Vec<u64>> {
        self.up()?;
        Err(not_ported("l3_intf_stats"))
    }
    fn l3_intf_admin_set(&mut self, _intf_id: IntfId, _v4: bool, _v6: bool) -> Result<()> {
        self.up()?;
        Err(not_ported("l3_intf_admin_set"))
    }
    fn l3_egress_glean(&mut self) -> Result<EgressId> {
        self.up()?;
        Err(not_ported("l3_egress_glean"))
    }
}

impl ops::Field for TeralynxBackend {
    fn fp_group_create(&mut self, _qset: &[FpQual], _priority: i32) -> Result<FpGroupId> {
        self.up()?;
        Err(not_ported("fp_group_create"))
    }
    fn fp_group_destroy(&mut self, _gid: FpGroupId) -> Result<()> {
        self.up()?;
        Err(not_ported("fp_group_destroy"))
    }
    fn fp_entry_create(&mut self, _gid: FpGroupId, _r: &FpRule) -> Result<FpEntryId> {
        self.up()?;
        Err(not_ported("fp_entry_create"))
    }
    fn fp_entry_destroy(&mut self, _eid: FpEntryId) -> Result<()> {
        self.up()?;
        Err(not_ported("fp_entry_destroy"))
    }
    fn fp_entry_counter(&self, _eid: FpEntryId) -> Result<FpCounter> {
        self.up()?;
        Err(not_ported("fp_entry_counter"))
    }
    /// Every `FpCap` but `SET_DSCP` (IFCS 0.12.41 refuses that action type), as the C++.
    fn fp_supported(&self, cap: FpCap) -> bool {
        cap != FpCap::SetDscp
    }
    fn fp_group_create_ex(&mut self, _qset: &[FpQual], _priority: i32, _x: FpGroupExtra) -> Result<FpGroupId> {
        self.up()?;
        Err(not_ported("fp_group_create_ex"))
    }
    fn fp_entry_update(&mut self, _eid: FpEntryId, _r: &FpRule) -> Result<()> {
        self.up()?;
        Err(not_ported("fp_entry_update"))
    }
    fn fp_entry_enable(&mut self, _eid: FpEntryId, _on: bool) -> Result<()> {
        self.up()?;
        Err(not_ported("fp_entry_enable"))
    }
    fn fp_group_free(&self, _gid: FpGroupId) -> Result<u32> {
        self.up()?;
        Err(not_ported("fp_group_free"))
    }
}

g2!(Policer {
    fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<PolicerId>;
    fn policer_set(&mut self, pid: PolicerId, cfg: &PolicerCfg) -> Result<()>;
    fn policer_destroy(&mut self, pid: PolicerId) -> Result<()>;
});

impl ops::Qos for TeralynxBackend {
    /// As the C++: `QoS` is programmed (`tl_qos.cpp`; its operations are G2).
    fn qos_supported(&self) -> bool {
        true
    }
    fn qos_map_create(&mut self, _kind: QosMapKind, _entries: &[QosMapEntry]) -> Result<QosMapId> {
        self.up()?;
        Err(not_ported("qos_map_create"))
    }
    fn qos_map_update(&mut self, _id: QosMapId, _entries: &[QosMapEntry]) -> Result<()> {
        self.up()?;
        Err(not_ported("qos_map_update"))
    }
    fn qos_map_destroy(&mut self, _id: QosMapId) -> Result<()> {
        self.up()?;
        Err(not_ported("qos_map_destroy"))
    }
    fn port_qos_map_set(&mut self, _hw: HwPort, _kind: QosMapKind, _id: Option<QosMapId>) -> Result<()> {
        self.up()?;
        Err(not_ported("port_qos_map_set"))
    }
    fn port_default_tc_set(&mut self, _hw: HwPort, _tc: u8) -> Result<()> {
        self.up()?;
        Err(not_ported("port_default_tc_set"))
    }
    fn queue_scheduler_set(&mut self, _hw: HwPort, _queue: u8, _cfg: Option<&SchedCfg>) -> Result<()> {
        self.up()?;
        Err(not_ported("queue_scheduler_set"))
    }
    fn port_scheduler_set(&mut self, _hw: HwPort, _cfg: Option<&SchedCfg>) -> Result<()> {
        self.up()?;
        Err(not_ported("port_scheduler_set"))
    }
    fn wred_create(&mut self, _cfg: &WredCfg) -> Result<WredId> {
        self.up()?;
        Err(not_ported("wred_create"))
    }
    fn wred_update(&mut self, _id: WredId, _cfg: &WredCfg) -> Result<()> {
        self.up()?;
        Err(not_ported("wred_update"))
    }
    fn wred_destroy(&mut self, _id: WredId) -> Result<()> {
        self.up()?;
        Err(not_ported("wred_destroy"))
    }
    fn queue_wred_set(&mut self, _hw: HwPort, _queue: u8, _id: Option<WredId>) -> Result<()> {
        self.up()?;
        Err(not_ported("queue_wred_set"))
    }
    fn queue_buffer_set(&mut self, _hw: HwPort, _queue: u8, _cfg: Option<&BufferCfg>) -> Result<()> {
        self.up()?;
        Err(not_ported("queue_buffer_set"))
    }
    fn pg_buffer_set(&mut self, _hw: HwPort, _pg: u8, _cfg: Option<&BufferCfg>) -> Result<()> {
        self.up()?;
        Err(not_ported("pg_buffer_set"))
    }
    fn queue_stats_get(&self, _hw: HwPort, _queue: u8, _ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        self.up()?;
        Err(not_ported("queue_stats_get"))
    }
    fn queue_stats_clear(&mut self, _hw: HwPort, _queue: u8, _ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        self.up()?;
        Err(not_ported("queue_stats_clear"))
    }
    fn pg_stats_get(&self, _hw: HwPort, _pg: u8, _ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        self.up()?;
        Err(not_ported("pg_stats_get"))
    }
    fn pg_stats_clear(&mut self, _hw: HwPort, _pg: u8, _ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        self.up()?;
        Err(not_ported("pg_stats_clear"))
    }
    // pool_stats_get: not a C++ operation either (the trait default).
}

impl ops::Mirror for TeralynxBackend {
    /// As the C++: mirroring is programmed (`tl_mirror.cpp`; its operations are G2).
    fn mirror_supported(&self) -> bool {
        true
    }
    fn mirror_create(&mut self, _cfg: &MirrorCfg) -> Result<MirrorId> {
        self.up()?;
        Err(not_ported("mirror_create"))
    }
    fn mirror_update(&mut self, _id: MirrorId, _cfg: &MirrorCfg) -> Result<()> {
        self.up()?;
        Err(not_ported("mirror_update"))
    }
    fn mirror_destroy(&mut self, _id: MirrorId) -> Result<()> {
        self.up()?;
        Err(not_ported("mirror_destroy"))
    }
    fn port_mirror_set(&mut self, _hw: HwPort, _ingress: bool, _ids: &[MirrorId]) -> Result<()> {
        self.up()?;
        Err(not_ported("port_mirror_set"))
    }
}

g2!(Sample {
    fn sample_create(&mut self, rate: u32) -> Result<SampleId>;
    fn sample_update(&mut self, id: SampleId, rate: u32) -> Result<()>;
    fn sample_destroy(&mut self, id: SampleId) -> Result<()>;
    fn port_sample_set(&mut self, hw: HwPort, ingress: bool, id: Option<SampleId>) -> Result<()>;
});

impl ops::Hash for TeralynxBackend {
    /// As the C++: the switch hash objects are programmed (`tl_lag.cpp` `hash_set`: G2).
    fn hash_supported(&self) -> bool {
        true
    }
    fn hash_set(&mut self, _lag: bool, _fields: &[i32], _seed: u32) -> Result<()> {
        self.up()?;
        Err(not_ported("hash_set"))
    }
}

g2!(Storm {
    fn port_storm_set(&mut self, hw: HwPort, kind: StormKind, policer: Option<PolicerId>) -> Result<()>;
    fn vlan_flood_set(&mut self, vid: Vid, kind: FloodKind, on: bool) -> Result<()>;
});

#[cfg(test)]
mod tests {
    use super::*;
    use sai_core::backend::ops::{Base, Hash, Mirror, Qos, Shell};

    #[test]
    fn before_init_nothing_is_called() {
        let mut b = TeralynxBackend::new();
        assert_eq!(b.name(), "teralynx");
        assert!(!b.is_fake());
        assert_eq!(b.cpu_port(), HwPort(0));
        assert_eq!(b.ports(), vec![]);
        assert_eq!(b.port_admin_set(HwPort(1), true), Err(Error::Uninitialized));
        assert_eq!(b.table_usage(), Err(Error::Uninitialized));
        assert_eq!(ops::Lag::lag_create(&mut b), Err(Error::Uninitialized));
        assert!(b.qos_supported() && b.mirror_supported() && b.hash_supported());
        assert!(ops::Field::fp_supported(&b, FpCap::Ranges) && !ops::Field::fp_supported(&b, FpCap::SetDscp));
        assert!(b.temperature_reader().is_none());
        assert_eq!(b.run_shell("help"), Ok(()));
        assert_eq!(b.run_shell("ports"), Err(Error::Uninitialized));
        b.shutdown();
    }

    #[test]
    fn init_refusals_before_the_sdk() {
        let mut b = TeralynxBackend::new();
        let (sink, _rx) = EventSink::channel();
        let p = |pairs: &[(&str, &str)]| Profile::from_pairs(pairs.iter().copied());
        assert_eq!(
            b.init(&p(&[(keys::NETDEV, "x")]), sink.clone()),
            Err(Error::InvalidParameter)
        );
        assert_eq!(b.init(&p(&[]), sink.clone()), Err(Error::Failure)); // no init yaml
        let yaml = std::env::temp_dir().join(format!("tl-init-{}.yaml", std::process::id()));
        std::fs::write(&yaml, "x: 1\n").unwrap();
        let y = yaml.to_str().unwrap();
        assert_eq!(
            b.init(&p(&[(keys::INIT_CONFIG_FILE, y)]), sink.clone()),
            Err(Error::Failure)
        ); // no port map
        assert_eq!(
            b.init(
                &p(&[(keys::INIT_CONFIG_FILE, y), (keys::PORT_MAP, "/nonexistent")]),
                sink.clone()
            ),
            Err(Error::Failure)
        );
        assert!(b.up.is_none());
    }
}

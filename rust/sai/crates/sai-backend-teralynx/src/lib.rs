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
//! | [`l2`], [`lag`], [`l3`] | `tl_l2`, `tl_lag`, `tl_l3` | G2 (package C): every operation (VLANs, FDB, flush, aging, flood control, L2 events; LAGs, hash; VRFs, interfaces, next hops, hosts, routes, ECMP, RIF counters) |
//! | [`hostif`], [`acl`] | `tl_hostif`, `tl_acl` | G2 (package D): every operation (netdevs create / adopt, traps, the `CoPP` rule table, system traps, CPU queue counters, the diag `copp`; policers, SAI ACL tables with ranges / egress / `OUT_PORTS` / REDIRECT / `SET_TC` / MIRROR, entry update and enable, storm control, the diag `acl`); genetlink is `sflow`'s |
//! | [`qos`], [`mirror`], [`sflow`] | `tl_qos`, `tl_mirror`, `tl_sflow` | G2 (package E): every operation (`QoS` maps + shared map groups, the TC -> queue identity rule, PFC, default TC, queue schedulers / port shaper in bits/s, WRED curve groups with the 0.12.41 refusals + rollback, queue buffer thresholds with `RESERVED_SIZE` refused, queue / PG / PFC counters; SPAN / ERSPAN sessions, shared DCDPs, collector sets, port mirroring, `MirrorArea::collector_set` for ACL MIRROR; samplers, the `sFlow` collectors, genetlink host interfaces over the ipd driver's netlink ioctls, the SAMPLEPACKET trap and its queue meter) |
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
pub mod mirror;
pub mod port;
pub mod qos;
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

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        let u = self.up_mut()?;
        u.l2.vlan_create(&u.core, vid.get())
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        let u = self.up_mut()?;
        u.l2.vlan_destroy(&u.core, vid.get())
    }

    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        let u = self.up_mut()?;
        u.l2.vlan_port_add(&u.core, &u.lag, vid.get(), dp(hw), untagged)
    }

    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        let u = self.up_mut()?;
        u.l2.vlan_port_remove(&u.core, vid.get(), dp(hw))
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        self.up()?.l2.vlan_members(vid.get())
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
    // PFC: the QoS area's (tl_qos.cpp)
    fn port_pfc_set(&mut self, hw: HwPort, separate: bool, tx: u8, rx: u8) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.pfc_set(&u.core, dp(hw), separate, tx, rx)
    }
    fn port_pfc_stat_get(&self, hw: HwPort, prio: u8, tx: bool) -> Result<u64> {
        qos::QosArea::pfc_stat_get(&self.up()?.core, dp(hw), prio, tx)
    }
}

impl ops::Lag for TeralynxBackend {
    fn lag_create(&mut self) -> Result<TrunkId> {
        let u = self.up_mut()?;
        u.lag.create(&u.core)
    }
    fn lag_destroy(&mut self, tid: TrunkId) -> Result<()> {
        let u = self.up_mut()?;
        u.lag.destroy(&u.core, tid)
    }
    fn lag_set_members(&mut self, tid: TrunkId, members: &[LagMember]) -> Result<()> {
        let u = self.up_mut()?;
        u.lag.set_members(&u.core, tid, members)
    }
}

impl ops::L2 for TeralynxBackend {
    fn l2_add(&mut self, mac: MacAddr, vid: Vid, hw: Dest, is_static: bool, discard: bool) -> Result<()> {
        let u = self.up()?;
        u.l2.l2_add(&u.core, &u.lag, mac.0, vid.get(), hw, is_static, discard)
    }
    fn l2_delete(&mut self, mac: MacAddr, vid: Vid) -> Result<()> {
        l2::L2Area::l2_delete(&self.up()?.core, mac.0, vid.get())
    }
    fn l2_flush(&mut self, hw: Option<Dest>, vid: Option<Vid>, statics: bool, dynamics: bool) -> Result<()> {
        let u = self.up()?;
        l2::L2Area::l2_flush(&u.core, &u.lag, hw, vid.map(Vid::get), statics, dynamics)
    }
    fn l2_age_set(&mut self, seconds: u32) -> Result<()> {
        l2::L2Area::l2_age_set(&self.up()?.core, seconds)
    }
}

impl ops::Hostif for TeralynxBackend {
    fn netif_create(
        &mut self,
        ifname: &str,
        hw: HwPort,
        _vid: Vid,
        mac: MacAddr,
        keep_tag: bool,
    ) -> Result<NetifCreated> {
        let u = self.up_mut()?;
        u.hostif
            .netif_create(&u.core, u.initialized, ifname, dp(hw), mac, keep_tag)
    }
    fn netif_destroy(&mut self, netif_id: NetifId, _filter_id: FilterId) -> Result<()> {
        let u = self.up_mut()?;
        u.hostif.netif_destroy(&u.core, netif_id)
    }
    fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()> {
        self.up_mut()?.hostif.netif_carrier_set(ifname, up)
    }
    fn netif_set_keep_tag(
        &mut self,
        netif_id: NetifId,
        _hw: HwPort,
        keep_tag: bool,
        _filter_id: Option<FilterId>,
    ) -> Result<FilterId> {
        let u = self.up_mut()?;
        u.hostif.netif_set_keep_tag(&u.core, netif_id, keep_tag)
    }
    fn trap_set(&mut self, kind: TrapKind, _to_cpu: bool) -> Result<()> {
        hostif::HostifArea::trap_set(&self.up()?.core, kind)
    }
    fn trap_program(&mut self, t: &TrapSpec, remove: bool) -> Result<()> {
        let u = self.up_mut()?;
        // copp_prio reads the chip limits where the C++ does (`c_.sw_limits`): first use only
        let limits = || switch::chip_limits_of(&u.core, &u.limits).unwrap_or_default();
        u.hostif
            .trap_program(&u.core, u.initialized, &mut u.acl, &mut u.sflow, &limits, t, remove)
    }
    // genetlink_*: the sFlow area's (tl_sflow.cpp)
    fn genetlink_create(&mut self, name: &str, mcgrp: &str) -> Result<GenlId> {
        self.up_mut()?.sflow.genetlink_create(name, mcgrp)
    }
    fn genetlink_destroy(&mut self, id: GenlId) -> Result<()> {
        let u = self.up_mut()?;
        u.sflow.genetlink_destroy(&u.core, id)
    }
    fn genetlink_trap_set(&mut self, id: GenlId, trap_type: i32, add: bool) -> Result<()> {
        let u = self.up_mut()?;
        u.sflow.genetlink_trap_set(&u.core, id, trap_type, add)
    }
}

impl ops::L3 for TeralynxBackend {
    fn l3_intf_create(&mut self, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<IntfId> {
        let u = self.up_mut()?;
        u.l3.intf_create(&u.core, &u.l2, mac.0, vid.get(), mtu, vrf)
    }
    fn l3_intf_update(&mut self, intf_id: IntfId, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.intf_update(&u.core, intf_id, mac.0, vid.get(), mtu, vrf)
    }
    fn l3_intf_destroy(&mut self, intf_id: IntfId) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.intf_destroy(&u.core, intf_id)
    }
    fn l3_station_add(&mut self, mac: MacAddr) -> Result<StationId> {
        let u = self.up_mut()?;
        Ok(u.l3.station_add(&u.core, mac.0))
    }
    /// Stations are bookkeeping only (`L3Area::station_delete`: success).
    fn l3_station_delete(&mut self, _station_id: StationId) -> Result<()> {
        self.up()?;
        Ok(())
    }
    fn l3_egress_create(&mut self, intf_id: IntfId, dmac: MacAddr, hw: Dest, vid: Vid) -> Result<EgressId> {
        let u = self.up_mut()?;
        u.l3.egress_create(&u.core, &u.lag, intf_id, dmac.0, hw, vid.get())
    }
    fn l3_egress_update(
        &mut self,
        egress_id: EgressId,
        intf_id: IntfId,
        dmac: MacAddr,
        hw: Dest,
        vid: Vid,
    ) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.egress_update(&u.core, &u.lag, egress_id, intf_id, dmac.0, hw, vid.get())
    }
    fn l3_egress_destroy(&mut self, egress_id: EgressId) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.egress_destroy(&u.core, egress_id)
    }
    fn l3_egress_cpu(&mut self) -> Result<EgressId> {
        Ok(self.up_mut()?.l3.egress_cpu())
    }
    fn l3_egress_drop(&mut self) -> Result<EgressId> {
        Ok(self.up_mut()?.l3.egress_drop())
    }
    fn l3_host_add(&mut self, vrf: Vrf, ip: IpAddr, egress_id: EgressId) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.host_add(&u.core, &mut u.hostif, vrf, ip, egress_id)
    }
    fn l3_host_delete(&mut self, vrf: Vrf, ip: IpAddr) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.host_delete(&u.core, vrf, ip)
    }
    fn l3_route_add(&mut self, vrf: Vrf, p: IpPrefix, target: RouteTarget, replace: bool) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.route_add(&u.core, &mut u.hostif, vrf, p, target, replace)
    }
    fn l3_route_delete(&mut self, vrf: Vrf, p: IpPrefix) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.route_delete(&u.core, vrf, p)
    }
    fn l3_ecmp_create(&mut self, egress: &[EgressId]) -> Result<EcmpId> {
        let u = self.up_mut()?;
        u.l3.ecmp_create(&u.core, egress)
    }
    fn l3_ecmp_update(&mut self, ecmp_id: EcmpId, egress: &[EgressId]) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.ecmp_update(&u.core, ecmp_id, egress)
    }
    fn l3_ecmp_destroy(&mut self, ecmp_id: EcmpId) -> Result<()> {
        let u = self.up_mut()?;
        u.l3.ecmp_destroy(&u.core, ecmp_id)
    }
    fn l3_intf_stat_supported(&self, id: ssys::sai_stat_id_t) -> bool {
        l3::L3Area::intf_stat_supported(id)
    }
    fn l3_intf_stats(&mut self, intf_id: IntfId, ids: &[ssys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>> {
        let u = self.up()?;
        u.l3.intf_stats(&u.core, intf_id, ids, clear)
    }
    fn l3_intf_admin_set(&mut self, intf_id: IntfId, v4: bool, v6: bool) -> Result<()> {
        let u = self.up()?;
        u.l3.intf_admin_set(&u.core, intf_id, v4, v6)
    }
    fn l3_egress_glean(&mut self) -> Result<EgressId> {
        Ok(self.up_mut()?.l3.egress_glean())
    }
}

impl ops::Field for TeralynxBackend {
    fn fp_group_create(&mut self, qset: &[FpQual], priority: i32) -> Result<FpGroupId> {
        let u = self.up_mut()?;
        u.acl.group_create(&u.core, qset, priority)
    }
    fn fp_group_destroy(&mut self, gid: FpGroupId) -> Result<()> {
        let u = self.up_mut()?;
        u.acl.group_destroy(&u.core, gid)
    }
    fn fp_entry_create(&mut self, gid: FpGroupId, r: &FpRule) -> Result<FpEntryId> {
        let u = self.up_mut()?;
        // MIRROR: the mirror area's collector sets (MirrorArea::collector_set)
        let mut peers = acl::Peers {
            l3: &u.l3,
            lag: &u.lag,
            mirror: &mut u.mirror,
        };
        u.acl.entry_create(&u.core, &mut peers, gid, r)
    }
    fn fp_entry_destroy(&mut self, eid: FpEntryId) -> Result<()> {
        let u = self.up_mut()?;
        // the mirror area's collector sets the entry held (MirrorArea::collector_set_release)
        u.acl.entry_destroy(&u.core, &mut u.mirror, eid)
    }
    fn fp_entry_counter(&self, eid: FpEntryId) -> Result<FpCounter> {
        let u = self.up()?;
        u.acl.entry_counter(&u.core, eid)
    }
    /// Every `FpCap` but `SET_DSCP`: IFCS 0.12.41 refuses that action type in an action profile
    /// ("Action not implemented/supported yet 16", chip 2026-11-16), as the C++.
    fn fp_supported(&self, cap: FpCap) -> bool {
        cap != FpCap::SetDscp
    }
    fn fp_group_create_ex(&mut self, qset: &[FpQual], priority: i32, x: FpGroupExtra) -> Result<FpGroupId> {
        let u = self.up_mut()?;
        u.acl.group_create_ex(&u.core, qset, priority, x)
    }
    fn fp_entry_update(&mut self, eid: FpEntryId, r: &FpRule) -> Result<()> {
        let u = self.up_mut()?;
        // MIRROR: as fp_entry_create
        let mut peers = acl::Peers {
            l3: &u.l3,
            lag: &u.lag,
            mirror: &mut u.mirror,
        };
        u.acl.entry_update(&u.core, &mut peers, eid, r)
    }
    fn fp_entry_enable(&mut self, eid: FpEntryId, on: bool) -> Result<()> {
        let u = self.up_mut()?;
        u.acl.entry_enable(&u.core, eid, on)
    }
    fn fp_group_free(&self, gid: FpGroupId) -> Result<u32> {
        self.up()?.acl.group_free(gid)
    }
}

impl ops::Policer for TeralynxBackend {
    fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<PolicerId> {
        Ok(self.up_mut()?.acl.policer_create(cfg))
    }
    fn policer_set(&mut self, pid: PolicerId, cfg: &PolicerCfg) -> Result<()> {
        let u = self.up_mut()?;
        if !u.acl.has_policer(pid) {
            return Err(Error::ItemNotFound);
        }
        let worst = u.acl.policer_set(&u.core, pid, cfg);
        // the sample policer's meter (tl_sflow), after the meters as in the C++
        u.sflow.policer_changed(&u.core, &u.acl, pid);
        worst
    }
    fn policer_destroy(&mut self, pid: PolicerId) -> Result<()> {
        let u = self.up_mut()?;
        u.acl.policer_destroy(&u.core, pid)
    }
}

impl ops::Qos for TeralynxBackend {
    /// As the C++: `QoS` is programmed (`tl_qos.cpp`).
    fn qos_supported(&self) -> bool {
        true
    }
    fn qos_map_create(&mut self, kind: QosMapKind, entries: &[QosMapEntry]) -> Result<QosMapId> {
        let u = self.up_mut()?;
        u.qos.map_create(&u.core, kind, entries)
    }
    fn qos_map_update(&mut self, id: QosMapId, entries: &[QosMapEntry]) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.map_update(&u.core, id, entries)
    }
    fn qos_map_destroy(&mut self, id: QosMapId) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.map_destroy(&u.core, id)
    }
    fn port_qos_map_set(&mut self, hw: HwPort, kind: QosMapKind, id: Option<QosMapId>) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.port_map_set(&u.core, dp(hw), kind, id)
    }
    fn port_default_tc_set(&mut self, hw: HwPort, tc: u8) -> Result<()> {
        qos::QosArea::default_tc_set(&self.up()?.core, dp(hw), tc)
    }
    fn queue_scheduler_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&SchedCfg>) -> Result<()> {
        qos::QosArea::queue_sched_set(&self.up()?.core, dp(hw), queue, cfg)
    }
    fn port_scheduler_set(&mut self, hw: HwPort, cfg: Option<&SchedCfg>) -> Result<()> {
        qos::QosArea::port_sched_set(&self.up()?.core, dp(hw), cfg)
    }
    fn wred_create(&mut self, cfg: &WredCfg) -> Result<WredId> {
        let u = self.up_mut()?;
        u.qos.wred_create(&u.core, cfg)
    }
    fn wred_update(&mut self, id: WredId, cfg: &WredCfg) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.wred_update(&u.core, id, cfg)
    }
    fn wred_destroy(&mut self, id: WredId) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.wred_destroy(&u.core, id)
    }
    fn queue_wred_set(&mut self, hw: HwPort, queue: u8, id: Option<WredId>) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.queue_wred_set(&u.core, dp(hw), queue, id)
    }
    fn queue_buffer_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&BufferCfg>) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.queue_buffer_set(&u.core, dp(hw), queue, cfg)
    }
    fn pg_buffer_set(&mut self, hw: HwPort, pg: u8, cfg: Option<&BufferCfg>) -> Result<()> {
        let u = self.up_mut()?;
        u.qos.pg_buffer_set(&u.core, dp(hw), pg, cfg)
    }
    fn queue_stats_get(&self, hw: HwPort, queue: u8, ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        let u = self.up()?;
        u.qos.queue_stats_get(&u.core, dp(hw), queue, ids)
    }
    fn queue_stats_clear(&mut self, hw: HwPort, queue: u8, ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        qos::QosArea::queue_stats_clear(&self.up()?.core, dp(hw), queue, ids)
    }
    fn pg_stats_get(&self, hw: HwPort, pg: u8, ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        let u = self.up()?;
        u.qos.pg_stats_get(&u.core, dp(hw), pg, ids)
    }
    fn pg_stats_clear(&mut self, hw: HwPort, pg: u8, ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        qos::QosArea::pg_stats_clear(&self.up()?.core, dp(hw), pg, ids)
    }
    // pool_stats_get: not a C++ operation either (the trait default).
}

impl ops::Mirror for TeralynxBackend {
    /// As the C++: mirroring is programmed (`tl_mirror.cpp`).
    fn mirror_supported(&self) -> bool {
        true
    }
    fn mirror_create(&mut self, cfg: &MirrorCfg) -> Result<MirrorId> {
        let u = self.up_mut()?;
        u.mirror.create(&u.core, cfg)
    }
    fn mirror_update(&mut self, id: MirrorId, cfg: &MirrorCfg) -> Result<()> {
        let u = self.up_mut()?;
        u.mirror.update(&u.core, id, cfg)
    }
    fn mirror_destroy(&mut self, id: MirrorId) -> Result<()> {
        let u = self.up_mut()?;
        u.mirror.destroy(&u.core, id)
    }
    fn port_mirror_set(&mut self, hw: HwPort, ingress: bool, ids: &[MirrorId]) -> Result<()> {
        let u = self.up_mut()?;
        u.mirror.port_set(&u.core, dp(hw), ingress, ids)
    }
}

impl ops::Sample for TeralynxBackend {
    fn sample_create(&mut self, rate: u32) -> Result<SampleId> {
        self.up_mut()?.mirror.sample_create(rate)
    }
    fn sample_update(&mut self, id: SampleId, rate: u32) -> Result<()> {
        let u = self.up_mut()?;
        u.mirror.sample_update(&u.core, &mut u.sflow, id, rate)
    }
    fn sample_destroy(&mut self, id: SampleId) -> Result<()> {
        self.up_mut()?.mirror.sample_destroy(id)
    }
    fn port_sample_set(&mut self, hw: HwPort, ingress: bool, id: Option<SampleId>) -> Result<()> {
        let u = self.up_mut()?;
        u.mirror.port_sample_set(&u.core, &mut u.sflow, dp(hw), ingress, id)
    }
}

impl ops::Hash for TeralynxBackend {
    /// As the C++: the switch hash objects are programmed (`tl_lag.cpp` `hash_set`).
    fn hash_supported(&self) -> bool {
        true
    }
    fn hash_set(&mut self, lag: bool, fields: &[i32], seed: u32) -> Result<()> {
        let u = self.up_mut()?;
        u.lag.hash_set(&u.core, lag, fields, seed)
    }
}

impl ops::Storm for TeralynxBackend {
    fn port_storm_set(&mut self, hw: HwPort, kind: StormKind, policer: Option<PolicerId>) -> Result<()> {
        let u = self.up_mut()?;
        // as fp_entry_create (storm entries mirror nothing)
        let mut peers = acl::Peers {
            l3: &u.l3,
            lag: &u.lag,
            mirror: &mut u.mirror,
        };
        u.acl.storm_set(&u.core, &mut peers, dp(hw), kind, policer)
    }
    fn vlan_flood_set(&mut self, vid: Vid, kind: FloodKind, on: bool) -> Result<()> {
        let u = self.up_mut()?;
        u.l2.vlan_flood_set(&u.core, vid.get(), kind, on)
    }
}

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

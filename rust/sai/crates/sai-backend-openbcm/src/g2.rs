//! The typed backend operations the SAI frontend calls (required by `sai_core::backend::ops`)
//! that this backend has not ported from `openbcm_backend.cpp` yet: each answers `NotSupported`
//! (a probe `false`), as their default did before the frontend was complete. Gate G2 replaces
//! them one by one with the SDK calls; nothing here programs the chip.
use crate::OpenbcmBackend;
use sai_core::backend::types::{
    AsicTemps, ChipLimits, Dest, EcmpId, EgressId, FilterId, FpCounter, FpEntryId, FpGroupId, FpQual, FpRule, IntfId,
    IpPrefix, LagMember, MacAddr, NetifCreated, NetifId, PolicerCfg, PolicerId, RouteTarget, StationId, TableUsage,
    TrapKind, TrunkId, Vrf,
};
use sai_core::backend::{HwPort, Vid};
use sai_core::error::{Error, Result};
use std::net::IpAddr;
use std::os::fd::BorrowedFd;

impl sai_core::backend::ops::Base for OpenbcmBackend {
    // G2
    fn is_fake(&self) -> bool {
        false
    }
    // G2
    fn table_usage(&self) -> Result<TableUsage> {
        Err(Error::NotSupported)
    }
    // G2
    fn chip_limits(&self) -> Result<ChipLimits> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Shell for OpenbcmBackend {
    // G2
    fn run_shell_to(&mut self, _cmd: &str, _fd: BorrowedFd<'_>) -> Result<()> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Temp for OpenbcmBackend {
    // G2
    fn asic_temperatures(&self) -> Result<AsicTemps> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Lag for OpenbcmBackend {
    // G2
    fn lag_create(&mut self) -> Result<TrunkId> {
        Err(Error::NotSupported)
    }
    // G2
    fn lag_destroy(&mut self, _tid: TrunkId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn lag_set_members(&mut self, _tid: TrunkId, _members: &[LagMember]) -> Result<()> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::L2 for OpenbcmBackend {
    // G2
    fn l2_add(&mut self, _mac: MacAddr, _vid: Vid, _hw: Dest, _is_static: bool, _discard: bool) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l2_delete(&mut self, _mac: MacAddr, _vid: Vid) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l2_flush(&mut self, _hw: Option<Dest>, _vid: Option<Vid>, _statics: bool, _dynamics: bool) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l2_age_set(&mut self, _seconds: u32) -> Result<()> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Hostif for OpenbcmBackend {
    // G2
    fn netif_create(
        &mut self,
        _ifname: &str,
        _hw: HwPort,
        _vid: Vid,
        _mac: MacAddr,
        _keep_tag: bool,
    ) -> Result<NetifCreated> {
        Err(Error::NotSupported)
    }
    // G2
    fn netif_destroy(&mut self, _netif_id: NetifId, _filter_id: FilterId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn netif_carrier_set(&mut self, _ifname: &str, _up: bool) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn netif_set_keep_tag(
        &mut self,
        _netif_id: NetifId,
        _hw: HwPort,
        _keep_tag: bool,
        _filter_id: Option<FilterId>,
    ) -> Result<FilterId> {
        Err(Error::NotSupported)
    }
    // G2
    fn trap_set(&mut self, _kind: TrapKind, _to_cpu: bool) -> Result<()> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::L3 for OpenbcmBackend {
    // G2
    fn l3_intf_create(&mut self, _mac: MacAddr, _vid: Vid, _mtu: u32, _vrf: Vrf) -> Result<IntfId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_intf_update(&mut self, _intf_id: IntfId, _mac: MacAddr, _vid: Vid, _mtu: u32, _vrf: Vrf) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_intf_destroy(&mut self, _intf_id: IntfId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_station_add(&mut self, _mac: MacAddr) -> Result<StationId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_station_delete(&mut self, _station_id: StationId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_egress_create(&mut self, _intf_id: IntfId, _dmac: MacAddr, _hw: Dest, _vid: Vid) -> Result<EgressId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_egress_update(
        &mut self,
        _egress_id: EgressId,
        _intf_id: IntfId,
        _dmac: MacAddr,
        _hw: Dest,
        _vid: Vid,
    ) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_egress_destroy(&mut self, _egress_id: EgressId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_egress_cpu(&mut self) -> Result<EgressId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_egress_drop(&mut self) -> Result<EgressId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_host_add(&mut self, _vrf: Vrf, _ip: IpAddr, _egress_id: EgressId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_host_delete(&mut self, _vrf: Vrf, _ip: IpAddr) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_route_add(&mut self, _vrf: Vrf, _p: IpPrefix, _target: RouteTarget, _replace: bool) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_route_delete(&mut self, _vrf: Vrf, _p: IpPrefix) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_ecmp_create(&mut self, _egress: &[EgressId]) -> Result<EcmpId> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_ecmp_update(&mut self, _ecmp_id: EcmpId, _egress: &[EgressId]) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn l3_ecmp_destroy(&mut self, _ecmp_id: EcmpId) -> Result<()> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Field for OpenbcmBackend {
    // G2
    fn fp_group_create(&mut self, _qset: &[FpQual], _priority: i32) -> Result<FpGroupId> {
        Err(Error::NotSupported)
    }
    // G2
    fn fp_group_destroy(&mut self, _gid: FpGroupId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn fp_entry_create(&mut self, _gid: FpGroupId, _r: &FpRule) -> Result<FpEntryId> {
        Err(Error::NotSupported)
    }
    // G2
    fn fp_entry_destroy(&mut self, _eid: FpEntryId) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn fp_entry_counter(&self, _eid: FpEntryId) -> Result<FpCounter> {
        Err(Error::NotSupported)
    }
}

impl sai_core::backend::ops::Policer for OpenbcmBackend {
    // G2
    fn policer_create(&mut self, _cfg: &PolicerCfg) -> Result<PolicerId> {
        Err(Error::NotSupported)
    }
    // G2
    fn policer_set(&mut self, _pid: PolicerId, _cfg: &PolicerCfg) -> Result<()> {
        Err(Error::NotSupported)
    }
    // G2
    fn policer_destroy(&mut self, _pid: PolicerId) -> Result<()> {
        Err(Error::NotSupported)
    }
}

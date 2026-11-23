//! The typed operations of `openbcm_backend.cpp` other than the field processor (`crate::field`):
//! each the C++ override's glue call(s), in its order, with its arguments, logged as its `RV()`
//! logs (the call's C++ text) or not where the C++ does not log.
use crate::{OpenbcmBackend, glue_dest, hw, log, quiet, rv};
use sai_core::backend::types::{
    AsicTemps, ChipLimits, Dest, EcmpId, EgressId, Fec, FilterId, GateVerdict, IntfId, IpPrefix, LagMember, Learn,
    LinkGate, Loopback, MacAddr, MdioAccess, MdioDev, NetifCreated, NetifId, PhyAddr, PortInterface, PortStat,
    RouteTarget, StationId, TableUsage, TrapKind, TrunkId, Vrf,
};
use sai_core::backend::{Base, Hostif, HwPort, L2, L3, Lag, Mdio, PortOps, Shell, Temp, Vid};
use sai_core::error::{Error, Result};
use sai_sys as sys;
use std::ffi::CString;
use std::io::Write as _;
use std::net::IpAddr;
use std::os::fd::BorrowedFd;

/// The glue's temperature read as the SAI layer's.
pub(crate) fn asic_temps(t: sdk_glue::Temperatures) -> AsicTemps {
    AsicTemps {
        cur: t.current().to_vec(),
        peak: t.peak().to_vec(),
    }
}

/// The switch chip's MDIO master for the SAI layer's external-PHY runtime: `sdk-glue`'s cloneable
/// bus (dead once the switch is shut down: `UNINITIALIZED`). Not logged, as the C++
/// `mdio_c45_read` / `_write`.
#[derive(Debug)]
pub(crate) struct Bus(pub(crate) sdk_glue::MdioBus);

fn mmd(dev: MdioDev) -> Result<sdk_glue::Mmd> {
    // Both are 1..=31 by construction.
    sdk_glue::Mmd::new(dev.get()).ok_or(Error::InvalidParameter)
}

impl MdioAccess for Bus {
    fn read(&self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16> {
        quiet(self.0.read(phy.0, mmd(dev)?, reg))
    }

    fn write(&self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()> {
        quiet(self.0.write(phy.0, mmd(dev)?, reg, val))
    }
}

fn vrf(v: Vrf) -> u32 {
    v.get()
}

impl Base for OpenbcmBackend {
    fn is_fake(&self) -> bool {
        false
    }

    fn hardware_info(&self) -> String {
        // ob_sdk_hw_info into a zeroed buffer, its result ignored (as the C++): "" on an error.
        self.up
            .as_ref()
            .map(|up| up.switch.hw_info().unwrap_or_default())
            .unwrap_or_default()
    }

    fn table_usage(&self) -> Result<TableUsage> {
        let t = rv("ob_sdk_table_usage(unit_, &t)", self.up()?.switch.table_usage())?;
        Ok(TableUsage {
            v4_routes_free: t.v4_routes_free,
            v6_routes_free: t.v6_routes_free,
            hosts_free: t.hosts_free,
            egress_free: t.egress_free,
            fdb_free: t.fdb_free,
            ecmp_groups_free: t.ecmp_groups_free,
            ecmp_members_free: t.ecmp_members_free,
        })
    }

    fn chip_limits(&self) -> Result<ChipLimits> {
        // sdk-glue clamps negative sizes to 0, as the C++ does here.
        let c = rv("ob_sdk_chip_limits(unit_, &c)", self.up()?.switch.chip_limits())?;
        Ok(ChipLimits {
            buffer_bytes: c.buffer_bytes,
            acl_prio_min: c.acl_prio_min,
            acl_prio_max: c.acl_prio_max,
            vrf_max: c.vrf_max,
            ecmp_groups: c.ecmp_groups,
            ecmp_paths: c.ecmp_paths,
            ecmp_members: c.ecmp_members,
        })
    }
}

impl Shell for OpenbcmBackend {
    fn run_shell(&mut self, cmd: &str) -> Result<()> {
        rv("ob_sdk_shell(unit_, cmd.c_str())", self.switch()?.run_shell(cmd))
    }

    fn run_shell_to(&mut self, cmd: &str, fd: BorrowedFd<'_>) -> Result<()> {
        rv(
            "ob_sdk_shell_to(unit_, cmd.c_str(), fd)",
            self.switch()?.run_shell_to(cmd, fd),
        )
    }
}

impl Temp for OpenbcmBackend {
    fn asic_temperatures(&self) -> Result<AsicTemps> {
        // Not RV(): the publisher would log every interval while reads fail. One read at a time
        // with the publisher's (sdk-glue's lock, the C++ temp_mu_).
        quiet(self.up()?.switch.temp_reader().read()).map(asic_temps)
    }
}

impl Mdio for OpenbcmBackend {
    fn mdio_c45_read(&self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16> {
        // Not RV(): the caller reports (a poll would log every pass).
        let bus = self.up()?.switch.mdio_bus();
        Bus(bus).read(phy, dev, reg)
    }

    fn mdio_c45_write(&self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()> {
        let bus = self.up()?.switch.mdio_bus();
        Bus(bus).write(phy, dev, reg, val)
    }
}

/// `sai_core`'s port counter as the glue's (the same `ob::PortStat` order).
const fn glue_stat(stat: PortStat) -> sdk_glue::PortStat {
    use sdk_glue::PortStat as G;
    match stat {
        PortStat::InOctets => G::InOctets,
        PortStat::InUcastPkts => G::InUcastPkts,
        PortStat::InNonUcastPkts => G::InNonUcastPkts,
        PortStat::InDiscards => G::InDiscards,
        PortStat::InErrors => G::InErrors,
        PortStat::InUnknownProtos => G::InUnknownProtos,
        PortStat::InBroadcastPkts => G::InBroadcastPkts,
        PortStat::InMulticastPkts => G::InMulticastPkts,
        PortStat::OutOctets => G::OutOctets,
        PortStat::OutUcastPkts => G::OutUcastPkts,
        PortStat::OutNonUcastPkts => G::OutNonUcastPkts,
        PortStat::OutDiscards => G::OutDiscards,
        PortStat::OutErrors => G::OutErrors,
        PortStat::OutBroadcastPkts => G::OutBroadcastPkts,
        PortStat::OutMulticastPkts => G::OutMulticastPkts,
    }
}

/// `sai_core`'s trap kind as the glue's int order (`ob::TrapKind`).
pub(crate) const fn glue_trap(kind: TrapKind) -> sdk_glue::TrapKind {
    use sdk_glue::TrapKind as G;
    match kind {
        TrapKind::ArpRequest => G::ArpRequest,
        TrapKind::ArpResponse => G::ArpResponse,
        TrapKind::Dhcp => G::Dhcp,
        TrapKind::Dhcpv6 => G::Dhcpv6,
        TrapKind::Igmp => G::Igmp,
        TrapKind::Mld => G::Mld,
        TrapKind::Nd => G::Nd,
        TrapKind::Lldp => G::Lldp,
        TrapKind::Lacp => G::Lacp,
        TrapKind::Stp => G::Stp,
        TrapKind::Other => G::Other,
    }
}

impl PortOps for OpenbcmBackend {
    // The SDK's per-port link scan handler: the scan asks it first and reads the ASIC link only
    // when it has no opinion. sdk-glue keeps the closure (the C++ gates_ map) and asks the glue
    // also to remove a gate the port does not have, as the C++.
    fn port_link_gate(&mut self, port: HwPort, gate: Option<LinkGate>) -> Result<()> {
        const CALL: &str = "ob_sdk_linkscan_port_gate(unit_, hw, on ? &OpenbcmBackend::on_gate : nullptr, this)";
        let switch = self.switch()?;
        match gate {
            Some(gate) => rv(
                CALL,
                switch.set_port_gate(hw(port), move || match gate() {
                    GateVerdict::Down => sdk_glue::Gate::Down,
                    GateVerdict::NoOpinion => sdk_glue::Gate::NoOpinion,
                }),
            ),
            None => rv(CALL, switch.clear_port_gate(hw(port))),
        }
    }

    fn port_frame_max_set(&mut self, port: HwPort, bytes: u32) -> Result<()> {
        rv(
            "ob_sdk_port_frame_max_set(unit_, hw, (int)bytes)",
            self.switch()?.port_frame_max_set(hw(port), bytes),
        )
    }

    fn port_fec_set(&mut self, port: HwPort, fec: Fec) -> Result<()> {
        // 1 RS, 2 FC, 0 none: the glue's ob::Fec order.
        let mode = match fec {
            Fec::None => sdk_glue::Fec::None,
            Fec::Rs => sdk_glue::Fec::Rs,
            Fec::Fc => sdk_glue::Fec::Fc,
        };
        rv(
            "ob_sdk_port_fec_set(unit_, hw, f == Fec::Rs ? 1 : f == Fec::Fc ? 2 : 0)",
            self.switch()?.port_fec_set(hw(port), mode),
        )
    }

    fn port_autoneg_set(&mut self, port: HwPort, on: bool) -> Result<()> {
        rv(
            "ob_sdk_port_autoneg_set(unit_, hw, on)",
            self.switch()?.port_autoneg_set(hw(port), on),
        )
    }

    fn port_interface_set(&mut self, port: HwPort, name: PortInterface) -> Result<()> {
        // A name of the table has no NUL byte.
        let name = CString::new(name.name()).map_err(|_| Error::InvalidParameter)?;
        rv(
            "ob_sdk_port_interface_set(unit_, hw, name.c_str())",
            self.switch()?.port_interface_set(hw(port), &name),
        )
    }

    fn port_ifg_set(&mut self, port: HwPort, bits: u32) -> Result<()> {
        rv(
            "ob_sdk_port_ifg_set(unit_, hw, (int)bits)",
            self.switch()?.port_ifg_set(hw(port), bits),
        )
    }

    fn port_loopback_set(&mut self, port: HwPort, lb: Loopback) -> Result<()> {
        // 1 MAC, 2 PHY, 0 none.
        let mode = match lb {
            Loopback::None => sdk_glue::Loopback::None,
            Loopback::Mac => sdk_glue::Loopback::Mac,
            Loopback::Phy => sdk_glue::Loopback::Phy,
        };
        rv(
            "ob_sdk_port_loopback_set(unit_, hw, lb == Loopback::Mac ? 1 : lb == Loopback::Phy ? 2 : 0)",
            self.switch()?.port_loopback_set(hw(port), mode),
        )
    }

    fn port_pvid_set(&mut self, port: HwPort, vid: Vid) -> Result<()> {
        rv(
            "ob_sdk_port_pvid_set(unit_, hw, vid)",
            self.switch()?.port_pvid_set(hw(port), vid.get()),
        )
    }

    fn port_learn_set(&mut self, port: HwPort, mode: Learn) -> Result<()> {
        // 0 hardware, 1 disable, 2 drop, 3 CPU.
        let mode = match mode {
            Learn::Hw => sdk_glue::Learn::Hw,
            Learn::Disable => sdk_glue::Learn::Disable,
            Learn::Drop => sdk_glue::Learn::Drop,
            Learn::Cpu => sdk_glue::Learn::Cpu,
        };
        rv(
            "ob_sdk_port_learn_set(unit_, hw, m == Learn::Disable ? 1 : m == Learn::Drop ? 2 : m == Learn::Cpu ? 3 : 0)",
            self.switch()?.port_learn_set(hw(port), mode),
        )
    }

    fn port_stat_get(&self, port: HwPort, stat: PortStat) -> Result<u64> {
        rv(
            "ob_sdk_port_stat_get(unit_, hw, (int)s, &v)",
            self.up()?.switch.port_stat_get(hw(port), glue_stat(stat)),
        )
    }

    fn port_stat_clear(&mut self, port: HwPort) -> Result<()> {
        rv(
            "ob_sdk_port_stat_clear(unit_, hw)",
            self.switch()?.port_stat_clear(hw(port)),
        )
    }
}

impl Lag for OpenbcmBackend {
    fn lag_create(&mut self) -> Result<TrunkId> {
        rv("ob_sdk_trunk_create(unit_, &tid)", self.switch()?.trunk_create()).map(TrunkId)
    }

    fn lag_destroy(&mut self, tid: TrunkId) -> Result<()> {
        rv("ob_sdk_trunk_destroy(unit_, tid)", self.switch()?.trunk_destroy(tid.0))
    }

    fn lag_set_members(&mut self, tid: TrunkId, members: &[LagMember]) -> Result<()> {
        // flags: 1 egress disable, 2 ingress disable; none empties the trunk.
        let members: Vec<sdk_glue::TrunkMember> = members
            .iter()
            .map(|m| sdk_glue::TrunkMember {
                port: hw(m.hw),
                egress_disable: m.egress_disable,
                ingress_disable: m.ingress_disable,
            })
            .collect();
        rv(
            "ob_sdk_trunk_set(unit_, tid, ports.data(), flags.data(), (int)m.size())",
            self.switch()?.trunk_set(tid.0, &members),
        )
    }
}

impl L2 for OpenbcmBackend {
    fn l2_add(&mut self, mac: MacAddr, vid: Vid, dest: Dest, is_static: bool, discard: bool) -> Result<()> {
        rv(
            "ob_sdk_l2_add(unit_, mac, vid, hw, st, discard)",
            self.switch()?
                .l2_add(mac.0, vid.get(), glue_dest(dest), is_static, discard),
        )
    }

    fn l2_delete(&mut self, mac: MacAddr, vid: Vid) -> Result<()> {
        rv(
            "ob_sdk_l2_delete(unit_, mac, vid)",
            self.switch()?.l2_delete(mac.0, vid.get()),
        )
    }

    fn l2_flush(&mut self, dest: Option<Dest>, vid: Option<Vid>, statics: bool, dynamics: bool) -> Result<()> {
        rv(
            "ob_sdk_l2_flush(unit_, hw, vid, statics, dynamics)",
            self.switch()?
                .l2_flush(dest.map(glue_dest), vid.map(Vid::get), statics, dynamics),
        )
    }

    fn l2_age_set(&mut self, seconds: u32) -> Result<()> {
        rv(
            "ob_sdk_l2_age_set(unit_, (int)seconds)",
            self.switch()?.l2_age_set(seconds),
        )
    }
}

impl Hostif for OpenbcmBackend {
    fn netif_create(
        &mut self,
        ifname: &str,
        port: HwPort,
        vid: Vid,
        mac: MacAddr,
        keep_tag: bool,
    ) -> Result<NetifCreated> {
        let n = rv(
            "ob_sdk_netif_create(unit_, ifname.c_str(), hw, vid, mac, keep_tag, &netif_id, &filter_id)",
            self.switch()?
                .netif_create(ifname, hw(port), vid.get(), mac.0, keep_tag),
        )?;
        Ok(NetifCreated {
            netif_id: NetifId(n.netif),
            filter_id: FilterId(n.filter),
        })
    }

    fn netif_destroy(&mut self, netif_id: NetifId, filter_id: FilterId) -> Result<()> {
        rv(
            "ob_sdk_netif_destroy(unit_, netif_id, filter_id)",
            self.switch()?.netif_destroy(Some(netif_id.0), Some(filter_id.0)),
        )
    }

    fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()> {
        // bcm-knet's procfs: "<ifname>=up|down" sets the netdev carrier (C++ fopen "w", one
        // fprintf, fclose). No glue call; works without an initialised chip, as the C++.
        let path = self.knet_link();
        let Ok(mut file) = std::fs::File::create(path) else {
            log::error(
                sys::SAI_API_HOSTIF,
                format_args!("cannot open {} (is linux-bcm-knet loaded?)", path.display()),
            );
            return Err(Error::Failure);
        };
        let line = format!("{ifname}={}\n", if up { "up" } else { "down" });
        file.write_all(line.as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| Error::Failure)
    }

    fn netif_set_keep_tag(
        &mut self,
        netif_id: NetifId,
        port: HwPort,
        keep_tag: bool,
        filter_id: Option<FilterId>,
    ) -> Result<FilterId> {
        rv(
            "ob_sdk_netif_filter_set(unit_, netif_id, hw, keep_tag, &filter_id)",
            self.switch()?
                .netif_filter_set(netif_id.0, hw(port), keep_tag, filter_id.map(|f| f.0)),
        )
        .map(FilterId)
    }

    fn trap_set(&mut self, kind: TrapKind, to_cpu: bool) -> Result<()> {
        rv(
            "ob_sdk_trap_set(unit_, (int)kind, to_cpu)",
            self.switch()?.trap_set(glue_trap(kind), to_cpu),
        )
    }
}

impl L3 for OpenbcmBackend {
    fn l3_intf_create(&mut self, mac: MacAddr, vid: Vid, mtu: u32, v: Vrf) -> Result<IntfId> {
        rv(
            "ob_sdk_l3_intf_create(unit_, mac, vid, (int)mtu, vrf, &id)",
            self.switch()?.l3_intf_create(mac.0, vid.get(), mtu, vrf(v)),
        )
        .map(IntfId)
    }

    fn l3_intf_update(&mut self, intf_id: IntfId, mac: MacAddr, vid: Vid, mtu: u32, v: Vrf) -> Result<()> {
        rv(
            "ob_sdk_l3_intf_update(unit_, id, mac, vid, (int)mtu, vrf)",
            self.switch()?.l3_intf_update(intf_id.0, mac.0, vid.get(), mtu, vrf(v)),
        )
    }

    fn l3_intf_destroy(&mut self, intf_id: IntfId) -> Result<()> {
        rv(
            "ob_sdk_l3_intf_destroy(unit_, id)",
            self.switch()?.l3_intf_destroy(intf_id.0),
        )
    }

    fn l3_station_add(&mut self, mac: MacAddr) -> Result<StationId> {
        let id = rv(
            "ob_sdk_l3_station_add(unit_, mac, &id)",
            self.switch()?.l3_station_add(mac.0),
        )?;
        log::notice(
            sys::SAI_API_ROUTER_INTERFACE,
            format_args!("my-station {id}: {mac} (IPv4, IPv6, ARP)"),
        );
        Ok(StationId(id))
    }

    fn l3_station_delete(&mut self, station_id: StationId) -> Result<()> {
        rv(
            "ob_sdk_l3_station_delete(unit_, id)",
            self.switch()?.l3_station_delete(station_id.0),
        )
    }

    fn l3_egress_create(&mut self, intf_id: IntfId, dmac: MacAddr, dest: Dest, vid: Vid) -> Result<EgressId> {
        rv(
            "ob_sdk_l3_egress_create(unit_, intf, dmac, hw, vid, &id)",
            self.switch()?
                .l3_egress_create(intf_id.0, dmac.0, glue_dest(dest), vid.get()),
        )
        .map(EgressId)
    }

    fn l3_egress_update(
        &mut self,
        egress_id: EgressId,
        intf_id: IntfId,
        dmac: MacAddr,
        dest: Dest,
        vid: Vid,
    ) -> Result<()> {
        rv(
            "ob_sdk_l3_egress_update(unit_, id, intf, dmac, hw, vid)",
            self.switch()?
                .l3_egress_update(egress_id.0, intf_id.0, dmac.0, glue_dest(dest), vid.get()),
        )
    }

    fn l3_egress_destroy(&mut self, egress_id: EgressId) -> Result<()> {
        let up = self.up_mut()?;
        // The shared CPU / drop objects are never destroyed (the glue keeps handing them out).
        if egress_id == up.cpu_egress || egress_id == up.drop_egress {
            return Err(Error::InvalidParameter);
        }
        rv(
            "ob_sdk_l3_egress_destroy(unit_, id)",
            up.switch.l3_egress_destroy(egress_id.0),
        )
    }

    fn l3_egress_cpu(&mut self) -> Result<EgressId> {
        // C++: Failure while the id is -1; here it exists whenever the chip is up.
        self.up().map(|up| up.cpu_egress).map_err(|_| Error::Failure)
    }

    fn l3_egress_drop(&mut self) -> Result<EgressId> {
        self.up().map(|up| up.drop_egress).map_err(|_| Error::Failure)
    }

    fn l3_host_add(&mut self, v: Vrf, ip: IpAddr, egress_id: EgressId) -> Result<()> {
        rv(
            "ob_sdk_l3_host_add(unit_, vrf, ip.v6, ip.addr, egress)",
            self.switch()?.l3_host_add(vrf(v), ip, egress_id.0),
        )
    }

    fn l3_host_delete(&mut self, v: Vrf, ip: IpAddr) -> Result<()> {
        rv(
            "ob_sdk_l3_host_delete(unit_, vrf, ip.v6, ip.addr)",
            self.switch()?.l3_host_delete(vrf(v), ip),
        )
    }

    fn l3_route_add(&mut self, v: Vrf, p: IpPrefix, target: RouteTarget, replace: bool) -> Result<()> {
        let up = self.up_mut()?;
        // The drop egress as a plain next hop: the route drops.
        let drop = target == RouteTarget::Egress(up.drop_egress);
        let target = match target {
            RouteTarget::Egress(e) => sdk_glue::RouteTarget::Egress(e.0),
            RouteTarget::Ecmp(e) => sdk_glue::RouteTarget::Ecmp(e.0),
        };
        rv(
            "ob_sdk_l3_route_add(unit_, vrf, p.addr.v6, p.addr.addr, p.mask.addr, target, ecmp, !ecmp && target == drop_egress_, replace)",
            up.switch
                .l3_route_add(vrf(v), p.addr(), p.mask(), target, drop, replace),
        )
    }

    fn l3_route_delete(&mut self, v: Vrf, p: IpPrefix) -> Result<()> {
        rv(
            "ob_sdk_l3_route_delete(unit_, vrf, p.addr.v6, p.addr.addr, p.mask.addr)",
            self.switch()?.l3_route_delete(vrf(v), p.addr(), p.mask()),
        )
    }

    fn l3_ecmp_create(&mut self, egress: &[EgressId]) -> Result<EcmpId> {
        let members: Vec<u32> = egress.iter().map(|e| e.0).collect();
        rv(
            "ob_sdk_l3_ecmp_set(unit_, &id, 0, m.data(), (int)m.size())",
            self.switch()?.l3_ecmp_create(&members),
        )
        .map(EcmpId)
    }

    fn l3_ecmp_update(&mut self, ecmp_id: EcmpId, egress: &[EgressId]) -> Result<()> {
        let members: Vec<u32> = egress.iter().map(|e| e.0).collect();
        rv(
            "ob_sdk_l3_ecmp_set(unit_, &id, 1, m.data(), (int)m.size())",
            self.switch()?.l3_ecmp_update(ecmp_id.0, &members),
        )
    }

    fn l3_ecmp_destroy(&mut self, ecmp_id: EcmpId) -> Result<()> {
        rv(
            "ob_sdk_l3_ecmp_destroy(unit_, id)",
            self.switch()?.l3_ecmp_destroy(ecmp_id.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{glue_stat, glue_trap};
    use sai_core::backend::types::{PortStat, TrapKind};

    #[test]
    fn the_orders_are_the_glues() {
        for (i, stat) in PortStat::ALL.into_iter().enumerate() {
            assert_eq!(usize::try_from(glue_stat(stat).raw()).ok(), Some(i), "{stat:?}");
        }
        let kinds = [
            TrapKind::ArpRequest,
            TrapKind::ArpResponse,
            TrapKind::Dhcp,
            TrapKind::Dhcpv6,
            TrapKind::Igmp,
            TrapKind::Mld,
            TrapKind::Nd,
            TrapKind::Lldp,
            TrapKind::Lacp,
            TrapKind::Stp,
            TrapKind::Other,
        ];
        assert_eq!(kinds.len(), sdk_glue::TrapKind::ALL.len());
        for (i, kind) in kinds.into_iter().enumerate() {
            assert_eq!(usize::try_from(glue_trap(kind).raw()).ok(), Some(i), "{kind:?}");
        }
    }
}

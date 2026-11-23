//! The typed operations of the virtual chip ([`crate::backend::ops`]), as `fake_backend.cpp`
//! has them: the same checks, the same answers (with the differences listed in [`super`]).
use super::{EgressKind, FpEntry, FpGroup, Genl, Intf, KnetNetif, L2Entry, VirtualBackend, take};
use crate::backend::ops::{
    Base, Field, Hash, Hostif, L2, L3, Lag, Mdio, Mirror, Policer, PortOps, Qos, Sample, Shell, Storm, Temp,
};
use crate::backend::types::{
    AsicTemps, BufferCfg, ChipLimits, Dest, EcmpId, EgressId, Fec, FilterId, FloodKind, FpCounter, FpEntryId,
    FpGroupId, FpQual, FpRule, GenlId, IntfId, IpPrefix, LagMember, Learn, LinkGate, Loopback, MacAddr, MdioDev,
    MirrorCfg, MirrorId, NetifCreated, NetifId, PhyAddr, PolicerCfg, PolicerId, PortInterface, PortStat, QosMapEntry,
    QosMapId, QosMapKind, RouteTarget, SampleId, SchedCfg, StationId, StormKind, TableUsage, TrapKind, TrapSpec,
    TrunkId, Vrf, WredCfg, WredId,
};
use crate::backend::{HwPort, StatsObject, Vid};
use crate::error::{Error, Result};
use sai_sys as sys;
use std::io::Write as _;
use std::net::IpAddr;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

// The C++ fake's capacities (fake_backend.cpp).
const V4_ROUTES: u32 = 16384;
const V6_ROUTES: u32 = 8192;
const HOSTS: u32 = 16384;
const EGRESS: u32 = 32768;
const FDB: u32 = 32768;
const VRF_MAX: u32 = 1023;
const ECMP_GROUPS: u32 = 4096;
const ECMP_PATHS: usize = 64;
const ECMP_MEMBERS: usize = 16384;
const BUFFER_BYTES: u64 = 20165 * 4 * 208;
const ACL_PRIO_MAX: u32 = 0x7fff_ffff;
const FP_GROUPS: usize = 12;
const FP_ENTRIES: usize = 2048;
const POLICERS: usize = 1024;
const LAGS: usize = 128;
const LAG_MEMBERS: usize = 64;
/// Queues and priority groups per port: 0..=7.
const QUEUE_MAX: u8 = 7;

fn left(capacity: u32, used: usize) -> u32 {
    capacity.saturating_sub(u32::try_from(used).unwrap_or(u32::MAX))
}

impl VirtualBackend {
    fn vrf_ok(vrf: Vrf) -> Result<()> {
        if vrf.get() <= VRF_MAX {
            Ok(())
        } else {
            Err(Error::InvalidParameter)
        }
    }

    /// An egress object that forwards (not the CPU / drop one).
    fn forwarding(&self, id: EgressId) -> bool {
        matches!(self.l3.egress.get(&id), Some(EgressKind::Forward { .. }))
    }

    fn ecmp_members_ok(&self, members: &[EgressId]) -> Result<()> {
        if members.is_empty() || members.len() > ECMP_PATHS || !members.iter().all(|m| self.forwarding(*m)) {
            return Err(Error::InvalidParameter);
        }
        Ok(())
    }

    fn queue_ok(&self, hw: HwPort, queue: u8) -> Result<()> {
        if self.ports.contains_key(&hw) && queue <= QUEUE_MAX {
            Ok(())
        } else {
            Err(Error::InvalidParameter)
        }
    }

    fn knet_netif(&mut self, ifname: &str) -> Option<&mut KnetNetif> {
        self.knet.values_mut().find(|n| n.name == ifname)
    }
}

fn policer_cfg_ok(c: &PolicerCfg) -> bool {
    // A meter that passes nothing is a mistake; RFC 2698: PIR >= CIR.
    !(c.cir == 0 && c.cbs == 0) && (!c.tr_tcm || c.pir >= c.cir)
}

const fn fake_queue_stat(id: sys::sai_stat_id_t) -> bool {
    matches!(
        id,
        sys::SAI_QUEUE_STAT_PACKETS | sys::SAI_QUEUE_STAT_BYTES | sys::SAI_QUEUE_STAT_DROPPED_PACKETS
    )
}

const fn fake_pg_stat(id: sys::sai_stat_id_t) -> bool {
    matches!(
        id,
        sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS | sys::SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES
    )
}

impl Base for VirtualBackend {
    fn is_fake(&self) -> bool {
        true
    }

    fn hardware_info(&self) -> String {
        "fake".to_owned()
    }

    fn table_usage(&self) -> Result<TableUsage> {
        self.check("table_usage")?;
        let v4 = self.l3.routes.keys().filter(|(_, p)| p.addr().is_ipv4()).count();
        let v6 = self.l3.routes.len() - v4;
        let host_units: usize = self
            .l3
            .hosts
            .keys()
            .map(|(_, ip)| if ip.is_ipv6() { 2 } else { 1 })
            .sum();
        let members: usize = self.l3.ecmps.values().map(Vec::len).sum();
        Ok(TableUsage {
            v4_routes_free: left(V4_ROUTES, v4),
            v6_routes_free: left(V6_ROUTES, v6),
            hosts_free: left(HOSTS, host_units),
            egress_free: left(EGRESS, self.l3.egress.len()),
            fdb_free: left(FDB, self.l2.len()),
            ecmp_groups_free: left(ECMP_GROUPS, self.l3.ecmps.len()),
            ecmp_members_free: left(u32::try_from(ECMP_MEMBERS).unwrap_or(u32::MAX), members),
        })
    }

    fn chip_limits(&self) -> Result<ChipLimits> {
        self.check("chip_limits")?;
        Ok(ChipLimits {
            buffer_bytes: BUFFER_BYTES,
            acl_prio_min: 0,
            acl_prio_max: ACL_PRIO_MAX,
            vrf_max: VRF_MAX,
            ecmp_groups: ECMP_GROUPS,
            ecmp_paths: u32::try_from(ECMP_PATHS).unwrap_or(u32::MAX),
            ecmp_members: u32::try_from(ECMP_MEMBERS).unwrap_or(u32::MAX),
        })
    }
}

/// How the virtual diag shell answers `phy raw c45 PHY DEV REG` (a read through the SDK's MDIO
/// master on the real chip): the C++ fake's echo, or the SDK's answer from the MDIO model, which
/// lets the MDIO tool's `--verify-only --diag-socket` run against the virtual chip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellMdio {
    /// `fake: <cmd>` for every command, as the C++ fake (the default).
    #[default]
    Echo,
    /// Reads answered `\t0x<reg>: 0x<value>\n` (the value through [`Mdio::mdio_c45_read`]: logged,
    /// a dead PHY fails the command); anything else echoed.
    Answer,
}

/// `phy raw c45 PHY DEV REG` (numbers `0x` hex or decimal, words in any case), as a read.
fn phy_raw_c45_read(cmd: &str) -> Option<(PhyAddr, MdioDev, u16)> {
    fn number(word: &str) -> Option<u32> {
        match word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => word.parse().ok(),
        }
    }
    let words: Vec<&str> = cmd.split_ascii_whitespace().collect();
    let [phy_word, raw, c45, phy, dev, reg] = words.as_slice() else {
        return None;
    };
    let keywords = [(phy_word, "phy"), (raw, "raw"), (c45, "c45")];
    if !keywords
        .iter()
        .all(|(word, keyword)| word.eq_ignore_ascii_case(keyword))
    {
        return None;
    }
    let dev = MdioDev::new(u8::try_from(number(dev)?).ok()?)?;
    Some((PhyAddr(number(phy)?), dev, u16::try_from(number(reg)?).ok()?))
}

impl Shell for VirtualBackend {
    fn run_shell(&mut self, cmd: &str) -> Result<()> {
        self.check("run_shell")?;
        self.shell_log.push(cmd.to_owned());
        Ok(())
    }

    /// Logs the command and writes `fake: <cmd>\n` to `fd`; with [`ShellMdio::Answer`], a
    /// `phy raw c45 PHY DEV REG` read gets the SDK shell's answer from the MDIO model instead.
    fn run_shell_to(&mut self, cmd: &str, fd: BorrowedFd<'_>) -> Result<()> {
        self.check("run_shell_to")?;
        self.shell_log.push(cmd.to_owned());
        let answer = match (self.shell_mdio, phy_raw_c45_read(cmd)) {
            (ShellMdio::Answer, Some((phy, dev, reg))) => {
                let val = self.mdio_c45_read(phy, dev, reg)?;
                format!("\t0x{reg:04x}: 0x{val:04x}\n")
            }
            _ => format!("fake: {cmd}\n"),
        };
        let mut out = std::fs::File::from(fd.try_clone_to_owned().map_err(|_| Error::Failure)?);
        out.write_all(answer.as_bytes()).map_err(|_| Error::Failure)
    }
}

impl Temp for VirtualBackend {
    /// The monitors' values; the peak is 0.5 C above.
    fn asic_temperatures(&self) -> Result<AsicTemps> {
        self.check("asic_temperatures")?;
        self.temp_model().read()
    }
}

impl Mdio for VirtualBackend {
    // One implementation for the backend and its handle ([`VirtualBackend::bus`]).
    fn mdio_c45_read(&self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16> {
        crate::backend::types::MdioAccess::read(&self.bus(), phy, dev, reg)
    }

    fn mdio_c45_write(&self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()> {
        crate::backend::types::MdioAccess::write(&self.bus(), phy, dev, reg, val)
    }
}

impl PortOps for VirtualBackend {
    fn port_link_gate(&mut self, hw: HwPort, gate: Option<LinkGate>) -> Result<()> {
        self.check("port_link_gate")?;
        self.port(hw)?;
        match gate {
            Some(gate) => self.gates.0.insert(hw, gate),
            None => self.gates.0.remove(&hw),
        };
        Ok(())
    }

    fn port_frame_max_set(&mut self, hw: HwPort, bytes: u32) -> Result<()> {
        self.check("port_frame_max_set")?;
        let port = self.port_mut(hw)?;
        if !(64..=9416).contains(&bytes) {
            return Err(Error::InvalidParameter);
        }
        port.frame_max = bytes;
        Ok(())
    }

    fn port_fec_set(&mut self, hw: HwPort, fec: Fec) -> Result<()> {
        self.check("port_fec_set")?;
        self.port_mut(hw)?.fec = fec;
        Ok(())
    }

    fn port_autoneg_set(&mut self, hw: HwPort, on: bool) -> Result<()> {
        self.check("port_autoneg_set")?;
        self.port_mut(hw)?.autoneg = on;
        Ok(())
    }

    /// As the chip: a four-lane interface type needs a four-lane port.
    fn port_interface_set(&mut self, hw: HwPort, name: PortInterface) -> Result<()> {
        self.check("port_interface_set")?;
        let port = self.port_mut(hw)?;
        if name.four_lanes() && port.lanes.len() != 4 {
            return Err(Error::InvalidParameter);
        }
        port.iface = Some(name);
        port.quirk_calls += 1;
        Ok(())
    }

    fn port_ifg_set(&mut self, hw: HwPort, bits: u32) -> Result<()> {
        self.check("port_ifg_set")?;
        let port = self.port_mut(hw)?;
        port.ifg = bits;
        port.quirk_calls += 1;
        Ok(())
    }

    /// In loopback the link comes up with the port enabled, whatever the profile's link mode.
    fn port_loopback_set(&mut self, hw: HwPort, lb: Loopback) -> Result<()> {
        self.check("port_loopback_set")?;
        self.port_mut(hw)?.lb = lb;
        self.update_link(hw);
        Ok(())
    }

    fn port_pvid_set(&mut self, hw: HwPort, vid: Vid) -> Result<()> {
        self.check("port_pvid_set")?;
        self.port_mut(hw)?.pvid = vid;
        Ok(())
    }

    fn port_learn_set(&mut self, hw: HwPort, mode: Learn) -> Result<()> {
        self.check("port_learn_set")?;
        self.port_mut(hw)?.learn = mode;
        Ok(())
    }

    fn port_stat_get(&self, hw: HwPort, stat: PortStat) -> Result<u64> {
        self.check("port_stat_get")?;
        self.port(hw)?;
        Ok(self
            .counters
            .get(&(StatsObject::Port(hw), stat.sai_id()))
            .copied()
            .unwrap_or(0))
    }

    fn port_stat_clear(&mut self, hw: HwPort) -> Result<()> {
        self.check("port_stat_clear")?;
        self.port(hw)?;
        self.counters.retain(|(object, _), _| *object != StatsObject::Port(hw));
        Ok(())
    }

    fn port_pfc_set(&mut self, hw: HwPort, separate: bool, tx: u8, rx: u8) -> Result<()> {
        self.check("port_pfc_set")?;
        self.port(hw)?;
        self.qos.pfc.insert(hw, (separate, tx, rx));
        Ok(())
    }

    fn port_pfc_stat_get(&self, hw: HwPort, prio: u8, tx: bool) -> Result<u64> {
        self.check("port_pfc_stat_get")?;
        Ok(self.qos.pfc_stats.get(&(hw, prio, tx)).copied().unwrap_or(0))
    }
}

impl Lag for VirtualBackend {
    fn lag_create(&mut self) -> Result<TrunkId> {
        self.check("lag_create")?;
        if self.lags.len() >= LAGS {
            return Err(Error::InsufficientResources);
        }
        let tid = TrunkId(take(&mut self.next.tid));
        self.lags.insert(tid, Vec::new());
        Ok(tid)
    }

    fn lag_destroy(&mut self, tid: TrunkId) -> Result<()> {
        self.check("lag_destroy")?;
        self.lags.remove(&tid).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    /// A port is in one trunk at most; 64 members at most.
    fn lag_set_members(&mut self, tid: TrunkId, members: &[LagMember]) -> Result<()> {
        self.check("lag_set_members")?;
        if !self.lags.contains_key(&tid) {
            return Err(Error::ItemNotFound);
        }
        if members.len() > LAG_MEMBERS {
            return Err(Error::InvalidParameter);
        }
        for m in members {
            self.port(m.hw)?;
            let elsewhere = self
                .lags
                .iter()
                .any(|(other, list)| *other != tid && list.iter().any(|x| x.hw == m.hw));
            if elsewhere {
                return Err(Error::InvalidParameter);
            }
        }
        self.lags.insert(tid, members.to_vec());
        Ok(())
    }
}

impl L2 for VirtualBackend {
    /// The VLAN must exist; the destination is a port, a LAG or the CPU port.
    fn l2_add(&mut self, mac: MacAddr, vid: Vid, hw: Dest, is_static: bool, discard: bool) -> Result<()> {
        self.check("l2_add")?;
        if !self.vlans.contains_key(&vid) {
            return Err(Error::InvalidParameter);
        }
        if hw != Dest::Port(HwPort(0)) && !self.dest_ok(hw) {
            return Err(Error::InvalidParameter);
        }
        self.l2.insert(
            (mac, vid),
            L2Entry {
                dest: hw,
                is_static,
                discard,
            },
        );
        Ok(())
    }

    fn l2_delete(&mut self, mac: MacAddr, vid: Vid) -> Result<()> {
        self.check("l2_delete")?;
        self.l2.remove(&(mac, vid)).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn l2_flush(&mut self, hw: Option<Dest>, vid: Option<Vid>, statics: bool, dynamics: bool) -> Result<()> {
        self.check("l2_flush")?;
        self.l2.retain(|(_, entry_vid), entry| {
            let matches = hw.is_none_or(|d| entry.dest == d)
                && vid.is_none_or(|v| *entry_vid == v)
                && if entry.is_static { statics } else { dynamics };
            !matches
        });
        Ok(())
    }

    fn l2_age_set(&mut self, seconds: u32) -> Result<()> {
        self.check("l2_age_set")?;
        self.age_seconds = seconds;
        Ok(())
    }
}

impl Hostif for VirtualBackend {
    /// A model entry, and in mode `tap` a TAP netdev of that name (carrier off).
    fn netif_create(
        &mut self,
        ifname: &str,
        hw: HwPort,
        vid: Vid,
        mac: MacAddr,
        keep_tag: bool,
    ) -> Result<NetifCreated> {
        self.check("netif_create")?;
        let gate = Arc::clone(&self.port(hw)?.up);
        if ifname.is_empty() || ifname.len() > 15 {
            return Err(Error::InvalidParameter);
        }
        if self.netdev_name_taken(ifname) {
            return Err(Error::ItemAlreadyExists);
        }
        let kernel = self.open_kernel(ifname, hw, mac.0, &gate, false)?;
        let filter = FilterId(take(&mut self.next.filter));
        let netif_id = NetifId(take(&mut self.next.netif));
        self.knet.insert(
            netif_id,
            KnetNetif {
                name: ifname.to_owned(),
                hw,
                vid,
                mac,
                keep_tag,
                carrier: false,
                filter,
                kernel,
            },
        );
        Ok(NetifCreated {
            netif_id,
            filter_id: filter,
        })
    }

    fn netif_destroy(&mut self, netif_id: NetifId, filter_id: FilterId) -> Result<()> {
        self.check("netif_destroy")?;
        // The filter goes with its netif (the C++ fake ignores the id too).
        let _ = filter_id;
        self.knet.remove(&netif_id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn netif_carrier_set(&mut self, ifname: &str, up: bool) -> Result<()> {
        self.check("netif_carrier_set")?;
        let netif = self.knet_netif(ifname).ok_or(Error::ItemNotFound)?;
        if let Some(kernel) = &netif.kernel {
            kernel.set_carrier(up)?;
        }
        netif.carrier = up;
        Ok(())
    }

    /// A new filter id; the old one goes with it (the C++ fake ignores `filter_id` too).
    fn netif_set_keep_tag(
        &mut self,
        netif_id: NetifId,
        hw: HwPort,
        keep_tag: bool,
        filter_id: Option<FilterId>,
    ) -> Result<FilterId> {
        self.check("netif_set_keep_tag")?;
        let _ = filter_id;
        if self.knet.get(&netif_id).is_none_or(|n| n.hw != hw) {
            return Err(Error::ItemNotFound);
        }
        let filter = FilterId(take(&mut self.next.filter));
        if let Some(netif) = self.knet.get_mut(&netif_id) {
            netif.keep_tag = keep_tag;
            netif.filter = filter;
        }
        Ok(filter)
    }

    fn trap_set(&mut self, kind: TrapKind, to_cpu: bool) -> Result<()> {
        self.check("trap_set")?;
        if kind == TrapKind::Other {
            return Err(Error::NotSupported);
        }
        self.traps.insert(kind, to_cpu);
        Ok(())
    }

    /// The C++ fake has none (the `ob::Backend` default, `NotSupported`): the SAI layer takes
    /// the `trap_set` + `CoPP` path. Fault injection applies (another error is final there).
    fn trap_program(&mut self, t: &TrapSpec, remove: bool) -> Result<()> {
        self.check("trap_program")?;
        let _ = (t, remove);
        Err(Error::NotSupported)
    }

    /// Only with the mirror model (`mirror_on`).
    fn genetlink_create(&mut self, name: &str, mcgrp: &str) -> Result<GenlId> {
        self.check("genetlink_create")?;
        if !self.toggles.mirror_on {
            return Err(Error::NotSupported);
        }
        let id = GenlId(self.next_qos_id());
        self.mirror.genls.insert(
            id,
            Genl {
                name: name.to_owned(),
                mcgrp: mcgrp.to_owned(),
                traps: std::collections::BTreeSet::new(),
            },
        );
        Ok(id)
    }

    fn genetlink_destroy(&mut self, id: GenlId) -> Result<()> {
        self.check("genetlink_destroy")?;
        self.mirror.genls.remove(&id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn genetlink_trap_set(&mut self, id: GenlId, trap_type: i32, add: bool) -> Result<()> {
        self.check("genetlink_trap_set")?;
        let genl = self.mirror.genls.get_mut(&id).ok_or(Error::ItemNotFound)?;
        if add {
            genl.traps.insert(trap_type);
        } else {
            genl.traps.remove(&trap_type);
        }
        Ok(())
    }
}

impl L3 for VirtualBackend {
    fn l3_intf_create(&mut self, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<IntfId> {
        self.check("l3_intf_create")?;
        if !self.vlans.contains_key(&vid) {
            return Err(Error::InvalidParameter);
        }
        Self::vrf_ok(vrf)?;
        let id = IntfId(take(&mut self.next.intf));
        self.l3.intfs.insert(id, Intf { mac, vid, mtu, vrf });
        Ok(id)
    }

    fn l3_intf_update(&mut self, intf_id: IntfId, mac: MacAddr, vid: Vid, mtu: u32, vrf: Vrf) -> Result<()> {
        self.check("l3_intf_update")?;
        let intf = self.l3.intfs.get_mut(&intf_id).ok_or(Error::ItemNotFound)?;
        *intf = Intf { mac, vid, mtu, vrf };
        Ok(())
    }

    fn l3_intf_destroy(&mut self, intf_id: IntfId) -> Result<()> {
        self.check("l3_intf_destroy")?;
        let in_use = self
            .l3
            .egress
            .values()
            .any(|e| matches!(e, EgressKind::Forward { intf, .. } if *intf == intf_id));
        if in_use {
            return Err(Error::ObjectInUse);
        }
        self.l3.intfs.remove(&intf_id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn l3_station_add(&mut self, mac: MacAddr) -> Result<StationId> {
        self.check("l3_station_add")?;
        let id = StationId(take(&mut self.next.station));
        self.l3.stations.insert(id, mac);
        Ok(id)
    }

    fn l3_station_delete(&mut self, station_id: StationId) -> Result<()> {
        self.check("l3_station_delete")?;
        self.l3
            .stations
            .remove(&station_id)
            .map(|_| ())
            .ok_or(Error::ItemNotFound)
    }

    fn l3_egress_create(&mut self, intf_id: IntfId, dmac: MacAddr, hw: Dest, vid: Vid) -> Result<EgressId> {
        self.check("l3_egress_create")?;
        if !self.l3.intfs.contains_key(&intf_id) {
            return Err(Error::InvalidParameter);
        }
        if !self.dest_ok(hw) {
            return Err(Error::InvalidPortNumber);
        }
        let id = EgressId(take(&mut self.next.egress));
        self.l3.egress.insert(
            id,
            EgressKind::Forward {
                intf: intf_id,
                dmac,
                dest: hw,
                vid,
            },
        );
        Ok(id)
    }

    fn l3_egress_update(
        &mut self,
        egress_id: EgressId,
        intf_id: IntfId,
        dmac: MacAddr,
        hw: Dest,
        vid: Vid,
    ) -> Result<()> {
        self.check("l3_egress_update")?;
        if !self.forwarding(egress_id) {
            return Err(Error::ItemNotFound);
        }
        if !self.l3.intfs.contains_key(&intf_id) {
            return Err(Error::InvalidParameter);
        }
        if !self.dest_ok(hw) {
            return Err(Error::InvalidPortNumber);
        }
        self.l3.egress.insert(
            egress_id,
            EgressKind::Forward {
                intf: intf_id,
                dmac,
                dest: hw,
                vid,
            },
        );
        Ok(())
    }

    fn l3_egress_destroy(&mut self, egress_id: EgressId) -> Result<()> {
        self.check("l3_egress_destroy")?;
        if Some(egress_id) == self.l3.cpu_egress || Some(egress_id) == self.l3.drop_egress {
            return Err(Error::InvalidParameter);
        }
        let in_use = self.l3.hosts.values().any(|e| *e == egress_id)
            || self.l3.routes.values().any(|t| *t == RouteTarget::Egress(egress_id))
            || self.l3.ecmps.values().any(|m| m.contains(&egress_id));
        if in_use {
            return Err(Error::ObjectInUse);
        }
        self.l3.egress.remove(&egress_id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn l3_egress_cpu(&mut self) -> Result<EgressId> {
        self.check("l3_egress_cpu")?;
        if let Some(id) = self.l3.cpu_egress {
            return Ok(id);
        }
        let id = EgressId(take(&mut self.next.egress));
        self.l3.egress.insert(id, EgressKind::Cpu);
        self.l3.cpu_egress = Some(id);
        Ok(id)
    }

    fn l3_egress_drop(&mut self) -> Result<EgressId> {
        self.check("l3_egress_drop")?;
        if let Some(id) = self.l3.drop_egress {
            return Ok(id);
        }
        let id = EgressId(take(&mut self.next.egress));
        self.l3.egress.insert(id, EgressKind::Drop);
        self.l3.drop_egress = Some(id);
        Ok(id)
    }

    fn l3_host_add(&mut self, vrf: Vrf, ip: IpAddr, egress_id: EgressId) -> Result<()> {
        self.check("l3_host_add")?;
        if !self.l3.egress.contains_key(&egress_id) {
            return Err(Error::InvalidParameter);
        }
        Self::vrf_ok(vrf)?;
        if self.l3.hosts.contains_key(&(vrf, ip)) {
            return Err(Error::ItemAlreadyExists);
        }
        self.l3.hosts.insert((vrf, ip), egress_id);
        Ok(())
    }

    fn l3_host_delete(&mut self, vrf: Vrf, ip: IpAddr) -> Result<()> {
        self.check("l3_host_delete")?;
        self.l3.hosts.remove(&(vrf, ip)).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn l3_route_add(&mut self, vrf: Vrf, p: IpPrefix, target: RouteTarget, replace: bool) -> Result<()> {
        self.check("l3_route_add")?;
        let exists = match target {
            RouteTarget::Egress(id) => self.l3.egress.contains_key(&id),
            RouteTarget::Ecmp(id) => self.l3.ecmps.contains_key(&id),
        };
        if !exists {
            return Err(Error::InvalidParameter);
        }
        Self::vrf_ok(vrf)?;
        match (self.l3.routes.contains_key(&(vrf, p)), replace) {
            (true, false) => return Err(Error::ItemAlreadyExists),
            (false, true) => return Err(Error::ItemNotFound),
            _ => {}
        }
        self.l3.routes.insert((vrf, p), target);
        Ok(())
    }

    fn l3_route_delete(&mut self, vrf: Vrf, p: IpPrefix) -> Result<()> {
        self.check("l3_route_delete")?;
        self.l3.routes.remove(&(vrf, p)).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    /// Members are egress objects that forward, 1..=64 of them; 4096 groups, 16384 members.
    fn l3_ecmp_create(&mut self, egress: &[EgressId]) -> Result<EcmpId> {
        self.check("l3_ecmp_create")?;
        self.ecmp_members_ok(egress)?;
        let used: usize = self.l3.ecmps.values().map(Vec::len).sum();
        if self.l3.ecmps.len() >= ECMP_GROUPS as usize || used + egress.len() > ECMP_MEMBERS {
            return Err(Error::TableFull);
        }
        let id = EcmpId(take(&mut self.next.ecmp));
        self.l3.ecmps.insert(id, egress.to_vec());
        Ok(id)
    }

    fn l3_ecmp_update(&mut self, ecmp_id: EcmpId, egress: &[EgressId]) -> Result<()> {
        self.check("l3_ecmp_update")?;
        if !self.l3.ecmps.contains_key(&ecmp_id) {
            return Err(Error::ItemNotFound);
        }
        self.ecmp_members_ok(egress)?;
        self.l3.ecmps.insert(ecmp_id, egress.to_vec());
        Ok(())
    }

    fn l3_ecmp_destroy(&mut self, ecmp_id: EcmpId) -> Result<()> {
        self.check("l3_ecmp_destroy")?;
        if self.l3.routes.values().any(|t| *t == RouteTarget::Ecmp(ecmp_id)) {
            return Err(Error::ObjectInUse);
        }
        self.l3.ecmps.remove(&ecmp_id).map(|_| ()).ok_or(Error::ItemNotFound)
    }
}

/// Test hooks of the L3 tables (the test port's `backend_host_insert` / `backend_host_erase`).
impl VirtualBackend {
    /// Puts a host entry into the table directly (as if the SDK had one the SAI layer does not
    /// know; an IPv6 host counts two in `table_usage`).
    pub fn host_insert(&mut self, vrf: Vrf, ip: IpAddr, egress: EgressId) {
        self.l3.hosts.insert((vrf, ip), egress);
    }

    /// Takes a host entry out of the table directly.
    pub fn host_erase(&mut self, vrf: Vrf, ip: IpAddr) {
        self.l3.hosts.remove(&(vrf, ip));
    }
}

impl Field for VirtualBackend {
    /// Twelve groups (Tomahawk IFP slices).
    fn fp_group_create(&mut self, qset: &[FpQual], priority: i32) -> Result<FpGroupId> {
        self.check("fp_group_create")?;
        if self.fp.groups.len() >= FP_GROUPS {
            return Err(Error::InsufficientResources);
        }
        let qset = qset.iter().map(|q| q.bit()).fold(0, |a, b| a | b);
        let gid = FpGroupId(take(&mut self.next.fp_group));
        self.fp.groups.insert(gid, FpGroup { qset, priority });
        Ok(gid)
    }

    fn fp_group_destroy(&mut self, gid: FpGroupId) -> Result<()> {
        self.check("fp_group_destroy")?;
        if self.fp.entries.values().any(|e| e.gid == gid) {
            return Err(Error::ObjectInUse);
        }
        self.fp.groups.remove(&gid).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    /// Qualifiers must be in the group's set; 2048 entries; the policer and ports must exist.
    fn fp_entry_create(&mut self, gid: FpGroupId, r: &FpRule) -> Result<FpEntryId> {
        self.check("fp_entry_create")?;
        let group = self.fp.groups.get(&gid).ok_or(Error::ItemNotFound)?;
        if r.m.present() & !group.qset != 0 {
            return Err(Error::NotSupported);
        }
        if self.fp.entries.len() >= FP_ENTRIES {
            return Err(Error::TableFull);
        }
        if r.policer.is_some_and(|p| !self.fp.policers.contains_key(&p)) {
            return Err(Error::InvalidParameter);
        }
        let ports = r.m.in_ports.as_deref().unwrap_or(&[]);
        if ports.iter().any(|p| !self.ports.contains_key(p)) {
            return Err(Error::InvalidPortNumber);
        }
        if r.m.in_ports.as_ref().is_some_and(Vec::is_empty) {
            return Err(Error::InvalidParameter);
        }
        let eid = FpEntryId(take(&mut self.next.fp_entry));
        self.fp.entries.insert(
            eid,
            FpEntry {
                gid,
                rule: r.clone(),
                packets: 0,
                bytes: 0,
            },
        );
        Ok(eid)
    }

    fn fp_entry_destroy(&mut self, eid: FpEntryId) -> Result<()> {
        self.check("fp_entry_destroy")?;
        self.fp.entries.remove(&eid).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    /// `NotSupported` for an entry without a counter.
    fn fp_entry_counter(&self, eid: FpEntryId) -> Result<FpCounter> {
        self.check("fp_entry_counter")?;
        let entry = self.fp.entries.get(&eid).ok_or(Error::ItemNotFound)?;
        if !entry.rule.counter {
            return Err(Error::NotSupported);
        }
        Ok(FpCounter {
            packets: entry.packets,
            bytes: entry.bytes,
        })
    }
}

impl Policer for VirtualBackend {
    fn policer_create(&mut self, cfg: &PolicerCfg) -> Result<PolicerId> {
        self.check("policer_create")?;
        if !policer_cfg_ok(cfg) {
            return Err(Error::InvalidParameter);
        }
        if self.fp.policers.len() >= POLICERS {
            return Err(Error::InsufficientResources);
        }
        let pid = PolicerId(take(&mut self.next.policer));
        self.fp.policers.insert(pid, *cfg);
        Ok(pid)
    }

    fn policer_set(&mut self, pid: PolicerId, cfg: &PolicerCfg) -> Result<()> {
        self.check("policer_set")?;
        if !self.fp.policers.contains_key(&pid) {
            return Err(Error::ItemNotFound);
        }
        if !policer_cfg_ok(cfg) {
            return Err(Error::InvalidParameter);
        }
        self.fp.policers.insert(pid, *cfg);
        Ok(())
    }

    fn policer_destroy(&mut self, pid: PolicerId) -> Result<()> {
        self.check("policer_destroy")?;
        if self.fp.entries.values().any(|e| e.rule.policer == Some(pid)) {
            return Err(Error::ObjectInUse);
        }
        self.fp.policers.remove(&pid).map(|_| ()).ok_or(Error::ItemNotFound)
    }
}

impl Qos for VirtualBackend {
    fn qos_supported(&self) -> bool {
        self.toggles.qos_on
    }

    fn qos_map_create(&mut self, kind: QosMapKind, entries: &[QosMapEntry]) -> Result<QosMapId> {
        self.check("qos_map_create")?;
        let id = QosMapId(self.next_qos_id());
        self.qos.qmaps.insert(id, (kind, entries.to_vec()));
        Ok(id)
    }

    fn qos_map_update(&mut self, id: QosMapId, entries: &[QosMapEntry]) -> Result<()> {
        self.check("qos_map_update")?;
        let map = self.qos.qmaps.get_mut(&id).ok_or(Error::ItemNotFound)?;
        map.1 = entries.to_vec();
        Ok(())
    }

    fn qos_map_destroy(&mut self, id: QosMapId) -> Result<()> {
        self.check("qos_map_destroy")?;
        if self.qos.port_qmap.values().any(|m| *m == id) {
            return Err(Error::ObjectInUse);
        }
        self.qos.qmaps.remove(&id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn port_qos_map_set(&mut self, hw: HwPort, kind: QosMapKind, id: Option<QosMapId>) -> Result<()> {
        self.check("port_qos_map_set")?;
        self.port(hw)?;
        match id {
            Some(id) if !self.qos.qmaps.contains_key(&id) => return Err(Error::ItemNotFound),
            Some(id) => self.qos.port_qmap.insert((hw, kind), id),
            None => self.qos.port_qmap.remove(&(hw, kind)),
        };
        Ok(())
    }

    fn port_default_tc_set(&mut self, hw: HwPort, tc: u8) -> Result<()> {
        self.check("port_default_tc_set")?;
        self.port(hw)?;
        self.qos.default_tc.insert(hw, tc);
        Ok(())
    }

    fn queue_scheduler_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&SchedCfg>) -> Result<()> {
        self.check("queue_scheduler_set")?;
        self.queue_ok(hw, queue)?;
        match cfg {
            Some(cfg) => self.qos.queue_sched.insert((hw, queue), *cfg),
            None => self.qos.queue_sched.remove(&(hw, queue)),
        };
        Ok(())
    }

    fn port_scheduler_set(&mut self, hw: HwPort, cfg: Option<&SchedCfg>) -> Result<()> {
        self.check("port_scheduler_set")?;
        self.port(hw)?;
        match cfg {
            Some(cfg) => self.qos.port_sched.insert(hw, *cfg),
            None => self.qos.port_sched.remove(&hw),
        };
        Ok(())
    }

    fn wred_create(&mut self, cfg: &WredCfg) -> Result<WredId> {
        self.check("wred_create")?;
        let id = WredId(self.next_qos_id());
        self.qos.wreds.insert(id, *cfg);
        Ok(id)
    }

    fn wred_update(&mut self, id: WredId, cfg: &WredCfg) -> Result<()> {
        self.check("wred_update")?;
        let wred = self.qos.wreds.get_mut(&id).ok_or(Error::ItemNotFound)?;
        *wred = *cfg;
        Ok(())
    }

    fn wred_destroy(&mut self, id: WredId) -> Result<()> {
        self.check("wred_destroy")?;
        if self.qos.queue_wred.values().any(|w| *w == id) {
            return Err(Error::ObjectInUse);
        }
        self.qos.wreds.remove(&id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn queue_wred_set(&mut self, hw: HwPort, queue: u8, id: Option<WredId>) -> Result<()> {
        self.check("queue_wred_set")?;
        self.queue_ok(hw, queue)?;
        match id {
            Some(id) if !self.qos.wreds.contains_key(&id) => return Err(Error::ItemNotFound),
            Some(id) => self.qos.queue_wred.insert((hw, queue), id),
            None => self.qos.queue_wred.remove(&(hw, queue)),
        };
        Ok(())
    }

    fn queue_buffer_set(&mut self, hw: HwPort, queue: u8, cfg: Option<&BufferCfg>) -> Result<()> {
        self.check("queue_buffer_set")?;
        self.queue_ok(hw, queue)?;
        match cfg {
            Some(cfg) => self.qos.queue_buf.insert((hw, queue), *cfg),
            None => self.qos.queue_buf.remove(&(hw, queue)),
        };
        Ok(())
    }

    fn pg_buffer_set(&mut self, hw: HwPort, pg: u8, cfg: Option<&BufferCfg>) -> Result<()> {
        self.check("pg_buffer_set")?;
        self.queue_ok(hw, pg)?;
        match cfg {
            Some(cfg) => self.qos.pg_buf.insert((hw, pg), *cfg),
            None => self.qos.pg_buf.remove(&(hw, pg)),
        };
        Ok(())
    }

    /// `PACKETS`, `BYTES`, `DROPPED_PACKETS` as a test set them; any other id `NotSupported`.
    fn queue_stats_get(&self, hw: HwPort, queue: u8, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>> {
        self.check("queue_stats_get")?;
        self.port(hw)?;
        let object = StatsObject::Queue { port: hw, index: queue };
        ids.iter()
            .map(|id| {
                if fake_queue_stat(*id) {
                    Ok(self.counters.get(&(object, *id)).copied().unwrap_or(0))
                } else {
                    Err(Error::NotSupported)
                }
            })
            .collect()
    }

    fn queue_stats_clear(&mut self, hw: HwPort, queue: u8, ids: &[sys::sai_stat_id_t]) -> Result<()> {
        self.check("queue_stats_clear")?;
        let object = StatsObject::Queue { port: hw, index: queue };
        for id in ids {
            if !fake_queue_stat(*id) {
                return Err(Error::NotSupported);
            }
            self.counters.remove(&(object, *id));
        }
        Ok(())
    }

    /// `PACKETS`, `BYTES` as a test set them; any other id `NotSupported`.
    fn pg_stats_get(&self, hw: HwPort, pg: u8, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>> {
        self.check("pg_stats_get")?;
        self.port(hw)?;
        let object = StatsObject::PriorityGroup { port: hw, index: pg };
        ids.iter()
            .map(|id| {
                if fake_pg_stat(*id) {
                    Ok(self.counters.get(&(object, *id)).copied().unwrap_or(0))
                } else {
                    Err(Error::NotSupported)
                }
            })
            .collect()
    }

    fn pg_stats_clear(&mut self, hw: HwPort, pg: u8, ids: &[sys::sai_stat_id_t]) -> Result<()> {
        self.check("pg_stats_clear")?;
        let object = StatsObject::PriorityGroup { port: hw, index: pg };
        for id in ids {
            if !fake_pg_stat(*id) {
                return Err(Error::NotSupported);
            }
            self.counters.remove(&(object, *id));
        }
        Ok(())
    }
}

impl Mirror for VirtualBackend {
    fn mirror_supported(&self) -> bool {
        self.toggles.mirror_on
    }

    fn mirror_create(&mut self, cfg: &MirrorCfg) -> Result<MirrorId> {
        self.check("mirror_create")?;
        self.port(cfg.monitor)?;
        let id = MirrorId(self.next_qos_id());
        self.mirror.mirrors.insert(id, *cfg);
        Ok(id)
    }

    fn mirror_update(&mut self, id: MirrorId, cfg: &MirrorCfg) -> Result<()> {
        self.check("mirror_update")?;
        if !self.mirror.mirrors.contains_key(&id) {
            return Err(Error::ItemNotFound);
        }
        self.port(cfg.monitor)?;
        self.mirror.mirrors.insert(id, *cfg);
        Ok(())
    }

    fn mirror_destroy(&mut self, id: MirrorId) -> Result<()> {
        self.check("mirror_destroy")?;
        if self.mirror.port_mirrors.values().any(|ids| ids.contains(&id)) {
            return Err(Error::ObjectInUse);
        }
        self.mirror.mirrors.remove(&id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn port_mirror_set(&mut self, hw: HwPort, ingress: bool, ids: &[MirrorId]) -> Result<()> {
        self.check("port_mirror_set")?;
        self.port(hw)?;
        if ids.iter().any(|id| !self.mirror.mirrors.contains_key(id)) {
            return Err(Error::ItemNotFound);
        }
        if ids.is_empty() {
            self.mirror.port_mirrors.remove(&(hw, ingress));
        } else {
            self.mirror.port_mirrors.insert((hw, ingress), ids.to_vec());
        }
        Ok(())
    }
}

impl Sample for VirtualBackend {
    fn sample_create(&mut self, rate: u32) -> Result<SampleId> {
        self.check("sample_create")?;
        let id = SampleId(self.next_qos_id());
        self.mirror.samples.insert(id, rate);
        Ok(id)
    }

    fn sample_update(&mut self, id: SampleId, rate: u32) -> Result<()> {
        self.check("sample_update")?;
        let sample = self.mirror.samples.get_mut(&id).ok_or(Error::ItemNotFound)?;
        *sample = rate;
        Ok(())
    }

    fn sample_destroy(&mut self, id: SampleId) -> Result<()> {
        self.check("sample_destroy")?;
        if self.mirror.port_samples.values().any(|s| *s == id) {
            return Err(Error::ObjectInUse);
        }
        self.mirror.samples.remove(&id).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn port_sample_set(&mut self, hw: HwPort, ingress: bool, id: Option<SampleId>) -> Result<()> {
        self.check("port_sample_set")?;
        self.port(hw)?;
        match id {
            Some(id) if !self.mirror.samples.contains_key(&id) => return Err(Error::ItemNotFound),
            Some(id) => self.mirror.port_samples.insert((hw, ingress), id),
            None => self.mirror.port_samples.remove(&(hw, ingress)),
        };
        Ok(())
    }
}

impl Hash for VirtualBackend {
    fn hash_supported(&self) -> bool {
        self.toggles.l2x_on
    }

    fn hash_set(&mut self, lag: bool, fields: &[i32], seed: u32) -> Result<()> {
        self.check("hash_set")?;
        if !self.toggles.l2x_on {
            return Err(Error::NotSupported);
        }
        self.l2x.hash.insert(lag, (fields.to_vec(), seed));
        Ok(())
    }
}

impl Storm for VirtualBackend {
    fn port_storm_set(&mut self, hw: HwPort, kind: StormKind, policer: Option<PolicerId>) -> Result<()> {
        self.check("port_storm_set")?;
        if !self.toggles.l2x_on {
            return Err(Error::NotSupported);
        }
        self.port(hw)?;
        match policer {
            Some(p) if !self.fp.policers.contains_key(&p) => return Err(Error::ItemNotFound),
            Some(p) => self.l2x.storm.insert((hw, kind), p),
            None => self.l2x.storm.remove(&(hw, kind)),
        };
        Ok(())
    }

    fn vlan_flood_set(&mut self, vid: Vid, kind: FloodKind, on: bool) -> Result<()> {
        self.check("vlan_flood_set")?;
        if !self.toggles.l2x_on {
            return Err(Error::NotSupported);
        }
        if !self.vlans.contains_key(&vid) {
            return Err(Error::ItemNotFound);
        }
        self.l2x.flood.insert((vid, kind), on);
        Ok(())
    }
}

// The C++ fake leaves these at the ob::Backend defaults (NotSupported / false): pool_stats_get, l3_intf_stat_supported, l3_intf_stats, l3_intf_admin_set, l3_egress_glean,
// port_stat_by_id, port_stats_by_id, fp_supported, fp_group_create_ex, fp_entry_update,
// fp_entry_enable, fp_group_free; so does this chip. Their area agents add a model when a test
// needs one (the Teralynx mock is their only oracle today).

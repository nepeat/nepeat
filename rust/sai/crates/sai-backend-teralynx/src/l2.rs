//! L2 (`tl_l2.{h,cpp}`). Ported in G1: what init does (`pre_enable`: multicast LAG resolution
//! before node enable; `base_init`: the STP instance with every port forwarding, VLAN 1 as an
//! empty L2VNI with its own flood group, the LAG area reset, the hash profile) and the L2 event
//! translation the init registers. VLANs, FDB, flush, aging and flood control are G2 (`g2.rs`).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ifcs::{Attrs, Handle, L2Event, sys};
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, MacAddr, TrunkId};
use sai_core::backend::{HwPort, Vid};
use sai_core::error::Result;
use sai_core::log;
use sai_sys as ssys;

use crate::core::{CPU_DEVPORT, Core, NODE};
use crate::lag::{self, LagArea};

/// A VLAN: its L2VNI, flood group, the CPU's `mc_dest` when `SAI_TERALYNX_CPU_FLOOD=on`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vlan {
    /// `IFCS_HANDLE_L2VNI(vid)`.
    pub l2vni: Handle,
    /// Its flood group (MDG).
    pub mdg: Handle,
    /// The CPU's `mc_dest` in it (CPU flood only).
    pub cpu_mcd: Handle,
}

/// The L2 area's state.
#[derive(Debug, Default)]
pub struct L2Area {
    /// The one STP instance.
    pub stp: Handle,
    /// `SAI_TERALYNX_CPU_FLOOD=on`.
    pub cpu_flood: bool,
    /// VLANs by VID.
    pub vlans: BTreeMap<u16, Vlan>,
    /// devport -> its untagged VID (`untagged_vid_`).
    pub untagged_vid: BTreeMap<u32, u16>,
}

impl L2Area {
    /// `L2Area::pre_enable`: node `MULTICAST_LAG_RESOLUTION_ENABLE` (libifcs refuses it once the
    /// node is enabled); `SAI_TERALYNX_LAG_FLOOD=off` skips it. A refusal is a warning.
    pub fn pre_enable(core: &Core) {
        if !core.settings.lag_flood {
            return;
        }
        let mut e = Attrs::new().data(sys::IFCS_NODE_ATTR_MULTICAST_LAG_RESOLUTION_ENABLE, true);
        let rc = core.lib.ifcs_node_attr_set(NODE, &mut e);
        if !rc.is_success() {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!(
                    "node MULTICAST_LAG_RESOLUTION_ENABLE: {} (a LAG cannot join a VLAN)",
                    rc.name()
                ),
            );
        }
    }

    /// `L2Area::base_init`: the STP instance of the vendor's `knet.py` with every port FORWARDING,
    /// VLAN 1, then the LAG area's profile hash. `devports`: the CPU then the port map.
    pub fn base_init(&mut self, core: &Core, lag: &mut LagArea, devports: &[u32]) -> Result<()> {
        self.cpu_flood = core.settings.cpu_flood;
        lag.reset();
        self.stp = Handle::NULL;
        let mut none = Attrs::none();
        core.call(core.lib.ifcs_stp_create(NODE, &mut self.stp, &mut none), "stp_create")?;
        let mut sps: Vec<Handle> = devports
            .iter()
            .map(|dp| core.sysport.get(dp).copied().unwrap_or_default())
            .collect();
        let mut fwd = Attrs::new().u32(sys::IFCS_STP_PORT_ATTR_STP_STATE, sys::IFCS_STP_PORT_STATE_FORWARDING);
        core.call(
            core.lib.ifcs_stp_port_add(NODE, self.stp, &mut sps, &mut fwd),
            "stp_port_add",
        )?;
        self.vlans.clear();
        self.untagged_vid.clear();
        self.vlan_make(core, 1)?;
        lag.apply_profile(core)
    }

    /// `L2Area::vlan_make`: an L2VNI with the STP instance, its own flood group, learning on and
    /// isai's max learn count; an L2VNI the SDK made itself is taken over.
    pub fn vlan_make(&mut self, core: &Core, vid: u16) -> Result<()> {
        let mut v = Vlan::default();
        core.call(
            core.lib.ifcs_mdg_create(NODE, &mut v.mdg, &mut Attrs::none()),
            "mdg_create",
        )?;
        v.l2vni = Handle::l2vni(u32::from(vid));
        let attrs = || {
            Attrs::new()
                .handle(sys::IFCS_L2VNI_ATTR_STP_INSTANCE, self.stp)
                .handle(sys::IFCS_L2VNI_ATTR_FLOOD_GROUP, v.mdg)
                .data(sys::IFCS_L2VNI_ATTR_LEARN_ENABLE, true)
                .u32(sys::IFCS_L2VNI_ATTR_L2_ENTRY_MAX_LEARN_COUNT, 0xffff_ffff)
        };
        let mut h = v.l2vni;
        let mut rc = core.lib.ifcs_l2vni_create(NODE, &mut h, &mut attrs());
        if rc.reason() == sys::IFCS_EXIST {
            // An L2VNI the SDK made itself (VLAN 1?): take it over.
            log::notice(
                ssys::SAI_API_VLAN,
                format_args!("L2VNI {vid} exists already; setting its STP instance and flood group"),
            );
            rc = core.lib.ifcs_l2vni_attr_set(NODE, v.l2vni.0, &mut attrs());
        }
        if !rc.is_success() {
            let _ = core.lib.ifcs_mdg_delete(NODE, v.mdg);
            return Err(core.fail(rc.reason(), "l2vni_create"));
        }
        if self.cpu_flood {
            let cpu_sp = core.sysport.get(&CPU_DEVPORT).copied().unwrap_or_default();
            let rc = core
                .lib
                .ifcs_l2vni_member_add(NODE, v.l2vni, &mut [cpu_sp], &mut Attrs::none());
            if !rc.is_success() && rc.reason() != sys::IFCS_EXIST {
                return Err(core.fail(rc.reason(), "l2vni_member_add(cpu)"));
            }
            let mut d = Attrs::new().handle(sys::IFCS_MC_DEST_ATTR_LOCAL_DESTINATION, cpu_sp);
            core.call(
                core.lib.ifcs_mc_dest_create(NODE, &mut v.cpu_mcd, &mut d),
                "mc_dest_create(cpu)",
            )?;
            core.call(
                core.lib
                    .ifcs_mdg_mc_dest_add(NODE, v.mdg, &mut [v.cpu_mcd], &mut Attrs::none()),
                "mdg_mc_dest_add(cpu)",
            )?;
        }
        self.vlans.insert(vid, v);
        Ok(())
    }
}

/// The C++ `hw_port = -1` of an event whose destination is unknown: a port no switch has, so the
/// SAI layer finds no bridge port for it (an aged entry then names the one it was learned on, as
/// the C++ `deliver_fdb` does).
pub const NO_DEST: Dest = Dest::Port(HwPort(u16::MAX));

/// `L2Area::on_l2` (libifcs' thread): only `MAC_L2VNI` keys of an L2VNI, flushes are ours; the
/// destination is the `ENTRY_DEST` attribute's (a LAG member's sysport maps to its LAG); a learned
/// address without one is dropped. `None`: nothing to report.
pub fn fdb_event(e: &L2Event, dest_of: impl Fn(Handle) -> Option<Dest>) -> Option<FdbEvent> {
    if e.key_type != sys::IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI.cast_signed() {
        return None;
    }
    if e.event_type == sys::IFCS_L2_EVENT_TYPE_FLUSHED.cast_signed() {
        return None; // our own flushes; the frontend knows
    }
    let learned = e.event_type != sys::IFCS_L2_EVENT_TYPE_AGED.cast_signed();
    if e.l2vni.module() != sys::IFCS_MOD_L2VNI {
        return None;
    }
    // C++: (uint16_t)IFCS_HANDLE_VALUE(vni); a value outside 1..=4094 is no VLAN the SAI has
    let vid = Vid::new(u16::try_from(e.l2vni.value() & 0xffff).unwrap_or(0)).ok()?;
    let mut dest = None;
    for &(id, value) in &e.attrs {
        if id == sys::IFCS_L2_ENTRY_ATTR_ENTRY_DEST {
            dest = dest_of(Handle(value));
        }
    }
    if learned && dest.is_none() {
        return None;
    }
    Some(FdbEvent {
        change: if learned { FdbChange::Learned } else { FdbChange::Aged },
        mac: MacAddr(e.mac),
        vid,
        hw_port: dest.unwrap_or(NO_DEST),
    })
}

/// A backend dest number (`lag_dest(tid)` or a devport) as the SAI layer's [`Dest`].
pub fn dest_from_raw(d: i32) -> Option<Dest> {
    let d = u32::try_from(d).ok()?;
    if d & Dest::LAG_BIT != 0 {
        return Some(Dest::Lag(TrunkId(d & !Dest::LAG_BIT)));
    }
    u16::try_from(d).ok().map(|p| Dest::Port(HwPort(p)))
}

/// `LagArea::dest_of_handle` for the callback: the LAG map first, then the sysports.
pub fn dest_of_handle(map: &lag::DestMap, dev_of_sysport: &BTreeMap<Handle, u32>, h: Handle) -> Option<Dest> {
    if let Some(d) = lag::lookup(map, h) {
        return dest_from_raw(d);
    }
    dev_of_sysport
        .get(&h)
        .and_then(|&dp| u16::try_from(dp).ok())
        .map(|p| Dest::Port(HwPort(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: u32, l2vni: Handle, dest: Option<u32>) -> L2Event {
        L2Event {
            event_type: event_type.cast_signed(),
            key_type: 0,
            mac: [0, 1, 2, 3, 4, 5],
            l2vni,
            attrs: dest
                .map(|d| vec![(sys::IFCS_L2_ENTRY_ATTR_ENTRY_DEST, d)])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn l2_events_as_on_l2() {
        let sp = Handle::sysport(3);
        let dev: BTreeMap<Handle, u32> = [(sp, 3)].into();
        let map = lag::DestMap::default();
        let dest_of = |h| dest_of_handle(&map, &dev, h);
        let learned = fdb_event(&event(0, Handle::l2vni(10), Some(sp.0)), dest_of).unwrap();
        assert_eq!(learned.change, FdbChange::Learned);
        assert_eq!(learned.vid.get(), 10);
        assert_eq!(learned.hw_port, Dest::Port(HwPort(3)));
        // moved is learned too
        assert_eq!(
            fdb_event(&event(2, Handle::l2vni(10), Some(sp.0)), dest_of)
                .unwrap()
                .change,
            FdbChange::Learned
        );
        // learned without a known destination: dropped; aged without one: reported, no port
        assert!(fdb_event(&event(0, Handle::l2vni(10), None), dest_of).is_none());
        let aged = fdb_event(&event(1, Handle::l2vni(10), Some(0x999)), dest_of).unwrap();
        assert_eq!((aged.change, aged.hw_port), (FdbChange::Aged, NO_DEST));
        // flushed, another key type, not an L2VNI
        assert!(fdb_event(&event(3, Handle::l2vni(10), Some(sp.0)), dest_of).is_none());
        assert!(fdb_event(&event(0, Handle::l3vni(10), Some(sp.0)), dest_of).is_none());
        let mut other = event(0, Handle::l2vni(10), Some(sp.0));
        other.key_type = 1;
        assert!(fdb_event(&other, dest_of).is_none());
        assert_eq!(dest_from_raw(0x4000_0002), Some(Dest::Lag(TrunkId(2))));
        assert_eq!(dest_from_raw(-1), None);
    }
}

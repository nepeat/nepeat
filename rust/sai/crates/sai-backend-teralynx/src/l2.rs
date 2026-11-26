//! L2 (`tl_l2.{h,cpp}`): VLANs (L2VNI + STP + a flood group of per-member `mc_dest`s), FDB entries,
//! flush, aging, VLAN flood control, L2 events; LAG members join VLANs as their LAG.
//!
//! At init (after the sysports are known): one STP instance with every port FORWARDING, VLAN 1
//! (empty), the LAG area reset, the hash profile. No per-port `mc_dest`s: as isai does, a VLAN
//! member gets its own `mc_dest` when it joins, so a port in no VLAN is in no L2VNI and no
//! `mc_dest`, which is what `ifcs_lag_member_add` requires. The CPU is in no VLAN: host traps
//! (`CoPP`) bring the protocols it needs; `SAI_TERALYNX_CPU_FLOOD=on` puts it in every VLAN's L2VNI
//! and flood group again (never reported as a member). Untagged egress is per sysport
//! (`EGRESS_UNTAGGED_VLAN_ID[_ENABLE]`): one untagged VLAN per port.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ifcs::{Attrs, Handle, L2Event, L2Key, sys};
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, FloodKind, MacAddr, TrunkId};
use sai_core::backend::{HwPort, Vid};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{CPU_DEVPORT, Core, NODE};
use crate::lag::{self, LagArea};

/// A VLAN member's IFCS destination (its sysport, or its LAG's handle while the port is in a LAG,
/// shared by all members and counted): its L2VNI membership and `mc_dest`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DestRef {
    /// Members behind it.
    pub refs: u32,
    /// Its `mc_dest` in the flood group.
    pub mcd: Handle,
}

/// A VLAN member (a devport).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Member {
    /// Untagged in this VLAN.
    pub untagged: bool,
    /// The destination it joined as.
    pub dest: Handle,
}

/// A VLAN: its L2VNI, flood group, the CPU's `mc_dest` when `SAI_TERALYNX_CPU_FLOOD=on`, the flood
/// control state, the members and their destinations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vlan {
    /// `IFCS_HANDLE_L2VNI(vid)`.
    pub l2vni: Handle,
    /// Its flood group (MDG) of the members' `mc_dest`s.
    pub mdg: Handle,
    /// The CPU's `mc_dest` in it (CPU flood only).
    pub cpu_mcd: Handle,
    /// Flood control off: an empty flood group (made on first use).
    pub empty_mdg: Handle,
    /// Flooding per [`FloodKind`] (unknown unicast, unknown multicast, broadcast).
    pub flood: [bool; 3],
    /// devport -> member.
    pub members: BTreeMap<u32, Member>,
    /// The L2VNI members / `mc_dest`s per destination.
    pub dests: BTreeMap<Handle, DestRef>,
}

impl Default for Vlan {
    fn default() -> Self {
        Self {
            l2vni: Handle::NULL,
            mdg: Handle::NULL,
            cpu_mcd: Handle::NULL,
            empty_mdg: Handle::NULL,
            flood: [true; 3],
            members: BTreeMap::new(),
            dests: BTreeMap::new(),
        }
    }
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

/// What a flood control change does (`vlan_flood_set`): IFCS has one flood group per L2VNI for
/// unknown unicast, unknown multicast and broadcast, so all three on = the members' flood group,
/// all three off = an empty one; a mixed setting is recorded and programmed only when the three
/// agree again (orchagent sets them one by one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloodStep {
    /// Mixed: record `want`, program nothing.
    Record([bool; 3]),
    /// All on or all off: program, then record `want`.
    Program {
        /// All off: the empty flood group (else the members' one).
        off: bool,
        /// The new state.
        want: [bool; 3],
    },
}

/// [`FloodStep`] of setting `kind` to `on` from `now`.
pub const fn flood_step(now: [bool; 3], kind: FloodKind, on: bool) -> FloodStep {
    let [mut uu, mut um, mut bc] = now;
    match kind {
        FloodKind::UnknownUnicast => uu = on,
        FloodKind::UnknownMulticast => um = on,
        FloodKind::Broadcast => bc = on,
    }
    let want = [uu, um, bc];
    let all = uu && um && bc;
    let none = !uu && !um && !bc;
    if !all && !none {
        FloodStep::Record(want)
    } else {
        FloodStep::Program { off: none, want }
    }
}

/// `L2Area::untagged_off`: the port's untagged egress off, forgotten.
fn untagged_off(core: &Core, untagged_vid: &mut BTreeMap<u32, u16>, hw: u32) {
    let sp = core.sysport.get(&hw).copied().unwrap_or_default();
    let mut a = Attrs::new().data(sys::IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, false);
    let _ = core.call(
        core.lib.ifcs_sysport_attr_set(NODE, sp.0, &mut a),
        "sysport egress untagged off",
    );
    untagged_vid.remove(&hw);
}

/// `L2Area::dest_join`: the first member behind a destination adds it to the L2VNI and gives it
/// an `mc_dest` in the flood group (isai: `l2vni_member_add`, `mc_dest_create` `LOCAL_DESTINATION`,
/// `mdg_mc_dest_add`; a LAG's `mc_dest` needs multicast LAG resolution on the node, `pre_enable`).
/// STP state stays per sysport: the LAG's members are STP ports from `base_init` (`stp_port_add`
/// refuses a LAG handle: "port check failed", chip 2026-11-16). A failure undoes what was done.
fn dest_join(core: &Core, v: &mut Vlan, dest: Handle) -> Result<()> {
    let (l2vni, mdg) = (v.l2vni, v.mdg);
    let d = v.dests.entry(dest).or_default();
    d.refs += 1;
    if d.refs > 1 {
        return Ok(());
    }
    let mut st = core.call(
        core.lib
            .ifcs_l2vni_member_add(NODE, l2vni, &mut [dest], &mut Attrs::none()),
        "l2vni_member_add",
    );
    if st.is_ok() {
        let mut a = Attrs::new().handle(sys::IFCS_MC_DEST_ATTR_LOCAL_DESTINATION, dest);
        d.mcd = Handle::NULL;
        st = core.call(core.lib.ifcs_mc_dest_create(NODE, &mut d.mcd, &mut a), "mc_dest_create");
        if st.is_ok() {
            st = core.call(
                core.lib
                    .ifcs_mdg_mc_dest_add(NODE, mdg, &mut [d.mcd], &mut Attrs::none()),
                "mdg_mc_dest_add",
            );
            if st.is_err() {
                let _ = core.lib.ifcs_mc_dest_delete(NODE, d.mcd);
            }
        }
        if st.is_err() {
            let _ = core.lib.ifcs_l2vni_member_remove(NODE, l2vni, &mut [dest]);
        }
    }
    if st.is_err() {
        v.dests.remove(&dest);
    }
    st
}

/// `L2Area::dest_leave`: the last member behind a destination takes it out again.
fn dest_leave(core: &Core, v: &mut Vlan, dest: Handle) {
    let Some(d) = v.dests.get_mut(&dest) else { return };
    d.refs = d.refs.saturating_sub(1);
    if d.refs > 0 {
        return;
    }
    let mcd = d.mcd;
    let _ = core.call(
        core.lib.ifcs_mdg_mc_dest_remove(NODE, v.mdg, &mut [mcd]),
        "mdg_mc_dest_remove",
    );
    let _ = core.call(core.lib.ifcs_mc_dest_delete(NODE, mcd), "mc_dest_delete");
    let _ = core.call(
        core.lib.ifcs_l2vni_member_remove(NODE, v.l2vni, &mut [dest]),
        "l2vni_member_remove",
    );
    v.dests.remove(&dest);
}

/// `L2Area::member_dest`: a port's sysport, or its LAG's handle while it is in one.
fn member_dest(core: &Core, lag: &LagArea, hw: u32) -> Handle {
    if let Some(h) = lag
        .lag_of_port(hw)
        .and_then(|tid| lag.dest_handle(core, Dest::Lag(TrunkId(tid))))
    {
        return h;
    }
    core.sysport.get(&hw).copied().unwrap_or_default()
}

/// `L2Area::l2_key`: a `MAC_L2VNI` key through the key accessors in the vendor's order
/// (`scripts/test_l2.py`): init, key type, select the `mac_l2vni` member, then its MAC and L2VNI.
pub fn l2_key(core: &Core, mac: [u8; 6], vid: u16) -> L2Key {
    let lib = &core.lib;
    let mut k = L2Key::zeroed();
    let _ = lib.ifcs_l2_entry_key_t_init(&mut k);
    let _ = lib.ifcs_l2_entry_key_t_key_type_set(&mut k, sys::IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI.cast_signed());
    let member = k.mac_l2vni();
    let _ = lib.ifcs_l2_entry_key_t_mac_l2vni_set(&mut k, member);
    let _ = lib.ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(&mut k, mac);
    let _ = lib.ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(&mut k, Handle::l2vni(u32::from(vid)));
    k
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
        let (stp, mdg) = (self.stp, v.mdg);
        let attrs = || {
            Attrs::new()
                .handle(sys::IFCS_L2VNI_ATTR_STP_INSTANCE, stp)
                .handle(sys::IFCS_L2VNI_ATTR_FLOOD_GROUP, mdg)
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

    /// `L2Area::vlan_create`.
    pub fn vlan_create(&mut self, core: &Core, vid: u16) -> Result<()> {
        if !(1..=4094).contains(&vid) {
            return Err(Error::InvalidParameter);
        }
        if self.vlans.contains_key(&vid) {
            return Err(Error::ItemAlreadyExists);
        }
        self.vlan_make(core, vid)
    }

    /// `L2Area::vlan_destroy`: the members out, the CPU's `mc_dest` (CPU flood), the learned entries
    /// (they hold the L2VNI; isai deletes them first too), the L2VNI (a refusal keeps the VLAN),
    /// its flood groups. VLAN 1 stays (`OBJECT_IN_USE`).
    pub fn vlan_destroy(&mut self, core: &Core, vid: u16) -> Result<()> {
        let Some(v) = self.vlans.get(&vid) else {
            return Err(Error::ItemNotFound);
        };
        if vid == 1 {
            return Err(Error::ObjectInUse);
        }
        let hws: Vec<u32> = v.members.keys().copied().collect();
        for hw in hws {
            let _ = self.vlan_port_remove(core, vid, hw);
        }
        let Some(v) = self.vlans.get_mut(&vid) else {
            return Err(Error::ItemNotFound);
        };
        if !v.cpu_mcd.is_null() {
            let cpu_sp = core.sysport.get(&CPU_DEVPORT).copied().unwrap_or_default();
            let mcd = v.cpu_mcd;
            let _ = core.call(
                core.lib.ifcs_mdg_mc_dest_remove(NODE, v.mdg, &mut [mcd]),
                "mdg_mc_dest_remove(cpu)",
            );
            let _ = core.call(core.lib.ifcs_mc_dest_delete(NODE, mcd), "mc_dest_delete(cpu)");
            let _ = core.call(
                core.lib.ifcs_l2vni_member_remove(NODE, v.l2vni, &mut [cpu_sp]),
                "l2vni_member_remove(cpu)",
            );
            v.cpu_mcd = Handle::NULL;
        }
        let (l2vni, mdg, empty) = (v.l2vni, v.mdg, v.empty_mdg);
        let _ = Self::flush_handle(core, None, Some(vid), false, true);
        core.call(core.lib.ifcs_l2vni_delete(NODE, l2vni), "l2vni_delete")?;
        let _ = core.call(core.lib.ifcs_mdg_delete(NODE, mdg), "mdg_delete");
        if !empty.is_null() {
            let _ = core.call(core.lib.ifcs_mdg_delete(NODE, empty), "mdg_delete (flood off)");
        }
        self.vlans.remove(&vid);
        Ok(())
    }

    /// `L2Area::vlan_port_add`: the frontend adds LAG members one by one (backend.h): a member of a
    /// LAG joins as the LAG handle, the L2VNI membership and `mc_dest` are the LAG's, shared and
    /// counted; untagged egress is set on the member sysport (isai). A second untagged VLAN of a
    /// port is refused (`NOT_SUPPORTED`: IFCS has one per port).
    pub fn vlan_port_add(&mut self, core: &Core, lag: &LagArea, vid: u16, hw: u32, untagged: bool) -> Result<()> {
        if !self.vlans.contains_key(&vid) {
            return Err(Error::ItemNotFound);
        }
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if untagged {
            if let Some(&u) = self.untagged_vid.get(&hw) {
                if u != vid {
                    log::error(
                        ssys::SAI_API_VLAN,
                        format_args!(
                            "devport {hw} is already untagged in VLAN {u}; IFCS has one untagged VLAN per port"
                        ),
                    );
                    return Err(Error::NotSupported);
                }
            }
        }
        let dest = member_dest(core, lag, hw);
        let Self {
            vlans, untagged_vid, ..
        } = self;
        let Some(v) = vlans.get_mut(&vid) else {
            return Err(Error::ItemNotFound);
        };
        let mut joined = v.members.get(&hw).copied();
        if let Some(m) = joined {
            if m.dest != dest {
                // joined / left a LAG while a member
                dest_leave(core, v, m.dest);
                v.members.remove(&hw);
                joined = None;
            }
        }
        if joined.is_none() {
            dest_join(core, v, dest)?;
        }
        if untagged {
            let sp = core.sysport.get(&hw).copied().unwrap_or_default();
            let mut a = Attrs::new()
                .u16(sys::IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID, vid)
                .data(sys::IFCS_SYSPORT_ATTR_EGRESS_UNTAGGED_VLAN_ID_ENABLE, true);
            if let Err(e) = core.call(
                core.lib.ifcs_sysport_attr_set(NODE, sp.0, &mut a),
                "sysport egress untagged",
            ) {
                if !v.members.contains_key(&hw) {
                    dest_leave(core, v, dest);
                }
                return Err(e);
            }
            untagged_vid.insert(hw, vid);
        } else if untagged_vid.get(&hw) == Some(&vid) {
            untagged_off(core, untagged_vid, hw);
        }
        v.members.insert(hw, Member { untagged, dest });
        Ok(())
    }

    /// `L2Area::vlan_port_remove`: not a member is success.
    pub fn vlan_port_remove(&mut self, core: &Core, vid: u16, hw: u32) -> Result<()> {
        let Self {
            vlans, untagged_vid, ..
        } = self;
        let Some(v) = vlans.get_mut(&vid) else {
            return Err(Error::ItemNotFound);
        };
        let Some(m) = v.members.get(&hw).copied() else {
            return Ok(());
        };
        if untagged_vid.get(&hw) == Some(&vid) {
            untagged_off(core, untagged_vid, hw);
        }
        dest_leave(core, v, m.dest);
        v.members.remove(&hw);
        Ok(())
    }

    /// `L2Area::vlan_members`: (devport, untagged), by devport.
    pub fn vlan_members(&self, vid: u16) -> Result<Vec<(HwPort, bool)>> {
        let v = self.vlans.get(&vid).ok_or(Error::ItemNotFound)?;
        Ok(v.members
            .iter()
            .filter_map(|(&hw, m)| u16::try_from(hw).ok().map(|p| (HwPort(p), m.untagged)))
            .collect())
    }

    /// `L2Area::vlan_l2vni`: the L2VNI of a VLAN (the L3 area's SVIs), `None`: no such VLAN.
    pub fn vlan_l2vni(&self, vid: u16) -> Option<Handle> {
        self.vlans.get(&vid).map(|v| v.l2vni)
    }

    /// `L2Area::l2_add`: `ifcs_l2_entry_create` with a `MAC_L2VNI` key, attributes `ENTRY_DEST` +
    /// `ENTRY_TYPE` (`SDK/scripts/test_l2.py`); the dest is the port's sysport or the LAG handle
    /// (isai). discard: `ENTRY_FWD_POLICY` = DROP (isai: `FWD_POLICY` 1).
    #[allow(clippy::too_many_arguments, reason = "the C++ operation's arguments")]
    pub fn l2_add(
        &self,
        core: &Core,
        lag: &LagArea,
        mac: [u8; 6],
        vid: u16,
        dest: Dest,
        is_static: bool,
        discard: bool,
    ) -> Result<()> {
        if !self.vlans.contains_key(&vid) {
            return Err(Error::InvalidParameter);
        }
        let dh = lag.dest_handle(core, dest).ok_or(Error::InvalidPortNumber)?;
        let mut k = l2_key(core, mac, vid);
        let ty = if is_static {
            sys::IFCS_L2_ENTRY_TYPE_STATIC
        } else {
            sys::IFCS_L2_ENTRY_TYPE_DYNAMIC
        };
        let mut a = Attrs::new()
            .handle(sys::IFCS_L2_ENTRY_ATTR_ENTRY_DEST, dh)
            .u32(sys::IFCS_L2_ENTRY_ATTR_ENTRY_TYPE, ty);
        if discard {
            a = a.fwd(
                sys::IFCS_L2_ENTRY_ATTR_ENTRY_FWD_POLICY,
                sys::IFCS_FWD_ACTION_DROP.cast_signed(),
            );
        }
        core.call(core.lib.ifcs_l2_entry_create(NODE, &mut k, &mut a), "l2_entry_create")
    }

    /// `L2Area::l2_delete`.
    pub fn l2_delete(core: &Core, mac: [u8; 6], vid: u16) -> Result<()> {
        let mut k = l2_key(core, mac, vid);
        core.call(core.lib.ifcs_l2_entry_delete(NODE, &mut k), "l2_entry_delete")
    }

    /// `L2Area::l2_flush`: `dest` (a port or LAG; unknown: `INVALID_PORT_NUMBER`) and / or `vid`,
    /// static and / or dynamic entries ([`L2Area::flush_handle`]).
    pub fn l2_flush(
        core: &Core,
        lag: &LagArea,
        dest: Option<Dest>,
        vid: Option<u16>,
        statics: bool,
        dynamics: bool,
    ) -> Result<()> {
        let want = match dest {
            Some(d) => Some(lag.dest_handle(core, d).ok_or(Error::InvalidPortNumber)?),
            None => None,
        };
        Self::flush_handle(core, want, vid, statics, dynamics)
    }

    /// The walk of `l2_flush` (`ifcs_l2_entry_get_all`, as `SDK/ifcs_cmds/l2_entry.py`): each
    /// `MAC_L2VNI` entry's type and destination read, the matching ones deleted one by one (no bulk
    /// delete-by-filter: only what was checked is removed); an entry whose attributes cannot be
    /// read is left. `want`: the IFCS destination (sysport or LAG handle).
    pub fn flush_handle(
        core: &Core,
        want: Option<Handle>,
        vid: Option<u16>,
        statics: bool,
        dynamics: bool,
    ) -> Result<()> {
        let (rc, keys, _) = core.lib.ifcs_l2_entry_get_all(NODE);
        core.call(rc, "l2_entry_get_all")?;
        for mut k in keys {
            if k.key_type() != sys::IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI.cast_signed() {
                continue;
            }
            if vid.is_some_and(|v| k.l2vni() != Handle::l2vni(u32::from(v))) {
                continue;
            }
            let mut a = Attrs::new()
                .of(sys::IFCS_L2_ENTRY_ATTR_ENTRY_TYPE)
                .of(sys::IFCS_L2_ENTRY_ATTR_ENTRY_DEST);
            let (rc, _) = core.lib.ifcs_l2_entry_attr_get(NODE, &mut k, &mut a);
            if !rc.is_success() {
                continue;
            }
            let is_static = a.u32_at(0) == sys::IFCS_L2_ENTRY_TYPE_STATIC;
            if if is_static { !statics } else { !dynamics } {
                continue;
            }
            if want.is_some_and(|w| a.handle_at(1) != w) {
                continue;
            }
            let _ = core.call(core.lib.ifcs_l2_entry_delete(NODE, &mut k), "l2_entry_delete (flush)");
        }
        Ok(())
    }

    /// `L2Area::vlan_flood_set` ([`flood_step`]): all three kinds on = the members' flood group, all
    /// off = an empty one (made on first use), mixed: recorded and logged once.
    pub fn vlan_flood_set(&mut self, core: &Core, vid: u16, kind: FloodKind, on: bool) -> Result<()> {
        let v = self.vlans.get_mut(&vid).ok_or(Error::ItemNotFound)?;
        let (off, want) = match flood_step(v.flood, kind, on) {
            FloodStep::Record(want) => {
                // Recorded so the next call can complete the change; nothing programmed while mixed.
                v.flood = want;
                core.log_once(
                    "l2:flood-mixed",
                    format_args!(
                        "VLAN flood control per kind: IFCS has one flood group per VLAN; applied only when all three kinds agree"
                    ),
                );
                return Ok(());
            }
            FloodStep::Program { off, want } => (off, want),
        };
        let mut fg = v.mdg;
        if off {
            if v.empty_mdg.is_null() {
                if let Err(e) = core.call(
                    core.lib.ifcs_mdg_create(NODE, &mut v.empty_mdg, &mut Attrs::none()),
                    "mdg_create (flood off)",
                ) {
                    v.empty_mdg = Handle::NULL;
                    return Err(e);
                }
            }
            fg = v.empty_mdg;
        }
        let mut a = Attrs::new().handle(sys::IFCS_L2VNI_ATTR_FLOOD_GROUP, fg);
        core.call(
            core.lib.ifcs_l2vni_attr_set(NODE, v.l2vni.0, &mut a),
            "l2vni flood group",
        )?;
        v.flood = want;
        log::notice(
            ssys::SAI_API_VLAN,
            format_args!(
                "VLAN {vid} flooding {}",
                if off { "off (empty flood group)" } else { "on" }
            ),
        );
        Ok(())
    }

    /// `L2Area::l2_age_set`: node `L2_ENTRY_AGING_INTERVAL` (u32, read back as u32 by the vendor's
    /// `utils/ifcs_utils.py`; units taken as seconds).
    pub fn l2_age_set(core: &Core, seconds: u32) -> Result<()> {
        let mut a = Attrs::new().u32(sys::IFCS_NODE_ATTR_L2_ENTRY_AGING_INTERVAL, seconds);
        core.call(
            core.lib.ifcs_node_attr_set(NODE, &mut a),
            "node l2_entry_aging_interval",
        )
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

    #[test]
    fn a_lag_member_sysport_reports_its_lag() {
        let sp = Handle::sysport(3);
        let lagh = Handle(0x2820_0105);
        let dev: BTreeMap<Handle, u32> = [(sp, 3)].into();
        let map = lag::DestMap::default();
        map.lock()
            .unwrap()
            .extend([(lagh, lag::lag_dest(1)), (sp, lag::lag_dest(1))]);
        let e = fdb_event(&event(0, Handle::l2vni(300), Some(sp.0)), |h| {
            dest_of_handle(&map, &dev, h)
        })
        .unwrap();
        assert_eq!(e.hw_port, Dest::Lag(TrunkId(1)));
        let e = fdb_event(&event(0, Handle::l2vni(200), Some(lagh.0)), |h| {
            dest_of_handle(&map, &dev, h)
        })
        .unwrap();
        assert_eq!((e.hw_port, e.vid.get()), (Dest::Lag(TrunkId(1)), 200));
    }

    #[test]
    fn flood_control_programs_only_when_the_three_agree() {
        use FloodKind::{Broadcast, UnknownMulticast, UnknownUnicast};
        let on = [true; 3];
        assert_eq!(
            flood_step(on, UnknownUnicast, false),
            FloodStep::Record([false, true, true])
        );
        assert_eq!(
            flood_step([false, true, true], UnknownMulticast, false),
            FloodStep::Record([false, false, true])
        );
        assert_eq!(
            flood_step([false, false, true], Broadcast, false),
            FloodStep::Program {
                off: true,
                want: [false; 3]
            }
        );
        assert_eq!(
            flood_step([false; 3], UnknownUnicast, true),
            FloodStep::Record([true, false, false])
        );
        assert_eq!(
            flood_step([true, true, false], Broadcast, true),
            FloodStep::Program { off: false, want: on }
        );
        // re-setting the state it has programs again (the C++ does)
        assert_eq!(
            flood_step(on, Broadcast, true),
            FloodStep::Program { off: false, want: on }
        );
        assert_eq!(Vlan::default().flood, on);
    }
}

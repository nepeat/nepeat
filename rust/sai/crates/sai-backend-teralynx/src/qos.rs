//! `QoS` and buffers (`tl_qos.{h,cpp}`), line by line: what each SAI `QoS` object becomes in IFCS, the way
//! isai 6.2.0 programs this chip (oracle, `devices/cs8210-32x/sai-oracle.md` "`QoS`"):
//!
//! - DSCP / DOT1P -> TC: one IFCS `qos_map` per SAI map (complete tables, `tc_dp` values, dp 0), a
//!   `qos_map_group` per (DSCP map, DOT1P map) pair shared by the ports with that pair, the sysport's
//!   `QOS_DSCP_TO_TC_DP_MAP` / `QOS_DOT1P_CFI_TO_TC_DP_MAP` = the group;
//! - TC -> queue: the chip's fixed identity (isai programs nothing; a non-identity map is
//!   `NOT_SUPPORTED`); TC -> PG, PFC priority -> queue / PG: accepted, nothing programmed (isai: the
//!   same); colour and remark maps (`TC_AND_COLOR_TO_*`): `NOT_SUPPORTED`;
//! - schedulers: queue `SCHEDULING_MODE` (strict / WDRR) + `WDRR_WEIGHT` + guaranteed / max rate (bits/s);
//!   the port shaper: devport `MAX_RATE` (+ burst);
//! - WRED: an `aqm_curve_group` per profile (thresholds in cells), queue `AQM_CURVE_GROUP` +
//!   `WRED_ENABLE` + `ECN_EXTENSION_ENABLE` (0.12.41's refusals: `NOT_SUPPORTED`, the queue rolled back);
//! - buffer profiles on queues: `RESERVED_SIZE` (cells, the port disabled around a change while it is
//!   up, as isai; refused by 0.12.41: then thresholds only), threshold mode and factors (dynamic th n:
//!   unicast factor n + 5, non-unicast n + 1); ingress PG profiles: kept, not programmed;
//! - counters: `queue_stats_get`, `bam_queue_stats_get` (PGs), devport PFC frames.
//!
//! Ids: one counter for maps and WRED profiles, from 1 (`next_`).
#![forbid(unsafe_code)]

use core::ffi::c_int;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};

use ifcs::{Attrs, Handle, QosMapInfo, QosMapList, QosMapSrc, QueueKey, sys};
use sai_core::backend::types::{BufferCfg, QosMapEntry, QosMapId, QosMapKind, SchedCfg, WredCfg, WredId};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};

/// An unused colour's thresholds: min = max = this, 100 % (isai).
const WRED_OFF_CELLS: u32 = 0xb2a0;
/// The DTM cell size isai reads on this chip (the fallback).
const CELL_DEFAULT: u32 = 224;

/// A SAI map (`Map`).
#[derive(Clone, Debug)]
struct Map {
    kind: QosMapKind,
    entries: Vec<QosMapEntry>,
    h: Handle,
}

/// A map group and its users (`Group`).
#[derive(Clone, Copy, Debug)]
struct Group {
    h: Handle,
    refs: u32,
}

/// A WRED profile (`Wred`).
#[derive(Clone, Copy, Debug)]
struct Wred {
    cfg: WredCfg,
    h: Handle,
}

/// A queue's buffer settings as the SDK had them (`QueueDefaults`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct QueueDefaults {
    reserved: u32,
    mode: u32,
    uc: u32,
    nonuc: u32,
    uc_static: u32,
    nonuc_static: u32,
}

/// (DSCP map, DOT1P map) of a port; `None` = none (the C++ -1).
type Key = (Option<u32>, Option<u32>);
/// (devport, queue).
type QKey = (u32, u32);

/// The `QoS` area's state (`QosArea`).
#[derive(Debug)]
pub struct QosArea {
    next: u32,
    /// The DTM cell size once read (`cell_`; 0: not yet). Read from `&self` paths (counters).
    cell: AtomicU32,
    maps: BTreeMap<u32, Map>,
    groups: BTreeMap<Key, Group>,
    /// devport -> the group key it uses.
    port_key: BTreeMap<u32, Key>,
    wreds: BTreeMap<u32, Wred>,
    /// (devport, queue) -> WRED id.
    queue_wred: BTreeMap<QKey, u32>,
    queue_defaults: BTreeMap<QKey, QueueDefaults>,
    /// What we programmed (cells).
    queue_reserved: BTreeMap<QKey, u32>,
    /// 0.12.41 refuses queue `RESERVED_SIZE` (chip 2026-11-16).
    reserve_refused: bool,
    /// The PG profile warning was given.
    pg_logged: bool,
    /// devport -> `PFC_MODE` as programmed.
    pfc_mode: BTreeMap<u32, u32>,
    /// Queues with `ECN_EXTENSION_ENABLE` on.
    queue_ecn: BTreeSet<QKey>,
}

impl Default for QosArea {
    fn default() -> Self {
        Self {
            next: 1,
            cell: AtomicU32::new(0),
            maps: BTreeMap::new(),
            groups: BTreeMap::new(),
            port_key: BTreeMap::new(),
            wreds: BTreeMap::new(),
            queue_wred: BTreeMap::new(),
            queue_defaults: BTreeMap::new(),
            queue_reserved: BTreeMap::new(),
            reserve_refused: false,
            pg_logged: false,
            pfc_mode: BTreeMap::new(),
            queue_ecn: BTreeSet::new(),
        }
    }
}

/// The kinds that program an IFCS map (`programmed`).
const fn programmed(k: QosMapKind) -> bool {
    matches!(k, QosMapKind::DscpToTc | QosMapKind::Dot1pToTc)
}

fn identity(e: &[QosMapEntry]) -> bool {
    e.iter().all(|x| x.value == x.key)
}

fn all_green(e: &[QosMapEntry]) -> bool {
    e.iter().all(|x| x.value_color == 0)
}

/// `check_kind`: kinds that program nothing (TC -> PG, the PFC priority maps, TC -> queue when it is the
/// identity, the colour maps when all green) are accepted; the rest `NOT_SUPPORTED`.
fn check_kind(core: &Core, k: QosMapKind, e: &[QosMapEntry]) -> Result<()> {
    match k {
        QosMapKind::DscpToTc
        | QosMapKind::Dot1pToTc
        | QosMapKind::TcToPg
        | QosMapKind::PfcPrioToQueue
        | QosMapKind::PfcPrioToPg => Ok(()),
        QosMapKind::TcToQueue => {
            if identity(e) {
                return Ok(());
            }
            core.log_once(
                "qos:tcq",
                format_args!("TC_TO_QUEUE: only the identity map (the chip's fixed TC = queue) is supported"),
            );
            Err(Error::NotSupported)
        }
        QosMapKind::DscpToColor | QosMapKind::Dot1pToColor => {
            if all_green(e) {
                return Ok(());
            }
            core.log_once(
                "qos:color",
                format_args!("DSCP/DOT1P_TO_COLOR: only all-green maps are supported"),
            );
            Err(Error::NotSupported)
        }
        QosMapKind::TcColorToDscp | QosMapKind::TcColorToDot1p => {
            core.log_once(
                "qos:remark",
                format_args!("TC_AND_COLOR_TO_DSCP/DOT1P (remarking) is not implemented"),
            );
            Err(Error::NotSupported)
        }
    }
}

/// SAI bytes/s as IFCS bits/s (a devport `MAX_RATE` of 12500000 drained a full queue at 12.5 Mbit/s on
/// the wire, chip 2026-11-16; isai passes them unscaled: 8x too slow).
const fn bits(bytes_per_s: u64) -> u64 {
    bytes_per_s.wrapping_mul(8)
}

/// Bytes to cells, rounded up (`(b + cell - 1) / cell`, as `uint32_t`).
fn cells_up(bytes: u64, cell: u32) -> u32 {
    let cell = u64::from(cell);
    #[allow(clippy::cast_possible_truncation, reason = "the C++'s (uint32_t) cast")]
    let c = (bytes.wrapping_add(cell).wrapping_sub(1) / cell) as u32;
    c
}

/// The threshold attributes of a queue buffer profile (`queue_buffer_set`): the SDK's values back
/// without one; dynamic th n: unicast factor n + 5, non-unicast n + 1 (isai: th 3 -> 8 / 4, th 1 -> 6 / 2;
/// below its range: factor 0, said once); static: the limit in cells for both.
fn thresholds(core: &Core, d: &QueueDefaults, b: Option<&BufferCfg>, cell: u32) -> Vec<(u32, u32)> {
    match b {
        None => vec![
            (sys::IFCS_QUEUE_ATTR_THRESHOLD_MODE, d.mode),
            (sys::IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR, d.uc),
            (sys::IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR, d.nonuc),
            (sys::IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT, d.uc_static),
            (sys::IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT, d.nonuc_static),
        ],
        Some(b) if b.dynamic => {
            // isai: th 3 -> 8 / 4, th 1 -> 6 / 2
            let mut uc = i32::from(b.dynamic_th) + 5;
            let mut nonuc = i32::from(b.dynamic_th) + 1;
            if uc < 0 || nonuc < 0 {
                // below isai's range: the smallest factor
                core.log_once(
                    "qos:th",
                    format_args!(
                        "buffer profile: SHARED_DYNAMIC_TH {} below the factor range; using factor 0",
                        b.dynamic_th
                    ),
                );
                uc = uc.max(0);
                nonuc = nonuc.max(0);
            }
            vec![
                (
                    sys::IFCS_QUEUE_ATTR_THRESHOLD_MODE,
                    sys::IFCS_QUEUE_THRESHOLD_MODE_DYNAMIC,
                ),
                (sys::IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR, uc.cast_unsigned()),
                (
                    sys::IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR,
                    nonuc.cast_unsigned(),
                ),
            ]
        }
        Some(b) => {
            let lim = cells_up(b.static_th, cell);
            vec![
                (
                    sys::IFCS_QUEUE_ATTR_THRESHOLD_MODE,
                    sys::IFCS_QUEUE_THRESHOLD_MODE_STATIC,
                ),
                (sys::IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT, lim),
                (sys::IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT, lim),
            ]
        }
    }
}

/// `attr_u32` of each (id, value), in order.
fn u32s(pairs: &[(u32, u32)]) -> Attrs {
    pairs.iter().fold(Attrs::new(), |a, (id, v)| a.u32(*id, *v))
}

/// `queue_stat_id`: the IFCS queue counter of a SAI one, and whether it counts cells.
fn queue_stat_id(s: ssys::sai_stat_id_t) -> Option<(c_int, bool)> {
    let id = match s {
        ssys::SAI_QUEUE_STAT_PACKETS => (sys::IFCS_QUEUE_STATS_ID_PACKET_TX_COUNT, false),
        ssys::SAI_QUEUE_STAT_BYTES => (sys::IFCS_QUEUE_STATS_ID_BYTE_TX_COUNT, false),
        ssys::SAI_QUEUE_STAT_DROPPED_PACKETS => (sys::IFCS_QUEUE_STATS_ID_PACKET_DROP_COUNT, false),
        ssys::SAI_QUEUE_STAT_CURR_OCCUPANCY_BYTES => (sys::IFCS_QUEUE_STATS_ID_TOTAL_USE_COUNT, true),
        ssys::SAI_QUEUE_STAT_WATERMARK_BYTES | ssys::SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES => {
            (sys::IFCS_QUEUE_STATS_ID_TOTAL_USE_WATERMARK, true)
        }
        ssys::SAI_QUEUE_STAT_WRED_ECN_MARKED_PACKETS => (sys::IFCS_QUEUE_STATS_ID_PACKET_ECN_MARKED_COUNT, false),
        ssys::SAI_QUEUE_STAT_WRED_ECN_MARKED_BYTES => (sys::IFCS_QUEUE_STATS_ID_BYTE_ECN_MARKED_COUNT, false),
        _ => return None,
    };
    Some((id.0.cast_signed(), id.1))
}

/// `pg_stat_id`: an ingress priority group = the BAM (ingress admission) queue of that index: drops,
/// occupancy, watermark (cells x the DTM cell size: unverified for BAM).
fn pg_stat_id(s: ssys::sai_stat_id_t) -> Option<(c_int, bool)> {
    let id = match s {
        ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_DROPPED_PACKETS => {
            (sys::IFCS_BAM_QUEUE_STATS_ID_PACKET_DROP_COUNT, false)
        }
        ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_CURR_OCCUPANCY_BYTES => {
            (sys::IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_COUNT, true)
        }
        ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_WATERMARK_BYTES
        | ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_SHARED_WATERMARK_BYTES => {
            (sys::IFCS_BAM_QUEUE_STATS_ID_TOTAL_USE_WATERMARK, true)
        }
        _ => return None,
    };
    Some((id.0.cast_signed(), id.1))
}

impl QosArea {
    /// `cell_bytes`: the DTM cell size (node `DTM_CELL_SIZE`, read once; else 224, said once).
    pub fn cell_bytes(&self, core: &Core) -> u32 {
        let c = self.cell.load(Ordering::Relaxed);
        if c != 0 {
            return c;
        }
        let mut a = Attrs::new().of(sys::IFCS_NODE_ATTR_DTM_CELL_SIZE);
        let (rc, _) = core.lib.ifcs_node_attr_get(NODE, &mut a);
        let c = if rc.is_success() && a.u16_at(0) != 0 {
            u32::from(a.u16_at(0))
        } else {
            core.log_once("qos:cell", format_args!("DTM cell size unreadable; using 224 bytes"));
            CELL_DEFAULT
        };
        self.cell.store(c, Ordering::Relaxed);
        c
    }

    /// `queue_key`: `ifcs_queue_t_init`, devport, queue id.
    fn queue_key(core: &Core, hw: u32, q: u32) -> QueueKey {
        let mut k = QueueKey::zeroed();
        let _ = core.lib.ifcs_queue_t_init(&mut k);
        let _ = core.lib.ifcs_queue_t_devport_set(&mut k, hw);
        let _ = core.lib.ifcs_queue_t_queue_id_set(&mut k, q);
        k
    }

    /// `queue_set`: queue attributes (`INVALID_PARAMETER` for a devport off the port map or a queue
    /// beyond 7).
    fn queue_set(core: &Core, hw: u32, q: u32, attrs: Attrs, what: &str) -> Result<()> {
        if core.front(hw).is_none() || q > 7 {
            return Err(Error::InvalidParameter);
        }
        let mut k = Self::queue_key(core, hw, q);
        let mut attrs = attrs;
        core.call(core.lib.ifcs_queue_attr_set(NODE, &mut k, &mut attrs), what)
    }

    // ------------------------------------------------------------- QoS maps

    /// `map_info`: the map's table as an `ifcs_qos_map_info_t` attribute (`QOS_MAP_INFO`).
    fn map_info(core: &Core, m: &Map) -> Attrs {
        let mut list = QosMapList::new();
        for e in &m.entries {
            let src = if m.kind == QosMapKind::DscpToTc {
                QosMapSrc::Dscp(e.key)
            } else {
                QosMapSrc::Dot1pCfi { dot1p: e.key, cfi: 0 } // CFI 0 (isai: 8 entries)
            };
            list.push(src, e.value, 0); // green: the colour maps are separate SAI maps
        }
        let mut info = QosMapInfo::zeroed();
        let _ = core.lib.ifcs_qos_map_info_t_init(&mut info);
        info.set_type(if m.kind == QosMapKind::DscpToTc {
            sys::IFCS_QOS_MAP_TYPE_DSCP_TO_TC_DP
        } else {
            sys::IFCS_QOS_MAP_TYPE_DOT1P_CFI_TO_TC_DP
        });
        Attrs::new().qos_map_info(sys::IFCS_QOS_MAP_ATTR_QOS_MAP_INFO, &info, &list)
    }

    /// `map_create`.
    pub fn map_create(&mut self, core: &Core, kind: QosMapKind, entries: &[QosMapEntry]) -> Result<QosMapId> {
        check_kind(core, kind, entries)?;
        let mut m = Map {
            kind,
            entries: entries.to_vec(),
            h: Handle::NULL,
        };
        if programmed(kind) {
            let mut a = Self::map_info(core, &m);
            core.call(core.lib.ifcs_qos_map_create(NODE, &mut m.h, &mut a), "qos map create")?;
        }
        let id = self.next;
        self.next += 1;
        self.maps.insert(id, m);
        Ok(QosMapId(id))
    }

    /// `map_update`.
    pub fn map_update(&mut self, core: &Core, id: QosMapId, entries: &[QosMapEntry]) -> Result<()> {
        let Some(cur) = self.maps.get(&id.0) else {
            return Err(Error::ItemNotFound);
        };
        check_kind(core, cur.kind, entries)?;
        let mut m = cur.clone();
        m.entries = entries.to_vec();
        if !m.h.is_null() {
            let mut a = Self::map_info(core, &m);
            core.call(core.lib.ifcs_qos_map_attr_set(NODE, m.h.0, &mut a), "qos map set")?;
        }
        self.maps.insert(id.0, m);
        Ok(())
    }

    /// `map_destroy`: `OBJECT_IN_USE` while a port uses it.
    pub fn map_destroy(&mut self, core: &Core, id: QosMapId) -> Result<()> {
        let Some(m) = self.maps.get(&id.0) else {
            return Err(Error::ItemNotFound);
        };
        if self.port_key.values().any(|k| k.0 == Some(id.0) || k.1 == Some(id.0)) {
            return Err(Error::ObjectInUse);
        }
        if !m.h.is_null() {
            core.call(core.lib.ifcs_qos_map_delete(NODE, m.h), "qos map delete")?;
        }
        self.maps.remove(&id.0);
        Ok(())
    }

    fn map_handle(&self, id: Option<u32>) -> Option<Handle> {
        id.map(|i| self.maps.get(&i).map_or(Handle::NULL, |m| m.h))
    }

    /// `group_get`: the group of a key (made on first use), one more user.
    fn group_get(&mut self, core: &Core, k: Key) -> Result<Handle> {
        if let Some(g) = self.groups.get_mut(&k) {
            g.refs += 1;
            return Ok(g.h);
        }
        let mut a = Attrs::new().u32(sys::IFCS_QOS_MAP_GROUP_ATTR_ACTION_TYPE, sys::IFCS_QOS_ACTION_TYPE_MAP);
        if let Some(h) = self.map_handle(k.0) {
            a = a.handle(sys::IFCS_QOS_MAP_GROUP_ATTR_DSCP_TO_TC_DP, h);
        }
        if let Some(h) = self.map_handle(k.1) {
            a = a.handle(sys::IFCS_QOS_MAP_GROUP_ATTR_DOT1P_CFI_TO_TC_DP, h);
        }
        let mut h = Handle::NULL;
        core.call(
            core.lib.ifcs_qos_map_group_create(NODE, &mut h, &mut a),
            "qos map group create",
        )?;
        self.groups.insert(k, Group { h, refs: 1 });
        Ok(h)
    }

    /// `group_put`: one user less; the last one deletes it.
    fn group_put(&mut self, core: &Core, k: Key) {
        let Some(g) = self.groups.get_mut(&k) else { return };
        g.refs -= 1;
        if g.refs > 0 {
            return;
        }
        let h = g.h;
        let _ = core.call(core.lib.ifcs_qos_map_group_delete(NODE, h), "qos map group delete");
        self.groups.remove(&k);
    }

    /// `port_map_set`: a port's map of one kind (`None`: the default).
    pub fn port_map_set(&mut self, core: &Core, hw: u32, kind: QosMapKind, id: Option<QosMapId>) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        let id = id.map(|i| i.0);
        if let Some(i) = id {
            let Some(m) = self.maps.get(&i) else {
                return Err(Error::ItemNotFound);
            };
            if m.kind != kind {
                return Err(Error::InvalidParameter);
            }
        }
        if !programmed(kind) {
            return Ok(()); // nothing per port (check_kind)
        }
        let old = self.port_key.get(&hw).copied().unwrap_or((None, None));
        let mut now = old;
        if kind == QosMapKind::DscpToTc {
            now.0 = id;
        } else {
            now.1 = id;
        }
        if now == old {
            return Ok(());
        }
        let any = now.0.is_some() || now.1.is_some();
        let mut h = Handle::NULL;
        if any {
            h = self.group_get(core, now)?;
        }
        let dscp = Attrs::new().handle(
            sys::IFCS_SYSPORT_ATTR_QOS_DSCP_TO_TC_DP_MAP,
            if now.0.is_some() { h } else { Handle::NULL },
        );
        let dot1p = Attrs::new().handle(
            sys::IFCS_SYSPORT_ATTR_QOS_DOT1P_CFI_TO_TC_DP_MAP,
            if now.1.is_some() { h } else { Handle::NULL },
        );
        let st = core
            .sp_set(hw, dscp, "sysport dscp map")
            .and_then(|()| core.sp_set(hw, dot1p, "sysport dot1p map"));
        if let Err(e) = st {
            if any {
                self.group_put(core, now);
            }
            return Err(e);
        }
        if any {
            self.port_key.insert(hw, now);
        } else {
            self.port_key.remove(&hw);
        }
        if old.0.is_some() || old.1.is_some() {
            self.group_put(core, old);
        }
        Ok(())
    }

    /// `default_tc_set`.
    pub fn default_tc_set(core: &Core, hw: u32, tc: u8) -> Result<()> {
        if tc > 7 {
            return Err(Error::InvalidParameter);
        }
        core.sp_set(
            hw,
            Attrs::new().u8(sys::IFCS_SYSPORT_ATTR_DEFAULT_TC, tc),
            "sysport default tc",
        )
    }

    // ------------------------------------------------------------------ PFC

    /// `pfc_set`: the devport's PFC mode (the SDK's read once; isai reads it every time), then the eight
    /// per-priority enables in one call.
    pub fn pfc_set(&mut self, core: &Core, hw: u32, separate: bool, tx: u8, rx: u8) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        let mode = if let Some(m) = self.pfc_mode.get(&hw) {
            *m
        } else {
            let m = core.dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_PFC_MODE).unwrap_or(0);
            self.pfc_mode.insert(hw, m);
            m
        };
        let want = if separate {
            sys::IFCS_DEVPORT_PFC_MODE_SEPARATE
        } else {
            sys::IFCS_DEVPORT_PFC_MODE_COMBINED
        };
        if mode != want {
            core.dp_set(
                hw,
                Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_PFC_MODE, want),
                "devport pfc mode",
            )?;
            self.pfc_mode.insert(hw, want);
        }
        let mut a = Attrs::new();
        for p in 0..8u32 {
            let t = u32::from(tx >> p) & 1;
            if separate {
                a = a
                    .u32(sys::IFCS_DEVPORT_ATTR_PFC0_TX_ENABLE + p, t)
                    .u32(sys::IFCS_DEVPORT_ATTR_PFC0_RX_ENABLE + p, u32::from(rx >> p) & 1);
            } else {
                a = a.u32(sys::IFCS_DEVPORT_ATTR_PFC0_ENABLE + p, t);
            }
        }
        core.dp_set(hw, a, "devport pfc")
    }

    /// `pfc_stat_get`: the devport's PFC frames of a priority.
    pub fn pfc_stat_get(core: &Core, hw: u32, prio: u8, tx: bool) -> Result<u64> {
        if core.front(hw).is_none() || prio > 7 {
            return Err(Error::InvalidParameter);
        }
        let base = if tx {
            sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_PFC0
        } else {
            sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_PFC0
        };
        let mut ids = [(base + u32::from(prio)).cast_signed()];
        let mut v = [0u64];
        let (rc, _) = core.lib.ifcs_devport_stats_get(NODE, hw, &mut ids, &mut v);
        core.call(rc, "devport pfc stats")?;
        Ok(v[0])
    }

    // ------------------------------------------------------------ schedulers

    /// `queue_sched_set`: isai's form: strict = mode 0; DWRR / WRR = mode 1 + weight; both rate enables
    /// written every time (0 when the profile has none); unset = WDRR weight 1. Rates in bits/s, bursts
    /// in bytes as SAI gives them (unverified).
    pub fn queue_sched_set(core: &Core, hw: u32, q: u8, s: Option<&SchedCfg>) -> Result<()> {
        if let Some(s) = s {
            if s.packets && (s.min_rate != 0 || s.max_rate != 0) {
                core.log_once(
                    "qos:pps",
                    format_args!("scheduler: packet-rate shaping is not supported (byte rates only)"),
                );
                return Err(Error::NotSupported);
            }
        }
        let mut a = Attrs::new();
        if s.is_some_and(|s| s.strict) {
            a = a.u32(
                sys::IFCS_QUEUE_ATTR_SCHEDULING_MODE,
                sys::IFCS_QUEUE_SCHEDULING_MODE_STRICT,
            );
        } else {
            a = a
                .u32(
                    sys::IFCS_QUEUE_ATTR_SCHEDULING_MODE,
                    sys::IFCS_QUEUE_SCHEDULING_MODE_WDRR,
                )
                .u32(sys::IFCS_QUEUE_ATTR_WDRR_WEIGHT, s.map_or(1, |s| s.weight));
        }
        let min = s.map_or(0, |s| s.min_rate);
        a = a.u32(sys::IFCS_QUEUE_ATTR_GUARANTEED_RATE_ENABLE, u32::from(min != 0));
        if let Some(s) = s.filter(|s| s.min_rate != 0) {
            a = a.u64(sys::IFCS_QUEUE_ATTR_GUARANTEED_RATE, bits(s.min_rate));
            if s.min_burst != 0 {
                #[allow(clippy::cast_possible_truncation, reason = "the C++'s (uint32_t) cast")]
                let b = s.min_burst as u32;
                a = a.u32(sys::IFCS_QUEUE_ATTR_GUARANTEED_RATE_BURST_SIZE, b);
            }
        }
        let max = s.map_or(0, |s| s.max_rate);
        a = a.u32(sys::IFCS_QUEUE_ATTR_MAX_RATE_ENABLE, u32::from(max != 0));
        if let Some(s) = s.filter(|s| s.max_rate != 0) {
            a = a.u64(sys::IFCS_QUEUE_ATTR_MAX_RATE, bits(s.max_rate));
            if s.max_burst != 0 {
                #[allow(clippy::cast_possible_truncation, reason = "the C++'s (uint32_t) cast")]
                let b = s.max_burst as u32;
                a = a.u32(sys::IFCS_QUEUE_ATTR_MAX_RATE_BURST_SIZE, b);
            }
        }
        Self::queue_set(core, hw, u32::from(q), a, "queue scheduler")
    }

    /// `port_sched_set`: the port shaper (devport `MAX_RATE` in bits/s, burst bytes).
    pub fn port_sched_set(core: &Core, hw: u32, s: Option<&SchedCfg>) -> Result<()> {
        if core.front(hw).is_none() {
            return Err(Error::InvalidPortNumber);
        }
        if s.is_some_and(|s| s.packets && s.max_rate != 0) {
            core.log_once(
                "qos:pps",
                format_args!("scheduler: packet-rate shaping is not supported (byte rates only)"),
            );
            return Err(Error::NotSupported);
        }
        let max = s.map_or(0, |s| s.max_rate);
        let mut a = Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_MAX_RATE_ENABLE, u32::from(max != 0));
        if let Some(s) = s.filter(|s| s.max_rate != 0) {
            a = a.u64(sys::IFCS_DEVPORT_ATTR_MAX_RATE, bits(s.max_rate));
            if s.max_burst != 0 {
                #[allow(clippy::cast_possible_truncation, reason = "the C++'s (uint32_t) cast")]
                let b = s.max_burst as u32;
                a = a.u32(sys::IFCS_DEVPORT_ATTR_MAX_BURST_SIZE, b);
            }
        }
        core.dp_set(hw, a, "devport shaper")
    }

    // ------------------------------------------------------------------ WRED

    /// `curve_attrs`: per colour min / max (bytes -> cells, down to a multiple of 8: isai's 100000 B ->
    /// 440, 2000000 B -> 8928) and probability; an unused colour `WRED_OFF_CELLS`, 100 %.
    fn curve_attrs(&self, core: &Core, w: &WredCfg) -> Attrs {
        let cell = self.cell_bytes(core);
        let ids = [
            [
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MIN_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAX_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_GREEN_MAXP_PERCENTAGE,
            ],
            [
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MIN_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAX_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_YELLOW_MAXP_PERCENTAGE,
            ],
            [
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MIN_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAX_THRESHOLD,
                sys::IFCS_AQM_CURVE_GROUP_ATTR_WRED_RED_MAXP_PERCENTAGE,
            ],
        ];
        let mut a = Attrs::new();
        for (c, id) in ids.iter().enumerate() {
            let on = w.enable.get(c).copied().unwrap_or(false);
            let at = |v: &[u32; 3]| v.get(c).copied().unwrap_or(0);
            let mn = if on { (at(&w.min) / cell) & !7 } else { WRED_OFF_CELLS };
            let mx = if on { (at(&w.max) / cell) & !7 } else { WRED_OFF_CELLS };
            let p = if on { at(&w.prob) } else { 100 };
            a = a.u32(id[0], mn).u32(id[1], mx).u32(id[2], p);
        }
        a
    }

    /// `wred_flags`: the queue's WRED / ECN switches, one attribute per call (a refusal then names the
    /// attribute); the averaging weight only when the profile has one; `ECN_EXTENSION_ENABLE` only when
    /// the profile marks (or to undo that). 0.12.41 refuses `WRED_ENABLE` 1 and `ECN_EXTENSION_ENABLE` on
    /// these queues ("Failed to Set PD State", PARAM, chip 2026-11-16): `NOT_SUPPORTED` then.
    fn wred_flags(&mut self, core: &Core, hw: u32, q: u32, w: Option<&WredCfg>) -> Result<()> {
        let k = (hw, q);
        let any = w.is_some_and(|w| w.enable.iter().any(|e| *e));
        let ecn = w.is_some_and(|w| w.ecn != ssys::SAI_ECN_MARK_MODE_NONE.cast_signed());
        let mut st = Ok(());
        if let Some(w) = w.filter(|w| w.weight != 0) {
            st = Self::queue_set(
                core,
                hw,
                q,
                Attrs::new().u32(sys::IFCS_QUEUE_ATTR_AQM_QSIZE_AVERAGING_WEIGHT, u32::from(w.weight)),
                "queue aqm averaging weight",
            );
        }
        if st.is_ok() {
            st = Self::queue_set(
                core,
                hw,
                q,
                Attrs::new().u32(sys::IFCS_QUEUE_ATTR_WRED_ENABLE, u32::from(any)),
                "queue wred enable",
            );
            if st.is_err() && any {
                core.log_once(
                    "qos:wred",
                    format_args!("WRED: the SDK refuses WRED_ENABLE on this queue (IFCS 0.12.41); not supported"),
                );
                st = Err(Error::NotSupported);
            }
        }
        if st.is_ok() && (ecn || self.queue_ecn.contains(&k)) {
            st = Self::queue_set(
                core,
                hw,
                q,
                Attrs::new().u32(sys::IFCS_QUEUE_ATTR_ECN_EXTENSION_ENABLE, u32::from(ecn)),
                "queue ecn enable",
            );
            if st.is_ok() {
                if ecn {
                    self.queue_ecn.insert(k);
                } else {
                    self.queue_ecn.remove(&k);
                }
            } else if ecn {
                core.log_once(
                    "qos:ecn",
                    format_args!("WRED ECN marking: the SDK refuses ECN_EXTENSION_ENABLE on this queue"),
                );
                st = Err(Error::NotSupported);
            }
        }
        st
    }

    /// `wred_create`: its curve group.
    pub fn wred_create(&mut self, core: &Core, w: &WredCfg) -> Result<WredId> {
        let mut x = Wred {
            cfg: *w,
            h: Handle::NULL,
        };
        let mut a = self.curve_attrs(core, w);
        core.call(
            core.lib.ifcs_aqm_curve_group_create(NODE, &mut x.h, &mut a),
            "aqm curve group create",
        )?;
        let id = self.next;
        self.next += 1;
        self.wreds.insert(id, x);
        Ok(WredId(id))
    }

    /// `wred_update`: the curve group, then the flags of every queue using it.
    pub fn wred_update(&mut self, core: &Core, id: WredId, w: &WredCfg) -> Result<()> {
        let Some(h) = self.wreds.get(&id.0).map(|x| x.h) else {
            return Err(Error::ItemNotFound);
        };
        let mut a = self.curve_attrs(core, w);
        core.call(
            core.lib.ifcs_aqm_curve_group_attr_set(NODE, h.0, &mut a),
            "aqm curve group set",
        )?;
        if let Some(x) = self.wreds.get_mut(&id.0) {
            x.cfg = *w;
        }
        let users: Vec<QKey> = self
            .queue_wred
            .iter()
            .filter(|(_, wid)| **wid == id.0)
            .map(|(k, _)| *k)
            .collect();
        for (hw, q) in users {
            // ECN / enable flags live on the queues
            self.wred_flags(core, hw, q, Some(w))?;
        }
        Ok(())
    }

    /// `wred_destroy`: `OBJECT_IN_USE` while a queue uses it.
    pub fn wred_destroy(&mut self, core: &Core, id: WredId) -> Result<()> {
        let Some(h) = self.wreds.get(&id.0).map(|x| x.h) else {
            return Err(Error::ItemNotFound);
        };
        if self.queue_wred.values().any(|w| *w == id.0) {
            return Err(Error::ObjectInUse);
        }
        core.call(core.lib.ifcs_aqm_curve_group_delete(NODE, h), "aqm curve group delete")?;
        self.wreds.remove(&id.0);
        Ok(())
    }

    /// `queue_wred_set`: isai's order: the curve group first, then averaging weight / ECN / enable; a
    /// refused flag takes the queue back to no curve.
    pub fn queue_wred_set(&mut self, core: &Core, hw: u32, q: u8, id: Option<WredId>) -> Result<()> {
        let q = u32::from(q);
        let k = (hw, q);
        if let Some(id) = id {
            let Some(x) = self.wreds.get(&id.0).copied() else {
                return Err(Error::ItemNotFound);
            };
            Self::queue_set(
                core,
                hw,
                q,
                Attrs::new().handle(sys::IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, x.h),
                "queue aqm curve",
            )?;
            if let Err(e) = self.wred_flags(core, hw, q, Some(&x.cfg)) {
                // back to no curve
                let _ = self.wred_flags(core, hw, q, None);
                let _ = Self::queue_set(
                    core,
                    hw,
                    q,
                    Attrs::new().handle(sys::IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, Handle::NULL),
                    "queue aqm curve",
                );
                return Err(e);
            }
            self.queue_wred.insert(k, id.0);
            return Ok(());
        }
        if !self.queue_wred.contains_key(&k) {
            return Ok(());
        }
        self.wred_flags(core, hw, q, None)?;
        Self::queue_set(
            core,
            hw,
            q,
            Attrs::new().handle(sys::IFCS_QUEUE_ATTR_AQM_CURVE_GROUP, Handle::NULL),
            "queue aqm curve",
        )?;
        self.queue_wred.remove(&k);
        Ok(())
    }

    // --------------------------------------------------------------- buffers

    /// `queue_defaults`: the queue's reserve and thresholds as the SDK has them, read at first use.
    fn queue_defaults(&mut self, core: &Core, hw: u32, q: u32) -> Result<QueueDefaults> {
        let k = (hw, q);
        if let Some(d) = self.queue_defaults.get(&k) {
            return Ok(*d);
        }
        let mut key = Self::queue_key(core, hw, q);
        let mut a = Attrs::new()
            .of(sys::IFCS_QUEUE_ATTR_RESERVED_SIZE)
            .of(sys::IFCS_QUEUE_ATTR_THRESHOLD_MODE)
            .of(sys::IFCS_QUEUE_ATTR_UC_DYNAMIC_THRESHOLD_FACTOR)
            .of(sys::IFCS_QUEUE_ATTR_NONUC_DYNAMIC_THRESHOLD_FACTOR)
            .of(sys::IFCS_QUEUE_ATTR_UC_STATIC_THRESHOLD_LIMIT)
            .of(sys::IFCS_QUEUE_ATTR_NONUC_STATIC_THRESHOLD_LIMIT);
        let (rc, _) = core.lib.ifcs_queue_attr_get(NODE, &mut key, &mut a);
        core.call(rc, "queue attr get")?;
        let d = QueueDefaults {
            reserved: a.u32_at(0),
            mode: a.u32_at(1),
            uc: a.u32_at(2),
            nonuc: a.u32_at(3),
            uc_static: a.u32_at(4),
            nonuc_static: a.u32_at(5),
        };
        self.queue_defaults.insert(k, d);
        self.queue_reserved.insert(k, d.reserved);
        Ok(d)
    }

    /// `queue_buffer_set`: isai: `RESERVED_SIZE` changes with the port disabled (admin down, set, up),
    /// then `FORCE_DO_NOT_ADMIT` / `FORCE_BYPASS_FLOW_CONTROL` 0; threshold changes alone are written on
    /// the running port. IFCS 0.12.41 refuses `RESERVED_SIZE` ("Attr id(2) is Not valid for this
    /// API(`ifcs_queue_attr_set`)", `SONiC`'s `BUFFER_QUEUE` on the chip, 2026-11-16): the reserve stays the
    /// SDK's, thresholds only, and from then on no reserve is tried.
    pub fn queue_buffer_set(&mut self, core: &Core, hw: u32, q: u8, b: Option<&BufferCfg>) -> Result<()> {
        let q = u32::from(q);
        if core.front(hw).is_none() || q > 7 {
            return Err(Error::InvalidParameter);
        }
        if b.is_some_and(|b| b.ingress) {
            return Err(Error::InvalidParameter);
        }
        let d = self.queue_defaults(core, hw, q)?;
        let cell = self.cell_bytes(core);
        let reserved = b.map_or(d.reserved, |b| cells_up(b.reserved, cell));
        let th = thresholds(core, &d, b, cell);
        let k = (hw, q);
        if self.queue_reserved.get(&k).copied().unwrap_or(0) == reserved || self.reserve_refused {
            return Self::queue_set(core, hw, q, u32s(&th), "queue thresholds");
        }
        let admin = core.dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_ADMIN_STATE).unwrap_or(0);
        if admin != 0 {
            core.dp_set(
                hw,
                Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_ADMIN_STATE, 0),
                "devport disable (queue reserve)",
            )?;
        }
        let mut all = vec![(sys::IFCS_QUEUE_ATTR_RESERVED_SIZE, reserved)];
        all.extend_from_slice(&th);
        let mut st = Self::queue_set(core, hw, q, u32s(&all), "queue reserve + thresholds");
        if st.is_err() {
            self.reserve_refused = true;
            log::warn(
                ssys::SAI_API_BUFFER,
                format_args!(
                    "teralynx: queue RESERVED_SIZE refused by this SDK; buffer profile size kept at the SDK's \
                     reserve, thresholds programmed"
                ),
            );
            st = Self::queue_set(core, hw, q, u32s(&th), "queue thresholds");
        }
        let up = if admin != 0 {
            core.dp_set(
                hw,
                Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_ADMIN_STATE, 1),
                "devport enable (queue reserve)",
            )
        } else {
            Ok(())
        };
        st?;
        up?;
        self.queue_reserved.insert(k, reserved);
        Self::queue_set(
            core,
            hw,
            q,
            Attrs::new()
                .u32(sys::IFCS_QUEUE_ATTR_FORCE_DO_NOT_ADMIT, 0)
                .u32(sys::IFCS_QUEUE_ATTR_FORCE_BYPASS_FLOW_CONTROL, 0),
            "queue admit",
        )
    }

    /// `pg_buffer_set`: ingress priority-group profiles are kept, not programmed (said once).
    pub fn pg_buffer_set(&mut self, core: &Core, hw: u32, pg: u8, b: Option<&BufferCfg>) -> Result<()> {
        if core.front(hw).is_none() || pg > 7 {
            return Err(Error::InvalidParameter);
        }
        if b.is_some() && !self.pg_logged {
            self.pg_logged = true;
            log::warn(
                ssys::SAI_API_BUFFER,
                format_args!(
                    "teralynx: ingress priority-group buffer profiles are kept, not programmed (the SDK init \
                     config has no lossless buffer partition; xoff/xon headroom has no effect)"
                ),
            );
        }
        Ok(())
    }

    // -------------------------------------------------------------- counters

    /// `queue_stats_get`: TX packets / bytes, drops, occupancy and watermark (cells x the DTM cell size:
    /// `SHARED_WATERMARK` is the total use, reserved included), ECN marks; one batched call. An unmapped
    /// id: `NOT_SUPPORTED` (nothing read).
    pub fn queue_stats_get(&self, core: &Core, hw: u32, q: u8, ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        let q = u32::from(q);
        if core.front(hw).is_none() || q > 7 {
            return Err(Error::InvalidParameter);
        }
        let mut v = Vec::with_capacity(ids.len());
        let mut cells = Vec::with_capacity(ids.len());
        for s in ids {
            let (id, cl) = queue_stat_id(*s).ok_or(Error::NotSupported)?;
            v.push(id);
            cells.push(cl);
        }
        if v.is_empty() {
            return Ok(Vec::new());
        }
        let mut k = Self::queue_key(core, hw, q);
        let mut vals = vec![0u64; v.len()];
        let (rc, _) = core.lib.ifcs_queue_stats_get(NODE, &mut k, &mut v, &mut vals);
        core.call(rc, "queue stats")?;
        let cell = u64::from(self.cell_bytes(core));
        Ok(vals
            .iter()
            .zip(&cells)
            .map(|(x, c)| if *c { x.wrapping_mul(cell) } else { *x })
            .collect())
    }

    /// `queue_stats_clear` (occupancy is a level: nothing to clear).
    pub fn queue_stats_clear(core: &Core, hw: u32, q: u8, ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        let q = u32::from(q);
        if core.front(hw).is_none() || q > 7 {
            return Err(Error::InvalidParameter);
        }
        let mut v = Vec::new();
        for s in ids {
            let (id, _) = queue_stat_id(*s).ok_or(Error::NotSupported)?;
            if *s == ssys::SAI_QUEUE_STAT_CURR_OCCUPANCY_BYTES {
                continue;
            }
            v.push(id);
        }
        if v.is_empty() {
            return Ok(());
        }
        let mut k = Self::queue_key(core, hw, q);
        core.call(
            core.lib.ifcs_queue_stats_clear(NODE, &mut k, &mut v),
            "queue stats clear",
        )
    }

    /// The BAM queue key of a PG (`ifcs_bam_queue_t_init`, then devport, queue id, dirty 3 directly).
    fn bam_key(core: &Core, hw: u32, pg: u32) -> QueueKey {
        let mut k = QueueKey::zeroed();
        let _ = core.lib.ifcs_bam_queue_t_init(&mut k);
        k.set_devport(hw);
        k.set_queue_id(pg);
        k.set_dirty(3);
        k
    }

    /// `pg_stats_get`.
    pub fn pg_stats_get(&self, core: &Core, hw: u32, pg: u8, ids: &[ssys::sai_stat_id_t]) -> Result<Vec<u64>> {
        let pg = u32::from(pg);
        if core.front(hw).is_none() || pg > 7 {
            return Err(Error::InvalidParameter);
        }
        let mut v = Vec::with_capacity(ids.len());
        let mut cells = Vec::with_capacity(ids.len());
        for s in ids {
            let (id, cl) = pg_stat_id(*s).ok_or(Error::NotSupported)?;
            v.push(id);
            cells.push(cl);
        }
        if v.is_empty() {
            return Ok(Vec::new());
        }
        let mut k = Self::bam_key(core, hw, pg);
        let mut vals = vec![0u64; v.len()];
        let (rc, _) = core.lib.ifcs_bam_queue_stats_get(NODE, &mut k, &mut v, &mut vals);
        core.call(rc, "bam queue stats")?;
        let cell = u64::from(self.cell_bytes(core));
        Ok(vals
            .iter()
            .zip(&cells)
            .map(|(x, c)| if *c { x.wrapping_mul(cell) } else { *x })
            .collect())
    }

    /// `pg_stats_clear`.
    pub fn pg_stats_clear(core: &Core, hw: u32, pg: u8, ids: &[ssys::sai_stat_id_t]) -> Result<()> {
        let pg = u32::from(pg);
        if core.front(hw).is_none() || pg > 7 {
            return Err(Error::InvalidParameter);
        }
        let mut v = Vec::new();
        for s in ids {
            let (id, _) = pg_stat_id(*s).ok_or(Error::NotSupported)?;
            if *s != ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_CURR_OCCUPANCY_BYTES {
                v.push(id);
            }
        }
        if v.is_empty() {
            return Ok(());
        }
        let mut k = Self::bam_key(core, hw, pg);
        core.call(
            core.lib.ifcs_bam_queue_stats_clear(NODE, &mut k, &mut v),
            "bam queue stats clear",
        )
    }

    /// `show` (the C++ `diag "qos"` text; no diag command calls it there either).
    pub fn show(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "qos: {} maps, {} map groups, {} ports mapped, {} wred, cell {} B",
            self.maps.len(),
            self.groups.len(),
            self.port_key.len(),
            self.wreds.len(),
            self.cell.load(Ordering::Relaxed)
        );
        let num = |v: Option<u32>| v.map_or(-1, i64::from);
        for (hw, k) in &self.port_key {
            let _ = writeln!(s, "  devport {hw}: dscp map {} dot1p map {}", num(k.0), num(k.1));
        }
        for ((hw, q), w) in &self.queue_wred {
            let _ = writeln!(s, "  queue {hw}.{q}: wred {w}");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: u32, value: u32) -> QosMapEntry {
        QosMapEntry {
            key,
            value,
            ..QosMapEntry::default()
        }
    }

    #[test]
    fn rates_cells_and_thresholds_as_the_cpp() {
        assert_eq!(bits(125_000_000), 1_000_000_000); // 125 MB/s -> 1 Gbit/s (t_qos)
        assert_eq!(cells_up(1518, 224), 7); // 1518 B -> 7 cells (t_qos)
        assert_eq!(cells_up(0, 224), 0);
        assert_eq!(cells_up(224, 224), 1);
        assert_eq!(cells_up(225, 224), 2);
        // WRED: bytes -> cells down to a multiple of 8 (isai: 100000 B -> 440, 2000000 B -> 8928)
        assert_eq!((100_000 / 224) & !7, 440);
        assert_eq!((2_000_000 / 224) & !7, 8928);
        assert_eq!((3_000_000 / 224) & !7, 13392);
    }

    #[test]
    fn map_kinds_programmed_and_identity_rules() {
        assert!(programmed(QosMapKind::DscpToTc) && programmed(QosMapKind::Dot1pToTc));
        assert!(!programmed(QosMapKind::TcToQueue) && !programmed(QosMapKind::TcToPg));
        assert!(identity(&[e(0, 0), e(1, 1), e(7, 7)]));
        assert!(!identity(&[e(1, 2), e(2, 1)]));
        assert!(identity(&[]));
        let green = QosMapEntry::default();
        let yellow = QosMapEntry {
            value_color: 1,
            ..QosMapEntry::default()
        };
        assert!(all_green(&[green]) && !all_green(&[green, yellow]));
    }

    #[test]
    fn counter_ids_map_as_tl_qos() {
        // the 8 mapped queue counters (t_qos: the capability query answers 8)
        let mapped = (0..64u32)
            .filter(|s| queue_stat_id(*s as ssys::sai_stat_id_t).is_some())
            .count();
        assert_eq!(mapped, 8);
        assert_eq!(
            queue_stat_id(ssys::SAI_QUEUE_STAT_SHARED_WATERMARK_BYTES),
            Some((sys::IFCS_QUEUE_STATS_ID_TOTAL_USE_WATERMARK.cast_signed(), true))
        );
        assert_eq!(
            queue_stat_id(ssys::SAI_QUEUE_STAT_PACKETS),
            Some((sys::IFCS_QUEUE_STATS_ID_PACKET_TX_COUNT.cast_signed(), false))
        );
        assert_eq!(queue_stat_id(ssys::SAI_QUEUE_STAT_DROPPED_BYTES), None);
        assert_eq!(
            pg_stat_id(ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_DROPPED_PACKETS),
            Some((sys::IFCS_BAM_QUEUE_STATS_ID_PACKET_DROP_COUNT.cast_signed(), false))
        );
        assert_eq!(pg_stat_id(ssys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS), None);
        let a = u32s(&[(3, 1), (6, 8), (9, 4)]);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn show_lists_ports_and_queues() {
        let mut q = QosArea::default();
        q.port_key.insert(1, (Some(1), None));
        q.queue_wred.insert((1, 3), 2);
        assert_eq!(
            q.show(),
            "qos: 0 maps, 0 map groups, 1 ports mapped, 0 wred, cell 0 B\n  devport 1: dscp map 1 dot1p map -1\n  queue 1.3: wred 2\n"
        );
    }
}

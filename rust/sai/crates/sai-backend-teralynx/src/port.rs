//! Ports (`tl_port.{h,cpp}`): admin and link state (with the optional link cache), speed, FEC,
//! autoneg, loopback, MTU, PVID, learning, the 15 frontend counters, and the link scan events.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ifcs::{Attrs, sys};
use sai_core::backend::types::{Fec, Learn, Loopback, PortStat};
use sai_core::backend::{HwPort, PortInfo, Vid};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE, speed_from_enum};
use sys::{
    IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL as RX_BYTES, IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST as RX_BC,
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP as RX_DROP, IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY as RX_ERR,
    IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST as RX_MC, IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST as RX_UC,
    IFCS_DEVPORT_STATS_ID_TX_BYTES_OK as TX_BYTES, IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST as TX_BC,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP as TX_DROP, IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY as TX_ERR,
    IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST as TX_MC, IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST as TX_UC,
};

/// Devports the link cache has a slot for (`MAX_DEVPORT`).
pub const MAX_DEVPORT: u32 = 512;
const LINK_UNKNOWN: i8 = -1;
const LINK_DOWN: i8 = 0;
const LINK_UP: i8 = 1;
/// The counter cache's age limit (the frontend asks one counter at a time).
const STAT_CACHE: Duration = Duration::from_millis(200);

/// What the link cache knows per devport (written by the link scan thread too: atomics only).
#[derive(Debug)]
pub struct LinkCache(Vec<AtomicI8>);

impl LinkCache {
    fn new() -> Self {
        Self((0..MAX_DEVPORT).map(|_| AtomicI8::new(LINK_UNKNOWN)).collect())
    }

    fn slot(&self, hw: u32) -> Option<&AtomicI8> {
        usize::try_from(hw).ok().and_then(|i| self.0.get(i))
    }

    /// `PortArea::link_event`: a link scan event's state (any thread).
    pub fn event(&self, hw: u32, up: bool) {
        if let Some(s) = self.slot(hw) {
            s.store(if up { LINK_UP } else { LINK_DOWN }, Ordering::SeqCst);
        }
    }
}

#[derive(Debug)]
struct StatCache {
    when: Instant,
    v: Vec<u64>,
}

/// The port area's state.
#[derive(Debug)]
pub struct PortArea {
    link: Arc<LinkCache>,
    link_cached: bool,
    stats_by_id: bool,
    stats: Mutex<BTreeMap<u32, StatCache>>,
}

/// The IFCS counters behind the frontend's 15 `PortStat`s (`k_stat_ids`).
const STAT_IDS: [u32; 12] = [
    sys::IFCS_DEVPORT_STATS_ID_RX_BYTES_ALL,
    sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_UNICAST,
    sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_MULTICAST,
    sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_BROADCAST,
    sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_DROP,
    sys::IFCS_DEVPORT_STATS_ID_RX_FRAMES_ERR_ANY,
    sys::IFCS_DEVPORT_STATS_ID_TX_BYTES_OK,
    sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_UNICAST,
    sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_MULTICAST,
    sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_BROADCAST,
    sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_DROP,
    sys::IFCS_DEVPORT_STATS_ID_TX_FRAMES_ERR_ANY,
];

fn ids(list: &[u32]) -> Vec<i32> {
    list.iter().map(|&i| i32::try_from(i).unwrap_or(i32::MAX)).collect()
}

impl Default for PortArea {
    fn default() -> Self {
        Self::new()
    }
}

impl PortArea {
    /// Before `configure`: every devport unknown.
    pub fn new() -> Self {
        Self {
            link: Arc::new(LinkCache::new()),
            link_cached: false,
            stats_by_id: true,
            stats: Mutex::new(BTreeMap::new()),
        }
    }

    /// The cache the link scan callback updates.
    pub fn link_cache(&self) -> Arc<LinkCache> {
        Arc::clone(&self.link)
    }

    /// `PortArea::configure` (at init, every port admin-down): the link and counter modes, every
    /// port-map devport known down.
    pub fn configure(&mut self, core: &Core) {
        self.link_cached = core.settings.link_cached;
        self.stats_by_id = core.settings.stats_by_id;
        for s in &self.link.0 {
            s.store(LINK_UNKNOWN, Ordering::SeqCst);
        }
        for spec in &core.specs {
            if spec.devport < MAX_DEVPORT {
                if let Some(s) = self.link.slot(spec.devport) {
                    s.store(LINK_DOWN, Ordering::SeqCst);
                }
            }
        }
        self.stats.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// `PortArea::infos`: the port map, autoneg as the SDK has it.
    pub fn infos(core: &Core) -> Vec<PortInfo> {
        core.specs
            .iter()
            .filter_map(|s| {
                let hw = u16::try_from(s.devport).ok()?;
                let autoneg = core
                    .dp_get_u32(s.devport, sys::IFCS_DEVPORT_ATTR_AUTO_NEG)
                    .map(|an| an != 0);
                Some(PortInfo {
                    hw: HwPort(hw),
                    lanes: s.lanes.clone(),
                    speed: s.speed,
                    name: format!("dp{}", s.devport),
                    autoneg: autoneg.unwrap_or(false),
                })
            })
            .collect()
    }

    /// `PortArea::all_down`: every port-map devport admin-down, results ignored.
    pub fn all_down(core: &Core) {
        for s in &core.specs {
            let mut a = Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_ADMIN_STATE, 0);
            let _ = core.lib.ifcs_devport_attr_set(NODE, s.devport, &mut a);
        }
    }

    /// `PortArea::admin_set`: down is known down, up is read once on the next get.
    pub fn admin_set(&self, core: &Core, hw: u32, up: bool) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let a = Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_ADMIN_STATE, u32::from(up));
        core.dp_set(hw, a, "admin_state")?;
        if hw < MAX_DEVPORT {
            if let Some(s) = self.link.slot(hw) {
                s.store(if up { LINK_UNKNOWN } else { LINK_DOWN }, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    /// `PortArea::link_get`: the cache in `cached` mode when it knows, else one `LINK_STATUS`
    /// read (which settles an unknown slot unless an event came in between).
    pub fn link_get(&self, core: &Core, hw: u32) -> Result<bool> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        if self.link_cached && hw < MAX_DEVPORT {
            if let Some(l) = self.link.slot(hw).map(|s| s.load(Ordering::SeqCst)) {
                if l != LINK_UNKNOWN {
                    return Ok(l == LINK_UP);
                }
            }
        }
        let v = core.dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_LINK_STATUS)?;
        let up = v == sys::IFCS_DEVPORT_LINK_STATUS_UP;
        if hw < MAX_DEVPORT {
            if let Some(s) = self.link.slot(hw) {
                let _ = s.compare_exchange(
                    LINK_UNKNOWN,
                    if up { LINK_UP } else { LINK_DOWN },
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
        }
        Ok(up)
    }

    /// `PortArea::speed_get`: the devport's speed, the port map's when the SDK's is unknown.
    pub fn speed_get(core: &Core, hw: u32) -> Result<u32> {
        let spec = core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let e = core.dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_SPEED)?;
        let mbps = speed_from_enum(e);
        Ok(if mbps == 0 { spec.speed } else { mbps })
    }

    /// `PortArea::speed_set`: only the configured speed (the init yaml fixes it).
    pub fn speed_set(core: &Core, hw: u32, mbps: u32) -> Result<()> {
        let cur = Self::speed_get(core, hw)?;
        if mbps == cur {
            return Ok(());
        }
        log::warn(
            ssys::SAI_API_PORT,
            format_args!("devport {hw}: speed {cur} -> {mbps} not supported (set by the init config)"),
        );
        Err(Error::NotSupported)
    }

    /// `PortArea::supported_speeds`: the one it has.
    pub fn supported_speeds(core: &Core, hw: u32) -> Result<Vec<u32>> {
        Ok(vec![Self::speed_get(core, hw)?])
    }

    /// `PortArea::frame_max_set`: sysport `MTU` = bytes + 14, at most 9290.
    pub fn frame_max_set(core: &Core, hw: u32, bytes: u32) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let mut v = bytes.wrapping_add(14);
        if v > 9290 {
            log::warn(
                ssys::SAI_API_PORT,
                format_args!("devport {hw}: frame {v} B clamped to the SDK L2 MTU limit 9290"),
            );
            v = 9290;
        }
        let v = u16::try_from(v).unwrap_or(u16::MAX);
        core.sp_set(hw, Attrs::new().u16(sys::IFCS_SYSPORT_ATTR_MTU, v), "mtu")
    }

    /// `PortArea::fec_set`: RS keeps a port's KR / KP, else KP for >= 200G or >= 50G lanes, KR
    /// otherwise; FC; none.
    pub fn fec_set(core: &Core, hw: u32, f: Fec) -> Result<()> {
        let spec = core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let cur = core.dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_FEC_MODE).unwrap_or(u32::MAX);
        let lanes = u32::try_from(spec.lanes.len()).unwrap_or(1).max(1);
        let lane = spec.speed / lanes;
        let want = match f {
            Fec::None => sys::IFCS_DEVPORT_FEC_MODE_NONE,
            Fec::Fc => sys::IFCS_DEVPORT_FEC_MODE_FC,
            Fec::Rs if cur == sys::IFCS_DEVPORT_FEC_MODE_KR || cur == sys::IFCS_DEVPORT_FEC_MODE_KP => cur,
            Fec::Rs if spec.speed >= 200_000 || lane >= 50_000 => sys::IFCS_DEVPORT_FEC_MODE_KP,
            Fec::Rs => sys::IFCS_DEVPORT_FEC_MODE_KR,
        };
        if cur == want {
            return Ok(());
        }
        core.dp_set(hw, Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_FEC_MODE, want), "fec_mode")
    }

    /// `PortArea::autoneg_set`: written only when it differs.
    pub fn autoneg_set(core: &Core, hw: u32, on: bool) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        if core
            .dp_get_u32(hw, sys::IFCS_DEVPORT_ATTR_AUTO_NEG)
            .is_ok_and(|cur| (cur != 0) == on)
        {
            return Ok(());
        }
        core.dp_set(
            hw,
            Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_AUTO_NEG, u32::from(on)),
            "auto_neg",
        )
    }

    /// `PortArea::loopback_set`: MAC -> PCS, PHY -> PMA.
    pub fn loopback_set(core: &Core, hw: u32, lb: Loopback) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let v = match lb {
            Loopback::Mac => sys::IFCS_DEVPORT_LOOPBACK_PCS,
            Loopback::Phy => sys::IFCS_DEVPORT_LOOPBACK_PMA,
            Loopback::None => sys::IFCS_DEVPORT_LOOPBACK_NONE,
        };
        core.dp_set(hw, Attrs::new().u32(sys::IFCS_DEVPORT_ATTR_LOOPBACK, v), "loopback")
    }

    /// `PortArea::pvid_set`: sysport `DEFAULT_CVID`.
    pub fn pvid_set(core: &Core, hw: u32, vid: Vid) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        core.sp_set(
            hw,
            Attrs::new().u16(sys::IFCS_SYSPORT_ATTR_DEFAULT_CVID, vid.get()),
            "default_cvid",
        )
    }

    /// `PortArea::learn_set`: sysport `LEARN_ENABLE`; drop / CPU modes are `NOT_IMPLEMENTED`.
    pub fn learn_set(core: &Core, hw: u32, mode: Learn) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        if matches!(mode, Learn::Drop | Learn::Cpu) {
            return Err(core.not_impl("learning mode drop/cpu"));
        }
        core.sp_set(
            hw,
            Attrs::new().data(sys::IFCS_SYSPORT_ATTR_LEARN_ENABLE, mode == Learn::Hw),
            "learn_enable",
        )
    }

    /// `PortArea::stat_get`: one `ifcs_devport_stats_get` per port and 200 ms (the 12 mapped
    /// counters by id, or all 112 in the vendor CLI's form), then the counter from the cache.
    pub fn stat_get(&self, core: &Core, hw: u32, s: PortStat) -> Result<u64> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let all = usize::try_from(sys::IFCS_DEVPORT_STATS_ID_ALL).unwrap_or(112);
        let mut caches = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let fresh = caches
            .get(&hw)
            .is_some_and(|c| now.duration_since(c.when) <= STAT_CACHE);
        if !fresh {
            let mut v = vec![0u64; all];
            if self.stats_by_id {
                let mut want = ids(&STAT_IDS);
                let mut vals = vec![0u64; want.len()];
                let (rc, _) = core.lib.ifcs_devport_stats_get(NODE, hw, &mut want, &mut vals);
                if !rc.is_success() && rc.reason() != sys::IFCS_UNSUPPORTED {
                    return Err(core.fail(rc.reason(), "devport stats get"));
                }
                for (id, val) in STAT_IDS.iter().zip(vals) {
                    if let Some(slot) = usize::try_from(*id).ok().and_then(|i| v.get_mut(i)) {
                        *slot = val;
                    }
                }
            } else {
                let (rc, _, vals) = core.lib.ifcs_devport_stats_get_all(NODE, hw);
                if !rc.is_success() && rc.reason() != sys::IFCS_UNSUPPORTED {
                    return Err(core.fail(rc.reason(), "devport stats get"));
                }
                v = vals;
            }
            caches.insert(hw, StatCache { when: now, v });
        }
        let x = |id: u32| -> u64 {
            caches
                .get(&hw)
                .and_then(|c| usize::try_from(id).ok().and_then(|i| c.v.get(i)))
                .copied()
                .unwrap_or(0)
        };
        Ok(match s {
            PortStat::InOctets => x(RX_BYTES),
            PortStat::InUcastPkts => x(RX_UC),
            PortStat::InNonUcastPkts => x(RX_MC).wrapping_add(x(RX_BC)),
            PortStat::InDiscards => x(RX_DROP),
            PortStat::InErrors => x(RX_ERR),
            PortStat::InUnknownProtos => 0, // no such MAC counter
            PortStat::InBroadcastPkts => x(RX_BC),
            PortStat::InMulticastPkts => x(RX_MC),
            PortStat::OutOctets => x(TX_BYTES), // as isai (RX side: BYTES_ALL)
            PortStat::OutUcastPkts => x(TX_UC),
            PortStat::OutNonUcastPkts => x(TX_MC).wrapping_add(x(TX_BC)),
            PortStat::OutDiscards => x(TX_DROP),
            PortStat::OutErrors => x(TX_ERR),
            PortStat::OutBroadcastPkts => x(TX_BC),
            PortStat::OutMulticastPkts => x(TX_MC),
        })
    }

    /// `PortArea::stat_clear`: every counter (`IFCS_DEVPORT_STATS_ID_MAX_COUNT` ids).
    pub fn stat_clear(&self, core: &Core, hw: u32) -> Result<()> {
        core.front(hw).ok_or(Error::InvalidPortNumber)?;
        let mut all: Vec<i32> = (0..sys::IFCS_DEVPORT_STATS_ID_MAX_COUNT)
            .map(|i| i32::try_from(i).unwrap_or(0))
            .collect();
        let rc = core.lib.ifcs_devport_stats_clear(NODE, hw, &mut all);
        self.stats.lock().unwrap_or_else(PoisonError::into_inner).remove(&hw);
        if !rc.is_success() {
            return Err(core.fail(rc.reason(), "devport stats clear"));
        }
        Ok(())
    }
}

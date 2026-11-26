//! Host interfaces (`tl_hostif.{h,cpp}`). Ported in G1: the netdev mode (`configure`), the
//! teardown at shutdown (no netdev exists while `netif_create` is G2), the system traps as isai sets
//! them (once, before the first netdev or trap: called by the G2 operations), and the CPU queue
//! counters the diag shell's `counters` shows. Netdevs, traps, `CoPP` and `copp` are G2.
#![forbid(unsafe_code)]

use ifcs::{Attrs, Handle, sys};
use sai_core::error::Result;
use sai_core::log;
use sai_sys as ssys;

use crate::core::{Core, NODE};
use crate::keys::NetdevMode;

/// The host interface area's state.
#[derive(Debug)]
pub struct HostifArea {
    /// `SAI_TERALYNX_NETDEV`.
    pub netdev: NetdevMode,
    /// The system traps were set (`sys_traps_done_`).
    pub sys_traps_done: bool,
    /// The shared IP2ME hostif trap (`trap_`).
    pub ip2me: Handle,
    /// The glean hostif trap (`glean_`).
    pub glean: Handle,
}

impl Default for HostifArea {
    fn default() -> Self {
        Self {
            netdev: NetdevMode::Adopt,
            sys_traps_done: false,
            ip2me: Handle::NULL,
            glean: Handle::NULL,
        }
    }
}

impl HostifArea {
    /// `HostifArea::configure` (at init).
    pub const fn configure(&mut self, netdev: NetdevMode) {
        self.netdev = netdev;
    }

    /// `HostifArea::teardown_all`: every netdev deleted / given back (none in G1), the traps
    /// forgotten (their IFCS objects go with `ifcs_deinit`).
    pub const fn teardown_all(&mut self) {
        self.ip2me = Handle::NULL;
        self.glean = Handle::NULL;
        self.sys_traps_done = false;
    }

    /// `HostifArea::system_traps_init`: isai 6.2.0's `create_switch` calls, in its order: system
    /// trap 23 (L3 MTU) queue 0 + enable, then disable; 17 (TTL) queue 0 + enable, enable; 16 (L2
    /// source miss / move) queue 0 + enable, then disable; 20 (IPv4 options) enable. Once;
    /// `SAI_TERALYNX_SYSTEM_TRAPS=sdk` leaves the SDK's.
    pub fn system_traps_init(&mut self, core: &Core, initialized: bool) {
        if self.sys_traps_done || !initialized {
            return;
        }
        self.sys_traps_done = true;
        if core.settings.system_traps_sdk {
            core.log_once(
                "systraps",
                format_args!("SAI_TERALYNX_SYSTEM_TRAPS=sdk: system traps left as the SDK has them"),
            );
            return;
        }
        let steps = [
            (sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, true, true),
            (sys::IFCS_HOSTIF_TRAP_L3MTU_CHECK_FAIL, false, false),
            (sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, true, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L3_TTL_CHECK_FAIL, false, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, true, true),
            (sys::IFCS_HOSTIF_TRAP_FWD_L2_SRC_MISS_MOVE, false, false),
            (sys::IFCS_HOSTIF_TRAP_FWD_IPV4_OPTIONS_DETECTED, false, true),
        ];
        for (reason, queue, enable) in steps {
            let mut a = Attrs::new();
            if queue {
                a = a.u32(sys::IFCS_HOSTIF_TRAP_ATTR_QUEUE_NUM, 0);
            }
            a = a.data(sys::IFCS_HOSTIF_TRAP_ATTR_ENABLE, enable);
            let _ = core.call(
                core.lib
                    .ifcs_hostif_trap_attr_set(NODE, Handle::hostif_trap(reason).0, &mut a),
                "hostif_trap_attr_set (system trap)",
            );
        }
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!("system traps as isai: TTL and IPv4 options to the CPU, L2 source miss/move and L3 MTU off"),
        );
    }
}

/// `HostifArea::cpu_queue_stats`: frames and bytes CPU queue `q` sent to the CPU, frames it
/// dropped (no log on failure, as the C++).
pub fn cpu_queue_stats(core: &Core, q: u32) -> Result<(u64, u64, u64)> {
    let mut ids = [
        sys::IFCS_CPU_QUEUE_STATS_ID_PACKET_TX_COUNT,
        sys::IFCS_CPU_QUEUE_STATS_ID_BYTE_TX_COUNT,
        sys::IFCS_CPU_QUEUE_STATS_ID_PACKET_DROP_COUNT,
    ]
    .map(|i| i32::try_from(i).unwrap_or(i32::MAX));
    let mut v = [0u64; 3];
    let (rc, _) = core.lib.ifcs_cpu_queue_stats_get(NODE, q, &mut ids, &mut v);
    rc.ok()?;
    Ok((v[0], v[1], v[2]))
}

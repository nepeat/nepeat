//! One interface, two implementations: every operation of `sdk_glue.h` as a method of [`Glue`],
//! which calls the real glue ([`crate::real`], the FFI) or the pure-Rust fake
//! ([`crate::rust_fake::Chip`]). The handles of this crate hold a `Glue` and know nothing else
//! about which one it is. Arguments are the C ones with buffers as slices (their lengths are
//! what is passed), results the C return code plus out-parameters.

use std::ffi::{CStr, c_int, c_uint};
use std::os::fd::BorrowedFd;

use sdk_glue_sys as sys;

#[cfg(not(ob_glue_rust_only))]
use crate::real;

/// Which glue a handle drives.
#[derive(Clone, Debug)]
pub(crate) enum Glue {
    /// `sdk_glue.c` (or the C fake) through `sdk-glue-sys`: whatever this program links.
    #[cfg(not(ob_glue_rust_only))]
    Real,
    /// The Rust fake ([`crate::rust_fake`]).
    #[cfg(feature = "rust-fake")]
    Fake(crate::rust_fake::Chip),
}

macro_rules! ops {
    ($( fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty; )*) => {
        impl Glue {
            $(
                #[allow(clippy::too_many_arguments, reason = "the glue's signature")]
                pub(crate) fn $name(&self, $($arg: $ty),*) -> $ret {
                    match self {
                        #[cfg(not(ob_glue_rust_only))]
                        Self::Real => real::$name($($arg),*),
                        #[cfg(feature = "rust-fake")]
                        Self::Fake(chip) => chip.$name($($arg),*),
                    }
                }
            )*
        }
    };
}

ops! {
    fn init(unit: c_int, config: &CStr, rc: &CStr, err: &mut [u8]) -> c_int;
    fn attach(unit: c_int, err: &mut [u8]) -> c_int;
    fn init_mdio(unit: c_int, config: &CStr, err: &mut [u8]) -> c_int;
    fn temperatures(unit: c_int, cur: &mut [c_int], peak: &mut [c_int], count: &mut c_int) -> c_int;
    fn th_overtemp_shutdown(unit: c_int) -> c_int;
    fn mdio_read(unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: &mut c_uint) -> c_int;
    fn mdio_write(unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: c_uint) -> c_int;
    fn linkscan_port_gate(unit: c_int, port: c_int, on: bool, key: usize) -> c_int;
    fn shutdown(unit: c_int) -> ();
    fn shell(unit: c_int, cmd: &CStr) -> c_int;
    fn shell_to(unit: c_int, cmd: &CStr, fd: BorrowedFd<'_>) -> c_int;
    fn register_callbacks(unit: c_int, link: bool, l2: bool, key: usize) -> c_int;
    fn unregister_callbacks(unit: c_int) -> ();
    fn hw_info(unit: c_int, buf: &mut [u8]) -> c_int;
    fn knet_cleanup(unit: c_int, netifs: &mut c_int, filters: &mut c_int) -> c_int;
    fn table_usage(unit: c_int, u: &mut sys::ob_table_usage_t) -> c_int;
    fn chip_limits(unit: c_int, l: &mut sys::ob_chip_limits_t) -> c_int;
    fn cpu_port(unit: c_int) -> c_int;
    fn ports(unit: c_int, out: &mut [sys::ob_port_info_t]) -> c_int;
    fn port_enable_set(unit: c_int, port: c_int, enable: c_int) -> c_int;
    fn port_link_get(unit: c_int, port: c_int, up: &mut c_int) -> c_int;
    fn port_speed_set(unit: c_int, port: c_int, mbps: c_int) -> c_int;
    fn port_speed_get(unit: c_int, port: c_int, mbps: &mut c_int) -> c_int;
    fn port_speeds(unit: c_int, port: c_int, out: &mut [u32]) -> c_int;
    fn port_frame_max_set(unit: c_int, port: c_int, bytes: c_int) -> c_int;
    fn port_fec_set(unit: c_int, port: c_int, mode: c_int) -> c_int;
    fn port_autoneg_set(unit: c_int, port: c_int, on: c_int) -> c_int;
    fn port_autoneg_get(unit: c_int, port: c_int, on: &mut c_int) -> c_int;
    fn port_interface_set(unit: c_int, port: c_int, name: &CStr) -> c_int;
    fn port_ifg_set(unit: c_int, port: c_int, bits: c_int) -> c_int;
    fn port_loopback_set(unit: c_int, port: c_int, mode: c_int) -> c_int;
    fn port_pvid_set(unit: c_int, port: c_int, vid: c_int) -> c_int;
    fn port_learn_set(unit: c_int, port: c_int, mode: c_int) -> c_int;
    fn port_stat_get(unit: c_int, port: c_int, which: c_int, v: &mut u64) -> c_int;
    fn port_stat_clear(unit: c_int, port: c_int) -> c_int;
    fn vlan_create(unit: c_int, vid: c_int) -> c_int;
    fn vlan_destroy(unit: c_int, vid: c_int) -> c_int;
    fn vlan_port_add(unit: c_int, vid: c_int, port: c_int, untagged: c_int) -> c_int;
    fn vlan_port_remove(unit: c_int, vid: c_int, port: c_int) -> c_int;
    fn vlan_members(unit: c_int, vid: c_int, ports: &mut [c_int], untagged: &mut [c_int]) -> c_int;
    fn l2_add(unit: c_int, mac: &[u8; 6], vid: c_int, port: c_int, is_static: c_int, discard: c_int) -> c_int;
    fn l2_delete(unit: c_int, mac: &[u8; 6], vid: c_int) -> c_int;
    fn l2_flush(unit: c_int, port: c_int, vid: c_int, statics: c_int, dynamics: c_int) -> c_int;
    fn l2_age_set(unit: c_int, seconds: c_int) -> c_int;
    fn netif_create(
        unit: c_int,
        name: &CStr,
        port: c_int,
        vid: c_int,
        mac: &[u8; 6],
        keep_tag: c_int,
        netif_id: &mut c_int,
        filter_id: &mut c_int,
    ) -> c_int;
    fn netif_destroy(unit: c_int, netif_id: c_int, filter_id: c_int) -> c_int;
    fn netif_filter_set(unit: c_int, netif_id: c_int, port: c_int, keep_tag: c_int, filter_id: &mut c_int) -> c_int;
    fn trap_set(unit: c_int, kind: c_int, to_cpu: c_int) -> c_int;
    fn trunk_create(unit: c_int, tid: &mut c_int) -> c_int;
    fn trunk_destroy(unit: c_int, tid: c_int) -> c_int;
    fn trunk_set(unit: c_int, tid: c_int, ports: &[c_int], flags: &[c_int]) -> c_int;
    fn l3_intf_create(unit: c_int, mac: &[u8; 6], vid: c_int, mtu: c_int, vrf: c_int, id: &mut c_int) -> c_int;
    fn l3_intf_update(unit: c_int, id: c_int, mac: &[u8; 6], vid: c_int, mtu: c_int, vrf: c_int) -> c_int;
    fn l3_intf_destroy(unit: c_int, id: c_int) -> c_int;
    fn l3_station_add(unit: c_int, mac: &[u8; 6], id: &mut c_int) -> c_int;
    fn l3_station_delete(unit: c_int, id: c_int) -> c_int;
    fn l3_egress_create(unit: c_int, intf: c_int, mac: &[u8; 6], port: c_int, vid: c_int, id: &mut c_int) -> c_int;
    fn l3_egress_update(unit: c_int, id: c_int, intf: c_int, mac: &[u8; 6], port: c_int, vid: c_int) -> c_int;
    fn l3_egress_special(unit: c_int, drop: c_int, id: &mut c_int) -> c_int;
    fn l3_egress_destroy(unit: c_int, id: c_int) -> c_int;
    fn l3_host_add(unit: c_int, vrf: c_int, addr: &[u8], egress: c_int) -> c_int;
    fn l3_host_delete(unit: c_int, vrf: c_int, addr: &[u8]) -> c_int;
    fn l3_route_add(
        unit: c_int,
        vrf: c_int,
        addr: &[u8],
        mask: &[u8],
        egress: c_int,
        ecmp: c_int,
        drop: c_int,
        replace: c_int,
    ) -> c_int;
    fn l3_route_delete(unit: c_int, vrf: c_int, addr: &[u8], mask: &[u8]) -> c_int;
    fn l3_ecmp_set(unit: c_int, id: &mut c_int, replace: c_int, egress: &[c_int]) -> c_int;
    fn l3_ecmp_destroy(unit: c_int, id: c_int) -> c_int;
    fn fp_group_create(unit: c_int, qset: u32, priority: c_int, gid: &mut c_int) -> c_int;
    fn fp_group_destroy(unit: c_int, gid: c_int) -> c_int;
    fn fp_entry_create(
        unit: c_int,
        gid: c_int,
        rule: &crate::field::RawRule<'_>,
        eid: &mut c_int,
        stat: &mut c_int,
    ) -> c_int;
    fn fp_entry_destroy(unit: c_int, eid: c_int, stat: c_int) -> c_int;
    fn fp_stat_get(unit: c_int, stat: c_int, packets: &mut u64, bytes: &mut u64) -> c_int;
    fn policer_create(unit: c_int, cfg: &sys::ob_policer_cfg_t, pid: &mut c_int) -> c_int;
    fn policer_set(unit: c_int, pid: c_int, cfg: &sys::ob_policer_cfg_t) -> c_int;
    fn policer_destroy(unit: c_int, pid: c_int) -> c_int;
}

impl Glue {
    /// What a bring-up uses when the caller names no fake: the glue this program links, or,
    /// in a build that links none and has the feature `rust-fake`, the process's Rust fake.
    pub(crate) fn process() -> Self {
        #[cfg(not(ob_glue_rust_only))]
        {
            Self::Real
        }
        #[cfg(ob_glue_rust_only)]
        {
            Self::Fake(crate::rust_fake::process_chip().clone())
        }
    }

    /// The process-wide "a handle is alive" flag of this glue: one for the real glue (its state
    /// is the process's), one per fake chip.
    pub(crate) fn open_flag(&self) -> &std::sync::atomic::AtomicBool {
        match self {
            #[cfg(not(ob_glue_rust_only))]
            Self::Real => &crate::unit::REAL_OPEN,
            #[cfg(feature = "rust-fake")]
            Self::Fake(chip) => chip.open_flag(),
        }
    }

    /// Whether this is the real glue linked with `sdk_glue.c` (not the C fake, not the Rust fake):
    /// only then do the process-boot rules of the real glue apply (`crate::unit`).
    pub(crate) fn is_real_sdk(&self) -> bool {
        match self {
            #[cfg(not(ob_glue_rust_only))]
            Self::Real => sys::LINKED == "real",
            #[cfg(feature = "rust-fake")]
            Self::Fake(_) => false,
        }
    }
}

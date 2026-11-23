//! The real glue: every call of a function of `sdk_glue.h` (`sdk-glue-sys`), the only FFI of this
//! crate. One function per glue function, each with at most one `unsafe` block and its
//! `// SAFETY:` comment; a function the header lets `sdk-glue-sys` declare `safe fn` (its rule: all
//! scalars, ranges checked by the C) is called without one.
//!
//! What every call here may assume is the crate's invariants (lib.rs, "Invariants"): [I1] a
//! bring-up runs alone, [I2] the other calls come from a live owner or handle, the ones on glue
//! state without a lock with `&mut` of the owner, [I3] the callback pointers are this crate's
//! trampolines and the context is a key that is never dereferenced. The slices give the
//! lengths passed with their pointers, so no call here can make the C write out of bounds.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::os::fd::{AsRawFd, BorrowedFd};

use sdk_glue_sys as sys;

use crate::link;

/// A slice's length as the glue's `int` (saturating: the glue writes at most that many).
fn len(n: usize) -> c_int {
    c_int::try_from(n).unwrap_or(c_int::MAX)
}

/// The registration key as the glue's `void* ctx`: an address-sized number, no provenance, never
/// dereferenced (the trampolines compare it).
fn ctx(key: usize) -> *mut c_void {
    std::ptr::without_provenance_mut(key)
}

pub(crate) fn init(unit: c_int, config: &CStr, rc: &CStr, err: &mut [u8]) -> c_int {
    let buf: *mut c_char = err.as_mut_ptr().cast();
    // SAFETY: config and rc are NUL-terminated and live across the call, only read; buf points
    // to err.len() writable bytes the glue writes at most len(err.len()) of (snprintf) and keeps
    // none of the pointers. [I1]: no other glue call runs on any thread.
    unsafe { sys::ob_sdk_init(unit, config.as_ptr(), rc.as_ptr(), buf, len(err.len())) }
}

pub(crate) fn attach(unit: c_int, err: &mut [u8]) -> c_int {
    let buf: *mut c_char = err.as_mut_ptr().cast();
    // SAFETY: buf points to err.len() writable bytes (snprintf with that length), not kept. [I1].
    unsafe { sys::ob_sdk_attach(unit, buf, len(err.len())) }
}

pub(crate) fn init_mdio(unit: c_int, config: &CStr, err: &mut [u8]) -> c_int {
    let buf: *mut c_char = err.as_mut_ptr().cast();
    // SAFETY: config NUL-terminated, live, only read; buf as for init. [I1].
    unsafe { sys::ob_sdk_init_mdio(unit, config.as_ptr(), buf, len(err.len())) }
}

pub(crate) fn temperatures(unit: c_int, cur: &mut [c_int], peak: &mut [c_int], count: &mut c_int) -> c_int {
    let max = len(cur.len().min(peak.len()));
    // SAFETY: cur and peak hold at least max ints each and the glue writes at most max to each,
    // one int to count; it keeps no pointer. It reads only flags a bring-up writes ([I1]).
    unsafe { sys::ob_sdk_temperatures(unit, cur.as_mut_ptr(), peak.as_mut_ptr(), max, count) }
}

pub(crate) fn th_overtemp_shutdown(unit: c_int) -> c_int {
    sys::ob_sdk_th_overtemp_shutdown(unit)
}

pub(crate) fn mdio_read(unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: &mut c_uint) -> c_int {
    // SAFETY: val is a live unsigned the glue writes once and does not keep; the rest are
    // integers. Bring-up flags: [I1].
    unsafe { sys::ob_sdk_mdio_c45_read(unit, phy, dev, reg, val) }
}

pub(crate) fn mdio_write(unit: c_int, phy: c_uint, dev: c_uint, reg: c_uint, val: c_uint) -> c_int {
    sys::ob_sdk_mdio_c45_write(unit, phy, dev, reg, val)
}

pub(crate) fn linkscan_port_gate(unit: c_int, port: c_int, on: bool, key: usize) -> c_int {
    let gate: sys::ob_gate_cb_t = if on { Some(link::on_gate) } else { None };
    // SAFETY: [I3]: gate is a function of this crate with the signature of ob_gate_cb_t, valid
    // for the life of the process, callable from any thread with any arguments, never
    // unwinding; ctx is a key, never dereferenced. The glue stores both in globals that only
    // this call writes, and [I2] serialises it (&mut of the switch).
    unsafe { sys::ob_sdk_linkscan_port_gate(unit, port, gate, ctx(key)) }
}

pub(crate) fn shutdown(unit: c_int) {
    // SAFETY: [I2]: called once, by the owner of the BCM layer as it is dropped (&mut); it
    // clears the glue's callback pointers, which only register / unregister (same owner) write.
    unsafe { sys::ob_sdk_shutdown(unit) }
}

pub(crate) fn shell(unit: c_int, cmd: &CStr) -> c_int {
    // SAFETY: cmd NUL-terminated, live across the call, only read (copied into a buffer).
    unsafe { sys::ob_sdk_shell(unit, cmd.as_ptr()) }
}

pub(crate) fn shell_to(unit: c_int, cmd: &CStr, fd: BorrowedFd<'_>) -> c_int {
    // SAFETY: cmd as for shell. fd is open for the whole call (BorrowedFd); the glue dup2()s it
    // onto the process's fd 1 for the command and restores fd 1 before returning, keeping
    // nothing. ([I2]: one shell command at a time, &mut of the switch.)
    unsafe { sys::ob_sdk_shell_to(unit, cmd.as_ptr(), fd.as_raw_fd()) }
}

pub(crate) fn register_callbacks(unit: c_int, link: bool, l2: bool, key: usize) -> c_int {
    let on_link: sys::ob_link_cb_t = if link { Some(link::on_link) } else { None };
    let on_l2: sys::ob_l2_cb_t = if l2 { Some(link::on_l2) } else { None };
    // SAFETY: [I3]: both are functions of this crate with the C signatures, valid for the life
    // of the process, callable from any thread with any arguments (on_l2 checks its pointer),
    // never unwinding; ctx a key. The glue checks each pointer for NULL before a call. [I2].
    unsafe { sys::ob_sdk_register_callbacks(unit, on_link, on_l2, ctx(key)) }
}

pub(crate) fn unregister_callbacks(unit: c_int) {
    // SAFETY: [I2]: only the owner of the registration calls it (&mut).
    unsafe { sys::ob_sdk_unregister_callbacks(unit) }
}

pub(crate) fn hw_info(unit: c_int, buf: &mut [u8]) -> c_int {
    let ptr: *mut c_char = buf.as_mut_ptr().cast();
    // SAFETY: ptr points to buf.len() writable bytes; the glue writes at most that many
    // (snprintf with len) and does not keep the pointer.
    unsafe { sys::ob_sdk_hw_info(unit, ptr, len(buf.len())) }
}

pub(crate) fn knet_cleanup(unit: c_int, netifs: &mut c_int, filters: &mut c_int) -> c_int {
    // SAFETY: two live ints the glue writes and does not keep. [I2] (&mut).
    unsafe { sys::ob_sdk_knet_cleanup(unit, netifs, filters) }
}

pub(crate) fn table_usage(unit: c_int, u: &mut sys::ob_table_usage_t) -> c_int {
    // SAFETY: u is a live ob_table_usage_t (layout checked against the header for this target
    // by sdk-glue-sys); the glue writes it whole (memset, then fields) and keeps no pointer.
    unsafe { sys::ob_sdk_table_usage(unit, u) }
}

pub(crate) fn chip_limits(unit: c_int, l: &mut sys::ob_chip_limits_t) -> c_int {
    // SAFETY: as table_usage, for ob_chip_limits_t.
    unsafe { sys::ob_sdk_chip_limits(unit, l) }
}

pub(crate) fn cpu_port(unit: c_int) -> c_int {
    sys::ob_sdk_cpu_port(unit)
}

pub(crate) fn ports(unit: c_int, out: &mut [sys::ob_port_info_t]) -> c_int {
    // SAFETY: out holds out.len() initialised ob_port_info_t (layout checked); the glue writes
    // at most len(out.len()) of them and keeps no pointer.
    unsafe { sys::ob_sdk_ports(unit, out.as_mut_ptr(), len(out.len())) }
}

pub(crate) fn port_enable_set(unit: c_int, port: c_int, enable: c_int) -> c_int {
    sys::ob_sdk_port_enable_set(unit, port, enable)
}

pub(crate) fn port_link_get(unit: c_int, port: c_int, up: &mut c_int) -> c_int {
    // SAFETY: up is a live int the glue writes once, not kept.
    unsafe { sys::ob_sdk_port_link_get(unit, port, up) }
}

pub(crate) fn port_speed_set(unit: c_int, port: c_int, mbps: c_int) -> c_int {
    sys::ob_sdk_port_speed_set(unit, port, mbps)
}

pub(crate) fn port_speed_get(unit: c_int, port: c_int, mbps: &mut c_int) -> c_int {
    // SAFETY: mbps is a live int the glue writes once, not kept.
    unsafe { sys::ob_sdk_port_speed_get(unit, port, mbps) }
}

pub(crate) fn port_speeds(unit: c_int, port: c_int, out: &mut [u32]) -> c_int {
    // SAFETY: out holds out.len() u32 (uint32_t); the glue writes at most that many, not kept.
    unsafe { sys::ob_sdk_port_speeds(unit, port, out.as_mut_ptr(), len(out.len())) }
}

pub(crate) fn port_frame_max_set(unit: c_int, port: c_int, bytes: c_int) -> c_int {
    sys::ob_sdk_port_frame_max_set(unit, port, bytes)
}

pub(crate) fn port_fec_set(unit: c_int, port: c_int, mode: c_int) -> c_int {
    sys::ob_sdk_port_fec_set(unit, port, mode)
}

pub(crate) fn port_autoneg_set(unit: c_int, port: c_int, on: c_int) -> c_int {
    sys::ob_sdk_port_autoneg_set(unit, port, on)
}

pub(crate) fn port_autoneg_get(unit: c_int, port: c_int, on: &mut c_int) -> c_int {
    // SAFETY: on is a live int the glue (the SDK) writes once, not kept.
    unsafe { sys::ob_sdk_port_autoneg_get(unit, port, on) }
}

pub(crate) fn port_interface_set(unit: c_int, port: c_int, name: &CStr) -> c_int {
    // SAFETY: name NUL-terminated, live, only compared (strcmp), not kept.
    unsafe { sys::ob_sdk_port_interface_set(unit, port, name.as_ptr()) }
}

pub(crate) fn port_ifg_set(unit: c_int, port: c_int, bits: c_int) -> c_int {
    sys::ob_sdk_port_ifg_set(unit, port, bits)
}

pub(crate) fn port_loopback_set(unit: c_int, port: c_int, mode: c_int) -> c_int {
    sys::ob_sdk_port_loopback_set(unit, port, mode)
}

pub(crate) fn port_pvid_set(unit: c_int, port: c_int, vid: c_int) -> c_int {
    sys::ob_sdk_port_pvid_set(unit, port, vid)
}

pub(crate) fn port_learn_set(unit: c_int, port: c_int, mode: c_int) -> c_int {
    sys::ob_sdk_port_learn_set(unit, port, mode)
}

pub(crate) fn port_stat_get(unit: c_int, port: c_int, which: c_int, v: &mut u64) -> c_int {
    // SAFETY: v is a live u64 the glue writes once, not kept; which is range-checked by the glue.
    unsafe { sys::ob_sdk_port_stat_get(unit, port, which, v) }
}

pub(crate) fn port_stat_clear(unit: c_int, port: c_int) -> c_int {
    sys::ob_sdk_port_stat_clear(unit, port)
}

pub(crate) fn vlan_create(unit: c_int, vid: c_int) -> c_int {
    sys::ob_sdk_vlan_create(unit, vid)
}

pub(crate) fn vlan_destroy(unit: c_int, vid: c_int) -> c_int {
    sys::ob_sdk_vlan_destroy(unit, vid)
}

pub(crate) fn vlan_port_add(unit: c_int, vid: c_int, port: c_int, untagged: c_int) -> c_int {
    sys::ob_sdk_vlan_port_add(unit, vid, port, untagged)
}

pub(crate) fn vlan_port_remove(unit: c_int, vid: c_int, port: c_int) -> c_int {
    sys::ob_sdk_vlan_port_remove(unit, vid, port)
}

pub(crate) fn vlan_members(unit: c_int, vid: c_int, ports: &mut [c_int], untagged: &mut [c_int]) -> c_int {
    let max = len(ports.len().min(untagged.len()));
    // SAFETY: both slices hold at least max ints; the glue writes at most max to each, not kept.
    unsafe { sys::ob_sdk_vlan_members(unit, vid, ports.as_mut_ptr(), untagged.as_mut_ptr(), max) }
}

pub(crate) fn l2_add(unit: c_int, mac: &[u8; 6], vid: c_int, port: c_int, is_static: c_int, discard: c_int) -> c_int {
    // SAFETY: mac points to 6 readable bytes (the uint8_t[6] of the header), copied, not kept.
    unsafe { sys::ob_sdk_l2_add(unit, mac.as_ptr(), vid, port, is_static, discard) }
}

pub(crate) fn l2_delete(unit: c_int, mac: &[u8; 6], vid: c_int) -> c_int {
    // SAFETY: as l2_add.
    unsafe { sys::ob_sdk_l2_delete(unit, mac.as_ptr(), vid) }
}

pub(crate) fn l2_flush(unit: c_int, port: c_int, vid: c_int, statics: c_int, dynamics: c_int) -> c_int {
    // SAFETY: integers; it reads the glue's trunk bitmap, which trunk create / destroy write
    // without a lock: [I2] serialises them (&mut of the switch). Port and vid were checked by the
    // caller (crate::l2), the glue does not.
    unsafe { sys::ob_sdk_l2_flush(unit, port, vid, statics, dynamics) }
}

pub(crate) fn l2_age_set(unit: c_int, seconds: c_int) -> c_int {
    sys::ob_sdk_l2_age_set(unit, seconds)
}

#[allow(clippy::too_many_arguments, reason = "the glue's signature")]
pub(crate) fn netif_create(
    unit: c_int,
    name: &CStr,
    port: c_int,
    vid: c_int,
    mac: &[u8; 6],
    keep_tag: c_int,
    netif_id: &mut c_int,
    filter_id: &mut c_int,
) -> c_int {
    // SAFETY: name NUL-terminated (copied, snprintf into the SDK's name field); mac 6 readable
    // bytes (copied); netif_id / filter_id live ints the glue writes; nothing is kept.
    unsafe {
        sys::ob_sdk_netif_create(
            unit,
            name.as_ptr(),
            port,
            vid,
            mac.as_ptr(),
            keep_tag,
            netif_id,
            filter_id,
        )
    }
}

pub(crate) fn netif_destroy(unit: c_int, netif_id: c_int, filter_id: c_int) -> c_int {
    sys::ob_sdk_netif_destroy(unit, netif_id, filter_id)
}

pub(crate) fn netif_filter_set(
    unit: c_int,
    netif_id: c_int,
    port: c_int,
    keep_tag: c_int,
    filter_id: &mut c_int,
) -> c_int {
    // SAFETY: filter_id is a live int the glue reads (the old filter) and writes (the new one).
    unsafe { sys::ob_sdk_netif_filter_set(unit, netif_id, port, keep_tag, filter_id) }
}

pub(crate) fn trap_set(unit: c_int, kind: c_int, to_cpu: c_int) -> c_int {
    // SAFETY: integers; the glue's cached L2 user entry indices are read and written without a
    // lock: [I2] serialises the calls (&mut of the switch). kind out of range: OB_E_UNAVAIL.
    unsafe { sys::ob_sdk_trap_set(unit, kind, to_cpu) }
}

pub(crate) fn trunk_create(unit: c_int, tid: &mut c_int) -> c_int {
    // SAFETY: tid is a live int the glue writes; the trunk bitmap: [I2] (&mut).
    unsafe { sys::ob_sdk_trunk_create(unit, tid) }
}

pub(crate) fn trunk_destroy(unit: c_int, tid: c_int) -> c_int {
    // SAFETY: integers; the trunk bitmap is written without a lock: [I2] (&mut).
    unsafe { sys::ob_sdk_trunk_destroy(unit, tid) }
}

pub(crate) fn trunk_set(unit: c_int, tid: c_int, ports: &[c_int], flags: &[c_int]) -> c_int {
    let n = len(ports.len().min(flags.len()));
    // SAFETY: both slices hold at least n ints, read only, not kept; the glue refuses n > 64.
    unsafe { sys::ob_sdk_trunk_set(unit, tid, ports.as_ptr(), flags.as_ptr(), n) }
}

pub(crate) fn l3_intf_create(unit: c_int, mac: &[u8; 6], vid: c_int, mtu: c_int, vrf: c_int, id: &mut c_int) -> c_int {
    // SAFETY: mac 6 readable bytes (copied); id a live int written once; nothing kept.
    unsafe { sys::ob_sdk_l3_intf_create(unit, mac.as_ptr(), vid, mtu, vrf, id) }
}

pub(crate) fn l3_intf_update(unit: c_int, id: c_int, mac: &[u8; 6], vid: c_int, mtu: c_int, vrf: c_int) -> c_int {
    // SAFETY: mac 6 readable bytes (copied), not kept.
    unsafe { sys::ob_sdk_l3_intf_update(unit, id, mac.as_ptr(), vid, mtu, vrf) }
}

pub(crate) fn l3_intf_destroy(unit: c_int, id: c_int) -> c_int {
    sys::ob_sdk_l3_intf_destroy(unit, id)
}

pub(crate) fn l3_station_add(unit: c_int, mac: &[u8; 6], id: &mut c_int) -> c_int {
    // SAFETY: mac 6 readable bytes (copied); id a live int the SDK writes; nothing kept.
    unsafe { sys::ob_sdk_l3_station_add(unit, mac.as_ptr(), id) }
}

pub(crate) fn l3_station_delete(unit: c_int, id: c_int) -> c_int {
    sys::ob_sdk_l3_station_delete(unit, id)
}

pub(crate) fn l3_egress_create(
    unit: c_int,
    intf: c_int,
    mac: &[u8; 6],
    port: c_int,
    vid: c_int,
    id: &mut c_int,
) -> c_int {
    // SAFETY: mac 6 readable bytes (copied); id a live int written once; nothing kept.
    unsafe { sys::ob_sdk_l3_egress_create(unit, intf, mac.as_ptr(), port, vid, id) }
}

pub(crate) fn l3_egress_update(unit: c_int, id: c_int, intf: c_int, mac: &[u8; 6], port: c_int, vid: c_int) -> c_int {
    // SAFETY: mac 6 readable bytes (copied), not kept.
    unsafe { sys::ob_sdk_l3_egress_update(unit, id, intf, mac.as_ptr(), port, vid) }
}

pub(crate) fn l3_egress_special(unit: c_int, drop: c_int, id: &mut c_int) -> c_int {
    // SAFETY: id a live int written once; the glue's cache of the two objects is written
    // without a lock: [I2] (&mut).
    unsafe { sys::ob_sdk_l3_egress_special(unit, drop, id) }
}

pub(crate) fn l3_egress_destroy(unit: c_int, id: c_int) -> c_int {
    sys::ob_sdk_l3_egress_destroy(unit, id)
}

/// The glue reads 16 address bytes with `v6`, else 4: the callers pass exactly that many.
pub(crate) fn l3_host_add(unit: c_int, vrf: c_int, addr: &[u8], egress: c_int) -> c_int {
    let v6 = c_int::from(addr.len() == 16);
    if addr.len() != 4 && addr.len() != 16 {
        return sys::OB_E_PARAM;
    }
    // SAFETY: addr holds 16 bytes when v6 and 4 otherwise (checked above): what the glue reads
    // (memcpy 16 / four bytes); nothing kept.
    unsafe { sys::ob_sdk_l3_host_add(unit, vrf, v6, addr.as_ptr(), egress) }
}

pub(crate) fn l3_host_delete(unit: c_int, vrf: c_int, addr: &[u8]) -> c_int {
    let v6 = c_int::from(addr.len() == 16);
    if addr.len() != 4 && addr.len() != 16 {
        return sys::OB_E_PARAM;
    }
    // SAFETY: as l3_host_add.
    unsafe { sys::ob_sdk_l3_host_delete(unit, vrf, v6, addr.as_ptr()) }
}

#[allow(clippy::too_many_arguments, reason = "the glue's signature")]
pub(crate) fn l3_route_add(
    unit: c_int,
    vrf: c_int,
    addr: &[u8],
    mask: &[u8],
    egress: c_int,
    ecmp: c_int,
    drop: c_int,
    replace: c_int,
) -> c_int {
    let v6 = c_int::from(addr.len() == 16);
    if (addr.len() != 4 && addr.len() != 16) || mask.len() != addr.len() {
        return sys::OB_E_PARAM;
    }
    // SAFETY: addr and mask hold 16 bytes each when v6, else 4 (checked above), read only.
    unsafe { sys::ob_sdk_l3_route_add(unit, vrf, v6, addr.as_ptr(), mask.as_ptr(), egress, ecmp, drop, replace) }
}

pub(crate) fn l3_route_delete(unit: c_int, vrf: c_int, addr: &[u8], mask: &[u8]) -> c_int {
    let v6 = c_int::from(addr.len() == 16);
    if (addr.len() != 4 && addr.len() != 16) || mask.len() != addr.len() {
        return sys::OB_E_PARAM;
    }
    // SAFETY: as l3_route_add.
    unsafe { sys::ob_sdk_l3_route_delete(unit, vrf, v6, addr.as_ptr(), mask.as_ptr()) }
}

pub(crate) fn l3_ecmp_set(unit: c_int, id: &mut c_int, replace: c_int, egress: &[c_int]) -> c_int {
    // SAFETY: id a live int (read with replace, written on success); egress holds len(egress)
    // ints, read only (the glue refuses more than 256); nothing kept.
    unsafe { sys::ob_sdk_l3_ecmp_set(unit, id, replace, egress.as_ptr(), len(egress.len())) }
}

pub(crate) fn l3_ecmp_destroy(unit: c_int, id: c_int) -> c_int {
    sys::ob_sdk_l3_ecmp_destroy(unit, id)
}

pub(crate) fn fp_group_create(unit: c_int, qset: u32, priority: c_int, gid: &mut c_int) -> c_int {
    // SAFETY: gid a live int written once, not kept.
    unsafe { sys::ob_sdk_fp_group_create(unit, qset, priority, gid) }
}

pub(crate) fn fp_group_destroy(unit: c_int, gid: c_int) -> c_int {
    sys::ob_sdk_fp_group_destroy(unit, gid)
}

pub(crate) fn fp_entry_create(
    unit: c_int,
    gid: c_int,
    rule: &crate::field::RawRule<'_>,
    eid: &mut c_int,
    stat: &mut c_int,
) -> c_int {
    // SAFETY: rule.raw() is a live ob_fp_rule_t (layout checked) whose ports pointer and
    // nports describe rule's borrowed slice, alive for the call (RawRule's lifetime); the glue
    // reads them and keeps nothing. eid / stat live ints written by the glue.
    unsafe { sys::ob_sdk_fp_entry_create(unit, gid, rule.raw(), eid, stat) }
}

pub(crate) fn fp_entry_destroy(unit: c_int, eid: c_int, stat: c_int) -> c_int {
    sys::ob_sdk_fp_entry_destroy(unit, eid, stat)
}

pub(crate) fn fp_stat_get(unit: c_int, stat: c_int, packets: &mut u64, bytes: &mut u64) -> c_int {
    // SAFETY: two live u64 the glue writes on success, not kept.
    unsafe { sys::ob_sdk_fp_stat_get(unit, stat, packets, bytes) }
}

pub(crate) fn policer_create(unit: c_int, cfg: &sys::ob_policer_cfg_t, pid: &mut c_int) -> c_int {
    // SAFETY: cfg a live ob_policer_cfg_t (layout checked), read only; pid written once.
    unsafe { sys::ob_sdk_policer_create(unit, cfg, pid) }
}

pub(crate) fn policer_set(unit: c_int, pid: c_int, cfg: &sys::ob_policer_cfg_t) -> c_int {
    // SAFETY: cfg as for policer_create, read only.
    unsafe { sys::ob_sdk_policer_set(unit, pid, cfg) }
}

pub(crate) fn policer_destroy(unit: c_int, pid: c_int) -> c_int {
    sys::ob_sdk_policer_destroy(unit, pid)
}

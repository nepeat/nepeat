//! `sFlow` samples to the host's psample (`tl_sflow.{h,cpp}`), line by line.
//!
//! `SONiC` asks for a `SAI_HOSTIF_TYPE_GENETLINK` host interface ("psample", multicast group "packets")
//! and a `HOSTIF_TABLE_ENTRY` (`TRAP_ID` = the SAMPLEPACKET trap, channel GENETLINK) sending the trap's
//! packets there. IFCS 0.12.41 has no netlink API; the ipd driver under it does (`ifcs::ipd`):
//! `IPD_CREATE_NETLINK` (SFLOW) makes the driver recognise the chip's sample frames (Ethernet / IPv4 /
//! GRE with the protocol set by `IPD_SET_PARAMS`, 0x8818), strip them and send them as psample messages
//! with the port's sample rate from `IPD_SET_NETDEV`.
//!
//! - `genetlink_create` records the name and group; nothing reaches the driver until the table entry for
//!   SAMPLEPACKET exists;
//! - `genetlink_trap_set(SAMPLEPACKET, on)`: `SET_PARAMS` + `CREATE_NETLINK`, then every sampled port's
//!   rate; off: `DELETE_NETLINK`. Other trap types: `NOT_SUPPORTED`. One `sFlow` netlink at a time;
//! - `trap_program` (`HostifArea::trap_program` hands the SAMPLEPACKET trap over): system trap
//!   `MIRROR_TO_CPU` enabled (disabled for DROP), as isai; the group's policer as the token-bucket meter of
//!   the CPU queue the samples arrive on (node `TRAFFIC_MONITOR_CPU_QUEUE`, else
//!   `SAI_TERALYNX_SFLOW_CPU_QUEUE`), only when that is the group's queue, and only with
//!   `SAI_TERALYNX_SFLOW_METER=on` (default off: on the chip the meter did not hold its rate). isai polices
//!   with an ACE matching the `sFlow` collector; 0.12.41 refuses a MIRROR match in every ingress match
//!   profile;
//! - `port_rate` (from the mirror area on every sampler change): the port's rate for the samples'
//!   metadata, pushed while a netlink is active; a driver without the rate ioctl refuses it: said once,
//!   the samples then carry rate 0.
//!
//! The driver drops its netlinks when libifcs closes `/dev/ipd`; [`SflowArea::shutdown`] deletes them
//! first. Ids from 1 (`next_`).
#![forbid(unsafe_code)]

use core::ffi::c_int;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use ifcs::{Attrs, Handle, ipd, sys};
use sai_core::backend::types::{GenlId, PolicerId, TrapSpec};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::acl::AclArea;
use crate::core::{Core, NODE};

/// The chip's sample frames: the GRE protocol of the `sFlow` collector's encapsulation (seen on the chip
/// with isai's collector tags).
pub const SHIM_GRE_PROTO: u16 = 0x8818;

/// A generic-netlink host interface (`Genl`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Genl {
    name: String,
    mcgrp: String,
    active: bool,
}

/// The `sFlow` area's state (`SflowArea`).
#[derive(Debug)]
pub struct SflowArea {
    next: u32,
    genl: BTreeMap<u32, Genl>,
    /// (devport, ingress) -> 1 in N.
    rates: BTreeMap<(u32, bool), u32>,
    /// The `/dev/ipd` descriptor (`fd_`, -1: not found yet).
    fd: c_int,
    rate_errors: u32,
    /// The SAMPLEPACKET trap as last programmed (`have_trap_` / `trap_`).
    trap: Option<TrapSpec>,
    /// The samples' CPU queue, -2 = not read yet (`queue_`).
    pub queue: i32,
    /// The CPU queue whose meter we set, -1 none (`policed_q_`).
    policed_q: i32,
    cir_kbps: u32,
    cbs_bytes: u32,
    /// The last refused meter (not retried for the same rate).
    failed_q: i32,
    failed_kbps: u32,
    /// Node `EGRESS_SFLOW_ENABLE` as last read, -1 = unknown (`egress_`).
    pub egress: i32,
}

impl Default for SflowArea {
    fn default() -> Self {
        Self {
            next: 1,
            genl: BTreeMap::new(),
            rates: BTreeMap::new(),
            fd: -1,
            rate_errors: 0,
            trap: None,
            queue: -2,
            policed_q: -1,
            cir_kbps: 0,
            cbs_bytes: 0,
            failed_q: -1,
            failed_kbps: 0,
            egress: -1,
        }
    }
}

/// `trap_to_cpu`: the actions that send the packet to the CPU.
const fn trap_to_cpu(a: i32) -> bool {
    let a = a.cast_unsigned();
    a == ssys::SAI_PACKET_ACTION_TRAP || a == ssys::SAI_PACKET_ACTION_COPY || a == ssys::SAI_PACKET_ACTION_LOG
}

/// A policer's meter on the samples' queue (packets at 84 B, frame + preamble + IPG, isai's ACL meter
/// conversion): (kbit/s at least 1, burst bytes), both at most `u32::MAX`.
fn meter_of(packets: bool, cir: u64, cbs: u64) -> (u32, u32) {
    let cbs = if cbs == 0 { 1 } else { cbs };
    let bps = if packets {
        cir.wrapping_mul(84).wrapping_mul(8)
    } else {
        cir.wrapping_mul(8)
    };
    let burst = if packets { cbs.wrapping_mul(84) } else { cbs };
    let kbps = u32::try_from(bps / 1000).unwrap_or(u32::MAX).max(1);
    let cbs = u32::try_from(burst).unwrap_or(u32::MAX);
    (kbps, cbs)
}

impl SflowArea {
    /// `SflowArea::pre_enable`: before node enable, `SAI_TERALYNX_EGRESS_SFLOW=on` sets node
    /// `EGRESS_SFLOW_ENABLE` (the init yaml's `egress_sflow_enable` is not honoured by 0.12.41).
    pub fn pre_enable(core: &Core) {
        if !core.settings.egress_sflow {
            return;
        }
        let mut e = Attrs::new().data(sys::IFCS_NODE_ATTR_EGRESS_SFLOW_ENABLE, true);
        let rc = core.lib.ifcs_node_attr_set(NODE, &mut e);
        if !rc.is_success() {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!("node EGRESS_SFLOW_ENABLE: {} (egress sampling stays off)", rc.name()),
            );
        }
    }

    /// `ipd_ready`: the test mock's hook, or the `/dev/ipd` libifcs opened (said once when there is none).
    fn ipd_ready(&mut self, core: &Core) -> bool {
        if core.lib.has_ipd_ioctl_hook() {
            return true; // the test mock: no device
        }
        if self.fd >= 0 {
            return true;
        }
        // the /dev/ipd libifcs opened (legacy_abi: /dev/ipd, else /dev/ipd0)
        if let Some(fd) = ipd::find_fd(&core.settings.ipd_dev) {
            self.fd = fd;
        }
        if self.fd < 0 {
            core.log_once(
                "sflow:nofd",
                format_args!("sflow: no open /dev/ipd in this process (libifcs not up?)"),
            );
        }
        self.fd >= 0
    }

    /// `ipd`: one ioctl; 0 or the errno (`ENODEV` without a device), a failure logged.
    fn ipd<T: ipd::Arg>(&mut self, core: &Core, nr: u32, arg: &mut T, what: &str) -> c_int {
        if !self.ipd_ready(core) {
            return ipd::ENODEV;
        }
        match core.lib.ipd_ioctl(self.fd, nr, arg) {
            Ok(()) => 0,
            Err(err) => {
                if err == ipd::EBADF {
                    self.fd = -1; // look again next time
                }
                log::warn(
                    ssys::SAI_API_HOSTIF,
                    format_args!(
                        "sflow: ipd {what} failed: {}",
                        ifcs::netdev::strerror(&std::io::Error::from_raw_os_error(err))
                    ),
                );
                err
            }
        }
    }

    /// `genetlink_create`: recorded (names of 1..=15 bytes), nothing reaches the driver.
    pub fn genetlink_create(&mut self, name: &str, mcgrp: &str) -> Result<GenlId> {
        if name.is_empty() || name.len() > 15 || mcgrp.is_empty() || mcgrp.len() > 15 {
            return Err(Error::InvalidParameter);
        }
        let id = self.next;
        self.next += 1;
        self.genl.insert(
            id,
            Genl {
                name: name.to_owned(),
                mcgrp: mcgrp.to_owned(),
                active: false,
            },
        );
        Ok(GenlId(id))
    }

    /// `genetlink_destroy`: its netlink deleted if active.
    pub fn genetlink_destroy(&mut self, core: &Core, id: GenlId) -> Result<()> {
        let Some(g) = self.genl.get(&id.0).cloned() else {
            return Err(Error::ItemNotFound);
        };
        if g.active {
            self.deactivate(core, id.0);
        }
        self.genl.remove(&id.0);
        Ok(())
    }

    /// `genetlink_trap_set`: SAMPLEPACKET only; one `sFlow` netlink at a time (the driver has one).
    pub fn genetlink_trap_set(&mut self, core: &Core, id: GenlId, trap_type: i32, on: bool) -> Result<()> {
        let Some(g) = self.genl.get(&id.0) else {
            return Err(Error::ItemNotFound);
        };
        if trap_type.cast_unsigned() != ssys::SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET {
            core.log_once(
                "sflow:trap",
                format_args!("sflow: only SAMPLEPACKET packets can go to a genetlink host interface"),
            );
            return Err(Error::NotSupported);
        }
        let active = g.active;
        if !on {
            if active {
                self.deactivate(core, id.0);
            }
            return Ok(());
        }
        if active {
            return Ok(());
        }
        if self.genl.values().any(|o| o.active) {
            return Err(Error::ItemAlreadyExists); // the driver has one sFlow netlink
        }
        self.activate(core, id.0)
    }

    /// `activate`: the samples' GRE protocol, the netlink, then every sampled port's rate.
    fn activate(&mut self, core: &Core, id: u32) -> Result<()> {
        let Some(g) = self.genl.get(&id).cloned() else {
            return Err(Error::ItemNotFound);
        };
        let mut p = ipd::Params::gre(SHIM_GRE_PROTO);
        let err = self.ipd(core, ipd::NR_SET_PARAMS, &mut p, "SET_PARAMS (sample GRE protocol)");
        if err != 0 {
            return Err(if err == ipd::ENODEV {
                Error::Uninitialized
            } else {
                Error::Failure
            });
        }
        let mut n = ipd::Netlink::sflow(&g.name, &g.mcgrp);
        if self.ipd(core, ipd::NR_CREATE_NETLINK, &mut n, "CREATE_NETLINK") != 0 {
            return Err(Error::Failure);
        }
        if let Some(x) = self.genl.get_mut(&id) {
            x.active = true;
        }
        self.rate_errors = 0;
        let rates: Vec<((u32, bool), u32)> = self.rates.iter().map(|(k, v)| (*k, *v)).collect();
        for ((hw, ingress), rate) in rates {
            let _ = self.push_rate(core, hw, ingress, rate);
        }
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!(
                "sflow: samples to genetlink {} / {} ({} sampled ports)",
                g.name,
                g.mcgrp,
                self.rates.len()
            ),
        );
        Ok(())
    }

    /// `deactivate`: `DELETE_NETLINK` (a failure only logged).
    fn deactivate(&mut self, core: &Core, id: u32) {
        let Some(g) = self.genl.get(&id).cloned() else { return };
        let mut n = ipd::Netlink::sflow(&g.name, &g.mcgrp);
        let _ = self.ipd(core, ipd::NR_DELETE_NETLINK, &mut n, "DELETE_NETLINK");
        if let Some(x) = self.genl.get_mut(&id) {
            x.active = false;
        }
    }

    /// `push_rate`: a sysport's sample rate to the driver (the first refusal said).
    fn push_rate(&mut self, core: &Core, hw: u32, ingress: bool, rate: u32) -> c_int {
        let Some(sp) = core.sysport.get(&hw).copied() else {
            return ipd::EINVAL;
        };
        let id = if ingress {
            ipd::ND_INGRESS_RATE
        } else {
            ipd::ND_EGRESS_RATE
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the C++'s (uint16_t) cast of the handle value"
        )]
        let sysport = sp.value() as u16;
        let mut b = ipd::NetdevInfo::rate(id, sysport, rate);
        let err = self.ipd(core, ipd::NR_SET_NETDEV, &mut b, "SET_NETDEV sample rate");
        if err != 0 {
            if self.rate_errors == 0 {
                log::warn(
                    ssys::SAI_API_HOSTIF,
                    format_args!(
                        "sflow: the driver refuses sample rates (ipd without ipd-sflow-psample.patch in \
                         legacy_abi mode?): samples carry rate 0"
                    ),
                );
            }
            self.rate_errors += 1;
        }
        err
    }

    /// `port_rate` (the mirror area, on every sampler change; 0: the sampler is off): pushed while a
    /// netlink is active.
    pub fn port_rate(&mut self, core: &Core, hw: u32, ingress: bool, rate: u32) {
        let key = (hw, ingress);
        if rate != 0 {
            self.rates.insert(key, rate);
        } else if self.rates.remove(&key).is_none() {
            return;
        }
        if self.genl.values().any(|g| g.active) {
            let _ = self.push_rate(core, hw, ingress, rate);
        }
    }

    // ------------------------------------------------- SAMPLEPACKET policing

    /// `trap_program`: the SAMPLEPACKET trap = system trap `MIRROR_TO_CPU`, ENABLE on (off for DROP;
    /// removal leaves it), as isai 6.2.0 (oracle sflow-police, 2026-11-16); then its group's policer as the
    /// samples' queue meter ([`SflowArea::police`]; a refused meter leaves the samples unpoliced, not
    /// lost: no error to the caller).
    pub fn trap_program(&mut self, core: &Core, acl: &AclArea, s: &TrapSpec, remove: bool) -> Result<()> {
        let h = Handle::hostif_trap(sys::IFCS_HOSTIF_TRAP_MIRROR_TO_CPU);
        if remove {
            self.unpolice(core);
            self.trap = None;
            return Ok(());
        }
        let old = self.trap;
        self.trap = Some(*s);
        let cpu = trap_to_cpu(s.action) && s.group_up;
        let was = old.map(|o| trap_to_cpu(o.action) && o.group_up);
        if was != Some(cpu) {
            let mut e = Attrs::new().data(sys::IFCS_HOSTIF_TRAP_ATTR_ENABLE, cpu);
            let st = core.call(
                core.lib.ifcs_hostif_trap_attr_set(NODE, h.0, &mut e),
                "hostif_trap_attr_set (MIRROR_TO_CPU)",
            );
            if let Err(e) = st {
                self.trap = old;
                return Err(e);
            }
        }
        let _ = self.police(core, acl);
        Ok(())
    }

    /// `policer_changed` (`AclArea::policer_set`): the samples' queue meter follows its policer.
    pub fn policer_changed(&mut self, core: &Core, acl: &AclArea, pid: PolicerId) {
        if self.trap.is_some_and(|t| t.policer == Some(pid)) {
            let _ = self.police(core, acl);
        }
    }

    /// `sample_queue`: the CPU queue of the samples (node `TRAFFIC_MONITOR_CPU_QUEUE`, a boot-time
    /// parameter; else `SAI_TERALYNX_SFLOW_CPU_QUEUE`; -1: unknown), read once.
    fn sample_queue(&mut self, core: &Core) -> i32 {
        if self.queue != -2 {
            return self.queue;
        }
        let mut a = Attrs::new().of(sys::IFCS_NODE_ATTR_TRAFFIC_MONITOR_CPU_QUEUE);
        let (rc, _) = core.lib.ifcs_node_attr_get(NODE, &mut a);
        let ok = rc.is_success();
        self.queue = if ok {
            a.u32_at(0).cast_signed()
        } else {
            core.settings.sflow_cpu_queue.unwrap_or(-1)
        };
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!(
                "sflow: samples on CPU queue {} ({})",
                self.queue,
                if ok {
                    "node TRAFFIC_MONITOR_CPU_QUEUE"
                } else if self.queue < 0 {
                    "unknown"
                } else {
                    "SAI_TERALYNX_SFLOW_CPU_QUEUE"
                }
            ),
        );
        self.queue
    }

    /// `unpolice`: our meter off.
    fn unpolice(&mut self, core: &Core) {
        if self.policed_q < 0 {
            return;
        }
        let mut e = Attrs::new().data(sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_INFORMATION_RATE_ENABLE, false);
        let _ = core.call(
            core.lib
                .ifcs_cpu_queue_attr_set(NODE, self.policed_q.cast_unsigned(), &mut e),
            "cpu_queue meter off (sflow)",
        );
        self.policed_q = -1;
        self.cir_kbps = 0;
        self.cbs_bytes = 0;
        self.failed_q = -1;
    }

    /// `police`: the samples' queue meter from the trap's group policer (`cpu_queue.py`: token bucket,
    /// committed rate kbit/s and burst bytes, u32), only when that queue is the group's queue (a meter on a
    /// shared queue would police other traps) and with `SAI_TERALYNX_SFLOW_METER=on` (on the chip,
    /// 2026-11-16, the meter dropped samples but not at its rate). `METER_MODE` (`TOKEN_BUCKET`, the only
    /// mode) is refused by `cpu_queue_attr_set`: not set. One attribute per call: a refusal names it.
    fn police(&mut self, core: &Core, acl: &AclArea) -> Result<()> {
        let trap = self.trap;
        let mut cfg = trap.and_then(|t| t.policer).and_then(|p| acl.policer_get(p));
        if cfg.is_some() && !core.settings.sflow_meter {
            core.log_once(
                "sflow:meter-off",
                format_args!(
                    "sflow: the sflow trap group's policer is not applied to samples (SAI_TERALYNX_SFLOW_METER off)"
                ),
            );
            cfg = None;
        }
        let q = if cfg.is_some() { self.sample_queue(core) } else { -1 };
        let tq = trap.map_or(0, |t| t.queue);
        if cfg.is_some() && i64::from(q) != i64::from(tq.cast_signed()) {
            core.log_once(
                &format!("sflow:police-q{q}-{tq}"),
                format_args!(
                    "sflow: samples arrive on CPU queue {q}, the trap group's queue is {tq}: not policed (init \
                     config traffic_monitor_cpu_queue = the group's queue to police them)"
                ),
            );
            cfg = None;
        }
        if self.policed_q >= 0 && (cfg.is_none() || self.policed_q != q) {
            self.unpolice(core);
        }
        let Some(cfg) = cfg else { return Ok(()) };
        let (kbps, cbs) = meter_of(cfg.packets, cfg.cir, cfg.cbs);
        if q == self.policed_q && kbps == self.cir_kbps && cbs == self.cbs_bytes {
            return Ok(()); // as programmed
        }
        if q == self.failed_q && kbps == self.failed_kbps {
            return Ok(()); // refused before
        }
        let steps = [
            Attrs::new().u32(sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_INFORMATION_RATE, kbps),
            Attrs::new().u32(sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_BURST_SIZE, cbs),
            Attrs::new().data(sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_INFORMATION_RATE_ENABLE, true),
        ];
        let ids = [
            sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_INFORMATION_RATE,
            sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_BURST_SIZE,
            sys::IFCS_CPU_QUEUE_ATTR_METER_COMMITTED_INFORMATION_RATE_ENABLE,
        ];
        for (mut x, id) in steps.into_iter().zip(ids) {
            let st = core.call(
                core.lib.ifcs_cpu_queue_attr_set(NODE, q.cast_unsigned(), &mut x),
                "cpu_queue meter (sflow)",
            );
            if let Err(e) = st {
                log::error(
                    ssys::SAI_API_HOSTIF,
                    format_args!("sflow: CPU queue {q} meter attribute {id} refused: samples not policed"),
                );
                self.policed_q = q;
                self.unpolice(core);
                self.failed_q = q;
                self.failed_kbps = kbps;
                return Err(e);
            }
        }
        self.policed_q = q;
        self.cir_kbps = kbps;
        self.cbs_bytes = cbs;
        log::notice(
            ssys::SAI_API_HOSTIF,
            format_args!(
                "sflow: samples policed on CPU queue {q}: {kbps} kbit/s, burst {cbs} B (policer {})",
                trap.and_then(|t| t.policer).map_or(-1, |p| i64::from(p.0))
            ),
        );
        Ok(())
    }

    /// `egress_enabled`: node `EGRESS_SFLOW_ENABLE` (a boot-time parameter), read on every egress sampler
    /// change (rare; a cached value would hide the init's); a change of the answer said.
    pub fn egress_enabled(&mut self, core: &Core) -> bool {
        let mut a = Attrs::new().of(sys::IFCS_NODE_ATTR_EGRESS_SFLOW_ENABLE);
        let (rc, _) = core.lib.ifcs_node_attr_get(NODE, &mut a);
        let on = i32::from(rc.is_success() && a.data_at(0) != 0);
        if on != self.egress {
            log::notice(
                ssys::SAI_API_SAMPLEPACKET,
                format_args!(
                    "sflow: node EGRESS_SFLOW_ENABLE {}",
                    if !rc.is_success() {
                        rc.name()
                    } else if on == 1 {
                        "on"
                    } else {
                        "off"
                    }
                ),
            );
        }
        self.egress = on;
        on == 1
    }

    /// `SflowArea::shutdown`: the active netlinks deleted, the rates and the device forgotten, our meter
    /// off, the trap and the cached node reads forgotten.
    pub fn shutdown(&mut self, core: &Core) {
        let active: Vec<u32> = self.genl.iter().filter(|(_, g)| g.active).map(|(id, _)| *id).collect();
        for id in active {
            self.deactivate(core, id);
        }
        self.rates.clear();
        self.fd = -1;
        self.unpolice(core);
        self.trap = None;
        self.queue = -2;
        self.egress = -1;
    }

    /// `show` (part of the C++ `diag "mirror"` text).
    pub fn show(&self) -> String {
        let mut s = String::new();
        for g in self.genl.values() {
            let _ = writeln!(
                s,
                "sflow: genetlink {} / {} {}, {} sampled ports, rate errors {}",
                g.name,
                g.mcgrp,
                if g.active { "active (SAMPLEPACKET)" } else { "idle" },
                self.rates.len(),
                self.rate_errors
            );
        }
        if self.genl.is_empty() {
            "sflow: no genetlink host interface\n".clone_into(&mut s);
        }
        if let Some(t) = &self.trap {
            let _ = writeln!(
                s,
                "sflow: SAMPLEPACKET trap action {} queue {} policer {} (yellow drop {}, red drop {})",
                t.action,
                t.queue,
                t.policer.map_or(-1, |p| i64::from(p.0)),
                i32::from(t.yellow_drop),
                i32::from(t.red_drop)
            );
        }
        if self.policed_q >= 0 {
            let _ = writeln!(
                s,
                "sflow: samples policed on CPU queue {}: meter {} kbit/s, burst {} B",
                self.policed_q, self.cir_kbps, self.cbs_bytes
            );
        } else if self.trap.is_some() {
            s.push_str("sflow: samples not policed\n");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_as_isai_converts_acl_meters() {
        // SONiC's sflow group: 1000 pps, burst 1000 packets -> 672 kbit/s, 84000 B
        assert_eq!(meter_of(true, 1000, 1000), (672, 84000));
        assert_eq!(meter_of(true, 2000, 1000), (1344, 84000));
        assert_eq!(meter_of(false, 125_000, 0), (1000, 1)); // bytes: x 8, burst at least 1
        assert_eq!(meter_of(false, 0, 0), (1, 1)); // at least 1 kbit/s
        assert_eq!(meter_of(false, u64::MAX / 8, u64::MAX), (u32::MAX, u32::MAX));
    }

    #[test]
    fn traps_to_cpu_and_names() {
        assert!(trap_to_cpu(ssys::SAI_PACKET_ACTION_TRAP.cast_signed()));
        assert!(trap_to_cpu(ssys::SAI_PACKET_ACTION_COPY.cast_signed()));
        assert!(trap_to_cpu(ssys::SAI_PACKET_ACTION_LOG.cast_signed()));
        assert!(!trap_to_cpu(ssys::SAI_PACKET_ACTION_DROP.cast_signed()));
        let mut s = SflowArea::default();
        assert_eq!(s.genetlink_create("", "packets"), Err(Error::InvalidParameter));
        assert_eq!(
            s.genetlink_create("psample", "0123456789abcdef"),
            Err(Error::InvalidParameter)
        );
        assert_eq!(s.genetlink_create("psample", "packets"), Ok(GenlId(1)));
        assert_eq!(s.genetlink_create("psample2", "packets"), Ok(GenlId(2)));
        assert_eq!(
            s.show(),
            "sflow: genetlink psample / packets idle, 0 sampled ports, rate errors 0\nsflow: genetlink psample2 / packets idle, 0 sampled ports, rate errors 0\n"
        );
        assert_eq!(SflowArea::default().show(), "sflow: no genetlink host interface\n");
    }
}

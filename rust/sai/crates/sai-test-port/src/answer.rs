//! Running a request: the library's own questions through [`sai_core::testing`], the virtual
//! backend's hooks and state through its accessors; the answer encoded as the header says.
use crate::encode::{Flag, Ip, List, Records, Text};
use crate::request::{Feature, Request, Table};
use crate::{Code, Context, Reply};
use sai_core::backend::types::{BufferCfg, FdbChange, FdbEvent, IntfId, PolicerCfg, RouteTarget, SchedCfg};
use sai_core::backend::types::{
    Dest, EgressId, Fec, FloodKind, FpEntryId, FpGroupId, GenlId, Learn, Loopback, MacAddr, MdioDev, MirrorId, PhyAddr,
    PolicerId, PortStat, QosMapId, QosMapKind, SampleId, StormKind, TrapKind, TrunkId, Vrf, WredId,
};
use sai_core::backend::{Backend, HwPort, L3 as _, StatsObject, Vid};
use sai_core::oid::{ObjectType, RawOid};
use sai_core::sai::Sai;
use sai_core::testing::{self, AreaAnswer, oid_layout};
use sai_core::value::IpPrefix;
use sai_core::virt::{EgressKind, FpEntry, VirtPort, VirtualBackend};
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::PoisonError;

/// A request's outcome: the records of an `OK` reply, or another reply.
type Outcome = Result<Records, Reply>;

impl Reply {
    fn with(code: Code, text: String) -> Self {
        Self { code, text }
    }

    /// An `ABSENT` reply (empty).
    fn absent() -> Self {
        Self::with(Code::Absent, String::new())
    }
}

/// `UNSUPPORTED` with its reason.
fn unsupported(name: &str, reason: &str) -> Reply {
    Reply::with(Code::Unsupported, format!("{name}: {reason}"))
}

/// `BAD_REQUEST` with its reason.
fn bad(name: &str, reason: &str) -> Reply {
    Reply::with(Code::BadRequest, format!("{name}: {reason}"))
}

/// One record `value=<v>`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the outcome of most requests, built in one place"
)]
fn value(v: impl core::fmt::Display) -> Outcome {
    Ok(Records::one(&[("value", &v)]))
}

/// No records.
#[allow(clippy::unnecessary_wraps, reason = "the outcome of every hook, built in one place")]
fn done() -> Outcome {
    Ok(Records::default())
}

/// An area's answer, or `UNSUPPORTED` naming the area that has not ported it.
fn area<T>(name: &str, answer: AreaAnswer<T>) -> Result<T, Reply> {
    answer.map_err(|n| unsupported(name, &format!("not answered yet by {} (area not ported)", n.area)))
}

// ---- numbers of the C++ int type as the Rust ids (out of range: no such thing)

fn hw(n: i64) -> Option<HwPort> {
    u16::try_from(n).ok().map(HwPort)
}

fn id32(n: i64) -> Option<u32> {
    u32::try_from(n).ok()
}

fn index8(n: i64) -> Option<u8> {
    u8::try_from(n).ok()
}

fn vid(n: i64) -> Option<Vid> {
    u16::try_from(n).ok().and_then(|v| Vid::new(v).ok())
}

/// `-1` for none.
fn or_none(n: Option<u32>) -> i64 {
    n.map_or(-1, i64::from)
}

// ---- the C++ test port's mappings (tests/test_port_cpp.cpp)

const fn sai_fec(fec: Fec) -> u32 {
    match fec {
        Fec::None => sys::SAI_PORT_FEC_MODE_NONE,
        Fec::Rs => sys::SAI_PORT_FEC_MODE_RS,
        Fec::Fc => sys::SAI_PORT_FEC_MODE_FC,
    }
}

const fn sai_loopback(lb: Loopback) -> u32 {
    match lb {
        Loopback::None => sys::SAI_PORT_LOOPBACK_MODE_NONE,
        Loopback::Mac => sys::SAI_PORT_LOOPBACK_MODE_MAC,
        Loopback::Phy => sys::SAI_PORT_LOOPBACK_MODE_PHY,
    }
}

/// `tp::Learn`: `LEARN_HW`, `LEARN_DISABLE`, `LEARN_DROP`, `LEARN_CPU`.
const fn tp_learn(learn: Learn) -> u8 {
    match learn {
        Learn::Hw => 0,
        Learn::Disable => 1,
        Learn::Drop => 2,
        Learn::Cpu => 3,
    }
}

/// `sai_qos_map_type_t` <-> `QosMapKind` (the C++ test port's table).
const QOS_MAP_TYPES: [(u32, QosMapKind); 10] = [
    (sys::SAI_QOS_MAP_TYPE_DSCP_TO_TC, QosMapKind::DscpToTc),
    (sys::SAI_QOS_MAP_TYPE_DOT1P_TO_TC, QosMapKind::Dot1pToTc),
    (sys::SAI_QOS_MAP_TYPE_TC_TO_QUEUE, QosMapKind::TcToQueue),
    (sys::SAI_QOS_MAP_TYPE_TC_TO_PRIORITY_GROUP, QosMapKind::TcToPg),
    (sys::SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_QUEUE, QosMapKind::PfcPrioToQueue),
    (
        sys::SAI_QOS_MAP_TYPE_PFC_PRIORITY_TO_PRIORITY_GROUP,
        QosMapKind::PfcPrioToPg,
    ),
    (sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DSCP, QosMapKind::TcColorToDscp),
    (sys::SAI_QOS_MAP_TYPE_TC_AND_COLOR_TO_DOT1P, QosMapKind::TcColorToDot1p),
    (sys::SAI_QOS_MAP_TYPE_DSCP_TO_COLOR, QosMapKind::DscpToColor),
    (sys::SAI_QOS_MAP_TYPE_DOT1P_TO_COLOR, QosMapKind::Dot1pToColor),
];

fn qos_kind(sai_type: i32) -> Option<QosMapKind> {
    let sai_type = u32::try_from(sai_type).ok()?;
    QOS_MAP_TYPES.iter().find(|(t, _)| *t == sai_type).map(|(_, k)| *k)
}

fn qos_type(kind: QosMapKind) -> u32 {
    QOS_MAP_TYPES.iter().find(|(_, k)| *k == kind).map_or(0, |(t, _)| *t)
}

const fn storm_kind(attr: u32) -> Option<StormKind> {
    match attr {
        sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID => Some(StormKind::Broadcast),
        sys::SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID => Some(StormKind::Multicast),
        sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID => Some(StormKind::UnknownUnicast),
        _ => None,
    }
}

const fn flood_kind(attr: u32) -> Option<FloodKind> {
    match attr {
        sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE => Some(FloodKind::UnknownUnicast),
        sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE => Some(FloodKind::UnknownMulticast),
        sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE => Some(FloodKind::Broadcast),
        _ => None,
    }
}

const NO_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

// ---- the entry point

/// Whether a request is answered without the library's state (and so without the API lock).
pub(crate) const fn is_pure(request: &Request) -> bool {
    matches!(
        request,
        Request::HasHardwareBackend
            | Request::EventsDelivered
            | Request::MakeOid { .. }
            | Request::OidType(_)
            | Request::OidIndex(_)
            | Request::OidSwitch(_)
            | Request::OidIndexMask
            | Request::AttrCaps
            | Request::LogToStderr(_)
    )
}

/// See [`crate::Query::answer`].
pub(crate) fn answer<B: Backend>(name: &str, request: &Request, sai: Option<&mut Sai<B>>, context: &Context) -> Reply {
    let outcome = match (pure(request, context), sai) {
        (Some(outcome), _) => outcome,
        // before sai_api_initialize there is no switch (the header's answer, not UNSUPPORTED)
        (None, None) if *request == Request::SwitchCreated => value(Flag(false)),
        (None, None) => Err(unsupported(name, "the API is not initialised")),
        (None, Some(sai)) => match library(name, request, sai) {
            Some(outcome) => outcome,
            None => chip(name, request, sai),
        },
    };
    match outcome {
        Ok(records) => Reply::with(Code::Ok, records.into_text()),
        Err(reply) => reply,
    }
}

/// What needs no library state: OID layout, build facts, the rim's counter, the log switch.
fn pure(request: &Request, context: &Context) -> Option<Outcome> {
    Some(match *request {
        Request::HasHardwareBackend => value(Flag(context.has_hardware_backend)),
        Request::EventsDelivered => value(context.events_delivered),
        Request::MakeOid {
            object_type,
            index,
            switch,
        } => value(oid_layout::make_oid(object_type, index, switch).0),
        Request::OidType(oid) => value(oid_layout::oid_type(RawOid(oid))),
        Request::OidIndex(oid) => value(oid_layout::oid_index(RawOid(oid))),
        Request::OidSwitch(oid) => value(oid_layout::oid_switch(RawOid(oid))),
        Request::OidIndexMask => value(oid_layout::INDEX_MASK),
        Request::AttrCaps => {
            let mut records = Records::default();
            for (ot, id, cap) in sai_core::caps::attributes() {
                records.push(&[
                    ("ot", &ot),
                    ("id", &id),
                    ("create", &Flag(cap.create)),
                    ("set", &Flag(cap.set)),
                    ("get", &Flag(cap.get)),
                ]);
            }
            Ok(records)
        }
        Request::LogToStderr(on) => {
            testing::log_to_stderr(on);
            done()
        }
        _ => return None,
    })
}

/// The library's view of its objects and its services.
fn library<B: Backend>(name: &str, request: &Request, sai: &Sai<B>) -> Option<Outcome> {
    let backend_number = |oid: u64| -> Outcome {
        let id = area(name, testing::backend_id(sai, RawOid(oid)))?;
        value(id.map_or(-1, testing::BackendId::number))
    };
    Some(match request {
        Request::SwitchCreated => value(Flag(testing::switch_created(sai))),
        Request::BackendId(oid) => backend_number(*oid),
        Request::Rif(oid) => {
            let rif = testing::rif(sai, RawOid(*oid)).unwrap_or_default();
            Ok(Records::one(&[
                ("intf", &or_none(rif.intf.map(IntfId::get))),
                ("vrf", &or_none(rif.vrf.map(Vrf::get))),
                ("vid", &rif.vid.map_or(0, Vid::get)),
            ]))
        }
        Request::PortLag(oid) => area(name, testing::port_lag(sai, RawOid(*oid)))
            .and_then(|lag| value(lag.map_or(0, |l| sai_core::oid::TypedId::raw(l).0))),
        Request::TrapProgrammed(oid) => value(Flag(testing::trap_programmed(sai, RawOid(*oid)))),
        Request::ObjectCount(ot) => match ObjectType::from_sai(*ot).and_then(|t| testing::object_count(sai, t)) {
            Some(count) => area(name, count).and_then(value),
            None => Err(bad(name, "answered for VIRTUAL_ROUTER, ACL_TABLE and ACL_ENTRY only")),
        },
        Request::AclBindingCount => area(name, testing::acl_binding_count(sai)).and_then(value),
        Request::CoppGroup => value(or_none(testing::copp_group(sai).map(FpGroupId::get))),
        Request::CoppRuleCount => value(testing::copp_rule_count(sai)),
        Request::NeighborEgress(rif, ip) => value(or_none(
            testing::neighbor_egress(sai, RawOid(*rif), *ip).map(EgressId::get),
        )),
        Request::DiagStart(path) if path.is_empty() => done(),
        Request::DiagStart(_) | Request::DiagStop => Err(unsupported(
            name,
            "the diag server is the rim's (sai-ffi answers this request itself)",
        )),
        Request::ExtPhyActive => area(name, testing::ext_phy_active(sai)).and_then(|on| value(Flag(on))),
        Request::ExtMacsecActive => area(name, testing::ext_macsec_active(sai)).and_then(|on| value(Flag(on))),
        Request::ExtPhyStatus => Ok(Records::one(&[("text", &Text(&testing::ext_phy_status(sai)))])),
        Request::ExtMacsecStatus => Ok(Records::one(&[("text", &Text(&testing::ext_macsec_status(sai)))])),
        _ => return None,
    })
}

/// The virtual backend's hooks and state.
fn chip<B: Backend>(name: &str, request: &Request, sai: &mut Sai<B>) -> Outcome {
    let created = testing::switch_created(sai);
    let Some(chip) = testing::virtual_backend_mut(sai) else {
        return Err(unsupported(
            name,
            if created {
                "the switch's backend is not the virtual one"
            } else {
                "no switch (create one with SAI_RS_BACKEND=virtual)"
            },
        ));
    };
    match hook(name, request, chip) {
        Some(outcome) => outcome,
        None => state(name, request, chip),
    }
}

/// The hooks: they change the chip model only.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per hook of the protocol, in the header's order"
)]
fn hook(name: &str, request: &Request, chip: &mut VirtualBackend) -> Option<Outcome> {
    let no_port = |n: i64| bad(name, &format!("no backend port {n}"));
    Some(match *request {
        Request::SetFeature(feature, on) => {
            let toggles = chip.toggles_mut();
            match feature {
                Feature::Qos => toggles.qos_on = on,
                Feature::Mirror => toggles.mirror_on = on,
                Feature::L2Extras => toggles.l2x_on = on,
            }
            done()
        }
        Request::InjectLink(n, up) => match hw(n).map(|hw| chip.inject_link(hw, up)) {
            Some(Ok(())) => done(),
            _ => Err(no_port(n)),
        },
        Request::Linkscan => {
            chip.linkscan();
            done()
        }
        Request::InjectFdb {
            learned,
            mac,
            vid: v,
            dest,
        } => match (vid(v), Dest::from_raw(dest)) {
            (Some(vid), Some(hw_port)) => {
                let change = if learned { FdbChange::Learned } else { FdbChange::Aged };
                chip.inject_fdb(FdbEvent {
                    change,
                    mac,
                    vid,
                    hw_port,
                });
                done()
            }
            (None, _) => Err(bad(name, &format!("bad VLAN id {v}"))),
            (_, None) => Err(bad(name, &format!("bad dest {dest}"))),
        },
        Request::SetPortStat(n, id, v) => {
            let port = hw(n).ok_or_else(|| no_port(n));
            match (port, PortStat::from_sai_id(id), testing::port_pfc_stat(id)) {
                (Err(e), ..) => Err(e),
                (Ok(hw), Some(stat), _) => {
                    chip.set_stat(hw, stat, v);
                    done()
                }
                (Ok(hw), None, Some((prio, tx))) => {
                    chip.set_pfc_stat(hw, prio, tx, v);
                    done()
                }
                (Ok(_), None, None) => Err(bad(
                    name,
                    &format!("no backend counter for port stat id {id} (PFC ids: as qos::port_pfc_stat maps them)"),
                )),
            }
        }
        Request::SetQueueStat(n, q, id, v) | Request::SetPgStat(n, q, id, v) => {
            let (Some(port), Some(index)) = (hw(n), index8(q)) else {
                return Some(Err(bad(name, &format!("bad port {n} or index {q}"))));
            };
            let object = if matches!(request, Request::SetQueueStat(..)) {
                StatsObject::Queue { port, index }
            } else {
                StatsObject::PriorityGroup { port, index }
            };
            chip.set_counter(object, id, v);
            done()
        }
        Request::FpHit(eid, packets, bytes) => {
            if let Some(eid) = id32(eid) {
                chip.fp_hit(FpEntryId(eid), packets, bytes);
            }
            done()
        }
        Request::SetTemperatures(ref tenths) => {
            chip.set_temps(tenths.clone());
            done()
        }
        Request::SetTemperatureFail(fail) => {
            chip.set_temp_fail(fail);
            done()
        }
        Request::MdioSet { phy, dev, reg, value } => {
            let dev = u8::try_from(dev).ok().and_then(MdioDev::new);
            let (Some(dev), Ok(reg), Ok(value)) = (dev, u16::try_from(reg), u16::try_from(value)) else {
                return Some(Err(bad(name, "dev must be 1..=31, reg and value 16 bits")));
            };
            let mdio = chip.mdio();
            let mut model = mdio.lock().unwrap_or_else(PoisonError::into_inner);
            model.regs.insert((PhyAddr(phy), dev, reg), value);
            done()
        }
        Request::MdioSetDead(phy, dead) => {
            let mdio = chip.mdio();
            let mut model = mdio.lock().unwrap_or_else(PoisonError::into_inner);
            if dead {
                model.dead.insert(PhyAddr(phy));
            } else {
                model.dead.remove(&PhyAddr(phy));
            }
            done()
        }
        Request::MdioLog { clear } => {
            let mdio = chip.mdio();
            let mut model = mdio.lock().unwrap_or_else(PoisonError::into_inner);
            let mut records = Records::default();
            for op in &model.log {
                records.push(&[
                    ("op", &if op.write { 'W' } else { 'R' }),
                    ("phy", &op.phy.0),
                    ("dev", &op.dev.get()),
                    ("reg", &op.reg),
                    ("val", &op.val),
                ]);
            }
            if clear {
                model.log.clear();
            }
            Ok(records)
        }
        Request::ShellLog => {
            let mut records = Records::default();
            for cmd in chip.shell_log() {
                records.push(&[("cmd", &Text(cmd))]);
            }
            Ok(records)
        }
        Request::BackendHostInsert(vrf, ip, egress) => match (id32(vrf), id32(egress)) {
            (Some(vrf), Some(egress)) => {
                chip.host_insert(Vrf(vrf), ip, EgressId(egress));
                done()
            }
            _ => Err(bad(name, "bad VRF or egress id")),
        },
        Request::BackendHostErase(vrf, ip) => {
            if let Some(vrf) = id32(vrf) {
                chip.host_erase(Vrf(vrf), ip);
            }
            done()
        }
        Request::BackendL2Insert(mac, v, dest) => match (vid(v), Dest::from_raw(dest)) {
            (Some(vid), Some(dest)) => {
                chip.insert_l2_directly(mac, vid, dest);
                done()
            }
            _ => Err(bad(name, &format!("bad VLAN id {v} or dest {dest}"))),
        },
        Request::BackendL2Erase(mac, v) => {
            if let Some(vid) = vid(v) {
                chip.erase_l2_directly(mac, vid);
            }
            done()
        }
        Request::DropEgress => value(chip.l3_egress_drop().map_or(-1, |id| i64::from(id.get()))),
        Request::BackendEgressDestroy(egress) => {
            let status = match id32(egress).map(|id| chip.l3_egress_destroy(EgressId(id))) {
                Some(Ok(())) => sys::SAI_STATUS_SUCCESS,
                Some(Err(e)) => e.status(),
                None => sys::SAI_STATUS_ITEM_NOT_FOUND,
            };
            Ok(Records::one(&[("status", &status)]))
        }
        Request::LagDest(tid) => match id32(tid).filter(|t| t & Dest::LAG_BIT == 0) {
            Some(tid) => value(Dest::Lag(TrunkId(tid)).raw()),
            None => Err(bad(name, &format!("bad trunk id {tid}"))),
        },
        Request::PortForgetQuirks(n) => {
            if let Some(hw) = hw(n) {
                chip.forget_port_quirks(hw);
            }
            done()
        }
        _ => return None,
    })
}

fn port_record(records: &mut Records, hw: HwPort, p: &VirtPort) {
    records.push(&[
        ("hw", &hw.0),
        ("admin", &Flag(p.admin)),
        ("link", &Flag(p.link)),
        ("fec", &sai_fec(p.fec)),
        ("autoneg", &Flag(p.autoneg)),
        ("iface", &Text(p.iface.map_or("", |i| i.name()))),
        ("ifg", &p.ifg),
        ("quirk_calls", &p.quirk_calls),
        ("loopback", &sai_loopback(p.lb)),
        ("pvid", &p.pvid.get()),
        ("frame_max", &p.frame_max),
        ("learn", &tp_learn(p.learn)),
    ]);
}

fn egress_record(records: &mut Records, id: EgressId, e: EgressKind) {
    let (intf, mac, dest, vid, cpu, drop) = match e {
        EgressKind::Forward { intf, dmac, dest, vid } => (intf.get(), dmac, dest.raw(), vid.get(), false, false),
        EgressKind::Cpu => (0, MacAddr::default(), 0, 0, true, false),
        EgressKind::Drop => (0, MacAddr::default(), 0, 0, false, true),
    };
    records.push(&[
        ("id", &id.get()),
        ("intf", &intf),
        ("mac", &mac),
        ("dest", &dest),
        ("vid", &vid),
        ("cpu", &Flag(cpu)),
        ("drop", &Flag(drop)),
    ]);
}

fn fp_entry_record(records: &mut Records, eid: FpEntryId, e: &FpEntry) {
    let m = &e.rule.m;
    let (dst, dst_mask) = match (m.dst_ip, m.dst_ip6) {
        (Some(v4), _) => (IpAddr::V4(v4.value), IpAddr::V4(v4.mask)),
        (None, Some(v6)) => (IpAddr::V6(v6.value), IpAddr::V6(v6.mask)),
        (None, None) => (NO_IP, NO_IP),
    };
    let in_ports: Vec<u16> = m.in_ports.iter().flatten().map(|hw| hw.0).collect();
    let ingress: Vec<u32> = e.rule.mirror_ingress.iter().map(|id| id.get()).collect();
    let egress: Vec<u32> = e.rule.mirror_egress.iter().map(|id| id.get()).collect();
    records.push(&[
        ("eid", &eid.get()),
        ("gid", &e.gid.get()),
        ("priority", &e.rule.priority),
        ("present", &m.present()),
        ("in_ports", &List(&in_ports)),
        ("ether", &m.ether_type.map_or(0, |x| x.value)),
        ("ip_type", &m.ip_type.unwrap_or(0)),
        ("l4_src", &m.l4_src_port.map_or(0, |x| x.value)),
        ("l4_dst", &m.l4_dst_port.map_or(0, |x| x.value)),
        ("dst", &Ip(dst)),
        ("dst_mask", &Ip(dst_mask)),
        ("drop", &Flag(e.rule.drop)),
        ("drop_cancel", &Flag(e.rule.drop_cancel)),
        ("copy_cpu", &Flag(e.rule.copy_cpu)),
        ("counter", &Flag(e.rule.counter)),
        ("policer", &or_none(e.rule.policer.map(PolicerId::get))),
        ("red_drop", &Flag(e.rule.red_drop)),
        ("cpu_copy_only", &Flag(e.rule.cpu_copy_only)),
        ("mirror_ingress", &List(&ingress)),
        ("mirror_egress", &List(&egress)),
    ]);
}

fn policer_record(c: &PolicerCfg) -> Records {
    Records::one(&[
        ("tr_tcm", &Flag(c.tr_tcm)),
        ("packets", &Flag(c.packets)),
        ("color_blind", &Flag(c.color_blind)),
        ("cir", &c.cir),
        ("cbs", &c.cbs),
        ("pir", &c.pir),
        ("pbs", &c.pbs),
    ])
}

fn sched_record(s: &SchedCfg) -> Records {
    Records::one(&[
        ("strict", &Flag(s.strict)),
        ("weight", &s.weight),
        ("packets", &Flag(s.packets)),
        ("min_rate", &s.min_rate),
        ("min_burst", &s.min_burst),
        ("max_rate", &s.max_rate),
        ("max_burst", &s.max_burst),
    ])
}

fn buffer_record(b: &BufferCfg) -> Records {
    Records::one(&[
        ("ingress", &Flag(b.ingress)),
        ("dynamic", &Flag(b.dynamic)),
        ("dynamic_th", &b.dynamic_th),
        ("static_th", &b.static_th),
        ("reserved", &b.reserved),
        ("xoff", &b.xoff),
        ("xon", &b.xon),
        ("xon_offset", &b.xon_offset),
        ("pool_size", &b.pool_size),
        ("pool_xoff", &b.pool_xoff),
    ])
}

/// A record, or `ABSENT`.
fn found(record: Option<Records>) -> Outcome {
    record.ok_or_else(Reply::absent)
}

/// The chip's state.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per state request of the protocol, in the header's order"
)]
fn state(name: &str, request: &Request, chip: &VirtualBackend) -> Outcome {
    let qos = chip.qos();
    let mirror = chip.mirror();
    let l3 = chip.l3();
    let fp = chip.fp();
    let hwq = |n: i64, q: i64| hw(n).zip(index8(q));
    match *request {
        Request::Count(table) => value(match table {
            Table::Stations => l3.stations.len(),
            Table::QosMaps => qos.qmaps.len(),
            Table::PortQosMaps => qos.port_qmap.len(),
            Table::QueueSchedulers => qos.queue_sched.len(),
            Table::PortSchedulers => qos.port_sched.len(),
            Table::QueueBuffers => qos.queue_buf.len(),
            Table::PgBuffers => qos.pg_buf.len(),
            Table::Mirrors => mirror.mirrors.len(),
            Table::PortMirrors => mirror.port_mirrors.len(),
            Table::Samples => mirror.samples.len(),
            Table::PortSamples => mirror.port_samples.len(),
            Table::Storm => chip.l2x().storm.len(),
            Table::Flood => chip.l2x().flood.len(),
            Table::FpGroups => fp.groups.len(),
            Table::FpEntries => fp.entries.len(),
            Table::Policers => fp.policers.len(),
        }),
        Request::Ports => {
            let mut records = Records::default();
            for (hw, port) in chip.port_states() {
                port_record(&mut records, hw, port);
            }
            Ok(records)
        }
        Request::Port(n) => found(hw(n).and_then(|hw| {
            let mut records = Records::default();
            port_record(&mut records, hw, chip.port_state(hw)?);
            Some(records)
        })),
        Request::L2AgeSeconds => value(chip.age_seconds()),
        Request::VlanExists(v) => value(Flag(vid(v).is_some_and(|vid| chip.vlans().contains_key(&vid)))),
        Request::VlanMembers(v) => {
            let mut records = Records::default();
            for (hw, untagged) in vid(v).and_then(|vid| chip.vlans().get(&vid)).into_iter().flatten() {
                records.push(&[("hw", &hw.0), ("untagged", &Flag(*untagged))]);
            }
            Ok(records)
        }
        Request::L2Entries => {
            let mut records = Records::default();
            for ((mac, vid), e) in chip.l2_table() {
                records.push(&[
                    ("mac", mac),
                    ("vid", &vid.get()),
                    ("dest", &e.dest.raw()),
                    ("static", &Flag(e.is_static)),
                    ("discard", &Flag(e.discard)),
                ]);
            }
            Ok(records)
        }
        Request::Netifs => {
            let mut records = Records::default();
            for (id, n) in chip.knet_netifs() {
                records.push(&[
                    ("id", &id.get()),
                    ("name", &Text(&n.name)),
                    ("hw", &n.hw.0),
                    ("vid", &n.vid.get()),
                    ("mac", &n.mac),
                    ("keep_tag", &Flag(n.keep_tag)),
                    ("carrier", &Flag(n.carrier)),
                    ("filter", &n.filter.get()),
                ]);
            }
            Ok(records)
        }
        Request::TrapToCpu(trap_type) => {
            let kind = sai_core::hostif::trap_kind(trap_type);
            value(Flag(kind != TrapKind::Other && chip.traps().get(&kind) == Some(&true)))
        }
        Request::L3Intfs => {
            let mut records = Records::default();
            for (id, i) in &l3.intfs {
                records.push(&[
                    ("id", &id.get()),
                    ("mac", &i.mac),
                    ("vid", &i.vid.get()),
                    ("mtu", &i.mtu),
                    ("vrf", &i.vrf.get()),
                ]);
            }
            Ok(records)
        }
        Request::EgressObjects => {
            let mut records = Records::default();
            for (id, e) in &l3.egress {
                egress_record(&mut records, *id, *e);
            }
            Ok(records)
        }
        Request::Egress(n) => found(id32(n).map(EgressId).and_then(|id| {
            let mut records = Records::default();
            egress_record(&mut records, id, *l3.egress.get(&id)?);
            Some(records)
        })),
        Request::Hosts => {
            let mut records = Records::default();
            for ((vrf, ip), egress) in &l3.hosts {
                records.push(&[("vrf", &vrf.get()), ("ip", &Ip(*ip)), ("egress", &egress.get())]);
            }
            Ok(records)
        }
        Request::HostEgress(vrf, ip) => value(or_none(
            id32(vrf).and_then(|vrf| l3.hosts.get(&(Vrf(vrf), ip))).map(|e| e.get()),
        )),
        Request::Routes => {
            let mut records = Records::default();
            for ((vrf, prefix), target) in &l3.routes {
                records.push(&[
                    ("vrf", &vrf.get()),
                    ("addr", &Ip(prefix.addr())),
                    ("mask", &Ip(prefix.mask())),
                    ("target", &route_target(*target)),
                ]);
            }
            Ok(records)
        }
        Request::RouteTarget(vrf, addr, mask) => value(or_none(
            id32(vrf)
                .zip(IpPrefix::new(addr, mask))
                .and_then(|(vrf, prefix)| l3.routes.get(&(Vrf(vrf), prefix)))
                .map(|t| route_target(*t)),
        )),
        Request::EcmpGroups => {
            let mut records = Records::default();
            for (id, members) in &l3.ecmps {
                let members: Vec<u32> = members.iter().map(|e| e.get()).collect();
                records.push(&[("id", &id.get()), ("egress", &List(&members))]);
            }
            Ok(records)
        }
        Request::LagMembers(tid) => {
            let members = id32(tid).and_then(|t| chip.lags().get(&TrunkId(t)));
            let Some(members) = members else {
                return Err(Reply::absent());
            };
            let mut records = Records::default();
            for m in members {
                records.push(&[
                    ("hw", &m.hw.0),
                    ("egress_disable", &Flag(m.egress_disable)),
                    ("ingress_disable", &Flag(m.ingress_disable)),
                ]);
            }
            Ok(records)
        }
        Request::FpGroup(gid) => found(
            id32(gid)
                .and_then(|g| fp.groups.get(&FpGroupId(g)))
                .map(|g| Records::one(&[("qset", &g.qset), ("priority", &g.priority)])),
        ),
        Request::FpEntries => {
            let mut records = Records::default();
            for (eid, e) in &fp.entries {
                fp_entry_record(&mut records, *eid, e);
            }
            Ok(records)
        }
        Request::FpEntry(eid) => found(id32(eid).map(FpEntryId).and_then(|eid| {
            let mut records = Records::default();
            fp_entry_record(&mut records, eid, fp.entries.get(&eid)?);
            Some(records)
        })),
        Request::Policer(pid) => found(
            id32(pid)
                .and_then(|p| fp.policers.get(&PolicerId(p)))
                .map(policer_record),
        ),
        Request::QosMap(id) => found(
            id32(id)
                .and_then(|i| qos.qmaps.get(&QosMapId(i)))
                .map(|(kind, entries)| {
                    let entries: Vec<String> = entries
                        .iter()
                        .map(|e| format!("{}.{}.{}.{}", e.key, e.key_color, e.value, e.value_color))
                        .collect();
                    Records::one(&[("type", &qos_type(*kind)), ("entries", &List(&entries))])
                }),
        ),
        Request::PortQosMap(n, sai_type) => value(or_none(
            hw(n)
                .zip(qos_kind(sai_type))
                .and_then(|key| qos.port_qmap.get(&key))
                .map(|id| id.get()),
        )),
        Request::PortDefaultTc(n) => value(
            hw(n)
                .and_then(|hw| qos.default_tc.get(&hw))
                .map_or(-1, |tc| i64::from(*tc)),
        ),
        Request::PortPfc(n) => found(
            hw(n)
                .and_then(|hw| qos.pfc.get(&hw))
                .map(|(separate, tx, rx)| Records::one(&[("separate", &Flag(*separate)), ("tx", tx), ("rx", rx)])),
        ),
        Request::QueueScheduler(n, q) => found(hwq(n, q).and_then(|k| qos.queue_sched.get(&k)).map(sched_record)),
        Request::PortScheduler(n) => found(hw(n).and_then(|hw| qos.port_sched.get(&hw)).map(sched_record)),
        Request::Wred(id) => found(id32(id).and_then(|i| qos.wreds.get(&WredId(i))).map(|w| {
            let enable = w.enable.map(Flag);
            Records::one(&[
                ("enable", &List(&enable)),
                ("min", &List(&w.min)),
                ("max", &List(&w.max)),
                ("prob", &List(&w.prob)),
                ("weight", &w.weight),
                ("ecn", &w.ecn),
            ])
        })),
        Request::QueueWred(n, q) => value(or_none(
            hwq(n, q).and_then(|k| qos.queue_wred.get(&k)).map(|id| id.get()),
        )),
        Request::QueueBuffer(n, q) => found(hwq(n, q).and_then(|k| qos.queue_buf.get(&k)).map(buffer_record)),
        Request::PgBuffer(n, q) => found(hwq(n, q).and_then(|k| qos.pg_buf.get(&k)).map(buffer_record)),
        Request::Mirror(id) => found(id32(id).and_then(|i| mirror.mirrors.get(&MirrorId(i))).map(|m| {
            Records::one(&[
                ("erspan", &Flag(m.erspan)),
                ("monitor", &m.monitor.0),
                ("truncate", &m.truncate),
                ("sample_rate", &m.sample_rate),
                ("tos", &m.tos),
                ("ttl", &m.ttl),
                ("src", &Ip(m.src)),
                ("dst", &Ip(m.dst)),
                ("smac", &m.smac),
                ("dmac", &m.dmac),
                ("gre_type", &m.gre_type),
                ("vlan", &Flag(m.vlan)),
                ("vlan_tpid", &m.vlan_tpid),
                ("vlan_id", &m.vlan_id),
                ("vlan_pri", &m.vlan_pri),
                ("vlan_cfi", &m.vlan_cfi),
            ])
        })),
        Request::PortMirrors(n, ingress) => {
            let ids: Vec<u32> = hw(n)
                .and_then(|hw| mirror.port_mirrors.get(&(hw, ingress)))
                .into_iter()
                .flatten()
                .map(|id| id.get())
                .collect();
            value(List(&ids))
        }
        Request::SampleRate(id) => value(
            id32(id)
                .and_then(|i| mirror.samples.get(&SampleId(i)))
                .copied()
                .unwrap_or(0),
        ),
        Request::PortSample(n, ingress) => value(or_none(
            hw(n)
                .and_then(|hw| mirror.port_samples.get(&(hw, ingress)))
                .map(|id| id.get()),
        )),
        Request::Genetlink(id) => found(id32(id).and_then(|i| mirror.genls.get(&GenlId(i))).map(|g| {
            Records::one(&[
                ("name", &Text(&g.name)),
                ("mcgrp", &Text(&g.mcgrp)),
                ("traps", &List(&g.traps)),
            ])
        })),
        Request::Hash(lag) => found(
            chip.l2x()
                .hash
                .get(&lag)
                .map(|(fields, seed)| Records::one(&[("fields", &List(fields)), ("seed", seed)])),
        ),
        Request::PortStorm(n, attr) => value(or_none(
            hw(n)
                .zip(storm_kind(attr))
                .and_then(|key| chip.l2x().storm.get(&key))
                .map(|id| id.get()),
        )),
        Request::VlanFlood(v, attr) => value(
            vid(v)
                .zip(flood_kind(attr))
                .and_then(|key| chip.l2x().flood.get(&key))
                .map_or(-1, |on| i64::from(*on)),
        ),
        _ => Err(bad(name, "not a request of the virtual backend (test port bug)")),
    }
}

const fn route_target(target: RouteTarget) -> u32 {
    match target {
        RouteTarget::Egress(id) => id.get(),
        RouteTarget::Ecmp(id) => id.get(),
    }
}

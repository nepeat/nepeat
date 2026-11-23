//! Mirror sessions (`LOCAL` = SPAN, `ENHANCED_REMOTE` = ERSPAN with L3 GRE), `SAMPLEPACKET`
//! (`SLOW_PATH`), and the port attributes that use them (`INGRESS` / `EGRESS_MIRROR_SESSION`,
//! `INGRESS` / `EGRESS_SAMPLEPACKET_ENABLE`). With [`crate::backend::Mirror::mirror_supported`]
//! every change goes through the mirror and sample operations; otherwise the objects are kept
//! here only and the port attributes are "not supported" (as before they existed). ACL entries
//! mirror through `FpRule::mirror_*` ([`crate::acl`], with [`mirror_valid`] and
//! [`mirror_backend_id`]). Ports `sai_mirror.cpp` (390 lines).
//!
//! State: [`MirrorSession`] and [`Samplepacket`] are typed objects; a port's sessions and samplers
//! are in [`crate::model::Port::kept`] under their attribute ids, so a session or sampler a port
//! names is in use (`OBJECT_IN_USE`) by the store's count, as is a session an ACL entry's mirror
//! action names (when the ACL area keeps it as a reference).
//!
//! Backend: [`crate::backend::Mirror`], [`crate::backend::Sample`].
//!
//! Registration: [`crate::dispatch::Owner::Mirror`] for `MIRROR_SESSION` and `SAMPLEPACKET` in
//! [`crate::dispatch::OWNERS`]; the port attributes through [`port_mirror_set`] /
//! [`port_mirror_get`] (called by the port code after the `QoS` and storm handlers).
use crate::backend::types::{MacAddr, MirrorCfg, MirrorId as HwMirrorId, SampleId as HwSampleId};
use crate::backend::{Backend, HwPort};
use crate::dispatch::{Area, Cx};
use crate::error::{Error, Result};
use crate::model::Port;
use crate::oid::{MirrorSessionId, ObjectType, PortId, RawOid, SamplepacketId, TypedId};
use crate::qos::{
    area_kind, backend_attr_error, create_arg, front, invalid, keep_on_port, kept_list, kept_oid, not_supported,
};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr};

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct MirrorArea;

impl Area for MirrorArea {
    /// [`refused_set`] (sessions and samplers).
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        refused_set(object_type, attr)
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        match object_type {
            ObjectType::MirrorSession => create_session(cx.backend, cx.store, attrs),
            ObjectType::Samplepacket => create_samplepacket(cx.backend, cx.store, attrs),
            _ => Err(Error::NotSupported),
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        match object_type {
            ObjectType::MirrorSession => {
                let id = MirrorSessionId::try_from(id)?;
                cx.store.ensure_removable::<MirrorSession>(id)?;
                if let Some(hw) = cx.store.get::<MirrorSession>(id)?.hw {
                    cx.backend.mirror_destroy(hw)?;
                }
                cx.store.remove::<MirrorSession>(id).map(drop)
            }
            ObjectType::Samplepacket => {
                let id = SamplepacketId::try_from(id)?;
                cx.store.ensure_removable::<Samplepacket>(id)?;
                if let Some(hw) = cx.store.get::<Samplepacket>(id)?.hw {
                    cx.backend.sample_destroy(hw)?;
                }
                cx.store.remove::<Samplepacket>(id).map(drop)
            }
            _ => Err(Error::NotSupported),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        match object_type {
            ObjectType::MirrorSession => set_session(cx.backend, cx.store, MirrorSessionId::try_from(id)?, attr),
            ObjectType::Samplepacket => set_samplepacket(cx.backend, cx.store, SamplepacketId::try_from(id)?, attr),
            _ => Err(not_supported(0)),
        }
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = backend;
        match object_type {
            ObjectType::MirrorSession => {
                get_session(store.get::<MirrorSession>(MirrorSessionId::try_from(id)?)?, meta.id)
            }
            ObjectType::Samplepacket => {
                get_samplepacket(store.get::<Samplepacket>(SamplepacketId::try_from(id)?)?, meta.id)
            }
            _ => Err(not_supported(0)),
        }
    }
}

/// This area's typed objects, as the store holds them ([`crate::model::Object::Mirror`]). They
/// refer to nothing: the monitor port is never removed, and the C++ does not count a session's
/// policer as in use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MirrorObject {
    /// A `MIRROR_SESSION`.
    Session(MirrorSession),
    /// A `SAMPLEPACKET`.
    Sample(Samplepacket),
}

impl MirrorObject {
    /// The object's type.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::Session(_) => ObjectType::MirrorSession,
            Self::Sample(_) => ObjectType::Samplepacket,
        }
    }
}

// ------------------------------------------------------------------ mirror sessions

/// The session types the C++ takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionType {
    /// SPAN to a local port.
    Local,
    /// ERSPAN: L3 GRE to a remote collector.
    EnhancedRemote,
}

/// A `MIRROR_SESSION` (C++ `MirrorObj`). `TC`, `CONGESTION_MODE` and `POLICER` are kept, not
/// programmed (mirror copies use the queue the SDK picks and are not policed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MirrorSession {
    kind: SessionType,
    monitor: PortId,
    policer: RawOid,
    cfg: MirrorCfg,
    tc: u8,
    congestion: i32,
    hw: Option<HwMirrorId>,
}

area_kind!(MirrorSession, MirrorSessionId, Mirror, MirrorObject::Session);

impl MirrorSession {
    /// SPAN or ERSPAN.
    pub const fn kind(&self) -> SessionType {
        self.kind
    }
    /// What the chip gets.
    pub const fn cfg(&self) -> &MirrorCfg {
        &self.cfg
    }
    /// The backend's session, `None` while it is kept only.
    pub const fn hw(&self) -> Option<HwMirrorId> {
        self.hw
    }
}

/// A session being built by a create (type and monitor port mandatory).
#[derive(Clone, Debug)]
struct SessionArgs {
    kind: Option<SessionType>,
    monitor: Option<PortId>,
    policer: RawOid,
    cfg: MirrorCfg,
    tc: u8,
    congestion: i32,
}

impl From<&MirrorSession> for SessionArgs {
    fn from(m: &MirrorSession) -> Self {
        Self {
            kind: Some(m.kind),
            monitor: Some(m.monitor),
            policer: m.policer,
            cfg: m.cfg,
            tc: m.tc,
            congestion: m.congestion,
        }
    }
}

/// The C++ `MirrorCfg` defaults (monitor set by the mandatory `MONITOR_PORT`).
const CFG_DEFAULT: MirrorCfg = MirrorCfg {
    erspan: false,
    monitor: HwPort(0),
    truncate: 0,
    sample_rate: 0,
    tos: 0,
    ttl: 255,
    src: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    dst: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    smac: MacAddr([0; 6]),
    dmac: MacAddr([0; 6]),
    gre_type: 0x88be,
    vlan: false,
    vlan_tpid: 0x8100,
    vlan_id: 0,
    vlan_pri: 0,
    vlan_cfi: 0,
};

/// The chip port of a front-panel port id (a LAG or the CPU port is no monitor port).
fn front_port(store: &Store, oid: RawOid) -> Option<(PortId, HwPort)> {
    let id = PortId::try_from(oid).ok()?;
    store.get::<Port>(id).ok().and_then(front).map(|hw| (id, hw))
}

impl SessionArgs {
    /// One attribute (C++ `mirror_attr`).
    fn apply(&mut self, store: &Store, attr: &Attr, index: usize, create: bool) -> Result<()> {
        let c = &mut self.cfg;
        match (attr.id, &attr.value) {
            (sys::SAI_MIRROR_SESSION_ATTR_TYPE, Value::Enum(t)) => {
                if !create {
                    return Err(invalid(index));
                }
                let kind = match *t {
                    t if t == sys::SAI_MIRROR_SESSION_TYPE_LOCAL.cast_signed() => SessionType::Local,
                    t if t == sys::SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE.cast_signed() => SessionType::EnhancedRemote,
                    // RSPAN, sFlow-type sessions: not implemented
                    _ => return Err(not_supported(index)),
                };
                c.erspan = kind == SessionType::EnhancedRemote;
                self.kind = Some(kind);
            }
            (sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT, Value::Oid(port)) => {
                let (id, hw) = front_port(store, *port).ok_or(invalid(index))?;
                self.monitor = Some(id);
                c.monitor = hw;
            }
            (sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE, Value::U16(v)) => c.truncate = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_SAMPLE_RATE, Value::U32(v)) => c.sample_rate = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_CONGESTION_MODE, Value::Enum(v)) => self.congestion = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_TC, Value::U8(v)) => self.tc = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_VLAN_TPID, Value::U16(v)) => c.vlan_tpid = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_VLAN_ID, Value::U16(v)) => c.vlan_id = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_VLAN_PRI, Value::U8(v)) => c.vlan_pri = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_VLAN_CFI, Value::U8(v)) => c.vlan_cfi = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_VLAN_HEADER_VALID, Value::Bool(v)) => c.vlan = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE, Value::Enum(t)) => {
                if !create || *t != sys::SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL.cast_signed() {
                    return Err(invalid(index));
                }
            }
            // Follows the addresses.
            (sys::SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION, Value::U8(v)) => {
                if *v != 4 && *v != 6 {
                    return Err(invalid(index));
                }
            }
            (sys::SAI_MIRROR_SESSION_ATTR_TOS, Value::U8(v)) => c.tos = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_TTL, Value::U8(v)) => c.ttl = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS, Value::IpAddress(ip)) => c.src = *ip,
            (sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS, Value::IpAddress(ip)) => c.dst = *ip,
            (sys::SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS, Value::Mac(mac)) => c.smac = MacAddr(*mac),
            (sys::SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS, Value::Mac(mac)) => c.dmac = MacAddr(*mac),
            (sys::SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE, Value::U16(v)) => c.gre_type = *v,
            (sys::SAI_MIRROR_SESSION_ATTR_POLICER, Value::Oid(policer)) => {
                if !policer.is_null() && !crate::acl::policer_valid(store, *policer) {
                    return Err(invalid(index));
                }
                self.policer = *policer;
            }
            _ => return Err(not_supported(index)),
        }
        Ok(())
    }

    fn session(self, hw: Option<HwMirrorId>) -> Option<MirrorSession> {
        Some(MirrorSession {
            kind: self.kind?,
            monitor: self.monitor?,
            policer: self.policer,
            cfg: self.cfg,
            tc: self.tc,
            congestion: self.congestion,
            hw,
        })
    }
}

fn create_session<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut args = SessionArgs {
        kind: None,
        monitor: None,
        policer: RawOid::NULL,
        cfg: CFG_DEFAULT,
        tc: 0,
        congestion: sys::SAI_MIRROR_SESSION_CONGESTION_MODE_INDEPENDENT.cast_signed(),
    };
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::MirrorSession, attr, index)?;
        args.apply(store, attr, index, true)?;
    }
    if args.kind.is_none() || args.monitor.is_none() {
        return Err(Error::MandatoryAttributeMissing);
    }
    let hw = if backend.mirror_supported() {
        Some(backend.mirror_create(&args.cfg)?)
    } else {
        None
    };
    let session = args.session(hw).ok_or(Error::Failure)?;
    store.insert(session).map(TypedId::raw)
}

fn set_session<B: Backend>(backend: &mut B, store: &mut Store, id: MirrorSessionId, attr: &Attr) -> Result<()> {
    let session = store.get::<MirrorSession>(id)?;
    let hw = session.hw;
    let mut args = SessionArgs::from(session);
    args.apply(store, attr, 0, false)?;
    if let Some(hw) = hw {
        backend
            .mirror_update(hw, &args.cfg)
            .map_err(|e| backend_attr_error(e, 0))?;
    }
    let changed = args.session(hw).ok_or(Error::Failure)?;
    store.update::<MirrorSession, _>(id, |m| *m = changed)
}

fn get_session(m: &MirrorSession, attr: sys::sai_attr_id_t) -> Result<Value> {
    let c = &m.cfg;
    Ok(match attr {
        sys::SAI_MIRROR_SESSION_ATTR_TYPE => Value::Enum(
            match m.kind {
                SessionType::Local => sys::SAI_MIRROR_SESSION_TYPE_LOCAL,
                SessionType::EnhancedRemote => sys::SAI_MIRROR_SESSION_TYPE_ENHANCED_REMOTE,
            }
            .cast_signed(),
        ),
        sys::SAI_MIRROR_SESSION_ATTR_MONITOR_PORT => Value::Oid(m.monitor.raw()),
        sys::SAI_MIRROR_SESSION_ATTR_TRUNCATE_SIZE => Value::U16(c.truncate),
        sys::SAI_MIRROR_SESSION_ATTR_SAMPLE_RATE => Value::U32(c.sample_rate),
        sys::SAI_MIRROR_SESSION_ATTR_CONGESTION_MODE => Value::Enum(m.congestion),
        sys::SAI_MIRROR_SESSION_ATTR_TC => Value::U8(m.tc),
        sys::SAI_MIRROR_SESSION_ATTR_VLAN_TPID => Value::U16(c.vlan_tpid),
        sys::SAI_MIRROR_SESSION_ATTR_VLAN_ID => Value::U16(c.vlan_id),
        sys::SAI_MIRROR_SESSION_ATTR_VLAN_PRI => Value::U8(c.vlan_pri),
        sys::SAI_MIRROR_SESSION_ATTR_VLAN_CFI => Value::U8(c.vlan_cfi),
        sys::SAI_MIRROR_SESSION_ATTR_VLAN_HEADER_VALID => Value::Bool(c.vlan),
        sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE => {
            Value::Enum(sys::SAI_ERSPAN_ENCAPSULATION_TYPE_MIRROR_L3_GRE_TUNNEL.cast_signed())
        }
        sys::SAI_MIRROR_SESSION_ATTR_IPHDR_VERSION => Value::U8(if c.dst.is_ipv6() { 6 } else { 4 }),
        sys::SAI_MIRROR_SESSION_ATTR_TOS => Value::U8(c.tos),
        sys::SAI_MIRROR_SESSION_ATTR_TTL => Value::U8(c.ttl),
        sys::SAI_MIRROR_SESSION_ATTR_SRC_IP_ADDRESS => Value::IpAddress(c.src),
        sys::SAI_MIRROR_SESSION_ATTR_DST_IP_ADDRESS => Value::IpAddress(c.dst),
        sys::SAI_MIRROR_SESSION_ATTR_SRC_MAC_ADDRESS => Value::Mac(c.smac.0),
        sys::SAI_MIRROR_SESSION_ATTR_DST_MAC_ADDRESS => Value::Mac(c.dmac.0),
        sys::SAI_MIRROR_SESSION_ATTR_GRE_PROTOCOL_TYPE => Value::U16(c.gre_type),
        sys::SAI_MIRROR_SESSION_ATTR_POLICER => Value::Oid(m.policer),
        _ => return Err(not_supported(0)),
    })
}

/// What a set of an attribute the metadata does not let a set carry answers, as the C++
/// (through [`crate::qos::refused_set`]): a session's `TYPE` and `ERSPAN_ENCAPSULATION_TYPE`
/// "invalid value", its other ones "not supported"; anything but a sampler's `SAMPLE_RATE`
/// "invalid value". `None`: another type.
pub(crate) fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
    Some(match (object_type, attr) {
        (
            ObjectType::MirrorSession,
            sys::SAI_MIRROR_SESSION_ATTR_TYPE | sys::SAI_MIRROR_SESSION_ATTR_ERSPAN_ENCAPSULATION_TYPE,
        )
        | (ObjectType::Samplepacket, _) => invalid(0),
        (ObjectType::MirrorSession, _) => not_supported(0),
        _ => return None,
    })
}

/// Whether `session` is a mirror session (C++ `mirror_valid`; ACL mirror actions).
pub(crate) fn mirror_valid(store: &Store, session: RawOid) -> bool {
    MirrorSessionId::try_from(session).is_ok_and(|id| store.get::<MirrorSession>(id).is_ok())
}

/// The backend's id of a session, `None` when there is none or it is not programmed (C++
/// `mirror_backend_id` = -1; ACL mirror actions).
pub(crate) fn mirror_backend_id(store: &Store, session: RawOid) -> Option<HwMirrorId> {
    MirrorSessionId::try_from(session)
        .ok()
        .and_then(|id| store.get::<MirrorSession>(id).ok())
        .and_then(|m| m.hw)
}

// ------------------------------------------------------------------ samplepacket

/// `SAI_SAMPLEPACKET_MODE_*`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleMode {
    /// The default.
    Exclusive,
    /// Shared.
    Shared,
}

/// A `SAMPLEPACKET` (C++ `SampleObj`): `SLOW_PATH` only, 1 in `rate` frames to the CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Samplepacket {
    rate: u32,
    mode: SampleMode,
    hw: Option<HwSampleId>,
}

area_kind!(Samplepacket, SamplepacketId, Mirror, MirrorObject::Sample);

impl Samplepacket {
    /// The rate.
    pub const fn rate(&self) -> u32 {
        self.rate
    }
    /// The backend's sampler, `None` while it is kept only.
    pub const fn hw(&self) -> Option<HwSampleId> {
        self.hw
    }
}

fn create_samplepacket<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut rate = None;
    let mut mode = SampleMode::Exclusive;
    for (index, attr) in attrs.iter().enumerate() {
        create_arg(ObjectType::Samplepacket, attr, index)?;
        match (attr.id, &attr.value) {
            (sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, Value::U32(r)) => {
                if *r == 0 {
                    return Err(invalid(index));
                }
                rate = Some(*r);
            }
            (sys::SAI_SAMPLEPACKET_ATTR_TYPE, Value::Enum(t)) => {
                if *t != sys::SAI_SAMPLEPACKET_TYPE_SLOW_PATH.cast_signed() {
                    return Err(not_supported(index));
                }
            }
            (sys::SAI_SAMPLEPACKET_ATTR_MODE, Value::Enum(m)) => {
                mode = match *m {
                    m if m == sys::SAI_SAMPLEPACKET_MODE_EXCLUSIVE.cast_signed() => SampleMode::Exclusive,
                    m if m == sys::SAI_SAMPLEPACKET_MODE_SHARED.cast_signed() => SampleMode::Shared,
                    _ => return Err(invalid(index)),
                };
            }
            _ => return Err(not_supported(index)),
        }
    }
    let rate = rate.ok_or(Error::MandatoryAttributeMissing)?;
    let hw = if backend.mirror_supported() {
        Some(backend.sample_create(rate)?)
    } else {
        None
    };
    store.insert(Samplepacket { rate, mode, hw }).map(TypedId::raw)
}

fn set_samplepacket<B: Backend>(backend: &mut B, store: &mut Store, id: SamplepacketId, attr: &Attr) -> Result<()> {
    let sample = store.get::<Samplepacket>(id)?;
    // TYPE, MODE: create only.
    let (sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE, Value::U32(rate)) = (attr.id, &attr.value) else {
        return Err(invalid(0));
    };
    if *rate == 0 {
        return Err(invalid(0));
    }
    if let Some(hw) = sample.hw {
        backend.sample_update(hw, *rate).map_err(|e| backend_attr_error(e, 0))?;
    }
    store.update::<Samplepacket, _>(id, |s| s.rate = *rate)
}

fn get_samplepacket(s: &Samplepacket, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_SAMPLEPACKET_ATTR_SAMPLE_RATE => Value::U32(s.rate),
        sys::SAI_SAMPLEPACKET_ATTR_TYPE => Value::Enum(sys::SAI_SAMPLEPACKET_TYPE_SLOW_PATH.cast_signed()),
        sys::SAI_SAMPLEPACKET_ATTR_MODE => Value::Enum(
            match s.mode {
                SampleMode::Exclusive => sys::SAI_SAMPLEPACKET_MODE_EXCLUSIVE,
                SampleMode::Shared => sys::SAI_SAMPLEPACKET_MODE_SHARED,
            }
            .cast_signed(),
        ),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ port attributes

/// The port's mirror and sample attributes (C++ `port_mirror_set`). `None`: not one of them, or
/// a backend without mirroring (the caller then answers "not supported", as before they existed).
pub(crate) fn port_mirror_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    port: PortId,
    attr: &Attr,
) -> Option<Result<()>> {
    if !backend.mirror_supported() {
        return None;
    }
    let ingress = match attr.id {
        sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION | sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE => true,
        sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION | sys::SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE => false,
        _ => return None,
    };
    Some(set_port_mirror(backend, store, port, attr, ingress))
}

fn set_port_mirror<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    port: PortId,
    attr: &Attr,
    ingress: bool,
) -> Result<()> {
    let hw = front(store.get::<Port>(port)?).ok_or(not_supported(0))?;
    match &attr.value {
        Value::OidList(sessions) => {
            if !sessions.iter().all(|s| mirror_valid(store, *s)) {
                return Err(invalid(0));
            }
            let ids: Vec<HwMirrorId> = sessions.iter().filter_map(|s| mirror_backend_id(store, *s)).collect();
            backend
                .port_mirror_set(hw, ingress, &ids)
                .map_err(|e| backend_attr_error(e, 0))?;
        }
        Value::Oid(sample) => {
            let id = if sample.is_null() {
                None
            } else {
                let s = SamplepacketId::try_from(*sample)
                    .ok()
                    .and_then(|id| store.get::<Samplepacket>(id).ok())
                    .ok_or(invalid(0))?;
                s.hw
            };
            backend
                .port_sample_set(hw, ingress, id)
                .map_err(|e| backend_attr_error(e, 0))?;
        }
        _ => return Err(Error::Failure),
    }
    keep_on_port(store, port, attr)
}

/// Gets of the port's mirror and sample attributes (C++ `port_mirror_get`). `None`: not one, or
/// a backend without mirroring.
pub(crate) fn port_mirror_get<B: Backend>(
    backend: &B,
    store: &Store,
    port: PortId,
    attr: sys::sai_attr_id_t,
) -> Option<Result<Value>> {
    if !backend.mirror_supported() {
        return None;
    }
    let p = match store.get::<Port>(port) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    Some(Ok(match attr {
        sys::SAI_PORT_ATTR_INGRESS_MIRROR_SESSION | sys::SAI_PORT_ATTR_EGRESS_MIRROR_SESSION => {
            Value::OidList(kept_list(&p.kept, attr))
        }
        sys::SAI_PORT_ATTR_INGRESS_SAMPLEPACKET_ENABLE | sys::SAI_PORT_ATTR_EGRESS_SAMPLEPACKET_ENABLE => {
            Value::Oid(kept_oid(&p.kept, attr))
        }
        _ => return None,
    }))
}

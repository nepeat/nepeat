//! LAGs: SDK trunks, the members' egress / ingress disable, and the replay of bridge port,
//! VLAN, PVID, learning and router interface state on a port when it joins or leaves (LACP runs
//! in the host: teamd on the members' own netdevs). Ports `sai_lag.cpp` (375 lines), with its
//! "attachment" helpers (`attach_valid`, `attach_dest`, `attach_phys`, `attach_pvid_set`): what a bridge
//! port or a router interface sits on, a port or a LAG.
//!
//! - A member is a front-panel port in no other LAG, without a bridge port or router interface
//!   of its own (orchagent removes the default bridge port and its VLAN 1 membership first).
//! - Creating a member programs the whole member list (`lag_set_members`), then replays the LAG's
//!   bridge port ([`crate::l2::bridge_port_member_join`]), router interface
//!   ([`crate::l3::rif_member_join`]) and PVID on the port; a failure undoes the joins and the member
//!   list. Removing one undoes them and puts the port back on PVID 1.
//! - The LAG's PVID goes to every member port (it is not programmed while there is none).
//! - `INGRESS_ACL` binds through the ACL area ([`crate::acl::acl_bind_set`]); `EGRESS_ACL` only as null.
//!
//! Backend: [`crate::backend::Lag`], [`crate::backend::PortOps::port_pvid_set`] (and, through the
//! replays, [`crate::backend::PortOps::port_learn_set`] and the VLAN operations).
//!
//! Registration: [`crate::dispatch::Owner::Lag`] for `LAG` and `LAG_MEMBER`.
use crate::attrs::CreateArgs;
use crate::backend::types::{Dest, LagMember as TrunkMember, TrunkId};
use crate::backend::{Backend, HwPort, Vid};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::l2::{self, Stub};
use crate::log;
use crate::model::{Kept, Port, PortKind};
use crate::oid::{LagId, LagMemberId, ObjectType, PortId, RawOid, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// What a bridge port or a router interface sits on: a port or a LAG (the C++ "attachment").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Attach {
    /// A port (the CPU port included, where the caller allows it).
    Port(PortId),
    /// A LAG.
    Lag(LagId),
}

impl Attach {
    /// The id as the C ABI carries it.
    pub fn raw(self) -> RawOid {
        match self {
            Self::Port(port) => port.raw(),
            Self::Lag(lag) => lag.raw(),
        }
    }

    /// The attachment a raw id names by its type bits (existence is not checked): `None` for an
    /// id of any other type.
    pub fn decode(raw: RawOid) -> Option<Self> {
        PortId::try_from(raw)
            .map(Self::Port)
            .or_else(|_| LagId::try_from(raw).map(Self::Lag))
            .ok()
    }
}

/// A LAG: an SDK trunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lag {
    /// The trunk.
    pub tid: TrunkId,
    /// `PORT_VLAN_ID`: what every member port gets (a router interface on the LAG sets it too).
    pub pvid: Vid,
    /// Kept attributes.
    pub kept: Kept,
}

/// A port's membership of a LAG. A LAG's members are its `LagMember`s in id order, which is
/// creation order (the order of the C++ member list and of `PORT_LIST`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LagMember {
    /// The LAG.
    pub lag: LagId,
    /// The port.
    pub port: PortId,
    /// `EGRESS_DISABLE`: not in the distribution (hash) set.
    pub egress_disable: bool,
    /// `INGRESS_DISABLE`: frames received on the port are not the LAG's.
    pub ingress_disable: bool,
    /// Kept attributes.
    pub kept: Kept,
}

/// LAG attributes kept without effect, with what a get answers while unset (C++ `lag_stubs`).
const LAG_STUBS: &[Stub] = &[
    (sys::SAI_LAG_ATTR_DEFAULT_VLAN_PRIORITY, Value::U8(0)),
    (sys::SAI_LAG_ATTR_DROP_UNTAGGED, Value::Bool(false)),
    (sys::SAI_LAG_ATTR_DROP_TAGGED, Value::Bool(false)),
    (sys::SAI_LAG_ATTR_TPID, Value::U16(0x8100)),
];
/// LAG member attributes kept without effect (C++ `member_stubs`).
const MEMBER_STUBS: &[Stub] = &[(sys::SAI_LAG_MEMBER_ATTR_WEIGHT, Value::U32(1))];

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct LagArea;

impl Area for LagArea {
    /// C++ `set_lag_attribute` (`PORT_LIST`), `set_lag_member_attribute` (`LAG_ID`, `PORT_ID`),
    /// `sai_lag.cpp:174`-325.
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        let invalid: &[sys::sai_attr_id_t] = match object_type {
            ObjectType::Lag => &[sys::SAI_LAG_ATTR_PORT_LIST],
            _ => &[sys::SAI_LAG_MEMBER_ATTR_LAG_ID, sys::SAI_LAG_MEMBER_ATTR_PORT_ID],
        };
        Some(crate::dispatch::cpp_refusal(attr, invalid))
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        match object_type {
            ObjectType::Lag => create_lag(cx.backend, cx.store, attrs).map(TypedId::raw),
            ObjectType::LagMember => create_member(cx.backend, cx.store, attrs).map(TypedId::raw),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        match object_type {
            ObjectType::Lag => remove_lag(cx.backend, cx.store, LagId::try_from(id)?),
            ObjectType::LagMember => remove_member(cx.backend, cx.store, LagMemberId::try_from(id)?),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        match object_type {
            ObjectType::Lag => set_lag(cx.backend, cx.store, LagId::try_from(id)?, attr),
            ObjectType::LagMember => set_member(cx.backend, cx.store, LagMemberId::try_from(id)?, attr),
            _ => Err(Error::InvalidObjectType),
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
            ObjectType::Lag => get_lag(store, LagId::try_from(id)?, meta.id),
            ObjectType::LagMember => get_member(store, LagMemberId::try_from(id)?, meta.id),
            _ => Err(Error::InvalidObjectType),
        }
    }
}

// ------------------------------------------------------------------ attachments

/// The members of a LAG, in creation order.
pub(crate) fn members(store: &Store, lag: LagId) -> impl Iterator<Item = (LagMemberId, &LagMember)> {
    store.iter::<LagMember>().filter(move |(_, m)| m.lag == lag)
}

/// The LAG a port is a member of (test port `port_lag`).
pub fn port_lag(store: &Store, port: PortId) -> Option<LagId> {
    store
        .iter::<LagMember>()
        .find(|(_, m)| m.port == port)
        .map(|(_, m)| m.lag)
}

/// Whether `raw` can carry a bridge port or a router interface: a LAG, or a front-panel port
/// that is in no LAG (C++ `attach_valid`). Not the CPU port, a LAG member, an id of another type
/// or one that names nothing.
pub(crate) fn attach_valid(store: &Store, raw: RawOid) -> bool {
    attach_of(store, raw).is_some()
}

/// [`attach_valid`] as the typed attachment.
pub(crate) fn attach_of(store: &Store, raw: RawOid) -> Option<Attach> {
    match Attach::decode(raw)? {
        Attach::Lag(lag) => store.get::<Lag>(lag).ok().map(|_| Attach::Lag(lag)),
        Attach::Port(port) => {
            let front = matches!(store.get::<Port>(port).ok()?.kind, PortKind::Front { .. });
            (front && port_lag(store, port).is_none()).then_some(Attach::Port(port))
        }
    }
}

/// Where traffic to a port (the CPU port too) or a LAG goes: the port or the LAG's trunk (C++
/// `attach_dest`). `None` for an id that names neither.
pub(crate) fn attach_dest(store: &Store, raw: RawOid) -> Option<Dest> {
    match Attach::decode(raw)? {
        Attach::Lag(lag) => store.get::<Lag>(lag).ok().map(|l| Dest::Lag(l.tid)),
        Attach::Port(port) => store.get::<Port>(port).ok().map(|p| Dest::Port(p.hw)),
    }
}

/// The chip ports of a port or LAG: the port, or the LAG's member ports in creation order (C++
/// `attach_phys`); none for an id that names neither.
pub(crate) fn attach_phys(store: &Store, raw: RawOid) -> Vec<HwPort> {
    Attach::decode(raw).map_or_else(Vec::new, |attach| {
        attach_ports(store, attach).into_iter().map(|(_, hw)| hw).collect()
    })
}

/// [`attach_phys`] with the port ids.
fn attach_ports(store: &Store, attach: Attach) -> Vec<(PortId, HwPort)> {
    let hw_of = |port: PortId| store.get::<Port>(port).ok().map(|p| (port, p.hw));
    match attach {
        Attach::Lag(lag) => members(store, lag).filter_map(|(_, m)| hw_of(m.port)).collect(),
        Attach::Port(port) => hw_of(port).into_iter().collect(),
    }
}

/// Sets the PVID of a port or LAG (C++ `attach_pvid_set`). A LAG keeps it whatever its members
/// answer, and every member port is tried: the error is the last member's that failed (the
/// C++ behaviour: neither the LAG's PVID nor the members that took it are put back). A port:
/// programmed, then kept. `InvalidParameter` for an id that names neither.
pub(crate) fn attach_pvid_set<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid, vid: Vid) -> Result<()> {
    let attach = Attach::decode(raw).ok_or(Error::InvalidParameter)?;
    match attach {
        Attach::Lag(lag) => {
            store
                .update::<Lag, _>(lag, |l| l.pvid = vid)
                .map_err(|_| Error::InvalidParameter)?;
            let mut result = Ok(());
            for (port, hw) in attach_ports(store, attach) {
                match backend.port_pvid_set(hw, vid) {
                    Ok(()) => store.update::<Port, _>(port, |p| p.pvid = vid)?,
                    Err(e) => result = Err(e),
                }
            }
            result
        }
        Attach::Port(port) => {
            let hw = store.get::<Port>(port).map_err(|_| Error::InvalidParameter)?.hw;
            backend.port_pvid_set(hw, vid)?;
            store.update::<Port, _>(port, |p| p.pvid = vid)
        }
    }
}

/// A change to a LAG's member list that is not stored yet.
#[derive(Clone, Copy)]
enum Change {
    /// The list as stored.
    None,
    /// A new member at the end.
    Add(TrunkMember),
    /// Without this member.
    Remove(LagMemberId),
    /// This member with other flags: egress disable, ingress disable.
    Flags(LagMemberId, bool, bool),
}

/// The trunk member list of a LAG with one change applied (C++ `sdk_members`).
fn trunk_members(store: &Store, lag: LagId, change: Change) -> Vec<TrunkMember> {
    let mut list: Vec<TrunkMember> = members(store, lag)
        .filter(|(id, _)| !matches!(change, Change::Remove(removed) if removed == *id))
        .filter_map(|(id, m)| {
            let hw = store.get::<Port>(m.port).ok()?.hw;
            let (egress_disable, ingress_disable) = match change {
                Change::Flags(changed, egress, ingress) if changed == id => (egress, ingress),
                _ => (m.egress_disable, m.ingress_disable),
            };
            Some(TrunkMember {
                hw,
                egress_disable,
                ingress_disable,
            })
        })
        .collect();
    if let Change::Add(member) = change {
        list.push(member);
    }
    list
}

// --------------------------------------------------------------------------- LAG

fn create_lag<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<LagId> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_LAG, attrs)?;
    let mut pvid = Vid::DEFAULT;
    let mut kept = Kept::new();
    let mut ingress_acl = RawOid::NULL;
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(vid)) => {
                pvid = Vid::new(*vid).map_err(|_| Error::attr(AttrError::InvalidValue, index))?;
            }
            // Bound once the LAG exists, below.
            (sys::SAI_LAG_ATTR_INGRESS_ACL | sys::SAI_LAG_ATTR_EGRESS_ACL, Value::Oid(acl)) => {
                if attr.id == sys::SAI_LAG_ATTR_EGRESS_ACL && !acl.is_null() {
                    return Err(Error::attr(AttrError::NotSupported, index));
                }
                if !acl.is_null() && !store.contains(*acl) {
                    return Err(Error::attr(AttrError::InvalidValue, index));
                }
                if attr.id == sys::SAI_LAG_ATTR_INGRESS_ACL {
                    ingress_acl = *acl;
                }
            }
            _ => l2::stub_set(LAG_STUBS, &mut kept, attr, index)?,
        }
    }
    let tid = backend.lag_create()?;
    let id = store.insert(Lag { tid, pvid, kept })?;
    if !ingress_acl.is_null() {
        // As the C++: the LAG exists whatever the binding answers.
        let _ = crate::acl::acl_bind_set(backend, store, id.raw(), true, ingress_acl);
    }
    log::notice(sys::SAI_API_LAG, format_args!("LAG created (trunk {})", tid.0));
    Ok(id)
}

fn remove_lag<B: Backend>(backend: &mut B, store: &mut Store, id: LagId) -> Result<()> {
    let tid = store.get::<Lag>(id)?.tid;
    if members(store, id).next().is_some() || l2::bridge_port_of(store, Attach::Lag(id)).is_some() {
        return Err(Error::ObjectInUse);
    }
    if crate::l3::has_port_rif(store, id.raw()) {
        return Err(Error::ObjectInUse);
    }
    // Anything else that names it (checked before the chip is touched).
    store.ensure_removable::<Lag>(id)?;
    match backend.lag_destroy(tid) {
        Ok(()) | Err(Error::ItemNotFound) => {}
        Err(e) => return Err(e),
    }
    store.remove::<Lag>(id)?;
    crate::acl::acl_bind_forget(backend, store, id.raw());
    Ok(())
}

fn set_lag<B: Backend>(backend: &mut B, store: &mut Store, id: LagId, attr: &Attr) -> Result<()> {
    store.get::<Lag>(id)?;
    match (attr.id, &attr.value) {
        (sys::SAI_LAG_ATTR_PORT_VLAN_ID, Value::U16(vid)) => {
            let vid = Vid::new(*vid).map_err(|_| Error::attr(AttrError::InvalidValue, 0))?;
            attach_pvid_set(backend, store, id.raw(), vid).map_err(|e| l2::attr_backend_status(e, 0))
        }
        (sys::SAI_LAG_ATTR_INGRESS_ACL | sys::SAI_LAG_ATTR_EGRESS_ACL, Value::Oid(acl)) => {
            crate::acl::acl_bind_set(backend, store, id.raw(), attr.id == sys::SAI_LAG_ATTR_INGRESS_ACL, *acl)
        }
        // Read-only (refused before this, with the C++ status: `refused_set`).
        (sys::SAI_LAG_ATTR_PORT_LIST, _) => Err(Error::attr(AttrError::InvalidValue, 0)),
        _ => {
            let mut kept = store.get::<Lag>(id)?.kept.clone();
            l2::stub_set(LAG_STUBS, &mut kept, attr, 0)?;
            store.update::<Lag, _>(id, |l| l.kept = kept)
        }
    }
}

fn get_lag(store: &Store, id: LagId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let lag = store.get::<Lag>(id)?;
    Ok(match attr {
        // The members (LAG_MEMBER objects, as the header says), in creation order.
        sys::SAI_LAG_ATTR_PORT_LIST => Value::OidList(members(store, id).map(|(m, _)| m.raw()).collect()),
        sys::SAI_LAG_ATTR_PORT_VLAN_ID => Value::U16(lag.pvid.get()),
        sys::SAI_LAG_ATTR_INGRESS_ACL | sys::SAI_LAG_ATTR_EGRESS_ACL => Value::Oid(crate::acl::acl_bind_get(
            store,
            id.raw(),
            attr == sys::SAI_LAG_ATTR_INGRESS_ACL,
        )),
        _ => l2::stub_get(LAG_STUBS, &lag.kept, attr)?,
    })
}

// ------------------------------------------------------------------------ members

fn create_member<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<LagMemberId> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_LAG_MEMBER, attrs)?;
    let mut lag_attr = None;
    let mut port_attr = None;
    let mut egress_disable = false;
    let mut ingress_disable = false;
    let mut kept = Kept::new();
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_LAG_MEMBER_ATTR_LAG_ID, Value::Oid(lag)) => lag_attr = Some((*lag, index)),
            (sys::SAI_LAG_MEMBER_ATTR_PORT_ID, Value::Oid(port)) => port_attr = Some((*port, index)),
            (sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(b)) => egress_disable = *b,
            (sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, Value::Bool(b)) => ingress_disable = *b,
            _ => l2::stub_set(MEMBER_STUBS, &mut kept, attr, index)?,
        }
    }
    let (Some((lag_raw, lag_index)), Some((port_raw, port_index))) = (lag_attr, port_attr) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    if lag_raw.is_null() || port_raw.is_null() {
        return Err(Error::MandatoryAttributeMissing);
    }
    let bad = |index| Error::attr(AttrError::InvalidValue, index);
    let lag = LagId::try_from(lag_raw).map_err(|_| bad(lag_index))?;
    let (tid, pvid) = store
        .get::<Lag>(lag)
        .map(|l| (l.tid, l.pvid))
        .map_err(|_| bad(lag_index))?;
    let port = PortId::try_from(port_raw).map_err(|_| bad(port_index))?;
    let (hw, name) = match store.get::<Port>(port) {
        Ok(Port {
            kind: PortKind::Front { .. },
            hw,
            name,
            ..
        }) => (*hw, name.clone()),
        _ => return Err(bad(port_index)),
    };
    if port_lag(store, port).is_some() {
        return Err(Error::ItemAlreadyExists);
    }
    // A member carries no L2 / L3 state of its own.
    if l2::bridge_port_of(store, Attach::Port(port)).is_some() || crate::l3::has_port_rif(store, port.raw()) {
        log::error(
            sys::SAI_API_LAG,
            format_args!("LAG member: port {name} still has a bridge port or router interface"),
        );
        return Err(Error::ObjectInUse);
    }

    let member = TrunkMember {
        hw,
        egress_disable,
        ingress_disable,
    };
    backend.lag_set_members(tid, &trunk_members(store, lag, Change::Add(member)))?;
    let joined = l2::bridge_port_member_join(backend, store, lag, hw)
        .and_then(|()| crate::l3::rif_member_join(backend, store, lag, hw))
        .and_then(|()| backend.port_pvid_set(hw, pvid));
    if let Err(e) = joined {
        crate::l3::rif_member_leave(backend, store, lag, hw);
        l2::bridge_port_member_leave(backend, store, lag, hw);
        let _ = backend.lag_set_members(tid, &trunk_members(store, lag, Change::None));
        return Err(e);
    }
    let id = store.insert(LagMember {
        lag,
        port,
        egress_disable,
        ingress_disable,
        kept,
    })?;
    store.update::<Port, _>(port, |p| p.pvid = pvid)?;
    // ACLs bound to the LAG now cover this port.
    crate::acl::acl_reprogram(backend, store);
    let on_off = |disabled: bool| if disabled { "off" } else { "on" };
    log::notice(
        sys::SAI_API_LAG,
        format_args!(
            "LAG trunk {}: member {name} (egress {}, ingress {})",
            tid.0,
            on_off(egress_disable),
            on_off(ingress_disable)
        ),
    );
    Ok(id)
}

fn remove_member<B: Backend>(backend: &mut B, store: &mut Store, id: LagMemberId) -> Result<()> {
    store.ensure_removable::<LagMember>(id)?;
    let member = store.get::<LagMember>(id)?;
    let (lag, port) = (member.lag, member.port);
    let tid = store.get::<Lag>(lag)?.tid;
    backend.lag_set_members(tid, &trunk_members(store, lag, Change::Remove(id)))?;
    let hw = store.get::<Port>(port).map(|p| p.hw);
    store.remove::<LagMember>(id)?;
    if let Ok(hw) = hw {
        crate::l3::rif_member_leave(backend, store, lag, hw);
        l2::bridge_port_member_leave(backend, store, lag, hw);
        if backend.port_pvid_set(hw, Vid::DEFAULT).is_ok() {
            store.update::<Port, _>(port, |p| p.pvid = Vid::DEFAULT)?;
        }
    }
    crate::acl::acl_reprogram(backend, store);
    Ok(())
}

fn set_member<B: Backend>(backend: &mut B, store: &mut Store, id: LagMemberId, attr: &Attr) -> Result<()> {
    let member = store.get::<LagMember>(id)?;
    let (mut egress, mut ingress) = (member.egress_disable, member.ingress_disable);
    match (attr.id, &attr.value) {
        (sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE, Value::Bool(b)) => egress = *b,
        (sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE, Value::Bool(b)) => ingress = *b,
        // Create-only (refused before this, with the C++ status: `refused_set`).
        (sys::SAI_LAG_MEMBER_ATTR_LAG_ID | sys::SAI_LAG_MEMBER_ATTR_PORT_ID, _) => {
            return Err(Error::attr(AttrError::InvalidValue, 0));
        }
        _ => {
            let mut kept = member.kept.clone();
            l2::stub_set(MEMBER_STUBS, &mut kept, attr, 0)?;
            return store.update::<LagMember, _>(id, |m| m.kept = kept);
        }
    }
    let lag = member.lag;
    let tid = store.get::<Lag>(lag).map_err(|_| Error::Failure)?.tid;
    backend
        .lag_set_members(tid, &trunk_members(store, lag, Change::Flags(id, egress, ingress)))
        .map_err(|e| l2::attr_backend_status(e, 0))?;
    store.update::<LagMember, _>(id, |m| {
        m.egress_disable = egress;
        m.ingress_disable = ingress;
    })
}

fn get_member(store: &Store, id: LagMemberId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let member = store.get::<LagMember>(id)?;
    Ok(match attr {
        sys::SAI_LAG_MEMBER_ATTR_LAG_ID => Value::Oid(member.lag.raw()),
        sys::SAI_LAG_MEMBER_ATTR_PORT_ID => Value::Oid(member.port.raw()),
        sys::SAI_LAG_MEMBER_ATTR_EGRESS_DISABLE => Value::Bool(member.egress_disable),
        sys::SAI_LAG_MEMBER_ATTR_INGRESS_DISABLE => Value::Bool(member.ingress_disable),
        _ => l2::stub_get(MEMBER_STUBS, &member.kept, attr)?,
    })
}

// ------------------------------------------------------------------------ test port

/// The test port's questions about LAGs ([`crate::testing`]): a LAG's trunk (C++ `LagObj::tid`).
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_backend_id<B: Backend>(
    sai: &crate::sai::Sai<B>,
    oid: RawOid,
) -> crate::testing::AreaAnswer<Option<crate::testing::BackendId>> {
    let Some(store) = sai.store() else { return Ok(None) };
    Ok(LagId::try_from(oid)
        .ok()
        .and_then(|id| store.get::<Lag>(id).ok())
        .map(|lag| crate::testing::BackendId::Trunk(lag.tid)))
}

/// The test port's questions about LAGs ([`crate::testing`]): the LAG a (live) port is a member
/// of (C++ `PortObj::lag`).
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_port_lag<B: Backend>(
    sai: &crate::sai::Sai<B>,
    port: PortId,
) -> crate::testing::AreaAnswer<Option<LagId>> {
    Ok(sai.store().and_then(|store| port_lag(store, port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_defaults_are_values_of_their_attributes() {
        for (object_type, stubs) in [
            (sys::SAI_OBJECT_TYPE_LAG, LAG_STUBS),
            (sys::SAI_OBJECT_TYPE_LAG_MEMBER, MEMBER_STUBS),
        ] {
            for (id, default) in stubs {
                let meta = sai_meta::attribute(object_type, *id);
                assert_eq!(meta.map(|m| m.value_type), Some(default.value_type()), "{id}");
            }
        }
    }

    #[test]
    fn attachments_decode_by_type() {
        let port = PortId::from_index(5);
        let lag = LagId::from_index(2);
        assert_eq!(Attach::decode(port.raw()), Some(Attach::Port(port)));
        assert_eq!(Attach::decode(lag.raw()), Some(Attach::Lag(lag)));
        assert_eq!(Attach::decode(RawOid::NULL), None);
        assert_eq!(Attach::decode(LagMemberId::from_index(1).raw()), None);
        assert_eq!(Attach::Lag(lag).raw(), lag.raw());
    }
}

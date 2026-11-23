//! The switch's `LAG_HASH` / `ECMP_HASH` objects (`NATIVE_HASH_FIELD_LIST`) with the switch's
//! `LAG_` / `ECMP_DEFAULT_HASH_SEED`, port storm-control policers, VLAN flood control. A backend
//! without them (`NOT_SUPPORTED`) gets the old behaviour: the attributes are kept, nothing is
//! programmed; without [`crate::backend::Hash::hash_supported`] the hash objects are not
//! there for the user at all (`LAG_HASH` / `ECMP_HASH` "not supported", the objects' ids invalid).
//! Ports `sai_hash.cpp` (174 lines).
//!
//! State: the hash objects are the attribute bags the switch builds ([`crate::model::Switch::objects`]
//! names them), their field list kept as `NATIVE_HASH_FIELD_LIST`; the seeds in
//! [`crate::model::Switch::kept`]; a port's storm policers in [`crate::model::Port::kept`] (so a
//! policer a port uses is in use by the store's count, as the C++ `policer_used` says).
//!
//! Backend: [`crate::backend::Hash`], [`crate::backend::Storm`].
//!
//! Registration: [`crate::dispatch::Owner::Hash`] for `HASH` in [`crate::dispatch::OWNERS`]; the
//! switch, port and VLAN attributes through [`switch_hash_set`] / [`switch_hash_get`],
//! [`port_storm_attr_set`] / [`port_storm_attr_get`], [`vlan_flood_attr`] / [`vlan_flood_apply`]
//! (called by the switch, port and VLAN code).
use crate::backend::types::{FloodKind, StormKind};
use crate::backend::{Backend, Vid};
use crate::dispatch::{Area, Cx};
use crate::error::{Error, Result};
use crate::log;
use crate::model::{Port, Switch};
use crate::oid::{ObjectType, PortId, RawOid, SwitchId};
use crate::qos::{backend_attr_error, invalid, keep_on_port, kept_oid, not_supported};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct HashArea;

impl Area for HashArea {
    /// C++ `set_hash_attribute`: anything but `NATIVE_HASH_FIELD_LIST` is `attr_not_supported(0)`
    /// (`sai_hash.cpp:83`).
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        let _ = object_type;
        Some(crate::dispatch::cpp_refusal(attr, &[]))
    }

    /// The hash objects are the switch's `LAG_HASH` / `ECMP_HASH`.
    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        let _ = (cx, object_type, attrs);
        log::warn(
            sys::SAI_API_HASH,
            format_args!("hash objects are the switch's LAG_HASH / ECMP_HASH; create refused"),
        );
        Err(Error::NotSupported)
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        let _ = (cx, object_type, id);
        Err(Error::NotSupported)
    }

    /// `NATIVE_HASH_FIELD_LIST` only (C++ `set_hash_attribute`).
    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        let _ = object_type;
        let lag = hash_of(cx.backend, cx.store, id)?;
        let (sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST, Value::I32List(fields)) = (attr.id, &attr.value) else {
            return Err(not_supported(0));
        };
        apply_hash(cx.backend, lag, fields, seed_of(cx.store, lag)).map_err(|e| backend_attr_error(e, 0))?;
        cx.store
            .update_bag(id, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = object_type;
        hash_of(backend, store, id)?;
        if meta.id != sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST {
            return Err(not_supported(0));
        }
        Ok(store
            .bag(id)?
            .attrs
            .get(&meta.id)
            .cloned()
            .unwrap_or(Value::I32List(Vec::new())))
    }
}

/// Which of the two hashes an object is (`true`: the LAG hash); `INVALID_OBJECT_ID` without a
/// hash backend (as the C++, whose lookup is gated).
fn hash_of<B: Backend>(backend: &B, store: &Store, id: RawOid) -> Result<bool> {
    let switch = store.iter::<Switch>().next().map(|(_, s)| s);
    let named = |attr| switch.and_then(|s| s.objects.get(&attr)) == Some(&id);
    if !backend.hash_supported() || store.object_type(id) != Some(ObjectType::Hash) {
        return Err(Error::InvalidObjectId);
    }
    if named(sys::SAI_SWITCH_ATTR_LAG_HASH) {
        Ok(true)
    } else if named(sys::SAI_SWITCH_ATTR_ECMP_HASH) {
        Ok(false)
    } else {
        Err(Error::InvalidObjectId)
    }
}

/// The seed kept for the LAG or ECMP hash (0 while unset).
fn seed_of(store: &Store, lag: bool) -> u32 {
    let attr = if lag {
        sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED
    } else {
        sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED
    };
    match store.iter::<Switch>().next().and_then(|(_, s)| s.kept.get(&attr)) {
        Some(Value::U32(seed)) => *seed,
        _ => 0,
    }
}

/// The fields of the LAG or ECMP hash object (empty while unset).
fn fields_of(store: &Store, lag: bool) -> Vec<i32> {
    let attr = if lag {
        sys::SAI_SWITCH_ATTR_LAG_HASH
    } else {
        sys::SAI_SWITCH_ATTR_ECMP_HASH
    };
    let hash = store
        .iter::<Switch>()
        .next()
        .and_then(|(_, s)| s.objects.get(&attr).copied());
    match hash
        .and_then(|h| store.bag(h).ok())
        .and_then(|b| b.attrs.get(&sys::SAI_HASH_ATTR_NATIVE_HASH_FIELD_LIST))
    {
        Some(Value::I32List(fields)) => fields.clone(),
        _ => Vec::new(),
    }
}

/// Programs a hash; the backend's `NOT_SUPPORTED` means "kept, not programmed" (C++ `apply_hash`).
fn apply_hash<B: Backend>(backend: &mut B, lag: bool, fields: &[i32], seed: u32) -> Result<()> {
    match backend.hash_set(lag, fields, seed) {
        Err(Error::NotSupported) => {
            log::info(
                sys::SAI_API_HASH,
                format_args!(
                    "{} hash kept, not programmed (backend has no hash control)",
                    if lag { "LAG" } else { "ECMP" }
                ),
            );
            Ok(())
        }
        other => other,
    }
}

/// `LAG_` / `ECMP_DEFAULT_HASH_SEED` with a hash backend (C++ `switch_hash_set`): the hash is
/// programmed with its fields and the new seed, and the seed kept. `None`: not handled here (no
/// hash backend, or another attribute): the caller keeps the seed as a stub.
pub(crate) fn switch_hash_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    switch: SwitchId,
    attr: &Attr,
) -> Option<Result<()>> {
    let lag = match attr.id {
        sys::SAI_SWITCH_ATTR_LAG_DEFAULT_HASH_SEED => true,
        sys::SAI_SWITCH_ATTR_ECMP_DEFAULT_HASH_SEED => false,
        _ => return None,
    };
    if !backend.hash_supported() {
        return None;
    }
    let Value::U32(seed) = attr.value else {
        return Some(Err(Error::Failure));
    };
    Some(
        apply_hash(backend, lag, &fields_of(store, lag), seed)
            .map_err(|e| backend_attr_error(e, 0))
            .and_then(|()| store.update::<Switch, _>(switch, |s| drop(s.kept.insert(attr.id, attr.value.clone())))),
    )
}

/// `LAG_HASH` / `ECMP_HASH` (C++ `switch_hash_get`): the objects with a hash backend, "not
/// supported" without one (in the C++ these fall through to the stubs, which do not have them).
/// `None`: another attribute.
pub(crate) fn switch_hash_get<B: Backend>(
    backend: &B,
    store: &Store,
    switch: SwitchId,
    attr: sys::sai_attr_id_t,
) -> Option<Result<Value>> {
    if attr != sys::SAI_SWITCH_ATTR_LAG_HASH && attr != sys::SAI_SWITCH_ATTR_ECMP_HASH {
        return None;
    }
    if !backend.hash_supported() {
        return Some(Err(not_supported(0)));
    }
    Some(
        store
            .get::<Switch>(switch)
            .and_then(|s| s.objects.get(&attr).map(|h| Value::Oid(*h)).ok_or(Error::Failure)),
    )
}

// ------------------------------------------------------------------ storm control

fn storm_kind(attr: sys::sai_attr_id_t) -> Option<StormKind> {
    match attr {
        sys::SAI_PORT_ATTR_BROADCAST_STORM_CONTROL_POLICER_ID => Some(StormKind::Broadcast),
        sys::SAI_PORT_ATTR_MULTICAST_STORM_CONTROL_POLICER_ID => Some(StormKind::Multicast),
        sys::SAI_PORT_ATTR_FLOOD_STORM_CONTROL_POLICER_ID => Some(StormKind::UnknownUnicast),
        _ => None,
    }
}

/// The port's storm-control policers (C++ `port_storm_attr_set`): the backend's
/// `NOT_SUPPORTED` is "not supported" (as before the hook); kept once programmed. `None`: not
/// one. The CPU port is asked of the backend like any other (as the C++).
pub(crate) fn port_storm_attr_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    port: PortId,
    attr: &Attr,
) -> Option<Result<()>> {
    let kind = storm_kind(attr.id)?;
    Some(set_storm(backend, store, port, kind, attr))
}

fn set_storm<B: Backend>(backend: &mut B, store: &mut Store, port: PortId, kind: StormKind, attr: &Attr) -> Result<()> {
    let Value::Oid(policer) = attr.value else {
        return Err(Error::Failure);
    };
    let pid = if policer.is_null() {
        None
    } else {
        if !crate::acl::policer_valid(store, policer) {
            return Err(invalid(0));
        }
        crate::acl::policer_backend_id(store, policer)
    };
    let hw = store.get::<Port>(port)?.hw;
    match backend.port_storm_set(hw, kind, pid) {
        Ok(()) => keep_on_port(store, port, attr),
        Err(Error::NotSupported) => Err(not_supported(0)),
        Err(e) => Err(backend_attr_error(e, 0)),
    }
}

/// Gets of the port's storm-control policers (C++ `port_storm_attr_get`). `None`: not one.
pub(crate) fn port_storm_attr_get(store: &Store, port: PortId, attr: sys::sai_attr_id_t) -> Option<Result<Value>> {
    storm_kind(attr)?;
    Some(store.get::<Port>(port).map(|p| Value::Oid(kept_oid(&p.kept, attr))))
}

// ------------------------------------------------------------------ VLAN flood control

/// The three VLAN `*_FLOOD_CONTROL_TYPE` attributes (C++ `vlan_flood_attr`).
pub(crate) fn vlan_flood_attr(id: sys::sai_attr_id_t) -> bool {
    flood_kind(id).is_some()
}

fn flood_kind(id: sys::sai_attr_id_t) -> Option<FloodKind> {
    match id {
        sys::SAI_VLAN_ATTR_UNKNOWN_UNICAST_FLOOD_CONTROL_TYPE => Some(FloodKind::UnknownUnicast),
        sys::SAI_VLAN_ATTR_UNKNOWN_MULTICAST_FLOOD_CONTROL_TYPE => Some(FloodKind::UnknownMulticast),
        sys::SAI_VLAN_ATTR_BROADCAST_FLOOD_CONTROL_TYPE => Some(FloodKind::Broadcast),
        _ => None,
    }
}

/// Programs a VLAN flood-control attribute (C++ `vlan_flood_apply`): `ALL` / `NONE` go to the
/// backend; `L2MC_GROUP` / `COMBINED`, and a backend without flood control, are kept, not
/// programmed. On success the caller keeps the value as a stub.
pub(crate) fn vlan_flood_apply<B: Backend>(backend: &mut B, vid: Vid, attr: &Attr) -> Result<()> {
    let Some(kind) = flood_kind(attr.id) else {
        return Ok(());
    };
    let on = match attr.value {
        Value::Enum(t) if t == sys::SAI_VLAN_FLOOD_CONTROL_TYPE_ALL.cast_signed() => true,
        Value::Enum(t) if t == sys::SAI_VLAN_FLOOD_CONTROL_TYPE_NONE.cast_signed() => false,
        _ => return Ok(()),
    };
    match backend.vlan_flood_set(vid, kind, on) {
        Ok(()) | Err(Error::NotSupported) => Ok(()),
        Err(e) => Err(backend_attr_error(e, 0)),
    }
}

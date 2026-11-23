//! Attribute-bag objects and entries: the object types whose whole state is their
//! attributes. One implementation serves them all, driven by the metadata:
//!
//! - create and set accept what the SAI headers allow (create-only / create-and-set flags,
//!   mandatory attributes, enumerators, the object types an id may name) and nothing else;
//! - an object id attribute must name a live object, and from then on it is a reference the
//!   store counts: the named object cannot be removed (`OBJECT_IN_USE`);
//! - a get of an attribute nobody set answers the header's `@default`;
//! - read-only attributes are computed from the store by [`computed`];
//! - the backend is asked first ([`Backend::object_create`] and friends). A backend that has
//!   no such object refuses, and nothing is stored: only the virtual backend says yes to
//!   everything.
//!
//! Types with behaviour of their own (ports, VLANs, bridge ports: [`crate::sai`]) are not
//! bags. The few rules the metadata does not carry (conditions) are in [`validate`].
use crate::attrs::{self, CreateArgs};
use crate::backend::{Backend, HostifNetdev};
use crate::error::{AttrError, Error, Result};
use crate::model::{Bag, Bridge, BridgePort, EntryKey, Kept, Port, PortKind, Switch, Vlan};
use crate::oid::{ObjectType, PortId, RawOid, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// Whether the SAI user creates and removes objects of this type. The others come with
/// the switch (queues, priority groups, scheduler groups, the STP instance, the hashes).
const fn user_created(object_type: ObjectType) -> bool {
    !matches!(
        object_type,
        ObjectType::Queue
            | ObjectType::IngressPriorityGroup
            | ObjectType::SchedulerGroup
            | ObjectType::Stp
            | ObjectType::Hash
    )
}

/// Every object id in `attrs` must name a live object; the error names the attribute.
pub(crate) fn check_references(store: &Store, attrs: &[Attr]) -> Result<()> {
    for (index, attr) in attrs.iter().enumerate() {
        if attr.value.object_ids().iter().any(|oid| !store.contains(*oid)) {
            return Err(Error::attr(AttrError::InvalidValue, index));
        }
    }
    Ok(())
}

fn kept_from(attrs: &[Attr]) -> Kept {
    attrs.iter().map(|a| (a.id, a.value.clone())).collect()
}

fn enum_of(attrs: &[Attr], id: sys::sai_attr_id_t) -> Option<i32> {
    attrs.iter().find(|a| a.id == id).and_then(|a| match a.value {
        Value::Enum(v) => Some(v),
        _ => None,
    })
}

/// The rules of a create that the metadata tables do not carry: attributes that are
/// mandatory under a condition, and keys that must be unique.
fn validate(store: &Store, object_type: ObjectType, attrs: &[Attr]) -> Result<()> {
    let find = |id: sys::sai_attr_id_t| attrs.iter().find(|a| a.id == id).map(|a| &a.value);
    let need = |id: sys::sai_attr_id_t| find(id).map(|_| ()).ok_or(Error::MandatoryAttributeMissing);
    let taken = |id: sys::sai_attr_id_t| {
        let wanted = find(id);
        wanted.is_some() && store.bags(object_type).any(|(_, bag)| bag.attrs.get(&id) == wanted)
    };
    match object_type {
        ObjectType::RouterInterface => match enum_of(attrs, sys::SAI_ROUTER_INTERFACE_ATTR_TYPE) {
            Some(t)
                if t == sys::SAI_ROUTER_INTERFACE_TYPE_PORT.cast_signed()
                    || t == sys::SAI_ROUTER_INTERFACE_TYPE_SUB_PORT.cast_signed() =>
            {
                need(sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID)
            }
            Some(t) if t == sys::SAI_ROUTER_INTERFACE_TYPE_VLAN.cast_signed() => {
                need(sys::SAI_ROUTER_INTERFACE_ATTR_VLAN_ID)
            }
            _ => Ok(()),
        },
        ObjectType::Hostif => {
            if is_netdev(attrs) {
                need(sys::SAI_HOSTIF_ATTR_OBJ_ID)?;
                need(sys::SAI_HOSTIF_ATTR_NAME)?;
                // The C++ library's rules (sai_hostif.cpp): a name a netdev can have, and
                // one netdev per port.
                if let Some(Value::CharData(name)) = find(sys::SAI_HOSTIF_ATTR_NAME) {
                    if name.is_empty() || name.len() > NETDEV_NAME_MAX {
                        return Err(Error::InvalidParameter);
                    }
                }
                if taken(sys::SAI_HOSTIF_ATTR_OBJ_ID) {
                    return Err(Error::ItemAlreadyExists);
                }
            }
            // One netdev per name.
            if taken(sys::SAI_HOSTIF_ATTR_NAME) {
                return Err(Error::ItemAlreadyExists);
            }
            Ok(())
        }
        ObjectType::NextHop => {
            if enum_of(attrs, sys::SAI_NEXT_HOP_ATTR_TYPE) == Some(sys::SAI_NEXT_HOP_TYPE_IP.cast_signed()) {
                need(sys::SAI_NEXT_HOP_ATTR_IP)?;
                need(sys::SAI_NEXT_HOP_ATTR_ROUTER_INTERFACE_ID)?;
            }
            Ok(())
        }
        // One trap per trap type.
        ObjectType::HostifTrap if taken(sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE) => Err(Error::ItemAlreadyExists),
        // The C++ library's rules (sai_lag.cpp): a front-panel port, in one LAG at most, that
        // has no bridge port or router interface of its own (orchagent removes the default
        // bridge ports first).
        ObjectType::LagMember => {
            let index = attrs
                .iter()
                .position(|a| a.id == sys::SAI_LAG_MEMBER_ATTR_PORT_ID)
                .ok_or(Error::MandatoryAttributeMissing)?;
            let Some(Value::Oid(port)) = find(sys::SAI_LAG_MEMBER_ATTR_PORT_ID) else {
                return Err(Error::MandatoryAttributeMissing);
            };
            let front = PortId::try_from(*port)
                .ok()
                .and_then(|id| store.get::<Port>(id).ok())
                .is_some_and(|p| matches!(p.kind, PortKind::Front { .. }));
            if !front {
                return Err(Error::attr(AttrError::InvalidValue, index));
            }
            if taken(sys::SAI_LAG_MEMBER_ATTR_PORT_ID) {
                return Err(Error::ItemAlreadyExists);
            }
            let bridged = store.iter::<BridgePort>().any(|(_, b)| b.port.raw() == *port);
            let routed = !naming(
                store,
                ObjectType::RouterInterface,
                sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID,
                *port,
            )
            .is_empty();
            if bridged || routed {
                return Err(Error::ObjectInUse);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Longest netdev name (`IFNAMSIZ` - 1).
const NETDEV_NAME_MAX: usize = 15;

fn is_netdev(attrs: &[Attr]) -> bool {
    enum_of(attrs, sys::SAI_HOSTIF_ATTR_TYPE) == Some(sys::SAI_HOSTIF_TYPE_NETDEV.cast_signed())
}

/// A host interface of type NETDEV, resolved for the backend: its port must be a front-panel
/// port (`NOT_SUPPORTED` otherwise, as in the C++), its MAC address is the switch's.
fn netdev_of<'a>(store: &Store, object_type: ObjectType, attrs: &'a [Attr]) -> Result<Option<HostifNetdev<'a>>> {
    if object_type != ObjectType::Hostif || !is_netdev(attrs) {
        return Ok(None);
    }
    let find = |id: sys::sai_attr_id_t| attrs.iter().find(|a| a.id == id).map(|a| &a.value);
    let Some(Value::CharData(name)) = find(sys::SAI_HOSTIF_ATTR_NAME) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let Some(Value::Oid(target)) = find(sys::SAI_HOSTIF_ATTR_OBJ_ID) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let port = PortId::try_from(*target)
        .ok()
        .and_then(|id| store.get::<Port>(id).ok())
        .filter(|port| matches!(port.kind, PortKind::Front { .. }))
        .ok_or(Error::NotSupported)?;
    Ok(Some(HostifNetdev {
        name,
        port: port.hw,
        mac: switch_mac(store).ok_or(Error::Failure)?,
        oper_up: matches!(find(sys::SAI_HOSTIF_ATTR_OPER_STATUS), Some(Value::Bool(true))),
    }))
}

/// Creates an attribute-bag object.
pub(crate) fn create<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    object_type: ObjectType,
    attrs: &[Attr],
) -> Result<RawOid> {
    if !user_created(object_type) {
        return Err(Error::NotSupported);
    }
    CreateArgs::check(object_type.sai(), attrs)?;
    check_references(store, attrs)?;
    validate(store, object_type, attrs)?;
    let id = store.peek_id(object_type);
    match netdev_of(store, object_type, attrs)? {
        Some(netdev) => backend.hostif_create(id, &netdev, attrs)?,
        None => backend.object_create(object_type.sai(), id, attrs)?,
    }
    store.insert_bag(Bag {
        object_type,
        attrs: kept_from(attrs),
    })
}

/// Removes an attribute-bag object.
pub(crate) fn remove<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    object_type: ObjectType,
    id: RawOid,
) -> Result<()> {
    if !user_created(object_type) {
        return Err(Error::NotSupported);
    }
    store.ensure_bag_removable(id)?;
    backend.object_remove(object_type.sai(), id)?;
    store.remove_bag(id).map(|_| ())
}

/// Sets one attribute of an attribute-bag object. The caller checked it against the
/// metadata.
pub(crate) fn set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    object_type: ObjectType,
    id: RawOid,
    attr: &Attr,
) -> Result<()> {
    store.bag(id)?;
    check_references(store, core::slice::from_ref(attr))?;
    backend.object_set(object_type.sai(), id, attr)?;
    store.update_bag(id, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
}

/// Gets one attribute of an attribute-bag object: what was set, else what is computed, else
/// the header's default.
pub(crate) fn get(store: &Store, id: RawOid, meta: &AttrMeta) -> Result<Value> {
    let bag = store.bag(id)?;
    if let Some(value) = bag.attrs.get(&meta.id) {
        return Ok(value.clone());
    }
    if let Some(value) = computed(store, id, bag, meta.id)? {
        return Ok(value);
    }
    if meta.flags.creatable() {
        if let Some(value) = attrs::default_value(meta) {
            return Ok(value);
        }
        // An object id that was not given (it is only valid under a condition that does not
        // hold for this object: a port interface's VLAN) names nothing.
        if meta.value_type == sai_meta::ValueType::Oid {
            return Ok(Value::Oid(RawOid::NULL));
        }
    }
    Err(Error::attr(AttrError::NotSupported, 0))
}

fn count(n: usize) -> Result<Value> {
    u32::try_from(n).map(Value::U32).map_err(|_| Error::Failure)
}

/// The bags of `object_type` whose attribute `attr` names `target`.
fn naming(store: &Store, object_type: ObjectType, attr: sys::sai_attr_id_t, target: RawOid) -> Vec<RawOid> {
    store
        .bags(object_type)
        .filter(|(_, bag)| bag.attrs.get(&attr) == Some(&Value::Oid(target)))
        .map(|(id, _)| id)
        .collect()
}

/// The switch's MAC address: the default of a virtual router's.
fn switch_mac(store: &Store) -> Option<[u8; 6]> {
    store.iter::<Switch>().next().map(|(_, s)| s.src_mac)
}

/// Read-only and derived attributes.
fn computed(store: &Store, id: RawOid, bag: &Bag, attr: sys::sai_attr_id_t) -> Result<Option<Value>> {
    Ok(Some(match (bag.object_type, attr) {
        (ObjectType::SchedulerGroup, sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_COUNT) => {
            count(naming(store, ObjectType::Queue, sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE, id).len())?
        }
        (ObjectType::SchedulerGroup, sys::SAI_SCHEDULER_GROUP_ATTR_CHILD_LIST) => Value::OidList(naming(
            store,
            ObjectType::Queue,
            sys::SAI_QUEUE_ATTR_PARENT_SCHEDULER_NODE,
            id,
        )),
        (ObjectType::Stp, sys::SAI_STP_ATTR_VLAN_LIST) => {
            Value::VlanList(store.iter::<Vlan>().map(|(_, v)| v.vid.get()).collect())
        }
        (ObjectType::Stp, sys::SAI_STP_ATTR_BRIDGE_ID) => Value::Oid(
            store
                .iter::<Bridge>()
                .next()
                .map_or(RawOid::NULL, |(bridge, _)| bridge.raw()),
        ),
        (ObjectType::Stp, sys::SAI_STP_ATTR_PORT_LIST) => Value::OidList(Vec::new()),
        (ObjectType::Lag, sys::SAI_LAG_ATTR_PORT_LIST) => Value::OidList(
            naming(store, ObjectType::LagMember, sys::SAI_LAG_MEMBER_ATTR_LAG_ID, id)
                .into_iter()
                .filter_map(
                    |member| match store.bag(member).ok()?.attrs.get(&sys::SAI_LAG_MEMBER_ATTR_PORT_ID) {
                        Some(Value::Oid(port)) => Some(*port),
                        _ => None,
                    },
                )
                .collect(),
        ),
        (ObjectType::NextHopGroup, sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_COUNT) => count(
            naming(
                store,
                ObjectType::NextHopGroupMember,
                sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID,
                id,
            )
            .len(),
        )?,
        (ObjectType::NextHopGroup, sys::SAI_NEXT_HOP_GROUP_ATTR_NEXT_HOP_MEMBER_LIST) => Value::OidList(naming(
            store,
            ObjectType::NextHopGroupMember,
            sys::SAI_NEXT_HOP_GROUP_MEMBER_ATTR_NEXT_HOP_GROUP_ID,
            id,
        )),
        // "attrvalue SAI_SWITCH_ATTR_SRC_MAC_ADDRESS"
        (ObjectType::VirtualRouter, sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS) => {
            Value::Mac(switch_mac(store).ok_or(Error::Failure)?)
        }
        // "attrvalue SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS": the router's, else the switch's.
        (ObjectType::RouterInterface, sys::SAI_ROUTER_INTERFACE_ATTR_SRC_MAC_ADDRESS) => {
            let router = match bag.attrs.get(&sys::SAI_ROUTER_INTERFACE_ATTR_VIRTUAL_ROUTER_ID) {
                Some(Value::Oid(vr)) => store.bag(*vr).ok(),
                _ => None,
            };
            match router.and_then(|vr| vr.attrs.get(&sys::SAI_VIRTUAL_ROUTER_ATTR_SRC_MAC_ADDRESS)) {
                Some(mac) => mac.clone(),
                None => Value::Mac(switch_mac(store).ok_or(Error::Failure)?),
            }
        }
        _ => return Ok(None),
    }))
}

/// The object an entry's key names must exist and be of the type the entry needs.
fn check_key(store: &Store, key: &EntryKey) -> Result<()> {
    let wanted = match key {
        // .1Q only: the VLAN the address is in (a .1D bridge as bv_id is not implemented).
        EntryKey::Fdb { .. } => ObjectType::Vlan,
        EntryKey::Neighbor { .. } => ObjectType::RouterInterface,
        EntryKey::Route { .. } => ObjectType::VirtualRouter,
    };
    if store.object_type(key.owner()) == Some(wanted) {
        Ok(())
    } else {
        Err(Error::InvalidParameter)
    }
}

/// Creates an entry.
pub(crate) fn create_entry<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    key: EntryKey,
    attrs: &[Attr],
) -> Result<()> {
    CreateArgs::check(key.entry_type().sai(), attrs)?;
    check_key(store, &key)?;
    check_references(store, attrs)?;
    if store.entry(&key).is_ok() {
        return Err(Error::ItemAlreadyExists);
    }
    backend.entry_create(&key, attrs)?;
    store.insert_entry(key, kept_from(attrs))
}

/// Removes an entry.
pub(crate) fn remove_entry<B: Backend>(backend: &mut B, store: &mut Store, key: &EntryKey) -> Result<()> {
    store.entry(key)?;
    backend.entry_remove(key)?;
    store.remove_entry(key).map(|_| ())
}

/// Sets one attribute of an entry.
pub(crate) fn set_entry<B: Backend>(backend: &mut B, store: &mut Store, key: &EntryKey, attr: &Attr) -> Result<()> {
    attrs::check_set(key.entry_type().sai(), attr)?;
    store.entry(key)?;
    check_references(store, core::slice::from_ref(attr))?;
    backend.entry_set(key, attr)?;
    store.update_entry(key, |attrs| drop(attrs.insert(attr.id, attr.value.clone())))
}

/// Gets attributes of an entry: what was set, else the header's default.
pub(crate) fn get_entry(store: &Store, key: &EntryKey, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
    let stored = store.entry(key)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| {
            let meta = attrs::meta(key.entry_type().sai(), *id, index)?;
            stored
                .get(id)
                .cloned()
                .or_else(|| attrs::default_value(meta))
                .ok_or(Error::attr(AttrError::NotSupported, index))
        })
        .collect()
}

/// `flush_fdb_entries`: removes the FDB entries that match every given attribute
/// (`BRIDGE_PORT_ID`, `BV_ID`, `ENTRY_TYPE`; the header's default type is dynamic).
/// Returns the removed keys. Not atomic: a backend failure stops the flush where it is.
pub(crate) fn flush_fdb<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_FDB_FLUSH, attrs)?;
    let find = |id: sys::sai_attr_id_t| attrs.iter().find(|a| a.id == id).map(|a| &a.value);
    let bridge_port = find(sys::SAI_FDB_FLUSH_ATTR_BRIDGE_PORT_ID);
    let bv_id = match find(sys::SAI_FDB_FLUSH_ATTR_BV_ID) {
        Some(Value::Oid(oid)) => Some(*oid),
        _ => None,
    };
    let wanted_type = match enum_of(attrs, sys::SAI_FDB_FLUSH_ATTR_ENTRY_TYPE) {
        None => Some(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC),
        Some(t) if t == sys::SAI_FDB_FLUSH_ENTRY_TYPE_DYNAMIC.cast_signed() => Some(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC),
        Some(t) if t == sys::SAI_FDB_FLUSH_ENTRY_TYPE_STATIC.cast_signed() => Some(sys::SAI_FDB_ENTRY_TYPE_STATIC),
        Some(_) => None, // all
    };
    let matching: Vec<EntryKey> = store
        .entries()
        .filter(|(key, stored)| {
            let EntryKey::Fdb { bv_id: in_vlan, .. } = key else {
                return false;
            };
            bv_id.is_none_or(|wanted| wanted == *in_vlan)
                && bridge_port.is_none_or(|wanted| stored.get(&sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID) == Some(wanted))
                && wanted_type.is_none_or(|wanted| {
                    stored.get(&sys::SAI_FDB_ENTRY_ATTR_TYPE) == Some(&Value::Enum(wanted.cast_signed()))
                })
        })
        .map(|(key, _)| *key)
        .collect();
    for key in &matching {
        remove_entry(backend, store, key)?;
    }
    Ok(matching)
}

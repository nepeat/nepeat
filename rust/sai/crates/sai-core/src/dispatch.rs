//! Who implements each object type: one table, so parallel area work does not meet in one
//! `match`.
//!
//! - [`OWNERS`] / [`ENTRY_OWNERS`] name the [`Owner`] of every object and entry type. The switch
//!   and ports have typed code in [`crate::sai`] ([`Owner::Core`]) and never reach this module;
//!   everything else is dispatched here by [`crate::sai::Sai`] (the `MACsec` types go to the
//!   `MACsec` model before, [`crate::macsec`]).
//! - Each area module (`crate::lag`, `crate::l3`, ...) has a unit type implementing [`Area`] for
//!   the types it owns, the C++ `sai_<area>.cpp` they port. The `match` in [`by_owner!`] has an
//!   arm for every area.
//! - What a refused attribute answers is the type's C++ answer too: [`Area::refused_set`],
//!   [`refused_create`], [`refused_get`].
use crate::backend::Backend;
use crate::error::{Error, Result};
use crate::model::EntryKey;
use crate::oid::{EntryType, ObjectType, RawOid, SwitchId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// The module that implements an object type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Typed code in [`crate::sai`] (switch, ports); not dispatched here.
    Core,
    /// [`crate::l2`]
    L2,
    /// [`crate::lag`]
    Lag,
    /// [`crate::hostif`]
    Hostif,
    /// [`crate::l3`]
    L3,
    /// [`crate::acl`] (ACL objects and policers)
    Acl,
    /// [`crate::qos`]
    Qos,
    /// [`crate::mirror`]
    Mirror,
    /// [`crate::hash`]
    Hash,
}

/// The owner of every implemented object type ([`ObjectType::ALL`]; a test checks each is
/// listed once). The comment names the area that takes a bag type over.
pub const OWNERS: &[(ObjectType, Owner)] = &[
    (ObjectType::Switch, Owner::Core),
    (ObjectType::Port, Owner::Core),
    (ObjectType::Vlan, Owner::L2),
    (ObjectType::VlanMember, Owner::L2),
    (ObjectType::Bridge, Owner::L2),
    (ObjectType::BridgePort, Owner::L2),
    (ObjectType::Stp, Owner::L2),
    (ObjectType::VirtualRouter, Owner::L3),
    (ObjectType::RouterInterface, Owner::L3),
    (ObjectType::NextHop, Owner::L3),
    (ObjectType::NextHopGroup, Owner::L3),
    (ObjectType::NextHopGroupMember, Owner::L3),
    (ObjectType::Hostif, Owner::Hostif),
    (ObjectType::HostifTrapGroup, Owner::Hostif),
    (ObjectType::HostifTrap, Owner::Hostif),
    (ObjectType::HostifUserDefinedTrap, Owner::Hostif),
    (ObjectType::HostifTableEntry, Owner::Hostif),
    (ObjectType::Queue, Owner::Qos),
    (ObjectType::IngressPriorityGroup, Owner::Qos),
    (ObjectType::SchedulerGroup, Owner::Qos),
    (ObjectType::Scheduler, Owner::Qos),
    (ObjectType::Policer, Owner::Acl),
    (ObjectType::AclTable, Owner::Acl),
    (ObjectType::AclEntry, Owner::Acl),
    (ObjectType::AclCounter, Owner::Acl),
    (ObjectType::AclRange, Owner::Acl),
    (ObjectType::AclTableGroup, Owner::Acl),
    (ObjectType::AclTableGroupMember, Owner::Acl),
    (ObjectType::BufferPool, Owner::Qos),
    (ObjectType::BufferProfile, Owner::Qos),
    (ObjectType::QosMap, Owner::Qos),
    (ObjectType::Wred, Owner::Qos),
    (ObjectType::Hash, Owner::Hash),
    (ObjectType::MirrorSession, Owner::Mirror),
    (ObjectType::Samplepacket, Owner::Mirror),
    (ObjectType::Lag, Owner::Lag),
    (ObjectType::LagMember, Owner::Lag),
];

/// The owner of every entry type.
pub const ENTRY_OWNERS: &[(EntryType, Owner)] = &[
    (EntryType::Fdb, Owner::L2),
    (EntryType::Neighbor, Owner::L3),
    (EntryType::Route, Owner::L3),
];

/// The owner of an object type (`Core` for one the table does not list: a test checks there is
/// none).
pub fn owner(object_type: ObjectType) -> Owner {
    OWNERS
        .iter()
        .find(|(t, _)| *t == object_type)
        .map_or(Owner::Core, |(_, o)| *o)
}

/// The owner of an entry type.
pub fn entry_owner(entry_type: EntryType) -> Owner {
    ENTRY_OWNERS
        .iter()
        .find(|(t, _)| *t == entry_type)
        .map_or(Owner::Core, |(_, o)| *o)
}

/// The switch and ports ([`crate::sai`]): never dispatched; every method answers
/// `NOT_IMPLEMENTED`.
#[derive(Clone, Copy, Debug)]
pub struct CoreArea;

impl Area for CoreArea {}

/// What an operation that changes the model gets: the backend, the store and the switch.
#[derive(Debug)]
pub struct Cx<'a, B> {
    /// The backend.
    pub backend: &'a mut B,
    /// The object store.
    pub store: &'a mut Store,
    /// The switch.
    pub switch: SwitchId,
}

/// The operations an area implements for the object and entry types it owns. Dispatch asks an
/// area only about its own types, so a default (`NOT_IMPLEMENTED`) answers only a type nobody
/// implements; [`Area::get_all`] and [`Area::refused_set`] have real defaults.
pub trait Area {
    /// Creates an object of `object_type` (the switch id was checked).
    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        let _ = (cx, object_type, attrs);
        Err(Error::NotImplemented)
    }
    /// Removes an object (it exists and has this type).
    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        let _ = (cx, object_type, id);
        Err(Error::NotImplemented)
    }
    /// Sets one attribute (checked against the metadata, references live).
    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        let _ = (cx, object_type, id, attr);
        Err(Error::NotImplemented)
    }
    /// Gets one attribute.
    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = (backend, store, object_type, id, meta);
        Err(Error::NotImplemented)
    }
    /// Gets the attributes of one get call, in the order of `ids`: [`Area::get`] for each (an id
    /// that is no attribute of the type: [`refused_get`]); an error names its position. An area
    /// overrides this where the C++ does something once per call (the ACL counter's SDK read).
    fn get_all<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        ids.iter()
            .enumerate()
            .map(|(index, attr)| {
                let meta = sai_meta::attribute(object_type.sai(), *attr)
                    .ok_or_else(|| refused_get(object_type.sai(), index))?;
                Self::get(backend, store, object_type, id, meta).map_err(|e| e.at_index(index))
            })
            .collect()
    }
    /// Creates an entry.
    fn create_entry<B: Backend>(cx: Cx<'_, B>, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        let _ = (cx, key, attrs);
        Err(Error::NotImplemented)
    }
    /// Removes an entry.
    fn remove_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey) -> Result<()> {
        let _ = (cx, key);
        Err(Error::NotImplemented)
    }
    /// Sets one attribute of an entry.
    fn set_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey, attr: &Attr) -> Result<()> {
        let _ = (cx, key, attr);
        Err(Error::NotImplemented)
    }
    /// Gets attributes of an entry.
    fn get_entry<B: Backend>(
        backend: &B,
        store: &Store,
        key: &EntryKey,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        let _ = (backend, store, key, ids);
        Err(Error::NotImplemented)
    }
    /// What a set answers that carries an attribute the metadata does not let a set carry
    /// (create-only, read-only, or no attribute of the type): the rim refuses those before it
    /// decodes the value, so this is decided by the id alone, as the type's C++ `set_*` function
    /// answers it once the object was found. `None`: the metadata's refusal stands.
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<crate::error::Error> {
        let _ = (object_type, attr);
        None
    }
    /// `flush_fdb_entries` (asked of the owner of FDB entries).
    fn flush_fdb<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
        let _ = (cx, attrs);
        Err(Error::NotImplemented)
    }
}

/// Calls `$method` of the area that `$owner` names.
macro_rules! by_owner {
    ($owner:expr, $method:ident($($arg:expr),* $(,)?)) => {
        match $owner {
            Owner::Core => <CoreArea as Area>::$method($($arg),*),
            Owner::L2 => <crate::l2::L2Area as Area>::$method($($arg),*),
            Owner::Lag => <crate::lag::LagArea as Area>::$method($($arg),*),
            Owner::Hostif => <crate::hostif::HostifArea as Area>::$method($($arg),*),
            Owner::L3 => <crate::l3::L3Area as Area>::$method($($arg),*),
            Owner::Acl => <crate::acl::AclArea as Area>::$method($($arg),*),
            Owner::Qos => <crate::qos::QosArea as Area>::$method($($arg),*),
            Owner::Mirror => <crate::mirror::MirrorArea as Area>::$method($($arg),*),
            Owner::Hash => <crate::hash::HashArea as Area>::$method($($arg),*),
        }
    };
}

pub(crate) fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
    by_owner!(owner(object_type), create(cx, object_type, attrs))
}

pub(crate) fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
    by_owner!(owner(object_type), remove(cx, object_type, id))
}

pub(crate) fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
    by_owner!(owner(object_type), set(cx, object_type, id, attr))
}

pub(crate) fn get_all<B: Backend>(
    backend: &B,
    store: &Store,
    object_type: ObjectType,
    id: RawOid,
    ids: &[sys::sai_attr_id_t],
) -> Result<Vec<Value>> {
    by_owner!(owner(object_type), get_all(backend, store, object_type, id, ids))
}

pub(crate) fn create_entry<B: Backend>(cx: Cx<'_, B>, key: EntryKey, attrs: &[Attr]) -> Result<()> {
    by_owner!(entry_owner(key.entry_type()), create_entry(cx, key, attrs))
}

pub(crate) fn remove_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey) -> Result<()> {
    by_owner!(entry_owner(key.entry_type()), remove_entry(cx, key))
}

pub(crate) fn set_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey, attr: &Attr) -> Result<()> {
    by_owner!(entry_owner(key.entry_type()), set_entry(cx, key, attr))
}

pub(crate) fn get_entry<B: Backend>(
    backend: &B,
    store: &Store,
    key: &EntryKey,
    ids: &[sys::sai_attr_id_t],
) -> Result<Vec<Value>> {
    by_owner!(entry_owner(key.entry_type()), get_entry(backend, store, key, ids))
}

/// The usual C++ answer to a set refused by id ([`Area::refused_set`]): `attr_invalid(0)` for the
/// ids the type's `set_*` function names as create-only (`invalid`), its fallthrough
/// `attr_not_supported(0)` for every other one.
pub(crate) fn cpp_refusal(attr: sys::sai_attr_id_t, invalid: &[sys::sai_attr_id_t]) -> crate::error::Error {
    use crate::error::{AttrError, Error};
    Error::attr(
        if invalid.contains(&attr) {
            AttrError::InvalidValue
        } else {
            AttrError::NotSupported
        },
        0,
    )
}

/// What a create answers for an attribute the metadata does not let a create carry (read-only,
/// or no attribute of the type) at position `index`: the type's C++ `create_*` answers it
/// `attr_not_supported(i)` (`attr_unknown(i)` for an unknown id of a `QOS_MAP` or `SCHEDULER`,
/// `sai_qos.cpp:157` / :315); the types the C++ never creates answer `NOT_SUPPORTED` whatever the
/// list holds (`sai_port.cpp:353`, `sai_l2.cpp:381` / :746, `sai_qos.cpp:423`, `sai_hash.cpp:68`; queues
/// and priority groups have no create function). The rim refuses those attributes before it
/// decodes their value, so this is decided by the id alone; [`crate::attrs::CreateArgs::check`]
/// answers the same for a list decoded already.
pub fn refused_create(
    object_type: sys::sai_object_type_t,
    attr: sys::sai_attr_id_t,
    index: usize,
) -> crate::error::Error {
    use crate::error::{AttrError, Error};
    match ObjectType::from_sai(object_type) {
        Some(
            ObjectType::Port
            | ObjectType::Bridge
            | ObjectType::Stp
            | ObjectType::SchedulerGroup
            | ObjectType::Hash
            | ObjectType::Queue
            | ObjectType::IngressPriorityGroup,
        ) => Error::NotSupported,
        Some(ObjectType::QosMap | ObjectType::Scheduler) if sai_meta::attribute(object_type, attr).is_none() => {
            Error::attr(AttrError::Unknown, index)
        }
        _ => Error::attr(AttrError::NotSupported, index),
    }
}

/// What a get answers for an id that is no attribute of the type, at position `index`: the C++
/// `get_*` fallthrough, `attr_not_supported(i)` (`attr_unknown(i)` for `QOS_MAP`, `SCHEDULER`
/// and `SCHEDULER_GROUP`, `sai_qos.cpp:227` / :388 / :483).
pub fn refused_get(object_type: sys::sai_object_type_t, index: usize) -> crate::error::Error {
    use crate::error::{AttrError, Error};
    let kind = match ObjectType::from_sai(object_type) {
        Some(ObjectType::QosMap | ObjectType::Scheduler | ObjectType::SchedulerGroup) => AttrError::Unknown,
        _ => AttrError::NotSupported,
    };
    Error::attr(kind, index)
}

/// [`Area::refused_set`] of the type's owner.
pub(crate) fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<crate::error::Error> {
    // the methods have a backend type parameter only where they take a backend
    match owner(object_type) {
        Owner::Core => <CoreArea as Area>::refused_set(object_type, attr),
        Owner::L2 => <crate::l2::L2Area as Area>::refused_set(object_type, attr),
        Owner::Lag => <crate::lag::LagArea as Area>::refused_set(object_type, attr),
        Owner::Hostif => <crate::hostif::HostifArea as Area>::refused_set(object_type, attr),
        Owner::L3 => <crate::l3::L3Area as Area>::refused_set(object_type, attr),
        Owner::Acl => <crate::acl::AclArea as Area>::refused_set(object_type, attr),
        Owner::Qos => <crate::qos::QosArea as Area>::refused_set(object_type, attr),
        Owner::Mirror => <crate::mirror::MirrorArea as Area>::refused_set(object_type, attr),
        Owner::Hash => <crate::hash::HashArea as Area>::refused_set(object_type, attr),
    }
}

pub(crate) fn flush_fdb<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
    by_owner!(entry_owner(EntryType::Fdb), flush_fdb(cx, attrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_has_exactly_one_owner() {
        for t in ObjectType::ALL {
            assert_eq!(OWNERS.iter().filter(|(o, _)| o == t).count(), 1, "{t:?}");
        }
        assert_eq!(OWNERS.len(), ObjectType::ALL.len());
        for t in EntryType::ALL {
            assert_eq!(ENTRY_OWNERS.iter().filter(|(o, _)| o == t).count(), 1, "{t:?}");
        }
        assert_eq!(owner(ObjectType::Lag), Owner::Lag);
        assert_eq!(owner(ObjectType::LagMember), Owner::Lag);
        assert_eq!(owner(ObjectType::Port), Owner::Core);
    }
}

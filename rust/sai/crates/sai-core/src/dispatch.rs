//! Who implements each object type: one table, so an area module takes over a type from the
//! attribute bags with a one-line change and parallel area work does not meet in one `match`.
//!
//! - [`OWNERS`] / [`ENTRY_OWNERS`] name the [`Owner`] of every object and entry type. Types with
//!   typed code in [`crate::sai`] (switch, port, VLAN, bridge) are [`Owner::Core`] and never reach
//!   this module; everything else is dispatched here by [`crate::sai::Sai`].
//! - Each area module (`crate::lag`, `crate::l3`, ...) has a unit type implementing [`Area`].
//!   Every method has a default body that is the bag behaviour ([`crate::bag`]), so an area
//!   overrides exactly what it has ported and inherits the rest.
//! - Taking a type over: implement the methods in the area module, then change the type's
//!   line in [`OWNERS`] from `Owner::Bag` to the area. The `match` in [`by_owner!`] already has
//!   an arm for every area, so nobody edits it.
//!
//! Proof of the mechanism: `LAG` and `LAG_MEMBER` are [`Owner::Lag`] ([`crate::lag`]), whose
//! member rules and `PORT_LIST` moved out of `bag.rs` unchanged.
use crate::backend::Backend;
use crate::error::Result;
use crate::model::EntryKey;
use crate::oid::{EntryType, ObjectType, RawOid, SwitchId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// The module that implements an object type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Typed code in [`crate::sai`] (switch, ports, VLANs, the bridge); not dispatched here.
    Core,
    /// The metadata-driven attribute bags ([`crate::bag`]): kept, not programmed by type.
    Bag,
    /// [`crate::port`]
    Port,
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
    (ObjectType::Vlan, Owner::Core),
    (ObjectType::VlanMember, Owner::Core),
    (ObjectType::Bridge, Owner::Core),
    (ObjectType::BridgePort, Owner::Core),
    (ObjectType::Stp, Owner::Bag),                   // -> L2
    (ObjectType::VirtualRouter, Owner::Bag),         // -> L3
    (ObjectType::RouterInterface, Owner::Bag),       // -> L3
    (ObjectType::NextHop, Owner::Bag),               // -> L3
    (ObjectType::NextHopGroup, Owner::Bag),          // -> L3
    (ObjectType::NextHopGroupMember, Owner::Bag),    // -> L3
    (ObjectType::Hostif, Owner::Bag),                // -> Hostif
    (ObjectType::HostifTrapGroup, Owner::Bag),       // -> Hostif
    (ObjectType::HostifTrap, Owner::Bag),            // -> Hostif
    (ObjectType::HostifUserDefinedTrap, Owner::Bag), // -> Hostif
    (ObjectType::HostifTableEntry, Owner::Bag),      // -> Hostif
    (ObjectType::Queue, Owner::Bag),                 // -> Qos
    (ObjectType::IngressPriorityGroup, Owner::Bag),  // -> Qos
    (ObjectType::SchedulerGroup, Owner::Bag),        // -> Qos
    (ObjectType::Scheduler, Owner::Bag),             // -> Qos
    (ObjectType::Policer, Owner::Bag),               // -> Acl
    (ObjectType::BufferPool, Owner::Bag),            // -> Qos
    (ObjectType::BufferProfile, Owner::Bag),         // -> Qos
    (ObjectType::QosMap, Owner::Bag),                // -> Qos
    (ObjectType::Wred, Owner::Bag),                  // -> Qos
    (ObjectType::Hash, Owner::Bag),                  // -> Hash
    (ObjectType::Lag, Owner::Lag),
    (ObjectType::LagMember, Owner::Lag),
];

/// The owner of every entry type.
pub const ENTRY_OWNERS: &[(EntryType, Owner)] = &[
    (EntryType::Fdb, Owner::Bag),      // -> L2 (with flush_fdb_entries)
    (EntryType::Neighbor, Owner::Bag), // -> L3
    (EntryType::Route, Owner::Bag),    // -> L3
];

/// The owner of an object type (`Bag` for one the table does not list).
pub fn owner(object_type: ObjectType) -> Owner {
    OWNERS
        .iter()
        .find(|(t, _)| *t == object_type)
        .map_or(Owner::Bag, |(_, o)| *o)
}

/// The owner of an entry type.
pub fn entry_owner(entry_type: EntryType) -> Owner {
    ENTRY_OWNERS
        .iter()
        .find(|(t, _)| *t == entry_type)
        .map_or(Owner::Bag, |(_, o)| *o)
}

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

/// The operations an area implements for the object and entry types it owns. Every default is
/// the attribute-bag behaviour.
pub trait Area {
    /// Creates an object of `object_type` (the switch id was checked).
    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        crate::bag::create(cx.backend, cx.store, object_type, attrs)
    }
    /// Removes an object (it exists and has this type).
    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        crate::bag::remove(cx.backend, cx.store, object_type, id)
    }
    /// Sets one attribute (checked against the metadata, references live).
    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        crate::bag::set(cx.backend, cx.store, object_type, id, attr)
    }
    /// Gets one attribute.
    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = (backend, object_type);
        crate::bag::get(store, id, meta)
    }
    /// Creates an entry.
    fn create_entry<B: Backend>(cx: Cx<'_, B>, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        crate::bag::create_entry(cx.backend, cx.store, key, attrs)
    }
    /// Removes an entry.
    fn remove_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey) -> Result<()> {
        crate::bag::remove_entry(cx.backend, cx.store, key)
    }
    /// Sets one attribute of an entry.
    fn set_entry<B: Backend>(cx: Cx<'_, B>, key: &EntryKey, attr: &Attr) -> Result<()> {
        crate::bag::set_entry(cx.backend, cx.store, key, attr)
    }
    /// Gets attributes of an entry.
    fn get_entry<B: Backend>(
        backend: &B,
        store: &Store,
        key: &EntryKey,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        let _ = backend;
        crate::bag::get_entry(store, key, ids)
    }
    /// `flush_fdb_entries` (asked of the owner of FDB entries).
    fn flush_fdb<B: Backend>(cx: Cx<'_, B>, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
        crate::bag::flush_fdb(cx.backend, cx.store, attrs)
    }
}

/// Calls `$method` of the area that `$owner` names.
macro_rules! by_owner {
    ($owner:expr, $method:ident($($arg:expr),* $(,)?)) => {
        match $owner {
            Owner::Core | Owner::Bag => <crate::bag::BagArea as Area>::$method($($arg),*),
            Owner::Port => <crate::port::PortArea as Area>::$method($($arg),*),
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

pub(crate) fn get<B: Backend>(
    backend: &B,
    store: &Store,
    object_type: ObjectType,
    id: RawOid,
    meta: &AttrMeta,
) -> Result<Value> {
    by_owner!(owner(object_type), get(backend, store, object_type, id, meta))
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

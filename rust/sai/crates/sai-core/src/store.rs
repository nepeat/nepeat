//! The object store: owner of every object, keeper of the reference rules.
//!
//! - An id is given out once: indices of created objects count up and are not reused, so an
//!   id kept after its object was removed never names a newer object.
//! - An object can only be stored when everything it refers to exists.
//! - An object that something refers to cannot be removed (`ObjectInUse`).
//!
//! Objects without an id (FDB, neighbor and route entries) live here too, under their key
//! ([`EntryKey`]); what their key and attributes name is referred to like anything else.
//!
//! [`Store::check_invariants`] states these as a checkable property; the property tests call
//! it after every operation.
use crate::error::{Error, Result};
use crate::model::{Bag, EntryKey, Kept, Kind, Object, references_of};
use crate::oid::{ObjectType, RawOid, TypedId};
use std::collections::BTreeMap;

#[derive(Debug)]
struct Entry {
    object: Object,
    /// How many stored objects and entries refer to this one.
    referrers: u32,
}

/// The object store of one switch.
#[derive(Debug, Default)]
pub struct Store {
    objects: BTreeMap<RawOid, Entry>,
    entries: BTreeMap<EntryKey, Kept>,
    next_index: BTreeMap<ObjectType, u64>,
}

/// What an entry refers to: the object in its key and the objects its attributes name.
fn entry_references(key: &EntryKey, attrs: &Kept) -> Vec<RawOid> {
    let mut targets = references_of(attrs);
    targets.push(key.owner());
    targets
}

impl Store {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id the next object of this type gets from [`Store::insert`] /
    /// [`Store::insert_bag`] (so that a backend can be told before the object is stored).
    pub fn peek_id(&self, object_type: ObjectType) -> RawOid {
        object_type.id(self.next_index.get(&object_type).copied().unwrap_or(1))
    }

    /// Stores `object` under a fresh id of its type.
    fn insert_fresh(&mut self, object: Object) -> Result<RawOid> {
        let object_type = object.object_type();
        let id = self.peek_id(object_type);
        self.insert_object(id, object)?;
        // Only an id that was given out is used up.
        let next = self.next_index.entry(object_type).or_insert(1);
        *next += 1;
        Ok(id)
    }

    fn insert_object(&mut self, id: RawOid, object: Object) -> Result<()> {
        if id.type_bits() != object.object_type().sai() {
            return Err(Error::InvalidObjectType);
        }
        if self.objects.contains_key(&id) {
            return Err(Error::ItemAlreadyExists);
        }
        self.retain_all(&object.references())?;
        self.objects.insert(id, Entry { object, referrers: 0 });
        Ok(())
    }

    /// Replaces a stored object by another of the same type, moving its references; when a
    /// new reference names a missing object nothing changes.
    fn replace_object(&mut self, id: RawOid, after: Object) -> Result<()> {
        let old_refs = self.objects.get(&id).ok_or(Error::ItemNotFound)?.object.references();
        self.retain_all(&after.references())?;
        self.release_all(&old_refs);
        let entry = self.objects.get_mut(&id).ok_or(Error::ItemNotFound)?;
        entry.object = after;
        Ok(())
    }

    fn remove_object(&mut self, id: RawOid) -> Result<Object> {
        match self.objects.get(&id) {
            Some(entry) if entry.referrers > 0 => return Err(Error::ObjectInUse),
            Some(_) => {}
            None => return Err(Error::ItemNotFound),
        }
        let entry = self.objects.remove(&id).ok_or(Error::ItemNotFound)?;
        self.release_all(&entry.object.references());
        Ok(entry.object)
    }

    /// Stores a new object under a fresh id.
    pub fn insert<K: Kind>(&mut self, object: K) -> Result<K::Id> {
        let raw = self.insert_fresh(object.wrap())?;
        K::Id::try_from(raw)
    }

    /// Stores a new object under an id the caller chose (ports: the chip's port number).
    pub fn insert_at<K: Kind>(&mut self, id: K::Id, object: K) -> Result<()> {
        self.insert_object(id.raw(), object.wrap())
    }

    /// The object with this id; `ItemNotFound` when there is none (never created, or removed).
    pub fn get<K: Kind>(&self, id: K::Id) -> Result<&K> {
        self.objects
            .get(&id.raw())
            .and_then(|e| K::peel(&e.object))
            .ok_or(Error::ItemNotFound)
    }

    /// Changes an object in place. References it gains or drops are accounted; when a new
    /// reference names a missing object the change is undone and the call fails.
    pub fn update<K: Kind + Clone, T>(&mut self, id: K::Id, change: impl FnOnce(&mut K) -> T) -> Result<T> {
        let mut after = self.get::<K>(id)?.clone();
        let result = change(&mut after);
        self.replace_object(id.raw(), after.wrap())?;
        Ok(result)
    }

    /// `ObjectInUse` while something refers to the object, `ItemNotFound` when it is absent.
    pub fn ensure_removable<K: Kind>(&self, id: K::Id) -> Result<()> {
        match self.objects.get(&id.raw()) {
            Some(entry) if K::peel(&entry.object).is_none() => Err(Error::ItemNotFound),
            Some(entry) if entry.referrers > 0 => Err(Error::ObjectInUse),
            Some(_) => Ok(()),
            None => Err(Error::ItemNotFound),
        }
    }

    /// Removes an object nothing refers to and returns it.
    pub fn remove<K: Kind>(&mut self, id: K::Id) -> Result<K> {
        self.ensure_removable::<K>(id)?;
        K::unwrap(self.remove_object(id.raw())?).ok_or(Error::Failure)
    }

    /// Every object of one type, in id order.
    pub fn iter<K: Kind>(&self) -> impl Iterator<Item = (K::Id, &K)> {
        self.objects
            .range(K::Id::TYPE.id_range())
            .filter_map(|(raw, entry)| Some((K::Id::try_from(*raw).ok()?, K::peel(&entry.object)?)))
    }

    /// Stores a new attribute-bag object under a fresh id.
    pub fn insert_bag(&mut self, bag: Bag) -> Result<RawOid> {
        self.insert_fresh(Object::Bag(bag))
    }

    /// Stores a new attribute-bag object under an id the caller chose (queues: port and
    /// queue number). The id must be of the bag's type.
    pub fn insert_bag_at(&mut self, id: RawOid, bag: Bag) -> Result<()> {
        self.insert_object(id, Object::Bag(bag))
    }

    /// The attribute-bag object with this id.
    pub fn bag(&self, id: RawOid) -> Result<&Bag> {
        match self.objects.get(&id).map(|e| &e.object) {
            Some(Object::Bag(bag)) => Ok(bag),
            _ => Err(Error::ItemNotFound),
        }
    }

    /// Changes the attributes of an attribute-bag object, with the reference accounting of
    /// [`Store::update`].
    pub fn update_bag<T>(&mut self, id: RawOid, change: impl FnOnce(&mut Kept) -> T) -> Result<T> {
        let mut after = self.bag(id)?.clone();
        let result = change(&mut after.attrs);
        self.replace_object(id, Object::Bag(after))?;
        Ok(result)
    }

    /// `ObjectInUse` while something refers to the attribute-bag object.
    pub fn ensure_bag_removable(&self, id: RawOid) -> Result<()> {
        self.bag(id)?;
        if self.referrers(id) > 0 {
            Err(Error::ObjectInUse)
        } else {
            Ok(())
        }
    }

    /// Removes an attribute-bag object nothing refers to and returns it.
    pub fn remove_bag(&mut self, id: RawOid) -> Result<Bag> {
        self.ensure_bag_removable(id)?;
        match self.remove_object(id)? {
            Object::Bag(bag) => Ok(bag),
            _ => Err(Error::Failure),
        }
    }

    /// Every attribute-bag object of one type, in id order.
    pub fn bags(&self, object_type: ObjectType) -> impl Iterator<Item = (RawOid, &Bag)> {
        self.objects
            .range(object_type.id_range())
            .filter_map(|(raw, entry)| match &entry.object {
                Object::Bag(bag) => Some((*raw, bag)),
                _ => None,
            })
    }

    /// Stores a new entry; everything its key and attributes name must exist.
    pub fn insert_entry(&mut self, key: EntryKey, attrs: Kept) -> Result<()> {
        if self.entries.contains_key(&key) {
            return Err(Error::ItemAlreadyExists);
        }
        self.retain_all(&entry_references(&key, &attrs))?;
        self.entries.insert(key, attrs);
        Ok(())
    }

    /// The attributes of an entry.
    pub fn entry(&self, key: &EntryKey) -> Result<&Kept> {
        self.entries.get(key).ok_or(Error::ItemNotFound)
    }

    /// Changes the attributes of an entry, with the reference accounting of [`Store::update`].
    pub fn update_entry<T>(&mut self, key: &EntryKey, change: impl FnOnce(&mut Kept) -> T) -> Result<T> {
        let before = self.entry(key)?;
        let mut after = before.clone();
        let old_refs = entry_references(key, before);
        let result = change(&mut after);
        self.retain_all(&entry_references(key, &after))?;
        self.release_all(&old_refs);
        self.entries.insert(*key, after);
        Ok(result)
    }

    /// Removes an entry and returns its attributes.
    pub fn remove_entry(&mut self, key: &EntryKey) -> Result<Kept> {
        let attrs = self.entries.remove(key).ok_or(Error::ItemNotFound)?;
        self.release_all(&entry_references(key, &attrs));
        Ok(attrs)
    }

    /// Every entry, in key order.
    pub fn entries(&self) -> impl Iterator<Item = (&EntryKey, &Kept)> {
        self.entries.iter()
    }

    /// Number of stored entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Whether an object with this raw id exists.
    pub fn contains(&self, raw: RawOid) -> bool {
        self.objects.contains_key(&raw)
    }

    /// The type of the object with this raw id.
    pub fn object_type(&self, raw: RawOid) -> Option<ObjectType> {
        self.objects.get(&raw).map(|e| e.object.object_type())
    }

    /// How many objects and entries refer to this one (0 when it does not exist).
    pub fn referrers(&self, raw: RawOid) -> u32 {
        self.objects.get(&raw).map_or(0, |e| e.referrers)
    }

    /// Number of stored objects (entries not counted).
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Checks the store's rules from scratch: every reference names a stored object, every
    /// reference count equals the number of references found, every object sits under an id
    /// of its own type. `Err` describes the first violation.
    pub fn check_invariants(&self) -> core::result::Result<(), String> {
        let mut counted: BTreeMap<RawOid, u32> = BTreeMap::new();
        for (raw, entry) in &self.objects {
            let type_of_object = entry.object.object_type();
            if ObjectType::from_sai(raw.type_bits()) != Some(type_of_object) {
                return Err(format!("{raw:?} holds a {type_of_object:?}"));
            }
            for target in entry.object.references() {
                if !self.objects.contains_key(&target) {
                    return Err(format!("{raw:?} refers to {target:?}, which does not exist"));
                }
                *counted.entry(target).or_default() += 1;
            }
        }
        for (key, attrs) in &self.entries {
            for target in entry_references(key, attrs) {
                if !self.objects.contains_key(&target) {
                    return Err(format!("{key:?} refers to {target:?}, which does not exist"));
                }
                *counted.entry(target).or_default() += 1;
            }
        }
        for (raw, entry) in &self.objects {
            let found = counted.get(raw).copied().unwrap_or(0);
            if found != entry.referrers {
                return Err(format!(
                    "{raw:?}: {} references counted, {found} found",
                    entry.referrers
                ));
            }
        }
        for (object_type, next) in &self.next_index {
            if let Some((raw, _)) = self
                .objects
                .range(object_type.id(*next)..=*object_type.id_range().end())
                .next()
            {
                // Ids chosen by the caller (ports, queues) belong to types nobody creates.
                return Err(format!("{raw:?} is at or beyond the next fresh index {next}"));
            }
        }
        Ok(())
    }

    /// Counts one more reference to each target; all or nothing.
    fn retain_all(&mut self, targets: &[RawOid]) -> Result<()> {
        if targets.iter().any(|t| !self.objects.contains_key(t)) {
            return Err(Error::InvalidObjectId);
        }
        for target in targets {
            if let Some(entry) = self.objects.get_mut(target) {
                entry.referrers += 1;
            }
        }
        Ok(())
    }

    fn release_all(&mut self, targets: &[RawOid]) {
        for target in targets {
            if let Some(entry) = self.objects.get_mut(target) {
                entry.referrers = entry.referrers.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{HwPort, Vid};
    use crate::model::{Bridge, BridgePort, Kept, Port, PortKind, Vlan, VlanMember};
    use crate::oid::{BridgePortId, PortId, VlanId};

    fn port(hw: u16) -> Port {
        Port {
            hw: HwPort(hw),
            kind: PortKind::Front { lanes: vec![1] },
            admin: false,
            speed: 1000,
            oper_up: false,
            kept: Kept::new(),
        }
    }

    #[test]
    fn references_keep_objects_alive_and_ids_are_not_reused() {
        let mut s = Store::new();
        let p = PortId::from_index(1);
        s.insert_at(p, port(1)).unwrap();
        assert_eq!(s.insert_at(p, port(1)), Err(Error::ItemAlreadyExists));
        let bridge = s.insert(Bridge { kept: Kept::new() }).unwrap();
        let bp = s
            .insert(BridgePort {
                bridge,
                port: p,
                admin: true,
                kept: Kept::new(),
            })
            .unwrap();
        let vlan = s
            .insert(Vlan {
                vid: Vid::new(10).unwrap(),
                kept: Kept::new(),
            })
            .unwrap();
        let member = s
            .insert(VlanMember {
                vlan,
                bridge_port: bp,
                untagged: true,
            })
            .unwrap();
        s.check_invariants().unwrap();

        assert_eq!(s.remove::<Vlan>(vlan).unwrap_err(), Error::ObjectInUse);
        assert_eq!(s.remove::<BridgePort>(bp).unwrap_err(), Error::ObjectInUse);
        assert_eq!(s.remove::<Port>(p).unwrap_err(), Error::ObjectInUse);
        s.remove::<VlanMember>(member).unwrap();
        assert_eq!(s.remove::<VlanMember>(member).unwrap_err(), Error::ItemNotFound);
        s.remove::<Vlan>(vlan).unwrap();
        s.check_invariants().unwrap();

        // a stale id stays stale: the next VLAN gets a new one
        let vlan2 = s
            .insert(Vlan {
                vid: Vid::new(10).unwrap(),
                kept: Kept::new(),
            })
            .unwrap();
        assert_ne!(vlan2, vlan);
        assert_eq!(s.get::<Vlan>(vlan).unwrap_err(), Error::ItemNotFound);

        // a reference to a missing object is refused and leaves no trace
        let before = s.len();
        assert_eq!(
            s.insert(VlanMember {
                vlan,
                bridge_port: bp,
                untagged: true
            })
            .unwrap_err(),
            Error::InvalidObjectId
        );
        assert_eq!(
            s.insert(VlanMember {
                vlan: vlan2,
                bridge_port: BridgePortId::from_index(99),
                untagged: true
            })
            .unwrap_err(),
            Error::InvalidObjectId
        );
        assert_eq!(s.len(), before);
        s.check_invariants().unwrap();
        assert_eq!(s.iter::<Vlan>().map(|(id, _)| id).collect::<Vec<VlanId>>(), [vlan2]);
    }

    #[test]
    fn update_moves_references() {
        let mut s = Store::new();
        let (p1, p2) = (PortId::from_index(1), PortId::from_index(2));
        s.insert_at(p1, port(1)).unwrap();
        s.insert_at(p2, port(2)).unwrap();
        let bridge = s.insert(Bridge { kept: Kept::new() }).unwrap();
        let bp = s
            .insert(BridgePort {
                bridge,
                port: p1,
                admin: true,
                kept: Kept::new(),
            })
            .unwrap();
        s.update::<BridgePort, _>(bp, |b| b.port = p2).unwrap();
        s.check_invariants().unwrap();
        s.remove::<Port>(p1).unwrap();
        assert_eq!(s.remove::<Port>(p2).unwrap_err(), Error::ObjectInUse);
        // a change to a missing target is refused and undone
        assert_eq!(
            s.update::<BridgePort, _>(bp, |b| b.port = p1).unwrap_err(),
            Error::InvalidObjectId
        );
        assert_eq!(s.get::<BridgePort>(bp).unwrap().port, p2);
        s.check_invariants().unwrap();
    }
}

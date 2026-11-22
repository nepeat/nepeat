//! The object store: owner of every object, keeper of the reference rules.
//!
//! - An id is given out once: indices of created objects count up and are not reused, so an
//!   id kept after its object was removed never names a newer object.
//! - An object can only be stored when everything it refers to exists.
//! - An object that something refers to cannot be removed (`ObjectInUse`).
//!
//! [`Store::check_invariants`] states these as a checkable property; the property tests call
//! it after every operation.
use crate::error::{Error, Result};
use crate::model::{Kind, Object};
use crate::oid::{ObjectType, RawOid, TypedId};
use std::collections::BTreeMap;

#[derive(Debug)]
struct Entry {
    object: Object,
    /// How many stored objects refer to this one.
    referrers: u32,
}

/// The object store of one switch.
#[derive(Debug, Default)]
pub struct Store {
    objects: BTreeMap<RawOid, Entry>,
    next_index: BTreeMap<ObjectType, u64>,
}

impl Store {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a new object under a fresh id.
    pub fn insert<K: Kind>(&mut self, object: K) -> Result<K::Id> {
        let next = self.next_index.entry(K::Id::TYPE).or_insert(1);
        let id = K::Id::from_index(*next);
        *next += 1;
        self.insert_at(id, object)?;
        Ok(id)
    }

    /// Stores a new object under an id the caller chose (ports: the chip's port number).
    pub fn insert_at<K: Kind>(&mut self, id: K::Id, object: K) -> Result<()> {
        if self.objects.contains_key(&id.raw()) {
            return Err(Error::ItemAlreadyExists);
        }
        let object = object.wrap();
        self.retain_all(&object.references())?;
        self.objects.insert(id.raw(), Entry { object, referrers: 0 });
        Ok(())
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
        let before = self.get::<K>(id)?.clone();
        let mut after = before.clone();
        let result = change(&mut after);
        let (old_refs, after) = (before.wrap().references(), after.wrap());
        self.retain_all(&after.references())?;
        self.release_all(&old_refs);
        let entry = self.objects.get_mut(&id.raw()).ok_or(Error::ItemNotFound)?;
        entry.object = after;
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
        let entry = self.objects.remove(&id.raw()).ok_or(Error::ItemNotFound)?;
        self.release_all(&entry.object.references());
        K::unwrap(entry.object).ok_or(Error::Failure)
    }

    /// Every object of one type, in id order.
    pub fn iter<K: Kind>(&self) -> impl Iterator<Item = (K::Id, &K)> {
        self.objects
            .range(K::Id::TYPE.id_range())
            .filter_map(|(raw, entry)| Some((K::Id::try_from(*raw).ok()?, K::peel(&entry.object)?)))
    }

    /// Whether an object with this raw id exists.
    pub fn contains(&self, raw: RawOid) -> bool {
        self.objects.contains_key(&raw)
    }

    /// Number of stored objects.
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
            let type_of_object = match &entry.object {
                Object::Switch(_) => ObjectType::Switch,
                Object::Port(_) => ObjectType::Port,
                Object::Vlan(_) => ObjectType::Vlan,
                Object::VlanMember(_) => ObjectType::VlanMember,
                Object::Bridge(_) => ObjectType::Bridge,
                Object::BridgePort(_) => ObjectType::BridgePort,
            };
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
        for (raw, entry) in &self.objects {
            let found = counted.get(raw).copied().unwrap_or(0);
            if found != entry.referrers {
                return Err(format!(
                    "{raw:?}: {} references counted, {found} found",
                    entry.referrers
                ));
            }
        }
        Ok(())
    }

    /// Counts one more reference to each target; all or nothing.
    fn retain_all(&mut self, targets: &[RawOid]) -> Result<()> {
        if let Some(missing) = targets.iter().find(|t| !self.objects.contains_key(t)) {
            let _ = missing;
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

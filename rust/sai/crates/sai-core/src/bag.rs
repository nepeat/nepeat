//! The reference rule every object type shares: an object id an attribute carries must name a
//! live object (the store then counts the reference: the named object cannot be removed,
//! `OBJECT_IN_USE`).
//!
//! This module held the attribute-bag implementation (one metadata-driven create / set / get /
//! remove for every type without code of its own, over the backend's generic `object_*` /
//! `entry_*` methods) until G1, when every type got its area ([`crate::dispatch::OWNERS`]); the
//! store's [`crate::model::Bag`] record stays for the objects the switch comes with (STP instance,
//! hashes, queues, priority and scheduler groups), which the areas answer for.
use crate::error::{AttrError, Error, Result};
use crate::store::Store;
use crate::value::Attr;

/// Every object id in `attrs` must name a live object; the error names the attribute.
pub(crate) fn check_references(store: &Store, attrs: &[Attr]) -> Result<()> {
    for (index, attr) in attrs.iter().enumerate() {
        // MACsec objects live in the MACsec model, which checks its own references
        if attr
            .value
            .object_ids()
            .iter()
            .any(|oid| !store.contains(*oid) && !crate::macsec::held_by_model(*oid))
        {
            return Err(Error::attr(AttrError::InvalidValue, index));
        }
    }
    Ok(())
}

//! ACL and policers: tables as field groups, entries as field entries (installed while the
//! table is bound; re-created on change with counters carried over, or updated in place with
//! [`crate::backend::types::FpCap::EntryUpdate`]), counters, ranges, table groups, policers, and the
//! `MACsec`-stage diversion of tables to the `MACsec` model ([`crate::macsec`]). Ports `sai_acl.cpp`
//! (1598 lines) without its `CoPP` part ([`crate::copp`]).
//!
//! Backend: [`crate::backend::Field`], [`crate::backend::Policer`].
//!
//! Registration: [`crate::dispatch::Owner::Acl`] for `POLICER` in [`crate::dispatch::OWNERS`],
//! and for `ACL_TABLE`, `ACL_ENTRY`, `ACL_COUNTER`, `ACL_RANGE`, `ACL_TABLE_GROUP(_MEMBER)` once
//! they are [`crate::oid::ObjectType`]s (`oid.rs` and the rim's ACL value decode: orchestrator).
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct AclArea;

impl Area for AclArea {}

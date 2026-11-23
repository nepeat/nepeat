//! Host interfaces and traps: KNET netdevs with their RX filter, generic netlink, trap groups,
//! the 27 SAI trap types on 11 [`crate::backend::types::TrapKind`]s, netif carrier from
//! `OPER_STATUS`, the `CoPP` refresh on every change ([`crate::copp`]). Ports `sai_hostif.cpp`
//! (543 lines; a backend with `trap_program` owns a trap fully, else `trap_set` + `CoPP` field
//! entries).
//!
//! Backend: [`crate::backend::Hostif`] (it replaces [`crate::backend::Backend::hostif_create`] and
//! the `object_*` calls for host interfaces).
//!
//! Registration: [`crate::dispatch::Owner::Hostif`] for `HOSTIF`, `HOSTIF_TRAP_GROUP`,
//! `HOSTIF_TRAP`, `HOSTIF_USER_DEFINED_TRAP` (C++: `NOT_SUPPORTED`), `HOSTIF_TABLE_ENTRY` in
//! [`crate::dispatch::OWNERS`].
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct HostifArea;

impl Area for HostifArea {}

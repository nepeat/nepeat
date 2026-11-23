//! The switch's `LAG_HASH` / `ECMP_HASH` objects and seeds, port storm-control policers, VLAN
//! flood control. On a backend `NotSupported` the value is kept as a stub. Ports `sai_hash.cpp`
//! (174 lines).
//!
//! Backend: [`crate::backend::Hash`], [`crate::backend::Storm`].
//!
//! Registration: [`crate::dispatch::Owner::Hash`] for `HASH` in [`crate::dispatch::OWNERS`]; the
//! storm and flood attributes come from [`crate::port`] and the VLAN code.
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct HashArea;

impl Area for HashArea {}

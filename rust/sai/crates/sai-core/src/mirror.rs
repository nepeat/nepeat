//! Mirror sessions (`LOCAL`, `ENHANCED_REMOTE` GRE), `SAMPLEPACKET` (`SLOW_PATH`), the port's
//! mirror and sample attributes. Kept only unless [`crate::backend::Mirror::mirror_supported`].
//! Ports `sai_mirror.cpp` (390 lines).
//!
//! Backend: [`crate::backend::Mirror`], [`crate::backend::Sample`],
//! [`crate::backend::Hostif::genetlink_create`] and friends (`sFlow`).
//!
//! Registration: [`crate::dispatch::Owner::Mirror`] for `MIRROR_SESSION` and `SAMPLEPACKET` once
//! they are [`crate::oid::ObjectType`]s (`oid.rs` and the rim's API tables: orchestrator).
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct MirrorArea;

impl Area for MirrorArea {}

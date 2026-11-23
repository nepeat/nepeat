//! The `MACsec` hook: the `MACSEC`, `MACSEC_PORT`, `MACSEC_FLOW`, `MACSEC_SC`, `MACSEC_SA` objects
//! and the `MACsec`-stage ACL tables go to the `sai-macsec` model (called natively, with a
//! `SeqRunner` over [`crate::backend::Mdio`]); the port's IFG follows `MACsec` state
//! ([`crate::backend::PortOps::port_ifg_set`]); stats capability for SA and flow. Ports the glue
//! of `sai_macsec.cpp` / `sai_macsec_rust.cpp` (`macsec_acl_*`, `port_macsec_set` / `get`,
//! `macsec_port_ifg`, `macsec_stats_capability`, `ext_macsec_load` / `attach` / `detach`).
//!
//! Registration: the `MACsec` object types join [`crate::dispatch::OWNERS`] (a new
//! `Owner::Macsec`) once they are [`crate::oid::ObjectType`]s and `sai-core` may depend on
//! `sai-macsec` (today `sai-macsec` depends on `sai-core`: the dependency direction is the
//! orchestrator's decision).

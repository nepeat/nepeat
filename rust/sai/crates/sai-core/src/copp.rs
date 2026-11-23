//! Control-plane policing: the one field group (priorities 100 / 150 / 200, the `DstCpu`
//! qualifier for IP2ME) that carries the traps `trap_set` cannot police, refreshed after every
//! change of a trap, trap group or their policer (`copp_refresh`). Ports the `CoPP` part of
//! `sai_acl.cpp`.
//!
//! Backend: [`crate::backend::Field`], [`crate::backend::Policer`],
//! [`crate::backend::Hostif::trap_set`] / `trap_program`.
//!
//! Registration: none of its own. It owns no object type; [`crate::hostif`] and [`crate::acl`]
//! call its refresh.

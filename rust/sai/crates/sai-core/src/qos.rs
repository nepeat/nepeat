//! `QoS`: maps, schedulers and scheduler groups (one per queue, for qosorch), WRED, buffer pools
//! and profiles, queue / priority group attributes and counters (`_ext READ_AND_CLEAR` = get then
//! clear). With [`crate::backend::Qos::qos_supported`] false (the hardware backends, and the
//! virtual chip unless `qos_on`) everything is kept and nothing programmed. Ports `sai_qos.cpp`
//! (1359 lines).
//!
//! Backend: [`crate::backend::Qos`], [`crate::backend::PortOps::port_pfc_set`] /
//! `port_pfc_stat_get`.
//!
//! Registration: [`crate::dispatch::Owner::Qos`] for `QUEUE`, `INGRESS_PRIORITY_GROUP`,
//! `SCHEDULER_GROUP`, `SCHEDULER`, `BUFFER_POOL`, `BUFFER_PROFILE`, `QOS_MAP`, `WRED` in
//! [`crate::dispatch::OWNERS`]; the port's `QoS` attributes come from [`crate::port`].
use crate::dispatch::Area;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct QosArea;

impl Area for QosArea {}

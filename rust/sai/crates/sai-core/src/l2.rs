//! L2: bridge ports on LAGs, FDB entries programmed into the L2 table, FDB flush, learn / age
//! events, STP (default instance only). Ports `sai_l2.cpp` (809 lines; VLAN membership, PVID
//! and learning applied to every LAG member; `vlan_member_create_internal(program=false)` for
//! the default VLAN) and the FDB part of `sai_switch.cpp` (`deliver_fdb`: a learn of a known
//! address on another bridge port is a SAI `MOVE`).
//!
//! Backend: [`crate::backend::L2`], the VLAN operations of [`crate::backend::Backend`],
//! [`crate::backend::PortOps::port_pvid_set`] / `port_learn_set`,
//! [`crate::backend::Storm::vlan_flood_set`]; events [`crate::backend::BackendEvent::Fdb`].
//!
//! Registration: [`crate::dispatch::Owner::L2`] for `STP` and the `FDB_ENTRY` entry type
//! (with `flush_fdb_entries`) in [`crate::dispatch::OWNERS`] / `ENTRY_OWNERS`; FDB events
//! arrive in [`fdb_event`]. VLAN, VLAN member, bridge and bridge port are typed in `sai.rs`
//! today ([`crate::dispatch::Owner::Core`]).
use crate::backend::types::FdbEvent;
use crate::dispatch::Area;
use crate::sai::Notification;
use crate::store::Store;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct L2Area;

impl Area for L2Area {}

/// An address the chip learned or aged: what the SAI user is to be told. Not ported yet: the
/// event is dropped, as before events existed (the L2 area adds the store update and an
/// `FDB_EVENT` notification, which needs a [`Notification`] variant and the rim's delivery).
pub(crate) fn fdb_event(store: &mut Store, event: FdbEvent) -> Vec<Notification> {
    let _ = (store, event);
    Vec::new()
}

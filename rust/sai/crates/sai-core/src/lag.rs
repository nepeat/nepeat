//! LAGs: trunks, members' egress / ingress disable, and the replay of bridge port, VLAN and RIF
//! state on a member port when it joins or leaves (`bridge_port_member_join` / `leave`,
//! `rif_member_join` / `leave`; LACP runs in the host, teamd). Ports `sai_lag.cpp` (375 lines).
//!
//! Backend: [`crate::backend::Lag`] (and, for the replay, [`crate::backend::L2`],
//! [`crate::backend::PortOps::port_pvid_set`] and the VLAN operations).
//!
//! Registration: [`crate::dispatch::Owner::Lag`] for `LAG` and `LAG_MEMBER` in
//! [`crate::dispatch::OWNERS`]. Today the objects are still attribute bags (no trunk is
//! programmed); this module holds the C++ library's member rules and `PORT_LIST`, moved out of
//! `bag.rs` unchanged as the proof of the dispatch table.
use crate::backend::Backend;
use crate::bag;
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::model::{BridgePort, Port, PortKind};
use crate::oid::{ObjectType, PortId, RawOid, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct LagArea;

impl Area for LagArea {
    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        bag::create_with(cx.backend, cx.store, object_type, attrs, member_rules)
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = backend;
        // Read-only, so never stored: computed before the bag's own answer.
        if object_type == ObjectType::Lag && meta.id == sys::SAI_LAG_ATTR_PORT_LIST {
            store.bag(id)?;
            return Ok(port_list(store, id));
        }
        bag::get(store, id, meta)
    }
}

/// The ports of a LAG's members.
fn port_list(store: &Store, lag: RawOid) -> Value {
    Value::OidList(
        bag::naming(store, ObjectType::LagMember, sys::SAI_LAG_MEMBER_ATTR_LAG_ID, lag)
            .into_iter()
            .filter_map(
                |member| match store.bag(member).ok()?.attrs.get(&sys::SAI_LAG_MEMBER_ATTR_PORT_ID) {
                    Some(Value::Oid(port)) => Some(*port),
                    _ => None,
                },
            )
            .collect(),
    )
}

/// The C++ library's rules (`sai_lag.cpp`): a member is a front-panel port, in one LAG at most,
/// that has no bridge port or router interface of its own (orchagent removes the default bridge
/// ports first).
fn member_rules(store: &Store, object_type: ObjectType, attrs: &[Attr]) -> Result<()> {
    if object_type != ObjectType::LagMember {
        return Ok(());
    }
    let index = attrs
        .iter()
        .position(|a| a.id == sys::SAI_LAG_MEMBER_ATTR_PORT_ID)
        .ok_or(Error::MandatoryAttributeMissing)?;
    let Some(Value::Oid(port)) = attrs.get(index).map(|a| &a.value) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    let front = PortId::try_from(*port)
        .ok()
        .and_then(|id| store.get::<Port>(id).ok())
        .is_some_and(|p| matches!(p.kind, PortKind::Front { .. }));
    if !front {
        return Err(Error::attr(AttrError::InvalidValue, index));
    }
    if !bag::naming(store, ObjectType::LagMember, sys::SAI_LAG_MEMBER_ATTR_PORT_ID, *port).is_empty() {
        return Err(Error::ItemAlreadyExists);
    }
    let bridged = store.iter::<BridgePort>().any(|(_, b)| b.port.raw() == *port);
    let routed = !bag::naming(
        store,
        ObjectType::RouterInterface,
        sys::SAI_ROUTER_INTERFACE_ATTR_PORT_ID,
        *port,
    )
    .is_empty();
    if bridged || routed {
        return Err(Error::ObjectInUse);
    }
    Ok(())
}

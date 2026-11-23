//! The test port's view of the library (cargo feature `testing`, off by default): the questions
//! of the C++ suite's `tests/test_port.h` about the core's own bookkeeping, answered with typed
//! values, and access to the virtual backend under the switch. The crate `sai-test-port` parses
//! the requests of `sai_rs_test_query` and encodes these answers (protocol: its
//! `include/sai_rs_test.h`); the meaning of every answer is the one `test_port.h` gives.
//!
//! A fact that lives in an area module comes from that module's `testing_*` function. Until the
//! area is ported far enough it answers [`NotPorted`], and the test port reports the question as
//! unsupported instead of guessing.
//!
//! Read-mostly: nothing here changes the object store; the virtual backend's hooks change only
//! the chip model, as the C++ test port's do.
use crate::backend::types::{
    EgressId, FpEntryId, FpGroupId, GenlId, IntfId, MirrorId, NetifId, PolicerId, QosMapId, SampleId, TrunkId, Vrf,
    WredId,
};
use crate::backend::{Backend, HwPort, Vid};
use crate::hostif::{HostInterface, HostifChip, Programming, Trap};
use crate::l3::{RouterInterface, VirtualRouter};
use crate::mirror::{MirrorSession, Samplepacket};
use crate::model::Port;
use crate::oid::{LagId, ObjectType, PortId, RawOid, RouterInterfaceId};
use crate::qos::{QosMap, Wred};
use crate::sai::Sai;
use crate::store::Store;
use crate::virt::VirtualBackend;
use sai_sys as sys;
use std::net::IpAddr;

/// An area module cannot answer this question yet: its port has not reached the fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotPorted {
    /// The module and its owner, e.g. `"lag.rs (A2)"`.
    pub area: &'static str,
}

/// An area module's answer to a test-port question.
pub type AreaAnswer<T> = Result<T, NotPorted>;

/// The backend object a SAI object maps to (`test_port.h` `backend_id`), by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendId {
    /// `PORT`: the chip port.
    Port(HwPort),
    /// `LAG`: the trunk.
    Trunk(TrunkId),
    /// `VIRTUAL_ROUTER`: the VRF (0 = the default virtual router).
    Vrf(Vrf),
    /// `ROUTER_INTERFACE`: the L3 interface.
    Intf(IntfId),
    /// `POLICER`: the meter.
    Policer(PolicerId),
    /// `ACL_TABLE`: the field group.
    FpGroup(FpGroupId),
    /// `ACL_ENTRY`: the field entry (none while it is not installed).
    FpEntry(FpEntryId),
    /// `QOS_MAP`
    QosMap(QosMapId),
    /// `WRED`
    Wred(WredId),
    /// `MIRROR_SESSION`
    Mirror(MirrorId),
    /// `SAMPLEPACKET`
    Sample(SampleId),
    /// `HOSTIF` of type NETDEV: the KNET netif.
    Netif(NetifId),
    /// `HOSTIF` of type GENETLINK: the generic-netlink interface.
    Genl(GenlId),
}

impl BackendId {
    /// The backend's number (what the C++ test port answers).
    pub fn number(self) -> i64 {
        i64::from(match self {
            Self::Port(hw) => u32::from(hw.0),
            Self::Trunk(id) => id.get(),
            Self::Vrf(id) => id.get(),
            Self::Intf(id) => id.get(),
            Self::Policer(id) => id.get(),
            Self::FpGroup(id) => id.get(),
            Self::FpEntry(id) => id.get(),
            Self::QosMap(id) => id.get(),
            Self::Wred(id) => id.get(),
            Self::Mirror(id) => id.get(),
            Self::Sample(id) => id.get(),
            Self::Netif(id) => id.get(),
            Self::Genl(id) => id.get(),
        })
    }
}

/// A router interface as the library keeps it (`test_port.h` `Rif`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RifFacts {
    /// Its L3 interface.
    pub intf: Option<IntfId>,
    /// Its VRF.
    pub vrf: Option<Vrf>,
    /// The VLAN its L3 interface sits on (a PORT / LAG RIF: the internal VLAN).
    pub vid: Option<Vid>,
}

/// The object id layout (`ob::make_oid` and friends, `sai_internal.h`): bits 63..56 object type,
/// 55..48 switch index, 47..0 index. Pure; the same as [`crate::oid`] uses (checked by its tests).
pub mod oid_layout {
    use crate::oid::RawOid;
    use sai_sys as sys;

    const TYPE_SHIFT: u32 = 56;
    const SWITCH_SHIFT: u32 = 48;
    /// The largest object index (`OID_INDEX_MASK`, 2^48 - 1).
    pub const INDEX_MASK: u64 = (1 << SWITCH_SHIFT) - 1;

    /// `ob::make_oid`: type and switch index cut to 8 bits, index to 48.
    pub fn make_oid(object_type: sys::sai_object_type_t, index: u64, switch: u32) -> RawOid {
        RawOid(
            (u64::from(object_type & 0xff) << TYPE_SHIFT)
                | (u64::from(switch & 0xff) << SWITCH_SHIFT)
                | (index & INDEX_MASK),
        )
    }

    /// `ob::oid_type`: the type bits; `NULL` for the null id and for numbers no type has.
    pub fn oid_type(oid: RawOid) -> sys::sai_object_type_t {
        let t = oid.type_bits();
        if oid.is_null() || t == sys::SAI_OBJECT_TYPE_NULL || t >= sys::SAI_OBJECT_TYPE_MAX {
            sys::SAI_OBJECT_TYPE_NULL
        } else {
            t
        }
    }

    /// `ob::oid_index`
    pub const fn oid_index(oid: RawOid) -> u64 {
        oid.0 & INDEX_MASK
    }

    /// `ob::oid_switch`
    pub const fn oid_switch(oid: RawOid) -> u32 {
        // 8 bits: the cast cannot cut anything.
        #[allow(clippy::cast_possible_truncation, reason = "masked to 8 bits")]
        let index = ((oid.0 >> SWITCH_SHIFT) & 0xff) as u32;
        index
    }
}

/// A switch object exists (created, not removed).
pub fn switch_created<B: Backend>(sai: &Sai<B>) -> bool {
    sai.switch_id().is_some()
}

/// An object of the store by its raw id, if the id is of type `K` and the object exists.
fn object<K: crate::model::Kind>(store: &Store, oid: RawOid) -> Option<&K> {
    K::Id::try_from(oid).ok().and_then(|id| store.get::<K>(id).ok())
}

/// The backend object a SAI object maps to; `None` when it has none (also: no switch, an unknown
/// id, a type without a backend object).
pub fn backend_id<B: Backend>(sai: &Sai<B>, oid: RawOid) -> AreaAnswer<Option<BackendId>> {
    let Some(store) = sai.store() else { return Ok(None) };
    Ok(match oid.type_bits() {
        sys::SAI_OBJECT_TYPE_PORT => object::<Port>(store, oid).map(|p| BackendId::Port(p.hw)),
        sys::SAI_OBJECT_TYPE_LAG => return crate::lag::testing_backend_id(sai, oid),
        sys::SAI_OBJECT_TYPE_POLICER | sys::SAI_OBJECT_TYPE_ACL_TABLE | sys::SAI_OBJECT_TYPE_ACL_ENTRY => {
            return crate::acl::testing_backend_id(sai, oid);
        }
        sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER => object::<VirtualRouter>(store, oid).map(|v| BackendId::Vrf(v.vrf)),
        sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE => object::<RouterInterface>(store, oid)
            .and_then(|r| r.intf)
            .map(BackendId::Intf),
        sys::SAI_OBJECT_TYPE_QOS_MAP => object::<QosMap>(store, oid).and_then(QosMap::hw).map(BackendId::QosMap),
        sys::SAI_OBJECT_TYPE_WRED => object::<Wred>(store, oid).and_then(Wred::hw).map(BackendId::Wred),
        sys::SAI_OBJECT_TYPE_MIRROR_SESSION => object::<MirrorSession>(store, oid)
            .and_then(MirrorSession::hw)
            .map(BackendId::Mirror),
        sys::SAI_OBJECT_TYPE_SAMPLEPACKET => object::<Samplepacket>(store, oid)
            .and_then(Samplepacket::hw)
            .map(BackendId::Sample),
        sys::SAI_OBJECT_TYPE_HOSTIF => object::<HostInterface>(store, oid).map(|h| match h.chip {
            HostifChip::Netdev { netif, .. } => BackendId::Netif(netif),
            HostifChip::Genetlink { genl } => BackendId::Genl(genl),
        }),
        _ => None,
    })
}

/// A router interface; `None` when there is no such RIF.
pub fn rif<B: Backend>(sai: &Sai<B>, oid: RawOid) -> Option<RifFacts> {
    let r = object::<RouterInterface>(sai.store()?, oid)?;
    Some(RifFacts {
        intf: r.intf,
        vrf: Some(r.vrf),
        vid: r.vid(),
    })
}

/// The LAG a port is a member of.
pub fn port_lag<B: Backend>(sai: &Sai<B>, port: RawOid) -> AreaAnswer<Option<LagId>> {
    let Some(store) = sai.store() else { return Ok(None) };
    match PortId::try_from(port) {
        Ok(id) if store.get::<Port>(id).is_ok() => crate::lag::testing_port_lag(sai, id),
        _ => Ok(None),
    }
}

/// The trap is programmed in the backend (`trap_set` or `trap_program`), not only kept.
pub fn trap_programmed<B: Backend>(sai: &Sai<B>, trap: RawOid) -> bool {
    sai.store()
        .and_then(|store| object::<Trap>(store, trap))
        .is_some_and(|t| t.programming != Programming::Kept)
}

/// The object types [`object_count`] answers for (the C++ test port's).
pub const COUNTED_TYPES: [ObjectType; 3] = [ObjectType::VirtualRouter, ObjectType::AclTable, ObjectType::AclEntry];

/// Live SAI objects of a type the library holds; `None` for a type not in [`COUNTED_TYPES`].
pub fn object_count<B: Backend>(sai: &Sai<B>, object_type: ObjectType) -> Option<AreaAnswer<usize>> {
    if !COUNTED_TYPES.contains(&object_type) {
        return None;
    }
    let Some(store) = sai.store() else { return Some(Ok(0)) };
    Some(match object_type {
        ObjectType::AclTable | ObjectType::AclEntry => crate::acl::testing_object_count(sai, object_type),
        _ => Ok(store.count_of(object_type)),
    })
}

/// PORT / LAG objects with an ingress ACL (table or table group) bound.
pub fn acl_binding_count<B: Backend>(sai: &Sai<B>) -> AreaAnswer<usize> {
    if sai.store().is_none() {
        return Ok(0);
    }
    crate::acl::testing_acl_binding_count(sai)
}

/// The field group the library keeps its `CoPP` rules in; `None` while there is none.
pub fn copp_group<B: Backend>(sai: &Sai<B>) -> Option<FpGroupId> {
    crate::copp::copp_group(sai.store()?)
}

/// The `CoPP` rules the library has installed.
pub fn copp_rule_count<B: Backend>(sai: &Sai<B>) -> usize {
    sai.store().map_or(0, crate::copp::copp_rule_count)
}

/// The egress object of the neighbor (rif, ip); `None` when there is no such neighbor.
pub fn neighbor_egress<B: Backend>(sai: &Sai<B>, rif: RawOid, ip: IpAddr) -> Option<EgressId> {
    let rif = RouterInterfaceId::try_from(rif).ok()?;
    crate::l3::neighbor_egress(sai.store()?, rif, ip)
}

/// An external-PHY table is attached to the live switch.
pub fn ext_phy_active<B: Backend>(sai: &Sai<B>) -> AreaAnswer<bool> {
    crate::extphy::testing_ext_phy_active(sai)
}

/// The external-PHY runtime's status text (the diag `extphy` answer).
pub fn ext_phy_status<B: Backend>(sai: &Sai<B>) -> String {
    crate::extphy::ext_phy_status(sai)
}

/// An external-`MACsec` table is attached to the live switch.
pub fn ext_macsec_active<B: Backend>(sai: &Sai<B>) -> AreaAnswer<bool> {
    crate::macsec::testing_ext_macsec_active(sai)
}

/// The external-`MACsec` runtime's status text (the diag `extmacsec` answer).
pub fn ext_macsec_status<B: Backend>(sai: &Sai<B>) -> String {
    crate::macsec::ext_macsec_status(sai)
}

/// Library log lines to stderr on or off (`SAI_OPENBCM_LOG_STDERR=1`).
pub fn log_to_stderr(on: bool) {
    crate::log::configure_stderr(on);
}

/// `SAI_PORT_STAT_PFC_<n>_{RX,TX}_PKTS` as (priority, tx), by the library's own mapping.
pub fn port_pfc_stat(id: sys::sai_stat_id_t) -> Option<(u8, bool)> {
    crate::qos::port_pfc_stat(id)
}

/// The virtual backend under the live switch; `None` without a switch or with another backend.
pub fn virtual_backend<B: Backend>(sai: &Sai<B>) -> Option<&VirtualBackend> {
    sai.switch_id()?;
    sai.backend().as_virtual()
}

/// The same, to change (the test hooks).
pub fn virtual_backend_mut<B: Backend>(sai: &mut Sai<B>) -> Option<&mut VirtualBackend> {
    sai.switch_id()?;
    sai.backend_mut().as_virtual_mut()
}

#[cfg(test)]
mod tests {
    use super::oid_layout::*;
    use crate::oid::{ObjectType, PortId, RawOid, TypedId};
    use sai_sys as sys;

    #[test]
    fn the_layout_is_the_one_the_ids_use() {
        let port = PortId::from_index(9).raw();
        assert_eq!(make_oid(sys::SAI_OBJECT_TYPE_PORT, 9, 0), port);
        assert_eq!(oid_type(port), sys::SAI_OBJECT_TYPE_PORT);
        assert_eq!(oid_index(port), 9);
        assert_eq!(oid_switch(make_oid(sys::SAI_OBJECT_TYPE_VLAN, 1, 3)), 3);
        assert_eq!(INDEX_MASK, (1 << 48) - 1);
        assert_eq!(oid_index(make_oid(sys::SAI_OBJECT_TYPE_LAG, u64::MAX, 0)), INDEX_MASK);
        // the switch of an id, as sai_switch_id_query decodes it
        assert_eq!(
            RawOid(make_oid(sys::SAI_OBJECT_TYPE_VLAN, 5, 0).0).switch_id(),
            make_oid(sys::SAI_OBJECT_TYPE_SWITCH, 1, 0)
        );
        assert_eq!(oid_type(RawOid::NULL), sys::SAI_OBJECT_TYPE_NULL);
        assert_eq!(
            oid_type(RawOid(u64::from(sys::SAI_OBJECT_TYPE_MAX) << 56)),
            sys::SAI_OBJECT_TYPE_NULL
        );
        assert_eq!(ObjectType::Port.sai(), sys::SAI_OBJECT_TYPE_PORT);
    }
}

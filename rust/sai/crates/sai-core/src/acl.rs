//! ACL: a table is one SDK field group (qualifier set = the table's match fields + in-ports),
//! an entry one SDK field entry. An entry applies to the ports its table is bound to (PORT /
//! LAG `INGRESS_ACL` = a table group with the table as a member, or the table itself; LAG:
//! every member port); an unbound table's entries are not installed. Tables in one group are
//! looked up in parallel (SDK groups always are). Entries are re-created when they change
//! (binding, LAG membership, attributes), counters carried over; with
//! [`FpCap::EntryUpdate`] updated in place, with [`FpCap::EntryEnable`] switched off instead of
//! removed. Tables with a `MACsec` stage go to the `MACsec` model ([`crate::macsec`]).
//! Ports `sai_acl.cpp` (1598 lines) without its policers ([`crate::policer`]) and `CoPP`
//! ([`crate::copp`]).
//!
//! Backend: [`crate::backend::Field`] (and [`crate::backend::Base::chip_limits`] for entry
//! priorities, [`crate::backend::Mirror::mirror_supported`] for mirror actions).
//!
//! Registration: [`crate::dispatch::Owner::Acl`] for `POLICER`, `ACL_TABLE`, `ACL_ENTRY`,
//! `ACL_COUNTER`, `ACL_RANGE`, `ACL_TABLE_GROUP`, `ACL_TABLE_GROUP_MEMBER` in
//! [`crate::dispatch::OWNERS`]; the objects are [`AclObject`]s in the store
//! ([`crate::model::Object::Acl`]); the bindings and the `CoPP` state are the switch's
//! [`AclState`]. Other areas call [`acl_bind_set`] / [`acl_bind_get`] (PORT and LAG
//! `INGRESS_ACL` / `EGRESS_ACL`), [`acl_reprogram`] (LAG membership) and the capability
//! helpers [`acl_ingress_actions`] / [`acl_egress_actions`] / [`acl_table_field_supported`].
//!
//! Where this differs from the C++, each also at the function: a failed get writes nothing. (A
//! create-only attribute in a set gets the C++ `INVALID_ATTR_VALUE_0`, [`Area::refused_set`]; a
//! counter get reads the SDK counter once per call, as the C++: [`Area::get_all`].)
use crate::backend::types::{
    FpCap, FpEntryId, FpGroupExtra, FpGroupId, FpMatch, FpQual, FpRule, MacAddr, Masked, Redirect,
};
use crate::backend::{Backend, HwPort};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::log;
use crate::model::{Kind, Object, Port, PortKind, Switch};
use crate::oid::{
    AclCounterId, AclEntryId, AclRangeId, AclTableGroupId, AclTableGroupMemberId, AclTableId, ObjectType, PortId,
    RawOid, SwitchId, TypedId,
};
use crate::policer::Policer;
use crate::store::Store;
use crate::value::{AclAction, AclField, AclFieldData, AclParam, Attr, FieldMatch, Value};
use sai_meta::{AttrMeta, ValueType};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) use crate::policer::{policer_backend_id, policer_valid, trap_policer};

const fn e(enumerator: u32) -> i32 {
    enumerator.cast_signed()
}

fn not_supported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

fn invalid(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

/// C++ `attr_backend_status`: a backend's `INVALID_PARAMETER` / `NOT_SUPPORTED` /
/// `NOT_IMPLEMENTED` as the status of the attribute at `index`.
pub(crate) fn attr_backend_status(err: Error, index: usize) -> Error {
    match err {
        Error::InvalidParameter => invalid(index),
        Error::NotSupported => not_supported(index),
        Error::NotImplemented => Error::attr(AttrError::NotImplemented, index),
        other => other,
    }
}

// ------------------------------------------------------------------ the model

/// The stage of a table, a group or a binding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// `INGRESS`
    #[default]
    Ingress,
    /// `EGRESS` (only with [`FpCap::Egress`])
    Egress,
}

impl Stage {
    /// The stage of a binding attribute (`INGRESS_ACL`: true, `EGRESS_ACL`: false).
    pub const fn of_binding(ingress: bool) -> Self {
        if ingress { Self::Ingress } else { Self::Egress }
    }

    /// The `sai_acl_stage_t` enumerator.
    pub const fn sai(self) -> i32 {
        match self {
            Self::Ingress => e(sys::SAI_ACL_STAGE_INGRESS),
            Self::Egress => e(sys::SAI_ACL_STAGE_EGRESS),
        }
    }
}

/// What an `ACL_RANGE` ranges over (C++ accepts the L4 port ranges).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeKind {
    /// `L4_SRC_PORT_RANGE`
    L4Src,
    /// `L4_DST_PORT_RANGE`
    L4Dst,
}

impl RangeKind {
    const fn sai(self) -> i32 {
        match self {
            Self::L4Src => e(sys::SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE),
            Self::L4Dst => e(sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE),
        }
    }
}

/// The switch's ACL state outside the objects: the bindings (C++ `acl_bind`, `acl_bind_egress`:
/// PORT / LAG -> table or table group), the next table's SDK group priority and the `CoPP` group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclState {
    bindings: BTreeMap<Stage, BTreeMap<RawOid, RawOid>>,
    next_priority: i32,
    /// The `CoPP` field group and rules ([`crate::copp`]).
    pub copp: crate::copp::CoppState,
}

impl Default for AclState {
    fn default() -> Self {
        Self {
            bindings: BTreeMap::new(),
            next_priority: 1,
            copp: crate::copp::CoppState::default(),
        }
    }
}

impl AclState {
    /// What the store keeps alive for the switch: the bound tables and groups (C++
    /// `acl_referenced`). The bound PORT or LAG is not held: a LAG may go while bound (its
    /// binding goes with it, [`acl_bind_set`] from the LAG area).
    pub fn references(&self) -> Vec<RawOid> {
        self.bindings.values().flat_map(|b| b.values().copied()).collect()
    }

    /// What `obj` is bound to in `stage`.
    pub fn bound(&self, obj: RawOid, stage: Stage) -> Option<RawOid> {
        self.bindings.get(&stage).and_then(|b| b.get(&obj)).copied()
    }

    fn binds(&self, stage: Stage) -> impl Iterator<Item = (RawOid, RawOid)> + '_ {
        self.bindings.get(&stage).into_iter().flatten().map(|(k, v)| (*k, *v))
    }
}

/// An ACL table on the ASIC (C++ `AclTableObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclTable {
    /// `ACL_STAGE`
    pub stage: Stage,
    /// `ACL_BIND_POINT_TYPE_LIST` (PORT / LAG)
    pub bind_types: Vec<i32>,
    /// `ACL_ACTION_TYPE_LIST`
    pub action_types: Vec<i32>,
    /// The `FIELD_*` attributes set to true.
    pub fields: BTreeSet<sys::sai_attr_id_t>,
    /// `FIELD_ACL_RANGE_TYPE`: kept (`AclOrch` asks for L4 port ranges on every table).
    pub range_types: Vec<i32>,
    /// `SIZE`
    pub size: u32,
    /// The SDK field group.
    pub gid: FpGroupId,
}

/// A table with a `MACsec` stage: its state is the `MACsec` model's ([`crate::macsec`]); the
/// store only gives it its id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacsecAclTable;

/// The SDK entry an ACL entry is installed as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The field entry.
    pub eid: FpEntryId,
    /// What it was installed with (C++ `key`).
    pub rule: FpRule,
    /// [`FpCap::EntryEnable`]: the installed entry is on (C++ `hw_enabled`).
    pub enabled: bool,
}

/// An ACL entry (C++ `AclEntryObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclEntry {
    /// `TABLE_ID`
    pub table: AclTableId,
    /// `PRIORITY`
    pub priority: u32,
    /// `ADMIN_STATE` (default true)
    pub admin: bool,
    /// The enabled match fields.
    pub fields: BTreeMap<sys::sai_attr_id_t, AclFieldData>,
    /// The enabled actions.
    pub actions: BTreeMap<sys::sai_attr_id_t, AclParam>,
    /// The SDK entry, while installed (boxed: the rule is large).
    pub installed: Option<Box<Installed>>,
}

impl AclEntry {
    /// `ACTION_COUNTER` (C++ `entry_counter`; null when absent).
    pub fn counter(&self) -> RawOid {
        match self.actions.get(&sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER) {
            Some(AclParam::Oid(k)) => *k,
            _ => RawOid::NULL,
        }
    }

    fn objects_of(&self, id: sys::sai_attr_id_t) -> &[RawOid] {
        match (self.fields.get(&id), self.actions.get(&id)) {
            (Some(AclFieldData::OidList(list)), _) | (_, Some(AclParam::OidList(list))) => list,
            _ => &[],
        }
    }
}

/// An entry of a `MACsec` table ([`crate::macsec`] keeps it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacsecAclEntry {
    /// Its table.
    pub table: AclTableId,
}

/// An ACL counter (C++ `AclCounterObj`); the entry counting into it is the one whose
/// `ACTION_COUNTER` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AclCounter {
    /// `TABLE_ID`
    pub table: AclTableId,
    /// `ENABLE_PACKET_COUNT`
    pub packets: bool,
    /// `ENABLE_BYTE_COUNT`
    pub bytes: bool,
    /// Packets counted before the current SDK entry (carried over re-creation; `PACKETS` set).
    pub base_packets: u64,
    /// The same for bytes.
    pub base_bytes: u64,
}

/// An L4 port range (only with [`FpCap::Ranges`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AclRange {
    /// `TYPE`
    pub kind: RangeKind,
    /// `LIMIT` minimum
    pub min: u32,
    /// `LIMIT` maximum (at most 65535)
    pub max: u32,
}

/// A table group (C++ `AclGroupObj`); its type is always `PARALLEL`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclGroup {
    /// `ACL_STAGE`
    pub stage: Stage,
    /// `ACL_BIND_POINT_TYPE_LIST`: kept, not checked (portsorch passes an object list here).
    pub bind_types: Vec<i32>,
}

/// A table's membership of a group (C++ `AclGroupMemberObj`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AclGroupMember {
    /// `ACL_TABLE_GROUP_ID`
    pub group: AclTableGroupId,
    /// `ACL_TABLE_ID`
    pub table: AclTableId,
    /// `PRIORITY` (kept)
    pub priority: u32,
}

/// The objects of this area as the store holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AclObject {
    /// `POLICER`
    Policer(Policer),
    /// `ACL_TABLE` on the ASIC
    Table(AclTable),
    /// `ACL_TABLE` with a `MACsec` stage
    MacsecTable(MacsecAclTable),
    /// `ACL_ENTRY` on the ASIC
    Entry(AclEntry),
    /// `ACL_ENTRY` of a `MACsec` table
    MacsecEntry(MacsecAclEntry),
    /// `ACL_COUNTER`
    Counter(AclCounter),
    /// `ACL_RANGE`
    Range(AclRange),
    /// `ACL_TABLE_GROUP`
    Group(AclGroup),
    /// `ACL_TABLE_GROUP_MEMBER`
    GroupMember(AclGroupMember),
}

impl AclObject {
    /// The object's type.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::Policer(_) => ObjectType::Policer,
            Self::Table(_) | Self::MacsecTable(_) => ObjectType::AclTable,
            Self::Entry(_) | Self::MacsecEntry(_) => ObjectType::AclEntry,
            Self::Counter(_) => ObjectType::AclCounter,
            Self::Range(_) => ObjectType::AclRange,
            Self::Group(_) => ObjectType::AclTableGroup,
            Self::GroupMember(_) => ObjectType::AclTableGroupMember,
        }
    }

    /// What the store keeps alive for it, as the C++ in-use checks: an entry its table, counter,
    /// policer, ranges and mirror sessions (not the ports, LAGs or next hops it names: the C++
    /// lets those go); a counter its table; a member its group and table.
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::Entry(entry) => {
                let mut refs = vec![entry.table.raw()];
                let counter = entry.counter();
                if !counter.is_null() {
                    refs.push(counter);
                }
                if let Some(AclParam::Oid(p)) = entry.actions.get(&sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER) {
                    if !p.is_null() {
                        refs.push(*p);
                    }
                }
                for id in [
                    sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE,
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS,
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS,
                ] {
                    refs.extend(entry.objects_of(id).iter().filter(|o| !o.is_null()));
                }
                refs
            }
            Self::MacsecEntry(entry) => vec![entry.table.raw()],
            Self::Counter(counter) => vec![counter.table.raw()],
            Self::GroupMember(member) => vec![member.group.raw(), member.table.raw()],
            Self::Policer(_) | Self::Table(_) | Self::MacsecTable(_) | Self::Range(_) | Self::Group(_) => Vec::new(),
        }
    }
}

macro_rules! acl_kinds {
    ($($variant:ident($ty:ty): $id:ty;)*) => {$(
        impl Kind for $ty {
            type Id = $id;
            fn wrap(self) -> Object {
                Object::Acl(AclObject::$variant(self))
            }
            fn peel(object: &Object) -> Option<&Self> {
                if let Object::Acl(AclObject::$variant(o)) = object { Some(o) } else { None }
            }
            fn peel_mut(object: &mut Object) -> Option<&mut Self> {
                if let Object::Acl(AclObject::$variant(o)) = object { Some(o) } else { None }
            }
            fn unwrap(object: Object) -> Option<Self> {
                if let Object::Acl(AclObject::$variant(o)) = object { Some(o) } else { None }
            }
        }
    )*};
}

acl_kinds! {
    Policer(Policer): crate::oid::PolicerId;
    Table(AclTable): AclTableId;
    MacsecTable(MacsecAclTable): AclTableId;
    Entry(AclEntry): AclEntryId;
    MacsecEntry(MacsecAclEntry): AclEntryId;
    Counter(AclCounter): AclCounterId;
    Range(AclRange): AclRangeId;
    Group(AclGroup): AclTableGroupId;
    GroupMember(AclGroupMember): AclTableGroupMemberId;
}

/// The switch and its object (the ACL state lives there).
pub(crate) fn switch_of(store: &Store) -> Option<(SwitchId, &Switch)> {
    store.iter::<Switch>().next()
}

fn acl_state(store: &Store) -> Option<&AclState> {
    switch_of(store).map(|(_, s)| &s.acl)
}

fn update_state<T>(store: &mut Store, change: impl FnOnce(&mut AclState) -> T) -> Result<T> {
    let (id, _) = switch_of(store).ok_or(Error::Uninitialized)?;
    store.update::<Switch, _>(id, |s| change(&mut s.acl))
}

fn get_typed<K: Kind>(store: &Store, raw: RawOid) -> Option<&K> {
    store.get::<K>(K::Id::try_from(raw).ok()?).ok()
}

// ------------------------------------------------------------------ values

fn enum_of(value: &Value) -> Result<i32> {
    match value {
        Value::Enum(v) | Value::I32(v) => Ok(*v),
        _ => Err(Error::Failure),
    }
}

fn i32_list(value: &Value) -> Result<Vec<i32>> {
    match value {
        Value::I32List(v) => Ok(v.clone()),
        _ => Err(Error::Failure),
    }
}

fn u32_of(value: &Value) -> Result<u32> {
    match value {
        Value::U32(v) => Ok(*v),
        _ => Err(Error::Failure),
    }
}

fn bool_of(value: &Value) -> Result<bool> {
    match value {
        Value::Bool(v) => Ok(*v),
        _ => Err(Error::Failure),
    }
}

fn oid_of(value: &Value) -> Result<RawOid> {
    match value {
        Value::Oid(v) => Ok(*v),
        _ => Err(Error::Failure),
    }
}

// ------------------------------------------------------------------ capabilities

/// The qualifier of a table match field this library programs (C++ `field_qual`;
/// `crate::caps` advertises the same).
fn field_qual(field: sys::sai_attr_id_t) -> Option<FpQual> {
    Some(match field {
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IP => FpQual::SrcIp,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP => FpQual::DstIp,
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_IPV6 => FpQual::SrcIp6,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IPV6 => FpQual::DstIp6,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_SRC_PORT => FpQual::L4SrcPort,
        sys::SAI_ACL_TABLE_ATTR_FIELD_L4_DST_PORT => FpQual::L4DstPort,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IP_PROTOCOL | sys::SAI_ACL_TABLE_ATTR_FIELD_IPV6_NEXT_HEADER => {
            FpQual::IpProtocol
        }
        sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE => FpQual::EtherType,
        sys::SAI_ACL_TABLE_ATTR_FIELD_TCP_FLAGS => FpQual::TcpFlags,
        sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_TYPE
        | sys::SAI_ACL_TABLE_ATTR_FIELD_ICMP_CODE
        | sys::SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_TYPE
        | sys::SAI_ACL_TABLE_ATTR_FIELD_ICMPV6_CODE => FpQual::IcmpTypeCode,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DSCP | sys::SAI_ACL_TABLE_ATTR_FIELD_ECN | sys::SAI_ACL_TABLE_ATTR_FIELD_TOS => {
            FpQual::Tos
        }
        sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_IP_TYPE => FpQual::IpType,
        sys::SAI_ACL_TABLE_ATTR_FIELD_SRC_MAC => FpQual::SrcMac,
        sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC => FpQual::DstMac,
        sys::SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_ID => FpQual::OuterVlan,
        sys::SAI_ACL_TABLE_ATTR_FIELD_TTL => FpQual::Ttl,
        sys::SAI_ACL_TABLE_ATTR_FIELD_IN_PORTS | sys::SAI_ACL_TABLE_ATTR_FIELD_IN_PORT => FpQual::InPorts,
        _ => return None,
    })
}

/// Fields outside [`FpQual`] ([`FpCap::OutPorts`]): the egress port of an egress table.
fn extra_field<B: Backend>(backend: &B, field: sys::sai_attr_id_t) -> bool {
    matches!(
        field,
        sys::SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS | sys::SAI_ACL_TABLE_ATTR_FIELD_OUT_PORT
    ) && backend.fp_supported(FpCap::OutPorts)
}

/// Whether a table match field is programmed (C++ `acl_table_field_supported`).
pub(crate) fn acl_table_field_supported<B: Backend>(backend: &B, field: sys::sai_attr_id_t) -> bool {
    field_qual(field).is_some() || extra_field(backend, field)
}

const BASE_ACTIONS: [i32; 3] = [
    e(sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION),
    e(sys::SAI_ACL_ACTION_TYPE_COUNTER),
    e(sys::SAI_ACL_ACTION_TYPE_SET_POLICER),
];

/// The `SAI_ACL_ACTION_TYPE_*` of ingress tables (C++ `acl_ingress_actions`: switch
/// `ACL_STAGE_INGRESS`, `MAX_ACL_ACTION_COUNT`).
pub(crate) fn acl_ingress_actions<B: Backend + ?Sized>(backend: &B) -> Vec<i32> {
    let mut v = BASE_ACTIONS.to_vec();
    if backend.mirror_supported() {
        v.push(e(sys::SAI_ACL_ACTION_TYPE_MIRROR_INGRESS));
        v.push(e(sys::SAI_ACL_ACTION_TYPE_MIRROR_EGRESS));
    }
    for (cap, action) in [
        (FpCap::Redirect, sys::SAI_ACL_ACTION_TYPE_REDIRECT),
        (FpCap::SetTc, sys::SAI_ACL_ACTION_TYPE_SET_TC),
        (FpCap::SetDscp, sys::SAI_ACL_ACTION_TYPE_SET_DSCP),
    ] {
        if backend.fp_supported(cap) {
            v.push(e(action));
        }
    }
    v
}

/// The same for egress tables (C++ `acl_egress_actions`: empty without [`FpCap::Egress`]).
pub(crate) fn acl_egress_actions<B: Backend + ?Sized>(backend: &B) -> Vec<i32> {
    if backend.fp_supported(FpCap::Egress) {
        vec![
            e(sys::SAI_ACL_ACTION_TYPE_PACKET_ACTION),
            e(sys::SAI_ACL_ACTION_TYPE_COUNTER),
        ]
    } else {
        Vec::new()
    }
}

const fn is_table_field(id: sys::sai_attr_id_t) -> bool {
    id >= sys::SAI_ACL_TABLE_ATTR_FIELD_START && id <= sys::SAI_ACL_TABLE_ATTR_FIELD_END
}

const fn is_entry_field(id: sys::sai_attr_id_t) -> bool {
    id >= sys::SAI_ACL_ENTRY_ATTR_FIELD_START && id <= sys::SAI_ACL_ENTRY_ATTR_FIELD_END
}

const fn is_entry_action(id: sys::sai_attr_id_t) -> bool {
    id >= sys::SAI_ACL_ENTRY_ATTR_ACTION_START && id <= sys::SAI_ACL_ENTRY_ATTR_ACTION_END
}

const fn entry_to_table_field(id: sys::sai_attr_id_t) -> sys::sai_attr_id_t {
    id - sys::SAI_ACL_ENTRY_ATTR_FIELD_START + sys::SAI_ACL_TABLE_ATTR_FIELD_START
}

/// A stage this backend has (C++: ingress, egress with [`FpCap::Egress`]).
fn stage_of<B: Backend>(backend: &B, value: i32) -> Option<Stage> {
    if value == e(sys::SAI_ACL_STAGE_INGRESS) {
        Some(Stage::Ingress)
    } else if value == e(sys::SAI_ACL_STAGE_EGRESS) && backend.fp_supported(FpCap::Egress) {
        Some(Stage::Egress)
    } else {
        None
    }
}

// ------------------------------------------------------------------ the area

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct AclArea;

impl Area for AclArea {
    /// C++ `set_policer_attribute` (`policer_attr`: `METER_TYPE`, `MODE`, `COLOR_SOURCE`),
    /// `set_acl_entry_attribute` (`TABLE_ID`), `set_acl_counter_attribute` (all but `PACKETS` /
    /// `BYTES`: `attr_invalid`); tables, table groups, members and ranges: `attr_invalid(0)`
    /// (all create-only), `sai_acl.cpp:85`-1540.
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        Some(match object_type {
            ObjectType::Policer => crate::dispatch::cpp_refusal(
                attr,
                &[
                    sys::SAI_POLICER_ATTR_METER_TYPE,
                    sys::SAI_POLICER_ATTR_MODE,
                    sys::SAI_POLICER_ATTR_COLOR_SOURCE,
                ],
            ),
            ObjectType::AclEntry => crate::dispatch::cpp_refusal(attr, &[sys::SAI_ACL_ENTRY_ATTR_TABLE_ID]),
            _ => invalid(0),
        })
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Policer => crate::policer::create(backend, store, attrs),
            ObjectType::AclTable => create_table(backend, store, attrs),
            ObjectType::AclEntry => create_entry(backend, store, attrs),
            ObjectType::AclCounter => create_counter(store, attrs),
            ObjectType::AclRange => create_range(backend, store, attrs),
            ObjectType::AclTableGroup => create_group(backend, store, attrs),
            ObjectType::AclTableGroupMember => create_member(backend, store, attrs),
            _ => Err(Error::NotImplemented), // not this area's (dispatch never asks)
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Policer => crate::policer::remove(backend, store, id),
            ObjectType::AclTable => remove_table(backend, store, id),
            ObjectType::AclEntry => remove_entry(backend, store, id),
            ObjectType::AclCounter => {
                let id = AclCounterId::try_from(id)?;
                store.remove::<AclCounter>(id).map(drop)
            }
            ObjectType::AclRange => {
                let id = AclRangeId::try_from(id)?;
                store.remove::<AclRange>(id).map(drop)
            }
            ObjectType::AclTableGroup => {
                let id = AclTableGroupId::try_from(id)?;
                store.remove::<AclGroup>(id).map(drop)
            }
            ObjectType::AclTableGroupMember => {
                store.remove::<AclGroupMember>(AclTableGroupMemberId::try_from(id)?)?;
                acl_reprogram(backend, store);
                Ok(())
            }
            _ => Err(Error::NotImplemented),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Policer => crate::policer::set(backend, store, id, attr),
            ObjectType::AclEntry => set_entry(backend, store, id, attr),
            ObjectType::AclCounter => set_counter(backend, store, id, attr),
            // Every table, range, group and member attribute is create-only (C++ attr_invalid(0)).
            ObjectType::AclTable
            | ObjectType::AclRange
            | ObjectType::AclTableGroup
            | ObjectType::AclTableGroupMember => Err(invalid(0)),
            _ => Err(Error::NotImplemented),
        }
    }

    /// C++ `get_acl_counter_attribute` reads the SDK counter once per call, after the lookup and
    /// before the attributes (`sai_acl.cpp:1270`); every other type per attribute.
    fn get_all<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        if object_type != ObjectType::AclCounter {
            return ids
                .iter()
                .enumerate()
                .map(|(index, attr)| {
                    let meta = sai_meta::attribute(object_type.sai(), *attr)
                        .ok_or_else(|| crate::dispatch::refused_get(object_type.sai(), index))?;
                    Self::get(backend, store, object_type, id, meta).map_err(|e| e.at_index(index))
                })
                .collect();
        }
        let read = counter_read(backend, store, AclCounterId::try_from(id)?)?;
        ids.iter()
            .enumerate()
            .map(|(index, attr)| get_counter(store, id, *attr, read).map_err(|e| e.at_index(index)))
            .collect()
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        match object_type {
            ObjectType::Policer => crate::policer::get(store, id, meta.id),
            ObjectType::AclTable => get_table(backend, store, id, meta),
            ObjectType::AclEntry => get_entry(backend, store, id, meta),
            ObjectType::AclCounter => get_counter(
                store,
                id,
                meta.id,
                counter_read(backend, store, AclCounterId::try_from(id)?)?,
            ),
            ObjectType::AclRange => get_range(store, id, meta.id),
            ObjectType::AclTableGroup => get_group(store, id, meta.id),
            ObjectType::AclTableGroupMember => get_member(store, id, meta.id),
            _ => Err(Error::NotImplemented),
        }
    }
}

// ------------------------------------------------------------------ tables

#[allow(clippy::too_many_lines, reason = "the C++ function's checks in its order")]
/// C++ `create_acl_table`: a `MACsec` stage goes to the `MACsec` model; else every attribute in
/// order (a match field this SAI does not program, a bind point other than PORT / LAG, a stage
/// other than ingress (or egress with [`FpCap::Egress`]) are `NOT_SUPPORTED`), the stage
/// mandatory, the action list checked against what the stage has, then the SDK field group
/// with the next priority (used up even when the backend refuses, as in the C++).
fn create_table<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    if crate::macsec::macsec_acl_stage(attrs) {
        let id = store.peek_id(ObjectType::AclTable);
        crate::macsec::macsec_acl_table_create(backend, store, id, attrs)?;
        return store.insert(MacsecAclTable).map(TypedId::raw);
    }
    let mut stage = None;
    let mut fields = BTreeSet::new();
    let (mut range_types, mut bind_types, mut action_types, mut size) = (Vec::new(), Vec::new(), Vec::new(), 0);
    for (i, a) in attrs.iter().enumerate() {
        if is_table_field(a.id) {
            if a.id == sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE {
                // AclOrch adds L4 port ranges to every ingress table; accepted and kept
                range_types = i32_list(&a.value)?;
                continue;
            }
            // The two user-defined-field group attributes in the range are object ids (C++ reads
            // their first byte as the bool): set means a non-null id here.
            let set = match &a.value {
                Value::Oid(o) => !o.is_null(),
                other => bool_of(other)?,
            };
            if !set {
                continue;
            }
            if !acl_table_field_supported(backend, a.id) {
                log::error(
                    sys::SAI_API_ACL,
                    format_args!("ACL table: match field {} not supported", a.id),
                );
                return Err(not_supported(i));
            }
            fields.insert(a.id);
            continue;
        }
        match a.id {
            sys::SAI_ACL_TABLE_ATTR_ACL_STAGE => {
                let v = enum_of(&a.value)?;
                let Some(s) = stage_of(backend, v) else {
                    log::error(
                        sys::SAI_API_ACL,
                        format_args!("ACL table: stage {v} not supported (ingress only)"),
                    );
                    return Err(not_supported(i));
                };
                stage = Some(s);
            }
            sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST => {
                bind_types = i32_list(&a.value)?;
                let ok =
                    |b: &i32| *b == e(sys::SAI_ACL_BIND_POINT_TYPE_PORT) || *b == e(sys::SAI_ACL_BIND_POINT_TYPE_LAG);
                if !bind_types.iter().all(ok) {
                    return Err(not_supported(i));
                }
            }
            sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST => action_types = i32_list(&a.value)?,
            sys::SAI_ACL_TABLE_ATTR_SIZE => size = u32_of(&a.value)?,
            _ => return Err(not_supported(i)),
        }
    }
    let stage = stage.ok_or(Error::MandatoryAttributeMissing)?;
    let egress = stage == Stage::Egress;
    let caps = [FpCap::Redirect, FpCap::SetTc, FpCap::SetDscp].map(|c| backend.fp_supported(c));
    let allowed = if egress {
        acl_egress_actions(backend)
    } else if caps.iter().any(|c| *c) {
        acl_ingress_actions(backend)
    } else {
        // the original rule (no mirror actions without a redirect / TC / DSCP capability)
        BASE_ACTIONS.to_vec()
    };
    if let Some(bad) = action_types.iter().find(|a| !allowed.contains(a)) {
        log::error(
            sys::SAI_API_ACL,
            format_args!("ACL table: action type {bad} not supported"),
        );
        let index = attrs
            .iter()
            .position(|a| a.id == sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST)
            .unwrap_or(0);
        return Err(not_supported(index));
    }
    let mut x = FpGroupExtra {
        egress,
        ..FpGroupExtra::default()
    };
    if backend.fp_supported(FpCap::Ranges) {
        for rt in &range_types {
            x.src_range |= *rt == e(sys::SAI_ACL_RANGE_TYPE_L4_SRC_PORT_RANGE);
            x.dst_range |= *rt == e(sys::SAI_ACL_RANGE_TYPE_L4_DST_PORT_RANGE);
        }
    }
    x.out_ports = fields.contains(&sys::SAI_ACL_TABLE_ATTR_FIELD_OUT_PORTS)
        || fields.contains(&sys::SAI_ACL_TABLE_ATTR_FIELD_OUT_PORT);
    for act in &action_types {
        x.redirect |= *act == e(sys::SAI_ACL_ACTION_TYPE_REDIRECT);
        x.set_tc |= *act == e(sys::SAI_ACL_ACTION_TYPE_SET_TC);
        x.set_dscp |= *act == e(sys::SAI_ACL_ACTION_TYPE_SET_DSCP);
        x.mirror |=
            *act == e(sys::SAI_ACL_ACTION_TYPE_MIRROR_INGRESS) || *act == e(sys::SAI_ACL_ACTION_TYPE_MIRROR_EGRESS);
    }
    let mut qset = BTreeSet::from([FpQual::InPorts]);
    qset.extend(
        fields
            .iter()
            .filter(|f| !extra_field(backend, **f))
            .filter_map(|f| field_qual(*f)),
    );
    let qset: Vec<FpQual> = qset.into_iter().collect();
    let priority = update_state(store, |s| {
        let p = s.next_priority;
        s.next_priority = s.next_priority.wrapping_add(1);
        p
    })?;
    let gid = backend.fp_group_create_ex(&qset, priority, x).inspect_err(|err| {
        log::error(
            sys::SAI_API_ACL,
            format_args!(
                "ACL table ({} match fields): SDK field group create failed ({})",
                fields.len(),
                err.status()
            ),
        );
    })?;
    let count = fields.len();
    let id = store.insert(AclTable {
        stage,
        bind_types,
        action_types,
        fields,
        range_types,
        size,
        gid,
    })?;
    log::notice(
        sys::SAI_API_ACL,
        format_args!("ACL table: SDK field group {}, {count} match fields", gid.0),
    );
    Ok(id.raw())
}

/// C++ `remove_acl_table`: in use while it has entries, is bound, a group member or a counter's
/// table (the store's references); the SDK group goes first (`ITEM_NOT_FOUND` counts as gone).
fn remove_table<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid) -> Result<()> {
    let id = AclTableId::try_from(raw)?;
    if store.get::<MacsecAclTable>(id).is_ok() {
        store.ensure_removable::<MacsecAclTable>(id)?;
        crate::macsec::macsec_acl_table_remove(backend, store, raw)?;
        return store.remove::<MacsecAclTable>(id).map(drop);
    }
    store.ensure_removable::<AclTable>(id)?;
    let gid = store.get::<AclTable>(id)?.gid;
    match backend.fp_group_destroy(gid) {
        Ok(()) | Err(Error::ItemNotFound) => {}
        Err(other) => return Err(other),
    }
    store.remove::<AclTable>(id).map(drop)
}

/// C++ `get_acl_table_attribute`.
fn get_table<B: Backend>(backend: &B, store: &Store, raw: RawOid, meta: &AttrMeta) -> Result<Value> {
    let id = AclTableId::try_from(raw)?;
    if store.get::<MacsecAclTable>(id).is_ok() {
        return crate::macsec::macsec_acl_table_get(backend, store, raw, meta);
    }
    let t = store.get::<AclTable>(id)?;
    let attr = meta.id;
    if attr == sys::SAI_ACL_TABLE_ATTR_FIELD_ACL_RANGE_TYPE {
        return Ok(Value::I32List(t.range_types.clone()));
    }
    if is_table_field(attr) {
        // never one of the table's fields: the user-defined-field groups (object ids) are null
        if meta.value_type == ValueType::Oid {
            return Ok(Value::Oid(RawOid::NULL));
        }
        return Ok(Value::Bool(t.fields.contains(&attr)));
    }
    Ok(match attr {
        sys::SAI_ACL_TABLE_ATTR_ACL_STAGE => Value::Enum(t.stage.sai()),
        sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST => Value::I32List(t.bind_types.clone()),
        sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST => Value::I32List(t.action_types.clone()),
        sys::SAI_ACL_TABLE_ATTR_SIZE => Value::U32(t.size),
        sys::SAI_ACL_TABLE_ATTR_ENTRY_LIST => Value::OidList(
            store
                .iter::<AclEntry>()
                .filter(|(_, entry)| entry.table == id)
                .map(|(eid, _)| eid.raw())
                .collect(),
        ),
        // NOT_SUPPORTED: CRM marks it unsupported
        sys::SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY => match backend.fp_group_free(t.gid) {
            Ok(free) => Value::U32(free),
            Err(Error::NotSupported) => return Err(not_supported(0)),
            Err(other) => return Err(other),
        },
        // AVAILABLE_ACL_COUNTER: CRM marks it unsupported
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ binding

/// The ports an object stands for (C++ `attach_phys`: a port, or a LAG's member ports).
fn attach_phys(store: &Store, obj: RawOid) -> Vec<HwPort> {
    crate::lag::attach_phys(store, obj)
}

/// The ports an ACL table applies to: everything bound to it directly or through a table group
/// it is a member of, in its stage (C++ `table_ports`).
fn table_ports(store: &Store, table: AclTableId) -> BTreeSet<HwPort> {
    let Some(bindings) = acl_state(store) else {
        return BTreeSet::new();
    };
    let stage = store.get::<AclTable>(table).map_or(Stage::Ingress, |t| t.stage);
    let mut out = BTreeSet::new();
    for (obj, acl) in bindings.binds(stage) {
        let hit = acl == table.raw()
            || (acl.type_bits() == sys::SAI_OBJECT_TYPE_ACL_TABLE_GROUP
                && store
                    .iter::<AclGroupMember>()
                    .any(|(_, m)| m.group.raw() == acl && m.table == table));
        if hit {
            out.extend(attach_phys(store, obj));
        }
    }
    out
}

/// C++ `acl_bind_set`: PORT / LAG `INGRESS_ACL` (`ingress`) / `EGRESS_ACL` set to a table, a
/// table group or null; an error is for attribute index 0 (the C++ index is 0 for every set, and
/// a LAG create ignores the answer). Without
/// [`FpCap::Egress`] an egress binding is refused unless it is null. The ACL must exist and be
/// of the binding's stage. The entries are then re-programmed.
pub(crate) fn acl_bind_set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    obj: RawOid,
    ingress: bool,
    acl: RawOid,
) -> Result<()> {
    let index = 0;
    let stage = Stage::of_binding(ingress);
    if stage == Stage::Egress && !backend.fp_supported(FpCap::Egress) {
        return if acl.is_null() {
            Ok(())
        } else {
            Err(not_supported(index))
        };
    }
    if !acl.is_null() {
        let acl_stage = match (get_typed::<AclGroup>(store, acl), get_typed::<AclTable>(store, acl)) {
            (Some(g), _) => g.stage,
            (None, Some(t)) => t.stage,
            (None, None) => return Err(invalid(index)),
        };
        if acl_stage != stage {
            return Err(invalid(index));
        }
    }
    update_state(store, |s| {
        let binds = s.bindings.entry(stage).or_default();
        if acl.is_null() {
            binds.remove(&obj);
        } else {
            binds.insert(obj, acl);
        }
        s.bindings.retain(|_, b| !b.is_empty());
    })?;
    acl_reprogram(backend, store);
    Ok(())
}

/// C++ `acl_bind_get`: what `obj` is bound to, ingress or egress (null: nothing).
pub(crate) fn acl_bind_get(store: &Store, obj: RawOid, ingress: bool) -> RawOid {
    acl_state(store)
        .and_then(|s| s.bound(obj, Stage::of_binding(ingress)))
        .unwrap_or(RawOid::NULL)
}

/// A removed LAG loses its ingress binding, and the entries are re-programmed if it had one
/// (C++ `remove_lag`: `if (c.acl_bind.erase(id)) acl_reprogram();`; its egress binding stays, as
/// in the C++, where it names nothing).
pub(crate) fn acl_bind_forget<B: Backend>(backend: &mut B, store: &mut Store, obj: RawOid) {
    let erased = update_state(store, |s| {
        let erased = s
            .bindings
            .get_mut(&Stage::Ingress)
            .is_some_and(|b| b.remove(&obj).is_some());
        s.bindings.retain(|_, b| !b.is_empty());
        erased
    });
    if erased == Ok(true) {
        acl_reprogram(backend, store);
    }
}

/// PORT / LAG objects with an ingress ACL bound (test port `acl_binding_count`).
pub fn acl_binding_count(store: &Store) -> usize {
    acl_state(store).map_or(0, |s| s.binds(Stage::Ingress).count())
}

// ------------------------------------------------------------------ entries

/// Colour actions of a policer onto a rule (C++ `apply_colors`).
pub(crate) fn apply_colors(r: &mut FpRule, p: &Policer, cpu_copy_only: bool) {
    r.policer = Some(p.pid);
    r.green_drop = p.settings.green.drops();
    r.yellow_drop = p.settings.yellow.drops();
    r.red_drop = p.settings.red.drops();
    r.cpu_copy_only = cpu_copy_only;
}

fn masked<T: Copy>(v: &FieldMatch<T>) -> Masked<T> {
    Masked {
        value: v.data,
        mask: v.mask,
    }
}

/// C++ `entry_rule`: the SDK rule of an entry; `None` = nothing to install (admin down without
/// [`FpCap::EntryEnable`], or no port it applies to).
#[allow(clippy::too_many_lines, reason = "one arm per field and action, as the C++ switch")]
fn entry_rule<B: Backend>(backend: &B, store: &Store, entry: &AclEntry) -> Option<FpRule> {
    use AclFieldData as D;
    if !entry.admin && !backend.fp_supported(FpCap::EntryEnable) {
        return None;
    }
    let mut ports = table_ports(store, entry.table);
    let mut r = FpRule {
        priority: entry.priority,
        ..FpRule::default()
    };
    let m: &mut FpMatch = &mut r.m;
    let phys_of = |objs: &[RawOid]| -> BTreeSet<HwPort> { objs.iter().flat_map(|o| attach_phys(store, *o)).collect() };
    for (id, d) in &entry.fields {
        match (*id, d) {
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_IP, D::Ip4(v)) => m.src_ip = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP, D::Ip4(v)) => m.dst_ip = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_IPV6, D::Ip6(v)) => m.src_ip6 = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IPV6, D::Ip6(v)) => m.dst_ip6 = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_SRC_PORT, D::U16(v)) => m.l4_src_port = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_L4_DST_PORT, D::U16(v)) => m.l4_dst_port = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_IP_PROTOCOL | sys::SAI_ACL_ENTRY_ATTR_FIELD_IPV6_NEXT_HEADER, D::U8(v)) => {
                m.ip_protocol = Some(masked(v));
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE, D::U16(v)) => m.ether_type = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_TCP_FLAGS, D::U8(v)) => m.tcp_flags = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMP_TYPE | sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_TYPE, D::U8(v)) => {
                let old = m.icmp_type_code.unwrap_or_default();
                m.icmp_type_code = Some(Masked {
                    value: (old.value & 0x00ff) | (u16::from(v.data) << 8),
                    mask: (old.mask & 0x00ff) | (u16::from(v.mask) << 8),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMP_CODE | sys::SAI_ACL_ENTRY_ATTR_FIELD_ICMPV6_CODE, D::U8(v)) => {
                let old = m.icmp_type_code.unwrap_or_default();
                m.icmp_type_code = Some(Masked {
                    value: (old.value & 0xff00) | u16::from(v.data),
                    mask: (old.mask & 0xff00) | u16::from(v.mask),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_DSCP, D::U8(v)) => {
                let old = m.tos.unwrap_or_default();
                m.tos = Some(Masked {
                    value: (old.value & 0x03) | ((v.data & 0x3f) << 2),
                    mask: (old.mask & 0x03) | ((v.mask & 0x3f) << 2),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ECN, D::U8(v)) => {
                let old = m.tos.unwrap_or_default();
                m.tos = Some(Masked {
                    value: (old.value & 0xfc) | (v.data & 0x03),
                    mask: (old.mask & 0xfc) | (v.mask & 0x03),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_TOS, D::U8(v)) => m.tos = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_IP_TYPE, D::Enum(v)) => {
                if v.data != e(sys::SAI_ACL_IP_TYPE_ANY) {
                    m.ip_type = Some(v.data);
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_SRC_MAC, D::Mac(v)) => {
                m.src_mac = Some(Masked {
                    value: MacAddr(v.data),
                    mask: MacAddr(v.mask),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC, D::Mac(v)) => {
                m.dst_mac = Some(Masked {
                    value: MacAddr(v.data),
                    mask: MacAddr(v.mask),
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_OUTER_VLAN_ID, D::U16(v)) => {
                m.outer_vlan = Some(Masked {
                    value: v.data & 0xfff,
                    mask: v.mask & 0xfff,
                });
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_TTL, D::U8(v)) => m.ttl = Some(masked(v)),
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE, D::OidList(objs)) => {
                for range in objs.iter().filter_map(|o| get_typed::<AclRange>(store, *o)) {
                    // LIMIT is checked to 0..=65535 at create
                    let bounds = (
                        u16::try_from(range.min).unwrap_or(u16::MAX),
                        u16::try_from(range.max).unwrap_or(u16::MAX),
                    );
                    match range.kind {
                        RangeKind::L4Src => m.l4_src_range = Some(bounds),
                        RangeKind::L4Dst => m.l4_dst_range = Some(bounds),
                    }
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS, D::OidList(objs)) => {
                m.out_ports = phys_of(objs).into_iter().collect();
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORT, D::Oid(obj)) => {
                m.out_ports = phys_of(core::slice::from_ref(obj)).into_iter().collect();
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS, D::OidList(objs)) => {
                let wanted = phys_of(objs);
                ports.retain(|p| wanted.contains(p));
            }
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_IN_PORT, D::Oid(obj)) => {
                let wanted = phys_of(core::slice::from_ref(obj));
                ports.retain(|p| wanted.contains(p));
            }
            _ => {}
        }
    }
    if ports.is_empty() {
        return None;
    }
    r.m.in_ports = Some(ports.into_iter().collect());
    for (id, a) in &entry.actions {
        match (*id, a) {
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, AclParam::Enum(v)) => match u32::try_from(*v).ok() {
                Some(sys::SAI_PACKET_ACTION_DROP | sys::SAI_PACKET_ACTION_DENY) => r.drop = true,
                Some(sys::SAI_PACKET_ACTION_FORWARD | sys::SAI_PACKET_ACTION_TRANSIT) => r.drop_cancel = true,
                Some(sys::SAI_PACKET_ACTION_COPY | sys::SAI_PACKET_ACTION_LOG) => r.copy_cpu = true,
                Some(sys::SAI_PACKET_ACTION_TRAP) => {
                    r.copy_cpu = true;
                    r.drop = true;
                }
                _ => {}
            },
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, AclParam::Oid(k)) => r.counter = !k.is_null(),
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT, AclParam::Oid(o)) => {
                r.redirect = if o.type_bits() == sys::SAI_OBJECT_TYPE_NEXT_HOP {
                    crate::l3::next_hop_egress(store, *o).map(Redirect::Egress)
                } else {
                    crate::lag::attach_dest(store, *o).map(Redirect::Dest)
                };
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_TC, AclParam::U8(v)) => r.set_tc = Some(*v),
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP, AclParam::U8(v)) => r.set_dscp = Some(v & 0x3f),
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER, AclParam::Oid(p)) => {
                if let Some(p) = crate::policer::policer(store, *p) {
                    apply_colors(&mut r, p, false);
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS, AclParam::OidList(sessions)) => {
                r.mirror_ingress = sessions
                    .iter()
                    .filter_map(|s| crate::mirror::mirror_backend_id(store, *s))
                    .collect();
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS, AclParam::OidList(sessions)) => {
                r.mirror_egress = sessions
                    .iter()
                    .filter_map(|s| crate::mirror::mirror_backend_id(store, *s))
                    .collect();
            }
            _ => {}
        }
    }
    Some(r)
}

/// C++ `counter_fold`: the SDK counter of an installed entry added to its SAI counter (before
/// the SDK entry goes).
fn counter_fold<B: Backend>(backend: &B, store: &mut Store, e: &AclEntry) {
    let Some(installed) = &e.installed else {
        return;
    };
    let Ok(counter) = AclCounterId::try_from(e.counter()) else {
        return;
    };
    if store.get::<AclCounter>(counter).is_err() {
        return;
    }
    if let Ok(hit) = backend.fp_entry_counter(installed.eid) {
        let _ = store.update::<AclCounter, _>(counter, |k| {
            k.base_packets = k.base_packets.wrapping_add(hit.packets);
            k.base_bytes = k.base_bytes.wrapping_add(hit.bytes);
        });
    }
}

/// C++ `entry_uninstall`: counter folded, SDK entry destroyed (its answer ignored).
fn entry_uninstall<B: Backend>(backend: &mut B, store: &mut Store, e: &mut AclEntry) {
    if e.installed.is_none() {
        return;
    }
    counter_fold(backend, store, e);
    if let Some(installed) = e.installed.take() {
        let _ = backend.fp_entry_destroy(installed.eid);
    }
}

/// C++ `entry_program`: install, re-install or remove an entry's SDK rule to match its state.
/// The same rule installed: only the on / off state ([`FpCap::EntryEnable`]); a changed rule:
/// updated in place ([`FpCap::EntryUpdate`]) or destroyed (counter folded) and created anew.
fn entry_program<B: Backend>(backend: &mut B, store: &mut Store, e: &mut AclEntry) -> Result<()> {
    let Some(r) = entry_rule(&*backend, store, e) else {
        entry_uninstall(backend, store, e);
        return Ok(());
    };
    let want_on = e.admin;
    let enable = backend.fp_supported(FpCap::EntryEnable);
    if let Some(installed) = e.installed.as_mut().filter(|i| i.rule == r) {
        if enable && want_on != installed.enabled {
            backend.fp_entry_enable(installed.eid, want_on)?;
            installed.enabled = want_on;
        }
        return Ok(());
    }
    let gid = store.get::<AclTable>(e.table).map_err(|_| Error::InvalidObjectId)?.gid;
    if let Some(installed) = e
        .installed
        .as_mut()
        .filter(|_| backend.fp_supported(FpCap::EntryUpdate))
    {
        // in place: the SDK counter keeps counting
        match backend.fp_entry_update(installed.eid, &r) {
            Ok(()) => {
                installed.rule = r;
                if enable && want_on != installed.enabled {
                    backend.fp_entry_enable(installed.eid, want_on)?;
                    installed.enabled = want_on;
                }
                return Ok(());
            }
            Err(err) => log::warn(
                sys::SAI_API_ACL,
                format_args!("ACL entry: in-place update failed ({}), re-creating it", err.status()),
            ),
        }
    }
    // the SDK entry is re-created (counters folded in)
    entry_uninstall(backend, store, e);
    let ports = r.m.in_ports.as_ref().map_or(0, Vec::len);
    let eid = backend.fp_entry_create(gid, &r).inspect_err(|err| {
        log::error(
            sys::SAI_API_ACL,
            format_args!(
                "ACL entry (priority {}, {ports} ports): SDK field entry create failed ({})",
                e.priority,
                err.status()
            ),
        );
    })?;
    let (drop, copy, counted) = (r.drop, r.copy_cpu, r.counter);
    let mut installed = Installed {
        eid,
        rule: r,
        enabled: true,
    };
    if !want_on {
        // FpCap::EntryEnable (entry_rule installs admin-down entries only then)
        match backend.fp_entry_enable(eid, false) {
            Ok(()) => installed.enabled = false,
            Err(err) => log::error(
                sys::SAI_API_ACL,
                format_args!("ACL entry: disabling SDK entry {} failed ({})", eid.0, err.status()),
            ),
        }
    }
    e.installed = Some(Box::new(installed));
    log::notice(
        sys::SAI_API_ACL,
        format_args!(
            "ACL entry: SDK entry {} in group {}, priority {}, {ports} ports{}{}{}",
            eid.0,
            gid.0,
            e.priority,
            if drop { ", drop" } else { "" },
            if copy { ", copy to CPU" } else { "" },
            if counted { ", counted" } else { "" }
        ),
    );
    Ok(())
}

/// C++ `acl_reprogram`: every entry made to match its state (after binding, LAG membership or
/// policer changes); a failure is logged and the next entry tried.
pub(crate) fn acl_reprogram<B: Backend>(backend: &mut B, store: &mut Store) {
    let ids: Vec<AclEntryId> = store.iter::<AclEntry>().map(|(id, _)| id).collect();
    for id in ids {
        let Ok(mut entry) = store.get::<AclEntry>(id).cloned() else {
            continue;
        };
        let result = entry_program(backend, store, &mut entry);
        // The references of an entry do not change here.
        let _ = store.update::<AclEntry, _>(id, |e| *e = entry);
        if let Err(err) = result {
            log::error(
                sys::SAI_API_ACL,
                format_args!("ACL entry {:x}: re-program failed ({})", id.raw().0, err.status()),
            );
        }
    }
}

/// A front-panel port (LAG members too) or a LAG: what an entry's port fields may name.
fn port_or_lag(store: &Store, obj: RawOid) -> bool {
    let front = PortId::try_from(obj)
        .ok()
        .and_then(|p| store.get::<Port>(p).ok())
        .is_some_and(|p| matches!(p.kind, PortKind::Front { .. }));
    front || store.object_type(obj) == Some(ObjectType::Lag)
}

/// The entry whose `ACTION_COUNTER` names `counter`, other than `except`.
fn counter_user(store: &Store, counter: RawOid, except: Option<AclEntryId>) -> Option<AclEntryId> {
    store
        .iter::<AclEntry>()
        .find(|(id, e)| Some(*id) != except && e.counter() == counter)
        .map(|(id, _)| id)
}

/// C++ `entry_attr`: one entry attribute into the model, validated against the table (statuses
/// as the C++: a field the table lacks or an object that is not a port / LAG / range / live
/// counter / policer / mirror session is `INVALID_ATTR_VALUE`, a field or action this SAI does
/// not program `NOT_SUPPORTED`, a counter another entry counts with `OBJECT_IN_USE`).
#[allow(clippy::too_many_lines, reason = "the C++ function, case by case")]
fn entry_attr<B: Backend>(
    backend: &B,
    store: &Store,
    e: &mut AclEntry,
    this: Option<AclEntryId>,
    table: Option<&AclTable>,
    a: &Attr,
    i: usize,
) -> Result<()> {
    if a.id == sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE {
        // ACL_RANGE objects (FpCap::Ranges)
        if !backend.fp_supported(FpCap::Ranges) {
            return Err(not_supported(i));
        }
        let data = match &a.value {
            Value::AclField(AclField::Enabled(data)) => data,
            Value::AclField(AclField::Disabled(_)) => {
                e.fields.remove(&a.id);
                return Ok(());
            }
            _ => return Err(Error::Failure),
        };
        let AclFieldData::OidList(objs) = data else {
            return Err(Error::Failure);
        };
        for o in objs {
            let range = get_typed::<AclRange>(store, *o).ok_or(invalid(i))?;
            if table.is_some_and(|t| !t.range_types.contains(&range.kind.sai())) {
                log::error(
                    sys::SAI_API_ACL,
                    format_args!("ACL entry: range type {} is not one of its table's", range.kind.sai()),
                );
                return Err(invalid(i));
            }
        }
        e.fields.insert(a.id, data.clone());
        return Ok(());
    }
    if is_entry_field(a.id) {
        let tf = entry_to_table_field(a.id);
        if !acl_table_field_supported(backend, tf) {
            return Err(not_supported(i));
        }
        let data = match &a.value {
            Value::AclField(AclField::Enabled(data)) => data,
            Value::AclField(AclField::Disabled(_)) => {
                e.fields.remove(&a.id);
                return Ok(());
            }
            _ => return Err(Error::Failure),
        };
        if table.is_some_and(|t| !t.fields.contains(&tf)) {
            log::error(
                sys::SAI_API_ACL,
                format_args!("ACL entry: match field {} is not a field of its table", a.id),
            );
            return Err(invalid(i));
        }
        let objs: &[RawOid] = match (a.id, data) {
            (
                sys::SAI_ACL_ENTRY_ATTR_FIELD_IN_PORTS | sys::SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORTS,
                AclFieldData::OidList(list),
            ) => list,
            (sys::SAI_ACL_ENTRY_ATTR_FIELD_IN_PORT | sys::SAI_ACL_ENTRY_ATTR_FIELD_OUT_PORT, AclFieldData::Oid(o)) => {
                core::slice::from_ref(o)
            }
            _ => &[],
        };
        // front-panel ports (LAG members too) or LAGs
        if !objs.iter().all(|o| port_or_lag(store, *o)) {
            return Err(invalid(i));
        }
        e.fields.insert(a.id, data.clone());
        return Ok(());
    }
    if is_entry_action(a.id) {
        let param = match &a.value {
            Value::AclAction(AclAction::Enabled(param)) => param,
            Value::AclAction(AclAction::Disabled(_)) => {
                e.actions.remove(&a.id);
                return Ok(());
            }
            _ => return Err(Error::Failure),
        };
        match (a.id, param) {
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION, AclParam::Enum(v)) => {
                let known = matches!(
                    u32::try_from(*v).ok(),
                    Some(
                        sys::SAI_PACKET_ACTION_DROP
                            | sys::SAI_PACKET_ACTION_DENY
                            | sys::SAI_PACKET_ACTION_FORWARD
                            | sys::SAI_PACKET_ACTION_TRANSIT
                            | sys::SAI_PACKET_ACTION_COPY
                            | sys::SAI_PACKET_ACTION_LOG
                            | sys::SAI_PACKET_ACTION_TRAP
                    )
                );
                if !known {
                    return Err(not_supported(i));
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER, AclParam::Oid(k)) => {
                let exists = get_typed::<AclCounter>(store, *k).is_some();
                if !k.is_null() && !exists {
                    return Err(invalid(i));
                }
                // one entry per counter (the SDK counter is the entry's)
                if exists && counter_user(store, *k, this).is_some() {
                    return Err(Error::ObjectInUse);
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_POLICER, AclParam::Oid(p)) => {
                if !p.is_null() && !crate::policer::policer_valid(store, *p) {
                    return Err(invalid(i));
                }
            }
            (
                sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_INGRESS | sys::SAI_ACL_ENTRY_ATTR_ACTION_MIRROR_EGRESS,
                AclParam::OidList(sessions),
            ) => {
                // mirror sessions (crate::mirror): only with a backend that mirrors
                if !backend.mirror_supported() {
                    return Err(not_supported(i));
                }
                if !sessions.iter().all(|s| crate::mirror::mirror_valid(store, *s)) {
                    return Err(invalid(i));
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_REDIRECT, AclParam::Oid(o)) => {
                if !backend.fp_supported(FpCap::Redirect) {
                    return Err(not_supported(i));
                }
                let next_hop = store.object_type(*o) == Some(ObjectType::NextHop);
                let attach = matches!(o.type_bits(), sys::SAI_OBJECT_TYPE_PORT | sys::SAI_OBJECT_TYPE_LAG)
                    && crate::lag::attach_valid(store, *o);
                if !next_hop && !attach {
                    return Err(invalid(i));
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_TC, AclParam::U8(v)) => {
                if !backend.fp_supported(FpCap::SetTc) {
                    return Err(not_supported(i));
                }
                if *v > 7 {
                    return Err(invalid(i));
                }
            }
            (sys::SAI_ACL_ENTRY_ATTR_ACTION_SET_DSCP, AclParam::U8(v)) => {
                if !backend.fp_supported(FpCap::SetDscp) {
                    return Err(not_supported(i));
                }
                if *v > 63 {
                    return Err(invalid(i));
                }
            }
            _ => {
                log::error(
                    sys::SAI_API_ACL,
                    format_args!("ACL entry: action {} not supported", a.id),
                );
                return Err(not_supported(i));
            }
        }
        e.actions.insert(a.id, param.clone());
        return Ok(());
    }
    match a.id {
        sys::SAI_ACL_ENTRY_ATTR_PRIORITY => {
            let p = u32_of(&a.value)?;
            if let Ok(l) = backend.chip_limits() {
                if p < l.acl_prio_min || p > l.acl_prio_max {
                    return Err(invalid(i));
                }
            }
            e.priority = p;
        }
        sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE => e.admin = bool_of(&a.value)?,
        _ => return Err(not_supported(i)),
    }
    Ok(())
}

/// C++ `create_acl_entry`: `TABLE_ID` mandatory; an entry of a `MACsec` table goes to the
/// `MACsec` model; else every attribute in order against the table, then the SDK entry (when
/// the table is bound), then the object.
fn create_entry<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let ti = attrs
        .iter()
        .position(|a| a.id == sys::SAI_ACL_ENTRY_ATTR_TABLE_ID)
        .ok_or(Error::MandatoryAttributeMissing)?;
    let raw_table = attrs.get(ti).map(|a| oid_of(&a.value)).ok_or(Error::Failure)??;
    let table_id = AclTableId::try_from(raw_table).map_err(|_| invalid(ti))?;
    if store.get::<MacsecAclTable>(table_id).is_ok() {
        let id = store.peek_id(ObjectType::AclEntry);
        crate::macsec::macsec_acl_entry_create(backend, store, id, attrs)?;
        return store.insert(MacsecAclEntry { table: table_id }).map(TypedId::raw);
    }
    let table = store.get::<AclTable>(table_id).map_err(|_| invalid(ti))?.clone();
    let mut e = AclEntry {
        table: table_id,
        priority: 0,
        admin: true,
        fields: BTreeMap::new(),
        actions: BTreeMap::new(),
        installed: None,
    };
    for (i, a) in attrs.iter().enumerate() {
        if a.id == sys::SAI_ACL_ENTRY_ATTR_TABLE_ID {
            continue;
        }
        entry_attr(&*backend, store, &mut e, None, Some(&table), a, i)?;
    }
    entry_program(backend, store, &mut e)?;
    store.insert(e).map(TypedId::raw)
}

/// C++ `remove_acl_entry`: the SDK entry goes (counter folded), then the object.
fn remove_entry<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid) -> Result<()> {
    let id = AclEntryId::try_from(raw)?;
    if store.get::<MacsecAclEntry>(id).is_ok() {
        crate::macsec::macsec_acl_entry_remove(backend, store, raw)?;
        return store.remove::<MacsecAclEntry>(id).map(drop);
    }
    let mut e = store.get::<AclEntry>(id)?.clone();
    entry_uninstall(backend, store, &mut e);
    store.remove::<AclEntry>(id).map(drop)
}

/// C++ `set_acl_entry_attribute`: the attribute into a copy of the entry; a counter change
/// folds the SDK counter into the old counter first; then the SDK rule is made to match. The
/// model takes the request even when that last step fails (the entry is then uninstalled), as
/// in the C++ (`sai_acl.cpp`:1139).
fn set_entry<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid, attr: &Attr) -> Result<()> {
    let id = AclEntryId::try_from(raw)?;
    if store.get::<MacsecAclEntry>(id).is_ok() {
        if attr.id == sys::SAI_ACL_ENTRY_ATTR_TABLE_ID {
            return Err(invalid(0));
        }
        return crate::macsec::macsec_acl_entry_set(backend, store, raw, attr);
    }
    let mut old = store.get::<AclEntry>(id)?.clone();
    if attr.id == sys::SAI_ACL_ENTRY_ATTR_TABLE_ID {
        return Err(invalid(0));
    }
    let table = store.get::<AclTable>(old.table).ok().cloned();
    let mut n = old.clone();
    entry_attr(&*backend, store, &mut n, Some(id), table.as_ref(), attr, 0)?;
    if old.counter() != n.counter() {
        // fold into the old counter first
        entry_uninstall(backend, store, &mut old);
    }
    n.installed = old.installed;
    let result = entry_program(backend, store, &mut n);
    store.update::<AclEntry, _>(id, |e| *e = n)?;
    result.map_err(|err| attr_backend_status(err, 0))
}

/// C++ `get_acl_entry_attribute`: an unset field or action answers `enable = false`; object
/// lists come back as they were given.
fn get_entry<B: Backend>(backend: &B, store: &Store, raw: RawOid, meta: &AttrMeta) -> Result<Value> {
    let id = AclEntryId::try_from(raw)?;
    if store.get::<MacsecAclEntry>(id).is_ok() {
        return crate::macsec::macsec_acl_entry_get(backend, store, raw, meta);
    }
    let e = store.get::<AclEntry>(id)?;
    let attr = meta.id;
    if is_entry_field(attr) {
        let ranges = attr == sys::SAI_ACL_ENTRY_ATTR_FIELD_ACL_RANGE_TYPE && backend.fp_supported(FpCap::Ranges);
        if !ranges && !acl_table_field_supported(backend, entry_to_table_field(attr)) {
            return Err(not_supported(0));
        }
        let ValueType::AclField(kind) = meta.value_type else {
            return Err(Error::Failure);
        };
        return Ok(Value::AclField(
            e.fields
                .get(&attr)
                .cloned()
                .map_or(AclField::Disabled(kind), AclField::Enabled),
        ));
    }
    if is_entry_action(attr) {
        let ValueType::AclAction(kind) = meta.value_type else {
            return Err(Error::Failure);
        };
        return Ok(Value::AclAction(
            e.actions
                .get(&attr)
                .cloned()
                .map_or(AclAction::Disabled(kind), AclAction::Enabled),
        ));
    }
    Ok(match attr {
        sys::SAI_ACL_ENTRY_ATTR_TABLE_ID => Value::Oid(e.table.raw()),
        sys::SAI_ACL_ENTRY_ATTR_PRIORITY => Value::U32(e.priority),
        sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE => Value::Bool(e.admin),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ counters

/// C++ `create_acl_counter`: `TABLE_ID` (an ASIC table) mandatory.
fn create_counter(store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let mut table = None;
    let mut k = AclCounter {
        table: AclTableId::from_index(0),
        packets: false,
        bytes: false,
        base_packets: 0,
        base_bytes: 0,
    };
    for (i, a) in attrs.iter().enumerate() {
        match (a.id, &a.value) {
            (sys::SAI_ACL_COUNTER_ATTR_TABLE_ID, Value::Oid(t)) => {
                let id = AclTableId::try_from(*t)
                    .ok()
                    .filter(|id| store.get::<AclTable>(*id).is_ok());
                table = Some(id.ok_or(invalid(i))?);
            }
            (sys::SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT, Value::Bool(b)) => k.packets = *b,
            (sys::SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT, Value::Bool(b)) => k.bytes = *b,
            (sys::SAI_ACL_COUNTER_ATTR_PACKETS, Value::U64(v)) => k.base_packets = *v,
            (sys::SAI_ACL_COUNTER_ATTR_BYTES, Value::U64(v)) => k.base_bytes = *v,
            _ => return Err(not_supported(i)),
        }
    }
    k.table = table.ok_or(Error::MandatoryAttributeMissing)?;
    store.insert(k).map(TypedId::raw)
}

/// C++ `counter_read`: the base plus the SDK counter of the entry counting into it, if
/// installed.
fn counter_read<B: Backend>(backend: &B, store: &Store, id: AclCounterId) -> Result<(u64, u64)> {
    let k = store.get::<AclCounter>(id)?;
    let (mut p, mut b) = (k.base_packets, k.base_bytes);
    let installed = counter_user(store, id.raw(), None)
        .and_then(|e| store.get::<AclEntry>(e).ok())
        .and_then(|e| e.installed.as_ref());
    if let Some(installed) = installed {
        if let Ok(hit) = backend.fp_entry_counter(installed.eid) {
            p = p.wrapping_add(hit.packets);
            b = b.wrapping_add(hit.bytes);
        }
    }
    Ok((p, b))
}

/// C++ `set_acl_counter_attribute`: `PACKETS` / `BYTES` = make the counter read this value from
/// now on.
fn set_counter<B: Backend>(backend: &B, store: &mut Store, raw: RawOid, attr: &Attr) -> Result<()> {
    let id = AclCounterId::try_from(raw)?;
    store.get::<AclCounter>(id)?;
    let Value::U64(value) = attr.value else {
        return Err(invalid(0));
    };
    if attr.id != sys::SAI_ACL_COUNTER_ATTR_PACKETS && attr.id != sys::SAI_ACL_COUNTER_ATTR_BYTES {
        return Err(invalid(0));
    }
    let (p, b) = counter_read(backend, store, id)?;
    store.update::<AclCounter, _>(id, |k| {
        if attr.id == sys::SAI_ACL_COUNTER_ATTR_PACKETS {
            k.base_packets = k.base_packets.wrapping_add(value.wrapping_sub(p));
        } else {
            k.base_bytes = k.base_bytes.wrapping_add(value.wrapping_sub(b));
        }
    })
}

/// One attribute of C++ `get_acl_counter_attribute`, with the call's counter read `(packets,
/// bytes)` ([`counter_read`], once per call).
fn get_counter(store: &Store, raw: RawOid, attr: sys::sai_attr_id_t, (packets, bytes): (u64, u64)) -> Result<Value> {
    let id = AclCounterId::try_from(raw)?;
    let k = store.get::<AclCounter>(id)?;
    Ok(match attr {
        sys::SAI_ACL_COUNTER_ATTR_TABLE_ID => Value::Oid(k.table.raw()),
        sys::SAI_ACL_COUNTER_ATTR_ENABLE_PACKET_COUNT => Value::Bool(k.packets),
        sys::SAI_ACL_COUNTER_ATTR_ENABLE_BYTE_COUNT => Value::Bool(k.bytes),
        sys::SAI_ACL_COUNTER_ATTR_PACKETS => Value::U64(packets),
        sys::SAI_ACL_COUNTER_ATTR_BYTES => Value::U64(bytes),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ ranges

/// C++ `create_acl_range`: only with [`FpCap::Ranges`]; `TYPE` (L4 source / destination port)
/// and `LIMIT` (min <= max <= 65535) mandatory.
fn create_range<B: Backend>(backend: &B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    if !backend.fp_supported(FpCap::Ranges) {
        return Err(Error::NotSupported);
    }
    let (mut kind, mut limit) = (None, None);
    for (i, a) in attrs.iter().enumerate() {
        match (a.id, &a.value) {
            (sys::SAI_ACL_RANGE_ATTR_TYPE, Value::Enum(t)) => {
                kind = Some(match *t {
                    t if t == RangeKind::L4Src.sai() => RangeKind::L4Src,
                    t if t == RangeKind::L4Dst.sai() => RangeKind::L4Dst,
                    _ => return Err(not_supported(i)),
                });
            }
            (sys::SAI_ACL_RANGE_ATTR_LIMIT, Value::U32Range { min, max }) => {
                if min > max || *max > 0xffff {
                    return Err(invalid(i));
                }
                limit = Some((*min, *max));
            }
            _ => return Err(not_supported(i)),
        }
    }
    let (Some(kind), Some((min, max))) = (kind, limit) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    store.insert(AclRange { kind, min, max }).map(TypedId::raw)
}

fn get_range(store: &Store, raw: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let r = store.get::<AclRange>(AclRangeId::try_from(raw)?)?;
    Ok(match attr {
        sys::SAI_ACL_RANGE_ATTR_TYPE => Value::Enum(r.kind.sai()),
        sys::SAI_ACL_RANGE_ATTR_LIMIT => Value::U32Range { min: r.min, max: r.max },
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ groups

/// C++ `create_acl_table_group`: `ACL_STAGE` mandatory, type `PARALLEL` only, the bind point
/// list kept unchecked (portsorch passes an object list there, which a big-endian sairedis reads
/// as {0} = PORT for every bind type).
fn create_group<B: Backend>(backend: &B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let (mut stage, mut bind_types) = (None, Vec::new());
    for (i, a) in attrs.iter().enumerate() {
        match a.id {
            sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE => {
                stage = Some(stage_of(backend, enum_of(&a.value)?).ok_or(not_supported(i))?);
            }
            sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST => bind_types = i32_list(&a.value)?,
            sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE => {
                if enum_of(&a.value)? != e(sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL) {
                    return Err(not_supported(i));
                }
            }
            _ => return Err(not_supported(i)),
        }
    }
    let stage = stage.ok_or(Error::MandatoryAttributeMissing)?;
    store.insert(AclGroup { stage, bind_types }).map(TypedId::raw)
}

fn get_group(store: &Store, raw: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let id = AclTableGroupId::try_from(raw)?;
    let g = store.get::<AclGroup>(id)?;
    Ok(match attr {
        sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_STAGE => Value::Enum(g.stage.sai()),
        sys::SAI_ACL_TABLE_GROUP_ATTR_ACL_BIND_POINT_TYPE_LIST => Value::I32List(g.bind_types.clone()),
        sys::SAI_ACL_TABLE_GROUP_ATTR_TYPE => Value::Enum(e(sys::SAI_ACL_TABLE_GROUP_TYPE_PARALLEL)),
        sys::SAI_ACL_TABLE_GROUP_ATTR_MEMBER_LIST => Value::OidList(
            store
                .iter::<AclGroupMember>()
                .filter(|(_, m)| m.group == id)
                .map(|(m, _)| m.raw())
                .collect(),
        ),
        _ => return Err(not_supported(0)),
    })
}

/// C++ `create_acl_table_group_member`: group and (ASIC) table mandatory and live, one member
/// per (group, table); the entries are then re-programmed.
fn create_member<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let (mut group, mut table, mut priority) = (None, None, 0);
    for (i, a) in attrs.iter().enumerate() {
        match (a.id, &a.value) {
            (sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID, Value::Oid(g)) => {
                let id = AclTableGroupId::try_from(*g)
                    .ok()
                    .filter(|id| store.get::<AclGroup>(*id).is_ok());
                group = Some(id.ok_or(invalid(i))?);
            }
            (sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID, Value::Oid(t)) => {
                let id = AclTableId::try_from(*t)
                    .ok()
                    .filter(|id| store.get::<AclTable>(*id).is_ok());
                table = Some(id.ok_or(invalid(i))?);
            }
            (sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY, Value::U32(p)) => priority = *p,
            _ => return Err(not_supported(i)),
        }
    }
    let (Some(group), Some(table)) = (group, table) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    if store
        .iter::<AclGroupMember>()
        .any(|(_, m)| m.group == group && m.table == table)
    {
        return Err(Error::ItemAlreadyExists);
    }
    let id = store.insert(AclGroupMember { group, table, priority })?;
    acl_reprogram(backend, store);
    Ok(id.raw())
}

fn get_member(store: &Store, raw: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let m = store.get::<AclGroupMember>(AclTableGroupMemberId::try_from(raw)?)?;
    Ok(match attr {
        sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_GROUP_ID => Value::Oid(m.group.raw()),
        sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_ACL_TABLE_ID => Value::Oid(m.table.raw()),
        sys::SAI_ACL_TABLE_GROUP_MEMBER_ATTR_PRIORITY => Value::U32(m.priority),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ test port

/// The backend id of an ACL object or policer (test port `backend_id`): `ACL_TABLE` its field
/// group, `ACL_ENTRY` its field entry (`None` while not installed), `POLICER` its meter.
pub fn acl_backend_id(store: &Store, raw: RawOid) -> Option<u32> {
    match raw.type_bits() {
        sys::SAI_OBJECT_TYPE_ACL_TABLE => get_typed::<AclTable>(store, raw).map(|t| t.gid.0),
        sys::SAI_OBJECT_TYPE_ACL_ENTRY => get_typed::<AclEntry>(store, raw)
            .and_then(|e| e.installed.as_ref())
            .map(|i| i.eid.0),
        sys::SAI_OBJECT_TYPE_POLICER => crate::policer::policer(store, raw).map(|p| p.pid.0),
        _ => None,
    }
}

/// The test port's `backend_id` for `POLICER`, `ACL_TABLE`, `ACL_ENTRY` ([`crate::testing`]).
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_backend_id<B: Backend>(
    sai: &crate::sai::Sai<B>,
    oid: RawOid,
) -> crate::testing::AreaAnswer<Option<crate::testing::BackendId>> {
    use crate::backend::types::PolicerId;
    use crate::testing::BackendId;
    let Some(store) = sai.store() else { return Ok(None) };
    let id = acl_backend_id(store, oid);
    Ok(match oid.type_bits() {
        sys::SAI_OBJECT_TYPE_ACL_TABLE => id.map(|g| BackendId::FpGroup(FpGroupId(g))),
        sys::SAI_OBJECT_TYPE_ACL_ENTRY => id.map(|e| BackendId::FpEntry(FpEntryId(e))),
        sys::SAI_OBJECT_TYPE_POLICER => id.map(|p| BackendId::Policer(PolicerId(p))),
        _ => None,
    })
}

/// The test port's `object_count` for `ACL_TABLE` / `ACL_ENTRY`: the ASIC ones (C++
/// `acl_tables.size()` / `acl_entries.size()`; `MACsec` tables and entries are the `MACsec`
/// model's).
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_object_count<B: Backend>(
    sai: &crate::sai::Sai<B>,
    object_type: ObjectType,
) -> crate::testing::AreaAnswer<usize> {
    let Some(store) = sai.store() else { return Ok(0) };
    Ok(match object_type {
        ObjectType::AclTable => store.iter::<AclTable>().count(),
        ObjectType::AclEntry => store.iter::<AclEntry>().count(),
        _ => 0,
    })
}

/// The test port's `acl_binding_count`.
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_acl_binding_count<B: Backend>(sai: &crate::sai::Sai<B>) -> crate::testing::AreaAnswer<usize> {
    Ok(sai.store().map_or(0, acl_binding_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_map_onto_qualifiers_as_the_cpp() {
        assert_eq!(
            field_qual(sys::SAI_ACL_TABLE_ATTR_FIELD_IPV6_NEXT_HEADER),
            Some(FpQual::IpProtocol)
        );
        assert_eq!(field_qual(sys::SAI_ACL_TABLE_ATTR_FIELD_ECN), Some(FpQual::Tos));
        assert_eq!(field_qual(sys::SAI_ACL_TABLE_ATTR_FIELD_IN_PORT), Some(FpQual::InPorts));
        assert_eq!(field_qual(sys::SAI_ACL_TABLE_ATTR_FIELD_OUTER_VLAN_PRI), None);
        assert_eq!(
            entry_to_table_field(sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_IP),
            sys::SAI_ACL_TABLE_ATTR_FIELD_DST_IP
        );
        assert!(is_entry_action(sys::SAI_ACL_ENTRY_ATTR_ACTION_COUNTER));
        assert!(!is_entry_field(sys::SAI_ACL_ENTRY_ATTR_PRIORITY));
    }
}

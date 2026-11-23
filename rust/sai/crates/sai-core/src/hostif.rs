//! Host interfaces and traps: KNET netdevs with their RX filter, generic netlink, trap groups,
//! 22 SAI trap types on 10 [`TrapKind`]s (every other type is [`TrapKind::Other`]), netif
//! carrier from `OPER_STATUS`, the `CoPP` refresh on every change ([`crate::copp`]). Ports `sai_hostif.cpp` (543 lines; a backend with
//! `trap_program` owns a trap fully, else `trap_set` + `CoPP` field entries).
//!
//! - `HOSTIF` of type NETDEV: one KNET netif on a front-panel port (`netif_create` with the
//!   port's PVID and the switch's MAC; RX keeps the VLAN tag unless `VLAN_TAG` is `STRIP`), its
//!   carrier set from `OPER_STATUS` at once and on every change (`netif_carrier_set`); a
//!   `VLAN_TAG` change re-creates the RX filter (`netif_set_keep_tag`). Type GENETLINK: a generic
//!   netlink family (`genetlink_create`), only with a backend that mirrors (sFlow to `psample`).
//!   `QUEUE` is kept.
//! - `HOSTIF_TRAP_GROUP`: `ADMIN_STATE`, `QUEUE`, `POLICER`, all kept; what they mean reaches the
//!   chip through the traps (`trap_program`) and the `CoPP` entries. The default group comes with
//!   the switch (`default_trap_group`).
//! - `HOSTIF_TRAP`: one per trap type. Programmed with `trap_program` when the backend has it
//!   ([`Programming::Full`]), else with `trap_set` on the type's [`TrapKind`]
//!   ([`Programming::Control`]); a kind the backend has no control for is kept only
//!   ([`Programming::Kept`]).
//! - `HOSTIF_TABLE_ENTRY`: kept (the KNET per-port filters already deliver what `SONiC`'s wildcard
//!   entry asks for); channel GENETLINK sends a trap's packets to a genetlink host interface
//!   (`genetlink_trap_set`).
//! - `HOSTIF_USER_DEFINED_TRAP`: `NOT_SUPPORTED`.
//!
//! Backend: [`crate::backend::Hostif`] (and [`crate::backend::Mirror::mirror_supported`]).
//! Other areas: `crate::copp::copp_refresh` after every change of a trap or trap group (as the
//! C++ calls it); [`traps`] and `trap_spec` for it; the policer's backend state from
//! `crate::acl::trap_policer`.
//!
//! Registration: [`crate::dispatch::Owner::Hostif`] for `HOSTIF`, `HOSTIF_TRAP_GROUP`,
//! `HOSTIF_TRAP`, `HOSTIF_USER_DEFINED_TRAP`, `HOSTIF_TABLE_ENTRY` in
//! [`crate::dispatch::OWNERS`]; the objects are [`HostifObject`]s in the store
//! ([`crate::model::Object::Hostif`]).
//!
//! Where this differs from the C++ (on purpose or by the Rust design), each also in the
//! function's comment: a failed get writes nothing; [`Programming`] is one state where the C++
//! has two flags. (An attribute a call may not carry gets the C++ status: [`Area::refused_set`],
//! [`crate::dispatch::refused_create`].)
use crate::attrs::CreateArgs;
use crate::backend::Backend;
use crate::backend::types::{FilterId, GenlId, MacAddr, NetifId, TrapKind, TrapSpec};
use crate::dispatch::{Area, Cx};
use crate::error::{AttrError, Error, Result};
use crate::model::{Kind, Object, Port, PortKind, Switch};
use crate::oid::{HostifId, HostifTableEntryId, HostifTrapGroupId, HostifTrapId, ObjectType, PortId, RawOid, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;

/// The area's entry in the dispatch table ([`crate::dispatch::Owner`]).
#[derive(Clone, Copy, Debug)]
pub struct HostifArea;

/// Longest netdev name (`IFNAMSIZ` - 1).
const NETDEV_NAME_MAX: usize = 15;
/// What the C++ copies out of `NAME` and `GENETLINK_MCGRP_NAME` (`SAI_HOSTIF_NAME_SIZE`,
/// `SAI_HOSTIF_GENETLINK_MCGRP_NAME_SIZE`: 16 bytes, the rest of the 32-byte field is dropped).
const NAME_SIZE: usize = 16;

const fn e(enumerator: u32) -> i32 {
    enumerator.cast_signed()
}

// ------------------------------------------------------------------ trap types

/// A `sai_hostif_trap_type_t` (checked against the headers by the metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrapType(pub i32);

impl TrapType {
    /// The protocol class a backend may steer (C++ `trap_kind`).
    pub fn kind(self) -> TrapKind {
        trap_kind(self.0)
    }
}

/// The [`TrapKind`] of a SAI trap type (C++ `trap_kind`, `sai_hostif.cpp`:10): 22 types on 10
/// kinds, everything else [`TrapKind::Other`].
pub fn trap_kind(trap_type: i32) -> TrapKind {
    let Ok(t) = u32::try_from(trap_type) else {
        return TrapKind::Other;
    };
    match t {
        sys::SAI_HOSTIF_TRAP_TYPE_ARP_REQUEST => TrapKind::ArpRequest,
        sys::SAI_HOSTIF_TRAP_TYPE_ARP_RESPONSE => TrapKind::ArpResponse,
        sys::SAI_HOSTIF_TRAP_TYPE_DHCP | sys::SAI_HOSTIF_TRAP_TYPE_DHCP_L2 => TrapKind::Dhcp,
        sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6 | sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2 => TrapKind::Dhcpv6,
        sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_QUERY
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_LEAVE
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V1_REPORT
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V2_REPORT
        | sys::SAI_HOSTIF_TRAP_TYPE_IGMP_TYPE_V3_REPORT => TrapKind::Igmp,
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_V2
        | sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_REPORT
        | sys::SAI_HOSTIF_TRAP_TYPE_IPV6_MLD_V1_DONE
        | sys::SAI_HOSTIF_TRAP_TYPE_MLD_V2_REPORT => TrapKind::Mld,
        sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_DISCOVERY
        | sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_SOLICITATION
        | sys::SAI_HOSTIF_TRAP_TYPE_IPV6_NEIGHBOR_ADVERTISEMENT => TrapKind::Nd,
        sys::SAI_HOSTIF_TRAP_TYPE_LLDP => TrapKind::Lldp,
        sys::SAI_HOSTIF_TRAP_TYPE_LACP => TrapKind::Lacp,
        sys::SAI_HOSTIF_TRAP_TYPE_STP | sys::SAI_HOSTIF_TRAP_TYPE_PVRST => TrapKind::Stp,
        _ => TrapKind::Other,
    }
}

/// A `sai_packet_action_t` (checked against the headers by the metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PacketAction(pub i32);

impl PacketAction {
    /// The packet reaches the CPU: `TRAP`, `COPY` or `LOG` (C++ `action_to_cpu`).
    pub const fn to_cpu(self) -> bool {
        self.0 == e(sys::SAI_PACKET_ACTION_TRAP)
            || self.0 == e(sys::SAI_PACKET_ACTION_COPY)
            || self.0 == e(sys::SAI_PACKET_ACTION_LOG)
    }
}

// ------------------------------------------------------------------ the objects

/// `SAI_HOSTIF_ATTR_VLAN_TAG`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VlanTag {
    /// `STRIP` (the default): the RX filter strips the tag.
    #[default]
    Strip,
    /// `KEEP`
    Keep,
    /// `ORIGINAL`
    Original,
}

impl VlanTag {
    /// The enumerator's meaning; `None` for another value.
    pub fn from_sai(value: i32) -> Option<Self> {
        match u32::try_from(value).ok()? {
            sys::SAI_HOSTIF_VLAN_TAG_STRIP => Some(Self::Strip),
            sys::SAI_HOSTIF_VLAN_TAG_KEEP => Some(Self::Keep),
            sys::SAI_HOSTIF_VLAN_TAG_ORIGINAL => Some(Self::Original),
            _ => None,
        }
    }

    /// The enumerator.
    pub const fn sai(self) -> i32 {
        match self {
            Self::Strip => e(sys::SAI_HOSTIF_VLAN_TAG_STRIP),
            Self::Keep => e(sys::SAI_HOSTIF_VLAN_TAG_KEEP),
            Self::Original => e(sys::SAI_HOSTIF_VLAN_TAG_ORIGINAL),
        }
    }

    /// RX keeps the tag (anything but `STRIP`, as the C++).
    pub const fn keeps_tag(self) -> bool {
        !matches!(self, Self::Strip)
    }
}

/// What a host interface is on the chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostifChip {
    /// Type NETDEV: a KNET netif on a front-panel port with its RX filter.
    Netdev {
        /// The port (`OBJ_ID`).
        port: PortId,
        /// The backend's netif.
        netif: NetifId,
        /// Its current RX filter.
        filter: FilterId,
    },
    /// Type GENETLINK: a generic-netlink family.
    Genetlink {
        /// The backend's id.
        genl: GenlId,
    },
}

/// A host interface (C++ `HostifObj`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostInterface {
    /// `NAME` (at most 16 bytes, as the C++ copies it).
    pub name: String,
    /// `OBJ_ID` as given (NETDEV: the port; GENETLINK: whatever came, null by default).
    pub obj: RawOid,
    /// `GENETLINK_MCGRP_NAME` as given (at most 16 bytes; empty by default).
    pub mcgrp: String,
    /// `OPER_STATUS`: the netdev's carrier.
    pub oper_up: bool,
    /// `VLAN_TAG`
    pub vlan_tag: VlanTag,
    /// `QUEUE` (kept).
    pub queue: u32,
    /// The backend's side.
    pub chip: HostifChip,
}

/// A trap group (C++ `TrapGroupObj`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrapGroup {
    /// `ADMIN_STATE` (default true).
    pub admin: bool,
    /// `QUEUE`: the CPU queue.
    pub queue: u32,
    /// `POLICER`: `CoPP` for the group's traps.
    pub policer: Option<RawOid>,
}

impl Default for TrapGroup {
    fn default() -> Self {
        Self {
            admin: true,
            queue: 0,
            policer: None,
        }
    }
}

/// How a trap reached the chip (C++ `TrapObj::programmed` / `full`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Programming {
    /// Kept only: the backend has no control for its kind (`programmed` false).
    #[default]
    Kept,
    /// `trap_set` steers its kind (`programmed`); `CoPP` entries police it.
    Control,
    /// The backend's `trap_program` owns it, queue and policer included (`full`): no `CoPP`
    /// entries. Once full, always full (the C++ never clears `full`; its state "full but not
    /// programmed", reachable only with a backend whose `trap_program` answer changes, is this).
    Full,
}

/// A trap (C++ `TrapObj`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trap {
    /// `TRAP_TYPE`
    pub trap_type: TrapType,
    /// `PACKET_ACTION`
    pub action: PacketAction,
    /// `TRAP_PRIORITY`
    pub priority: u32,
    /// `TRAP_GROUP` (the default group when none or null was given).
    pub group: HostifTrapGroupId,
    /// What the backend did with it.
    pub programming: Programming,
}

/// `SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE` (checked against the headers by the metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableEntryType(pub i32);

/// `SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE` (checked against the headers by the metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelType(pub i32);

impl ChannelType {
    /// `GENETLINK`: the trap's packets go to a genetlink host interface.
    pub const fn is_genetlink(self) -> bool {
        self.0 == e(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_GENETLINK)
    }
}

/// A host interface table entry (C++ `HostifTableEntryObj`). The ids are kept as given, not
/// held: the C++ checks only that no entry names a host interface that is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableEntry {
    /// `TYPE`
    pub entry_type: TableEntryType,
    /// `OBJ_ID`
    pub obj: RawOid,
    /// `TRAP_ID`
    pub trap: RawOid,
    /// `CHANNEL_TYPE`
    pub channel: ChannelType,
    /// `HOST_IF`
    pub hostif: RawOid,
}

/// The objects of this area as the store holds them ([`crate::model::Object::Hostif`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostifObject {
    /// `HOSTIF`
    HostInterface(HostInterface),
    /// `HOSTIF_TRAP_GROUP`
    TrapGroup(TrapGroup),
    /// `HOSTIF_TRAP`
    Trap(Trap),
    /// `HOSTIF_TABLE_ENTRY`
    TableEntry(TableEntry),
}

impl HostifObject {
    /// The object's type.
    pub const fn object_type(&self) -> ObjectType {
        match self {
            Self::HostInterface(_) => ObjectType::Hostif,
            Self::TrapGroup(_) => ObjectType::HostifTrapGroup,
            Self::Trap(_) => ObjectType::HostifTrap,
            Self::TableEntry(_) => ObjectType::HostifTableEntry,
        }
    }

    /// What the store keeps alive for it: a trap its group (a group in use cannot go, as in the
    /// C++), a trap group its policer (the C++ `policer_in_use`).
    pub fn references(&self) -> Vec<RawOid> {
        match self {
            Self::Trap(t) => vec![t.group.raw()],
            Self::TrapGroup(g) => g.policer.into_iter().collect(),
            Self::HostInterface(_) | Self::TableEntry(_) => Vec::new(),
        }
    }
}

macro_rules! hostif_kinds {
    ($($ty:ident: $id:ty;)*) => {$(
        impl Kind for $ty {
            type Id = $id;
            fn wrap(self) -> Object {
                Object::Hostif(HostifObject::$ty(self))
            }
            fn peel(object: &Object) -> Option<&Self> {
                if let Object::Hostif(HostifObject::$ty(o)) = object { Some(o) } else { None }
            }
            fn peel_mut(object: &mut Object) -> Option<&mut Self> {
                if let Object::Hostif(HostifObject::$ty(o)) = object { Some(o) } else { None }
            }
            fn unwrap(object: Object) -> Option<Self> {
                if let Object::Hostif(HostifObject::$ty(o)) = object { Some(o) } else { None }
            }
        }
    )*};
}

hostif_kinds! {
    HostInterface: HostifId;
    TrapGroup: HostifTrapGroupId;
    Trap: HostifTrapId;
    TableEntry: HostifTableEntryId;
}

// ------------------------------------------------------------------ for other areas and tests

/// The switch's default trap group, for [`crate::sai`]'s switch creation (C++ `sai_switch.cpp`:295:
/// `make_oid(HOSTIF_TRAP_GROUP, 1)`, admin up, queue 0, no policer).
pub(crate) fn default_trap_group(store: &mut Store) -> Result<RawOid> {
    store.insert(TrapGroup::default()).map(TypedId::raw)
}

/// Every trap, in id order (C++ `ctx().traps`; `copp_refresh` walks them).
pub fn traps(store: &Store) -> impl Iterator<Item = (HostifTrapId, &Trap)> {
    store.iter::<Trap>()
}

/// A trap with its group and the group's policer (C++ `trap_spec`, `sai_hostif.cpp`:286): what
/// `trap_program` gets.
pub(crate) fn trap_spec(store: &Store, trap: &Trap) -> TrapSpec {
    let mut spec = TrapSpec {
        trap_type: trap.trap_type.0,
        action: trap.action.0,
        priority: trap.priority,
        queue: 0,
        group_up: true,
        policer: None,
        yellow_drop: false,
        red_drop: false,
    };
    if let Ok(group) = store.get::<TrapGroup>(trap.group) {
        spec.queue = group.queue;
        spec.group_up = group.admin;
        if let Some(policer) = group.policer.and_then(|p| crate::acl::trap_policer(store, p)) {
            spec.policer = Some(policer.pid);
            spec.yellow_drop = policer.yellow_drop;
            spec.red_drop = policer.red_drop;
        }
    }
    spec
}

/// The backend's id of a host interface: its netif (NETDEV) or genetlink id (the test port's
/// `backend_id` for `HOSTIF`). `None` for another object.
pub fn hostif_backend_id(store: &Store, id: RawOid) -> Option<u32> {
    let hostif = store.get::<HostInterface>(HostifId::try_from(id).ok()?).ok()?;
    Some(match hostif.chip {
        HostifChip::Netdev { netif, .. } => netif.get(),
        HostifChip::Genetlink { genl } => genl.get(),
    })
}

/// Whether a trap is programmed in the backend (the test port's `trap_programmed`; false: kept
/// only, or not a trap).
pub fn trap_programmed(store: &Store, id: RawOid) -> bool {
    HostifTrapId::try_from(id)
        .ok()
        .and_then(|id| store.get::<Trap>(id).ok())
        .is_some_and(|t| t.programming != Programming::Kept)
}

/// Destroys the netdevs of the host interfaces at switch removal (C++ `switch_clear_objects`,
/// `sai_switch.cpp`:336: `netif_destroy` for every host interface with a netif, in id order,
/// answers ignored; genetlink families are left to the backend's shutdown).
pub(crate) fn hostif_netifs_destroy<B: Backend + ?Sized>(backend: &mut B, store: &Store) {
    for (_, hostif) in store.iter::<HostInterface>() {
        if let HostifChip::Netdev { netif, filter, .. } = hostif.chip {
            let _ = backend.netif_destroy(netif, filter);
        }
    }
}

// ------------------------------------------------------------------ dispatch

impl Area for HostifArea {
    /// C++ `set_hostif_attribute`, `set_hostif_trap_group_attribute`, `set_hostif_trap_attribute`:
    /// `attr_not_supported(0)`; `set_hostif_table_entry_attribute`: `attr_invalid(0)` (all
    /// create-only), `sai_hostif.cpp:145`-490. The user-defined trap has no C++ set function (a null
    /// table entry): the metadata's refusal.
    fn refused_set(object_type: ObjectType, attr: sys::sai_attr_id_t) -> Option<Error> {
        match object_type {
            ObjectType::HostifTableEntry => Some(Error::attr(AttrError::InvalidValue, 0)),
            ObjectType::HostifUserDefinedTrap => None,
            _ => Some(crate::dispatch::cpp_refusal(attr, &[])),
        }
    }

    fn create<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, attrs: &[Attr]) -> Result<RawOid> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Hostif => create_hostif(backend, store, attrs),
            ObjectType::HostifTrapGroup => create_trap_group(store, attrs),
            ObjectType::HostifTrap => create_trap(backend, store, attrs),
            ObjectType::HostifTableEntry => create_table_entry(backend, store, attrs),
            // C++ create_user_trap; no user-defined trap can exist, so nothing else is reached
            _ => Err(Error::NotSupported),
        }
    }

    fn remove<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid) -> Result<()> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Hostif => remove_hostif(backend, store, HostifId::try_from(id)?),
            ObjectType::HostifTrapGroup => store.remove::<TrapGroup>(HostifTrapGroupId::try_from(id)?).map(drop),
            ObjectType::HostifTrap => remove_trap(backend, store, HostifTrapId::try_from(id)?),
            ObjectType::HostifTableEntry => remove_table_entry(backend, store, HostifTableEntryId::try_from(id)?),
            _ => Err(Error::InvalidObjectId),
        }
    }

    fn set<B: Backend>(cx: Cx<'_, B>, object_type: ObjectType, id: RawOid, attr: &Attr) -> Result<()> {
        let Cx { backend, store, .. } = cx;
        match object_type {
            ObjectType::Hostif => set_hostif(backend, store, HostifId::try_from(id)?, attr),
            ObjectType::HostifTrapGroup => set_trap_group(backend, store, HostifTrapGroupId::try_from(id)?, attr),
            ObjectType::HostifTrap => set_trap(backend, store, HostifTrapId::try_from(id)?, attr),
            // C++ set_hostif_table_entry_attribute: INVALID_ATTR_VALUE_0 for anything (every
            // attribute is create-only: refused before this with the same status, `refused_set`).
            ObjectType::HostifTableEntry => Err(Error::attr(AttrError::InvalidValue, 0)),
            _ => Err(Error::InvalidObjectId),
        }
    }

    fn get<B: Backend>(
        backend: &B,
        store: &Store,
        object_type: ObjectType,
        id: RawOid,
        meta: &AttrMeta,
    ) -> Result<Value> {
        let _ = backend;
        match object_type {
            ObjectType::Hostif => get_hostif(store.get::<HostInterface>(HostifId::try_from(id)?)?, meta.id),
            ObjectType::HostifTrapGroup => {
                get_trap_group(store.get::<TrapGroup>(HostifTrapGroupId::try_from(id)?)?, meta.id)
            }
            ObjectType::HostifTrap => get_trap(store.get::<Trap>(HostifTrapId::try_from(id)?)?, meta.id),
            ObjectType::HostifTableEntry => {
                get_table_entry(store.get::<TableEntry>(HostifTableEntryId::try_from(id)?)?, meta.id)
            }
            _ => Err(Error::InvalidObjectId),
        }
    }
}

fn not_supported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

fn invalid_value(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

/// The first `NAME_SIZE` bytes of a name (on a character boundary), as the C++ copies it.
fn c_name(text: &str) -> String {
    let mut end = text.len().min(NAME_SIZE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

fn switch_mac(store: &Store) -> Result<MacAddr> {
    store
        .iter::<Switch>()
        .next()
        .map(|(_, s)| MacAddr(s.src_mac))
        .ok_or(Error::Failure)
}

fn default_group(store: &Store) -> Result<HostifTrapGroupId> {
    let switch = store.iter::<Switch>().next().map(|(_, s)| s).ok_or(Error::Failure)?;
    let raw = switch
        .objects
        .get(&sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP)
        .ok_or(Error::Failure)?;
    HostifTrapGroupId::try_from(*raw)
}

/// A trap group named by `TRAP_GROUP`: null is the default group, another id must be a group
/// (`attr_invalid(index)` otherwise).
fn named_group(store: &Store, raw: RawOid, index: usize) -> Result<HostifTrapGroupId> {
    if raw.is_null() {
        return default_group(store);
    }
    let group = HostifTrapGroupId::try_from(raw).map_err(|_| invalid_value(index))?;
    store.get::<TrapGroup>(group).map_err(|_| invalid_value(index))?;
    Ok(group)
}

fn copp_refresh<B: Backend>(backend: &mut B, store: &mut Store) {
    crate::copp::copp_refresh(backend, store);
}

// ------------------------------------------------------------------ HOSTIF

/// The attributes of a host interface create (C++ `create_hostif`'s loop).
struct HostifArgs {
    /// `TYPE` and its index.
    kind: Option<(i32, usize)>,
    name: Option<String>,
    obj: RawOid,
    mcgrp: String,
    oper_up: bool,
    vlan_tag: VlanTag,
    queue: u32,
}

impl HostifArgs {
    fn parse(attrs: &[Attr]) -> Result<Self> {
        CreateArgs::check(sys::SAI_OBJECT_TYPE_HOSTIF, attrs)?;
        let mut args = Self {
            kind: None,
            name: None,
            obj: RawOid::NULL,
            mcgrp: String::new(),
            oper_up: false,
            vlan_tag: VlanTag::Strip,
            queue: 0,
        };
        for (index, attr) in attrs.iter().enumerate() {
            match (attr.id, &attr.value) {
                (sys::SAI_HOSTIF_ATTR_TYPE, Value::Enum(t)) => {
                    if *t != e(sys::SAI_HOSTIF_TYPE_NETDEV) && *t != e(sys::SAI_HOSTIF_TYPE_GENETLINK) {
                        return Err(not_supported(index));
                    }
                    args.kind = Some((*t, index));
                }
                (sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME, Value::CharData(s)) => args.mcgrp = c_name(s),
                (sys::SAI_HOSTIF_ATTR_OBJ_ID, Value::Oid(o)) => args.obj = *o,
                (sys::SAI_HOSTIF_ATTR_NAME, Value::CharData(s)) => args.name = Some(c_name(s)),
                (sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(up)) => args.oper_up = *up,
                (sys::SAI_HOSTIF_ATTR_QUEUE, Value::U32(q)) => args.queue = *q,
                (sys::SAI_HOSTIF_ATTR_VLAN_TAG, Value::Enum(t)) => {
                    args.vlan_tag = VlanTag::from_sai(*t).ok_or(invalid_value(index))?;
                }
                _ => return Err(not_supported(index)),
            }
        }
        Ok(args)
    }

    fn into_hostif(self, name: String, chip: HostifChip) -> HostInterface {
        HostInterface {
            name,
            obj: self.obj,
            mcgrp: self.mcgrp,
            oper_up: self.oper_up,
            vlan_tag: self.vlan_tag,
            queue: self.queue,
            chip,
        }
    }
}

/// C++ `create_hostif` (`sai_hostif.cpp`:37). Check order as there: type, then (GENETLINK with a
/// name) the genetlink path, else the netdev's mandatory attributes, name length, front-panel
/// port, duplicates, the backend.
fn create_hostif<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let args = HostifArgs::parse(attrs)?;
    if let (Some((kind, type_index)), Some(name)) = (args.kind, &args.name) {
        if kind == e(sys::SAI_HOSTIF_TYPE_GENETLINK) {
            let name = name.clone();
            return create_genetlink(backend, store, args, name, type_index);
        }
    }
    let (Some(_), Some(name)) = (args.kind, args.name.clone()) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    if args.obj.is_null() {
        return Err(Error::MandatoryAttributeMissing);
    }
    if name.is_empty() || name.len() > NETDEV_NAME_MAX {
        return Err(Error::InvalidParameter);
    }
    // a front-panel PORT only (a LAG's netdev is teamd's, the CPU port has none)
    let (port, hw, pvid) = PortId::try_from(args.obj)
        .ok()
        .and_then(|id| store.get::<Port>(id).ok().map(|p| (id, p)))
        .filter(|(_, p)| matches!(p.kind, PortKind::Front { .. }))
        .map(|(id, p)| (id, p.hw, p.pvid))
        .ok_or(Error::NotSupported)?;
    if store
        .iter::<HostInterface>()
        .any(|(_, h)| h.name == name || h.obj == args.obj)
    {
        return Err(Error::ItemAlreadyExists);
    }
    let mac = switch_mac(store)?;
    let made = backend.netif_create(&name, hw, pvid, mac, args.vlan_tag.keeps_tag())?;
    // The C++ ignores the carrier call's status: the host interface exists either way.
    let _ = backend.netif_carrier_set(&name, args.oper_up);
    let chip = HostifChip::Netdev {
        port,
        netif: made.netif_id,
        filter: made.filter_id,
    };
    store
        .insert(args.into_hostif(name, chip))
        .map(TypedId::raw)
        .inspect_err(|_| {
            // not reached (nothing it names can be missing); the netif must not outlive the call
            let _ = backend.netif_destroy(made.netif_id, made.filter_id);
        })
}

/// The GENETLINK branch of C++ `create_hostif` (`sai_hostif.cpp`:78): only with a backend that
/// mirrors; name and multicast group; one per name.
fn create_genetlink<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    args: HostifArgs,
    name: String,
    type_index: usize,
) -> Result<RawOid> {
    if !backend.mirror_supported() {
        return Err(not_supported(type_index));
    }
    if name.is_empty() || args.mcgrp.is_empty() {
        return Err(Error::MandatoryAttributeMissing);
    }
    if store.iter::<HostInterface>().any(|(_, h)| h.name == name) {
        return Err(Error::ItemAlreadyExists);
    }
    let genl = match backend.genetlink_create(&name, &args.mcgrp) {
        Ok(genl) => genl,
        Err(Error::NotSupported) => return Err(not_supported(type_index)),
        Err(other) => return Err(other),
    };
    store
        .insert(args.into_hostif(name, HostifChip::Genetlink { genl }))
        .map(TypedId::raw)
        .inspect_err(|_| {
            let _ = backend.genetlink_destroy(genl);
        })
}

/// C++ `remove_hostif`: not while a table entry names it; the backend's `ITEM_NOT_FOUND` counts
/// as gone.
fn remove_hostif<B: Backend>(backend: &mut B, store: &mut Store, id: HostifId) -> Result<()> {
    let chip = store.get::<HostInterface>(id)?.chip;
    if store.iter::<TableEntry>().any(|(_, t)| t.hostif == id.raw()) {
        return Err(Error::ObjectInUse);
    }
    let gone = match chip {
        HostifChip::Genetlink { genl } => backend.genetlink_destroy(genl),
        HostifChip::Netdev { netif, filter, .. } => backend.netif_destroy(netif, filter),
    };
    match gone {
        Ok(()) | Err(Error::ItemNotFound) => store.remove::<HostInterface>(id).map(drop),
        Err(other) => Err(other),
    }
}

/// C++ `set_hostif_attribute` (`sai_hostif.cpp`:145).
fn set_hostif<B: Backend>(backend: &mut B, store: &mut Store, id: HostifId, attr: &Attr) -> Result<()> {
    let hostif = store.get::<HostInterface>(id)?.clone();
    match (attr.id, &attr.value) {
        (sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(up)) => {
            if matches!(hostif.chip, HostifChip::Netdev { .. }) {
                backend.netif_carrier_set(&hostif.name, *up)?;
            }
            store.update::<HostInterface, _>(id, |h| h.oper_up = *up)
        }
        (sys::SAI_HOSTIF_ATTR_VLAN_TAG, Value::Enum(t)) => {
            let tag = VlanTag::from_sai(*t).ok_or(invalid_value(0))?;
            // C++: the port of OBJ_ID or FAILURE (a GENETLINK host interface has no netif:
            // FAILURE here; the C++ would call netif_set_keep_tag with netif -1 if its OBJ_ID
            // named a port)
            let HostifChip::Netdev { port, netif, .. } = hostif.chip else {
                return Err(Error::Failure);
            };
            let hw = store.get::<Port>(port).map_err(|_| Error::Failure)?.hw;
            // C++ passes -1 as the old filter (sai_hostif.cpp:165), so the SDK glue never
            // destroys it: kept as it is (a finding, not fixed here)
            let filter = backend.netif_set_keep_tag(netif, hw, tag.keeps_tag(), None)?;
            store.update::<HostInterface, _>(id, |h| {
                h.vlan_tag = tag;
                if let HostifChip::Netdev { filter: f, .. } = &mut h.chip {
                    *f = filter;
                }
            })
        }
        (sys::SAI_HOSTIF_ATTR_QUEUE, Value::U32(q)) => store.update::<HostInterface, _>(id, |h| h.queue = *q),
        _ => Err(not_supported(0)),
    }
}

fn get_hostif(hostif: &HostInterface, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_HOSTIF_ATTR_TYPE => Value::Enum(match hostif.chip {
            HostifChip::Netdev { .. } => e(sys::SAI_HOSTIF_TYPE_NETDEV),
            HostifChip::Genetlink { .. } => e(sys::SAI_HOSTIF_TYPE_GENETLINK),
        }),
        sys::SAI_HOSTIF_ATTR_OBJ_ID => Value::Oid(hostif.obj),
        sys::SAI_HOSTIF_ATTR_NAME => Value::CharData(hostif.name.clone()),
        sys::SAI_HOSTIF_ATTR_OPER_STATUS => Value::Bool(hostif.oper_up),
        sys::SAI_HOSTIF_ATTR_QUEUE => Value::U32(hostif.queue),
        sys::SAI_HOSTIF_ATTR_VLAN_TAG => Value::Enum(hostif.vlan_tag.sai()),
        sys::SAI_HOSTIF_ATTR_GENETLINK_MCGRP_NAME => Value::CharData(hostif.mcgrp.clone()),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ HOSTIF_TRAP_GROUP

/// C++ `trap_group_attr`: a policer must be a live `POLICER` (`policer_valid`).
fn trap_group_attr(store: &Store, group: &mut TrapGroup, attr: &Attr, index: usize) -> Result<()> {
    match (attr.id, &attr.value) {
        (sys::SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE, Value::Bool(up)) => group.admin = *up,
        (sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE, Value::U32(q)) => group.queue = *q,
        (sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER, Value::Oid(p)) => {
            if !p.is_null() && store.object_type(*p) != Some(ObjectType::Policer) {
                return Err(invalid_value(index));
            }
            group.policer = (!p.is_null()).then_some(*p);
        }
        _ => return Err(not_supported(index)),
    }
    Ok(())
}

/// C++ `create_hostif_trap_group`: no backend call.
fn create_trap_group(store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP, attrs)?;
    let mut group = TrapGroup::default();
    for (index, attr) in attrs.iter().enumerate() {
        trap_group_attr(store, &mut group, attr, index)?;
    }
    store.insert(group).map(TypedId::raw)
}

/// C++ `set_hostif_trap_group_attribute`: the policer always refreshes `CoPP`; queue and admin
/// state only when a trap the backend owns (`full`) is in the group.
fn set_trap_group<B: Backend>(backend: &mut B, store: &mut Store, id: HostifTrapGroupId, attr: &Attr) -> Result<()> {
    let mut group = *store.get::<TrapGroup>(id)?;
    trap_group_attr(store, &mut group, attr, 0)?;
    store.update::<TrapGroup, _>(id, |g| *g = group)?;
    let refresh = attr.id == sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER
        || store
            .iter::<Trap>()
            .any(|(_, t)| t.programming == Programming::Full && t.group == id);
    if refresh {
        copp_refresh(backend, store);
    }
    Ok(())
}

fn get_trap_group(group: &TrapGroup, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_HOSTIF_TRAP_GROUP_ATTR_ADMIN_STATE => Value::Bool(group.admin),
        sys::SAI_HOSTIF_TRAP_GROUP_ATTR_QUEUE => Value::U32(group.queue),
        sys::SAI_HOSTIF_TRAP_GROUP_ATTR_POLICER => Value::Oid(group.policer.unwrap_or(RawOid::NULL)),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ HOSTIF_TRAP

/// Programs `trap` with `action` (C++ `trap_program`, `sai_hostif.cpp`:307): the backend's
/// `trap_program` when it has one (any answer but `NOT_SUPPORTED` is final), else `trap_set` on
/// the kind, where `NOT_SUPPORTED` means "kept, not programmed" and is a success. `trap` changes
/// only on success.
fn program<B: Backend>(backend: &mut B, store: &Store, trap: &mut Trap, action: PacketAction) -> Result<()> {
    let want = Trap { action, ..*trap };
    match backend.trap_program(&trap_spec(store, &want), false) {
        Ok(()) => {
            trap.programming = Programming::Full;
            return Ok(());
        }
        Err(Error::NotSupported) => {}
        Err(other) => return Err(other),
    }
    let programming = match backend.trap_set(trap.trap_type.kind(), action.to_cpu()) {
        Ok(()) => Programming::Control,
        Err(Error::NotSupported) => Programming::Kept,
        Err(other) => return Err(other),
    };
    if trap.programming != Programming::Full {
        trap.programming = programming;
    }
    Ok(())
}

/// C++ `create_hostif_trap` (`sai_hostif.cpp`:327): one trap per type; programmed before it is
/// stored, `CoPP` refreshed after.
fn create_trap<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_HOSTIF_TRAP, attrs)?;
    let mut group = default_group(store)?;
    let (mut trap_type, mut action, mut priority) = (None, None, 0);
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE, Value::Enum(t)) => trap_type = Some(TrapType(*t)),
            (sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, Value::Enum(a)) => action = Some(PacketAction(*a)),
            (sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY, Value::U32(p)) => priority = *p,
            (sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(g)) => group = named_group(store, *g, index)?,
            _ => return Err(not_supported(index)),
        }
    }
    let (Some(trap_type), Some(action)) = (trap_type, action) else {
        return Err(Error::MandatoryAttributeMissing);
    };
    if store.iter::<Trap>().any(|(_, t)| t.trap_type == trap_type) {
        return Err(Error::ItemAlreadyExists);
    }
    let mut trap = Trap {
        trap_type,
        action,
        priority,
        group,
        programming: Programming::Kept,
    };
    program(backend, store, &mut trap, action)?;
    let id = match store.insert(trap) {
        Ok(id) => id,
        Err(error) => {
            // not reached (its group exists); the chip must not keep what the model does not have
            unprogram(backend, store, &trap);
            return Err(error);
        }
    };
    copp_refresh(backend, store);
    Ok(id.raw())
}

/// What C++ `remove_hostif_trap` asks of the backend; the answers are ignored (as the C++
/// does: a failed `trap_set(kind, false)` leaves the chip steering the protocol).
fn unprogram<B: Backend>(backend: &mut B, store: &Store, trap: &Trap) {
    match trap.programming {
        Programming::Full => {
            let _ = backend.trap_program(&trap_spec(store, trap), true);
        }
        Programming::Control => {
            let _ = backend.trap_set(trap.trap_type.kind(), false);
        }
        Programming::Kept => {}
    }
}

/// C++ `remove_hostif_trap`: the backend is told (answers ignored), the trap goes, `CoPP` is
/// refreshed.
fn remove_trap<B: Backend>(backend: &mut B, store: &mut Store, id: HostifTrapId) -> Result<()> {
    let trap = *store.get::<Trap>(id)?;
    unprogram(backend, store, &trap);
    store.remove::<Trap>(id)?;
    copp_refresh(backend, store);
    Ok(())
}

/// C++ `set_hostif_trap_attribute` (`sai_hostif.cpp`:376).
fn set_trap<B: Backend>(backend: &mut B, store: &mut Store, id: HostifTrapId, attr: &Attr) -> Result<()> {
    let mut trap = *store.get::<Trap>(id)?;
    match (attr.id, &attr.value) {
        (sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION, Value::Enum(a)) => {
            let action = PacketAction(*a);
            let mut result = program(backend, store, &mut trap, action);
            if result.is_ok() {
                trap.action = action;
                result = store.update::<Trap, _>(id, |t| *t = trap);
            }
            // the C++ refreshes whatever the outcome
            copp_refresh(backend, store);
            result
        }
        (sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY, Value::U32(p)) => {
            store.update::<Trap, _>(id, |t| t.priority = *p)?;
            if trap.programming == Programming::Full {
                copp_refresh(backend, store);
            }
            Ok(())
        }
        (sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP, Value::Oid(g)) => {
            let group = named_group(store, *g, 0)?;
            store.update::<Trap, _>(id, |t| t.group = group)?;
            copp_refresh(backend, store);
            Ok(())
        }
        _ => Err(not_supported(0)),
    }
}

fn get_trap(trap: &Trap, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_HOSTIF_TRAP_ATTR_TRAP_TYPE => Value::Enum(trap.trap_type.0),
        sys::SAI_HOSTIF_TRAP_ATTR_PACKET_ACTION => Value::Enum(trap.action.0),
        sys::SAI_HOSTIF_TRAP_ATTR_TRAP_PRIORITY => Value::U32(trap.priority),
        sys::SAI_HOSTIF_TRAP_ATTR_TRAP_GROUP => Value::Oid(trap.group.raw()),
        _ => return Err(not_supported(0)),
    })
}

// ------------------------------------------------------------------ HOSTIF_TABLE_ENTRY

/// The host interface and the trap an entry names, if both exist: the host interface's
/// genetlink id (`None` for a NETDEV one) and the trap's type.
fn genetlink_target(store: &Store, entry: &TableEntry) -> Option<(Option<GenlId>, TrapType)> {
    let hostif = store
        .get::<HostInterface>(HostifId::try_from(entry.hostif).ok()?)
        .ok()?;
    let trap = store.get::<Trap>(HostifTrapId::try_from(entry.trap).ok()?).ok()?;
    let genl = match hostif.chip {
        HostifChip::Genetlink { genl } => Some(genl),
        HostifChip::Netdev { .. } => None,
    };
    Some((genl, trap.trap_type))
}

/// C++ `create_hostif_table_entry` (`sai_hostif.cpp`:424): kept, except channel GENETLINK, which
/// needs a genetlink host interface and a trap and is programmed.
fn create_table_entry<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    CreateArgs::check(sys::SAI_OBJECT_TYPE_HOSTIF_TABLE_ENTRY, attrs)?;
    let mut entry = TableEntry {
        entry_type: TableEntryType(e(sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_WILDCARD)),
        obj: RawOid::NULL,
        trap: RawOid::NULL,
        channel: ChannelType(e(sys::SAI_HOSTIF_TABLE_ENTRY_CHANNEL_TYPE_NETDEV_PHYSICAL_PORT)),
        hostif: RawOid::NULL,
    };
    let (mut has_type, mut has_channel) = (false, false);
    for (index, attr) in attrs.iter().enumerate() {
        match (attr.id, &attr.value) {
            (sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE, Value::Enum(t)) => {
                entry.entry_type = TableEntryType(*t);
                has_type = true;
            }
            (sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_OBJ_ID, Value::Oid(o)) => entry.obj = *o,
            (sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID, Value::Oid(t)) => entry.trap = *t,
            (sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE, Value::Enum(c)) => {
                entry.channel = ChannelType(*c);
                has_channel = true;
            }
            (sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF, Value::Oid(h)) => entry.hostif = *h,
            _ => return Err(not_supported(index)),
        }
    }
    if !has_type || !has_channel {
        return Err(Error::MandatoryAttributeMissing);
    }
    if entry.channel.is_genetlink() {
        let trap_id = entry.entry_type.0 == e(sys::SAI_HOSTIF_TABLE_ENTRY_TYPE_TRAP_ID);
        let Some((Some(genl), trap_type)) = genetlink_target(store, &entry).filter(|_| trap_id) else {
            return Err(Error::InvalidParameter);
        };
        backend.genetlink_trap_set(genl, trap_type.0, true)?;
        return store.insert(entry).map(TypedId::raw).inspect_err(|_| {
            let _ = backend.genetlink_trap_set(genl, trap_type.0, false);
        });
    }
    store.insert(entry).map(TypedId::raw)
}

/// C++ `remove_hostif_table_entry`: a GENETLINK entry whose host interface and trap still exist
/// stops the trap's packets there (`ITEM_NOT_FOUND` counts as stopped).
fn remove_table_entry<B: Backend>(backend: &mut B, store: &mut Store, id: HostifTableEntryId) -> Result<()> {
    let entry = *store.get::<TableEntry>(id)?;
    if entry.channel.is_genetlink() {
        // C++: h && t (a NETDEV host interface cannot be named here: create refused it)
        if let Some((Some(genl), trap_type)) = genetlink_target(store, &entry) {
            match backend.genetlink_trap_set(genl, trap_type.0, false) {
                Ok(()) | Err(Error::ItemNotFound) => {}
                Err(other) => return Err(other),
            }
        }
    }
    store.remove::<TableEntry>(id).map(drop)
}

fn get_table_entry(entry: &TableEntry, attr: sys::sai_attr_id_t) -> Result<Value> {
    Ok(match attr {
        sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TYPE => Value::Enum(entry.entry_type.0),
        sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_OBJ_ID => Value::Oid(entry.obj),
        sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_TRAP_ID => Value::Oid(entry.trap),
        sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_CHANNEL_TYPE => Value::Enum(entry.channel.0),
        sys::SAI_HOSTIF_TABLE_ENTRY_ATTR_HOST_IF => Value::Oid(entry.hostif),
        _ => return Err(not_supported(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_22_trap_types_of_the_cpp_have_a_kind_the_rest_is_other() {
        let with_kind: Vec<u32> = (0..0x1_0000)
            .filter(|t| trap_kind(i32::try_from(*t).unwrap()) != TrapKind::Other)
            .collect();
        assert_eq!(with_kind.len(), 22);
        assert_eq!(trap_kind(e(sys::SAI_HOSTIF_TRAP_TYPE_BGP)), TrapKind::Other);
        assert_eq!(trap_kind(-1), TrapKind::Other);
        assert_eq!(trap_kind(e(sys::SAI_HOSTIF_TRAP_TYPE_PVRST)), TrapKind::Stp);
        assert_eq!(trap_kind(e(sys::SAI_HOSTIF_TRAP_TYPE_DHCPV6_L2)), TrapKind::Dhcpv6);
    }

    #[test]
    fn actions_to_the_cpu_and_vlan_tags() {
        for (action, to_cpu) in [
            (sys::SAI_PACKET_ACTION_TRAP, true),
            (sys::SAI_PACKET_ACTION_COPY, true),
            (sys::SAI_PACKET_ACTION_LOG, true),
            (sys::SAI_PACKET_ACTION_DROP, false),
            (sys::SAI_PACKET_ACTION_FORWARD, false),
            (sys::SAI_PACKET_ACTION_COPY_CANCEL, false),
        ] {
            assert_eq!(PacketAction(e(action)).to_cpu(), to_cpu, "{action}");
        }
        for tag in [VlanTag::Strip, VlanTag::Keep, VlanTag::Original] {
            assert_eq!(VlanTag::from_sai(tag.sai()), Some(tag));
        }
        assert_eq!(VlanTag::from_sai(77), None);
        assert!(!VlanTag::Strip.keeps_tag() && VlanTag::Original.keeps_tag());
    }

    #[test]
    fn names_are_cut_as_the_cpp_copies_them() {
        assert_eq!(c_name("Ethernet0"), "Ethernet0");
        assert_eq!(c_name("0123456789abcdefXYZ"), "0123456789abcdef");
        // never inside a character
        assert_eq!(c_name("0123456789abcde\u{e9}"), "0123456789abcde");
    }
}

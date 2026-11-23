//! `sai_attribute_value_t` (a C union) to and from [`Value`], by the attribute's
//! [`ValueType`] from the metadata: the one place that knows which union member an
//! attribute travels in.
use crate::Status;
use crate::ptr;
use core::ffi::c_char;
use sai_core::error::{AttrError, Error};
use sai_core::macsec::Secret;
use sai_core::model::EntryKey;
use sai_core::oid::RawOid;
use sai_core::value::{
    AclAction, AclField, AclFieldData, AclKind, AclParam, AclResource, FieldMatch, IpPrefix, Pointer, QosMapEntry,
    QosParams, Value,
};
use sai_meta::ValueType;
use sai_sys as sys;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Reads one member of the union (or one field of a member).
macro_rules! member {
    ($value:ident . $($field:ident).+) => {
        // SAFETY: the contract of `decode` / `encode`: the C caller initialised this member (the
        // one the metadata names for the attribute). Every member read through this macro is
        // plain data (integers, byte arrays, a count with a raw pointer) for which each bit
        // pattern is a value.
        unsafe { $value.$($field).+ }
    };
}

const BAD_VALUE: Error = Error::Attr(AttrError::InvalidValue, 0);

/// `sai_ip4_t` is a 32-bit integer whose bytes in memory are the address in network order,
/// whatever the host's byte order: the address is the integer's memory image.
fn ip4_from(raw: sys::sai_ip4_t) -> Ipv4Addr {
    Ipv4Addr::from(raw.to_ne_bytes())
}

fn ip4_to(addr: Ipv4Addr) -> sys::sai_ip4_t {
    sys::sai_ip4_t::from_ne_bytes(addr.octets())
}

/// An address out of `sai_ip_addr_t` by its family; a family that is neither is the value's
/// fault.
///
/// # Safety
/// The member of `addr` that `family` names (`ip4` or `ip6`) is initialised.
unsafe fn ip_from(family: sys::sai_ip_addr_family_t, addr: &sys::sai_ip_addr_t) -> Result<IpAddr, Error> {
    match family {
        sys::SAI_IP_ADDR_FAMILY_IPV4 => {
            // SAFETY: this function's contract; an integer, for which every bit pattern is a value.
            let raw = unsafe { addr.ip4 };
            Ok(IpAddr::V4(ip4_from(raw)))
        }
        sys::SAI_IP_ADDR_FAMILY_IPV6 => {
            // SAFETY: this function's contract; a byte array, for which every bit pattern is a value.
            let raw = unsafe { addr.ip6 };
            Ok(IpAddr::V6(Ipv6Addr::from(raw)))
        }
        _ => Err(BAD_VALUE),
    }
}

fn ip_to(addr: IpAddr) -> (sys::sai_ip_addr_family_t, sys::sai_ip_addr_t) {
    match addr {
        IpAddr::V4(v4) => {
            // The whole union is written, so no byte of it is left as the caller had it.
            let mut raw = sys::sai_ip_addr_t { ip6: [0; 16] };
            raw.ip4 = ip4_to(v4);
            (sys::SAI_IP_ADDR_FAMILY_IPV4, raw)
        }
        IpAddr::V6(v6) => (sys::SAI_IP_ADDR_FAMILY_IPV6, sys::sai_ip_addr_t { ip6: v6.octets() }),
    }
}

/// # Safety
/// As [`ip_from`], for the address of `raw`.
unsafe fn address_from(raw: &sys::sai_ip_address_t) -> Result<IpAddr, Error> {
    // SAFETY: this function's contract is `ip_from`'s.
    unsafe { ip_from(raw.addr_family, &raw.addr) }
}

/// # Safety
/// As [`ip_from`], for the address and the mask of `raw`.
unsafe fn prefix_from(raw: &sys::sai_ip_prefix_t) -> Result<IpPrefix, Error> {
    // SAFETY: this function's contract is `ip_from`'s.
    let addr = unsafe { ip_from(raw.addr_family, &raw.addr) }?;
    // SAFETY: this function's contract is `ip_from`'s.
    let mask = unsafe { ip_from(raw.addr_family, &raw.mask) }?;
    IpPrefix::new(addr, mask).ok_or(BAD_VALUE)
}

fn prefix_to(prefix: IpPrefix) -> sys::sai_ip_prefix_t {
    let (addr_family, addr) = ip_to(prefix.addr());
    let (_, mask) = ip_to(prefix.mask());
    sys::sai_ip_prefix_t {
        addr_family,
        addr,
        mask,
    }
}

fn qos_from(raw: &sys::sai_qos_map_params_t) -> QosParams {
    QosParams {
        tc: raw.tc,
        dscp: raw.dscp,
        dot1p: raw.dot1p,
        prio: raw.prio,
        pg: raw.pg,
        queue_index: raw.queue_index,
        color: raw.color.cast_signed(),
        mpls_exp: raw.mpls_exp,
        fc: raw.fc,
        dei: raw.dei,
        vc: raw.vc,
    }
}

fn qos_to(params: &QosParams) -> sys::sai_qos_map_params_t {
    sys::sai_qos_map_params_t {
        tc: params.tc,
        dscp: params.dscp,
        dot1p: params.dot1p,
        prio: params.prio,
        pg: params.pg,
        queue_index: params.queue_index,
        color: params.color.cast_unsigned(),
        mpls_exp: params.mpls_exp,
        fc: params.fc,
        dei: params.dei,
        vc: params.vc,
    }
}

/// The key struct of an entry-keyed object type (`sai_fdb_entry_t`, ...).
pub(crate) trait EntryStruct {
    /// The SAI object type the struct is the key of.
    const OBJECT_TYPE: sys::sai_object_type_t;
    /// The switch id the key carries, and the key.
    ///
    /// # Safety
    /// The struct was filled in by the C caller: for an IP address in it, the member its
    /// address family names is initialised.
    unsafe fn key(&self) -> Result<(RawOid, EntryKey), Error>;
}

impl EntryStruct for sys::sai_fdb_entry_t {
    const OBJECT_TYPE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_FDB_ENTRY;
    unsafe fn key(&self) -> Result<(RawOid, EntryKey), Error> {
        Ok((
            RawOid(self.switch_id),
            EntryKey::Fdb {
                bv_id: RawOid(self.bv_id),
                mac: self.mac_address,
            },
        ))
    }
}

impl EntryStruct for sys::sai_neighbor_entry_t {
    const OBJECT_TYPE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY;
    unsafe fn key(&self) -> Result<(RawOid, EntryKey), Error> {
        // SAFETY: this function's contract is `address_from`'s.
        let ip = unsafe { address_from(&self.ip_address) }.map_err(|_| Error::InvalidParameter)?;
        Ok((
            RawOid(self.switch_id),
            EntryKey::Neighbor {
                rif: RawOid(self.rif_id),
                ip,
            },
        ))
    }
}

impl EntryStruct for sys::sai_route_entry_t {
    const OBJECT_TYPE: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_ROUTE_ENTRY;
    unsafe fn key(&self) -> Result<(RawOid, EntryKey), Error> {
        // SAFETY: this function's contract is `prefix_from`'s.
        let prefix = unsafe { prefix_from(&self.destination) }.map_err(|_| Error::InvalidParameter)?;
        Ok((
            RawOid(self.switch_id),
            EntryKey::Route {
                vr: RawOid(self.vr_id),
                prefix,
            },
        ))
    }
}

/// Copies a list member of an input value into a `Vec`.
macro_rules! list_in {
    ($value:ident . $($field:ident).+) => {{
        let list = member!($value.$($field).+);
        // SAFETY: the contract of `decode`: a list member's pointer names `count` initialised
        // elements unless the count is zero, which is `in_slice`'s contract.
        unsafe { ptr::in_slice(list.list.cast_const(), list.count) }
            .map(<[_]>::to_vec)
            .map_err(|_| Error::attr(AttrError::InvalidValue, 0))?
    }};
}

/// Writes a result list into the buffer a list member of an output value names.
macro_rules! list_out {
    ($slot:ident . $($field:ident).+, $items:expr) => {{
        let mut list = member!($slot.$($field).+);
        // SAFETY: the contract of `encode`: the caller set the member's count to the capacity of
        // the buffer its pointer names (or to zero), which is `write_list`'s contract.
        let outcome = unsafe { ptr::write_list(&mut list.count, list.list, $items) };
        $slot.$($field).+ = list;
        outcome
    }};
}

/// Reads a C `bool` field of a struct inside the union (an ACL field's or action's `enable`)
/// as the byte it is: a C caller may store any non-zero value, which is not a Rust `bool`.
macro_rules! flag {
    ($value:ident . $($field:ident).+) => {
        // SAFETY: the contract of `decode`: the C caller initialised the member this field is
        // part of, so the byte is initialised; it is read as a `u8`, for which every bit
        // pattern is a value.
        unsafe { (&raw const $value.$($field).+).cast::<u8>().read() } != 0
    };
}

/// Decodes an ACL entry field (`aclfield`) or action (`aclaction`) by the kind of its data.
/// Data, mask and parameter are read only when the field or action is enabled (SAI leaves
/// them undefined otherwise).
///
/// # Safety
/// As [`decode`]: the caller initialised `aclfield` (for `field`) or `aclaction`: its
/// `enable` byte and, when that is set, the data / mask / parameter members `kind` names,
/// lists with their elements.
unsafe fn acl_decode(field: bool, kind: AclKind, value: &sys::sai_attribute_value_t) -> Result<Value, Error> {
    const fn m<T>(data: T, mask: T) -> FieldMatch<T> {
        FieldMatch { data, mask }
    }
    let not_supported = Error::attr(AttrError::NotSupported, 0);
    // A member the union does not have (the metadata scanner never names one).
    let exists = if field {
        kind != AclKind::IpAddress
    } else {
        !matches!(kind, AclKind::U64 | AclKind::U8List)
    };
    if !exists {
        return Err(not_supported);
    }
    if field {
        if !flag!(value.aclfield.enable) {
            return Ok(Value::AclField(AclField::Disabled(kind)));
        }
        let data = match kind {
            AclKind::Bool => AclFieldData::Bool(member!(value.aclfield.data.u8_) != 0),
            AclKind::U8 => AclFieldData::U8(m(member!(value.aclfield.data.u8_), member!(value.aclfield.mask.u8_))),
            AclKind::U16 => AclFieldData::U16(m(member!(value.aclfield.data.u16_), member!(value.aclfield.mask.u16_))),
            AclKind::U32 => AclFieldData::U32(m(member!(value.aclfield.data.u32_), member!(value.aclfield.mask.u32_))),
            AclKind::I32 => AclFieldData::I32(m(member!(value.aclfield.data.s32), member!(value.aclfield.mask.s32))),
            AclKind::Enum => AclFieldData::Enum(m(member!(value.aclfield.data.s32), member!(value.aclfield.mask.s32))),
            AclKind::U64 => AclFieldData::U64(m(member!(value.aclfield.data.u64_), member!(value.aclfield.mask.u64_))),
            AclKind::Mac => AclFieldData::Mac(m(member!(value.aclfield.data.mac), member!(value.aclfield.mask.mac))),
            AclKind::Ip4 => AclFieldData::Ip4(m(
                ip4_from(member!(value.aclfield.data.ip4)),
                ip4_from(member!(value.aclfield.mask.ip4)),
            )),
            AclKind::Ip6 => AclFieldData::Ip6(m(
                Ipv6Addr::from(member!(value.aclfield.data.ip6)),
                Ipv6Addr::from(member!(value.aclfield.mask.ip6)),
            )),
            AclKind::Oid => AclFieldData::Oid(RawOid(member!(value.aclfield.data.oid))),
            AclKind::OidList => {
                AclFieldData::OidList(list_in!(value.aclfield.data.objlist).into_iter().map(RawOid).collect())
            }
            AclKind::U8List => AclFieldData::U8List(m(
                list_in!(value.aclfield.data.u8list),
                list_in!(value.aclfield.mask.u8list),
            )),
            AclKind::IpAddress => return Err(not_supported),
        };
        return Ok(Value::AclField(AclField::Enabled(data)));
    }
    if !flag!(value.aclaction.enable) {
        return Ok(Value::AclAction(AclAction::Disabled(kind)));
    }
    let param = match kind {
        AclKind::Bool => AclParam::Bool(member!(value.aclaction.parameter.u8_) != 0),
        AclKind::U8 => AclParam::U8(member!(value.aclaction.parameter.u8_)),
        AclKind::U16 => AclParam::U16(member!(value.aclaction.parameter.u16_)),
        AclKind::U32 => AclParam::U32(member!(value.aclaction.parameter.u32_)),
        AclKind::I32 => AclParam::I32(member!(value.aclaction.parameter.s32)),
        AclKind::Enum => AclParam::Enum(member!(value.aclaction.parameter.s32)),
        AclKind::Mac => AclParam::Mac(member!(value.aclaction.parameter.mac)),
        AclKind::Ip4 => AclParam::Ip4(ip4_from(member!(value.aclaction.parameter.ip4))),
        AclKind::Ip6 => AclParam::Ip6(Ipv6Addr::from(member!(value.aclaction.parameter.ip6))),
        AclKind::Oid => AclParam::Oid(RawOid(member!(value.aclaction.parameter.oid))),
        AclKind::OidList => AclParam::OidList(
            list_in!(value.aclaction.parameter.objlist)
                .into_iter()
                .map(RawOid)
                .collect(),
        ),
        AclKind::IpAddress => AclParam::IpAddress(match member!(value.aclaction.parameter.ipaddr.addr_family) {
            sys::SAI_IP_ADDR_FAMILY_IPV4 => IpAddr::V4(ip4_from(member!(value.aclaction.parameter.ipaddr.addr.ip4))),
            sys::SAI_IP_ADDR_FAMILY_IPV6 => {
                IpAddr::V6(Ipv6Addr::from(member!(value.aclaction.parameter.ipaddr.addr.ip6)))
            }
            _ => return Err(BAD_VALUE),
        }),
        AclKind::U64 | AclKind::U8List => return Err(not_supported),
    };
    Ok(Value::AclAction(AclAction::Enabled(param)))
}

/// Encodes an ACL entry field or action into the slot of a get, as the C++ library does: a
/// disabled one writes `enable = false` and nothing else; an enabled one writes `enable`, the
/// data and mask members (or the parameter) and fills a list into the caller's buffer.
///
/// # Safety
/// As [`encode`]: for a list (`OidList`, `U8List`) the caller initialised the list member
/// that carries it (for `U8List` the data's and the mask's): count = capacity of the buffer
/// its pointer names, or zero.
unsafe fn acl_encode(result: &Value, slot: &mut sys::sai_attribute_value_t) -> Result<(), Status> {
    match result {
        Value::AclField(AclField::Disabled(_)) => slot.aclfield.enable = false,
        Value::AclField(AclField::Enabled(data)) => {
            slot.aclfield.enable = true;
            match data {
                AclFieldData::Bool(v) => slot.aclfield.data.booldata = *v,
                AclFieldData::U8(v) => (slot.aclfield.data.u8_, slot.aclfield.mask.u8_) = (v.data, v.mask),
                AclFieldData::U16(v) => (slot.aclfield.data.u16_, slot.aclfield.mask.u16_) = (v.data, v.mask),
                AclFieldData::U32(v) => (slot.aclfield.data.u32_, slot.aclfield.mask.u32_) = (v.data, v.mask),
                AclFieldData::I32(v) | AclFieldData::Enum(v) => {
                    (slot.aclfield.data.s32, slot.aclfield.mask.s32) = (v.data, v.mask);
                }
                AclFieldData::U64(v) => (slot.aclfield.data.u64_, slot.aclfield.mask.u64_) = (v.data, v.mask),
                AclFieldData::Mac(v) => (slot.aclfield.data.mac, slot.aclfield.mask.mac) = (v.data, v.mask),
                AclFieldData::Ip4(v) => {
                    (slot.aclfield.data.ip4, slot.aclfield.mask.ip4) = (ip4_to(v.data), ip4_to(v.mask));
                }
                AclFieldData::Ip6(v) => {
                    (slot.aclfield.data.ip6, slot.aclfield.mask.ip6) = (v.data.octets(), v.mask.octets());
                }
                AclFieldData::Oid(v) => slot.aclfield.data.oid = v.0,
                AclFieldData::OidList(items) => {
                    let raw: Vec<sys::sai_object_id_t> = items.iter().map(|oid| oid.0).collect();
                    return list_out!(slot.aclfield.data.objlist, &raw);
                }
                AclFieldData::U8List(v) => {
                    let data = list_out!(slot.aclfield.data.u8list, &v.data);
                    let mask = list_out!(slot.aclfield.mask.u8list, &v.mask);
                    return data.and(mask);
                }
            }
        }
        Value::AclAction(AclAction::Disabled(_)) => slot.aclaction.enable = false,
        Value::AclAction(AclAction::Enabled(param)) => {
            slot.aclaction.enable = true;
            match param {
                AclParam::Bool(v) => slot.aclaction.parameter.booldata = *v,
                AclParam::U8(v) => slot.aclaction.parameter.u8_ = *v,
                AclParam::U16(v) => slot.aclaction.parameter.u16_ = *v,
                AclParam::U32(v) => slot.aclaction.parameter.u32_ = *v,
                AclParam::I32(v) | AclParam::Enum(v) => slot.aclaction.parameter.s32 = *v,
                AclParam::Mac(v) => slot.aclaction.parameter.mac = *v,
                AclParam::Ip4(v) => slot.aclaction.parameter.ip4 = ip4_to(*v),
                AclParam::Ip6(v) => slot.aclaction.parameter.ip6 = v.octets(),
                AclParam::Oid(v) => slot.aclaction.parameter.oid = v.0,
                AclParam::OidList(items) => {
                    let raw: Vec<sys::sai_object_id_t> = items.iter().map(|oid| oid.0).collect();
                    return list_out!(slot.aclaction.parameter.objlist, &raw);
                }
                AclParam::IpAddress(ip) => {
                    let (addr_family, addr) = ip_to(*ip);
                    slot.aclaction.parameter.ipaddr = sys::sai_ip_address_t { addr_family, addr };
                }
            }
        }
        _ => return Err(sys::SAI_STATUS_FAILURE),
    }
    Ok(())
}

/// Decodes an attribute value the C caller passed in (create, set).
///
/// An error is for attribute index 0; the caller re-indexes it.
///
/// # Safety
/// The caller of the SAI function initialised the member of `value` that `value_type` names;
/// when that member is a list, its pointer names `count` initialised elements that are not
/// written to during the call, or its count is zero.
#[allow(clippy::too_many_lines, reason = "one arm per member of the union")]
pub(crate) unsafe fn decode(value_type: ValueType, value: &sys::sai_attribute_value_t) -> Result<Value, Error> {
    Ok(match value_type {
        // Read as a byte: a C caller may store any non-zero value, which is not a Rust bool.
        ValueType::Bool => Value::Bool(member!(value.u8_) != 0),
        ValueType::CharData => {
            let raw = member!(value.chardata);
            let bytes: Vec<u8> = raw
                .iter()
                .map(|c| u8::from_ne_bytes(c.to_ne_bytes()))
                .take_while(|b| *b != 0)
                .collect();
            Value::CharData(String::from_utf8_lossy(&bytes).into_owned())
        }
        ValueType::U8 => Value::U8(member!(value.u8_)),
        ValueType::I8 => Value::I8(member!(value.s8)),
        ValueType::U16 => Value::U16(member!(value.u16_)),
        ValueType::I16 => Value::I16(member!(value.s16)),
        ValueType::U32 => Value::U32(member!(value.u32_)),
        ValueType::I32 => Value::I32(member!(value.s32)),
        ValueType::U64 => Value::U64(member!(value.u64_)),
        ValueType::I64 => Value::I64(member!(value.s64)),
        ValueType::Enum => Value::Enum(member!(value.s32)),
        ValueType::Mac => Value::Mac(member!(value.mac)),
        ValueType::Oid => Value::Oid(RawOid(member!(value.oid))),
        ValueType::OidList => Value::OidList(list_in!(value.objlist).into_iter().map(RawOid).collect()),
        ValueType::U8List => Value::U8List(list_in!(value.u8list)),
        ValueType::I8List => Value::I8List(list_in!(value.s8list)),
        ValueType::U16List => Value::U16List(list_in!(value.u16list)),
        ValueType::U32List => Value::U32List(list_in!(value.u32list)),
        ValueType::I32List => Value::I32List(list_in!(value.s32list)),
        ValueType::VlanList => Value::VlanList(list_in!(value.vlanlist)),
        ValueType::U32Range => {
            let range = member!(value.u32range);
            Value::U32Range {
                min: range.min,
                max: range.max,
            }
        }
        // The address is kept as a number; `notify` turns it back into a pointer.
        ValueType::Pointer => Value::Pointer(Pointer::from_addr(member!(value.ptr).expose_provenance())),
        ValueType::I16List => Value::I16List(list_in!(value.s16list)),
        ValueType::U16Range => {
            let range = member!(value.u16range);
            Value::U16Range {
                min: range.min,
                max: range.max,
            }
        }
        ValueType::I32Range => {
            let range = member!(value.s32range);
            Value::I32Range {
                min: range.min,
                max: range.max,
            }
        }
        ValueType::Ip4 => Value::Ip4(ip4_from(member!(value.ip4))),
        ValueType::Ip6 => Value::Ip6(Ipv6Addr::from(member!(value.ip6))),
        ValueType::IpAddress => {
            let raw = member!(value.ipaddr);
            // SAFETY: the contract of `decode`: the caller initialised `ipaddr`, that is its
            // family and the address member the family names.
            Value::IpAddress(unsafe { address_from(&raw) }?)
        }
        ValueType::IpPrefix => {
            let raw = member!(value.ipprefix);
            // SAFETY: the contract of `decode`: the caller initialised `ipprefix`, that is its
            // family and the address and mask members the family names.
            Value::IpPrefix(unsafe { prefix_from(&raw) }?)
        }
        ValueType::QosMapList => Value::QosMapList(
            list_in!(value.qosmap)
                .iter()
                .map(|entry| QosMapEntry {
                    key: qos_from(&entry.key),
                    value: qos_from(&entry.value),
                })
                .collect(),
        ),
        ValueType::MapList => Value::MapList(
            list_in!(value.maplist)
                .iter()
                .map(|entry| (entry.key, entry.value))
                .collect(),
        ),
        ValueType::AclResourceList => Value::AclResourceList(
            list_in!(value.aclresource)
                .iter()
                .map(|entry| AclResource {
                    stage: entry.stage.cast_signed(),
                    bind_point: entry.bind_point.cast_signed(),
                    avail_num: entry.avail_num,
                })
                .collect(),
        ),
        ValueType::Timespec => {
            let time = member!(value.timespec);
            Value::Timespec {
                secs: time.tv_sec,
                nanos: time.tv_nsec,
            }
        }
        ValueType::AclField(kind) | ValueType::AclAction(kind) => {
            // SAFETY: the contract of `decode`: the caller initialised `aclfield` / `aclaction`
            // for this attribute (the member the metadata names), which is `acl_decode`'s.
            return unsafe { acl_decode(matches!(value_type, ValueType::AclField(_)), kind, value) };
        }
        ValueType::MacsecSak | ValueType::MacsecSalt | ValueType::MacsecAuthKey => {
            // SAFETY: the contract of `decode` is `decode_secret`'s.
            return unsafe { decode_secret(value_type, value) };
        }
        // `aclcapability` is a result only (every attribute of the type is read-only), and
        // its first field is a C `bool`, which must not be read as a Rust one from a caller.
        ValueType::AclCapability | ValueType::Other => return Err(Error::attr(AttrError::NotSupported, 0)),
    })
}

/// `MACsec` key material: copied once from the caller's member into a [`Secret`] (heap, wiped on
/// drop), through a reference, never a temporary array. A byte array has no invalid values; the
/// arrays have the size the `Secret` takes (`sai.h`'s `sai_macsec_*_t`).
///
/// # Safety
/// The caller of the SAI function initialised the member `value_type` names (`macsecsak`,
/// `macsecsalt` or `macsecauthkey`).
unsafe fn decode_secret(value_type: ValueType, value: &sys::sai_attribute_value_t) -> Result<Value, Error> {
    let bad = || Error::attr(AttrError::InvalidValue, 0);
    Ok(match value_type {
        ValueType::MacsecSak => {
            // SAFETY: this function's contract: `macsecsak` is initialised.
            let bytes = unsafe { &value.macsecsak };
            Value::MacsecSak(Secret::from_slice(bytes).ok_or_else(bad)?)
        }
        ValueType::MacsecSalt => {
            // SAFETY: this function's contract: `macsecsalt` is initialised.
            let bytes = unsafe { &value.macsecsalt };
            Value::MacsecSalt(Secret::from_slice(bytes).ok_or_else(bad)?)
        }
        ValueType::MacsecAuthKey => {
            // SAFETY: this function's contract: `macsecauthkey` is initialised.
            let bytes = unsafe { &value.macsecauthkey };
            Value::MacsecAuthKey(Secret::from_slice(bytes).ok_or_else(bad)?)
        }
        _ => return Err(Error::Failure),
    })
}

/// Encodes a result into the value slot of a get call. Scalars overwrite the slot; lists go
/// to the buffer the slot names, with SAI's buffer-overflow convention ([`ptr::write_list`]).
///
/// # Safety
/// When `result` is a list, the caller of the SAI function initialised the list member of
/// `slot` that carries it: the count is the capacity of the buffer the pointer names (valid
/// for that many writes, not accessed otherwise during the call), or zero.
pub(crate) unsafe fn encode(result: &Value, slot: &mut sys::sai_attribute_value_t) -> Result<(), Status> {
    match result {
        Value::Bool(v) => slot.booldata = *v,
        Value::CharData(text) => {
            let mut chars: [c_char; 32] = [0; 32];
            // At most 31 bytes: the last stays NUL.
            for (dst, src) in chars.iter_mut().zip(text.bytes().take(31)) {
                *dst = c_char::from_ne_bytes(src.to_ne_bytes());
            }
            slot.chardata = chars;
        }
        Value::U8(v) => slot.u8_ = *v,
        Value::I8(v) => slot.s8 = *v,
        Value::U16(v) => slot.u16_ = *v,
        Value::I16(v) => slot.s16 = *v,
        Value::U32(v) => slot.u32_ = *v,
        Value::I32(v) | Value::Enum(v) => slot.s32 = *v,
        Value::U64(v) => slot.u64_ = *v,
        Value::I64(v) => slot.s64 = *v,
        Value::Mac(v) => slot.mac = *v,
        Value::Oid(v) => slot.oid = v.0,
        Value::U32Range { min, max } => slot.u32range = sys::sai_u32_range_t { min: *min, max: *max },
        Value::Pointer(v) => slot.ptr = core::ptr::with_exposed_provenance_mut(v.addr()),
        Value::OidList(items) => {
            let raw: Vec<sys::sai_object_id_t> = items.iter().map(|oid| oid.0).collect();
            return list_out!(slot.objlist, &raw);
        }
        Value::U8List(items) => return list_out!(slot.u8list, items),
        Value::I8List(items) => return list_out!(slot.s8list, items),
        Value::U16List(items) => return list_out!(slot.u16list, items),
        Value::U32List(items) => return list_out!(slot.u32list, items),
        Value::I32List(items) => return list_out!(slot.s32list, items),
        Value::VlanList(items) => return list_out!(slot.vlanlist, items),
        Value::I16List(items) => return list_out!(slot.s16list, items),
        Value::U16Range { min, max } => slot.u16range = sys::sai_u16_range_t { min: *min, max: *max },
        Value::I32Range { min, max } => slot.s32range = sys::sai_s32_range_t { min: *min, max: *max },
        Value::Ip4(addr) => slot.ip4 = ip4_to(*addr),
        Value::Ip6(addr) => slot.ip6 = addr.octets(),
        Value::IpAddress(ip) => {
            let (addr_family, addr) = ip_to(*ip);
            slot.ipaddr = sys::sai_ip_address_t { addr_family, addr };
        }
        Value::IpPrefix(prefix) => slot.ipprefix = prefix_to(*prefix),
        Value::QosMapList(items) => {
            let raw: Vec<sys::sai_qos_map_t> = items
                .iter()
                .map(|entry| sys::sai_qos_map_t {
                    key: qos_to(&entry.key),
                    value: qos_to(&entry.value),
                })
                .collect();
            return list_out!(slot.qosmap, &raw);
        }
        Value::MapList(items) => {
            let raw: Vec<sys::sai_map_t> = items
                .iter()
                .map(|(key, value)| sys::sai_map_t {
                    key: *key,
                    value: *value,
                })
                .collect();
            return list_out!(slot.maplist, &raw);
        }
        Value::AclCapability {
            action_list_mandatory,
            actions,
            non_contiguous_exact_match,
            match_type,
        } => {
            // Only the list field is read: the flags are C `bool`s the caller need not have
            // set, and a byte that is neither 0 nor 1 is not a Rust `bool`.
            let mut action_list = member!(slot.aclcapability.action_list);
            // SAFETY: the contract of `encode`: the caller set the list's count to the capacity
            // of the buffer its pointer names (or to zero), which is `write_list`'s contract.
            let outcome = unsafe { ptr::write_list(&mut action_list.count, action_list.list, actions) };
            slot.aclcapability = sys::sai_acl_capability_t {
                is_action_list_mandatory: *action_list_mandatory,
                action_list,
                supported_match_type: match_type.cast_unsigned(),
                is_non_contiguous_bits_exact_match_supported: *non_contiguous_exact_match,
            };
            return outcome;
        }
        Value::AclResourceList(items) => {
            let raw: Vec<sys::sai_acl_resource_t> = items
                .iter()
                .map(|entry| sys::sai_acl_resource_t {
                    stage: entry.stage.cast_unsigned(),
                    bind_point: entry.bind_point.cast_unsigned(),
                    avail_num: entry.avail_num,
                })
                .collect();
            return list_out!(slot.aclresource, &raw);
        }
        Value::Timespec { secs, nanos } => {
            slot.timespec = sys::sai_timespec_t {
                tv_sec: *secs,
                tv_nsec: *nanos,
            };
        }
        // SAFETY: the contract of `encode` is `acl_encode`'s.
        Value::AclField(_) | Value::AclAction(_) => return unsafe { acl_encode(result, slot) },
        // MACsec key material is write-only: never handed back (the model answers none)
        Value::MacsecSak(_) | Value::MacsecSalt(_) | Value::MacsecAuthKey(_) => return Err(sys::SAI_STATUS_FAILURE),
    }
    Ok(())
}

#[cfg(test)]
// The unsafe blocks of the tests pass null, zero counts or pointers to live locals of the
// stated length; one comment here instead of one per assertion.
#[allow(clippy::undocumented_unsafe_blocks, clippy::cast_ptr_alignment)]
mod tests {
    use super::*;

    fn zeroed() -> sys::sai_attribute_value_t {
        sys::sai_attribute_value_t { u64_: 0 }
    }

    #[test]
    fn scalars_round_trip_through_the_union() {
        let values = [
            Value::Bool(true),
            Value::U8(200),
            Value::I8(-3),
            Value::U16(4094),
            Value::I16(-2),
            Value::U32(100_000),
            Value::I32(-7),
            Value::U64(u64::MAX - 1),
            Value::I64(i64::MIN),
            Value::Enum(3),
            Value::Mac([1, 2, 3, 4, 5, 6]),
            Value::Oid(RawOid(0x2100_0000_0000_0001)),
            Value::U32Range { min: 5, max: 9 },
            Value::CharData("Ethernet0".into()),
            Value::U16Range { min: 1, max: 4094 },
            Value::I32Range { min: -5, max: 9 },
            Value::Ip4(Ipv4Addr::new(10, 1, 2, 3)),
            Value::Ip6(Ipv6Addr::new(0xfc00, 1, 2, 3, 4, 5, 6, 7)),
            Value::IpAddress(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
            Value::IpAddress(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1))),
            Value::IpPrefix(
                IpPrefix::new(
                    IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0)),
                    IpAddr::V4(Ipv4Addr::new(255, 255, 0, 0)),
                )
                .unwrap(),
            ),
            Value::IpPrefix(
                IpPrefix::new(
                    IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)),
                    IpAddr::V6(Ipv6Addr::new(0xffff, 0xffff, 0, 0, 0, 0, 0, 0)),
                )
                .unwrap(),
            ),
            Value::Timespec {
                secs: 1 << 40,
                nanos: 999_999_999,
            },
        ];
        for value in values {
            let mut slot = zeroed();
            // SAFETY: not a list.
            unsafe { encode(&value, &mut slot) }.unwrap();
            // SAFETY: the member for this value type was written by encode just above.
            assert_eq!(unsafe { decode(value.value_type(), &slot) }, Ok(value));
        }
    }

    // ---- The union against the C compiler (tests/union_check.c) ----

    /// The cases of `tests/union_check.c`: its name for the case, the member's type, and the
    /// value the C side sets through that member.
    #[allow(clippy::too_many_lines)]
    fn cases() -> Vec<(&'static str, ValueType, Value)> {
        let ip6 = Ipv6Addr::from([0xfc, 0x00, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0x07]);
        let ip6_net = Ipv6Addr::from([0xfc, 0x00, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mask6 = Ipv6Addr::from([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let qos = |n: u8| QosParams {
            tc: 1 + n,
            dscp: 10 + n,
            dot1p: 2 + n,
            prio: 3 + n,
            pg: 4 + n,
            queue_index: 5 + n,
            color: if n % 2 == 1 {
                sys::SAI_PACKET_COLOR_RED
            } else {
                sys::SAI_PACKET_COLOR_YELLOW
            }
            .cast_signed(),
            mpls_exp: 6 + n,
            fc: 7 + n,
            dei: n % 2,
            vc: 8 + n,
        };
        let mut cases = vec![
            ("bool", ValueType::Bool, Value::Bool(true)),
            ("u8", ValueType::U8, Value::U8(0x5c)),
            ("s8", ValueType::I8, Value::I8(-3)),
            ("u16", ValueType::U16, Value::U16(0x1234)),
            ("s16", ValueType::I16, Value::I16(-2)),
            ("u32", ValueType::U32, Value::U32(0x0102_0304)),
            ("s32", ValueType::I32, Value::I32(-7)),
            ("enum", ValueType::Enum, Value::Enum(0x0001_0203)),
            ("u64", ValueType::U64, Value::U64(0x0102_0304_0506_0708)),
            ("s64", ValueType::I64, Value::I64(-(1 << 40))),
            ("oid", ValueType::Oid, Value::Oid(RawOid(0x2100_0000_0000_0042))),
            ("ptr", ValueType::Pointer, Value::Pointer(Pointer::from_addr(0x1000))),
            ("mac", ValueType::Mac, Value::Mac([0x02, 0x11, 0x22, 0x33, 0x44, 0x55])),
            ("ip4", ValueType::Ip4, Value::Ip4(Ipv4Addr::new(10, 1, 2, 3))),
            ("ip6", ValueType::Ip6, Value::Ip6(ip6)),
            ("chardata", ValueType::CharData, Value::CharData("Ethernet0".into())),
            (
                "u32range",
                ValueType::U32Range,
                Value::U32Range {
                    min: 5,
                    max: 0x0102_0304,
                },
            ),
            ("u16range", ValueType::U16Range, Value::U16Range { min: 1, max: 4094 }),
            ("s32range", ValueType::I32Range, Value::I32Range { min: -5, max: 9 }),
            (
                "ipaddr4",
                ValueType::IpAddress,
                Value::IpAddress(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
            ),
            ("ipaddr6", ValueType::IpAddress, Value::IpAddress(IpAddr::V6(ip6))),
            (
                "ipprefix4",
                ValueType::IpPrefix,
                Value::IpPrefix(
                    IpPrefix::new(
                        IpAddr::V4(Ipv4Addr::new(10, 1, 0, 0)),
                        IpAddr::V4(Ipv4Addr::new(255, 255, 0, 0)),
                    )
                    .unwrap(),
                ),
            ),
            (
                "ipprefix6",
                ValueType::IpPrefix,
                Value::IpPrefix(IpPrefix::new(IpAddr::V6(ip6_net), IpAddr::V6(mask6)).unwrap()),
            ),
            (
                "timespec",
                ValueType::Timespec,
                Value::Timespec {
                    secs: 1 << 40,
                    nanos: 999_999_999,
                },
            ),
            (
                "objlist",
                ValueType::OidList,
                Value::OidList(vec![
                    RawOid(0x2100_0000_0000_0010),
                    RawOid(0x20),
                    RawOid(0x0102_0304_0506_0708),
                ]),
            ),
            ("u8list", ValueType::U8List, Value::U8List(vec![1, 2, 0xfe])),
            ("s8list", ValueType::I8List, Value::I8List(vec![-1, 2, -3])),
            (
                "u16list",
                ValueType::U16List,
                Value::U16List(vec![0x0102, 0x0304, 0x0506]),
            ),
            ("s16list", ValueType::I16List, Value::I16List(vec![-1, 2, -3])),
            ("u32list", ValueType::U32List, Value::U32List(vec![0x0102_0304, 5, 6])),
            ("s32list", ValueType::I32List, Value::I32List(vec![-1, 0x0001_0203, -3])),
            ("vlanlist", ValueType::VlanList, Value::VlanList(vec![1, 100, 4094])),
            (
                "maplist",
                ValueType::MapList,
                Value::MapList(vec![(1, -1), (0x0102_0304, 20)]),
            ),
            (
                "aclresource",
                ValueType::AclResourceList,
                Value::AclResourceList(vec![AclResource {
                    stage: sys::SAI_ACL_STAGE_EGRESS.cast_signed(),
                    bind_point: sys::SAI_ACL_BIND_POINT_TYPE_VLAN.cast_signed(),
                    avail_num: 300,
                }]),
            ),
            (
                "qosmap",
                ValueType::QosMapList,
                Value::QosMapList(
                    (0..2)
                        .map(|i| QosMapEntry {
                            key: qos(i),
                            value: qos(20 + i),
                        })
                        .collect(),
                ),
            ),
            (
                "aclcapability",
                ValueType::AclCapability,
                Value::AclCapability {
                    action_list_mandatory: true,
                    actions: vec![3, 0x0001_0203],
                    non_contiguous_exact_match: false,
                    match_type: sys::SAI_ACL_TABLE_SUPPORTED_MATCH_TYPE_EXACT.cast_signed(),
                },
            ),
        ];
        // ACL fields and actions: one case per member an attribute of the headers uses.
        let oids = vec![
            RawOid(0x2100_0000_0000_0010),
            RawOid(0x20),
            RawOid(0x0102_0304_0506_0708),
        ];
        let field = |name, data: AclFieldData| {
            (
                name,
                ValueType::AclField(data.kind()),
                Value::AclField(AclField::Enabled(data)),
            )
        };
        let action = |name, param: AclParam| {
            (
                name,
                ValueType::AclAction(param.kind()),
                Value::AclAction(AclAction::Enabled(param)),
            )
        };
        cases.extend([
            (
                "aclfield_off",
                ValueType::AclField(AclKind::Ip4),
                Value::AclField(AclField::Disabled(AclKind::Ip4)),
            ),
            field("aclfield_bool", AclFieldData::Bool(true)),
            field("aclfield_u8", AclFieldData::U8(fm(0x5c, 0xf0))),
            field("aclfield_u16", AclFieldData::U16(fm(0x0806, 0xffff))),
            field("aclfield_u32", AclFieldData::U32(fm(0x0102_0304, 0xffff_0000))),
            field(
                "aclfield_u64",
                AclFieldData::U64(fm(0x0102_0304_0506_0708, 0xffff_ffff_0000_0000)),
            ),
            field("aclfield_enum", AclFieldData::Enum(fm(3, -1))),
            field(
                "aclfield_mac",
                AclFieldData::Mac(fm([0x02, 0x11, 0x22, 0x33, 0x44, 0x55], [0xff, 0xff, 0xff, 0, 0, 0])),
            ),
            field(
                "aclfield_ip4",
                AclFieldData::Ip4(fm(Ipv4Addr::new(10, 1, 2, 3), Ipv4Addr::new(255, 255, 0, 0))),
            ),
            field("aclfield_ip6", AclFieldData::Ip6(fm(ip6, mask6))),
            field("aclfield_oid", AclFieldData::Oid(RawOid(0x2100_0000_0000_0042))),
            field("aclfield_objlist", AclFieldData::OidList(oids.clone())),
            field(
                "aclfield_u8list",
                AclFieldData::U8List(fm(vec![1, 2, 3], vec![0xff, 0xf0, 0x0f])),
            ),
            (
                "aclaction_off",
                ValueType::AclAction(AclKind::Enum),
                Value::AclAction(AclAction::Disabled(AclKind::Enum)),
            ),
            action("aclaction_bool", AclParam::Bool(true)),
            action("aclaction_u8", AclParam::U8(0x5c)),
            action("aclaction_u16", AclParam::U16(0x1234)),
            action("aclaction_u32", AclParam::U32(0x0102_0304)),
            action("aclaction_s32", AclParam::I32(-7)),
            action(
                "aclaction_enum",
                AclParam::Enum(sys::SAI_PACKET_ACTION_DROP.cast_signed()),
            ),
            action("aclaction_mac", AclParam::Mac([0x02, 0x11, 0x22, 0x33, 0x44, 0x55])),
            action("aclaction_ip4", AclParam::Ip4(Ipv4Addr::new(10, 1, 2, 3))),
            action("aclaction_ip6", AclParam::Ip6(ip6)),
            action("aclaction_oid", AclParam::Oid(RawOid(0x2100_0000_0000_0042))),
            action("aclaction_objlist", AclParam::OidList(oids)),
            action("aclaction_ipaddr6", AclParam::IpAddress(IpAddr::V6(ip6))),
        ]);
        cases
    }

    /// The prebuilt C helper: from `$SAI_RS_TEST_BIN` (a test bundle on a target), else where
    /// build.rs left it. `None`: there was no C compiler at build time.
    fn union_check() -> Option<std::path::PathBuf> {
        if let Some(dir) = std::env::var_os("SAI_RS_TEST_BIN") {
            let exe = std::path::Path::new(&dir).join("union-check");
            assert!(exe.is_file(), "{} is missing from the test bundle", exe.display());
            return Some(exe);
        }
        option_env!("SAI_FFI_UNION_CHECK")
            .map(std::path::PathBuf::from)
            .filter(|exe| exe.is_file())
    }

    fn hex(bytes: &[u8]) -> String {
        use core::fmt::Write as _;
        if bytes.is_empty() {
            return "-".into();
        }
        bytes.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
    }

    fn unhex(text: &str) -> Vec<u8> {
        if text == "-" {
            return Vec::new();
        }
        (0..text.len() / 2)
            .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    const VALUE_SIZE: usize = size_of::<sys::sai_attribute_value_t>();

    fn value_from(bytes: &[u8]) -> sys::sai_attribute_value_t {
        assert_eq!(bytes.len(), VALUE_SIZE);
        let mut slot = core::mem::MaybeUninit::<sys::sai_attribute_value_t>::uninit();
        // SAFETY: VALUE_SIZE bytes are copied into a union of that size.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), slot.as_mut_ptr().cast::<u8>(), VALUE_SIZE) };
        // SAFETY: every byte was written just above, and every member is plain data.
        unsafe { slot.assume_init() }
    }

    fn bytes_of(slot: &sys::sai_attribute_value_t) -> Vec<u8> {
        // SAFETY: the union was made by `value_from`: every byte of it is initialised.
        unsafe { core::slice::from_raw_parts(core::ptr::from_ref(slot).cast::<u8>(), VALUE_SIZE) }.to_vec()
    }

    /// An ACL field's data and mask.
    const fn fm<T>(data: T, mask: T) -> FieldMatch<T> {
        FieldMatch { data, mask }
    }

    /// The bytes one list element takes in the caller's buffer.
    fn element_size(value: &Value) -> (usize, usize) {
        match value {
            Value::OidList(v)
            | Value::AclField(AclField::Enabled(AclFieldData::OidList(v)))
            | Value::AclAction(AclAction::Enabled(AclParam::OidList(v))) => (v.len(), 8),
            Value::U8List(v) => (v.len(), 1),
            Value::I8List(v) => (v.len(), 1),
            Value::U16List(v) | Value::VlanList(v) => (v.len(), 2),
            Value::I16List(v) => (v.len(), 2),
            Value::U32List(v) => (v.len(), 4),
            Value::I32List(v) => (v.len(), 4),
            Value::MapList(v) => (v.len(), size_of::<sys::sai_map_t>()),
            Value::AclResourceList(v) => (v.len(), size_of::<sys::sai_acl_resource_t>()),
            Value::QosMapList(v) => (v.len(), size_of::<sys::sai_qos_map_t>()),
            Value::AclCapability { actions, .. } => (actions.len(), 4),
            // the data list at the buffer's start, the mask list 16 bytes in (union_check.c)
            Value::AclField(AclField::Enabled(AclFieldData::U8List(m))) => (16 + m.mask.len(), 1),
            _ => (0, 0),
        }
    }

    /// Every member of the union this module reads or writes, against the C compiler: the
    /// encoder's output is inspected by C through the member the SAI header names (with
    /// every other byte of the value still as the caller left it), and values made in C
    /// through that member are decoded here. On a big-endian target this is the test that
    /// tells a u8 read through a u32 from a u8.
    #[test]
    #[cfg_attr(miri, ignore = "runs a C program")]
    #[allow(clippy::too_many_lines)]
    fn the_c_compiler_agrees_on_every_member() {
        use std::io::{BufRead, BufReader, Write};
        let Some(exe) = union_check() else {
            eprintln!(
                "SKIPPED the_c_compiler_agrees_on_every_member: no C helper was built (no C compiler at build time)"
            );
            return;
        };
        let mut child = std::process::Command::new(&exe)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", exe.display()));
        let mut to_c = child.stdin.take().unwrap();
        let mut from_c = BufReader::new(child.stdout.take().unwrap());
        let mut ask = |request: String| {
            writeln!(to_c, "{request}").unwrap();
            to_c.flush().unwrap();
            let mut answer = String::new();
            from_c.read_line(&mut answer).unwrap();
            answer.trim().to_string()
        };
        assert_eq!(
            ask("S".into()),
            format!("{VALUE_SIZE} {}", size_of::<*const ()>()),
            "the helper was built for another target"
        );

        // The caller's list buffer: aligned for every element type, 8 elements of the largest.
        let mut buffer = [0u64; 64];
        let addr = buffer.as_mut_ptr().expose_provenance();
        let mut checked = 0;
        for (name, value_type, value) in cases() {
            let (count, size) = element_size(&value);

            // Encode: C prepares the caller's value (garbage in every byte, a list's count
            // and pointer set through the member), Rust encodes into it, C inspects it.
            let prepared = ask(format!("R {name} {addr} 8"));
            let mut slot = value_from(&unhex(&prepared));
            buffer.fill(0);
            // SAFETY: for a list, C set the member's count to 8 and its pointer to `buffer`,
            // which holds 8 elements of any of the element types.
            unsafe { encode(&value, &mut slot) }.unwrap_or_else(|status| panic!("{name}: encode {status}"));
            // SAFETY: `buffer` is a live array; count * size bytes of it are within it.
            let elements = unsafe { core::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), count * size) };
            let verdict = ask(format!("E {name} {addr} {} {}", hex(&bytes_of(&slot)), hex(elements)));
            assert_eq!(verdict, "ok", "{name}: what Rust encoded, as C reads it");
            checked += 1;

            // Decode: C makes the value through the member, Rust decodes the image.
            if value_type == ValueType::AclCapability {
                continue; // a result only: never decoded
            }
            let made = ask(format!("M {name} {addr}"));
            let (image, elements) = made.split_once(' ').unwrap();
            let elements = unhex(elements);
            assert_eq!(elements.len(), count * size, "{name}: element size");
            buffer.fill(0);
            // SAFETY: at most 8 elements of at most 32 bytes: within `buffer`.
            unsafe {
                core::ptr::copy_nonoverlapping(elements.as_ptr(), buffer.as_mut_ptr().cast::<u8>(), elements.len());
            }
            let slot = value_from(&unhex(image));
            // SAFETY: C initialised the member of this case; a list points to `buffer`, which
            // now holds the elements C made.
            let decoded = unsafe { decode(value_type, &slot) };
            assert_eq!(decoded, Ok(value), "{name}: what C made, as Rust decodes it");
            checked += 1;
        }
        drop(to_c);
        assert!(child.wait().unwrap().success());
        // Every member type the rim knows is in the table (a new one must be added to both sides).
        let covered: Vec<ValueType> = cases().iter().map(|(_, value_type, _)| *value_type).collect();
        for value_type in [
            ValueType::Bool,
            ValueType::CharData,
            ValueType::U8,
            ValueType::I8,
            ValueType::U16,
            ValueType::I16,
            ValueType::U32,
            ValueType::I32,
            ValueType::U64,
            ValueType::I64,
            ValueType::Enum,
            ValueType::Mac,
            ValueType::Oid,
            ValueType::OidList,
            ValueType::U8List,
            ValueType::I8List,
            ValueType::U16List,
            ValueType::U32List,
            ValueType::I32List,
            ValueType::VlanList,
            ValueType::U32Range,
            ValueType::Pointer,
            ValueType::I16List,
            ValueType::U16Range,
            ValueType::I32Range,
            ValueType::Ip4,
            ValueType::Ip6,
            ValueType::IpAddress,
            ValueType::IpPrefix,
            ValueType::QosMapList,
            ValueType::MapList,
            ValueType::AclCapability,
            ValueType::AclResourceList,
            ValueType::Timespec,
        ] {
            assert!(covered.contains(&value_type), "{value_type:?} has no case");
        }
        // And every attribute of every implemented object type travels in one of them, or is
        // not decoded at all.
        let mut undecoded = 0;
        for (object_type, _) in sai_meta::OBJECT_TYPES {
            if !sai_core::caps::implemented(object_type) {
                continue;
            }
            for meta in sai_meta::attributes(object_type) {
                if meta.value_type == ValueType::Other {
                    undecoded += 1;
                } else {
                    assert!(covered.contains(&meta.value_type), "{}", meta.name);
                }
            }
        }
        eprintln!(
            "union test: {checked} images agreed with C; {undecoded} attributes of implemented types have an undecoded type"
        );
    }

    #[test]
    fn an_ipv4_address_is_its_bytes_in_network_order_on_every_host() {
        let mut slot = zeroed();
        // SAFETY: not a list.
        unsafe { encode(&Value::Ip4(Ipv4Addr::new(10, 1, 2, 3)), &mut slot) }.unwrap();
        // SAFETY: a u64 covers the union's first bytes, all written or zero.
        let image = unsafe { slot.u64_ }.to_ne_bytes();
        assert_eq!(&image[..4], &[10, 1, 2, 3]);
        let mut slot = zeroed();
        // SAFETY: not a list.
        unsafe { encode(&Value::IpAddress(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))), &mut slot) }.unwrap();
        // SAFETY: ipaddr was written by encode.
        let raw = unsafe { slot.ipaddr };
        assert_eq!(raw.addr_family, sys::SAI_IP_ADDR_FAMILY_IPV4);
        // SAFETY: the whole address union was written by encode.
        assert_eq!(
            unsafe { raw.addr.ip6 },
            [10, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        // a family that is neither is the value's fault, and nothing of the address is read
        let slot = sys::sai_attribute_value_t {
            ipaddr: sys::sai_ip_address_t {
                addr_family: 7,
                addr: sys::sai_ip_addr_t { ip6: [0; 16] },
            },
        };
        // SAFETY: ipaddr is initialised.
        assert_eq!(unsafe { decode(ValueType::IpAddress, &slot) }, Err(BAD_VALUE));
        // address and mask of different families cannot be expressed in the C struct; a mask
        // that leaves host bits set is normalised away
        let prefix = sys::sai_ip_prefix_t {
            addr_family: sys::SAI_IP_ADDR_FAMILY_IPV4,
            addr: ip_to(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))).1,
            mask: ip_to(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 0))).1,
        };
        let slot = sys::sai_attribute_value_t { ipprefix: prefix };
        // SAFETY: ipprefix is initialised.
        let Ok(Value::IpPrefix(decoded)) = (unsafe { decode(ValueType::IpPrefix, &slot) }) else {
            panic!("not a prefix");
        };
        assert_eq!(decoded.addr(), IpAddr::V4(Ipv4Addr::new(10, 1, 2, 0)));
    }

    #[test]
    fn struct_lists_and_the_acl_capability() {
        let entry = |n: u8| QosMapEntry {
            key: QosParams {
                dscp: n,
                color: 2,
                ..QosParams::default()
            },
            value: QosParams {
                tc: n / 8,
                queue_index: 7,
                vc: 1,
                ..QosParams::default()
            },
        };
        let value = Value::QosMapList((0..64).map(entry).collect());
        let mut buf = vec![
            sys::sai_qos_map_t {
                key: qos_to(&QosParams::default()),
                value: qos_to(&QosParams::default()),
            };
            64
        ];
        let mut slot = sys::sai_attribute_value_t {
            qosmap: sys::sai_qos_map_list_t {
                count: 64,
                list: buf.as_mut_ptr(),
            },
        };
        // SAFETY: the list member names `buf`, 64 elements.
        unsafe { encode(&value, &mut slot) }.unwrap();
        // SAFETY: the member was initialised above and names the 64 written elements.
        assert_eq!(unsafe { decode(ValueType::QosMapList, &slot) }, Ok(value));

        let value = Value::MapList(vec![(1, -1), (2, 20)]);
        let mut buf = [sys::sai_map_t { key: 0, value: 0 }; 2];
        let mut slot = sys::sai_attribute_value_t {
            maplist: sys::sai_map_list_t {
                count: 2,
                list: buf.as_mut_ptr(),
            },
        };
        // SAFETY: the list member names `buf`, 2 elements.
        unsafe { encode(&value, &mut slot) }.unwrap();
        // SAFETY: as above.
        assert_eq!(unsafe { decode(ValueType::MapList, &slot) }, Ok(value));

        let value = Value::AclResourceList(vec![AclResource {
            stage: 1,
            bind_point: 2,
            avail_num: 300,
        }]);
        let mut buf = [sys::sai_acl_resource_t {
            stage: 0,
            bind_point: 0,
            avail_num: 0,
        }; 1];
        let mut slot = sys::sai_attribute_value_t {
            aclresource: sys::sai_acl_resource_list_t {
                count: 1,
                list: buf.as_mut_ptr(),
            },
        };
        // SAFETY: the list member names `buf`, 1 element.
        unsafe { encode(&value, &mut slot) }.unwrap();
        // SAFETY: as above.
        assert_eq!(unsafe { decode(ValueType::AclResourceList, &slot) }, Ok(value));

        // The capability: the flag is written, the list goes to the caller's buffer, and the
        // flag the caller left (any byte) is never read.
        let mut actions = [0i32; 4];
        let mut slot = sys::sai_attribute_value_t {
            aclcapability: sys::sai_acl_capability_t {
                is_action_list_mandatory: false,
                action_list: sys::sai_s32_list_t {
                    count: 4,
                    list: actions.as_mut_ptr(),
                },
                supported_match_type: 0,
                is_non_contiguous_bits_exact_match_supported: true,
            },
        };
        let value = Value::AclCapability {
            action_list_mandatory: true,
            actions: vec![3, 9],
            non_contiguous_exact_match: false,
            match_type: 1,
        };
        // SAFETY: the list names `actions`, 4 elements.
        unsafe { encode(&value, &mut slot) }.unwrap();
        // SAFETY: the whole member was written by encode.
        let written = unsafe { slot.aclcapability };
        assert!(written.is_action_list_mandatory && !written.is_non_contiguous_bits_exact_match_supported);
        assert_eq!((written.action_list.count, actions), (2, [3, 9, 0, 0]));
        assert_eq!(written.supported_match_type, 1);
        // SAFETY: nothing is read for a type that is a result only.
        assert_eq!(
            unsafe { decode(ValueType::AclCapability, &slot) },
            Err(Error::attr(AttrError::NotSupported, 0))
        );
    }

    #[test]
    fn entry_keys() {
        let fdb = sys::sai_fdb_entry_t {
            switch_id: 0x2100_0000_0000_0000,
            mac_address: [2, 0, 0, 0, 0, 1],
            bv_id: 0x2600_0000_0000_0002,
        };
        // SAFETY: no address in the struct.
        assert_eq!(
            unsafe { fdb.key() },
            Ok((
                RawOid(0x2100_0000_0000_0000),
                EntryKey::Fdb {
                    bv_id: RawOid(0x2600_0000_0000_0002),
                    mac: [2, 0, 0, 0, 0, 1]
                }
            ))
        );
        let (addr_family, addr) = ip_to(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));
        let mut neighbor = sys::sai_neighbor_entry_t {
            switch_id: 1,
            rif_id: 2,
            ip_address: sys::sai_ip_address_t { addr_family, addr },
        };
        // SAFETY: the address was written whole by ip_to.
        assert_eq!(
            unsafe { neighbor.key() },
            Ok((
                RawOid(1),
                EntryKey::Neighbor {
                    rif: RawOid(2),
                    ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))
                }
            ))
        );
        neighbor.ip_address.addr_family = 9;
        // SAFETY: as above; an unknown family reads nothing.
        assert_eq!(unsafe { neighbor.key() }, Err(Error::InvalidParameter));
        let route = sys::sai_route_entry_t {
            switch_id: 1,
            vr_id: 3,
            destination: prefix_to(
                IpPrefix::new(
                    IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)),
                    IpAddr::V6(Ipv6Addr::new(0xffff, 0, 0, 0, 0, 0, 0, 0)),
                )
                .unwrap(),
            ),
        };
        // SAFETY: address and mask were written whole by prefix_to.
        let Ok((_, EntryKey::Route { vr, prefix })) = (unsafe { route.key() }) else {
            panic!("not a route key");
        };
        assert_eq!((vr, prefix.addr().is_ipv6()), (RawOid(3), true));
    }

    #[test]
    fn acl_enable_is_read_as_a_byte_and_disabled_data_is_never_read() {
        // enable = 2 (any non-zero C bool), data / mask of the attribute's member
        let mut slot = zeroed();
        slot.aclfield = sys::sai_acl_field_data_t {
            enable: false,
            mask: sys::sai_acl_field_data_mask_t { u16_: 0xffff },
            data: sys::sai_acl_field_data_data_t { u16_: 0x0806 },
        };
        // SAFETY: the first byte of aclfield is its enable flag (offset 0, checked by bindgen).
        unsafe { (&raw mut slot.aclfield.enable).cast::<u8>().write(2) };
        // SAFETY: aclfield with its u16 members was written above.
        assert_eq!(
            unsafe { decode(ValueType::AclField(AclKind::U16), &slot) },
            Ok(Value::AclField(AclField::Enabled(AclFieldData::U16(FieldMatch {
                data: 0x0806,
                mask: 0xffff
            }))))
        );
        // disabled: a list's null pointer with a count is not looked at
        slot.aclfield = sys::sai_acl_field_data_t {
            enable: false,
            mask: sys::sai_acl_field_data_mask_t { u64_: 0 },
            data: sys::sai_acl_field_data_data_t {
                objlist: sys::sai_object_list_t {
                    count: 5,
                    list: core::ptr::null_mut(),
                },
            },
        };
        // SAFETY: enable is initialised; nothing else is read when it is false.
        assert_eq!(
            unsafe { decode(ValueType::AclField(AclKind::OidList), &slot) },
            Ok(Value::AclField(AclField::Disabled(AclKind::OidList)))
        );
        // enabled with that list: the attribute's fault, not a read through null
        // SAFETY: as above; the null list is refused before anything is read.
        unsafe { (&raw mut slot.aclfield.enable).cast::<u8>().write(1) };
        assert_eq!(
            unsafe { decode(ValueType::AclField(AclKind::OidList), &slot) },
            Err(Error::attr(AttrError::InvalidValue, 0))
        );
        // a get of an unset field writes enable = false and leaves the caller's list alone
        let mut ids = [7u64; 2];
        slot.aclfield.data.objlist = sys::sai_object_list_t {
            count: 2,
            list: ids.as_mut_ptr(),
        };
        // SAFETY: not a list result.
        unsafe { encode(&Value::AclField(AclField::Disabled(AclKind::OidList)), &mut slot) }.unwrap();
        // SAFETY: aclfield was written above.
        let enable = unsafe { (&raw const slot.aclfield.enable).cast::<u8>().read() };
        let count = member!(slot.aclfield.data.objlist).count;
        assert_eq!((enable, count, ids), (0, 2, [7, 7]));
        // a member the union does not have
        // SAFETY: nothing is read for an unsupported pair.
        assert_eq!(
            unsafe { decode(ValueType::AclAction(AclKind::U64), &slot) },
            Err(Error::attr(AttrError::NotSupported, 0))
        );
    }

    #[test]
    fn bool_is_read_as_a_byte_and_chardata_is_cut() {
        let slot = sys::sai_attribute_value_t { u8_: 2 };
        // SAFETY: u8_ (the byte booldata occupies) is initialised.
        assert_eq!(unsafe { decode(ValueType::Bool, &slot) }, Ok(Value::Bool(true)));
        let mut slot = zeroed();
        let long = "x".repeat(40);
        // SAFETY: not a list.
        unsafe { encode(&Value::CharData(long), &mut slot) }.unwrap();
        // SAFETY: chardata was written by encode.
        assert_eq!(
            unsafe { decode(ValueType::CharData, &slot) },
            Ok(Value::CharData("x".repeat(31)))
        );
        // SAFETY: nothing is read for a type that is not decoded.
        assert_eq!(
            unsafe { decode(ValueType::Other, &slot) },
            Err(Error::attr(AttrError::NotSupported, 0))
        );
    }

    #[test]
    fn lists_in_and_out() {
        let mut ids = [0x10u64, 0x20, 0x30];
        let slot = sys::sai_attribute_value_t {
            objlist: sys::sai_object_list_t {
                count: 3,
                list: ids.as_mut_ptr(),
            },
        };
        // SAFETY: objlist names the 3 elements of `ids`.
        assert_eq!(
            unsafe { decode(ValueType::OidList, &slot) },
            Ok(Value::OidList(vec![RawOid(0x10), RawOid(0x20), RawOid(0x30)]))
        );
        // zero count: the pointer is not looked at
        let slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 0,
                list: core::ptr::null_mut(),
            },
        };
        // SAFETY: a zero count.
        assert_eq!(unsafe { decode(ValueType::U32List, &slot) }, Ok(Value::U32List(vec![])));
        // a count with a null pointer is the attribute's fault
        let slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 2,
                list: core::ptr::null_mut(),
            },
        };
        // SAFETY: null is refused before anything is read.
        assert_eq!(
            unsafe { decode(ValueType::U32List, &slot) },
            Err(Error::attr(AttrError::InvalidValue, 0))
        );

        let result = Value::U32List(vec![1, 2, 3, 4]);
        let mut buf = [0u32; 8];
        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 8,
                list: buf.as_mut_ptr(),
            },
        };
        // SAFETY (the three encodes below): the list member names a live buffer of `count` elements, or count is 0.
        assert_eq!(unsafe { encode(&result, &mut slot) }, Ok(()));
        // SAFETY: u32list was initialised above and updated by encode.
        assert_eq!(unsafe { slot.u32list.count }, 4);
        assert_eq!(buf, [1, 2, 3, 4, 0, 0, 0, 0]);

        let mut small = [9u32; 2];
        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 2,
                list: small.as_mut_ptr(),
            },
        };
        assert_eq!(
            unsafe { encode(&result, &mut slot) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        // SAFETY: as above.
        assert_eq!(unsafe { slot.u32list.count }, 4);
        assert_eq!(small, [9, 9]);

        let mut slot = sys::sai_attribute_value_t {
            u32list: sys::sai_u32_list_t {
                count: 0,
                list: core::ptr::null_mut(),
            },
        };
        assert_eq!(
            unsafe { encode(&result, &mut slot) },
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
        );
        // SAFETY: as above.
        assert_eq!(unsafe { slot.u32list.count }, 4);
    }
}

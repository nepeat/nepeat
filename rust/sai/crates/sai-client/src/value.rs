//! Attributes as owned values, and their C form for one call (`Encoded`): an array of zeroed
//! `sai_attribute_t` (the C programs `memset` theirs) whose list members point into storage the
//! `Encoded` owns, so the pointers live exactly as long as the call needs them. A get's answer is
//! decoded from the bytes the library wrote back (`raw::value_bytes`) and from that storage.
#![forbid(unsafe_code)]

use core::mem::{offset_of, size_of};
use std::net::IpAddr;

use crate::callback;
use crate::raw;
use crate::sys;

/// The member of an ACL field's data / mask or of an ACL action's parameter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AclData {
    /// nothing written (zero)
    #[default]
    None,
    /// `u8`
    U8(u8),
    /// `u16`
    U16(u16),
    /// `u32`
    U32(u32),
    /// `s32`
    S32(i32),
    /// `ip4` (network order bytes)
    Ip4([u8; 4]),
    /// `ip6`
    Ip6([u8; 16]),
    /// `mac`
    Mac([u8; 6]),
    /// `oid`
    Oid(sys::sai_object_id_t),
    /// `objlist`
    Oids(Vec<sys::sai_object_id_t>),
}

/// `sai_acl_field_data_t`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AclField {
    /// `enable`
    pub enable: bool,
    /// `data`
    pub data: AclData,
    /// `mask`
    pub mask: AclData,
}

/// `sai_acl_action_data_t`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AclAction {
    /// `enable`
    pub enable: bool,
    /// `parameter`
    pub parameter: AclData,
}

/// `sai_qos_map_params_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs, reason = "the header's field names")]
pub struct QosParams {
    pub tc: u8,
    pub dscp: u8,
    pub dot1p: u8,
    pub prio: u8,
    pub pg: u8,
    pub queue_index: u8,
    pub color: u32,
    pub mpls_exp: u8,
    pub fc: u8,
    pub dei: u8,
    pub vc: u8,
}

/// `sai_qos_map_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QosMapEntry {
    /// `key`
    pub key: QosParams,
    /// `value`
    pub value: QosParams,
}

/// A notification callback this crate provides (its trampolines deliver [`crate::Event`]s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notify {
    /// `sai_port_state_change_notification_fn`
    PortState,
    /// `sai_fdb_event_notification_fn`
    Fdb,
    /// `sai_switch_shutdown_request_notification_fn`
    Shutdown,
    /// a null pointer
    Null,
}

/// An IP prefix (`sai_ip_prefix_t`): address and mask bytes in network order (v4: the first 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// IPv6 (else IPv4)
    pub v6: bool,
    /// the address
    pub addr: [u8; 16],
    /// the mask
    pub mask: [u8; 16],
}

/// An attribute value: the member of `sai_attribute_value_t` it travels in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Value {
    /// nothing written (a get of a member this crate does not decode)
    #[default]
    None,
    /// `booldata`
    Bool(bool),
    /// `u8`
    U8(u8),
    /// `s8`
    S8(i8),
    /// `u16`
    U16(u16),
    /// `u32`
    U32(u32),
    /// `s32`
    S32(i32),
    /// `u64`
    U64(u64),
    /// `oid`
    Oid(sys::sai_object_id_t),
    /// `mac`
    Mac([u8; 6]),
    /// `ipaddr`
    Ip(IpAddr),
    /// `chardata`: at most 31 bytes are sent (NUL-terminated)
    Chardata(Vec<u8>),
    /// `objlist`: for a get, the room (zeros)
    OidList(Vec<sys::sai_object_id_t>),
    /// `u32list`
    U32List(Vec<u32>),
    /// `s32list`
    S32List(Vec<i32>),
    /// `qosmap`
    QosMap(Vec<QosMapEntry>),
    /// `u32range` (min, max)
    U32Range(u32, u32),
    /// `aclfield` (not decoded by a get)
    AclField(AclField),
    /// `aclaction` (not decoded by a get)
    AclAction(AclAction),
    /// `ptr`: one of this crate's callbacks
    Notify(Notify),
}

/// One attribute. After a get: `value` holds what the library wrote (lists: their first
/// `min(count, room)` entries) and `count` the list count the library wrote (0 for scalars).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attr {
    /// the attribute id
    pub id: sys::sai_attr_id_t,
    /// the value
    pub value: Value,
    /// a list's count as the library wrote it
    pub count: u32,
}

impl Attr {
    /// An attribute with a value.
    pub fn new(id: sys::sai_attr_id_t, value: Value) -> Self {
        Self { id, value, count: 0 }
    }
}

/// Where an attribute's list lives in an `Encoded`.
#[derive(Clone, Copy, Debug)]
enum Store {
    None,
    Oids(usize),
    U32(usize),
    S32(usize),
    Maps(usize),
}

/// The C form of an attribute list for one call.
pub(crate) struct Encoded {
    pub(crate) raw: Vec<sys::sai_attribute_t>,
    stores: Vec<Store>,
    oids: Vec<Vec<sys::sai_object_id_t>>,
    u32s: Vec<Vec<u32>>,
    s32s: Vec<Vec<i32>>,
    maps: Vec<Vec<sys::sai_qos_map_t>>,
}

fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A list's pointer: NULL for an empty one (as an empty `std::vector`'s `data()` in the C++).
fn ptr_of<T>(v: &mut [T]) -> *mut T {
    if v.is_empty() {
        core::ptr::null_mut()
    } else {
        v.as_mut_ptr()
    }
}

fn params(p: &QosParams) -> sys::sai_qos_map_params_t {
    let mut q: sys::sai_qos_map_params_t = raw::zeroed();
    q.tc = p.tc;
    q.dscp = p.dscp;
    q.dot1p = p.dot1p;
    q.prio = p.prio;
    q.pg = p.pg;
    q.queue_index = p.queue_index;
    q.color = p.color;
    q.mpls_exp = p.mpls_exp;
    q.fc = p.fc;
    q.dei = p.dei;
    q.vc = p.vc;
    q
}

fn unparams(q: &sys::sai_qos_map_params_t) -> QosParams {
    QosParams {
        tc: q.tc,
        dscp: q.dscp,
        dot1p: q.dot1p,
        prio: q.prio,
        pg: q.pg,
        queue_index: q.queue_index,
        color: q.color,
        mpls_exp: q.mpls_exp,
        fc: q.fc,
        dei: q.dei,
        vc: q.vc,
    }
}

/// IPv4 bytes as `sai_ip4_t` holds them (network order in memory).
fn ip4(b: [u8; 4]) -> sys::sai_ip4_t {
    u32::from_ne_bytes(b)
}

/// 16 bytes for an `sai_ip_addr_t` (written as `ip6`: the whole union is initialised); IPv4 in the
/// first four (where `ip4` lives), the rest 0, as after the C programs' `memset`.
fn addr16(v4: Option<[u8; 4]>, v6: [u8; 16]) -> sys::sai_ip_addr_t {
    let ip6 = v4.map_or(v6, |b| {
        let mut x = [0u8; 16];
        x[..4].copy_from_slice(&b);
        x
    });
    sys::sai_ip_addr_t { ip6 }
}

/// `sai_ip_address_t` of an address.
pub(crate) fn ip_address(ip: IpAddr) -> sys::sai_ip_address_t {
    match ip {
        IpAddr::V4(a) => sys::sai_ip_address_t {
            addr_family: sys::SAI_IP_ADDR_FAMILY_IPV4,
            addr: addr16(Some(a.octets()), [0; 16]),
        },
        IpAddr::V6(a) => sys::sai_ip_address_t {
            addr_family: sys::SAI_IP_ADDR_FAMILY_IPV6,
            addr: addr16(None, a.octets()),
        },
    }
}

/// `sai_ip_prefix_t` of a prefix.
pub(crate) fn ip_prefix(p: &Prefix) -> sys::sai_ip_prefix_t {
    let four = |b: &[u8; 16]| [b[0], b[1], b[2], b[3]];
    if p.v6 {
        sys::sai_ip_prefix_t {
            addr_family: sys::SAI_IP_ADDR_FAMILY_IPV6,
            addr: addr16(None, p.addr),
            mask: addr16(None, p.mask),
        }
    } else {
        sys::sai_ip_prefix_t {
            addr_family: sys::SAI_IP_ADDR_FAMILY_IPV4,
            addr: addr16(Some(four(&p.addr)), [0; 16]),
            mask: addr16(Some(four(&p.mask)), [0; 16]),
        }
    }
}

// No padding in the IP structs (all of them is initialised by the constructors above).
const _: () = {
    assert!(size_of::<sys::sai_ip_address_t>() == 20);
    assert!(size_of::<sys::sai_ip_prefix_t>() == 36);
};

impl Encoded {
    pub(crate) fn new(attrs: &[Attr]) -> Self {
        let mut e = Self {
            raw: Vec::with_capacity(attrs.len()),
            stores: Vec::with_capacity(attrs.len()),
            oids: Vec::new(),
            u32s: Vec::new(),
            s32s: Vec::new(),
            maps: Vec::new(),
        };
        for a in attrs {
            let mut x: sys::sai_attribute_t = raw::zeroed();
            x.id = a.id;
            let store = e.fill(&mut x, &a.value);
            e.raw.push(x);
            e.stores.push(store);
        }
        e
    }

    fn oid_list(&mut self, v: &[sys::sai_object_id_t]) -> (usize, sys::sai_object_list_t) {
        let mut list = v.to_vec();
        let l = sys::sai_object_list_t {
            count: len32(list.len()),
            list: ptr_of(&mut list),
        };
        self.oids.push(list);
        (self.oids.len() - 1, l)
    }

    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_lines,
        reason = "x: the C attribute, u: its value union, v: the Rust value, as in sai_script.cpp"
    )]
    fn fill(&mut self, x: &mut sys::sai_attribute_t, v: &Value) -> Store {
        let u = &mut x.value;
        match v {
            Value::None => {}
            Value::Bool(b) => u.booldata = *b,
            Value::U8(n) => u.u8_ = *n,
            Value::S8(n) => u.s8 = *n,
            Value::U16(n) => u.u16_ = *n,
            Value::U32(n) => u.u32_ = *n,
            Value::S32(n) => u.s32 = *n,
            Value::U64(n) => u.u64_ = *n,
            Value::Oid(n) => u.oid = *n,
            Value::Mac(m) => u.mac = *m,
            Value::Ip(ip) => u.ipaddr = ip_address(*ip),
            Value::Chardata(s) => {
                let mut c = [0; 32];
                for (d, b) in c.iter_mut().zip(s.iter().take(31)) {
                    *d = (*b).cast_signed_char();
                }
                u.chardata = c;
            }
            Value::OidList(l) => {
                let (i, ol) = self.oid_list(l);
                u.objlist = ol;
                return Store::Oids(i);
            }
            Value::U32List(l) => {
                let mut list = l.clone();
                u.u32list = sys::sai_u32_list_t {
                    count: len32(list.len()),
                    list: ptr_of(&mut list),
                };
                self.u32s.push(list);
                return Store::U32(self.u32s.len() - 1);
            }
            Value::S32List(l) => {
                let mut list = l.clone();
                u.s32list = sys::sai_s32_list_t {
                    count: len32(list.len()),
                    list: ptr_of(&mut list),
                };
                self.s32s.push(list);
                return Store::S32(self.s32s.len() - 1);
            }
            Value::QosMap(l) => {
                let mut list: Vec<sys::sai_qos_map_t> = l
                    .iter()
                    .map(|m| {
                        let mut q: sys::sai_qos_map_t = raw::zeroed();
                        q.key = params(&m.key);
                        q.value = params(&m.value);
                        q
                    })
                    .collect();
                u.qosmap = sys::sai_qos_map_list_t {
                    count: len32(list.len()),
                    list: ptr_of(&mut list),
                };
                self.maps.push(list);
                return Store::Maps(self.maps.len() - 1);
            }
            Value::U32Range(lo, hi) => {
                u.u32range = sys::sai_u32_range_t { min: *lo, max: *hi };
            }
            Value::AclField(f) => {
                u.aclfield.enable = f.enable;
                match &f.data {
                    AclData::None => {}
                    AclData::U8(n) => u.aclfield.data.u8_ = *n,
                    AclData::U16(n) => u.aclfield.data.u16_ = *n,
                    AclData::U32(n) => u.aclfield.data.u32_ = *n,
                    AclData::S32(n) => u.aclfield.data.s32 = *n,
                    AclData::Ip4(n) => u.aclfield.data.ip4 = ip4(*n),
                    AclData::Ip6(n) => u.aclfield.data.ip6 = *n,
                    AclData::Mac(n) => u.aclfield.data.mac = *n,
                    AclData::Oid(n) => u.aclfield.data.oid = *n,
                    AclData::Oids(l) => u.aclfield.data.objlist = self.oid_list(l).1,
                }
                match &f.mask {
                    AclData::None | AclData::Oid(_) | AclData::Oids(_) => {}
                    AclData::U8(n) => u.aclfield.mask.u8_ = *n,
                    AclData::U16(n) => u.aclfield.mask.u16_ = *n,
                    AclData::U32(n) => u.aclfield.mask.u32_ = *n,
                    AclData::S32(n) => u.aclfield.mask.s32 = *n,
                    AclData::Ip4(n) => u.aclfield.mask.ip4 = ip4(*n),
                    AclData::Ip6(n) => u.aclfield.mask.ip6 = *n,
                    AclData::Mac(n) => u.aclfield.mask.mac = *n,
                }
            }
            Value::AclAction(a) => {
                u.aclaction.enable = a.enable;
                match &a.parameter {
                    AclData::None => {}
                    AclData::U8(n) => u.aclaction.parameter.u8_ = *n,
                    AclData::U16(n) => u.aclaction.parameter.u16_ = *n,
                    AclData::U32(n) => u.aclaction.parameter.u32_ = *n,
                    AclData::S32(n) => u.aclaction.parameter.s32 = *n,
                    AclData::Ip4(n) => u.aclaction.parameter.ip4 = ip4(*n),
                    AclData::Ip6(n) => u.aclaction.parameter.ip6 = *n,
                    AclData::Mac(n) => u.aclaction.parameter.mac = *n,
                    AclData::Oid(n) => u.aclaction.parameter.oid = *n,
                    AclData::Oids(l) => u.aclaction.parameter.objlist = self.oid_list(l).1,
                }
            }
            Value::Notify(n) => u.ptr = callback::address(*n),
        }
        Store::None
    }

    /// What the library wrote, as the kinds `attrs` were given as.
    pub(crate) fn decode(&self, attrs: &mut [Attr]) {
        for ((a, x), store) in attrs.iter_mut().zip(&self.raw).zip(&self.stores) {
            let b = raw::value_bytes(x);
            let count = read_u32(&b, 0);
            a.count = 0;
            a.value = match (&a.value, store) {
                (Value::Bool(_), _) => Value::Bool(b.first().is_some_and(|v| *v != 0)),
                (Value::U8(_), _) => Value::U8(b.first().copied().unwrap_or(0)),
                (Value::S8(_), _) => Value::S8(i8::from_ne_bytes([b.first().copied().unwrap_or(0)])),
                (Value::U16(_), _) => Value::U16(u16::from_ne_bytes(bytes(&b, 0))),
                (Value::U32(_), _) => Value::U32(read_u32(&b, 0)),
                (Value::S32(_), _) => Value::S32(i32::from_ne_bytes(bytes(&b, 0))),
                (Value::U64(_), _) => Value::U64(u64::from_ne_bytes(bytes(&b, 0))),
                (Value::Oid(_), _) => Value::Oid(u64::from_ne_bytes(bytes(&b, 0))),
                (Value::Mac(_), _) => Value::Mac(bytes(&b, 0)),
                (Value::U32Range(..), _) => Value::U32Range(
                    read_u32(&b, offset_of!(sys::sai_u32_range_t, min)),
                    read_u32(&b, offset_of!(sys::sai_u32_range_t, max)),
                ),
                (Value::Ip(_), _) => {
                    let fam = read_u32(&b, offset_of!(sys::sai_ip_address_t, addr_family));
                    let at = offset_of!(sys::sai_ip_address_t, addr);
                    if fam == sys::SAI_IP_ADDR_FAMILY_IPV6 {
                        Value::Ip(IpAddr::from(bytes::<16>(&b, at)))
                    } else {
                        Value::Ip(IpAddr::from(bytes::<4>(&b, at)))
                    }
                }
                (Value::Chardata(_), _) => {
                    let c: [u8; 32] = bytes(&b, 0);
                    Value::Chardata(c.iter().take_while(|v| **v != 0).copied().collect())
                }
                (Value::OidList(_), Store::Oids(i)) => {
                    a.count = count;
                    Value::OidList(head(self.oids.get(*i), count))
                }
                (Value::U32List(_), Store::U32(i)) => {
                    a.count = count;
                    Value::U32List(head(self.u32s.get(*i), count))
                }
                (Value::S32List(_), Store::S32(i)) => {
                    a.count = count;
                    Value::S32List(head(self.s32s.get(*i), count))
                }
                (Value::QosMap(_), Store::Maps(i)) => {
                    a.count = count;
                    Value::QosMap(
                        head(self.maps.get(*i), count)
                            .iter()
                            .map(|m| QosMapEntry {
                                key: unparams(&m.key),
                                value: unparams(&m.value),
                            })
                            .collect(),
                    )
                }
                (v, _) => v.clone(),
            };
        }
    }
}

// The list structs carry their count first (checked here, at compile time).
const _: () = {
    assert!(offset_of!(sys::sai_object_list_t, count) == 0);
    assert!(offset_of!(sys::sai_u32_list_t, count) == 0);
    assert!(offset_of!(sys::sai_s32_list_t, count) == 0);
    assert!(offset_of!(sys::sai_qos_map_list_t, count) == 0);
    assert!(size_of::<sys::sai_attribute_value_t>() >= 32);
};

fn head<T: Clone>(v: Option<&Vec<T>>, count: u32) -> Vec<T> {
    let n = usize::try_from(count).unwrap_or(usize::MAX);
    v.map(|v| v.iter().take(n).cloned().collect()).unwrap_or_default()
}

fn bytes<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(s) = b.get(at..at + N) {
        out.copy_from_slice(s);
    }
    out
}

fn read_u32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(bytes(b, at))
}

/// A byte as the platform's `c_char` (signed on `x86_64`, unsigned on `aarch64`).
trait AsCChar {
    fn cast_signed_char(self) -> core::ffi::c_char;
}

impl AsCChar for u8 {
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_lossless,
        reason = "c_char is i8 or u8: the byte as it is"
    )]
    fn cast_signed_char(self) -> core::ffi::c_char {
        self as core::ffi::c_char
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_point_into_the_storage_and_decode_back() {
        let attrs = vec![
            Attr::new(1, Value::U32List(vec![0; 4])),
            Attr::new(2, Value::Mac([1, 2, 3, 4, 5, 6])),
            Attr::new(3, Value::Ip("10.0.0.1".parse().unwrap())),
        ];
        let e = Encoded::new(&attrs);
        assert_eq!(e.raw.len(), 3);
        let mut out = attrs.clone();
        e.decode(&mut out);
        assert_eq!(out[0].value, Value::U32List(vec![0; 4]));
        assert_eq!(out[0].count, 4);
        assert_eq!(out[1].value, Value::Mac([1, 2, 3, 4, 5, 6]));
        assert_eq!(out[2].value, Value::Ip("10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn chardata_is_cut_at_31_bytes() {
        let attrs = vec![Attr::new(1, Value::Chardata(vec![b'a'; 40]))];
        let e = Encoded::new(&attrs);
        let mut out = attrs.clone();
        e.decode(&mut out);
        assert_eq!(out[0].value, Value::Chardata(vec![b'a'; 31]));
    }
}

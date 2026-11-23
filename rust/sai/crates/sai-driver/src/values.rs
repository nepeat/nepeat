//! Attribute values: the kind the C++ driver gives an attribute by its `@type` text (`vt_of`),
//! the script text of a value into a [`Value`] (`value_in`) and a [`Value`] into the C++ driver's
//! text (`value_out`), and the names of object ids.
use crate::names::{enum_name, enum_prefix, enum_type, enum_value};
use crate::text::{
    ParseError, Parsed, fail, hex_bytes, ip_str, ip6_str, list_items, mac_str, num, parse_ip, parse_mac, split, unquote,
};
use crate::types;
use sai_core::macsec::Secret;
use sai_core::oid::RawOid;
use sai_core::value::{
    AclAction, AclField, AclFieldData, AclKind, AclParam, FieldMatch, QosMapEntry, QosParams, Value,
};
use sai_meta::ValueType;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A list a get may fill holds this many elements (C++ `prepare_get`); a longer one is a
/// `BUFFER_OVERFLOW`. Lists print at most this many elements too.
pub const LIST_CAPACITY: usize = 256;

/// A value kind of the C++ driver other than the ACL data wrappers (`vt_simple`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Simple {
    /// `bool`
    Bool,
    /// `sai_uint8_t`
    U8,
    /// `sai_int8_t`
    I8,
    /// `sai_uint16_t`
    U16,
    /// `sai_int16_t`
    I16,
    /// `sai_uint32_t`
    U32,
    /// `sai_int32_t`
    I32,
    /// `sai_uint64_t`
    U64,
    /// `sai_int64_t`
    I64,
    /// An enum type of the headers.
    Enum(&'static str),
    /// `sai_mac_t`
    Mac,
    /// `sai_ip_address_t`
    Ip,
    /// `sai_ip4_t`
    Ip4,
    /// `sai_ip6_t`
    Ip6,
    /// `sai_object_id_t`
    Oid,
    /// `sai_object_list_t`
    OidList,
    /// `sai_u8_list_t`
    U8List,
    /// `sai_s8_list_t`
    I8List,
    /// `sai_u16_list_t`
    U16List,
    /// `sai_s16_list_t`
    I16List,
    /// `sai_u32_list_t`
    U32List,
    /// `sai_s32_list_t`, of enumerators of a type when the `@type` names one
    I32List(Option<&'static str>),
    /// `sai_vlan_list_t`
    VlanList,
    /// `sai_u32_range_t`
    U32Range,
    /// `char`
    Chars,
    /// `sai_qos_map_list_t`
    QosMapList,
    /// `sai_macsec_sak_t`
    MacsecSak,
    /// `sai_macsec_auth_key_t`
    MacsecAuthKey,
    /// `sai_macsec_salt_t`
    MacsecSalt,
}

/// The kind of an attribute's value (C++ `VT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A plain value.
    Simple(Simple),
    /// `sai_acl_field_data_t <T>`; `None`: a `<T>` the driver has no kind for.
    AclField(Option<Simple>),
    /// `sai_acl_action_data_t <T>`.
    AclAction(Option<Simple>),
}

/// C++ `vt_simple`.
fn simple(t: &str) -> Option<Simple> {
    Some(match t {
        "bool" => Simple::Bool,
        "sai_uint8_t" => Simple::U8,
        "sai_int8_t" => Simple::I8,
        "sai_uint16_t" => Simple::U16,
        "sai_int16_t" => Simple::I16,
        "sai_uint32_t" => Simple::U32,
        "sai_int32_t" => Simple::I32,
        "sai_uint64_t" => Simple::U64,
        "sai_int64_t" => Simple::I64,
        "sai_mac_t" => Simple::Mac,
        "sai_ip_address_t" => Simple::Ip,
        "sai_ip4_t" => Simple::Ip4,
        "sai_ip6_t" => Simple::Ip6,
        "sai_object_id_t" => Simple::Oid,
        "sai_object_list_t" => Simple::OidList,
        "sai_u8_list_t" => Simple::U8List,
        "sai_s8_list_t" => Simple::I8List,
        "sai_u16_list_t" => Simple::U16List,
        "sai_s16_list_t" => Simple::I16List,
        "sai_u32_list_t" => Simple::U32List,
        "sai_s32_list_t" => Simple::I32List(None),
        "sai_vlan_list_t" => Simple::VlanList,
        "sai_u32_range_t" => Simple::U32Range,
        "char" => Simple::Chars,
        "sai_qos_map_list_t" => Simple::QosMapList,
        "sai_macsec_sak_t" => Simple::MacsecSak,
        "sai_macsec_auth_key_t" => Simple::MacsecAuthKey,
        "sai_macsec_salt_t" => Simple::MacsecSalt,
        other => Simple::Enum(enum_type(other)?),
    })
}

/// The kind of an attribute by its `@type` text (C++ `vt_of`); `None`: "unsupported type".
pub fn kind_of(type_text: &str) -> Option<Kind> {
    let mut words = type_text.split_whitespace();
    let a = words.next().unwrap_or_default();
    let b = words.next().unwrap_or_default();
    match a {
        "sai_acl_field_data_t" => Some(Kind::AclField(simple(b))),
        "sai_acl_action_data_t" => Some(Kind::AclAction(simple(b))),
        _ => match simple(a)? {
            Simple::I32List(None) => Some(Kind::Simple(Simple::I32List(enum_type(b)))),
            s => Some(Kind::Simple(s)),
        },
    }
}

/// Whether a get of this kind fills a list (C++ `is_list`).
pub const fn is_list(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Simple(
            Simple::OidList
                | Simple::U8List
                | Simple::I8List
                | Simple::U16List
                | Simple::I16List
                | Simple::U32List
                | Simple::I32List(_)
                | Simple::VlanList
                | Simple::QosMapList
        ) | Kind::AclField(Some(Simple::OidList))
            | Kind::AclAction(Some(Simple::OidList))
    )
}

/// The element count of a list value (what the rim writes back, `ptr::write_list`); `None` for a
/// value that is no list (a disabled ACL field or action: its list member keeps the caller's
/// capacity).
pub fn list_count(value: &Value) -> Option<usize> {
    Some(match value {
        Value::OidList(v)
        | Value::AclField(AclField::Enabled(AclFieldData::OidList(v)))
        | Value::AclAction(AclAction::Enabled(AclParam::OidList(v))) => v.len(),
        Value::U8List(v) => v.len(),
        Value::I8List(v) => v.len(),
        Value::U16List(v) | Value::VlanList(v) => v.len(),
        Value::I16List(v) => v.len(),
        Value::U32List(v) => v.len(),
        Value::I32List(v) => v.len(),
        Value::QosMapList(v) => v.len(),
        _ => return None,
    })
}

/// Object names: the script's, the switch's, and `<type>#<n>` for ids first seen in an answer.
#[derive(Clone, Debug, Default)]
pub struct Names {
    by_name: BTreeMap<String, RawOid>,
    by_oid: BTreeMap<RawOid, String>,
    auto: BTreeMap<u32, u32>,
}

impl Names {
    /// Binds a name (C++ `bind_name`: an id keeps the first name it got).
    pub fn bind(&mut self, name: &str, oid: RawOid) {
        self.by_name.insert(name.to_owned(), oid);
        self.by_oid.entry(oid).or_insert_with(|| name.to_owned());
    }

    /// Forgets every name (the switch was removed).
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    /// Whether a name is bound.
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The id of a name; `null` is the null id (C++ `oid_named`).
    pub fn oid(&self, name: &str) -> Parsed<RawOid> {
        if name == "null" {
            return Ok(RawOid::NULL);
        }
        self.by_name
            .get(name)
            .copied()
            .ok_or_else(|| ParseError(format!("unknown name '{name}'")))
    }

    /// The name of an id, made up on first sight (C++ `oid_name`).
    pub fn name(&mut self, oid: RawOid) -> String {
        if oid.is_null() {
            return "null".to_owned();
        }
        if let Some(n) = self.by_oid.get(&oid) {
            return n.clone();
        }
        let ot = oid.sai_object_type();
        let base = types::of(ot).map_or_else(|| format!("oid{ot}"), |t| t.name.to_owned());
        let n = self.auto.entry(ot).or_insert(0);
        *n += 1;
        let name = format!("{base}#{n}");
        self.bind(&name, oid);
        name
    }
}

/// An enumerator by its short name, full name or number (C++ `enum_in`).
pub fn enum_in(ty: &str, s: &str) -> Parsed<i64> {
    if s.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        return num(s);
    }
    enum_value(ty, &format!("{}{s}", enum_prefix(ty)))
        .or_else(|| enum_value(ty, s))
        .ok_or_else(|| ParseError(format!("'{s}' is not a {ty}")))
}

// The C++ driver stores numbers with C casts (`(uint8_t)num(s)`): the same truncation here.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the C driver's casts"
)]
mod cast {
    pub(super) const fn u8(v: i64) -> u8 {
        v as u8
    }
    pub(super) const fn i8(v: i64) -> i8 {
        v as i8
    }
    pub(super) const fn u16(v: i64) -> u16 {
        v as u16
    }
    pub(super) const fn i16(v: i64) -> i16 {
        v as i16
    }
    pub(super) const fn u32(v: i64) -> u32 {
        v as u32
    }
    pub(super) const fn i32(v: i64) -> i32 {
        v as i32
    }
    pub(super) const fn u64(v: i64) -> u64 {
        v as u64
    }
}

fn list_in<T>(s: &str, item: impl Fn(&str) -> Parsed<T>) -> Parsed<Vec<T>> {
    list_items(s)?.iter().map(|i| item(i)).collect()
}

/// `sai_ip4_t` from an address text: an IPv6 text gives its first four bytes (the C++ driver
/// reads `addr.ip4` of the union).
fn ip4_in(s: &str) -> Parsed<Ipv4Addr> {
    Ok(match parse_ip(s)? {
        IpAddr::V4(a) => a,
        IpAddr::V6(a) => {
            let [b0, b1, b2, b3, ..] = a.octets();
            Ipv4Addr::new(b0, b1, b2, b3)
        }
    })
}

/// `sai_ip6_t` from an address text: an IPv4 text gives its four bytes and twelve zeros.
fn ip6_in(s: &str) -> Parsed<Ipv6Addr> {
    Ok(match parse_ip(s)? {
        IpAddr::V6(a) => a,
        IpAddr::V4(a) => {
            let mut b = [0u8; 16];
            for (d, s) in b.iter_mut().zip(a.octets()) {
                *d = s;
            }
            Ipv6Addr::from(b)
        }
    })
}

/// `tc:5/color:1` (C++ `qos_params`; fields not given are 0).
fn qos_params(s: &str) -> Parsed<QosParams> {
    let mut p = QosParams::default();
    for f in split(s, '/') {
        let Some((k, v)) = f.split_once(':') else {
            return fail(format!("bad QoS map field '{f}'"));
        };
        let v = num(v)?;
        match k {
            "tc" => p.tc = cast::u8(v),
            "dscp" => p.dscp = cast::u8(v),
            "dot1p" => p.dot1p = cast::u8(v),
            "prio" => p.prio = cast::u8(v),
            "pg" => p.pg = cast::u8(v),
            "queue" => p.queue_index = cast::u8(v),
            "color" => p.color = cast::i32(v),
            "mpls_exp" => p.mpls_exp = cast::u8(v),
            "fc" => p.fc = cast::u8(v),
            _ => return fail(format!("unknown QoS map field '{k}'")),
        }
    }
    Ok(p)
}

/// `N` bytes of key material from `2N` hex digits, straight into a [`Secret`].
fn secret<const N: usize>(s: &str) -> Parsed<Secret<N>> {
    let mut bytes = hex_bytes(s, N)?;
    let secret = Secret::from_slice(&bytes).ok_or_else(|| ParseError(format!("expected {N} hex bytes")));
    zeroize::Zeroize::zeroize(&mut bytes);
    secret
}

/// The value of a script text, by the attribute's kind (C++ `value_in`). `Ok(None)`: the C++
/// driver could encode it, but a Rust [`Value`] has no form for it (ACL data of 8 / 16-bit
/// signed members): the rim decodes no such member either (`ValueType::Other`).
/// `travel` is the member the attribute travels in (disabled ACL data keep only that).
pub fn value_in(kind: Kind, travel: ValueType, s: &str, names: &Names) -> Parsed<Option<Value>> {
    match kind {
        Kind::Simple(k) => simple_in(k, s, names),
        Kind::AclField(sub) => field_in(sub, travel, s, names),
        Kind::AclAction(sub) => action_in(sub, travel, s, names),
    }
}

fn simple_in(k: Simple, s: &str, names: &Names) -> Parsed<Option<Value>> {
    Ok(Some(match k {
        Simple::Bool => match s {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return fail("expected true / false"),
        },
        Simple::U8 => Value::U8(cast::u8(num(s)?)),
        Simple::I8 => Value::I8(cast::i8(num(s)?)),
        Simple::U16 => Value::U16(cast::u16(num(s)?)),
        Simple::I16 => Value::I16(cast::i16(num(s)?)),
        Simple::U32 => Value::U32(cast::u32(num(s)?)),
        Simple::I32 => Value::I32(cast::i32(num(s)?)),
        Simple::U64 => Value::U64(cast::u64(num(s)?)),
        Simple::I64 => Value::I64(num(s)?),
        Simple::Enum(t) => Value::Enum(cast::i32(enum_in(t, s)?)),
        Simple::Mac => Value::Mac(parse_mac(s)?),
        Simple::Ip => Value::IpAddress(parse_ip(s)?),
        Simple::Ip4 => Value::Ip4(ip4_in(s)?),
        Simple::Ip6 => Value::Ip6(ip6_in(s)?),
        Simple::Oid => Value::Oid(names.oid(s)?),
        Simple::OidList => Value::OidList(list_in(s, |i| names.oid(i))?),
        Simple::U8List => Value::U8List(list_in(s, |i| num(i).map(cast::u8))?),
        Simple::I8List => Value::I8List(list_in(s, |i| num(i).map(cast::i8))?),
        Simple::U16List => Value::U16List(list_in(s, |i| num(i).map(cast::u16))?),
        Simple::I16List => Value::I16List(list_in(s, |i| num(i).map(cast::i16))?),
        Simple::U32List => Value::U32List(list_in(s, |i| num(i).map(cast::u32))?),
        Simple::I32List(None) => Value::I32List(list_in(s, |i| num(i).map(cast::i32))?),
        Simple::I32List(Some(t)) => Value::I32List(list_in(s, |i| enum_in(t, i).map(cast::i32))?),
        Simple::VlanList => Value::VlanList(list_in(s, |i| num(i).map(cast::u16))?),
        Simple::U32Range => {
            let Some((min, max)) = s.split_once('-') else {
                return fail("expected <min>-<max>");
            };
            Value::U32Range {
                min: cast::u32(num(min)?),
                max: cast::u32(num(max)?),
            }
        }
        Simple::Chars => {
            let text = unquote(s);
            if text.len() >= 32 {
                return fail("text too long");
            }
            Value::CharData(text.to_owned())
        }
        Simple::QosMapList => Value::QosMapList(list_in(s, |item| {
            let Some((key, value)) = item.split_once('=') else {
                return fail("QoS map entry without '='");
            };
            Ok(QosMapEntry {
                key: qos_params(key)?,
                value: qos_params(value)?,
            })
        })?),
        // key material: into a Secret at once (the driver's own copy is wiped with the Vec)
        Simple::MacsecSak => Value::MacsecSak(secret(s)?),
        Simple::MacsecAuthKey => Value::MacsecAuthKey(secret(s)?),
        Simple::MacsecSalt => Value::MacsecSalt(secret(s)?),
    }))
}

/// Data and mask from their texts; a mask not given is the type's zero (the C++ driver's memset).
fn masked<T: Default>(data: &str, mask: &str, parse: impl Fn(&str) -> Parsed<T>) -> Parsed<FieldMatch<T>> {
    Ok(FieldMatch {
        data: parse(data)?,
        mask: if mask.is_empty() { T::default() } else { parse(mask)? },
    })
}

/// The data and mask of an ACL field (C++ `field_data_in`): `off`, `<data>` or `<data>/<mask>`
/// (a mask not given is 0).
fn field_in(sub: Option<Simple>, travel: ValueType, text: &str, names: &Names) -> Parsed<Option<Value>> {
    if text == "off" {
        return Ok(match travel {
            ValueType::AclField(k) => Some(Value::AclField(AclField::Disabled(k))),
            _ => None,
        });
    }
    let (data, mask) = match text.split_once('/') {
        Some(pair) if sub != Some(Simple::OidList) => pair,
        _ => (text, ""),
    };
    let field = match sub {
        Some(Simple::Bool) => AclFieldData::Bool(data == "true"),
        Some(Simple::U8) => AclFieldData::U8(masked(data, mask, |x| num(x).map(cast::u8))?),
        Some(Simple::U16) => AclFieldData::U16(masked(data, mask, |x| num(x).map(cast::u16))?),
        Some(Simple::U32) => AclFieldData::U32(masked(data, mask, |x| num(x).map(cast::u32))?),
        Some(Simple::I32) => AclFieldData::I32(masked(data, mask, |x| num(x).map(cast::i32))?),
        Some(Simple::Enum(ty)) => AclFieldData::Enum(FieldMatch {
            data: cast::i32(enum_in(ty, data)?),
            mask: if mask.is_empty() { 0 } else { cast::i32(num(mask)?) },
        }),
        Some(Simple::U64) => AclFieldData::U64(masked(data, mask, |x| num(x).map(cast::u64))?),
        Some(Simple::Mac) => AclFieldData::Mac(masked(data, mask, parse_mac)?),
        Some(Simple::Ip4) => AclFieldData::Ip4(FieldMatch {
            data: ip4_in(data)?,
            mask: if mask.is_empty() {
                Ipv4Addr::UNSPECIFIED
            } else {
                ip4_in(mask)?
            },
        }),
        Some(Simple::Ip6) => AclFieldData::Ip6(FieldMatch {
            data: ip6_in(data)?,
            mask: if mask.is_empty() {
                Ipv6Addr::UNSPECIFIED
            } else {
                ip6_in(mask)?
            },
        }),
        Some(Simple::Oid) => AclFieldData::Oid(names.oid(data)?),
        Some(Simple::OidList) => AclFieldData::OidList(list_in(data, |i| names.oid(i))?),
        // parsed as the C++ driver does; no Rust member
        Some(Simple::I8) => return masked(data, mask, |x| num(x).map(cast::i8)).map(|_| None),
        Some(Simple::I16) => return masked(data, mask, |x| num(x).map(cast::i16)).map(|_| None),
        _ => return fail("unsupported ACL field type"),
    };
    Ok(Some(Value::AclField(AclField::Enabled(field))))
}

/// The zero parameter of an enabled action (`on`: the C++ driver leaves the union zeroed).
fn zero_param(kind: AclKind) -> Option<AclParam> {
    Some(match kind {
        AclKind::Bool => AclParam::Bool(false),
        AclKind::U8 => AclParam::U8(0),
        AclKind::U16 => AclParam::U16(0),
        AclKind::U32 => AclParam::U32(0),
        AclKind::I32 => AclParam::I32(0),
        AclKind::Enum => AclParam::Enum(0),
        AclKind::Mac => AclParam::Mac([0; 6]),
        AclKind::Ip4 => AclParam::Ip4(Ipv4Addr::UNSPECIFIED),
        AclKind::Ip6 => AclParam::Ip6(Ipv6Addr::UNSPECIFIED),
        AclKind::Oid => AclParam::Oid(RawOid::NULL),
        AclKind::OidList => AclParam::OidList(Vec::new()),
        AclKind::IpAddress => AclParam::IpAddress(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        AclKind::U64 | AclKind::U8List => return None,
    })
}

/// An ACL action (C++ `action_in`): `off`, `on` or `<parameter>`.
fn action_in(sub: Option<Simple>, travel: ValueType, s: &str, names: &Names) -> Parsed<Option<Value>> {
    let ValueType::AclAction(kind) = travel else {
        // parsed for the C++ driver's errors, then refused by the rim
        return action_in(sub, ValueType::AclAction(AclKind::Bool), s, names).map(|_| None);
    };
    if s == "off" {
        return Ok(Some(Value::AclAction(AclAction::Disabled(kind))));
    }
    if s == "on" {
        return Ok(zero_param(kind).map(|p| Value::AclAction(AclAction::Enabled(p))));
    }
    let param = match sub {
        Some(Simple::Bool) => AclParam::Bool(s == "true"),
        Some(Simple::U8) => AclParam::U8(cast::u8(num(s)?)),
        Some(Simple::U16) => AclParam::U16(cast::u16(num(s)?)),
        Some(Simple::U32) => AclParam::U32(cast::u32(num(s)?)),
        Some(Simple::I32) => AclParam::I32(cast::i32(num(s)?)),
        Some(Simple::Enum(t)) => AclParam::Enum(cast::i32(enum_in(t, s)?)),
        Some(Simple::Mac) => AclParam::Mac(parse_mac(s)?),
        Some(Simple::Ip4) => AclParam::Ip4(ip4_in(s)?),
        Some(Simple::Ip) => AclParam::IpAddress(parse_ip(s)?),
        Some(Simple::Oid) => AclParam::Oid(names.oid(s)?),
        Some(Simple::OidList) => AclParam::OidList(list_in(s, |i| names.oid(i))?),
        Some(Simple::I8 | Simple::I16) => return num(s).map(|_| None),
        _ => return fail("unsupported ACL action type"),
    };
    Ok(Some(Value::AclAction(AclAction::Enabled(param))))
}

fn list_out<T>(items: &[T], f: impl FnMut(&T) -> String) -> String {
    let shown: Vec<String> = items.iter().take(LIST_CAPACITY).map(f).collect();
    format!("[{}]", shown.join(","))
}

/// `tc.dscp.dot1p.prio.pg.queue.color.mpls_exp.fc` (C++ `qos_params_out`).
fn qos_out(p: &QosParams) -> String {
    format!(
        "{}.{}.{}.{}.{}.{}.{}.{}.{}",
        p.tc, p.dscp, p.dot1p, p.prio, p.pg, p.queue_index, p.color, p.mpls_exp, p.fc
    )
}

const UNPRINTABLE: &str = "<unprintable>";

/// A value as the C++ driver prints it (`value_out`), by the attribute's kind. A value of another
/// member than the kind's (the core checks that it answers the attribute's member, so this means
/// a driver or metadata disagreement) prints as `<unprintable>`.
pub fn value_out(kind: Kind, value: &Value, names: &mut Names) -> String {
    match (kind, value) {
        (Kind::AclField(sub), Value::AclField(f)) => field_out(sub, f, names),
        (Kind::AclAction(sub), Value::AclAction(a)) => action_out(sub, a, names),
        (Kind::Simple(k), v) => simple_out(k, v, names),
        _ => UNPRINTABLE.to_owned(),
    }
}

fn simple_out(k: Simple, v: &Value, names: &mut Names) -> String {
    match (k, v) {
        (Simple::Bool, Value::Bool(b)) => b.to_string(),
        (Simple::U8, Value::U8(x)) => x.to_string(),
        (Simple::I8, Value::I8(x)) => x.to_string(),
        (Simple::U16, Value::U16(x)) => x.to_string(),
        (Simple::I16, Value::I16(x)) => x.to_string(),
        (Simple::U32, Value::U32(x)) => x.to_string(),
        (Simple::I32, Value::I32(x)) => x.to_string(),
        (Simple::U64, Value::U64(x)) => x.to_string(),
        (Simple::I64, Value::I64(x)) => x.to_string(),
        (Simple::Enum(t), Value::Enum(x)) => enum_name(t, i64::from(*x)),
        (Simple::Mac, Value::Mac(m)) => mac_str(m),
        (Simple::Ip, Value::IpAddress(ip)) => ip_str(ip),
        (Simple::Ip4, Value::Ip4(a)) => a.to_string(),
        (Simple::Ip6, Value::Ip6(a)) => ip6_str(a),
        (Simple::Oid, Value::Oid(o)) => names.name(*o),
        (Simple::OidList, Value::OidList(l)) => list_out(l, |o| names.name(*o)),
        (Simple::U8List, Value::U8List(l)) => list_out(l, ToString::to_string),
        (Simple::I8List, Value::I8List(l)) => list_out(l, ToString::to_string),
        (Simple::U16List, Value::U16List(l)) | (Simple::VlanList, Value::VlanList(l)) => {
            list_out(l, ToString::to_string)
        }
        (Simple::I16List, Value::I16List(l)) => list_out(l, ToString::to_string),
        (Simple::U32List, Value::U32List(l)) => list_out(l, ToString::to_string),
        (Simple::I32List(None), Value::I32List(l)) => list_out(l, ToString::to_string),
        (Simple::I32List(Some(t)), Value::I32List(l)) => list_out(l, |x| enum_name(t, i64::from(*x))),
        (Simple::U32Range, Value::U32Range { min, max }) => format!("{min}-{max}"),
        (Simple::Chars, Value::CharData(s)) => {
            // the rim's encode: at most 31 bytes, NUL-terminated
            let bytes: Vec<u8> = s.bytes().take(31).take_while(|b| *b != 0).collect();
            format!("\"{}\"", String::from_utf8_lossy(&bytes))
        }
        (Simple::QosMapList, Value::QosMapList(l)) => {
            list_out(l, |e| format!("{}={}", qos_out(&e.key), qos_out(&e.value)))
        }
        _ => UNPRINTABLE.to_owned(),
    }
}

/// `<data>/<mask>`.
fn dm<T: ToString>(m: &FieldMatch<T>) -> String {
    format!("{}/{}", m.data.to_string(), m.mask.to_string())
}

fn field_out(sub: Option<Simple>, f: &AclField, names: &mut Names) -> String {
    let AclField::Enabled(data) = f else {
        return "off".to_owned();
    };
    match (sub, data) {
        (Some(Simple::Bool), AclFieldData::Bool(b)) => b.to_string(),
        (Some(Simple::U8), AclFieldData::U8(m)) => dm(m),
        (Some(Simple::U16), AclFieldData::U16(m)) => dm(m),
        (Some(Simple::U32), AclFieldData::U32(m)) => dm(m),
        (Some(Simple::I32), AclFieldData::I32(m)) => dm(m),
        (Some(Simple::Enum(t)), AclFieldData::Enum(m)) => enum_name(t, i64::from(m.data)),
        (Some(Simple::U64), AclFieldData::U64(m)) => dm(m),
        (Some(Simple::Mac), AclFieldData::Mac(m)) => format!("{}/{}", mac_str(&m.data), mac_str(&m.mask)),
        (Some(Simple::Ip4), AclFieldData::Ip4(m)) => dm(m),
        (Some(Simple::Ip6), AclFieldData::Ip6(m)) => format!("{}/{}", ip6_str(&m.data), ip6_str(&m.mask)),
        (Some(Simple::Oid), AclFieldData::Oid(o)) => names.name(*o),
        (Some(Simple::OidList), AclFieldData::OidList(l)) => list_out(l, |o| names.name(*o)),
        _ => UNPRINTABLE.to_owned(),
    }
}

fn action_out(sub: Option<Simple>, a: &AclAction, names: &mut Names) -> String {
    let AclAction::Enabled(p) = a else {
        return "off".to_owned();
    };
    match (sub, p) {
        (None, _) => "on".to_owned(),
        (Some(Simple::Bool), AclParam::Bool(b)) => b.to_string(),
        (Some(Simple::U8), AclParam::U8(x)) => x.to_string(),
        (Some(Simple::U16), AclParam::U16(x)) => x.to_string(),
        (Some(Simple::U32), AclParam::U32(x)) => x.to_string(),
        (Some(Simple::I32), AclParam::I32(x)) => x.to_string(),
        (Some(Simple::Enum(t)), AclParam::Enum(x)) => enum_name(t, i64::from(*x)),
        (Some(Simple::Mac), AclParam::Mac(m)) => mac_str(m),
        (Some(Simple::Ip4), AclParam::Ip4(x)) => x.to_string(),
        (Some(Simple::Ip), AclParam::IpAddress(ip)) => ip_str(ip),
        (Some(Simple::Oid), AclParam::Oid(o)) => names.name(*o),
        (Some(Simple::OidList), AclParam::OidList(l)) => list_out(l, |o| names.name(*o)),
        _ => UNPRINTABLE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_vt_of() {
        assert_eq!(kind_of("bool"), Some(Kind::Simple(Simple::Bool)));
        assert_eq!(
            kind_of("sai_vlan_tagging_mode_t"),
            Some(Kind::Simple(Simple::Enum("sai_vlan_tagging_mode_t")))
        );
        assert_eq!(
            kind_of("sai_s32_list_t sai_port_fec_mode_t"),
            Some(Kind::Simple(Simple::I32List(Some("sai_port_fec_mode_t"))))
        );
        assert_eq!(kind_of("sai_s32_list_t"), Some(Kind::Simple(Simple::I32List(None))));
        assert_eq!(
            kind_of("sai_acl_field_data_t sai_ip4_t"),
            Some(Kind::AclField(Some(Simple::Ip4)))
        );
        assert_eq!(
            kind_of("sai_acl_action_data_t sai_nonsense_t"),
            Some(Kind::AclAction(None))
        );
        assert_eq!(kind_of("sai_pointer_t"), None);
        assert_eq!(kind_of("sai_ip_prefix_t"), None);
        assert!(is_list(Kind::Simple(Simple::QosMapList)));
        assert!(!is_list(Kind::Simple(Simple::U32Range)));
    }

    #[test]
    fn values_round_trip_through_the_text() {
        let mut names = Names::default();
        names.bind("port0", RawOid((u64::from(sai_sys::SAI_OBJECT_TYPE_PORT) << 56) | 1));
        let k = |t: &str| kind_of(t).unwrap();
        let io = |t: &str, travel, s: &str| {
            let v = value_in(k(t), travel, s, &names).unwrap().unwrap();
            value_out(k(t), &v, &mut names.clone())
        };
        assert_eq!(io("sai_vlan_tagging_mode_t", ValueType::Enum, "TAGGED"), "TAGGED");
        assert_eq!(
            io(
                "sai_vlan_tagging_mode_t",
                ValueType::Enum,
                "SAI_VLAN_TAGGING_MODE_UNTAGGED"
            ),
            "UNTAGGED"
        );
        assert_eq!(io("sai_vlan_tagging_mode_t", ValueType::Enum, "2"), "PRIORITY_TAGGED");
        assert_eq!(io("sai_uint8_t", ValueType::U8, "0x1ff"), "255");
        assert_eq!(
            io("sai_mac_t", ValueType::Mac, "02:11:22:33:44:55"),
            "02:11:22:33:44:55"
        );
        assert_eq!(
            io("sai_ip_address_t", ValueType::IpAddress, "2001:db8::1"),
            "2001:db8:0:0:0:0:0:1"
        );
        assert_eq!(
            io("sai_object_list_t", ValueType::OidList, "[port0,null]"),
            "[port0,null]"
        );
        assert_eq!(io("sai_u32_range_t", ValueType::U32Range, "5-9"), "5-9");
        assert_eq!(io("char", ValueType::CharData, "\"Ethernet 0\""), "\"Ethernet 0\"");
        assert_eq!(
            io(
                "sai_qos_map_list_t",
                ValueType::QosMapList,
                "[dscp:46=tc:5,tc:2/color:1=dscp:12]"
            ),
            "[0.46.0.0.0.0.0.0.0=5.0.0.0.0.0.0.0.0,2.0.0.0.0.0.1.0.0=0.12.0.0.0.0.0.0.0]"
        );
        assert_eq!(
            io(
                "sai_acl_field_data_t sai_ip4_t",
                ValueType::AclField(AclKind::Ip4),
                "10.0.0.1/255.255.255.255"
            ),
            "10.0.0.1/255.255.255.255"
        );
        assert_eq!(
            io(
                "sai_acl_field_data_t sai_uint16_t",
                ValueType::AclField(AclKind::U16),
                "2048"
            ),
            "2048/0"
        );
        assert_eq!(
            io(
                "sai_acl_field_data_t sai_uint16_t",
                ValueType::AclField(AclKind::U16),
                "off"
            ),
            "off"
        );
        assert_eq!(
            io(
                "sai_acl_action_data_t sai_packet_action_t",
                ValueType::AclAction(AclKind::Enum),
                "DROP"
            ),
            "DROP"
        );
        assert_eq!(
            io(
                "sai_acl_action_data_t sai_packet_action_t",
                ValueType::AclAction(AclKind::Enum),
                "on"
            ),
            "DROP"
        );
    }

    #[test]
    fn script_errors_read_like_the_cpp_driver() {
        let names = Names::default();
        let k = |t: &str| kind_of(t).unwrap();
        let err = |t: &str, s: &str| value_in(k(t), ValueType::Bool, s, &names).unwrap_err().0;
        assert_eq!(err("bool", "yes"), "expected true / false");
        assert_eq!(
            err("sai_vlan_tagging_mode_t", "SOMETIMES"),
            "'SOMETIMES' is not a sai_vlan_tagging_mode_t"
        );
        assert_eq!(err("sai_object_id_t", "nobody"), "unknown name 'nobody'");
        assert_eq!(err("sai_u32_range_t", "5"), "expected <min>-<max>");
        assert_eq!(err("char", "0123456789012345678901234567890123"), "text too long");
        assert_eq!(err("sai_object_list_t", "port0"), "expected a list [..], got 'port0'");
        // a MACsec key parses into a secret of its size
        assert!(matches!(
            value_in(k("sai_macsec_salt_t"), ValueType::MacsecSalt, &"00".repeat(12), &names),
            Ok(Some(Value::MacsecSalt(_)))
        ));
        assert!(value_in(k("sai_macsec_sak_t"), ValueType::MacsecSak, &"00".repeat(12), &names).is_err());
    }

    #[test]
    fn unknown_ids_are_named_by_type_in_order_of_sight() {
        let mut names = Names::default();
        let q = |i: u64| RawOid((u64::from(sai_sys::SAI_OBJECT_TYPE_QUEUE) << 56) | i);
        assert_eq!(names.name(q(7)), "queue#1");
        assert_eq!(names.name(q(3)), "queue#2");
        assert_eq!(names.name(q(7)), "queue#1");
        assert_eq!(names.oid("queue#2"), Ok(q(3)));
        assert_eq!(names.name(RawOid::NULL), "null");
        names.forget();
        assert_eq!(names.name(q(3)), "queue#1");
    }
}

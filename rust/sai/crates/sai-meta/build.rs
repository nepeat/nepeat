//! Reads the attribute annotations of the SAI headers (the `@type`, `@flags`, `@objects`,
//! `@allownull`, `@condition` / `@validonly` tags in the comment above each attribute
//! enumerator: the source SAI's own metadata generator starts from) and writes one table per
//! object type. Attribute ids and object types are not computed here: the generated code
//! names the `sai-sys` constants, so the numbers are the C compiler's.
// A build script reports a failure by panicking with a message.
#![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

#[derive(Default)]
struct Tags {
    ty: Option<String>,
    flags: Vec<String>,
    objects: Vec<String>,
    allow_null: bool,
    conditional: bool,
    ignore: bool,
    default: Option<String>,
}

struct Attr {
    name: String,
    tags: Tags,
}

fn main() {
    // DEP_SAI_HEADERS_INCLUDE: exported by sai-sys's build script (links = "sai-headers").
    let inc = PathBuf::from(env::var_os("DEP_SAI_HEADERS_INCLUDE").expect("sai-sys exports the header directory"));
    println!("cargo:rerun-if-changed={}", inc.display());
    let mut headers: Vec<PathBuf> = fs::read_dir(&inc)
        .expect("SAI include directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "h"))
        .collect();
    headers.sort();
    let sources: Vec<String> = headers.iter().map(|p| fs::read_to_string(p).expect("header")).collect();

    // Pass 1: enum types with their enumerators (an attribute of such a type travels as s32;
    // the enumerators are its valid values) and object types.
    let mut enums: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut object_types = Vec::new();
    for src in &sources {
        let mut open: Option<Vec<String>> = None;
        for line in src.lines() {
            let line = line.trim();
            if line.starts_with("typedef enum _sai_") {
                open = Some(Vec::new());
            } else if let Some(name) = line.strip_prefix("} sai_").and_then(|r| r.strip_suffix("_t;")) {
                if let Some(values) = open.take() {
                    enums.insert(format!("sai_{name}_t"), values);
                }
            } else if let Some(values) = open.as_mut() {
                let name: String = line
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                // Range markers are bounds of the enum, not values an attribute can take.
                let marker = name.ends_with("_START")
                    || name.ends_with("_END")
                    || name.ends_with("_MAX")
                    || name.ends_with("_RANGE_BASE")
                    || name.contains("CUSTOM_RANGE");
                if name.starts_with("SAI_") && !marker && !values.contains(&name) {
                    values.push(name);
                }
            }
            if let Some(rest) = line.strip_prefix("SAI_OBJECT_TYPE_") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                let full = format!("SAI_OBJECT_TYPE_{name}");
                if !matches!(
                    name.as_str(),
                    "MAX" | "CUSTOM_RANGE_START" | "CUSTOM_RANGE_END" | "EXTENSIONS_RANGE_START"
                ) && !rest.contains("= SAI_OBJECT_TYPE_")
                    && !object_types.contains(&full)
                {
                    object_types.push(full);
                }
            }
        }
    }

    // Pass 2: attribute enums.
    let mut tables: BTreeMap<String, Vec<Attr>> = BTreeMap::new();
    let mut unmapped = Vec::new();
    for src in &sources {
        let mut current: Option<(String, Vec<Attr>)> = None;
        let mut tags = Tags::default();
        let mut in_comment = false;
        for line in src.lines() {
            let line = line.trim();
            if let Some(name) = line
                .strip_prefix("typedef enum _sai_")
                .and_then(|r| r.strip_suffix("_attr_t"))
            {
                current = Some((name.to_uppercase(), Vec::new()));
                tags = Tags::default();
                continue;
            }
            let Some((object, attrs)) = current.as_mut() else {
                continue;
            };
            if line.starts_with("} sai_") {
                let object_type = format!("SAI_OBJECT_TYPE_{object}");
                if let Some((_, attrs)) = current.take() {
                    if object_types.contains(&object_type) {
                        tables.insert(object_type, attrs);
                    } else {
                        unmapped.push(object_type);
                    }
                }
                continue;
            }
            if line.starts_with("/**") {
                in_comment = true;
                tags = Tags::default();
            }
            if in_comment {
                parse_tag(line, &mut tags);
                if line.ends_with("*/") {
                    in_comment = false;
                }
                continue;
            }
            let name: String = line
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if name.starts_with(&format!("SAI_{object}_ATTR_")) {
                let tags = std::mem::take(&mut tags);
                if tags.ty.is_some() && !tags.ignore {
                    attrs.push(Attr { name, tags });
                }
            }
        }
    }
    assert!(
        tables.len() > 80,
        "sai-meta: only {} attribute enums found: the header scan is stale",
        tables.len()
    );

    let mut out = String::new();
    // One table per enum type that an attribute or a counter list uses.
    let used: BTreeSet<String> = tables
        .values()
        .flatten()
        .filter_map(|a| enum_of(a.tags.ty.as_deref().unwrap_or_default(), &enums))
        .chain(enums.keys().filter(|e| e.ends_with("_stat_t")).cloned())
        .collect();
    for name in &used {
        let values = enums.get(name).expect("a used enum is a known enum");
        let list: Vec<String> = values.iter().map(|v| format!("sys::{v} as i32")).collect();
        writeln!(
            out,
            "static {}: [i32; {}] = [{}];",
            enum_ident(name),
            values.len(),
            list.join(", ")
        )
        .expect("write");
    }
    for (object_type, attrs) in &tables {
        let ident = object_type.trim_start_matches("SAI_OBJECT_TYPE_");
        writeln!(out, "static {ident}: [AttrMeta; {}] = [", attrs.len()).expect("write");
        for a in attrs {
            let (value_type, type_name) = value_type(a.tags.ty.as_deref().unwrap_or_default(), &enums);
            let flags = if a.tags.flags.is_empty() {
                "Flags::NONE".to_string()
            } else {
                a.tags
                    .flags
                    .iter()
                    .map(|f| format!("Flags::{f}.bits()"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            let flags = if a.tags.flags.is_empty() {
                flags
            } else {
                format!("Flags::from_bits({flags})")
            };
            let objects: Vec<String> = a.tags.objects.iter().map(|o| format!("sys::{o}")).collect();
            let enum_values = enum_of(a.tags.ty.as_deref().unwrap_or_default(), &enums)
                .map_or_else(|| "&[]".to_string(), |e| format!("&{}", enum_ident(&e)));
            let default = default_expr(a.tags.default.as_deref().unwrap_or_default());
            writeln!(
                out,
                "    AttrMeta {{ object_type: sys::{object_type}, id: sys::{}, name: \"{}\", value_type: ValueType::{value_type}, \
                 type_name: \"{type_name}\", flags: {flags}, allowed_objects: &[{}], allow_null: {}, conditional: {}, \
                 enum_values: {enum_values}, default: {default} }},",
                a.name,
                a.name,
                objects.join(", "),
                a.tags.allow_null,
                a.tags.conditional
            )
            .expect("write");
        }
        out.push_str("];\n");
    }
    out.push_str("/// The attributes of an object type, in header order.\n");
    out.push_str(
        "#[allow(clippy::too_many_lines)]\npub fn attributes(object_type: sys::sai_object_type_t) -> &'static [AttrMeta] {\n    match object_type {\n",
    );
    for object_type in tables.keys() {
        writeln!(
            out,
            "        sys::{object_type} => &{},",
            object_type.trim_start_matches("SAI_OBJECT_TYPE_")
        )
        .expect("write");
    }
    out.push_str("        _ => &[],\n    }\n}\n");
    out.push_str("/// The counter ids of an object type (its `sai_*_stat_t` enumerators); empty when it has none.\n");
    out.push_str("pub fn stat_ids(object_type: sys::sai_object_type_t) -> &'static [i32] {\n    match object_type {\n");
    for object_type in &object_types {
        let stat = format!(
            "sai_{}_stat_t",
            object_type.trim_start_matches("SAI_OBJECT_TYPE_").to_lowercase()
        );
        if used.contains(&stat) {
            writeln!(out, "        sys::{object_type} => &{},", enum_ident(&stat)).expect("write");
        }
    }
    out.push_str("        _ => &[],\n    }\n}\n");
    out.push_str("/// Every object type of the headers with its name.\n");
    writeln!(
        out,
        "pub static OBJECT_TYPES: [(sys::sai_object_type_t, &str); {}] = [",
        object_types.len()
    )
    .expect("write");
    for o in &object_types {
        writeln!(out, "    (sys::{o}, \"{o}\"),").expect("write");
    }
    out.push_str("];\n");
    out.push_str("/// Attribute enums of the headers that belong to no object type (not in the tables).\n");
    writeln!(
        out,
        "pub static UNMAPPED_ATTR_ENUMS: [&str; {}] = {unmapped:?};",
        unmapped.len()
    )
    .expect("write");
    let dest = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("meta.rs");
    fs::write(dest, out).expect("write meta.rs");
}

fn parse_tag(line: &str, tags: &mut Tags) {
    let Some(at) = line.find('@') else { return };
    let Some(text) = line.get(at + 1..) else { return };
    let (tag, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let rest = rest.trim().trim_end_matches("*/").trim();
    match tag {
        "type" => tags.ty = Some(rest.to_string()),
        "flags" => {
            tags.flags = rest
                .split('|')
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty())
                .collect();
        }
        "objects" => {
            tags.objects = rest
                .split(',')
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty())
                .collect();
        }
        "allownull" => tags.allow_null = rest == "true",
        "condition" | "validonly" => tags.conditional = true,
        "default" => tags.default = Some(rest.to_string()),
        "ignore" => tags.ignore = true,
        _ => {}
    }
}

/// The union member an `@type` travels in, and the C type name for messages.
fn value_type(ty: &str, enums: &BTreeMap<String, Vec<String>>) -> (&'static str, String) {
    let mut words = ty.split_whitespace();
    let first = words.next().unwrap_or_default();
    let second = words.next();
    let vt = match first {
        "bool" => "Bool",
        "char" => "CharData",
        "sai_uint8_t" | "sai_cos_t" | "sai_queue_index_t" => "U8",
        "sai_int8_t" => "I8",
        "sai_uint16_t" | "sai_vlan_id_t" => "U16",
        "sai_int16_t" => "I16",
        "sai_uint32_t" | "sai_label_id_t" => "U32",
        "sai_int32_t" => "I32",
        "sai_uint64_t" => "U64",
        "sai_int64_t" => "I64",
        "sai_mac_t" => "Mac",
        "sai_object_id_t" => "Oid",
        "sai_object_list_t" => "OidList",
        "sai_u8_list_t" => "U8List",
        "sai_s8_list_t" => "I8List",
        "sai_u16_list_t" => "U16List",
        "sai_u32_list_t" => "U32List",
        "sai_s32_list_t" => "I32List",
        "sai_s16_list_t" => "I16List",
        "sai_u16_range_t" => "U16Range",
        "sai_s32_range_t" => "I32Range",
        "sai_ip4_t" => "Ip4",
        "sai_ip6_t" => "Ip6",
        "sai_ip_address_t" => "IpAddress",
        "sai_ip_prefix_t" => "IpPrefix",
        "sai_qos_map_list_t" => "QosMapList",
        "sai_map_list_t" => "MapList",
        "sai_acl_capability_t" => "AclCapability",
        "sai_acl_resource_list_t" => "AclResourceList",
        "sai_timespec_t" => "Timespec",
        "sai_vlan_list_t" => "VlanList",
        "sai_u32_range_t" => "U32Range",
        "sai_pointer_t" => "Pointer",
        // An enum typedef and nothing else: the s32 member.
        t if second.is_none() && enums.contains_key(t) => "Enum",
        _ => "Other",
    };
    (vt, second.map_or_else(|| first.to_string(), |s| format!("{first} {s}")))
}

/// The enum type whose enumerators are an attribute's values: `@type sai_x_t` or
/// `@type sai_s32_list_t sai_x_t`.
fn enum_of(ty: &str, enums: &BTreeMap<String, Vec<String>>) -> Option<String> {
    let mut words = ty.split_whitespace();
    let first = words.next()?;
    let name = match (first, words.next()) {
        ("sai_s32_list_t" | "sai_int32_t", Some(second)) => second,
        (first, None) => first,
        _ => return None,
    };
    enums.contains_key(name).then(|| name.to_string())
}

/// The name of an enum's value table in the generated code.
fn enum_ident(name: &str) -> String {
    format!("E_{}", name.to_uppercase())
}

/// The header's `@default` as a `DefaultValue` expression. Defaults that need the object or
/// the vendor to be known ("internal", "vendor", "attrvalue ...", "const", "disabled") are
/// `Special`: per-type code answers them or nobody does.
fn default_expr(text: &str) -> String {
    let first = text.split_whitespace().next().unwrap_or_default();
    let number = first
        .strip_prefix("0x")
        .map_or_else(|| first.parse::<i64>().ok(), |hex| i64::from_str_radix(hex, 16).ok());
    let constant = first.starts_with("SAI_")
        && first
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    match first {
        "" => "DefaultValue::None".into(),
        "true" => "DefaultValue::Bool(true)".into(),
        "false" => "DefaultValue::Bool(false)".into(),
        "SAI_NULL_OBJECT_ID" => "DefaultValue::NullOid".into(),
        "empty" => "DefaultValue::Empty".into(),
        "NULL" => "DefaultValue::NullPointer".into(),
        "\"\"" => "DefaultValue::EmptyText".into(),
        "0.0.0.0" => "DefaultValue::ZeroIp".into(),
        "00:00:00:00:00:00" => "DefaultValue::ZeroMac".into(),
        _ if constant => format!("DefaultValue::Int(sys::{first} as i64)"),
        _ => number.map_or_else(|| "DefaultValue::Special".into(), |n| format!("DefaultValue::Int({n})")),
    }
}

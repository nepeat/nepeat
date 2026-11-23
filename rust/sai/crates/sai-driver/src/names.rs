//! The enumerators of the SAI headers by name (built by `build.rs`, as `tests/driver_names.awk` does
//! for the C++ driver), and the C++ driver's rules for reading and printing them.
use sai_sys as sys;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// `(type, enumerator, value)` for every enumerator of every `typedef enum _sai_*_t` of
/// `$SAI_INC/sai*.h`, in header order.
#[allow(
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::unnecessary_cast,
    reason = "generated: every enumerator widened to i64 (C++ `(long long)N`)"
)]
static ENUMS: &[(&str, &str, i64)] = include!(concat!(env!("OUT_DIR"), "/names.rs"));

fn types() -> &'static BTreeSet<&'static str> {
    static TYPES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    TYPES.get_or_init(|| ENUMS.iter().map(|(t, _, _)| *t).collect())
}

/// The enum type of that name (`sai_vlan_tagging_mode_t`), as the table has it.
pub fn enum_type(name: &str) -> Option<&'static str> {
    types().get(name).copied()
}

/// The value of an enumerator of a type (C++ `enum_value`).
pub fn enum_value(ty: &str, name: &str) -> Option<i64> {
    ENUMS
        .iter()
        .find(|(t, n, _)| *t == ty && *n == name)
        .map(|(_, _, v)| *v)
}

/// A range marker, not a value (C++ `is_marker`).
pub fn is_marker(name: &str) -> bool {
    name.ends_with("_START")
        || name.ends_with("_END")
        || name.ends_with("_MAX")
        || name.contains("CUSTOM_RANGE")
        || name.contains("RANGE_BASE")
}

/// `sai_vlan_tagging_mode_t` -> `SAI_VLAN_TAGGING_MODE_` (C++ `enum_prefix`).
pub fn enum_prefix(ty: &str) -> String {
    let stem = ty.strip_suffix("_t").unwrap_or(ty);
    format!("{}_", stem.to_ascii_uppercase())
}

/// How the C++ driver prints a value of an enum type (`enum_name`): the first enumerator of that
/// value that is not a range marker, without the type's prefix; else the first marker; else the
/// number.
pub fn enum_name(ty: &str, value: i64) -> String {
    let prefix = enum_prefix(ty);
    let strip = |n: &str| n.strip_prefix(prefix.as_str()).unwrap_or(n).to_owned();
    let mut marker = None;
    for (_, name, _) in ENUMS.iter().filter(|(t, _, v)| *t == ty && *v == value) {
        if !is_marker(name) {
            return strip(name);
        }
        marker.get_or_insert(*name);
    }
    marker.map_or_else(|| value.to_string(), strip)
}

/// A port's counter id by its short name (`IF_IN_OCTETS`).
pub fn port_stat(short: &str) -> Option<i64> {
    enum_value("sai_port_stat_t", &format!("SAI_PORT_STAT_{short}"))
}

/// Checks the table against a few `sai-sys` constants.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_print_like_the_cpp_driver() {
        assert_eq!(enum_name("sai_vlan_tagging_mode_t", 1), "TAGGED");
        assert_eq!(
            enum_value("sai_vlan_tagging_mode_t", "SAI_VLAN_TAGGING_MODE_TAGGED"),
            Some(i64::from(sys::SAI_VLAN_TAGGING_MODE_TAGGED))
        );
        assert_eq!(enum_name("sai_port_oper_status_t", 1), "UP");
        // an unknown value prints as its number
        assert_eq!(enum_name("sai_vlan_tagging_mode_t", 77), "77");
        assert_eq!(enum_prefix("sai_bridge_type_t"), "SAI_BRIDGE_TYPE_");
        assert_eq!(enum_name("sai_bridge_type_t", 0), "1Q");
        assert!(enum_type("sai_packet_action_t").is_some());
        assert!(enum_type("sai_no_such_t").is_none());
        assert_eq!(
            port_stat("IF_IN_OCTETS"),
            Some(i64::from(sys::SAI_PORT_STAT_IF_IN_OCTETS))
        );
    }

    #[test]
    fn a_marker_is_printed_only_when_nothing_else_has_the_value() {
        // SAI_PORT_STAT_START = SAI_PORT_STAT_IF_IN_OCTETS = 0: the value's own name wins
        assert_eq!(enum_name("sai_port_stat_t", 0), "IF_IN_OCTETS");
    }
}

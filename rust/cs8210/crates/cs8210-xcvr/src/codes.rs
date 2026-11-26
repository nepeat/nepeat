//! SFF-8024 code tables (generated: `codes_gen.rs`) and the Ethernet rate of the host
//! electrical interface codes an application may advertise.

#[allow(dead_code, clippy::unreadable_literal)]
#[rustfmt::skip]
mod generated {
    include!("codes_gen.rs");
}

use generated::{
    ACTIVE_CABLE_MEDIA_INTERFACE, BASE_T_MEDIA_INTERFACE, CONNECTORS, HOST_ELECTRICAL_INTERFACE,
    NM_850_MEDIA_INTERFACE, PASSIVE_COPPER_MEDIA_INTERFACE, SM_MEDIA_INTERFACE, XCVR_IDENTIFIER_ABBRV,
    XCVR_IDENTIFIERS,
};

fn find(table: &'static [(u8, &'static str)], code: u8) -> Option<&'static str> {
    table.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// SFF-8024 identifier (byte 0) name.
#[must_use]
pub fn identifier(code: u8) -> Option<&'static str> {
    find(XCVR_IDENTIFIERS, code)
}

/// Short identifier name ("QSFP-DD", "QSFP28", ...).
#[must_use]
pub fn identifier_abbrev(code: u8) -> Option<&'static str> {
    find(XCVR_IDENTIFIER_ABBRV, code)
}

/// Connector type name.
#[must_use]
pub fn connector(code: u8) -> Option<&'static str> {
    find(CONNECTORS, code)
}

/// CMIS module media type (lower page byte 85).
pub const MEDIA_MMF: u8 = 0x01;
/// Single-mode fibre.
pub const MEDIA_SMF: u8 = 0x02;
/// Passive copper cable.
pub const MEDIA_PASSIVE_COPPER: u8 = 0x03;
/// Active cable.
pub const MEDIA_ACTIVE_CABLE: u8 = 0x04;
/// BASE-T.
pub const MEDIA_BASE_T: u8 = 0x05;

/// Module media type name.
#[must_use]
pub fn media_type(code: u8) -> Option<&'static str> {
    Some(match code {
        MEDIA_MMF => "optical MMF",
        MEDIA_SMF => "optical SMF",
        MEDIA_PASSIVE_COPPER => "passive copper",
        MEDIA_ACTIVE_CABLE => "active cable",
        MEDIA_BASE_T => "BASE-T",
        _ => return None,
    })
}

/// Host electrical interface name.
#[must_use]
pub fn host_interface(code: u8) -> Option<&'static str> {
    find(HOST_ELECTRICAL_INTERFACE, code)
}

/// Media interface name; its table depends on the media type.
#[must_use]
pub fn media_interface(media_type: u8, code: u8) -> Option<&'static str> {
    let table = match media_type {
        MEDIA_MMF => NM_850_MEDIA_INTERFACE,
        MEDIA_SMF => SM_MEDIA_INTERFACE,
        MEDIA_PASSIVE_COPPER => PASSIVE_COPPER_MEDIA_INTERFACE,
        MEDIA_ACTIVE_CABLE => ACTIVE_CABLE_MEDIA_INTERFACE,
        MEDIA_BASE_T => BASE_T_MEDIA_INTERFACE,
        _ => return None,
    };
    find(table, code)
}

/// Ethernet rate (Gb/s) of a host electrical interface code: the Ethernet AUI and
/// copper PMD codes of SFF-8024 Table 4-5 that a switch port speed can match (CPRI,
/// OTN, Fibre Channel and Infiniband codes have none).
#[must_use]
pub fn host_interface_gbps(code: u8) -> Option<u32> {
    Some(match code {
        0x02..=0x04 | 0x13 => 10,                       // XAUI, XFI, SFI, 10GBASE-CX4
        0x05 | 0x14..=0x16 => 25,                       // 25GAUI, 25GBASE-CR
        0x06 | 0x07 | 0x17 => 40,                       // XLAUI, XLPPI, 40GBASE-CR4
        0x08..=0x0a | 0x18 | 0x43..=0x45 => 50,         // LAUI-2, 50GAUI-2/-1, 50GBASE-CR(2)
        0x0b..=0x0d | 0x19..=0x1b | 0x41 | 0x42 => 100, // CAUI-4, 100GAUI-4/-2, 100GBASE-CR10/4/2
        0x0e | 0x0f | 0x1c => 200,                      // 200GAUI-8/-4, 200GBASE-CR4
        0x10 | 0x11 | 0x1d => 400,                      // 400GAUI-16/-8, 400G CR8
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_the_codes_of_the_lab_modules() {
        assert_eq!(identifier_abbrev(0x18), Some("QSFP-DD"));
        assert_eq!(host_interface(0x11), Some("400GAUI-8 C2M (Annex 120E)"));
        assert_eq!(media_interface(MEDIA_SMF, 0x1c), Some("400GBASE-DR4 (Cl 124)"));
        assert_eq!(media_type(MEDIA_PASSIVE_COPPER), Some("passive copper"));
        assert_eq!(host_interface_gbps(0x11), Some(400));
        assert_eq!(host_interface_gbps(0x0d), Some(100));
        assert_eq!(host_interface_gbps(0x1d), Some(400));
        assert_eq!(host_interface_gbps(0x33), None);
    }
}

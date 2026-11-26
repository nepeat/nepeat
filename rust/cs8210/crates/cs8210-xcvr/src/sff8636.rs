//! SFF-8636 (QSFP28 / QSFP+): identity, power class and high-power enable, monitors.

use serde::Serialize;

use crate::{ascii, be16, byte, codes, dom};

/// Register offsets and bits.
pub mod reg {
    /// Lower page: status; bit 2 = flat memory, bit 0 = data not ready.
    pub const STATUS: u8 = 2;
    /// Lower page: Rx LOS bits 3:0, Tx LOS 7:4 (latched).
    pub const LOS: u8 = 3;
    /// Lower page: Tx fault bits 3:0 (latched).
    pub const TX_FAULT: u8 = 4;
    /// Lower page: CDR loss of lock, Rx 3:0, Tx 7:4 (latched).
    pub const CDR_LOL: u8 = 5;
    /// Lower page: temperature, s16 / 256 deg C.
    pub const TEMPERATURE: u8 = 22;
    /// Lower page: supply voltage, u16 * 100 uV.
    pub const VCC: u8 = 26;
    /// Lower page: Rx power lanes 1-4, u16 * 0.1 uW.
    pub const RX_POWER: u8 = 34;
    /// Lower page: Tx bias lanes 1-4, u16 * 2 uA.
    pub const TX_BIAS: u8 = 42;
    /// Lower page: Tx power lanes 1-4, u16 * 0.1 uW.
    pub const TX_POWER: u8 = 50;
    /// Lower page: Tx disable, bits 3:0.
    pub const TX_DISABLE: u8 = 86;
    /// Lower page: power control.
    pub const POWER_CONTROL: u8 = 93;
    /// `POWER_CONTROL`: `Power_override` (software, not the `LPMode` pin, decides).
    pub const POWER_OVERRIDE: u8 = 1 << 0;
    /// `POWER_CONTROL`: `Power_set` (with override: low power).
    pub const POWER_SET: u8 = 1 << 1;
    /// `POWER_CONTROL`: High Power Class Enable (classes 5-7).
    pub const HIGH_POWER_5_7: u8 = 1 << 2;
    /// `POWER_CONTROL`: High Power Class Enable (class 8).
    pub const HIGH_POWER_8: u8 = 1 << 3;
    /// Upper page 00h: extended identifier (power class).
    pub const EXT_IDENTIFIER: u8 = 129;
    /// Upper page 00h: connector.
    pub const CONNECTOR: u8 = 130;
    /// Upper page 00h: vendor name (16).
    pub const VENDOR_NAME: u8 = 148;
    /// Upper page 00h: vendor OUI (3).
    pub const VENDOR_OUI: u8 = 165;
    /// Upper page 00h: part number (16).
    pub const PART_NUMBER: u8 = 168;
    /// Upper page 00h: revision (2).
    pub const PART_REVISION: u8 = 184;
    /// Upper page 00h: extended specification compliance (SFF-8024 table 4-4).
    pub const EXT_COMPLIANCE: u8 = 192;
    /// Upper page 00h: serial number (16).
    pub const SERIAL_NUMBER: u8 = 196;
    /// Upper page 00h: date code (8).
    pub const DATE_CODE: u8 = 212;
    /// Upper page 00h: diagnostic monitoring type.
    pub const DIAG_TYPE: u8 = 220;
}

/// Power class (1-8) from the extended identifier (byte 129).
#[must_use]
pub fn power_class(ext_id: u8) -> u8 {
    if ext_id & 0x20 != 0 {
        return 8;
    }
    match ext_id & 3 {
        0 => (ext_id >> 6) + 1,
        n => n + 4,
    }
}

/// The power control byte that lets a module of `class` draw its power with the `LPMode`
/// pin deasserted, when `current` does not already (None: nothing to write).
#[must_use]
pub fn high_power_control(current: u8, class: u8) -> Option<u8> {
    let want = match class {
        8 => reg::HIGH_POWER_5_7 | reg::HIGH_POWER_8,
        5..=7 => reg::HIGH_POWER_5_7,
        _ => 0,
    };
    let mut v = current | want;
    if v & reg::POWER_OVERRIDE != 0 {
        v &= !reg::POWER_SET; // software override present: ask for high power
    }
    (v != current).then_some(v)
}

/// What lower page + upper page 00h say about a module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Identity {
    /// SFF-8024 identifier code
    pub identifier: u8,
    /// its short name
    pub form_factor: String,
    /// flat memory
    pub flat_memory: bool,
    /// vendor name
    pub vendor: String,
    /// vendor OUI
    pub vendor_oui: String,
    /// part number
    pub part_number: String,
    /// revision
    pub revision: String,
    /// serial number
    pub serial: String,
    /// date code
    pub date_code: String,
    /// connector name
    pub connector: String,
    /// power class (1-8)
    pub power_class: u8,
    /// extended specification compliance code (100G types)
    pub ext_compliance: u8,
}

/// Parse lower page + upper page 00h (256 bytes).
#[must_use]
pub fn identity(page0: &[u8]) -> Option<Identity> {
    let s = |off: u8, n: usize| {
        page0
            .get(usize::from(off)..usize::from(off) + n)
            .map(ascii)
            .unwrap_or_default()
    };
    let identifier = byte(page0, 0)?;
    let conn = byte(page0, usize::from(reg::CONNECTOR))?;
    let &[o0, o1, o2] = page0.get(usize::from(reg::VENDOR_OUI)..usize::from(reg::VENDOR_OUI) + 3)? else {
        return None;
    };
    Some(Identity {
        identifier,
        form_factor: codes::identifier_abbrev(identifier).unwrap_or("unknown").to_string(),
        flat_memory: byte(page0, usize::from(reg::STATUS))? & 0x04 != 0,
        vendor: s(reg::VENDOR_NAME, 16),
        vendor_oui: format!("{o0:02x}:{o1:02x}:{o2:02x}"),
        part_number: s(reg::PART_NUMBER, 16),
        revision: s(reg::PART_REVISION, 2),
        serial: s(reg::SERIAL_NUMBER, 16),
        date_code: s(reg::DATE_CODE, 8),
        connector: codes::connector(conn).map_or_else(|| format!("code {conn:#04x}"), str::to_string),
        power_class: power_class(byte(page0, usize::from(reg::EXT_IDENTIFIER))?),
        ext_compliance: byte(page0, usize::from(reg::EXT_COMPLIANCE))?,
    })
}

/// Monitors from the lower page (4 lanes); `dom` = the module implements digital
/// diagnostics (byte 220 bits 2..=5 not all zero).
#[must_use]
pub fn monitors(lower: &[u8], diag_type: u8) -> dom::Monitors {
    let has = diag_type & 0x3c != 0;
    let w = |r: u8, i: u8| be16(lower, usize::from(r) + 2 * usize::from(i));
    let flag = |r: u8, bit: u8| byte(lower, usize::from(r)).map(|b| b >> bit & 1 != 0);
    dom::Monitors {
        temperature_c: has
            .then(|| be16(lower, usize::from(reg::TEMPERATURE)))
            .flatten()
            .map(|v| f64::from(v.cast_signed()) / 256.0),
        voltage_v: has
            .then(|| be16(lower, usize::from(reg::VCC)))
            .flatten()
            .map(|v| f64::from(v) / 10_000.0),
        lanes: (0..4u8)
            .map(|i| dom::Lane {
                lane: i + 1,
                tx_power_mw: has
                    .then(|| w(reg::TX_POWER, i))
                    .flatten()
                    .map(|v| f64::from(v) / 10_000.0),
                rx_power_mw: has
                    .then(|| w(reg::RX_POWER, i))
                    .flatten()
                    .map(|v| f64::from(v) / 10_000.0),
                tx_bias_ma: has.then(|| w(reg::TX_BIAS, i)).flatten().map(|v| f64::from(v) / 500.0),
                rx_los: flag(reg::LOS, i),
                tx_fault: flag(reg::TX_FAULT, i),
                tx_cdr_lol: flag(reg::CDR_LOL, i + 4),
                rx_cdr_lol: flag(reg::CDR_LOL, i),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qsfp28() -> Vec<u8> {
        let mut m = vec![0u8; 256];
        m[0] = 0x11;
        m[2] = 0x00;
        m[3] = 0x02; // Rx LOS lane 2
        m[22..24].copy_from_slice(&10_368u16.to_be_bytes());
        m[26..28].copy_from_slice(&33000u16.to_be_bytes());
        m[34..36].copy_from_slice(&5000u16.to_be_bytes());
        m[42..44].copy_from_slice(&3500u16.to_be_bytes());
        m[50..52].copy_from_slice(&7000u16.to_be_bytes());
        m[128] = 0x11;
        m[129] = 0xc2; // class 4 bits + class 6 in 1:0
        m[130] = 0x0c;
        m[148..153].copy_from_slice(b"ACME ");
        m[168..176].copy_from_slice(b"QSFP-LR4");
        m[192] = 0x02;
        m[196..200].copy_from_slice(b"SN01");
        m[220] = 0x0c;
        m
    }

    #[test]
    fn identity_power_and_monitors() {
        let m = qsfp28();
        let id = identity(&m).unwrap();
        assert_eq!(id.form_factor, "QSFP28");
        assert_eq!(id.vendor, "ACME");
        assert_eq!(id.part_number, "QSFP-LR4");
        assert_eq!(id.power_class, 6);
        assert_eq!(id.connector, "MPO 1x12");
        let mon = monitors(&m, m[220]);
        assert!((mon.temperature_c.unwrap() - 40.5).abs() < 1e-9);
        assert!((mon.voltage_v.unwrap() - 3.3).abs() < 1e-9);
        assert!((mon.lanes[0].rx_power_mw.unwrap() - 0.5).abs() < 1e-9);
        assert!((mon.lanes[0].tx_bias_ma.unwrap() - 7.0).abs() < 1e-9);
        assert!((mon.lanes[0].tx_power_mw.unwrap() - 0.7).abs() < 1e-9);
        assert_eq!(mon.lanes[1].rx_los, Some(true));
        assert_eq!(monitors(&m, 0).temperature_c, None);
    }

    #[test]
    fn power_classes() {
        assert_eq!(power_class(0x00), 1);
        assert_eq!(power_class(0xc0), 4);
        assert_eq!(power_class(0xc1), 5);
        assert_eq!(power_class(0xc3), 7);
        assert_eq!(power_class(0xe3), 8);
        assert_eq!(high_power_control(0, 4), None);
        assert_eq!(high_power_control(0, 6), Some(0x04));
        assert_eq!(high_power_control(0x04, 6), None);
        assert_eq!(high_power_control(0, 8), Some(0x0c));
        assert_eq!(high_power_control(0x03, 4), Some(0x01));
    }
}

//! CMIS 4.0 / 5.x (QSFP-DD, OSFP): identity, module and data path state, applications,
//! monitors. Offsets of the lower page are 0-127; of an upper page 128-255 (with its page).
//! Names follow the specification; sonic-platform-common's `sonic_xcvr` CMIS memory map
//! uses the same offsets.

use serde::Serialize;

use crate::{ascii, be16, byte, codes, dom};

/// Register offsets and bits.
pub mod reg {
    /// Lower page: SFF-8024 identifier.
    pub const ID: u8 = 0;
    /// Lower page: CMIS revision (major.minor nibbles).
    pub const REVISION: u8 = 1;
    /// Lower page: memory model etc.; bit 7 = flat memory.
    pub const MEMORY_MODEL: u8 = 2;
    /// Lower page: module state in bits 3:1, bit 0 = interrupt deasserted.
    pub const MODULE_STATE: u8 = 3;
    /// Lower page: module temperature, s16 / 256 deg C.
    pub const TEMPERATURE: u8 = 14;
    /// Lower page: supply voltage, u16 * 100 uV.
    pub const VCC: u8 = 16;
    /// Lower page: module global controls.
    pub const MODULE_CONTROL: u8 = 26;
    /// `MODULE_CONTROL` bit: `LowPwrRequestSW`.
    pub const LOW_PWR_REQUEST_SW: u8 = 1 << 4;
    /// `MODULE_CONTROL` bit: `LowPwrAllowRequestHW` (the `LPMode` pin counts).
    pub const LOW_PWR_ALLOW_REQUEST_HW: u8 = 1 << 6;
    /// Lower page: module media type (SFF-8024 table 4-3... media type encodings).
    pub const MEDIA_TYPE: u8 = 85;
    /// Lower page: first application advertisement (4 bytes each, 8 of them).
    pub const APPLICATIONS: u8 = 86;

    /// Upper page 00h: vendor name (16).
    pub const VENDOR_NAME: u8 = 129;
    /// Upper page 00h: vendor OUI (3).
    pub const VENDOR_OUI: u8 = 145;
    /// Upper page 00h: part number (16).
    pub const PART_NUMBER: u8 = 148;
    /// Upper page 00h: revision (2).
    pub const PART_REVISION: u8 = 164;
    /// Upper page 00h: serial number (16).
    pub const SERIAL_NUMBER: u8 = 166;
    /// Upper page 00h: date code (8).
    pub const DATE_CODE: u8 = 182;
    /// Upper page 00h: power class in bits 7:5 (class = value + 1).
    pub const POWER_CLASS: u8 = 200;
    /// Upper page 00h: maximum power, 0.25 W units.
    pub const MAX_POWER: u8 = 201;
    /// Upper page 00h: cable assembly length (bits 7:6 multiplier, 5:0 value).
    pub const CABLE_LENGTH: u8 = 202;
    /// Upper page 00h: connector type.
    pub const CONNECTOR: u8 = 203;

    /// Page 01h (advertising).
    pub const PAGE_ADVERTISING: u8 = 0x01;
    /// Page 01h: `DPDeinit` (7:4) / `DPInit` (3:0) maximum durations (timing codes).
    pub const DP_TIMINGS: u8 = 144;
    /// Page 01h: module-level monitors supported (bit 0 temperature, bit 1 VCC).
    pub const MODULE_MONITORS: u8 = 159;
    /// Page 01h: lane monitors supported (bit 0 Tx bias, 1 Tx power, 2 Rx power), Tx bias scale in 4:3.
    pub const LANE_MONITORS: u8 = 160;
    /// Page 01h: `ModulePwrDn` (7:4) / `ModulePwrUp` (3:0) maximum durations.
    pub const MODULE_TIMINGS: u8 = 167;

    /// Page 10h (lane and data path control).
    pub const PAGE_DP_CONTROL: u8 = 0x10;
    /// Page 10h: `DataPathDeinit`, one bit per host lane.
    pub const DATAPATH_DEINIT: u8 = 128;
    /// Page 10h: Tx disable, one bit per media lane.
    pub const TX_DISABLE: u8 = 130;
    /// Page 10h: `Apply_DataPathInit` for staged control set 0, one bit per host lane.
    pub const APPLY_DPINIT: u8 = 143;
    /// Page 10h: staged set 0 `ApSel` code per host lane (8 bytes):
    /// bits 7:4 `ApSel`, 3:1 `DataPathID` (first lane, 0-based), 0 explicit control.
    pub const STAGED_APSEL: u8 = 145;

    /// Page 11h (lane and data path status).
    pub const PAGE_DP_STATUS: u8 = 0x11;
    /// Page 11h: data path state, a nibble per host lane (lane 1 = low nibble of 128).
    pub const DP_STATE: u8 = 128;
    /// Page 11h: Tx fault flags (latched), a bit per lane.
    pub const TX_FAULT: u8 = 135;
    /// Page 11h: Tx LOS flags.
    pub const TX_LOS: u8 = 136;
    /// Page 11h: Tx CDR loss-of-lock flags.
    pub const TX_CDR_LOL: u8 = 137;
    /// Page 11h: Rx LOS flags.
    pub const RX_LOS: u8 = 147;
    /// Page 11h: Rx CDR loss-of-lock flags.
    pub const RX_CDR_LOL: u8 = 148;
    /// Page 11h: Tx optical power per lane, u16 * 0.1 uW (8 lanes).
    pub const TX_POWER: u8 = 154;
    /// Page 11h: Tx bias per lane, u16 * 2 uA (times the page 01h scale).
    pub const TX_BIAS: u8 = 170;
    /// Page 11h: Rx optical power per lane, u16 * 0.1 uW.
    pub const RX_POWER: u8 = 186;
    /// Page 11h: config status, a nibble per host lane.
    pub const CONFIG_STATUS: u8 = 202;
    /// Page 11h: active control set `ApSel` code per host lane (8 bytes, as `STAGED_APSEL`).
    pub const ACTIVE_APSEL: u8 = 206;
}

/// Module state (lower page byte 3, bits 3:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ModuleState {
    /// `ModuleLowPwr`
    LowPwr,
    /// `ModulePwrUp`
    PwrUp,
    /// `ModuleReady`
    Ready,
    /// `ModulePwrDn`
    PwrDn,
    /// `ModuleFault`
    Fault,
    /// a reserved code
    Reserved(u8),
}

impl ModuleState {
    /// From byte 3.
    #[must_use]
    pub fn from_byte(b: u8) -> Self {
        match (b >> 1) & 7 {
            1 => Self::LowPwr,
            2 => Self::PwrUp,
            3 => Self::Ready,
            4 => Self::PwrDn,
            5 => Self::Fault,
            c => Self::Reserved(c),
        }
    }
}

/// Data path state of a host lane (page 11h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DpState {
    /// `DataPathDeactivated`
    Deactivated,
    /// `DataPathInit`
    Init,
    /// `DataPathDeinit`
    Deinit,
    /// `DataPathActivated`
    Activated,
    /// `DataPathTxTurnOn`
    TxTurnOn,
    /// `DataPathTxTurnOff`
    TxTurnOff,
    /// `DataPathInitialized`
    Initialized,
    /// a reserved code
    Reserved(u8),
}

impl DpState {
    fn from_nibble(n: u8) -> Self {
        match n {
            1 => Self::Deactivated,
            2 => Self::Init,
            3 => Self::Deinit,
            4 => Self::Activated,
            5 => Self::TxTurnOn,
            6 => Self::TxTurnOff,
            7 => Self::Initialized,
            c => Self::Reserved(c),
        }
    }
}

/// Config status of a host lane (page 11h, after `Apply_DataPathInit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConfigStatus {
    /// `ConfigUndefined`
    Undefined,
    /// `ConfigSuccess`
    Success,
    /// `ConfigInProgress`
    InProgress,
    /// one of the `ConfigRejected`* codes (2..=7, or reserved)
    Rejected(u8),
}

impl ConfigStatus {
    fn from_nibble(n: u8) -> Self {
        match n {
            0 => Self::Undefined,
            1 => Self::Success,
            12 => Self::InProgress,
            c => Self::Rejected(c),
        }
    }
}

/// Per-lane nibbles (lane 1 = low nibble of the first byte), as page 11h stores them.
fn nibbles(four: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, b) in four.iter().take(4).enumerate() {
        if let Some(lo) = out.get_mut(2 * i) {
            *lo = b & 0x0f;
        }
        if let Some(hi) = out.get_mut(2 * i + 1) {
            *hi = b >> 4;
        }
    }
    out
}

/// Data path state of the 8 host lanes, from page 11h bytes 128..=131.
#[must_use]
pub fn dp_states(four: &[u8]) -> [DpState; 8] {
    nibbles(four).map(DpState::from_nibble)
}

/// Config status of the 8 host lanes, from page 11h bytes 202..=205.
#[must_use]
pub fn config_status(four: &[u8]) -> [ConfigStatus; 8] {
    nibbles(four).map(ConfigStatus::from_nibble)
}

/// One advertised application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Application {
    /// `ApSel` code (1-based position in the list)
    pub sel: u8,
    /// host electrical interface code (SFF-8024)
    pub host_if: u8,
    /// media interface code (table per media type)
    pub media_if: u8,
    /// host lanes per data path
    pub host_lanes: u8,
    /// media lanes per data path
    pub media_lanes: u8,
    /// host lane assignment options: bit i = a data path may start on lane i+1
    pub host_lane_assignment: u8,
}

impl Application {
    /// `ApSel` code with `DataPathID` `first_lane` (0-based) for the staged control set.
    #[must_use]
    pub fn staged_code(&self, first_lane: u8) -> u8 {
        (self.sel << 4) | ((first_lane & 7) << 1)
    }
}

/// The applications of the lower page (bytes 86..=117; the list ends at host code 0xff).
#[must_use]
pub fn applications(lower: &[u8]) -> Vec<Application> {
    let mut out = Vec::new();
    for i in 0..8u8 {
        let base = usize::from(reg::APPLICATIONS) + 4 * usize::from(i);
        let Some(&[host_if, media_if, lanes, assign]) =
            lower.get(base..base + 4).and_then(|s| <&[u8; 4]>::try_from(s).ok())
        else {
            break;
        };
        if host_if == 0xff || host_if == 0 {
            break;
        }
        out.push(Application {
            sel: i + 1,
            host_if,
            media_if,
            host_lanes: lanes >> 4,
            media_lanes: lanes & 0x0f,
            host_lane_assignment: assign,
        });
    }
    out
}

/// Maximum duration (ms) of a CMIS timing code (page 01h), None when not specified.
#[must_use]
pub fn timing_ms(code: u8) -> Option<u64> {
    Some(match code & 0x0f {
        0 => 1,
        1 => 5,
        2 => 10,
        3 => 50,
        4 => 100,
        5 => 500,
        6 => 1_000,
        7 => 5_000,
        8 => 10_000,
        9 => 60_000,
        10 => 300_000,
        11 => 600_000,
        12 => 3_000_000,
        13 => 6_000_000,
        _ => return None,
    })
}

/// What lower page + upper page 00h say about a module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Identity {
    /// SFF-8024 identifier code
    pub identifier: u8,
    /// its short name
    pub form_factor: String,
    /// CMIS revision "major.minor"
    pub cmis_revision: String,
    /// flat memory (no pages beyond 00h)
    pub flat_memory: bool,
    /// vendor name
    pub vendor: String,
    /// vendor OUI "aa:bb:cc"
    pub vendor_oui: String,
    /// part number
    pub part_number: String,
    /// part revision
    pub revision: String,
    /// serial number
    pub serial: String,
    /// date code (YYMMDD + lot)
    pub date_code: String,
    /// media type code (byte 85)
    pub media_type_code: u8,
    /// its name
    pub media_type: String,
    /// connector name
    pub connector: String,
    /// power class (1-8)
    pub power_class: u8,
    /// maximum power (W)
    pub max_power_w: f64,
    /// cable length (m) for cable assemblies
    pub cable_length_m: Option<f64>,
    /// advertised applications
    pub applications: Vec<Application>,
}

/// Human description of an application ("400GAUI-8 C2M (Annex 120E) / 400GBASE-DR4 (Cl 124), 8x4").
#[must_use]
pub fn describe(app: &Application, media_type: u8) -> String {
    let host = codes::host_interface(app.host_if).map_or_else(|| format!("host {:#04x}", app.host_if), str::to_string);
    let media = codes::media_interface(media_type, app.media_if)
        .map_or_else(|| format!("media {:#04x}", app.media_if), str::to_string);
    format!("{host} / {media}, {}x{}", app.host_lanes, app.media_lanes)
}

/// Parse lower page + upper page 00h (256 bytes, optoe layout).
#[must_use]
pub fn identity(page0: &[u8]) -> Option<Identity> {
    let s = |off: u8, n: usize| {
        page0
            .get(usize::from(off)..usize::from(off) + n)
            .map(ascii)
            .unwrap_or_default()
    };
    let identifier = byte(page0, 0)?;
    let rev = byte(page0, usize::from(reg::REVISION))?;
    let media = byte(page0, usize::from(reg::MEDIA_TYPE))?;
    let pc = byte(page0, usize::from(reg::POWER_CLASS))?;
    let len = byte(page0, usize::from(reg::CABLE_LENGTH))?;
    let conn = byte(page0, usize::from(reg::CONNECTOR))?;
    let &[o0, o1, o2] = page0.get(usize::from(reg::VENDOR_OUI)..usize::from(reg::VENDOR_OUI) + 3)? else {
        return None;
    };
    let cable = (media == codes::MEDIA_PASSIVE_COPPER || media == codes::MEDIA_ACTIVE_CABLE || len != 0).then(|| {
        let mult = match len >> 6 {
            0 => 0.1,
            1 => 1.0,
            2 => 10.0,
            _ => 100.0,
        };
        f64::from(len & 0x3f) * mult
    });
    Some(Identity {
        identifier,
        form_factor: codes::identifier_abbrev(identifier).unwrap_or("unknown").to_string(),
        cmis_revision: format!("{}.{}", rev >> 4, rev & 0x0f),
        flat_memory: byte(page0, usize::from(reg::MEMORY_MODEL))? & 0x80 != 0,
        vendor: s(reg::VENDOR_NAME, 16),
        vendor_oui: format!("{o0:02x}:{o1:02x}:{o2:02x}"),
        part_number: s(reg::PART_NUMBER, 16),
        revision: s(reg::PART_REVISION, 2),
        serial: s(reg::SERIAL_NUMBER, 16),
        date_code: s(reg::DATE_CODE, 8),
        media_type_code: media,
        media_type: codes::media_type(media).map_or_else(|| format!("code {media:#04x}"), str::to_string),
        connector: codes::connector(conn).map_or_else(|| format!("code {conn:#04x}"), str::to_string),
        power_class: (pc >> 5) + 1,
        max_power_w: f64::from(byte(page0, usize::from(reg::MAX_POWER))?) * 0.25,
        cable_length_m: cable,
        applications: applications(page0),
    })
}

/// Module-level monitors from the lower page, if the module has them (page 01h byte 159
/// when paged; a flat passive cable has none).
#[must_use]
pub fn module_monitors(lower: &[u8], page01: Option<&[u8]>) -> (Option<f64>, Option<f64>) {
    let adv = match page01 {
        Some(p) => byte(p, usize::from(reg::MODULE_MONITORS) - 128).unwrap_or(0),
        None if byte(lower, usize::from(reg::MEDIA_TYPE)) == Some(codes::MEDIA_PASSIVE_COPPER) => 0,
        None => 0b11,
    };
    let temp = (adv & 1 != 0)
        .then(|| be16(lower, usize::from(reg::TEMPERATURE)))
        .flatten()
        .map(|raw| f64::from(raw.cast_signed()) / 256.0);
    let vcc = (adv & 2 != 0)
        .then(|| be16(lower, usize::from(reg::VCC)))
        .flatten()
        .map(|raw| f64::from(raw) / 10_000.0);
    (temp, vcc)
}

/// Lane monitors and flags from page 11h (bytes 128..=255 as `page11[0..128]`) with the
/// page 01h advertisement (bytes 128..=255 as `page01[0..128]`); `lanes` media lanes.
#[must_use]
pub fn lane_monitors(page11: &[u8], page01: &[u8], lanes: u8) -> Vec<dom::Lane> {
    let at = |r: u8| usize::from(r) - 128;
    let adv = byte(page01, at(reg::LANE_MONITORS)).unwrap_or(0);
    let bias_scale = f64::from(1u8 << ((adv >> 3) & 3).min(2));
    let bit = |r: u8, lane: u8| byte(page11, at(r)).map(|b| b >> lane & 1 != 0);
    (0..lanes.min(8))
        .map(|i| {
            let word = |r: u8| be16(page11, at(r) + 2 * usize::from(i));
            dom::Lane {
                lane: i + 1,
                tx_power_mw: (adv & 2 != 0)
                    .then(|| word(reg::TX_POWER))
                    .flatten()
                    .map(|v| f64::from(v) / 10_000.0),
                rx_power_mw: (adv & 4 != 0)
                    .then(|| word(reg::RX_POWER))
                    .flatten()
                    .map(|v| f64::from(v) / 10_000.0),
                tx_bias_ma: (adv & 1 != 0)
                    .then(|| word(reg::TX_BIAS))
                    .flatten()
                    .map(|v| f64::from(v) / 500.0 * bias_scale),
                rx_los: bit(reg::RX_LOS, i),
                tx_fault: bit(reg::TX_FAULT, i),
                tx_cdr_lol: bit(reg::TX_CDR_LOL, i),
                rx_cdr_lol: bit(reg::RX_CDR_LOL, i),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab;

    #[test]
    fn lab_dr4_identity() {
        let id = identity(&lab::dr4_page0()).unwrap();
        assert_eq!(id.form_factor, "QSFP-DD");
        assert_eq!(id.cmis_revision, "4.0");
        assert!(!id.flat_memory);
        assert_eq!(id.vendor, "MOLEX");
        assert_eq!(id.vendor_oui, "00:09:3a");
        assert_eq!(id.part_number, "1064281405");
        assert_eq!(id.serial, "G21084B2455");
        assert_eq!(id.media_type, "optical SMF");
        assert_eq!(id.connector, "MPO 1x12");
        assert_eq!(id.power_class, 6);
        assert!((id.max_power_w - 12.0).abs() < 1e-9);
        assert_eq!(id.applications.len(), 2);
        let a = id.applications[0];
        assert_eq!(
            (a.sel, a.host_if, a.media_if, a.host_lanes, a.media_lanes),
            (1, 0x11, 0x1c, 8, 4)
        );
        assert_eq!(
            describe(&a, id.media_type_code),
            "400GAUI-8 C2M (Annex 120E) / 400GBASE-DR4 (Cl 124), 8x4"
        );
        let b = id.applications[1];
        assert_eq!(
            (b.sel, b.host_if, b.host_lanes, b.media_lanes, b.host_lane_assignment),
            (2, 0x0d, 2, 1, 0x55)
        );
        assert_eq!(a.staged_code(0), 0x10);
        assert_eq!(b.staged_code(2), 0x24);
    }

    #[test]
    fn lab_dac_identity() {
        let id = identity(&lab::dac_page0()).unwrap();
        assert!(id.flat_memory);
        assert_eq!(id.vendor, "TE Connectivity");
        assert_eq!(id.part_number, "2459398-1");
        assert_eq!(id.media_type, "passive copper");
        assert_eq!(id.power_class, 1);
        assert!((id.cable_length_m.unwrap() - 1.8).abs() < 1e-9);
        assert_eq!(id.applications[0].host_if, 0x1d);
        assert_eq!(module_monitors(&lab::dac_page0(), None), (None, None));
    }

    #[test]
    fn lab_dr4_state_and_monitors() {
        let p0 = lab::dr4_page0();
        assert_eq!(ModuleState::from_byte(p0[3]), ModuleState::Ready);
        let p01 = lab::dr4_page01();
        let (t, v) = module_monitors(&p0, Some(&p01));
        assert!((t.unwrap() - 32.0 - f64::from(0x24) / 256.0).abs() < 1e-9);
        assert!((v.unwrap() - 3.3687).abs() < 1e-9);
        let p11 = lab::dr4_page11();
        assert_eq!(dp_states(&p11[0..4]), [DpState::Activated; 8]);
        let active: Vec<u8> = p11[78..86].to_vec();
        assert_eq!(active, vec![0x20, 0x20, 0x24, 0x24, 0x28, 0x28, 0x2c, 0x2c]); // app 2: 4 x 100G
        let lanes = lane_monitors(&p11, &p01, 4);
        assert_eq!(lanes.len(), 4);
        assert!((lanes[0].tx_power_mw.unwrap() - 1.7921).abs() < 1e-9);
        assert!((lanes[3].rx_power_mw.unwrap() - 0.0422).abs() < 1e-9);
        assert!((lanes[0].tx_bias_ma.unwrap() - 16388.0 / 500.0 * 4.0).abs() < 1e-9); // scale code 2 = x4
        assert_eq!(lanes[3].rx_los, Some(true));
        assert_eq!(lanes[0].rx_los, Some(false));
        assert_eq!(timing_ms(p01[167 - 128]), Some(10_000));
    }

    #[test]
    fn nibble_order() {
        let s = dp_states(&[0x41, 0x44, 0x44, 0x14]);
        assert_eq!(s[0], DpState::Deactivated);
        assert_eq!(s[1], DpState::Activated);
        assert_eq!(s[6], DpState::Activated);
        assert_eq!(s[7], DpState::Deactivated);
        assert_eq!(config_status(&[0x1c, 0x11, 0x22, 0x11])[0], ConfigStatus::InProgress);
        assert_eq!(config_status(&[0x1c, 0x11, 0x22, 0x11])[4], ConfigStatus::Rejected(2));
    }
}

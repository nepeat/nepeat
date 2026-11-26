//! The CS8210-32X power supplies (two Delta TDPS-1500AB-6 B bays on this 19-inch variant).
//!
//! - Status: system CPLD 0x60 reg 0x08: PSU1 present bit 4, PSU2 bit 5 (active low), input good
//!   bits 2 / 3, output good bits 0 / 1 (vendor `sys_cpld.ko`). Reg 0xa8 carries PSU 3/4 on the
//!   21-inch variant only (sys CPLD 0x09 bit 0 = 0); here it reads 0 and is not wired.
//! - `PMBus` at 0x59 on mux 0x72 (behind 0x70:2 -> 0x74:0): PSU1 on channel 1, PSU2 on channel 0
//!   (the vendor's 15-0059 / 14-0059). A PSU without AC still answers from the standby rail.
//!   [`bind`] reads its identity and binds the kernel's generic `pmbus` driver, so the readings
//!   are a hwmon device (`node_hwmon_*` for free) that [`metrics`] also republishes.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// `PMBus` address of the PSU controller.
pub const PMBUS_ADDR: u8 = 0x59;
/// mux 0x72 channel per PSU slot.
pub const SLOTS: [(u8, u8); 2] = [(1, 1), (2, 0)];

/// Status bits of one PSU from the system CPLD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// a PSU is in the bay
    pub present: bool,
    /// its AC input is good
    pub input_good: bool,
    /// its output is good
    pub output_good: bool,
}

/// Decode reg 0x08 for PSU 1 or 2.
#[must_use]
pub fn status(reg08: u8, psu: u8) -> Status {
    let (p, i, o) = if psu == 1 { (4, 2, 0) } else { (5, 3, 1) };
    Status {
        present: reg08 >> p & 1 == 0,
        input_good: reg08 >> i & 1 == 1,
        output_good: reg08 >> o & 1 == 1,
    }
}

/// The adapter of mux 0x72's channel `ch` (`sys` = /sys normally).
#[must_use]
pub fn adapter(sys: &Path, ch: u8) -> Option<(u32, PathBuf)> {
    let devs = sys.join("bus/i2c/devices");
    for e in fs::read_dir(&devs).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.ends_with("-0072") {
            continue;
        }
        let link = fs::read_link(e.path().join(format!("channel-{ch}"))).ok()?;
        let bus = link.file_name()?.to_string_lossy().strip_prefix("i2c-")?.parse().ok()?;
        return Some((bus, devs.join(format!("{bus}-00{PMBUS_ADDR:02x}"))));
    }
    None
}

/// `PMBus` LINEAR11 to a float.
#[must_use]
pub fn linear11(raw: u16) -> f64 {
    let exp = i32::from(raw.cast_signed() >> 11);
    let mant = i32::from(((raw & 0x7ff) << 5).cast_signed() >> 5);
    f64::from(mant) * 2f64.powi(exp)
}

/// One hwmon reading: (label, kind, value in SI units).
pub type Reading = (String, &'static str, f64);

/// The hwmon readings of the bound pmbus device.
#[must_use]
pub fn hwmon_readings(dev: &Path) -> Vec<Reading> {
    let mut out = Vec::new();
    let Some(hw) = fs::read_dir(dev.join("hwmon"))
        .ok()
        .and_then(|mut d| d.next())
        .and_then(Result::ok)
        .map(|e| e.path())
    else {
        return out;
    };
    let Ok(rd) = fs::read_dir(&hw) else { return out };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        let Some(base) = n.strip_suffix("_input") else { continue };
        let (kind, scale) = match base.trim_end_matches(|c: char| c.is_ascii_digit()) {
            "in" => ("volts", 1e-3),
            "curr" => ("amperes", 1e-3),
            "power" => ("watts", 1e-6),
            "temp" => ("celsius", 1e-3),
            "fan" => ("rpm", 1.0),
            _ => continue,
        };
        let label = fs::read_to_string(hw.join(format!("{base}_label")))
            .map_or_else(|_| base.to_string(), |s| s.trim().to_string());
        if let Some(v) = fs::read_to_string(hw.join(&n))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
        {
            out.push((label, kind, v * scale));
        }
    }
    out
}

/// Prometheus text for the PSUs.
#[must_use]
pub fn metrics(reg08: Option<u8>, readings: &[(u8, Vec<Reading>)]) -> String {
    let mut s = String::new();
    let fam = |s: &mut String, name: &str, help: &str| {
        let _ = writeln!(s, "# HELP {name} {help}\n# TYPE {name} gauge");
    };
    if let Some(r) = reg08 {
        for (name, help, f) in [
            ("switch_psu_present", "1 if a PSU is in the bay (system CPLD).", 0usize),
            (
                "switch_psu_input_good",
                "1 if the PSU's AC input is good (system CPLD).",
                1,
            ),
            (
                "switch_psu_output_good",
                "1 if the PSU's output is good (system CPLD).",
                2,
            ),
        ] {
            fam(&mut s, name, help);
            for (psu, _) in SLOTS {
                let st = status(r, psu);
                let v = match f {
                    0 => st.present,
                    1 => st.input_good,
                    _ => st.output_good,
                };
                let _ = writeln!(s, "{name}{{psu=\"{psu}\"}} {}", u8::from(v));
            }
        }
    }
    for (kind, name, help) in [
        (
            "volts",
            "switch_psu_voltage_volts",
            "PSU PMBus voltage reading (sensor = vin / vout1 ...).",
        ),
        ("amperes", "switch_psu_current_amperes", "PSU PMBus current reading."),
        (
            "watts",
            "switch_psu_power_watts",
            "PSU PMBus power reading (pin = from the wall, pout1 = delivered).",
        ),
        ("celsius", "switch_psu_temperature_celsius", "PSU PMBus temperature."),
        ("rpm", "switch_psu_fan_rpm", "PSU fan speed."),
    ] {
        let rows: Vec<String> = readings
            .iter()
            .flat_map(|(psu, r)| {
                r.iter()
                    .filter(|(_, k, _)| *k == kind)
                    .map(move |(l, _, v)| format!("{name}{{psu=\"{psu}\",sensor=\"{l}\"}} {v}"))
            })
            .collect();
        if !rows.is_empty() {
            fam(&mut s, name, help);
            for r in rows {
                let _ = writeln!(s, "{r}");
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        // 2026-11-26: PSU2 fed (input + output good), PSU1 without AC, both present
        assert_eq!(
            status(0x4a, 2),
            Status {
                present: true,
                input_good: true,
                output_good: true
            }
        );
        assert_eq!(
            status(0x4a, 1),
            Status {
                present: true,
                input_good: false,
                output_good: false
            }
        );
        assert_eq!(
            status(0x45, 1),
            Status {
                present: true,
                input_good: true,
                output_good: true
            }
        );
        assert!((linear11(0xebaa) - 117.25).abs() < 1e-9);
        assert!((linear11(0x02b4) - 692.0).abs() < 1e-9);
        let m = metrics(Some(0x4a), &[(2, vec![("pin".into(), "watts", 692.0)])]);
        assert!(m.contains("switch_psu_input_good{psu=\"1\"} 0"));
        assert!(m.contains("switch_psu_power_watts{psu=\"2\",sensor=\"pin\"} 692"));
    }
}

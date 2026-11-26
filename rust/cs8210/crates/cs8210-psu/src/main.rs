//! `cs8210-psu`: bind the PSUs' pmbus driver, print their status and readings.
#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

use cs8210_psu::{PMBUS_ADDR, Reading, SLOTS, adapter, hwmon_readings, metrics, status};
use cs8210_sys::i2cdev::I2cDev;

const USAGE: &str = "usage: cs8210-psu bind | metrics | show";
const SYS_CPLD: u8 = 0x60;
const SYS_CPLD_NODE: &str = "i2c-mux@74/i2c@0/i2c-mux@71/i2c@2";
const RUN: &str = "/run/cs8210-psu";

macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

fn reg08() -> Option<u8> {
    let dir = std::fs::read_dir("/sys/bus/i2c/devices").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if std::fs::canonicalize(p.join("of_node")).is_ok_and(|n| n.to_string_lossy().ends_with(SYS_CPLD_NODE)) {
            let dev = I2cDev::open(&Path::new("/dev").join(e.file_name())).ok()?;
            return dev.read_byte_data(SYS_CPLD, 0x08).ok();
        }
    }
    None
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string()
}

/// Identity over `PMBus` (before the driver claims the address), then the generic pmbus driver.
fn bind() -> Result<(), String> {
    std::fs::create_dir_all(RUN).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("modprobe").arg("pmbus").status();
    for (psu, ch) in SLOTS {
        let Some((bus, dev)) = adapter(Path::new("/sys"), ch) else {
            eprintln!("PSU {psu}: no mux 0x72 channel {ch}");
            continue;
        };
        if dev.exists() {
            out!("PSU {psu}: i2c-{bus} 0x{PMBUS_ADDR:02x} already bound");
            continue;
        }
        let i2c = I2cDev::open(Path::new(&format!("/dev/i2c-{bus}"))).map_err(|e| e.to_string())?;
        let Ok(rev) = i2c.read_byte_data(PMBUS_ADDR, 0x98) else {
            out!("PSU {psu}: no PMBus answer on i2c-{bus} (bay empty)");
            continue;
        };
        let blk = |r: u8| i2c.read_block_data(PMBUS_ADDR, r).map(|b| text(&b)).unwrap_or_default();
        let id = serde_json::json!({
            "psu": psu, "bus": bus, "pmbus_revision": rev,
            "vendor": blk(0x99), "model": blk(0x9a), "revision": blk(0x9b), "serial": blk(0x9e),
        });
        std::fs::write(format!("{RUN}/psu{psu}.json"), id.to_string()).map_err(|e| e.to_string())?;
        drop(i2c);
        let nd = format!("/sys/bus/i2c/devices/i2c-{bus}/new_device");
        std::fs::write(&nd, format!("pmbus 0x{PMBUS_ADDR:02x}")).map_err(|e| format!("{nd}: {e}"))?;
        out!("PSU {psu}: {id}; pmbus driver bound on i2c-{bus}");
    }
    Ok(())
}

fn readings() -> Vec<(u8, Vec<Reading>)> {
    SLOTS
        .iter()
        .filter_map(|&(psu, ch)| adapter(Path::new("/sys"), ch).map(|(_, dev)| (psu, hwmon_readings(&dev))))
        .collect()
}

fn run() -> Result<(), String> {
    match std::env::args().nth(1).as_deref() {
        Some("bind") => bind(),
        Some("metrics") => {
            let r = reg08();
            let m = metrics(r, &readings());
            if r.is_none() && m.is_empty() {
                return Err("system CPLD unreadable and no PMBus readings".into());
            }
            out!("{}", m.trim_end());
            Ok(())
        }
        Some("show") => {
            let r = reg08();
            for (psu, rd) in readings() {
                let st = r.map(|v| status(v, psu));
                let id = std::fs::read_to_string(format!("{RUN}/psu{psu}.json")).unwrap_or_default();
                out!("PSU {psu}: {st:?} {id}");
                for (l, k, v) in rd {
                    out!("  {l:<8} {v:>10.3} {k}");
                }
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

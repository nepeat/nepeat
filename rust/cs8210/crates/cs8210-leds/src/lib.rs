//! The CS8210-32X's LED controls over the `SoC`'s I2C mux tree.
//!
//! Port LEDs: two LED CPLDs decode the Teralynx ASIC's serial LED stream (`MACsec` LED
//! CPLD 0x63 for cages 1-16, switch LED CPLD 0x64 for cages 17-32). The CPU can only
//! switch a whole CPLD to a test colour (reg 0x09 bit 0 `led_enable`, colour in reg
//! 0x0a bits 2:0) or back to the stream; per-port state comes from the ASIC, whose LED
//! stream needs the vendor's `led_cfg` / `led_refresh_*` register writes at SDK init
//! (devices/cs8210-32x/sonic.md "LEDs"). System LED: system CPLD 0x60 reg 0x20 bits 2:0.
//! PSU and fan LEDs are the system CPLD's own (reg 0x21 test-mode bits clear = automatic).
//!
//! Register meanings: the vendor's `sys_cpld.ko` (disassembly + emulation,
//! devices/cs8210-32x/cpld-diff.md) and diag `leds.yaml`.
#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

/// Where a CPLD sits: the devicetree node of its I2C adapter (mux channel) + address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    /// short name
    pub name: &'static str,
    /// DT node path tail of the adapter
    pub node: &'static str,
    /// 7-bit address
    pub addr: u8,
}

/// `MACsec` LED CPLD: cages 1-16.
pub const MACSEC_LED: Device = Device {
    name: "macsec-led-cpld",
    node: "i2c-mux@74/i2c@1/i2c-mux@73/i2c@0",
    addr: 0x63,
};
/// Switch LED CPLD: cages 17-32.
pub const SWITCH_LED: Device = Device {
    name: "switch-led-cpld",
    node: "i2c-mux@74/i2c@0/i2c-mux@71/i2c@5",
    addr: 0x64,
};
/// System CPLD.
pub const SYSTEM: Device = Device {
    name: "system-cpld",
    node: "i2c-mux@74/i2c@0/i2c-mux@71/i2c@2",
    addr: 0x60,
};

/// LED CPLD registers.
pub mod reg {
    /// version
    pub const VERSION: u8 = 0x00;
    /// scratch (reads 0xde)
    pub const SCRATCH: u8 = 0x01;
    /// bit 0: 1 = every port shows the CPU's test colour, 0 = the ASIC's LED stream
    pub const LED_ENABLE: u8 = 0x09;
    /// bits 2:0: test colour
    pub const COLOUR: u8 = 0x0a;
    /// system CPLD: bits 2:0 system LED, 4:3 PWR1, 6:5 PWR2
    pub const SYS_LEDS: u8 = 0x20;
    /// system CPLD: bits 1:0 fan LED; test mode bits 4 PWR1, 5 PWR2, 7 fan (clear = automatic)
    pub const SYS_LED_MODE: u8 = 0x21;
}

/// A port LED test colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    /// all off
    Off,
    /// red
    Red,
    /// green
    Green,
    /// blue
    Blue,
    /// white (red + green + blue)
    White,
    /// red + green (inferred: the codes look like active-low R/B/G bits)
    Yellow,
    /// green + blue (inferred)
    Cyan,
    /// red + blue (inferred)
    Magenta,
}

impl Colour {
    /// All, in walk order.
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::White,
        Self::Yellow,
        Self::Cyan,
        Self::Magenta,
        Self::Off,
    ];

    /// Register code. Red 6, green 3, blue 5, off 7 (vendor leds.yaml), white 0
    /// (`sys_cpld.ko`); the other three follow from those being active-low bits
    /// (bit 0 red, bit 1 blue, bit 2 green) and are not vendor-documented.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Off => 7,
            Self::Red => 6,
            Self::Green => 3,
            Self::Blue => 5,
            Self::White => 0,
            Self::Yellow => 2,
            Self::Cyan => 1,
            Self::Magenta => 4,
        }
    }

    /// From a register code.
    #[must_use]
    pub fn from_code(c: u8) -> Self {
        match c & 7 {
            6 => Self::Red,
            3 => Self::Green,
            5 => Self::Blue,
            0 => Self::White,
            2 => Self::Yellow,
            1 => Self::Cyan,
            4 => Self::Magenta,
            _ => Self::Off,
        }
    }

    /// Vendor-documented code (not inferred).
    #[must_use]
    pub fn documented(self) -> bool {
        matches!(self, Self::Off | Self::Red | Self::Green | Self::Blue | Self::White)
    }

    /// Parse a name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.to_string() == s.to_ascii_lowercase())
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::White => "white",
            Self::Yellow => "yellow",
            Self::Cyan => "cyan",
            Self::Magenta => "magenta",
        })
    }
}

/// System LED state (system CPLD 0x20 bits 2:0; vendor leds.yaml).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemLed {
    /// 0
    Off,
    /// 1
    Red,
    /// 2
    Amber,
    /// 3
    BlinkRed,
    /// 4
    BlinkAmber,
    /// 5
    BlinkGreen,
    /// 6 (what the vendor driver sets at probe)
    Green,
    /// 7: no vendor meaning (the power-on value here, 0x20 = 0x7f)
    Undefined,
}

impl SystemLed {
    /// Register code.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Red => 1,
            Self::Amber => 2,
            Self::BlinkRed => 3,
            Self::BlinkAmber => 4,
            Self::BlinkGreen => 5,
            Self::Green => 6,
            Self::Undefined => 7,
        }
    }

    /// From bits 2:0.
    #[must_use]
    pub fn from_code(c: u8) -> Self {
        match c & 7 {
            0 => Self::Off,
            1 => Self::Red,
            2 => Self::Amber,
            3 => Self::BlinkRed,
            4 => Self::BlinkAmber,
            5 => Self::BlinkGreen,
            6 => Self::Green,
            _ => Self::Undefined,
        }
    }

    /// Parse a name ("green", "blink-amber", ...).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        (0..7)
            .map(Self::from_code)
            .find(|l| l.to_string() == s.to_ascii_lowercase())
    }
}

impl fmt::Display for SystemLed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::Red => "red",
            Self::Amber => "amber",
            Self::BlinkRed => "blink-red",
            Self::BlinkAmber => "blink-amber",
            Self::BlinkGreen => "blink-green",
            Self::Green => "green",
            Self::Undefined => "undefined(7)",
        })
    }
}

/// PSU / fan LED colour codes (system CPLD, test mode only).
#[must_use]
pub fn psu_fan_colour(c: u8) -> &'static str {
    match c & 3 {
        0 => "off",
        1 => "red",
        2 => "green",
        _ => "amber",
    }
}

/// Register access (the real i2c-dev or a test fake).
pub trait Bus {
    /// read register `reg` of device `d`
    fn read(&mut self, d: Device, reg: u8) -> Result<u8, String>;
    /// write it
    fn write(&mut self, d: Device, reg: u8, v: u8) -> Result<(), String>;
}

/// Write, then read back.
pub fn write_checked<B: Bus>(b: &mut B, d: Device, reg: u8, v: u8, mask: u8) -> Result<(), String> {
    b.write(d, reg, v)?;
    let back = b.read(d, reg)?;
    if back & mask != v & mask {
        return Err(format!(
            "{} reg {reg:#04x}: wrote {v:#04x}, reads back {back:#04x}",
            d.name
        ));
    }
    Ok(())
}

/// Show a test colour on one LED CPLD (all its 16 ports).
pub fn set_test<B: Bus>(b: &mut B, d: Device, c: Colour) -> Result<(), String> {
    let cur = b.read(d, reg::COLOUR)?;
    write_checked(b, d, reg::COLOUR, (cur & !7) | c.code(), 7)?;
    let en = b.read(d, reg::LED_ENABLE)?;
    write_checked(b, d, reg::LED_ENABLE, en | 1, 1)
}

/// Back to the ASIC's LED stream (test colour off too).
pub fn set_stream<B: Bus>(b: &mut B, d: Device) -> Result<(), String> {
    let en = b.read(d, reg::LED_ENABLE)?;
    write_checked(b, d, reg::LED_ENABLE, en & !1, 1)?;
    let cur = b.read(d, reg::COLOUR)?;
    write_checked(b, d, reg::COLOUR, (cur & !7) | Colour::Off.code(), 7)
}

/// Set the system LED, keeping the PSU bits; true if it changed.
pub fn set_system<B: Bus>(b: &mut B, l: SystemLed) -> Result<bool, String> {
    let cur = b.read(SYSTEM, reg::SYS_LEDS)?;
    let new = (cur & !7) | l.code();
    if new == cur {
        return Ok(false);
    }
    write_checked(b, SYSTEM, reg::SYS_LEDS, new, 0xff)?;
    Ok(true)
}

/// The I2C adapter (/dev/i2c-N) whose DT node ends in `tail`.
#[must_use]
pub fn adapter(sys: &Path, tail: &str) -> Option<PathBuf> {
    let dir = std::fs::read_dir(sys.join("bus/i2c/devices")).ok()?;
    let mut hits: Vec<PathBuf> = dir
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("i2c-"))
        .filter(|e| {
            std::fs::canonicalize(e.path().join("of_node"))
                .is_ok_and(|p| p.to_string_lossy().ends_with(&format!("/{tail}")))
        })
        .map(|e| PathBuf::from("/dev").join(e.file_name()))
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// i2c-dev, buses found by DT node.
#[derive(Default)]
pub struct I2c {
    open: Vec<(&'static str, cs8210_sys::i2cdev::I2cDev)>,
}

impl I2c {
    fn dev(&mut self, d: Device) -> Result<&cs8210_sys::i2cdev::I2cDev, String> {
        if !self.open.iter().any(|(n, _)| *n == d.node) {
            let p = adapter(Path::new("/sys"), d.node)
                .ok_or_else(|| format!("{}: no I2C adapter at DT node {}", d.name, d.node))?;
            let dev = cs8210_sys::i2cdev::I2cDev::open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            self.open.push((d.node, dev));
        }
        self.open
            .iter()
            .find(|(n, _)| *n == d.node)
            .map(|(_, x)| x)
            .ok_or_else(|| "adapter".into())
    }
}

impl Bus for I2c {
    fn read(&mut self, d: Device, reg: u8) -> Result<u8, String> {
        let dev = self.dev(d)?;
        dev.read_byte_data(d.addr, reg)
            .map_err(|e| format!("{} {:#04x} reg {reg:#04x}: {e}", dev.path().display(), d.addr))
    }

    fn write(&mut self, d: Device, reg: u8, v: u8) -> Result<(), String> {
        let dev = self.dev(d)?;
        dev.write_byte_data(d.addr, reg, v)
            .map_err(|e| format!("{} {:#04x} reg {reg:#04x}: {e}", dev.path().display(), d.addr))
    }
}

/// What the system LED should show: green when fand runs without failsafe and the I2C
/// FPGA is not marked dead; blinking amber otherwise.
#[must_use]
pub fn wanted_system(fand_state: Option<&str>, fand_age_s: Option<u64>, fpga_dead: bool) -> (SystemLed, String) {
    if fpga_dead {
        return (SystemLed::BlinkAmber, "I2C FPGA marked dead".into());
    }
    let Some(text) = fand_state else {
        return (SystemLed::BlinkAmber, "no fand state".into());
    };
    if fand_age_s.is_none_or(|a| a > 60) {
        return (SystemLed::BlinkAmber, "fand state stale".into());
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) if v.get("failsafe") == Some(&serde_json::Value::Bool(false)) => {
            (SystemLed::Green, "fand running, FPGA alive".into())
        }
        Ok(_) => (SystemLed::BlinkAmber, "fand in failsafe".into()),
        Err(_) => (SystemLed::BlinkAmber, "fand state unreadable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake(HashMap<(u8, u8), u8>);
    impl Bus for Fake {
        fn read(&mut self, d: Device, reg: u8) -> Result<u8, String> {
            Ok(*self.0.get(&(d.addr, reg)).unwrap_or(&0))
        }
        fn write(&mut self, d: Device, reg: u8, v: u8) -> Result<(), String> {
            self.0.insert((d.addr, reg), v);
            Ok(())
        }
    }

    #[test]
    fn colours_roundtrip() {
        for c in Colour::ALL {
            assert_eq!(Colour::from_code(c.code()), c);
            assert_eq!(Colour::parse(&c.to_string()), Some(c));
        }
        assert_eq!(Colour::Red.code(), 6);
        assert!(!Colour::Yellow.documented());
        assert_eq!(SystemLed::parse("blink-green"), Some(SystemLed::BlinkGreen));
        assert_eq!(SystemLed::from_code(0x7f), SystemLed::Undefined);
    }

    #[test]
    fn test_and_stream_and_system() {
        let mut b = Fake::default();
        b.0.insert((0x64, reg::COLOUR), 0x07);
        b.0.insert((0x60, reg::SYS_LEDS), 0x7f);
        set_test(&mut b, SWITCH_LED, Colour::Green).unwrap();
        assert_eq!(b.0[&(0x64, reg::LED_ENABLE)], 1);
        assert_eq!(b.0[&(0x64, reg::COLOUR)], 3);
        set_stream(&mut b, SWITCH_LED).unwrap();
        assert_eq!((b.0[&(0x64, reg::LED_ENABLE)], b.0[&(0x64, reg::COLOUR)]), (0, 7));
        assert!(set_system(&mut b, SystemLed::Green).unwrap());
        assert_eq!(
            b.0[&(0x60, reg::SYS_LEDS)],
            0x7e,
            "PSU bits kept (the vendor's normal 0x7e)"
        );
        assert!(!set_system(&mut b, SystemLed::Green).unwrap());
    }

    #[test]
    fn system_policy() {
        assert_eq!(
            wanted_system(Some(r#"{"failsafe": false}"#), Some(2), false).0,
            SystemLed::Green
        );
        assert_eq!(
            wanted_system(Some(r#"{"failsafe": true}"#), Some(2), false).0,
            SystemLed::BlinkAmber
        );
        assert_eq!(
            wanted_system(Some(r#"{"failsafe": false}"#), Some(600), false).0,
            SystemLed::BlinkAmber
        );
        assert_eq!(wanted_system(None, None, false).0, SystemLed::BlinkAmber);
        assert_eq!(
            wanted_system(Some(r#"{"failsafe": false}"#), Some(2), true).0,
            SystemLed::BlinkAmber
        );
    }
}

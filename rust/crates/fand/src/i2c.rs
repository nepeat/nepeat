//! I2C/SMBus register access for `type: i2c` sensors and outputs.
//!
//! The real backend is Linux i2c-dev: open `/dev/i2c-N`, `I2C_SLAVE` (or
//! `I2C_SLAVE_FORCE`), then one `I2C_SMBUS` byte- or word-data transfer, via
//! `libc` only. Each transfer opens the device afresh, so a bus that appears
//! late (module load) or is re-created needs no resolution state.
//!
//! Under a test root (`sysfs_root` other than `/`) the device path is
//! rebased, and if `<root>/dev/i2c-N` is a *directory* it is a fake register
//! file: `<root>/dev/i2c-N/0x68/0x50` holds register 0x50 of address 0x68 as
//! a number. A missing file reads as a NACK; writes never create files.
//! Tests that need device behaviour (a CPLD that takes over) implement
//! [`Bus`] themselves, see `fakesys::FakeCpld`.

use crate::sysfs;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One device on one bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub bus: u32,
    pub address: u16,
    /// I2C_SLAVE_FORCE: talk to the address even if a kernel driver owns it.
    pub force: bool,
}

impl fmt::Display for Target {
    /// Linux's device name: "4-0068".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{:04x}", self.bus, self.address)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    #[default]
    Byte,
    /// SMBus word: little-endian on the wire.
    Word,
}

impl Width {
    pub fn max(self) -> u32 {
        match self {
            Width::Byte => 0xff,
            Width::Word => 0xffff,
        }
    }
}

/// Raw SMBus byte/word-data access. Values are the wire value (a word as the
/// SMBus little-endian number); byte order and sign are applied by [`Reg`].
pub trait Bus: Send + Sync {
    fn read(&self, t: Target, reg: u8, width: Width) -> Result<u16, String>;
    fn write(&self, t: Target, reg: u8, width: Width, value: u16) -> Result<(), String>;
}

/// A register: device, register number, width, byte order, sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reg {
    pub target: Target,
    pub reg: u8,
    pub width: Width,
    /// Word registers with the high byte first (LM75-style).
    pub big_endian: bool,
    pub signed: bool,
}

impl fmt::Display for Reg {
    /// "4-0068:0x50".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:0x{:02x}", self.target, self.reg)
    }
}

impl Reg {
    fn swap(&self, v: u16) -> u16 {
        if self.width == Width::Word && self.big_endian {
            v.swap_bytes()
        } else {
            v
        }
    }

    /// The register as an unsigned number (byte order applied, no sign).
    pub fn read_raw(&self, bus: &dyn Bus) -> Result<u32, String> {
        bus.read(self.target, self.reg, self.width)
            .map(|v| u32::from(self.swap(v)))
            .map_err(|e| format!("i2c {self}: read: {e}"))
    }

    /// The register as a number, sign-extended if `signed`.
    pub fn read(&self, bus: &dyn Bus) -> Result<f64, String> {
        let v = self.read_raw(bus)?;
        Ok(match (self.signed, self.width) {
            (false, _) => f64::from(v),
            (true, Width::Byte) => f64::from(v as u8 as i8),
            (true, Width::Word) => f64::from(v as u16 as i16),
        })
    }

    pub fn write(&self, bus: &dyn Bus, value: u32) -> Result<(), String> {
        if value > self.width.max() {
            return Err(format!("i2c {self}: {value} does not fit the register"));
        }
        bus.write(self.target, self.reg, self.width, self.swap(value as u16))
            .map_err(|e| format!("i2c {self}: writing 0x{value:02x}: {e}"))
    }
}

/// i2c-dev, or the register-file fake under a test root.
pub struct System {
    root: PathBuf,
}

impl System {
    pub fn new(root: &Path) -> System {
        System {
            root: root.to_path_buf(),
        }
    }

    fn dev(&self, bus: u32) -> PathBuf {
        sysfs::rebase(&self.root, Path::new(&format!("/dev/i2c-{bus}")))
    }

    fn fake_file(dir: &Path, t: Target, reg: u8) -> PathBuf {
        dir.join(format!("0x{:02x}", t.address)).join(format!("0x{reg:02x}"))
    }
}

impl Bus for System {
    fn read(&self, t: Target, reg: u8, width: Width) -> Result<u16, String> {
        let dev = self.dev(t.bus);
        if dev.is_dir() {
            let p = Self::fake_file(&dev, t, reg);
            let s = fs::read_to_string(&p).map_err(|_| "no acknowledge (fake register missing)".to_string())?;
            let v = parse_int(s.trim())?;
            return u16::try_from(v)
                .ok()
                .filter(|v| u32::from(*v) <= width.max())
                .ok_or_else(|| format!("fake register holds {v}, too wide"));
        }
        linux::transfer(&dev, t, reg, width, None)
    }

    fn write(&self, t: Target, reg: u8, width: Width, value: u16) -> Result<(), String> {
        let dev = self.dev(t.bus);
        if dev.is_dir() {
            let p = Self::fake_file(&dev, t, reg);
            if !p.exists() {
                return Err("no acknowledge (fake register missing)".into());
            }
            return sysfs::write_value(&p, &value.to_string()).map_err(|e| e.to_string());
        }
        linux::transfer(&dev, t, reg, width, Some(value)).map(|_| ())
    }
}

/// "104", "0x68", "0X68".
pub fn parse_int(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let r = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16),
        None => t.parse::<u64>(),
    };
    r.map_err(|_| format!("not an integer: {t:?} (use e.g. 104 or 0x68)"))
}

/// The i2c-dev ioctls. The only unsafe code besides `exec::kill_group`.
#[allow(unsafe_code)]
mod linux {
    use super::{Target, Width, io};
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;
    use std::path::Path;

    // <linux/i2c-dev.h>, <linux/i2c.h>
    const I2C_SLAVE: libc::c_ulong = 0x0703;
    const I2C_SLAVE_FORCE: libc::c_ulong = 0x0706;
    const I2C_SMBUS: libc::c_ulong = 0x0720;
    const I2C_SMBUS_READ: u8 = 1;
    const I2C_SMBUS_WRITE: u8 = 0;
    const I2C_SMBUS_BYTE_DATA: u32 = 2;
    const I2C_SMBUS_WORD_DATA: u32 = 3;

    #[repr(C)]
    union SmbusData {
        byte: u8,
        word: u16,
        block: [u8; 34],
    }

    #[repr(C)]
    struct SmbusIoctlData {
        read_write: u8,
        command: u8,
        size: u32,
        data: *mut SmbusData,
    }

    fn os_err(what: &str) -> String {
        format!("{what}: {}", io::Error::last_os_error())
    }

    pub fn transfer(dev: &Path, t: Target, reg: u8, width: Width, write: Option<u16>) -> Result<u16, String> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dev)
            .map_err(|e| format!("{}: {e}", dev.display()))?;
        let fd = f.as_raw_fd();
        let req = if t.force { I2C_SLAVE_FORCE } else { I2C_SLAVE };
        // SAFETY: plain integer argument; fd is open for the call's duration.
        if unsafe { libc::ioctl(fd, req as _, libc::c_ulong::from(t.address)) } < 0 {
            let hint = if t.force {
                ""
            } else {
                " (a kernel driver may own it; see 'force')"
            };
            return Err(format!(
                "{}{hint}",
                os_err(&format!("claiming address 0x{:02x}", t.address))
            ));
        }
        let mut data = SmbusData { block: [0; 34] };
        match (write, width) {
            (Some(v), Width::Byte) => data.byte = v as u8,
            (Some(v), Width::Word) => data.word = v,
            (None, _) => {}
        }
        let mut args = SmbusIoctlData {
            read_write: if write.is_some() {
                I2C_SMBUS_WRITE
            } else {
                I2C_SMBUS_READ
            },
            command: reg,
            size: match width {
                Width::Byte => I2C_SMBUS_BYTE_DATA,
                Width::Word => I2C_SMBUS_WORD_DATA,
            },
            data: &mut data,
        };
        // SAFETY: args and data are valid, properly laid out (#[repr(C)] as in
        // <linux/i2c-dev.h>) and outlive the call; the kernel writes at most
        // the 34-byte union.
        if unsafe { libc::ioctl(fd, I2C_SMBUS as _, &mut args as *mut SmbusIoctlData) } < 0 {
            return Err(os_err(if write.is_some() { "smbus write" } else { "smbus read" }));
        }
        // SAFETY: the union was zero-initialised as a whole, every field is
        // plain bytes.
        Ok(unsafe {
            match width {
                Width::Byte => u16::from(data.byte),
                Width::Word => data.word,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakesys::FakeSys;

    #[test]
    fn register_files_under_a_test_root() {
        let fs = FakeSys::new("i2c");
        fs.i2c_reg(4, 0x68, 0x50, 42);
        fs.i2c_reg(7, 0x48, 0x00, 0x80e7); // LM75 word, big-endian on the wire: 0xe780
        let bus = System::new(&fs.root);
        let t = Target {
            bus: 4,
            address: 0x68,
            force: false,
        };
        let pwm = Reg {
            target: t,
            reg: 0x50,
            width: Width::Byte,
            big_endian: false,
            signed: false,
        };
        assert_eq!(pwm.to_string(), "4-0068:0x50");
        assert_eq!(pwm.read(&bus).unwrap(), 42.0);
        pwm.write(&bus, 9).unwrap();
        assert_eq!(pwm.read_raw(&bus).unwrap(), 9);
        assert!(pwm.write(&bus, 300).unwrap_err().contains("does not fit"));
        let missing = Reg { reg: 0x51, ..pwm };
        assert!(
            missing
                .read(&bus)
                .unwrap_err()
                .contains("i2c 4-0068:0x51: read: no acknowledge")
        );
        assert!(missing.write(&bus, 1).is_err(), "never creates registers");
        let temp = Reg {
            target: Target {
                bus: 7,
                address: 0x48,
                force: false,
            },
            reg: 0,
            width: Width::Word,
            big_endian: true,
            signed: true,
        };
        // 0xe780 as i16 = -6272; /256 = -24.5 C.
        assert_eq!(temp.read(&bus).unwrap() / 256.0, -24.5);
        // Real i2c-dev path that does not exist: an error, not a panic.
        let none = System::new(Path::new("/nonexistent-root"));
        assert!(
            none.read(t, 0x50, Width::Byte)
                .unwrap_err()
                .contains("/nonexistent-root/dev/i2c-4")
        );
    }

    #[test]
    fn integers_in_decimal_and_hex() {
        assert_eq!(parse_int("104").unwrap(), 104);
        assert_eq!(parse_int("0x68").unwrap(), 104);
        assert_eq!(parse_int("0X2A").unwrap(), 42);
        assert!(parse_int("0xzz").is_err());
        assert!(parse_int("-1").is_err());
    }
}

//! A fake sysfs tree in a temporary directory, for tests and simulation.
//! Mirrors the real layout: /sys/class/hwmon/hwmonN are symlinks into
//! /sys/devices/..., each with a `device` link and optional `of_node`.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct FakeSys {
    pub root: PathBuf,
}

impl FakeSys {
    pub fn new(tag: &str) -> FakeSys {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("fand-test-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sys/class/hwmon")).expect("create fake sysfs");
        FakeSys { root }
    }

    /// Write a file relative to the root (parents created), return its path.
    pub fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.root.join(rel.trim_start_matches('/'));
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).expect("mkdir");
        }
        fs::write(&p, format!("{content}\n")).expect("write fake attr");
        p
    }

    /// Create hwmon<index> named `name` under /sys/devices/<device>, with an
    /// optional devicetree node /sys/firmware/devicetree/base/<of_node>.
    pub fn hwmon(&self, index: u32, name: &str, device: &str, of_node: Option<&str>) -> PathBuf {
        let dev = self.root.join("sys/devices").join(device);
        let dir = dev.join(format!("hwmon/hwmon{index}"));
        fs::create_dir_all(&dir).expect("mkdir hwmon");
        fs::write(dir.join("name"), format!("{name}\n")).expect("name");
        symlink(&dev, dir.join("device")).expect("device link");
        if let Some(node) = of_node {
            let n = self.root.join("sys/firmware/devicetree/base").join(node);
            fs::create_dir_all(&n).expect("of_node");
            symlink(&n, dev.join("of_node")).expect("of_node link");
        }
        symlink(&dir, self.root.join(format!("sys/class/hwmon/hwmon{index}"))).expect("class link");
        self.root.join(format!("sys/class/hwmon/hwmon{index}"))
    }

    /// Set an attribute in a hwmon (or any) directory.
    pub fn set(&self, dir: &Path, attr: &str, value: &str) {
        fs::write(dir.join(attr), format!("{value}\n")).expect("set attr");
    }

    pub fn get(&self, dir: &Path, attr: &str) -> String {
        fs::read_to_string(dir.join(attr))
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    pub fn remove(&self, dir: &Path, attr: &str) {
        let _ = fs::remove_file(dir.join(attr));
    }

    /// A register in the fake I2C register files (see `crate::i2c`):
    /// `<root>/dev/i2c-<bus>/0x<addr>/0x<reg>`. Returns its path.
    pub fn i2c_reg(&self, bus: u32, addr: u16, reg: u8, value: u32) -> PathBuf {
        self.write(&format!("dev/i2c-{bus}/0x{addr:02x}/0x{reg:02x}"), &value.to_string())
    }
}

impl Drop for FakeSys {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A simulated board CPLD in the style of the Edgecore DMC-12's (Lattice
/// MachXO3 at one I2C address), for closed-loop tests of `type: i2c`
/// outputs. Registers:
///
/// - `0x01` version (read-only);
/// - `0x2a` WDT flag: 3 after a reset; while bit 1 is set the CPLD runs the
///   fans at full speed whatever 0x50 says. Writing 0 clears it, and it then
///   reads 1 (as observed on u01), unless `stuck_flag` is set;
/// - `0x50` PWM step (0 off, 1..steps; duty = 4.6 + (step-1) * 95.4/62 on
///   a 64-step CPLD);
/// - `0x60..` tachs, RPM / 100: full speed while the flag is set, else
///   following the step, times a per-fan factor (0 = stalled).
///
/// Any other register reads 0xcc. `dead` makes every transfer fail (a hung
/// bus), and every write is logged in `writes`.
pub struct FakeCpld {
    pub state: std::sync::Mutex<CpldState>,
}

pub struct CpldState {
    pub bus: u32,
    pub address: u16,
    pub version: u8,
    pub flag: u8,
    pub step: u8,
    pub steps: u8,
    pub full_rpm: f64,
    pub fan_factor: Vec<f64>,
    pub dead: bool,
    pub stuck_flag: bool,
    pub writes: Vec<(u8, u16)>,
}

impl FakeCpld {
    /// A v7+ (64-step) CPLD with three fans, just after a reset: flag 3,
    /// step 9, fans at full speed.
    pub fn new(bus: u32, address: u16) -> FakeCpld {
        FakeCpld {
            state: std::sync::Mutex::new(CpldState {
                bus,
                address,
                version: 0x88,
                flag: 3,
                step: 9,
                steps: 63,
                full_rpm: 14400.0,
                fan_factor: vec![1.0; 3],
                dead: false,
                stuck_flag: false,
                writes: Vec::new(),
            }),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut CpldState) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// What a watchdog/CPLD reset does: flag 3, step 9.
    pub fn reset(&self) {
        self.with(|s| {
            s.flag = 3;
            s.step = 9;
        });
    }
}

impl CpldState {
    pub fn duty(&self) -> f64 {
        match self.step {
            0 => 0.0,
            st if self.steps == 63 => 4.6 + f64::from(st - 1) * 95.4 / 62.0,
            st => 17.6 + f64::from(st - 1) * 82.4 / 14.0,
        }
    }

    pub fn taken_over(&self) -> bool {
        self.flag & 2 != 0
    }

    pub fn rpm(&self, fan: usize) -> f64 {
        let f = self.fan_factor.get(fan).copied().unwrap_or(0.0);
        let d = if self.taken_over() {
            100.0
        } else {
            self.duty().min(100.0)
        };
        (self.full_rpm * d / 100.0 * f / 100.0).round() * 100.0
    }
}

impl crate::i2c::Bus for FakeCpld {
    fn read(&self, t: crate::i2c::Target, reg: u8, _w: crate::i2c::Width) -> Result<u16, String> {
        self.with(|s| {
            if s.dead || t.bus != s.bus || t.address != s.address {
                return Err("Remote I/O error (os error 121)".into());
            }
            Ok(match reg {
                0x01 => u16::from(s.version),
                0x2a => u16::from(s.flag),
                0x50 => u16::from(s.step),
                0x60..=0x6f if usize::from(reg - 0x60) < s.fan_factor.len() => {
                    (s.rpm(usize::from(reg - 0x60)) / 100.0) as u16
                }
                _ => 0xcc,
            })
        })
    }

    fn write(&self, t: crate::i2c::Target, reg: u8, _w: crate::i2c::Width, v: u16) -> Result<(), String> {
        self.with(|s| {
            if s.dead || t.bus != s.bus || t.address != s.address {
                return Err("Remote I/O error (os error 121)".into());
            }
            s.writes.push((reg, v));
            match reg {
                0x2a if !s.stuck_flag => s.flag = if v == 0 { 1 } else { v as u8 },
                0x50 => s.step = (v as u8).min(s.steps),
                _ => {}
            }
            Ok(())
        })
    }
}

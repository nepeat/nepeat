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

/// A simulated fan-board CPLD in the style of the Celestica CS8210-32X's
/// (I2C 0x66 on one mux channel), for `present`, `led` and isolation tests.
/// Slot `n` (1-based) has its registers at `0x20 + 0x10 * (n - 1)`:
///
/// - `+0` / `+1` front / rear tach, RPM / 100, following `+2` linearly up
///   to `full_rpm` at 255, times the rotor's factor (0 = stalled); 0 while
///   the slot is empty;
/// - `+2` duty, raw 0..255;
/// - `+4` LED, bits 1:0 (1 green, 2 red, 3 off), the other bits kept;
/// - `+6` presence, bit 0: 0 = present.
///
/// `0x00` is the version (3); `0x05` a "force every fan to full speed"
/// register (0xff = forced: every duty register reads and acts 0xff;
/// `force_stuck` ignores writes to it). Any other register reads 0xcc.
/// Slots whose `present_unreadable` is set NACK their presence register;
/// every write is logged in `writes`.
///
/// A `stuck` rotor reads 0 (windmilled backwards by the others' backdraft)
/// until every slot has run at or below `unstick_raw` for `unstick_secs`
/// (by `now`, which the test advances). A rotor with factor 0 is dead.
pub struct FakeFanCpld {
    pub state: std::sync::Mutex<FanCpldState>,
}

pub struct FanCpldState {
    pub bus: u32,
    pub address: u16,
    pub version: u8,
    pub full_rpm: f64,
    pub duty: Vec<u8>,
    pub led: Vec<u8>,
    pub present: Vec<bool>,
    pub present_unreadable: Vec<bool>,
    /// Per slot: [front, rear] speed factors.
    pub factor: Vec<[f64; 2]>,
    pub writes: Vec<(u8, u16)>,
    pub now: f64,
    pub stuck: Vec<[bool; 2]>,
    pub unstick_raw: u8,
    pub unstick_secs: f64,
    low_since: Option<f64>,
    pub force: u8,
    pub force_stuck: bool,
    /// Register reads so far.
    pub reads: usize,
}

impl FakeFanCpld {
    /// `slots` present modules at full speed, LEDs off with the upper bits set (0xf3).
    pub fn new(bus: u32, address: u16, slots: usize) -> FakeFanCpld {
        FakeFanCpld {
            state: std::sync::Mutex::new(FanCpldState {
                bus,
                address,
                version: 3,
                full_rpm: 19000.0,
                duty: vec![0xff; slots],
                led: vec![0xf3; slots],
                present: vec![true; slots],
                present_unreadable: vec![false; slots],
                factor: vec![[1.0, 1.0]; slots],
                writes: Vec::new(),
                now: 0.0,
                stuck: vec![[false, false]; slots],
                unstick_raw: 0x10,
                unstick_secs: 8.0,
                low_since: None,
                force: 0,
                force_stuck: false,
                reads: 0,
            }),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FanCpldState) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl FanCpldState {
    /// Slot index (0-based) and register offset of a register, if in a slot.
    fn slot(&self, reg: u8) -> Option<(usize, u8)> {
        let r = usize::from(reg);
        if r < 0x20 {
            return None;
        }
        let s = (r - 0x20) / 0x10;
        (s < self.duty.len()).then_some((s, reg & 0x0f))
    }

    pub fn forced(&self) -> bool {
        self.force == 0xff
    }

    /// The duty a slot runs at (0xff while forced).
    pub fn effective(&self, slot: usize) -> u8 {
        if self.forced() { 0xff } else { self.duty[slot] }
    }

    /// Unstick rotors after a long enough calm (called on every access).
    pub fn advance(&mut self) {
        let low = (0..self.duty.len()).all(|s| self.effective(s) <= self.unstick_raw);
        if !low {
            self.low_since = None;
            return;
        }
        let since = *self.low_since.get_or_insert(self.now);
        if self.now - since >= self.unstick_secs {
            self.stuck.fill([false, false]);
        }
    }

    pub fn rpm(&self, slot: usize, rotor: usize) -> f64 {
        if !self.present[slot] || self.stuck[slot][rotor] {
            return 0.0;
        }
        let f = self.factor[slot][rotor];
        (self.full_rpm * f64::from(self.effective(slot)) / 255.0 * f / 100.0).round() * 100.0
    }
}

impl crate::i2c::Bus for FakeFanCpld {
    fn read(&self, t: crate::i2c::Target, reg: u8, _w: crate::i2c::Width) -> Result<u16, String> {
        self.with(|s| {
            if t.bus != s.bus || t.address != s.address {
                return Err("Remote I/O error (os error 121)".into());
            }
            s.advance();
            s.reads += 1;
            if reg == 0 {
                return Ok(u16::from(s.version));
            }
            if reg == 5 {
                return Ok(u16::from(s.force));
            }
            Ok(match s.slot(reg) {
                Some((n, 0)) => (s.rpm(n, 0) / 100.0) as u16,
                Some((n, 1)) => (s.rpm(n, 1) / 100.0) as u16,
                Some((n, 2)) => u16::from(s.effective(n)),
                Some((n, 4)) => u16::from(s.led[n]),
                Some((n, 6)) if s.present_unreadable[n] => return Err("Remote I/O error (os error 121)".into()),
                Some((n, 6)) => u16::from(!s.present[n]) | 0x02,
                _ => 0xcc,
            })
        })
    }

    fn write(&self, t: crate::i2c::Target, reg: u8, _w: crate::i2c::Width, v: u16) -> Result<(), String> {
        self.with(|s| {
            if t.bus != s.bus || t.address != s.address {
                return Err("Remote I/O error (os error 121)".into());
            }
            s.writes.push((reg, v));
            match s.slot(reg) {
                Some((n, 2)) => s.duty[n] = v as u8,
                Some((n, 4)) => s.led[n] = v as u8,
                _ if reg == 5 && !s.force_stuck => s.force = v as u8,
                _ => {}
            }
            s.advance();
            Ok(())
        })
    }
}

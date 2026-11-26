//! A simulated I2C FPGA (BAR0 register protocol + 32 cages of [`SimModule`]s), for the
//! tests and for `cs8210-optics daemon --simulate DIR` (the `SONiC` platform tests).
//!
//! `DIR` uses the fake-board layout of `ai_re/nix/modules/boards/cs8210/fake_board.py`:
//! `port<N>.bin` = the module in cage N (optoe image), `sideband.json` =
//! `{"reset": mask, "lpmode": mask, "int": mask, "uninit": [cages at power-up channel
//! values], "dead": bool}`.

use std::path::Path;

use cs8210_xcvr::sim::SimModule;

use crate::bar::{Bar, BarError};
use crate::regs::{BUFFER_WORDS, CAGES, field};

const VERSION: u32 = 0x010b_0009;
const ALL_ONES: u32 = 0xffff_ffff;

#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    profile: u32,
    select: u32,
    mreset: u32,
    command: u32,
    start: u32,
    status: u32,
}

/// The simulated FPGA.
#[derive(Debug, Clone)]
pub struct SimFpga {
    version: u32,
    reset: u32,
    lpmode: u32,
    interrupt: u32,
    int_mask: u32,
    channels: Vec<Channel>,
    wbuf: Vec<[u32; 32]>,
    rbuf: Vec<[u32; 32]>,
    modules: Vec<Option<SimModule>>,
    dead: bool,
    reads: u64,
    writes: Vec<(usize, u32)>,
}

impl Default for SimFpga {
    fn default() -> Self {
        Self::new()
    }
}

impl SimFpga {
    /// Power-up state: no modules, every cage in reset, channels at power-up values.
    #[must_use]
    pub fn new() -> Self {
        let n = usize::from(CAGES);
        Self {
            version: VERSION,
            reset: ALL_ONES,
            lpmode: 0,
            interrupt: 0,
            int_mask: ALL_ONES,
            channels: vec![
                Channel {
                    profile: field::POWER_UP_PROFILE,
                    ..Channel::default()
                };
                n
            ],
            wbuf: vec![[0; 32]; n],
            rbuf: vec![[0; 32]; n],
            modules: vec![None; n],
            dead: false,
            reads: 0,
            writes: Vec::new(),
        }
    }

    /// From a fake-board directory (see the module docs).
    pub fn from_dir(dir: &Path) -> std::io::Result<Self> {
        let mut s = Self::new();
        let sb: serde_json::Value = std::fs::read_to_string(dir.join("sideband.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(serde_json::Value::Null);
        let mask = |k: &str| {
            sb.get(k)
                .and_then(serde_json::Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
        };
        s.reset = mask("reset").unwrap_or(0);
        s.lpmode = mask("lpmode").unwrap_or(0);
        s.interrupt = mask("int").unwrap_or(0);
        s.dead = sb.get("dead").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let uninit: Vec<u64> = sb
            .get("uninit")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        for (i, ch) in s.channels.iter_mut().enumerate() {
            let cage = i as u64 + 1;
            ch.profile = if uninit.contains(&cage) {
                field::POWER_UP_PROFILE
            } else {
                field::PROBE_PROFILE
            };
        }
        for n in 1..=CAGES {
            let p = dir.join(format!("port{n}.bin"));
            if p.exists() {
                s.insert(n, SimModule::from_image(std::fs::read(&p)?));
            }
        }
        Ok(s)
    }

    /// Plug a module into cage `n`.
    pub fn insert(&mut self, n: u8, m: SimModule) {
        if let Some(slot) = self.modules.get_mut(usize::from(n.saturating_sub(1))) {
            *slot = Some(m);
        }
    }

    /// Pull the module out of cage `n`.
    pub fn remove(&mut self, n: u8) {
        if let Some(slot) = self.modules.get_mut(usize::from(n.saturating_sub(1))) {
            *slot = None;
        }
    }

    /// The module in cage `n`.
    #[must_use]
    pub fn module(&self, n: u8) -> Option<&SimModule> {
        self.modules.get(usize::from(n.saturating_sub(1)))?.as_ref()
    }

    /// The module in cage `n`, mutable.
    pub fn module_mut(&mut self, n: u8) -> Option<&mut SimModule> {
        self.modules.get_mut(usize::from(n.saturating_sub(1)))?.as_mut()
    }

    /// From now on every read returns all ones (a `PCIe` completion timeout) and writes vanish.
    pub fn kill(&mut self) {
        self.dead = true;
    }

    /// Change the version register (as a reconfigured or confused FPGA would).
    pub fn set_version(&mut self, v: u32) {
        self.version = v;
    }

    /// BAR reads so far.
    #[must_use]
    pub fn reads(&self) -> u64 {
        self.reads
    }

    /// Every BAR write so far (offset, value).
    #[must_use]
    pub fn writes(&self) -> &[(usize, u32)] {
        &self.writes
    }

    /// `ResetL` mask.
    #[must_use]
    pub fn reset_mask(&self) -> u32 {
        self.reset
    }

    /// `LPMode` mask.
    #[must_use]
    pub fn lpmode_mask(&self) -> u32 {
        self.lpmode
    }

    fn present(&self) -> u32 {
        self.modules
            .iter()
            .enumerate()
            .filter(|(_, m)| m.is_some())
            .fold(0, |a, (i, _)| a | 1 << i)
    }

    fn run(&mut self, ch: usize) -> u32 {
        let Some(c) = self.channels.get(ch).copied() else {
            return field::STATUS_ERROR;
        };
        let addr = u8::try_from((c.command >> 8) & 0x7f).unwrap_or(0);
        let n = u8::try_from(c.command & 0xff).unwrap_or(0);
        let reg = u8::try_from(c.start & 0xff).unwrap_or(0);
        let read = c.command & field::CMD_READ != 0;
        if c.profile != field::PROBE_PROFILE || c.mreset & field::MRESET_HOLD != 0 || self.reset >> ch & 1 != 0 {
            return field::STATUS_ERROR;
        }
        let wdata: Vec<u8> = self
            .wbuf
            .get(ch)
            .map(|w| w.iter().flat_map(|x| x.to_le_bytes()).take(usize::from(n)).collect())
            .unwrap_or_default();
        let Some(Some(m)) = self.modules.get_mut(ch) else {
            return field::STATUS_ERROR;
        };
        if read {
            match m.read(addr, reg, n) {
                Ok(data) => {
                    if let Some(rb) = self.rbuf.get_mut(ch) {
                        let mut padded = data;
                        padded.resize(128, 0);
                        for (i, w) in padded.chunks(4).enumerate() {
                            if let (Some(slot), Ok(b)) = (rb.get_mut(i), <[u8; 4]>::try_from(w)) {
                                *slot = u32::from_le_bytes(b);
                            }
                        }
                    }
                    0
                }
                Err(_) => field::STATUS_ERROR,
            }
        } else if m.write(addr, reg, &wdata).is_ok() {
            0
        } else {
            field::STATUS_ERROR
        }
    }
}

impl Bar for SimFpga {
    fn read32(&mut self, off: usize) -> Result<u32, BarError> {
        self.reads += 1;
        if self.dead {
            return Ok(ALL_ONES);
        }
        let n = usize::from(CAGES);
        Ok(match off {
            0x000 => self.version,
            0x004 | 0x150 => 0,
            0x140 => self.present(),
            0x160 => self.interrupt,
            0x168 => self.int_mask,
            0x170 => self.lpmode,
            0x178 => self.reset,
            o if (0x1000..0x1000 + 0x100 * n).contains(&o) => {
                let (ch, r) = ((o - 0x1000) / 0x100, (o - 0x1000) % 0x100);
                let c = self.channels.get(ch).copied().unwrap_or_default();
                match r {
                    0x00 => c.profile,
                    0x08 => c.select,
                    0x0c => c.mreset,
                    0x54 => c.start,
                    0x60 => c.status,
                    _ => return Err(BarError(format!("sim: channel register {r:#x} read"))),
                }
            }
            o if (0x8000..0x8000 + 0x200 * n).contains(&o) && (o - 0x8000) % 0x200 < 4 * usize::from(BUFFER_WORDS) => {
                let (ch, w) = ((o - 0x8000) / 0x200, ((o - 0x8000) % 0x200) / 4);
                self.rbuf.get(ch).and_then(|b| b.get(w)).copied().unwrap_or(0)
            }
            o => return Err(BarError(format!("sim: BAR0+{o:#x} read: nothing there"))),
        })
    }

    fn write32(&mut self, off: usize, v: u32) -> Result<(), BarError> {
        self.writes.push((off, v));
        if self.dead {
            return Ok(());
        }
        let n = usize::from(CAGES);
        match off {
            0x168 => self.int_mask = v,
            0x170 => self.lpmode = v,
            0x178 => self.reset = v,
            o if (0x1000..0x1000 + 0x100 * n).contains(&o) => {
                let (ch, r) = ((o - 0x1000) / 0x100, (o - 0x1000) % 0x100);
                let start = r == 0x54 && v & field::START_BUSY != 0;
                if let Some(c) = self.channels.get_mut(ch) {
                    match r {
                        0x00 => c.profile = v,
                        0x08 => c.select = v,
                        0x0c => c.mreset = v & !(1 << 31), // bit 31 self-clears
                        0x50 => c.command = v,
                        0x54 => c.start = v & !field::START_BUSY, // done at once
                        _ => return Err(BarError(format!("sim: channel register {r:#x} written"))),
                    }
                }
                if start {
                    let st = self.run(ch);
                    if let Some(c) = self.channels.get_mut(ch) {
                        c.status = st;
                    }
                }
            }
            o if (0x4000..0x4000 + 0x200 * n).contains(&o) && (o - 0x4000) % 0x200 < 4 * usize::from(BUFFER_WORDS) => {
                let (ch, w) = ((o - 0x4000) / 0x200, ((o - 0x4000) % 0x200) / 4);
                if let Some(slot) = self.wbuf.get_mut(ch).and_then(|b| b.get_mut(w)) {
                    *slot = v;
                }
            }
            o => return Err(BarError(format!("sim: BAR0+{o:#x} written: not writable"))),
        }
        Ok(())
    }
}

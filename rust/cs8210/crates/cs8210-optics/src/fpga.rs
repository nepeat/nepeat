//! The I2C FPGA as operations: sideband snapshot, `ResetL` / `LPMode` bits, channel
//! probe, I2C transfers. Every access goes through [`Reg`]'s allow-list; a register
//! that is never all ones while the FPGA answers (version, start, status, profile)
//! reading 0xffffffff marks the FPGA dead (a `PCIe` completion timeout returns all ones
//! here, once cs8210-pcie-errors made the root ports do that), and after that nothing
//! touches it again.

use std::fmt;
use std::time::Duration;

use crate::bar::Bar;
use crate::regs::{Cage, ChannelReg, Global, Reg, field};

const ALL_ONES: u32 = 0xffff_ffff;

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FpgaError {
    /// The FPGA stopped answering (or was found so); no further access.
    Dead(String),
    /// The module did not acknowledge (status bit 1).
    Nack {
        /// cage
        cage: Cage,
        /// two-wire address
        addr: u8,
        /// register offset
        reg: u8,
    },
    /// The transfer never finished (busy after 50 polls).
    Busy(Cage),
    /// The channel is not in the state the vendor probe leaves it in.
    ChannelNotReady(Cage, String),
    /// A request outside what a transfer can carry, or a register the allow-list refuses.
    Refused(String),
}

impl fmt::Display for FpgaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dead(why) => write!(f, "I2C FPGA dead: {why}"),
            Self::Nack { cage, addr, reg } => write!(f, "{cage}: address {addr:#04x} register {reg:#04x}: NACK"),
            Self::Busy(cage) => write!(f, "{cage}: transfer still busy after {} polls", field::POLLS),
            Self::ChannelNotReady(cage, why) => write!(f, "{cage}: I2C channel not ready: {why}"),
            Self::Refused(why) => write!(f, "refused: {why}"),
        }
    }
}

impl std::error::Error for FpgaError {}

/// The sideband registers at one moment (bit `n - 1` = cage n).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct Sideband {
    /// module present
    pub present: u32,
    /// `ResetL` asserted (held in reset)
    pub reset: u32,
    /// `LPMode` asserted
    pub lpmode: u32,
    /// `IntL` asserted
    pub interrupt: u32,
}

/// A channel's configuration registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelState {
    /// clock profiles
    pub profile: u32,
    /// profile select
    pub select: u32,
    /// master reset register
    pub master_reset: u32,
    /// start / busy
    pub start: u32,
}

impl ChannelState {
    /// Why this is not the vendor probe's state, or None.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if self.master_reset & field::MRESET_HOLD != 0 {
            Some("master held in reset".into())
        } else if self.profile != field::PROBE_PROFILE {
            Some(format!(
                "clock profiles {:#010x}, not the probe's {:#010x}",
                self.profile,
                field::PROBE_PROFILE
            ))
        } else if self.select & 3 == 3 {
            Some("profile 4 selected".into())
        } else if self.start & field::START_BUSY != 0 {
            Some("a transfer is still busy".into())
        } else {
            None
        }
    }

    /// SCL rate of the selected profile.
    #[must_use]
    pub fn clock_hz(&self) -> Option<u32> {
        let sel = self.select & 3;
        field::clock_hz((self.profile >> (8 * sel)) & 0xff)
    }
}

/// The FPGA behind a BAR.
pub struct Fpga<B> {
    bar: B,
    version: Option<u32>,
    dead: Option<String>,
    poll_interval: Duration,
}

impl<B: Bar> Fpga<B> {
    /// Over `bar`; `poll_interval` between busy polls (vendor: 1 ms).
    pub fn new(bar: B, poll_interval: Duration) -> Self {
        Self {
            bar,
            version: None,
            dead: None,
            poll_interval,
        }
    }

    /// Why it is dead, if it is.
    #[must_use]
    pub fn dead(&self) -> Option<&str> {
        self.dead.as_deref()
    }

    /// Mark it dead (also used when the root port reports the link gone).
    pub fn mark_dead(&mut self, why: String) {
        if self.dead.is_none() {
            self.dead = Some(why);
        }
    }

    /// The BAR (tests, the simulator).
    pub fn bar_ref(&self) -> &B {
        &self.bar
    }

    /// The BAR, mutable (tests, the simulator).
    pub fn bar_mut(&mut self) -> &mut B {
        &mut self.bar
    }

    /// Give the BAR back.
    pub fn into_bar(self) -> B {
        self.bar
    }

    /// Version register as first read.
    #[must_use]
    pub fn version(&self) -> Option<u32> {
        self.version
    }

    fn alive(&self) -> Result<(), FpgaError> {
        self.dead.as_ref().map_or(Ok(()), |w| Err(FpgaError::Dead(w.clone())))
    }

    fn read(&mut self, r: Reg) -> Result<u32, FpgaError> {
        self.alive()?;
        if !r.readable() {
            return Err(FpgaError::Refused(format!(
                "BAR0+{:#x} is not a read the vendor driver makes",
                r.offset()
            )));
        }
        self.bar.read32(r.offset()).map_err(|e| FpgaError::Refused(e.0))
    }

    /// A register that is never all ones while the FPGA answers.
    fn read_live(&mut self, r: Reg, what: &str) -> Result<u32, FpgaError> {
        let v = self.read(r)?;
        if v == ALL_ONES {
            let why = format!("{what} (BAR0+{:#x}) reads 0xffffffff", r.offset());
            self.mark_dead(why.clone());
            return Err(FpgaError::Dead(why));
        }
        Ok(v)
    }

    fn write(&mut self, r: Reg, v: u32) -> Result<(), FpgaError> {
        self.alive()?;
        if !r.writable() {
            return Err(FpgaError::Refused(format!(
                "BAR0+{:#x} is not a write the vendor driver makes",
                r.offset()
            )));
        }
        self.bar.write32(r.offset(), v).map_err(|e| FpgaError::Refused(e.0))
    }

    /// Version check (a changed or all-ones version = dead), then the sideband registers.
    pub fn sideband(&mut self) -> Result<Sideband, FpgaError> {
        let v = self.read_live(Reg::Global(Global::Version), "version")?;
        match self.version {
            None => self.version = Some(v),
            Some(old) if old != v => {
                let why = format!("version register changed {old:#010x} -> {v:#010x}");
                self.mark_dead(why.clone());
                return Err(FpgaError::Dead(why));
            }
            Some(_) => {}
        }
        Ok(Sideband {
            present: self.read(Reg::Global(Global::Present))?,
            reset: self.read(Reg::Global(Global::Reset))?,
            lpmode: self.read(Reg::Global(Global::LpMode))?,
            interrupt: self.read(Reg::Global(Global::Interrupt))?,
        })
    }

    fn set_bit(&mut self, g: Global, cage: Cage, on: bool) -> Result<bool, FpgaError> {
        let old = self.read(Reg::Global(g))?;
        let new = if on { old | cage.bit() } else { old & !cage.bit() };
        if new != old {
            self.write(Reg::Global(g), new)?;
        }
        Ok(new != old)
    }

    /// Hold (`true`) or release a module's `ResetL`; true if it changed.
    pub fn set_reset(&mut self, cage: Cage, held: bool) -> Result<bool, FpgaError> {
        self.set_bit(Global::Reset, cage, held)
    }

    /// Assert (`true`) or deassert `LPMode`; true if it changed.
    pub fn set_lpmode(&mut self, cage: Cage, on: bool) -> Result<bool, FpgaError> {
        self.set_bit(Global::LpMode, cage, on)
    }

    /// A channel's configuration registers.
    pub fn channel(&mut self, cage: Cage) -> Result<ChannelState, FpgaError> {
        Ok(ChannelState {
            profile: self.read_live(Reg::Channel(cage, ChannelReg::Profile), "clock profile")?,
            select: self.read(Reg::Channel(cage, ChannelReg::ProfileSelect))?,
            master_reset: self.read(Reg::Channel(cage, ChannelReg::MasterReset))?,
            start: self.read_live(Reg::Channel(cage, ChannelReg::Start), "start")?,
        })
    }

    /// The vendor probe's channel writes (400 kHz profile), in its order.
    pub fn init_channel(&mut self, cage: Cage) -> Result<(), FpgaError> {
        self.write(Reg::Channel(cage, ChannelReg::MasterReset), field::PROBE_MRESET)?;
        self.write(
            Reg::Channel(cage, ChannelReg::ProfileSelect),
            field::PROBE_PROFILE_SELECT,
        )?;
        self.write(Reg::Channel(cage, ChannelReg::Profile), field::PROBE_PROFILE)?;
        match self.channel(cage)?.problem() {
            None => Ok(()),
            Some(p) => Err(FpgaError::ChannelNotReady(cage, format!("after the probe writes: {p}"))),
        }
    }

    /// Probe the channel if it is not in the probe's state (or report why it cannot be used).
    pub fn ensure_channel(&mut self, cage: Cage) -> Result<bool, FpgaError> {
        match self.channel(cage)?.problem() {
            None => Ok(false),
            Some(_) => self.init_channel(cage).map(|()| true),
        }
    }

    /// Pulse the channel's master reset (bit 0), as the vendor sysfs `port<N>_i2c_master_reset`.
    pub fn master_reset(&mut self, cage: Cage) -> Result<(), FpgaError> {
        let r = Reg::Channel(cage, ChannelReg::MasterReset);
        let v = self.read(r)?;
        self.write(r, v | field::MRESET_HOLD)?;
        std::thread::sleep(Duration::from_millis(10));
        self.write(r, v & !field::MRESET_HOLD)
    }

    /// One transfer as the vendor driver does it: (data words,) command, start, poll
    /// (sleep first), status read unconditionally (error wins over busy), data words.
    fn xfer(&mut self, cage: Cage, addr: u8, reg: u8, write: Option<&[u8]>, nread: u8) -> Result<Vec<u8>, FpgaError> {
        let n = write.map_or(nread, |d| u8::try_from(d.len()).unwrap_or(u8::MAX));
        let max = if write.is_some() {
            field::WRITE_CHUNK
        } else {
            field::READ_CHUNK
        };
        if n == 0 || n > max || usize::from(reg) + usize::from(n) > 0x100 || !(0x03..=0x77).contains(&addr) {
            return Err(FpgaError::Refused(format!(
                "transfer addr {addr:#04x} reg {reg:#04x} len {n}"
            )));
        }
        if let Some(d) = write {
            for (w, chunk) in d.chunks(4).enumerate() {
                let mut b = [0u8; 4];
                b.get_mut(..chunk.len())
                    .ok_or_else(|| FpgaError::Refused("chunk".into()))?
                    .copy_from_slice(chunk);
                let word = u8::try_from(w).map_err(|_| FpgaError::Refused("buffer word".into()))?;
                self.write(Reg::WriteBuffer(cage, word), u32::from_le_bytes(b))?;
            }
        }
        self.write(
            Reg::Channel(cage, ChannelReg::Command),
            crate::regs::command(write.is_none(), addr, n),
        )?;
        self.write(
            Reg::Channel(cage, ChannelReg::Start),
            field::START_BUSY | u32::from(reg),
        )?;
        let mut busy = true;
        for _ in 0..field::POLLS {
            std::thread::sleep(self.poll_interval);
            if self.read_live(Reg::Channel(cage, ChannelReg::Start), "start")? & field::START_BUSY == 0 {
                busy = false;
                break;
            }
        }
        let st = self.read_live(Reg::Channel(cage, ChannelReg::Status), "status")?;
        if st & field::STATUS_ERROR != 0 {
            return Err(FpgaError::Nack { cage, addr, reg });
        }
        if busy {
            return Err(FpgaError::Busy(cage));
        }
        if write.is_some() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(usize::from(n));
        for w in 0..n.div_ceil(4) {
            out.extend_from_slice(&self.read(Reg::ReadBuffer(cage, w))?.to_le_bytes());
        }
        out.truncate(usize::from(n));
        Ok(out)
    }

    /// `len` bytes at `reg` of two-wire address `addr` (in 32-byte transfers).
    pub fn i2c_read(&mut self, cage: Cage, addr: u8, reg: u8, len: u16) -> Result<Vec<u8>, FpgaError> {
        if usize::from(reg) + usize::from(len) > 0x100 {
            return Err(FpgaError::Refused(format!("read of {len} bytes at {reg:#04x} wraps")));
        }
        let mut out = Vec::with_capacity(usize::from(len));
        let mut pos = u16::from(reg);
        let end = pos + len;
        while pos < end {
            let k = (end - pos).min(u16::from(field::READ_CHUNK));
            let (r, k8) = (u8::try_from(pos).unwrap_or(0xff), u8::try_from(k).unwrap_or(0));
            out.extend(self.xfer(cage, addr, r, None, k8)?);
            pos += k;
        }
        Ok(out)
    }

    /// Write `data` at `reg` (in 32-byte transfers).
    pub fn i2c_write(&mut self, cage: Cage, addr: u8, reg: u8, data: &[u8]) -> Result<(), FpgaError> {
        if usize::from(reg) + data.len() > 0x100 {
            return Err(FpgaError::Refused(format!(
                "write of {} bytes at {reg:#04x} wraps",
                data.len()
            )));
        }
        for (i, chunk) in data.chunks(usize::from(field::WRITE_CHUNK)).enumerate() {
            let off = usize::from(reg) + i * usize::from(field::WRITE_CHUNK);
            self.xfer(cage, addr, u8::try_from(off).unwrap_or(0xff), Some(chunk), 0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimFpga;
    use cs8210_xcvr::sim::SimModule;

    fn c(n: u8) -> Cage {
        Cage::new(n).unwrap()
    }

    fn fpga(sim: SimFpga) -> Fpga<SimFpga> {
        Fpga::new(sim, Duration::ZERO)
    }

    #[test]
    fn transfer_needs_probe_and_reset_release() {
        let mut sim = SimFpga::new();
        sim.insert(1, SimModule::lab_dr4());
        let mut f = fpga(sim);
        let sb = f.sideband().unwrap();
        assert_eq!(sb.present, 1);
        assert_eq!(sb.reset, 0xffff_ffff);
        // held in reset, channel at power-up values: NACK, and not ready
        assert!(f.channel(c(1)).unwrap().problem().is_some());
        assert!(matches!(f.i2c_read(c(1), 0x50, 0, 1), Err(FpgaError::Nack { .. })));
        assert!(f.ensure_channel(c(1)).unwrap());
        assert!(!f.ensure_channel(c(1)).unwrap());
        assert_eq!(f.channel(c(1)).unwrap().clock_hz(), Some(400_000));
        assert!(matches!(f.i2c_read(c(1), 0x50, 0, 1), Err(FpgaError::Nack { .. })));
        assert!(f.set_reset(c(1), false).unwrap());
        assert!(!f.set_reset(c(1), false).unwrap());
        let id = f.i2c_read(c(1), 0x50, 0, 256).unwrap();
        assert_eq!(id.len(), 256);
        assert_eq!(&id[148..158], b"1064281405");
        f.i2c_write(c(1), 0x50, 127, &[0x11]).unwrap();
        assert_eq!(f.i2c_read(c(1), 0x50, 128, 4).unwrap(), vec![0x44; 4]);
        assert!(matches!(f.i2c_read(c(2), 0x50, 0, 1), Err(FpgaError::Nack { .. }))); // empty cage
    }

    #[test]
    fn all_ones_is_dead_and_final() {
        let mut sim = SimFpga::new();
        sim.insert(1, SimModule::lab_dr4());
        let mut f = fpga(sim);
        f.sideband().unwrap();
        f.bar.kill();
        assert!(matches!(f.sideband(), Err(FpgaError::Dead(_))));
        let reads = f.bar.reads();
        assert!(matches!(f.i2c_read(c(1), 0x50, 0, 1), Err(FpgaError::Dead(_))));
        assert!(matches!(f.set_reset(c(1), false), Err(FpgaError::Dead(_))));
        assert_eq!(f.bar.reads(), reads, "no access after death");
    }

    #[test]
    fn version_change_is_dead() {
        let mut f = fpga(SimFpga::new());
        f.sideband().unwrap();
        f.bar.set_version(0x0102_0304);
        assert!(matches!(f.sideband(), Err(FpgaError::Dead(w)) if w.contains("changed")));
    }

    #[test]
    fn refuses_what_a_transfer_cannot_carry() {
        let mut f = fpga(SimFpga::new());
        assert!(matches!(f.i2c_read(c(1), 0x50, 250, 10), Err(FpgaError::Refused(_))));
        assert!(matches!(f.i2c_read(c(1), 0x02, 0, 1), Err(FpgaError::Refused(_))));
        assert_eq!(f.bar.writes(), []);
    }
}

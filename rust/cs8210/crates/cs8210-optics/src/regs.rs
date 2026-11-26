//! The I2C FPGA's register map (PCI `1d0f:8571`, BAR0), as types: which offset each
//! register has, which may be read and which written. Recovered from the vendor driver
//! `i2c_accel_fpga.ko` v0.11 (ai_re/devices/cs8210-32x/reviews/2026-11-10-i2c-fpga-protocol.md)
//! and the FPGA's own channel logic (devices/cs8210-32x/fpga/i2cfpga/CHANNEL.md).
//!
//! Every access the crate can make is a [`Reg`] whose [`Reg::readable`] /
//! [`Reg::writable`] allows it: exactly the accesses the vendor driver makes, at its
//! width (32 bit). An unacknowledged MMIO read on this `SoC` is an asynchronous `SError`
//! (kernel panic), so nothing else is reachable from here.

/// PCI vendor id.
pub const VENDOR: u16 = 0x1d0f;
/// PCI device id.
pub const DEVICE: u16 = 0x8571;
/// Bytes of BAR0 mapped (everything the vendor driver touches is below 0xC000).
pub const MAP_LEN: usize = 0x10000;
/// Front-panel cages, one FPGA I2C channel each.
pub const CAGES: u8 = 32;

/// A front-panel cage, 1..=32: FPGA channel `n - 1`, sideband bit `n - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cage(u8);

impl Cage {
    /// Cage `n` if 1..=32.
    #[must_use]
    pub fn new(n: u8) -> Option<Self> {
        (1..=CAGES).contains(&n).then_some(Self(n))
    }

    /// All 32 cages.
    pub fn all() -> impl Iterator<Item = Self> {
        (1..=CAGES).map(Self)
    }

    /// Cage number (1-based).
    #[must_use]
    pub fn number(self) -> u8 {
        self.0
    }

    /// Its bit in the sideband registers.
    #[must_use]
    pub fn bit(self) -> u32 {
        1 << (self.0 - 1)
    }

    /// Its FPGA channel (0-based).
    #[must_use]
    pub fn channel(self) -> usize {
        usize::from(self.0 - 1)
    }
}

impl std::fmt::Display for Cage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cage {}", self.0)
    }
}

/// Global registers (sideband: one bit per cage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Global {
    /// bits 23:8 FPGA version, 7:0 board version (never all ones while the FPGA answers)
    Version,
    /// scratch
    Scratch,
    /// 1 = module present
    Present,
    /// presence changed (latched; the vendor IRQ handler reads it)
    PresenceChange,
    /// `IntL`: 1 = the module asserts its interrupt (cage 1 on 2026-11-25: set while the
    /// module's byte 3 bit 0 said "interrupt asserted")
    Interrupt,
    /// interrupt mask (power-up all masked; `INTx` is not delivered on mainline anyway)
    InterruptMask,
    /// `LPMode` pin: 1 = asserted (low power)
    LpMode,
    /// `ResetL` pin: 1 = module held in reset (power-up: every cage)
    Reset,
}

impl Global {
    /// BAR0 offset.
    #[must_use]
    pub const fn offset(self) -> usize {
        match self {
            Self::Version => 0x000,
            Self::Scratch => 0x004,
            Self::Present => 0x140,
            Self::PresenceChange => 0x150,
            Self::Interrupt => 0x160,
            Self::InterruptMask => 0x168,
            Self::LpMode => 0x170,
            Self::Reset => 0x178,
        }
    }
}

/// Registers of one I2C channel (block `0x1000 + channel * 0x100`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelReg {
    /// byte k = SCL clock code of profile k+1 (0x18 400 kHz, 0x09 1 MHz, 0x66 100 kHz)
    Profile,
    /// bits 1:0 = selected profile - 1
    ProfileSelect,
    /// bit 0 = master held in reset; bit 31 = self-clearing soft reset (the probe writes it)
    MasterReset,
    /// bit 28 read, bits 15:8 7-bit address, 7:0 length (write only, as the vendor driver)
    Command,
    /// bit 31 start / busy, bits 7:0 register offset
    Start,
    /// bit 1 = error (NACK)
    Status,
}

impl ChannelReg {
    const fn offset(self) -> usize {
        match self {
            Self::Profile => 0x00,
            Self::ProfileSelect => 0x08,
            Self::MasterReset => 0x0c,
            Self::Command => 0x50,
            Self::Start => 0x54,
            Self::Status => 0x60,
        }
    }
}

/// Channel register blocks.
const CH_BASE: usize = 0x1000;
const CH_STRIDE: usize = 0x100;
/// Per-channel data buffers.
const WRITE_BUFFER: usize = 0x4000;
const READ_BUFFER: usize = 0x8000;
const BUFFER_STRIDE: usize = 0x200;
/// Bytes of a channel buffer the vendor driver uses (128).
pub const BUFFER_BYTES: usize = 0x80;
/// Words of it.
pub const BUFFER_WORDS: u8 = 32;

/// One BAR0 register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    /// a global register
    Global(Global),
    /// a channel register
    Channel(Cage, ChannelReg),
    /// word `n` (0..32) of a channel's write buffer
    WriteBuffer(Cage, u8),
    /// word `n` of a channel's read buffer
    ReadBuffer(Cage, u8),
}

impl Reg {
    /// BAR0 offset.
    #[must_use]
    pub fn offset(self) -> usize {
        match self {
            Self::Global(g) => g.offset(),
            Self::Channel(c, r) => CH_BASE + c.channel() * CH_STRIDE + r.offset(),
            Self::WriteBuffer(c, w) => WRITE_BUFFER + c.channel() * BUFFER_STRIDE + 4 * usize::from(w),
            Self::ReadBuffer(c, w) => READ_BUFFER + c.channel() * BUFFER_STRIDE + 4 * usize::from(w),
        }
    }

    /// A read the vendor driver makes.
    #[must_use]
    pub fn readable(self) -> bool {
        match self {
            Self::Global(_) => true,
            Self::Channel(_, r) => r != ChannelReg::Command,
            Self::WriteBuffer(..) => false,
            Self::ReadBuffer(_, w) => w < BUFFER_WORDS,
        }
    }

    /// A write the vendor driver makes (its sysfs bit stores are read-modify-write of
    /// `LPMode`, `ResetL` and the interrupt mask; the channel probe and transfers).
    #[must_use]
    pub fn writable(self) -> bool {
        match self {
            Self::Global(g) => matches!(g, Global::LpMode | Global::Reset | Global::InterruptMask),
            Self::Channel(_, r) => r != ChannelReg::Status,
            Self::WriteBuffer(_, w) => w < BUFFER_WORDS,
            Self::ReadBuffer(..) => false,
        }
    }
}

/// Register fields and values.
pub mod field {
    /// Command: read transfer.
    pub const CMD_READ: u32 = 1 << 28;
    /// Start: start (write) / busy (read).
    pub const START_BUSY: u32 = 1 << 31;
    /// Status: NACK / error.
    pub const STATUS_ERROR: u32 = 1 << 1;
    /// Master reset: master held in reset.
    pub const MRESET_HOLD: u32 = 1 << 0;
    /// What the vendor probe writes: master reset register (bit 31: soft reset pulse).
    pub const PROBE_MRESET: u32 = 0x8000_0000;
    /// ... profile select (profile 1).
    pub const PROBE_PROFILE_SELECT: u32 = 0;
    /// ... clock profiles: 1 = 400 kHz (0x18), 2 = 1 MHz (0x09), 3 = 100 kHz (0x66).
    pub const PROBE_PROFILE: u32 = 0x0066_0918;
    /// A channel's clock profiles at power-up (before the probe).
    pub const POWER_UP_PROFILE: u32 = 0x0008_1766;
    /// Polls of the busy bit, 1 ms apart, before a transfer counts as stuck (vendor: 50).
    pub const POLLS: u32 = 50;
    /// Bytes per read transfer (vendor `sfp_module` / i2cdump block size).
    pub const READ_CHUNK: u8 = 32;
    /// Bytes per write transfer.
    pub const WRITE_CHUNK: u8 = 32;
    /// SCL rate of a clock code.
    #[must_use]
    pub fn clock_hz(code: u32) -> Option<u32> {
        match code {
            0x18 => Some(400_000),
            0x09 => Some(1_000_000),
            0x66 => Some(100_000),
            _ => None,
        }
    }
}

/// Command word of a transfer.
#[must_use]
pub fn command(read: bool, addr: u8, len: u8) -> u32 {
    (if read { field::CMD_READ } else { 0 }) | (u32::from(addr & 0x7f) << 8) | u32::from(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_the_vendor_drivers() {
        let c1 = Cage::new(1).unwrap();
        let c32 = Cage::new(32).unwrap();
        assert_eq!(Reg::Global(Global::Reset).offset(), 0x178);
        assert_eq!(Reg::Channel(c1, ChannelReg::Profile).offset(), 0x1000);
        assert_eq!(
            Reg::Channel(c32, ChannelReg::Status).offset(),
            0x1000 + 31 * 0x100 + 0x60
        );
        assert_eq!(Reg::WriteBuffer(c1, 1).offset(), 0x4004);
        assert_eq!(Reg::ReadBuffer(c32, 31).offset(), 0x8000 + 31 * 0x200 + 124);
        assert_eq!(command(true, 0x50, 32), 0x1000_5020);
        assert!(Cage::new(0).is_none() && Cage::new(33).is_none());
    }

    #[test]
    fn allowlist() {
        let c = Cage::new(3).unwrap();
        assert!(Reg::Global(Global::Present).readable());
        assert!(!Reg::Global(Global::Present).writable());
        assert!(!Reg::Global(Global::Version).writable());
        assert!(Reg::Global(Global::Reset).writable() && Reg::Global(Global::LpMode).writable());
        assert!(!Reg::Channel(c, ChannelReg::Command).readable());
        assert!(!Reg::Channel(c, ChannelReg::Status).writable());
        assert!(!Reg::WriteBuffer(c, 0).readable() && !Reg::ReadBuffer(c, 0).writable());
        assert!(!Reg::ReadBuffer(c, BUFFER_WORDS).readable());
        assert!(!Reg::WriteBuffer(c, BUFFER_WORDS).writable());
    }
}

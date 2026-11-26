//! The one owner of the CS8210-32X's I2C FPGA (PCI `1d0f:8571`): cage presence,
//! `ResetL`, `LPMode`, `IntL`, per-cage I2C channels, module bring-up (CMIS / SFF-8636 via
//! `cs8210-xcvr`), EEPROM and monitors for `SONiC` (a Unix socket the board's
//! `sonic_platform` calls) and Prometheus (a node-exporter textfile).
//!
//! Layers: [`regs`] (the register map as types, the allow-list) -> [`bar`] (real
//! mapping or [`sim`]) -> [`fpga`] (operations, stall detection) -> [`cage`] (per-cage
//! state, module register access) -> [`daemon`] (policy, socket, metrics). [`pci`]
//! and [`owner`] keep the stall safeguards: the root port check, the dead marker, the
//! one flock.
#![forbid(unsafe_code)]

pub mod bar;
pub mod cage;
pub mod daemon;
pub mod fpga;
pub mod metrics;
pub mod owner;
pub mod pci;
pub mod regs;
pub mod sim;

use bar::{Bar, BarError};

/// The BAR behind the daemon: the device, the simulator, or nothing (FPGA unusable).
pub enum Backend {
    /// `/sys/bus/pci/devices/<bdf>/resource0`, mapped
    Device(cs8210_sys::mmio::Mmio),
    /// `--simulate`
    Sim(Box<sim::SimFpga>),
    /// no access at all (dead marker present, device missing ...)
    None,
}

impl Bar for Backend {
    fn read32(&mut self, offset: usize) -> Result<u32, BarError> {
        match self {
            Self::Device(m) => m.read32(offset),
            Self::Sim(s) => s.read32(offset),
            Self::None => Err(BarError("no I2C FPGA mapping".into())),
        }
    }

    fn write32(&mut self, offset: usize, value: u32) -> Result<(), BarError> {
        match self {
            Self::Device(m) => m.write32(offset, value),
            Self::Sim(s) => s.write32(offset, value),
            Self::None => Err(BarError("no I2C FPGA mapping".into())),
        }
    }
}

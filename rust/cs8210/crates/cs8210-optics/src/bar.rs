//! BAR0 access as a trait: the real mapping (`cs8210_sys::mmio`) or the simulator.

use std::fmt;

/// A failed BAR access (outside the mapping; the simulator's refusals).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarError(pub String);

impl fmt::Display for BarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BarError {}

/// 32-bit loads and stores at BAR0 offsets.
pub trait Bar {
    /// One 32-bit load.
    fn read32(&mut self, offset: usize) -> Result<u32, BarError>;
    /// One 32-bit store.
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), BarError>;
}

impl Bar for cs8210_sys::mmio::Mmio {
    fn read32(&mut self, offset: usize) -> Result<u32, BarError> {
        cs8210_sys::mmio::Mmio::read32(self, offset).map_err(|e| BarError(e.to_string()))
    }

    fn write32(&mut self, offset: usize, value: u32) -> Result<(), BarError> {
        cs8210_sys::mmio::Mmio::write32(self, offset, value).map_err(|e| BarError(e.to_string()))
    }
}

// SPDX-License-Identifier: GPL-2.0

//! How the HAL reaches the hardware: traits the kernel side implements over
//! its MMIO, DMA and PCI abstractions (and the tests over mock banks). The HAL
//! itself never touches memory it does not own.

/// One register window (a BAR). Offsets are bytes from its start, accesses are
/// 32-bit little endian, as the Alpine SoC's registers are.
///
/// Contract for implementors: `w32` has Linux `writel()` ordering (normal
/// memory writes before it are visible to the device first); `r32` has
/// `readl()` ordering (normal memory reads after it are not satisfied before
/// it). The HAL relies on this for the Tx completion head read (`crhp`).
pub trait RegIo {
    fn r32(&self, off: usize) -> u32;
    fn w32(&self, off: usize, val: u32);
}

/// Busy waits (the C HAL's al_udelay).
pub trait Delay {
    fn udelay(&self, us: u32);
}

/// The function's PCI configuration space (FLR, al_eth_flr_rmn).
pub trait PciCfg {
    fn read32(&self, off: u16) -> u32;
    fn write32(&self, off: u16, val: u32);
    /// Called at the end of the FLR, after its own config restore. The FLR
    /// resets what the HAL does not save: MSI-X Enable and the vector table.
    /// The C enables MSI-X only after it (al_eth_up: hw_init, then
    /// setup_int_mode); a driver whose vectors already exist restores them
    /// here (pci_restore_msi_state). No register access of its own in the HAL.
    fn after_flr(&self) {}
}

/// A submission descriptor ring: `len()` entries of four 32-bit words in
/// device-visible (coherent) memory, stored little endian.
///
/// Barrier: `publish()` runs after the HAL wrote descriptors and before it
/// rings the doorbell; it must make those writes visible to the device
/// (`dma_wmb()`; the C HAL issues `mb()` there).
pub trait DescMem {
    fn write(&mut self, idx: usize, words: [u32; 4]);
    fn read(&self, idx: usize) -> [u32; 4];
    fn len(&self) -> usize;
    /// Bus address of entry 0 (what the queue's base registers get).
    fn dma_addr(&self) -> u64;
    fn publish(&self);
}

/// A completion descriptor ring the device writes.
///
/// Barrier: the HAL reads a descriptor's control word with `ctrl()` to see
/// whether the device wrote it, then calls `dma_rmb()` once before reading
/// the rest of the packet's descriptors with `read()`. (The C HAL has no
/// barrier there; on arm64 the later loads could otherwise see stale data.)
pub trait CDescMem {
    fn ctrl(&self, idx: usize) -> u32;
    fn read(&self, idx: usize) -> [u32; 4];
    fn len(&self) -> usize;
    fn dma_addr(&self) -> u64;
    fn dma_rmb(&self);
}

/// The three register windows of one Ethernet unit (BARs 0, 4 and 2).
pub struct Bars<R: RegIo> {
    pub udma: R,
    pub ec: R,
    pub mac: R,
}

/// al_reg_write32_masked: read, replace the bits of `mask` with `data`, write.
pub(crate) fn masked<R: RegIo>(io: &R, off: usize, mask: u32, data: u32) {
    let t = io.r32(off);
    io.w32(off, (mask & data) | (!mask & t));
}

/// AL_REG_FIELD_SET.
pub(crate) const fn field_set(reg: u32, mask: u32, shift: u32, val: u32) -> u32 {
    (reg & !mask) | ((val << shift) & mask)
}

/// AL_REG_FIELD_GET.
pub(crate) const fn field_get(reg: u32, mask: u32, shift: u32) -> u32 {
    (reg & mask) >> shift
}

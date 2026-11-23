// SPDX-License-Identifier: GPL-2.0

//! Interrupt controller groups (al_hal_iofic.c, al_hal_udma_iofic.[ch]). An
//! Iofic is a controller at an offset inside a BAR: the UDMA's primary and
//! secondary controllers, the EC's and the MAC's.

use crate::hal::io::{field_set, RegIo};
use crate::hal::regs::{k, off};

/// Interrupt group (AL_INT_GROUP_A..D).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
}

/// One interrupt controller: its base offset in a register window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Iofic {
    base: usize,
}

/// The UDMA's primary controller (al_udma_iofic_reg_base_get, PRIMARY).
pub const UDMA_PRIMARY: Iofic = Iofic { base: off::U_MAIN_IOFIC };
/// The UDMA's secondary controller (groups A: M2S errors, B: S2M errors).
pub const UDMA_SECONDARY: Iofic = Iofic { base: off::U_SEC_IOFIC };
/// The Ethernet controller's (in the EC window).
pub const EC: Iofic = Iofic { base: off::EC_IOFIC };

/// al_iofic_mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Legacy,
    MsixPerQueue,
    MsixPerGroup,
}

impl Iofic {
    const fn grp(self, g: Group, reg: usize) -> usize {
        self.base + g as usize * off::IO_GRP_STRIDE + reg
    }

    /// al_iofic_config
    pub fn config<R: RegIo>(self, io: &R, g: Group, flags: u32) {
        io.w32(self.grp(g, off::IO_INT_CONTROL), flags);
    }

    /// al_iofic_moder_res_config
    pub fn moder_res_config<R: RegIo>(self, io: &R, g: Group, resolution: u8) {
        let o = self.grp(g, off::IO_INT_CONTROL);
        let r = io.r32(o);
        io.w32(o, field_set(r, k::INT_CONTROL_GRP_MOD_RES_MASK, k::INT_CONTROL_GRP_MOD_RES_SHIFT, resolution as u32));
    }

    /// al_iofic_msix_moder_interval_config. The C takes uint8_t interval: the
    /// value is cut to 8 bits there too.
    pub fn msix_moder_interval_config<R: RegIo>(self, io: &R, g: Group, vector: u8, interval: u8) {
        let o = self.base
            + off::IO_GRP_INT_MOD0
            + g as usize * off::IO_GRP_INT_MOD_GRP_STRIDE
            + vector as usize * off::IO_GRP_INT_MOD_VEC_STRIDE;
        let r = io.r32(o);
        io.w32(o, field_set(r, k::INT_MOD_INTV_MASK, k::INT_MOD_INTV_SHIFT, interval as u32));
    }

    /// al_iofic_unmask: write 0 to the mask-clear bits (1 has no effect).
    pub fn unmask<R: RegIo>(self, io: &R, g: Group, mask: u32) {
        io.w32(self.grp(g, off::IO_INT_MASK_CLEAR), !mask);
    }

    /// Offset of the mask-clear register (al_iofic_unmask_offset_get): the
    /// NAPI poll writes `!(1 << queue)` there to unmask its queue.
    pub const fn unmask_offset(self, g: Group) -> usize {
        self.grp(g, off::IO_INT_MASK_CLEAR)
    }

    /// al_iofic_mask (read-modify-write: callers serialise).
    pub fn mask<R: RegIo>(self, io: &R, g: Group, mask: u32) {
        let o = self.grp(g, off::IO_INT_MASK);
        let r = io.r32(o);
        io.w32(o, r | mask);
    }

    /// al_iofic_read_cause
    pub fn read_cause<R: RegIo>(self, io: &R, g: Group) -> u32 {
        io.r32(self.grp(g, off::IO_INT_CAUSE))
    }

    /// al_iofic_clear_cause
    pub fn clear_cause<R: RegIo>(self, io: &R, g: Group, mask: u32) {
        io.w32(self.grp(g, off::IO_INT_CAUSE), !mask);
    }

    /// al_iofic_abort_mask
    pub fn abort_mask<R: RegIo>(self, io: &R, g: Group, mask: u32) {
        io.w32(self.grp(g, off::IO_INT_ABORT_MASK), mask);
    }
}

/// al_udma_main_iofic_config
fn udma_main_config<R: RegIo>(io: &R, mode: Mode) {
    use Group::*;
    let p = UDMA_PRIMARY;
    match mode {
        Mode::Legacy => {
            p.config(io, A, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_MASK_MSI_X | k::INT_CONTROL_GRP_CLEAR_ON_READ);
            p.config(io, B, k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
            p.config(io, C, k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
            p.config(io, D, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_MASK_MSI_X | k::INT_CONTROL_GRP_CLEAR_ON_READ);
        }
        Mode::MsixPerQueue => {
            p.config(io, A, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_AUTO_MASK | k::INT_CONTROL_GRP_AUTO_CLEAR);
            p.config(io, B, k::INT_CONTROL_GRP_AUTO_CLEAR | k::INT_CONTROL_GRP_AUTO_MASK | k::INT_CONTROL_GRP_CLEAR_ON_READ);
            p.config(io, C, k::INT_CONTROL_GRP_AUTO_CLEAR | k::INT_CONTROL_GRP_AUTO_MASK | k::INT_CONTROL_GRP_CLEAR_ON_READ);
            p.config(io, D, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
        }
        Mode::MsixPerGroup => {
            p.config(io, A, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_AUTO_CLEAR | k::INT_CONTROL_GRP_AUTO_MASK);
            p.config(io, B, k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
            p.config(io, C, k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
            p.config(io, D, k::INT_CONTROL_GRP_SET_ON_POSEDGE | k::INT_CONTROL_GRP_CLEAR_ON_READ | k::INT_CONTROL_GRP_MASK_MSI_X);
        }
    }
}

/// al_udma_iofic_config: the mode of the primary controller, the error
/// interrupts of the secondary one.
pub fn udma_config<R: RegIo>(io: &R, mode: Mode, m2s_errors_disable: u32, m2s_aborts_disable: u32, s2m_errors_disable: u32, s2m_aborts_disable: u32) {
    udma_main_config(io, mode);
    let s = UDMA_SECONDARY;
    s.unmask(io, Group::A, !m2s_errors_disable);
    s.abort_mask(io, Group::A, m2s_aborts_disable);
    s.unmask(io, Group::B, !s2m_errors_disable);
    s.abort_mask(io, Group::B, s2m_aborts_disable);
}

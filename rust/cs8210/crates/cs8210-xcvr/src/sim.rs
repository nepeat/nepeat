//! A module's two-wire memory as a simulator: optoe-layout image, page select, and the
//! CMIS behaviour the bring-up relies on (`LowPwrRequestSW`, `DataPathDeinit`,
//! `Apply_DataPathInit` with config status). Used by the tests and by the daemon's
//! `--simulate` mode (the `SONiC` platform tests run against it).

use crate::cmis::reg;
use crate::lab;
use crate::memory::{ADDR_A0, ADDR_A2, Kind, PAGE_SELECT};

/// One simulated module.
#[derive(Debug, Clone)]
pub struct SimModule {
    image: Vec<u8>,
    nack: bool,
    reject: Option<u8>,
    writes: u64,
}

impl SimModule {
    /// A module with this optoe image (bytes 0-255 lower + upper 00h, then upper pages
    /// 1, 2, ... at `128 + 128 * n`; an SFP's A2h at 256-511). Byte 127 holds the page.
    #[must_use]
    pub fn from_image(image: Vec<u8>) -> Self {
        Self {
            image,
            nack: false,
            reject: None,
            writes: 0,
        }
    }

    /// The cage 1 module of the lab (Molex 400G-DR4, CMIS 4.0, on its 4x100G default).
    #[must_use]
    pub fn lab_dr4() -> Self {
        let mut img = lab::dr4_page0();
        img.resize(128 + 128 * 0x12, 0);
        for (page, bytes) in [
            (0x01u8, lab::dr4_page01()),
            (0x10, lab::dr4_page10()),
            (0x11, lab::dr4_page11()),
        ] {
            let at = 128 * usize::from(page) + 128;
            if let Some(dst) = img.get_mut(at..at + 128) {
                dst.copy_from_slice(&bytes);
            }
        }
        Self::from_image(img)
    }

    /// The optoe image as it is now.
    #[must_use]
    pub fn image(&self) -> &[u8] {
        &self.image
    }

    fn kind(&self) -> Kind {
        Kind::from_bytes(self.lower_byte(0), self.lower_byte(2))
    }

    /// Lower-page byte.
    #[must_use]
    pub fn lower_byte(&self, reg: u8) -> u8 {
        self.image.get(usize::from(reg)).copied().unwrap_or(0)
    }

    /// Byte `reg` (128..=255) of upper page `page`.
    #[must_use]
    pub fn page_byte(&self, page: u8, reg: u8) -> u8 {
        self.image
            .get(128 * usize::from(page) + usize::from(reg))
            .copied()
            .unwrap_or(0)
    }

    fn set_page_byte(&mut self, page: u8, reg: u8, v: u8) {
        if let Some(b) = self.image.get_mut(128 * usize::from(page) + usize::from(reg)) {
            *b = v;
        }
    }

    fn lane_nibble(&mut self, page: u8, base: u8, lane: u8, v: u8) {
        let r = base + lane / 2;
        let old = self.page_byte(page, r);
        let new = if lane.is_multiple_of(2) {
            (old & 0xf0) | v
        } else {
            (old & 0x0f) | (v << 4)
        };
        self.set_page_byte(page, r, new);
    }

    /// Active `ApSel` codes (page 11h).
    #[must_use]
    pub fn active_apsel(&self) -> [u8; 8] {
        std::array::from_fn(|i| self.page_byte(reg::PAGE_DP_STATUS, reg::ACTIVE_APSEL + u8::try_from(i).unwrap_or(0)))
    }

    /// Every host lane `DataPathActivated`.
    #[must_use]
    pub fn dp_activated(&self) -> bool {
        (0..4).all(|i| self.page_byte(reg::PAGE_DP_STATUS, reg::DP_STATE + i) == 0x44)
    }

    /// Writes made so far (page selects not counted).
    #[must_use]
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// NACK every transfer (a module that does not answer).
    pub fn set_nack(&mut self, on: bool) {
        self.nack = on;
    }

    /// Reject the next `Apply_DataPathInit` with this config status code.
    pub fn reject_apply(&mut self, code: u8) {
        self.reject = Some(code);
    }

    /// `LowPwrRequestSW` set: the module sits in `ModuleLowPwr`.
    pub fn force_low_power(&mut self) {
        if let Some(b) = self.image.get_mut(usize::from(reg::MODULE_CONTROL)) {
            *b |= reg::LOW_PWR_REQUEST_SW;
        }
        if let Some(b) = self.image.get_mut(usize::from(reg::MODULE_STATE)) {
            *b = (*b & !0x0e) | (1 << 1);
        }
    }

    /// What a power glitch leaves: `ModuleReady` again, every data path deactivated.
    pub fn glitch(&mut self) {
        for i in 0..4 {
            self.set_page_byte(reg::PAGE_DP_STATUS, reg::DP_STATE + i, 0x11);
        }
    }

    fn offset(&self, addr: u8, reg: u8) -> Result<usize, String> {
        let kind = self.kind();
        match (addr, kind) {
            (ADDR_A2, Kind::Sfp) => Ok(256 + usize::from(reg)),
            (ADDR_A0, Kind::Sfp) => Ok(usize::from(reg)),
            (ADDR_A0, k) if reg < 128 || !k.paged() => Ok(usize::from(reg)),
            (ADDR_A0, _) => Ok(128 * usize::from(self.lower_byte(PAGE_SELECT)) + usize::from(reg)),
            _ => Err(format!("address {addr:#04x}: NACK")),
        }
    }

    /// A read transfer.
    pub fn read(&mut self, addr: u8, reg: u8, len: u8) -> Result<Vec<u8>, String> {
        if self.nack {
            return Err("NACK".into());
        }
        let mut out = Vec::with_capacity(usize::from(len));
        for i in 0..len {
            let r = reg.checked_add(i).ok_or("register offset wraps")?;
            let at = self.offset(addr, r)?;
            out.push(
                *self
                    .image
                    .get(at)
                    .ok_or_else(|| format!("offset {at} beyond the image: NACK"))?,
            );
        }
        Ok(out)
    }

    /// A write transfer, with the module's side effects.
    pub fn write(&mut self, addr: u8, reg: u8, data: &[u8]) -> Result<(), String> {
        if self.nack {
            return Err("NACK".into());
        }
        if !(reg == PAGE_SELECT && data.len() == 1) {
            self.writes += 1; // page selects are not counted
        }
        let page = self.lower_byte(PAGE_SELECT);
        for (i, &v) in data.iter().enumerate() {
            let r = reg
                .checked_add(u8::try_from(i).map_err(|_| "too long")?)
                .ok_or("register offset wraps")?;
            if r == PAGE_SELECT && !self.kind().paged() {
                continue;
            }
            let at = self.offset(addr, r)?;
            *self.image.get_mut(at).ok_or("beyond the image: NACK")? = v;
            if matches!(self.kind(), Kind::Cmis { .. }) {
                self.cmis_effect(page, r, v);
            }
        }
        Ok(())
    }

    fn cmis_effect(&mut self, page: u8, r: u8, v: u8) {
        let (st, ctl) = (reg::PAGE_DP_STATUS, reg::PAGE_DP_CONTROL);
        match (page, r) {
            (_, reg::MODULE_CONTROL) => {
                let state = if v & reg::LOW_PWR_REQUEST_SW != 0 { 1 } else { 3 };
                if let Some(b) = self.image.get_mut(usize::from(reg::MODULE_STATE)) {
                    *b = (*b & !0x0e) | (state << 1);
                }
            }
            (p, reg::DATAPATH_DEINIT) if p == ctl => {
                for lane in 0..8 {
                    let s = if v >> lane & 1 != 0 {
                        1
                    } else if self.page_byte(st, reg::ACTIVE_APSEL + lane) >> 4 != 0 {
                        4
                    } else {
                        1
                    };
                    self.lane_nibble(st, reg::DP_STATE, lane, s);
                }
            }
            (p, reg::APPLY_DPINIT) if p == ctl => {
                let apps = crate::cmis::applications(&self.image).len();
                for lane in 0..8u8 {
                    if v >> lane & 1 == 0 {
                        continue;
                    }
                    let deactivated = self.page_byte(st, reg::DP_STATE + lane / 2) >> (4 * (lane % 2)) & 0x0f == 1;
                    let code = self.page_byte(ctl, reg::STAGED_APSEL + lane);
                    let status = if !deactivated {
                        6
                    } else if let Some(c) = self.reject {
                        c
                    } else if (1..=apps).contains(&usize::from(code >> 4)) {
                        self.set_page_byte(st, reg::ACTIVE_APSEL + lane, code);
                        1
                    } else {
                        3
                    };
                    self.lane_nibble(st, reg::CONFIG_STATUS, lane, status);
                }
                self.set_page_byte(ctl, reg::APPLY_DPINIT, 0);
            }
            _ => {}
        }
    }
}

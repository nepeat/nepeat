//! One cage: what the daemon knows about its module, and register access to it through
//! the FPGA (page select cached, optoe flat offsets).

use std::time::Duration;

use cs8210_xcvr::bringup::{Bringup, ModuleIo, PortMode};
use cs8210_xcvr::cmis::{self, reg as creg};
use cs8210_xcvr::dom::Monitors;
use cs8210_xcvr::memory::{self, ADDR_A0, Kind, PAGE_SELECT};
use cs8210_xcvr::sff8636;
use serde_json::{Value, json};

use crate::bar::Bar;
use crate::fpga::{Fpga, FpgaError};
use crate::regs::Cage;

/// Settle time after a module write (a module may NACK while it commits).
const WRITE_SETTLE: Duration = Duration::from_millis(2);

/// A cage's state in the daemon.
#[derive(Debug)]
pub struct CageState {
    /// which cage
    pub cage: Cage,
    /// declared (a `SONiC` port uses it): its module is released from reset and brought up
    pub managed: bool,
    /// what its ports need (None: leave the module's default application)
    pub mode: Option<PortMode>,
    /// module present (last sideband snapshot)
    pub present: bool,
    /// `ResetL` is held by a reset request until this time (ms)
    pub hold_until: u64,
    /// the bring-up since the last release / insertion
    pub bringup: Option<Bringup>,
    /// when to step it next (ms)
    pub next_step: u64,
    /// memory map, once read
    pub kind: Option<Kind>,
    /// page select cache (None: unknown)
    pub page: Option<u8>,
    /// lower page + upper page 00h, read once the module is up
    pub page0: Option<Vec<u8>>,
    /// CMIS page 01h (advertising), paged modules
    pub page01: Option<Vec<u8>>,
    /// last monitors
    pub monitors: Option<Monitors>,
    /// last CMIS module state
    pub module_state: Option<String>,
    /// last CMIS data path state per host lane
    pub datapath: Vec<String>,
    /// last CMIS active `ApSel` code per host lane
    pub active_apsel: Vec<u8>,
    /// next monitor read (ms)
    pub next_dom: u64,
    /// failed transfers
    pub i2c_errors: u64,
    /// last error
    pub last_error: Option<String>,
    /// times the daemon released `ResetL`
    pub releases: u64,
    /// consecutive monitor reads that found an up module out of its state
    pub degraded: u32,
}

impl CageState {
    /// A cage nothing is known about yet.
    #[must_use]
    pub fn new(cage: Cage, managed: bool, mode: Option<PortMode>) -> Self {
        Self {
            cage,
            managed,
            mode,
            present: false,
            hold_until: 0,
            bringup: None,
            next_step: 0,
            kind: None,
            page: None,
            page0: None,
            page01: None,
            monitors: None,
            module_state: None,
            datapath: Vec::new(),
            active_apsel: Vec::new(),
            next_dom: 0,
            i2c_errors: 0,
            last_error: None,
            releases: 0,
            degraded: 0,
        }
    }

    /// Forget the module (removed, or reset).
    pub fn forget(&mut self) {
        self.bringup = None;
        self.kind = None;
        self.page = None;
        self.page0 = None;
        self.page01 = None;
        self.monitors = None;
        self.module_state = None;
        self.datapath.clear();
        self.active_apsel.clear();
    }

    /// Bring-up finished (Ready or Failed for good).
    #[must_use]
    pub fn up(&self) -> bool {
        matches!(
            self.bringup.as_ref().map(Bringup::phase),
            Some(cs8210_xcvr::bringup::Phase::Ready { .. })
        )
    }

    /// Record a transfer error.
    pub fn error(&mut self, e: &FpgaError) {
        self.i2c_errors += 1;
        self.last_error = Some(e.to_string());
        self.page = None;
    }

    /// JSON for `status`.
    #[must_use]
    pub fn json(&self, sb: crate::fpga::Sideband) -> Value {
        let bit = self.cage.bit();
        let identity = self.page0.as_deref().and_then(|p| match self.kind {
            Some(Kind::Cmis { .. }) => cmis::identity(p).and_then(|i| serde_json::to_value(i).ok()),
            Some(Kind::Sff8636 { .. }) => sff8636::identity(p).and_then(|i| serde_json::to_value(i).ok()),
            _ => None,
        });
        json!({
            "cage": self.cage.number(),
            "present": sb.present & bit != 0,
            "reset": sb.reset & bit != 0,
            "lpmode": sb.lpmode & bit != 0,
            "interrupt": sb.interrupt & bit != 0,
            "managed": self.managed,
            "mode": self.mode.map(|m| format!("{}x{}G", m.ports, m.gbps)),
            "kind": self.kind,
            "bringup": self.bringup.as_ref().map(|b| json!({"state": b.phase(), "attempt": b.attempts()})),
            "identity": identity,
            "module_state": self.module_state,
            "datapath": self.datapath,
            "active_apsel": self.active_apsel,
            "monitors": self.monitors,
            "i2c_errors": self.i2c_errors,
            "last_error": self.last_error,
            "releases": self.releases,
        })
    }
}

/// Register access to one cage's module (implements the bring-up's [`ModuleIo`]).
pub struct CageIo<'a, B> {
    /// the FPGA
    pub fpga: &'a mut Fpga<B>,
    /// the cage
    pub cage: Cage,
    /// its page select cache
    pub page: &'a mut Option<u8>,
}

impl<B: Bar> CageIo<'_, B> {
    fn select(&mut self, page: Option<u8>, reg: u8) -> Result<(), FpgaError> {
        if let Some(p) = page.filter(|_| reg >= 128)
            && *self.page != Some(p)
        {
            *self.page = None;
            self.fpga.i2c_write(self.cage, ADDR_A0, PAGE_SELECT, &[p])?;
            *self.page = Some(p);
        }
        Ok(())
    }

    /// `len` bytes at `reg` of `addr` (page selected first for an upper-page access).
    pub fn read_at(&mut self, addr: u8, page: Option<u8>, reg: u8, len: u8) -> Result<Vec<u8>, FpgaError> {
        let r = self
            .select(page, reg)
            .and_then(|()| self.fpga.i2c_read(self.cage, addr, reg, u16::from(len)));
        if r.is_err() {
            *self.page = None;
        }
        r
    }

    /// Write `data` at `reg` of `addr`.
    pub fn write_at(&mut self, addr: u8, page: Option<u8>, reg: u8, data: &[u8]) -> Result<(), FpgaError> {
        let r = self
            .select(page, reg)
            .and_then(|()| self.fpga.i2c_write(self.cage, addr, reg, data));
        if r.is_err()
            || (addr == ADDR_A0 && reg <= PAGE_SELECT && usize::from(reg) + data.len() > usize::from(PAGE_SELECT))
        {
            *self.page = None; // a write that covered byte 127 changed the page behind the cache
        }
        std::thread::sleep(WRITE_SETTLE);
        r
    }

    /// The module's memory map (bytes 0 and 2).
    pub fn kind(&mut self, cached: Option<Kind>) -> Result<Kind, FpgaError> {
        if let Some(k) = cached {
            return Ok(k);
        }
        let b = self.read_at(ADDR_A0, None, 0, 3)?;
        Ok(Kind::from_bytes(
            b.first().copied().unwrap_or(0),
            b.get(2).copied().unwrap_or(0),
        ))
    }

    /// optoe flat-offset read (what `sonic_xcvr`'s `read_eeprom` asks for).
    pub fn read_flat(&mut self, kind: Kind, offset: usize, len: usize) -> Result<Vec<u8>, String> {
        let segs = memory::segments(kind, offset, len).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(len);
        for s in segs {
            out.extend(
                self.read_at(s.addr, s.page, s.offset, s.len)
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(out)
    }

    /// optoe flat-offset write.
    pub fn write_flat(&mut self, kind: Kind, offset: usize, data: &[u8]) -> Result<(), String> {
        let segs = memory::segments(kind, offset, data.len()).map_err(|e| e.to_string())?;
        let mut pos = 0usize;
        for s in segs {
            let chunk = data
                .get(pos..pos + usize::from(s.len))
                .ok_or("segment beyond the data")?;
            self.write_at(s.addr, s.page, s.offset, chunk)
                .map_err(|e| e.to_string())?;
            pos += usize::from(s.len);
        }
        Ok(())
    }
}

impl<B: Bar> ModuleIo for CageIo<'_, B> {
    type Error = FpgaError;

    fn read(&mut self, page: Option<u8>, reg: u8, len: u8) -> Result<Vec<u8>, FpgaError> {
        self.read_at(ADDR_A0, page, reg, len)
    }

    fn write(&mut self, page: Option<u8>, reg: u8, data: &[u8]) -> Result<(), FpgaError> {
        self.write_at(ADDR_A0, page, reg, data)
    }
}

/// Read identity (and page 01h for a paged CMIS module) into `st`.
pub fn read_identity<B: Bar>(fpga: &mut Fpga<B>, st: &mut CageState) -> Result<(), FpgaError> {
    let mut io = CageIo {
        fpga,
        cage: st.cage,
        page: &mut st.page,
    };
    let kind = io.kind(st.kind)?;
    let mut p0 = io.read_at(ADDR_A0, None, 0, 128)?;
    p0.extend(io.read_at(ADDR_A0, kind.paged().then_some(0), 128, 128)?);
    let p01 = if matches!(kind, Kind::Cmis { flat: false }) {
        Some(io.read_at(ADDR_A0, Some(creg::PAGE_ADVERTISING), 128, 128)?)
    } else {
        None
    };
    st.kind = Some(kind);
    st.page0 = Some(p0);
    st.page01 = p01;
    Ok(())
}

/// Read the monitors (and CMIS states) into `st`.
pub fn read_monitors<B: Bar>(fpga: &mut Fpga<B>, st: &mut CageState) -> Result<(), FpgaError> {
    let Some(kind) = st.kind else { return Ok(()) };
    let mut io = CageIo {
        fpga,
        cage: st.cage,
        page: &mut st.page,
    };
    let lower = io.read_at(ADDR_A0, None, 0, 128)?;
    match kind {
        Kind::Cmis { flat } => {
            st.module_state = lower.get(3).map(|b| format!("{:?}", cmis::ModuleState::from_byte(*b)));
            let (temperature_c, voltage_v) = cmis::module_monitors(&lower, st.page01.as_deref());
            let mut m = Monitors {
                temperature_c,
                voltage_v,
                lanes: Vec::new(),
            };
            if !flat {
                let p11 = io.read_at(ADDR_A0, Some(creg::PAGE_DP_STATUS), 128, 128)?;
                let at = |r: u8| usize::from(r - 128);
                st.datapath = p11
                    .get(at(creg::DP_STATE)..at(creg::DP_STATE) + 4)
                    .map(|s| cmis::dp_states(s).iter().map(|d| format!("{d:?}")).collect())
                    .unwrap_or_default();
                st.active_apsel = p11
                    .get(at(creg::ACTIVE_APSEL)..at(creg::ACTIVE_APSEL) + 8)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default();
                let apps = st.page0.as_deref().map(cmis::applications).unwrap_or_default();
                let sel = st.active_apsel.first().map_or(0, |c| c >> 4);
                // media lanes of the module: per data path x data paths over the 8 host lanes
                let lanes = apps
                    .iter()
                    .find(|a| a.sel == sel)
                    .map_or(8, |a| a.media_lanes.saturating_mul(8 / a.host_lanes.clamp(1, 8)).min(8));
                if let Some(p01) = st.page01.as_deref() {
                    m.lanes = cmis::lane_monitors(&p11, p01, lanes);
                }
            }
            st.monitors = Some(m);
        }
        Kind::Sff8636 { .. } => {
            let diag = st
                .page0
                .as_deref()
                .and_then(|p| p.get(usize::from(sff8636::reg::DIAG_TYPE)).copied())
                .unwrap_or(0);
            st.monitors = Some(sff8636::monitors(&lower, diag));
        }
        Kind::Sfp | Kind::Other { .. } => {}
    }
    Ok(())
}

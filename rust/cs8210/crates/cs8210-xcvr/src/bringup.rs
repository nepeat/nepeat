//! Bringing a module up after `ResetL` is released: identify it, let it draw its power
//! class (SFF-8636 high-power enable; CMIS `LowPwrRequestSW` clear), wait for CMIS
//! `ModuleReady` and, on a paged CMIS module, put every host lane on the application the
//! cage's ports need (Deinit -> staged `ApSel` -> `Apply_DataPathInit` -> `ConfigSuccess` ->
//! clear Deinit + Tx enable -> `DataPathActivated`), as CMIS 4.0/5.x section 6 and
//! `SONiC`'s xcvrd CMIS manager do it.
//!
//! [`Bringup::step`] never blocks: the caller polls it (its I/O goes through
//! [`ModuleIo`]) with a monotonic millisecond clock and calls again after the delay
//! it returns. Every wait has a deadline from the module's own advertised durations
//! (page 01h), bounded to [`MIN_WAIT_MS`]..[`MAX_WAIT_MS`].

use serde::Serialize;

use crate::cmis::{self, Application, ConfigStatus, DpState, ModuleState, reg};
use crate::memory::Kind;
use crate::{codes, sff8636};

/// Shortest deadline of a wait (modules advertising "< 1 ms" still get this).
pub const MIN_WAIT_MS: u64 = 2_000;
/// Longest deadline of a wait.
pub const MAX_WAIT_MS: u64 = 60_000;
/// How long a module may NACK after reset release before identification fails.
pub const IDENTIFY_MS: u64 = 15_000;
/// Poll interval while a step waits on the module.
pub const POLL_MS: u64 = 100;
/// Delay before a failed bring-up is retried from the start.
pub const RETRY_MS: u64 = 60_000;
/// Attempts before a failure stays.
pub const ATTEMPTS: u32 = 3;

/// Register access of one module: `page` None = the lower page (or an unpaged module).
pub trait ModuleIo {
    /// Transfer error.
    type Error: std::fmt::Display;
    /// `len` bytes at register `reg` (of `page` for 128..=255).
    fn read(&mut self, page: Option<u8>, reg: u8, len: u8) -> Result<Vec<u8>, Self::Error>;
    /// Write `data` at `reg`.
    fn write(&mut self, page: Option<u8>, reg: u8, data: &[u8]) -> Result<(), Self::Error>;
}

/// What a cage carries: `ports` ports of `gbps` each over its 8 host lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PortMode {
    /// ports in the cage (1, 2, 4 or 8)
    pub ports: u8,
    /// speed of each (Gb/s)
    pub gbps: u32,
}

impl PortMode {
    /// Host lanes per port.
    #[must_use]
    pub fn lanes(self) -> u8 {
        8 / self.ports.clamp(1, 8)
    }
}

/// The application for `mode`: host lanes per data path = lanes per port, host interface
/// rate = port speed, a data path may start at every port's first lane; lowest `ApSel` wins.
#[must_use]
pub fn choose(apps: &[Application], mode: PortMode) -> Option<Application> {
    let lanes = mode.lanes();
    let starts = (0..mode.ports).fold(0u8, |m, p| m | 1 << (p * lanes));
    apps.iter().copied().find(|a| {
        a.host_lanes == lanes
            && codes::host_interface_gbps(a.host_if) == Some(mode.gbps)
            && a.host_lane_assignment & starts == starts
    })
}

/// Staged `ApSel` codes of the 8 host lanes for `mode` on `app`.
#[must_use]
pub fn staged_codes(app: &Application, mode: PortMode) -> [u8; 8] {
    let lanes = mode.lanes();
    let mut out = [0u8; 8];
    for (i, c) in out.iter_mut().enumerate() {
        let lane = u8::try_from(i).unwrap_or(0);
        *c = app.staged_code(lane - lane % lanes);
    }
    out
}

/// Where a bring-up is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// reading identifier and memory map (the module may still NACK after reset)
    Identify,
    /// CMIS: waiting for `ModuleReady`
    WaitReady,
    /// CMIS: `DataPathDeinit` set, waiting for `DataPathDeactivated`
    Deinit,
    /// CMIS: staged set applied, waiting for `ConfigSuccess`
    Apply,
    /// CMIS: Deinit cleared and Tx enabled, waiting for `DataPathActivated`
    Activate,
    /// up: on `app` (None: the module needs no application select)
    Ready {
        /// `ApSel` code in use (paged CMIS) or None
        app: Option<u8>,
        /// description of it
        detail: String,
    },
    /// gave up (retried after `RETRY_MS`, ATTEMPTS times)
    Failed {
        /// why
        reason: String,
    },
}

/// What the caller does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// call `step` again after this many ms
    After(u64),
    /// nothing to do until the module changes (Ready, or Failed for good)
    Idle,
}

/// The bring-up of one module (from reset release or insertion).
#[derive(Debug, Clone)]
pub struct Bringup {
    mode: Option<PortMode>,
    phase: Phase,
    started: u64,
    deadline: u64,
    attempts: u32,
    kind: Option<Kind>,
    media_type: u8,
    apps: Vec<Application>,
    target: Option<(Application, [u8; 8])>,
    dp_init_ms: u64,
    dp_deinit_ms: u64,
}

fn bounded(ms: Option<u64>) -> u64 {
    ms.unwrap_or(MAX_WAIT_MS).clamp(MIN_WAIT_MS, MAX_WAIT_MS)
}

impl Bringup {
    /// A bring-up for a cage carrying `mode` (None: leave the module's default
    /// application alone), starting at `now` (ms).
    #[must_use]
    pub fn new(mode: Option<PortMode>, now: u64) -> Self {
        Self {
            mode,
            phase: Phase::Identify,
            started: now,
            deadline: now + IDENTIFY_MS,
            attempts: 1,
            kind: None,
            media_type: 0,
            apps: Vec::new(),
            target: None,
            dp_init_ms: MAX_WAIT_MS,
            dp_deinit_ms: MAX_WAIT_MS,
        }
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The module's memory map, once identified.
    #[must_use]
    pub fn kind(&self) -> Option<Kind> {
        self.kind
    }

    /// Attempt number (1-based).
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    fn fail(&mut self, now: u64, reason: String) -> Next {
        self.phase = Phase::Failed { reason };
        self.deadline = now + RETRY_MS;
        if self.attempts < ATTEMPTS {
            Next::After(RETRY_MS)
        } else {
            Next::Idle
        }
    }

    fn wait(&mut self, now: u64, what: &str) -> Next {
        if now >= self.deadline {
            let t = now - self.started;
            return self.fail(now, format!("timeout ({t} ms) {what}"));
        }
        Next::After(POLL_MS)
    }

    fn enter(&mut self, phase: Phase, now: u64, ms: u64) -> Next {
        self.phase = phase;
        self.started = now;
        self.deadline = now + ms;
        Next::After(POLL_MS)
    }

    /// Advance as far as the module allows now.
    pub fn step<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Next {
        let r = match self.phase.clone() {
            Phase::Identify => self.identify(io, now),
            Phase::WaitReady => self.wait_ready(io, now),
            Phase::Deinit => self.deinit(io, now),
            Phase::Apply => self.apply(io, now),
            Phase::Activate => self.activate(io, now),
            Phase::Ready { .. } => Ok(Next::Idle),
            Phase::Failed { .. } => {
                if self.attempts < ATTEMPTS && now >= self.deadline {
                    let attempts = self.attempts + 1;
                    *self = Self::new(self.mode, now);
                    self.attempts = attempts;
                    Ok(Next::After(0))
                } else if self.attempts < ATTEMPTS {
                    Ok(Next::After(self.deadline - now))
                } else {
                    Ok(Next::Idle)
                }
            }
        };
        match r {
            Ok(n) => n,
            Err(e) if self.phase == Phase::Identify => {
                if now >= self.deadline {
                    self.fail(now, format!("module does not answer: {e}"))
                } else {
                    Next::After(500)
                }
            }
            Err(e) => self.fail(now, format!("I2C: {e}")),
        }
    }

    fn identify<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Result<Next, I::Error> {
        let lower = io.read(None, 0, 128)?;
        let (id, b2) = (lower.first().copied().unwrap_or(0), lower.get(2).copied().unwrap_or(0));
        let kind = Kind::from_bytes(id, b2);
        self.kind = Some(kind);
        match kind {
            Kind::Sff8636 { flat } => {
                let up = io.read((!flat).then_some(0), 128, 128)?;
                let class = sff8636::power_class(up.get(1).copied().unwrap_or(0));
                let pc = lower
                    .get(usize::from(sff8636::reg::POWER_CONTROL))
                    .copied()
                    .unwrap_or(0);
                if let Some(v) = sff8636::high_power_control(pc, class) {
                    io.write(None, sff8636::reg::POWER_CONTROL, &[v])?;
                }
                self.phase = Phase::Ready {
                    app: None,
                    detail: format!("SFF-8636, power class {class}"),
                };
                Ok(Next::Idle)
            }
            Kind::Cmis { flat } => {
                let up = io.read((!flat).then_some(0), 128, 128)?;
                let mut page0 = lower.clone();
                page0.extend_from_slice(&up);
                self.media_type = page0.get(usize::from(reg::MEDIA_TYPE)).copied().unwrap_or(0);
                self.apps = cmis::applications(&page0);
                let mut pwr_up = None;
                if !flat {
                    let p01 = io.read(Some(reg::PAGE_ADVERTISING), 128, 128)?;
                    let at = |r: u8| p01.get(usize::from(r - 128)).copied().unwrap_or(0xff);
                    pwr_up = cmis::timing_ms(at(reg::MODULE_TIMINGS));
                    self.dp_init_ms = bounded(cmis::timing_ms(at(reg::DP_TIMINGS)));
                    self.dp_deinit_ms = bounded(cmis::timing_ms(at(reg::DP_TIMINGS) >> 4));
                }
                let ctl = lower.get(usize::from(reg::MODULE_CONTROL)).copied().unwrap_or(0);
                if ctl & reg::LOW_PWR_REQUEST_SW != 0 {
                    io.write(None, reg::MODULE_CONTROL, &[ctl & !reg::LOW_PWR_REQUEST_SW])?;
                }
                Ok(self.enter(Phase::WaitReady, now, bounded(pwr_up)))
            }
            Kind::Sfp | Kind::Other { .. } => {
                self.phase = Phase::Ready {
                    app: None,
                    detail: format!("identifier {id:#04x}: no bring-up"),
                };
                Ok(Next::Idle)
            }
        }
    }

    fn wait_ready<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Result<Next, I::Error> {
        let st = io.read(None, reg::MODULE_STATE, 1)?;
        match ModuleState::from_byte(st.first().copied().unwrap_or(0)) {
            ModuleState::Ready => {}
            ModuleState::Fault => return Ok(self.fail(now, "ModuleFault".into())),
            ModuleState::LowPwr => return Ok(self.wait(now, "in ModuleLowPwr (LPMode pin asserted?)")),
            s => return Ok(self.wait(now, &format!("waiting for ModuleReady ({s:?})"))),
        }
        let flat = matches!(self.kind, Some(Kind::Cmis { flat: true }));
        let Some(mode) = self.mode.filter(|_| !flat) else {
            self.phase = Phase::Ready {
                app: None,
                detail: "ModuleReady".into(),
            };
            return Ok(Next::Idle);
        };
        let Some(app) = choose(&self.apps, mode) else {
            let want = format!("{}x{}G", mode.ports, mode.gbps);
            return Ok(self.fail(now, format!("no advertised application carries {want}")));
        };
        let codes = staged_codes(&app, mode);
        self.target = Some((app, codes));
        if self.datapath_on_target(io)? {
            return Ok(self.ready(app));
        }
        // Tx off while the data path is reconfigured, then deinit every host lane.
        io.write(Some(reg::PAGE_DP_CONTROL), reg::TX_DISABLE, &[0xff])?;
        io.write(Some(reg::PAGE_DP_CONTROL), reg::DATAPATH_DEINIT, &[0xff])?;
        Ok(self.enter(Phase::Deinit, now, self.dp_deinit_ms))
    }

    fn ready(&mut self, app: Application) -> Next {
        self.phase = Phase::Ready {
            app: Some(app.sel),
            detail: cmis::describe(&app, self.media_type),
        };
        Next::Idle
    }

    fn datapath_on_target<I: ModuleIo>(&mut self, io: &mut I) -> Result<bool, I::Error> {
        let Some((_, codes)) = self.target else {
            return Ok(false);
        };
        let st = io.read(Some(reg::PAGE_DP_STATUS), reg::DP_STATE, 4)?;
        let active = io.read(Some(reg::PAGE_DP_STATUS), reg::ACTIVE_APSEL, 8)?;
        let on = active.iter().zip(codes).all(|(a, c)| a & 0xfe == c & 0xfe);
        Ok(on && cmis::dp_states(&st).iter().all(|s| *s == DpState::Activated))
    }

    fn deinit<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Result<Next, I::Error> {
        let st = io.read(Some(reg::PAGE_DP_STATUS), reg::DP_STATE, 4)?;
        if !cmis::dp_states(&st).iter().all(|s| *s == DpState::Deactivated) {
            return Ok(self.wait(now, "waiting for DataPathDeactivated"));
        }
        let Some((_, codes)) = self.target else {
            return Ok(self.fail(now, "no target".into()));
        };
        io.write(Some(reg::PAGE_DP_CONTROL), reg::STAGED_APSEL, &codes)?;
        io.write(Some(reg::PAGE_DP_CONTROL), reg::APPLY_DPINIT, &[0xff])?;
        Ok(self.enter(Phase::Apply, now, MIN_WAIT_MS * 2))
    }

    fn apply<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Result<Next, I::Error> {
        let cs = cmis::config_status(&io.read(Some(reg::PAGE_DP_STATUS), reg::CONFIG_STATUS, 4)?);
        if let Some((lane, ConfigStatus::Rejected(code))) = cs
            .iter()
            .enumerate()
            .find(|(_, s)| matches!(s, ConfigStatus::Rejected(_)))
        {
            return Ok(self.fail(
                now,
                format!("application rejected on host lane {}: code {code}", lane + 1),
            ));
        }
        if !cs.iter().all(|s| *s == ConfigStatus::Success) {
            return Ok(self.wait(now, "waiting for ConfigSuccess"));
        }
        io.write(Some(reg::PAGE_DP_CONTROL), reg::DATAPATH_DEINIT, &[0x00])?;
        io.write(Some(reg::PAGE_DP_CONTROL), reg::TX_DISABLE, &[0x00])?;
        Ok(self.enter(Phase::Activate, now, self.dp_init_ms + MIN_WAIT_MS))
    }

    fn activate<I: ModuleIo>(&mut self, io: &mut I, now: u64) -> Result<Next, I::Error> {
        if self.datapath_on_target(io)?
            && let Some((app, _)) = self.target
        {
            return Ok(self.ready(app));
        }
        Ok(self.wait(now, "waiting for DataPathActivated"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab;
    use crate::sim::SimModule;

    struct Io<'a>(&'a mut SimModule);
    impl ModuleIo for Io<'_> {
        type Error = String;
        fn read(&mut self, page: Option<u8>, reg: u8, len: u8) -> Result<Vec<u8>, String> {
            if let Some(p) = page {
                self.0.write(0x50, 127, &[p])?;
            }
            self.0.read(0x50, reg, len)
        }
        fn write(&mut self, page: Option<u8>, reg: u8, data: &[u8]) -> Result<(), String> {
            if let Some(p) = page {
                self.0.write(0x50, 127, &[p])?;
            }
            self.0.write(0x50, reg, data)
        }
    }

    fn run(b: &mut Bringup, m: &mut SimModule) -> Phase {
        let mut now = 0;
        for _ in 0..1000 {
            match b.step(&mut Io(m), now) {
                Next::Idle => break,
                Next::After(ms) => now += ms.max(1),
            }
            if matches!(b.phase(), Phase::Failed { .. }) {
                break;
            }
        }
        b.phase().clone()
    }

    const M400: PortMode = PortMode { ports: 1, gbps: 400 };

    #[test]
    fn dr4_from_4x100_default_to_1x400() {
        let mut m = SimModule::lab_dr4();
        let mut b = Bringup::new(Some(M400), 0);
        let p = run(&mut b, &mut m);
        assert_eq!(
            p,
            Phase::Ready {
                app: Some(1),
                detail: "400GAUI-8 C2M (Annex 120E) / 400GBASE-DR4 (Cl 124), 8x4".into()
            }
        );
        assert_eq!(m.active_apsel(), [0x10; 8]);
        assert!(m.dp_activated());
        assert_eq!(m.page_byte(0x10, 130), 0x00); // Tx enabled again
        // a second run finds it on target and writes nothing
        let writes = m.writes();
        let mut b = Bringup::new(Some(M400), 0);
        assert!(matches!(run(&mut b, &mut m), Phase::Ready { app: Some(1), .. }));
        assert_eq!(m.writes(), writes);
    }

    #[test]
    fn dr4_breakout_keeps_app2() {
        let mut m = SimModule::lab_dr4();
        let writes = m.writes();
        let mut b = Bringup::new(Some(PortMode { ports: 4, gbps: 100 }), 0);
        assert!(matches!(run(&mut b, &mut m), Phase::Ready { app: Some(2), .. }));
        assert_eq!(m.writes(), writes, "already on 4x100G: nothing written");
    }

    #[test]
    fn no_matching_application_fails_without_writes() {
        let mut m = SimModule::lab_dr4();
        let writes = m.writes();
        let mut b = Bringup::new(Some(PortMode { ports: 2, gbps: 200 }), 0);
        let p = run(&mut b, &mut m);
        assert!(
            matches!(&p, Phase::Failed { reason } if reason.contains("2x200G")),
            "{p:?}"
        );
        assert_eq!(m.writes(), writes);
    }

    #[test]
    fn flat_dac_needs_nothing() {
        let mut m = SimModule::from_image(lab::dac_page0());
        let mut b = Bringup::new(Some(M400), 0);
        assert_eq!(
            run(&mut b, &mut m),
            Phase::Ready {
                app: None,
                detail: "ModuleReady".into()
            }
        );
        assert_eq!(m.writes(), 0);
    }

    #[test]
    fn low_power_request_is_cleared() {
        let mut m = SimModule::lab_dr4();
        m.force_low_power();
        let mut b = Bringup::new(None, 0);
        assert!(matches!(run(&mut b, &mut m), Phase::Ready { app: None, .. }));
        assert_eq!(m.lower_byte(26) & cmis::reg::LOW_PWR_REQUEST_SW, 0);
    }

    #[test]
    fn rejected_application_is_reported() {
        let mut m = SimModule::lab_dr4();
        m.reject_apply(3);
        let mut b = Bringup::new(Some(M400), 0);
        let p = run(&mut b, &mut m);
        assert!(
            matches!(&p, Phase::Failed { reason } if reason.contains("rejected") && reason.contains("code 3")),
            "{p:?}"
        );
        // retried later, three attempts in all
        let mut now = 1_000_000;
        for _ in 0..50 {
            if let Next::After(ms) = b.step(&mut Io(&mut m), now) {
                now += ms;
            }
        }
        assert_eq!(b.attempts(), ATTEMPTS);
    }

    #[test]
    fn unanswering_module_times_out() {
        let mut m = SimModule::lab_dr4();
        m.set_nack(true);
        let mut b = Bringup::new(Some(M400), 0);
        let p = run(&mut b, &mut m);
        assert!(
            matches!(&p, Phase::Failed { reason } if reason.contains("does not answer")),
            "{p:?}"
        );
    }

    #[test]
    fn choose_and_codes() {
        let apps = cmis::applications(&lab::dr4_page0());
        assert_eq!(choose(&apps, M400).map(|a| a.sel), Some(1));
        assert_eq!(choose(&apps, PortMode { ports: 4, gbps: 100 }).map(|a| a.sel), Some(2));
        assert_eq!(choose(&apps, PortMode { ports: 8, gbps: 50 }), None);
        let a2 = choose(&apps, PortMode { ports: 4, gbps: 100 }).unwrap();
        assert_eq!(
            staged_codes(&a2, PortMode { ports: 4, gbps: 100 }),
            [0x20, 0x20, 0x24, 0x24, 0x28, 0x28, 0x2c, 0x2c]
        );
    }
}

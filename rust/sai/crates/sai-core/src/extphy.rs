//! The external PHY runtime's seam in the core (AS7772 retimers and `MACsec` PHYs): the
//! per-switch state ([`External`], kept in the store), switch creation's `ext_phy_load` /
//! `ext_phy_attach` / `ext_phy_detach`, the admin hook of the port area ([`ext_phy_admin`]),
//! the diag socket's `extphy`, raw MDIO and transactions, and the `MACsec` clear-window keys. The
//! runtime itself (link gate, debounce, admin sequences, counters, status text, the per-PHY MDIO
//! arbitration and diag leases) is the crate `sai-extphy`; this module wires it to the backend.
//!
//! # The gate and the bus
//!
//! The backend asks each gate on its link-scan thread ([`crate::backend::types::LinkGate`]),
//! where neither the SAI lock nor the backend can be taken. So the runtime gets its own handle
//! of the backend's MDIO master at attach ([`crate::backend::Backend::mdio_bus`]: on the
//! virtual chip the register file behind its own lock; through the recording decorator every
//! access is logged as the `mdio_c45_read` / `_write` it stands for, as the C++ gate's calls on
//! the recording backend are). A gate closure holds a clone of the runtime and of the `MACsec`
//! hold: it takes the runtime's PHY slot for its port's PHYs and the handle's lock, never the
//! SAI lock. Lock order: SAI lock, a diag lease, a PHY's slot, the bus master's (`sai-extphy`,
//! "Serialisation").
//!
//! # The diag socket's raw MDIO
//!
//! With a table attached ([`manages_phys`]), `phy raw c45 ...` and `extphy txn ...` are served
//! by [`crate::diag::DiagPort`] **without** the SAI lock, over the same MDIO handle, inside the
//! arbitration; what it does not serve (and everything without a table) runs through the
//! backend's shell under the SAI lock via [`ext_phy_raw_shell`].
//!
//! # Profile keys
//!
//! | key | values (default) | |
//! |---|---|---|
//! | `SAI_OPENBCM_EXT_PHY_FILE` | path of an `ext-phy 1` table (none) | C++ |
//! | `SAI_OPENBCM_EXT_MACSEC_FILE` | path of an `ext-macsec 1` table (none) | C++ |
//! | `SAI_OPENBCM_MACSEC_HOLD_CLOSED` | `all` or first lanes `1,5,...` of ports of the `MACsec` table: their link stays down (the gate) until the port has a `MACSEC_PORT` (off) | designed, off: owner's decision on the clear-traffic window |
//! | `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP` | `1`: removing a `MACSEC_PORT` leaves its engine on, data dropped, and start-up resets a left-on engine without bypass (`0`) | designed, off: the same decision |
//!
//! A value that does not parse refuses the switch (`INVALID_PARAMETER`), as the table files do.
use crate::backend::types::{GateVerdict, LinkGate, MdioAccess, MdioBus, MdioDev, PhyAddr};
use crate::backend::{Backend, HwPort, Profile};
use crate::error::{Error, Result};
use crate::macsec::{Hold, MacsecOptions, MacsecState};
use crate::sai::Sai;
use crate::store::Store;
use sai_extphy::{Api, BusStatus, ChipPort, Dev, Level, LoadError, Logger, PhyBus, PortRef, Verdict};
use sai_sys as sys;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

pub use sai_extphy::{
    ArbCounts, ChipPort as ChipPortNo, Clock, DiagTarget, DiagWho, ExtPhy, LEASE_DEFAULT_MS, LEASE_MAX_MS, LeaseScope,
    LeaseView, MACSEC_POLL_DEFER_MAX_MS, ManualClock, SystemClock, TxnRefused,
};

/// `SAI_OPENBCM_EXT_PHY_FILE`
pub const EXT_PHY_FILE: &str = "SAI_OPENBCM_EXT_PHY_FILE";
/// `SAI_OPENBCM_EXT_MACSEC_FILE`
pub const EXT_MACSEC_FILE: &str = "SAI_OPENBCM_EXT_MACSEC_FILE";
/// `SAI_OPENBCM_MACSEC_HOLD_CLOSED` (designed, off by default; module documentation).
pub const MACSEC_HOLD_CLOSED: &str = "SAI_OPENBCM_MACSEC_HOLD_CLOSED";
/// `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP` (designed, off by default).
pub const MACSEC_ENGINES_ON_AT_STOP: &str = "SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP";
/// The keys this module reads.
pub const PROFILE_KEYS: [&str; 4] = [
    EXT_PHY_FILE,
    EXT_MACSEC_FILE,
    MACSEC_HOLD_CLOSED,
    MACSEC_ENGINES_ON_AT_STOP,
];

/// Which ports `SAI_OPENBCM_MACSEC_HOLD_CLOSED` names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum HoldSpec {
    /// Off.
    #[default]
    Off,
    /// Every port with a `MACsec` engine.
    All,
    /// The ports starting at these lanes.
    Lanes(BTreeSet<u32>),
}

/// The external-PHY and `MACsec` state of one switch: kept in the store.
pub struct External {
    pub(crate) phy: ExtPhy,
    pub(crate) macsec: MacsecState,
    pub(crate) options: MacsecOptions,
    pub(crate) hold_spec: HoldSpec,
    /// The ports whose gate this switch registered.
    gated: Vec<HwPort>,
}

impl fmt::Debug for External {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("External")
            .field("phy", &self.phy)
            .field("macsec", &self.macsec)
            .field("options", &self.options)
            .field("gated", &self.gated)
            .finish_non_exhaustive()
    }
}

impl Default for External {
    /// No table, nothing held: the state of a switch without external PHYs.
    fn default() -> Self {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        Self {
            phy: ExtPhy::inactive(clock, logger()),
            macsec: MacsecState::default(),
            options: MacsecOptions::default(),
            hold_spec: HoldSpec::Off,
            gated: Vec::new(),
        }
    }
}

/// The runtime's log lines into the library's log.
fn logger() -> Logger {
    Arc::new(|api, level, text: &str| {
        let api = match api {
            Api::Switch => sys::SAI_API_SWITCH,
            Api::Port => sys::SAI_API_PORT,
        };
        let level = match level {
            Level::Error => sys::SAI_LOG_LEVEL_ERROR,
            Level::Warn => sys::SAI_LOG_LEVEL_WARN,
            Level::Notice => sys::SAI_LOG_LEVEL_NOTICE,
            Level::Info => sys::SAI_LOG_LEVEL_INFO,
        };
        crate::log::emit(api, level, format_args!("{text}"));
    })
}

/// The backend's MDIO handle as the runtime's bus.
#[derive(Debug)]
struct HandleBus(MdioBus);

impl PhyBus for HandleBus {
    fn read(&self, phy: u16, dev: Dev, reg: u16) -> core::result::Result<u16, BusStatus> {
        let dev = MdioDev::new(dev.get()).ok_or(BusStatus(Error::InvalidParameter.status()))?;
        self.0
            .read(PhyAddr(u32::from(phy)), dev, reg)
            .map_err(|e| BusStatus(e.status()))
    }
    fn write(&self, phy: u16, dev: Dev, reg: u16, value: u16) -> core::result::Result<(), BusStatus> {
        let dev = MdioDev::new(dev.get()).ok_or(BusStatus(Error::InvalidParameter.status()))?;
        self.0
            .write(PhyAddr(u32::from(phy)), dev, reg, value)
            .map_err(|e| BusStatus(e.status()))
    }
}

/// `1` / `0` / absent.
fn flag(profile: &Profile, key: &str) -> Result<bool> {
    match profile.get(key).unwrap_or_default() {
        "" | "0" => Ok(false),
        "1" => Ok(true),
        other => {
            crate::log::error(sys::SAI_API_SWITCH, format_args!("{key} '{other}': 0|1"));
            Err(Error::InvalidParameter)
        }
    }
}

/// `all`, first lanes `1,5`, or empty.
fn hold_spec(profile: &Profile) -> Result<HoldSpec> {
    let text = profile.get(MACSEC_HOLD_CLOSED).unwrap_or_default().trim();
    if text.is_empty() {
        return Ok(HoldSpec::Off);
    }
    if text == "all" {
        return Ok(HoldSpec::All);
    }
    text.split(',')
        .map(|lane| lane.trim().parse::<u32>().ok().filter(|l| *l > 0))
        .collect::<Option<BTreeSet<u32>>>()
        .map(HoldSpec::Lanes)
        .ok_or_else(|| {
            crate::log::error(
                sys::SAI_API_SWITCH,
                format_args!("{MACSEC_HOLD_CLOSED} '{text}': all, or first lanes of MACsec ports (1,5,...)"),
            );
            Error::InvalidParameter
        })
}

/// `ext_phy_load` (before the backend is chosen; also the rim's precheck): reads and parses
/// `SAI_OPENBCM_EXT_PHY_FILE` (cannot open: `FAILURE`, a bad table: `INVALID_PARAMETER`, both
/// refuse the switch), and the `MACsec` clear-window keys.
pub(crate) fn ext_phy_load(profile: &Profile) -> Result<External> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let phy = ExtPhy::load(profile.get(EXT_PHY_FILE), clock, logger()).map_err(|e| match e {
        LoadError::CannotOpen => Error::Failure,
        LoadError::Parse(_) => Error::InvalidParameter,
    })?;
    let options = MacsecOptions {
        engines_on_at_stop: flag(profile, MACSEC_ENGINES_ON_AT_STOP)?,
    };
    Ok(External {
        phy,
        macsec: MacsecState::default(),
        options,
        hold_spec: hold_spec(profile)?,
        gated: Vec::new(),
    })
}

/// The link gate of a port: the `MACsec` hold first, then the runtime's verdict.
fn gate_of(phy: &ExtPhy, hold: &Hold, hw: HwPort) -> LinkGate {
    let (phy, hold) = (phy.clone(), hold.clone());
    Arc::new(move || {
        if hold.closed(hw) {
            return GateVerdict::Down;
        }
        match phy.gate(ChipPort(hw.0)) {
            Verdict::Down => GateVerdict::Down,
            Verdict::NoOpinion => GateVerdict::NoOpinion,
        }
    })
}

/// Registers the gate of `hw`; refused: logged, the caller detaches.
pub(crate) fn register_gate<B: Backend + ?Sized>(backend: &mut B, ext: &mut External, hw: HwPort) -> Result<()> {
    backend
        .port_link_gate(hw, Some(gate_of(&ext.phy, &ext.macsec.hold, hw)))
        .inspect_err(|e| {
            crate::log::error(
                sys::SAI_API_SWITCH,
                format_args!(
                    "external PHY table: hw port {}: the backend cannot gate its link ({})",
                    hw.0,
                    e.status()
                ),
            );
        })?;
    if !ext.gated.contains(&hw) {
        ext.gated.push(hw);
    }
    Ok(())
}

/// `ext_phy_attach` (after backend init, before the ports are disabled): the backend's ports
/// (asked first, table or not, as the C++ does), the table's lanes to ports, the MDIO probe
/// (`mdio_c45_read(0, 1, 0)`: `NOT_SUPPORTED` = no MDIO), the gates.
pub(crate) fn ext_phy_attach<B: Backend + ?Sized>(backend: &mut B, ext: &mut External) -> Result<()> {
    let ports: Vec<PortRef> = backend
        .ports()
        .into_iter()
        .map(|p| PortRef {
            hw: ChipPort(p.hw.0),
            first_lane: p.lanes.first().copied(),
            name: p.name,
        })
        .collect();
    if !ext.phy.has_ports() {
        return Ok(());
    }
    let bus = backend.mdio_bus();
    let gated = ext
        .phy
        .attach(
            &ports,
            Box::new(HandleBus(bus.clone().unwrap_or_else(|| Arc::new(NoBus)))),
        )
        .map_err(|_| Error::InvalidParameter)?;
    let probe = MdioDev::new(1)
        .ok_or(Error::Failure)
        .and_then(|dev| backend.mdio_c45_read(PhyAddr(0), dev, 0));
    if bus.is_none() || probe == Err(Error::NotSupported) {
        crate::log::error(
            sys::SAI_API_SWITCH,
            format_args!("external PHY table: backend {} has no MDIO access", backend.name()),
        );
        ext_phy_detach(backend, ext);
        return Err(Error::NotSupported);
    }
    for hw in &gated {
        if let Err(e) = register_gate(backend, ext, HwPort(hw.0)) {
            ext_phy_detach(backend, ext);
            return Err(e);
        }
    }
    ext.phy.log_attached(gated.len());
    Ok(())
}

/// The bus of a backend without an MDIO handle (never used: attach refuses such a backend).
#[derive(Debug)]
struct NoBus;

impl MdioAccess for NoBus {
    fn read(&self, _phy: PhyAddr, _dev: MdioDev, _reg: u16) -> Result<u16> {
        Err(Error::NotSupported)
    }
    fn write(&self, _phy: PhyAddr, _dev: MdioDev, _reg: u16, _val: u16) -> Result<()> {
        Err(Error::NotSupported)
    }
}

/// `ext_phy_detach` (before the backend goes away): the gates an active table registered
/// are removed (the C++ removes every port with a link sequence once the table is active,
/// registered or not), then the table is forgotten. The `MACsec` hold's gates go too.
pub(crate) fn ext_phy_detach<B: Backend + ?Sized>(backend: &mut B, ext: &External) {
    let mut gated: Vec<HwPort> = ext.phy.gated_ports().into_iter().map(|p| HwPort(p.0)).collect();
    for hw in &ext.gated {
        if !gated.contains(hw) {
            gated.push(*hw);
        }
    }
    // without the runtime's lock: the backend may be waiting for a gate to return
    for hw in gated {
        let _ = backend.port_link_gate(hw, None);
    }
    ext.phy.clear();
}

/// `ext_phy_admin`: the external PHY's line side for a port's admin state, on before the MAC
/// is enabled, off after it is disabled (the port area, [`crate::port`]). The sequence's MDIO
/// goes through the runtime's bus handle.
pub(crate) fn ext_phy_admin<B: Backend + ?Sized>(backend: &mut B, store: &Store, hw: HwPort, up: bool) {
    let _ = backend;
    ext_phy_admin_at(store.ext(), hw, up);
}

/// [`ext_phy_admin`] on the state of a switch that is being built (not in a store yet).
pub(crate) fn ext_phy_admin_at(ext: &External, hw: HwPort, up: bool) {
    ext.phy.admin(ChipPort(hw.0), up);
}

/// The diag socket's `extphy` answer (C++ `ext_phy_status`).
pub fn ext_phy_status<B: Backend>(sai: &Sai<B>) -> String {
    sai.store().map_or_else(
        || "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n".to_owned(),
        |s| s.ext().phy.status(),
    )
}

/// Runs `body`, raw MDIO of the diag socket (`phy raw ...`) on the backend's shell, within the
/// PHY arbitration (C++ `ext_phy_foreign_mdio`): never inside a library sequence on `target`,
/// refused inside another client's transaction; without a transaction (`who` [`DiagWho::Bare`])
/// it holds the polls of `target` off `holdoff-ms` after it. Under the SAI lock (the caller's):
/// only for what the arbitration's own path ([`crate::diag::DiagPort`], no SAI lock) does not
/// serve.
pub fn ext_phy_raw_shell<B: Backend, T>(
    sai: &mut Sai<B>,
    who: DiagWho,
    target: DiagTarget,
    body: impl FnOnce(&mut B) -> T,
) -> core::result::Result<T, TxnRefused> {
    let phy = sai.store().map(|s| s.ext().phy.clone());
    match phy {
        Some(phy) => phy.diag_access(who, target, || body(sai.backend_mut())),
        None => Ok(body(sai.backend_mut())),
    }
}

/// Whether the library runs sequences of its own on external PHYs (an external PHY or
/// `MACsec` table is attached): only then does the diag socket serve clause-45 raw MDIO itself
/// (outside the SAI lock); otherwise the SDK shell answers it as before.
pub(crate) fn manages_phys<B: Backend>(sai: &Sai<B>) -> bool {
    sai.store()
        .is_some_and(|s| s.ext().phy.active() || s.ext().macsec.model.as_ref().is_some_and(|m| m.active()))
}

/// Whether an external PHY table is attached (C++ `ext_phy_active`).
pub fn ext_phy_active<B: Backend>(sai: &Sai<B>) -> bool {
    sai.store().is_some_and(|s| s.ext().phy.active())
}

/// The switch's external PHY runtime (tests: counters, state).
pub fn ext_phy<B: Backend>(sai: &Sai<B>) -> Option<ExtPhy> {
    sai.store().map(|s| s.ext().phy.clone())
}

/// Replaces the clock of the switch's runtime (tests: a [`ManualClock`], so the poll period
/// and the hold-off pass without sleeping). Nothing without a switch.
pub fn set_clock<B: Backend>(sai: &Sai<B>, clock: Arc<dyn Clock>) {
    if let Some(store) = sai.store() {
        store.ext().phy.set_clock(clock);
    }
}

/// The test port's question ([`crate::testing`]): an external PHY table is attached.
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_ext_phy_active<B: Backend>(sai: &Sai<B>) -> crate::testing::AreaAnswer<bool> {
    Ok(ext_phy_active(sai))
}

//! The state of the model and the logic that brings a PHY to what the objects say
//! (`reconcile`): which sequences run, in which order, with which parameters.
use crate::hkey::AuthKeyPolicy;
use crate::ids::{AclEntryId, AclTableId, Alloc, An, Cipher, Dir, FlowId, MacsecId, MacsecPortId, PortKey, SaId, ScId};
use crate::runner::{SeqRunner, seq};
use crate::secret::{Params, Results, param};
use crate::stats::{CounterMode, Counters};
use crate::value::AclField;
use sai_core::oid::{PortId, RawOid};
use sai_core::{Error, Result};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Instant;

/// Severity of a log line (the C++ `OB_ERR` / `OB_WARN` / `OB_NOTICE` / `OB_INFO`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something failed.
    Error,
    /// Something was accepted that will not behave as asked.
    Warn,
    /// A state change worth a line in the journal.
    Notice,
    /// Detail.
    Info,
}

/// Receives the model's log lines. No line ever contains key bytes.
pub type Logger = Box<dyn FnMut(Level, &str) + Send>;

/// Hands out the id of a new object of a SAI object type (crate documentation, D7: in the
/// library the ids come from the allocator every other object uses, so an ACL table of a
/// MACsec stage never shares an id with one of the ASIC's). The id must be of that type and
/// of switch 0 (`[ids]` layout); `None` (or an id that is not) fails the create with
/// `SAI_STATUS_FAILURE` before anything changed. Without one the model counts by itself.
pub type IdAlloc = Box<dyn FnMut(sys::sai_object_type_t) -> Option<RawOid> + Send>;

/// A port of the chip as the board table needs it, before the SAI port objects exist
/// ([`Macsec::attach_chip`]: the library attaches the table while the switch is being
/// created, then hands the SAI ids over with [`Macsec::set_switch_ports`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipPort {
    /// The chip's logical port.
    pub key: PortKey,
    /// Its name in log lines and the status text.
    pub name: String,
    /// Its lanes; the table names a port by its first lane.
    pub lanes: Vec<u32>,
}

/// A front-panel port of the switch, as the model needs to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchPort {
    /// Its SAI id.
    pub id: PortId,
    /// The chip's logical port: what sequences are run for.
    pub key: PortKey,
    /// Its name in log lines and the status text (the SDK's port name).
    pub name: String,
    /// Its lanes; the table names a port by its first lane.
    pub lanes: Vec<u32>,
}

/// A `port <first lane> <phy>` line of the external MACsec table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineAddr {
    /// First lane of the port.
    pub lane: u32,
    /// MDIO address word of its PHY (shown in the status text; the runner addresses the PHY itself).
    pub phy: u32,
}

/// What the model needs of the external MACsec table besides its sequences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    /// The `counters` line.
    pub counters: CounterMode,
    /// The `ifg-bits` line; 0 = leave the inter-frame gap alone.
    pub ifg_bits: u32,
    /// The `port` lines.
    pub engines: Vec<EngineAddr>,
}

/// Behaviour that is not in the board table.
#[derive(Default)]
pub struct Options {
    /// Whether `SAI_MACSEC_SA_ATTR_AUTH_KEY` is checked against the SAK.
    pub auth_key: AuthKeyPolicy,
    /// Where log lines go; `None` drops them.
    pub log: Option<Logger>,
    /// Where object ids come from; `None`: the model's own counter.
    pub alloc: Option<IdAlloc>,
}

/// What the PHY of one port holds, one direction (`Hw`).
#[derive(Debug, Default)]
pub(crate) struct Engine {
    pub(crate) enabled: bool,
    pub(crate) gate: bool,
    /// Installed SA per AN.
    sa: [Option<SaId>; 4],
    /// Egress: the transmitting SA.
    pub(crate) tx: Option<SaId>,
    pub(crate) runs: u64,
    pub(crate) fails: u64,
    pub(crate) max_us: u64,
    pub(crate) last_fail: String,
}

impl Engine {
    pub(crate) fn slot(&self, an: An) -> Option<SaId> {
        self.sa.get(an.idx()).copied().flatten()
    }

    pub(crate) fn set_slot(&mut self, an: An, sa: Option<SaId>) {
        if let Some(slot) = self.sa.get_mut(an.idx()) {
            *slot = sa;
        }
    }

    pub(crate) fn installed_count(&self) -> usize {
        self.sa.iter().flatten().count()
    }

    /// Back to "in bypass, nothing installed"; the run statistics stay.
    pub(crate) fn clear(&mut self) {
        self.enabled = false;
        self.gate = false;
        self.sa = [None; 4];
        self.tx = None;
    }
}

/// A port with a MACsec PHY (`PortHw`).
#[derive(Debug)]
pub(crate) struct EnginePort {
    pub(crate) phy: u32,
    pub(crate) name: String,
    /// Its SAI id; `None` while the SAI has none (C++: no `hw2port` entry).
    pub(crate) port: Option<PortId>,
    egress: Engine,
    ingress: Engine,
}

impl EnginePort {
    pub(crate) fn engine(&self, dir: Dir) -> &Engine {
        match dir {
            Dir::Egress => &self.egress,
            Dir::Ingress => &self.ingress,
        }
    }

    pub(crate) fn engine_mut(&mut self, dir: Dir) -> &mut Engine {
        match dir {
            Dir::Egress => &mut self.egress,
            Dir::Ingress => &mut self.ingress,
        }
    }
}

#[derive(Debug)]
pub(crate) struct MsPort {
    pub(crate) dir: Dir,
    pub(crate) port: PortId,
    pub(crate) hw: PortKey,
}

#[derive(Debug)]
pub(crate) struct MsFlow {
    pub(crate) dir: Dir,
    pub(crate) acc: Counters,
}

#[derive(Debug)]
pub(crate) struct MsSc {
    pub(crate) dir: Dir,
    pub(crate) flow: FlowId,
    /// As on the wire.
    pub(crate) sci: [u8; 8],
    pub(crate) cipher: Cipher,
    pub(crate) replay: bool,
    pub(crate) window: u32,
    /// The port an SA of this SC was last installed on; never cleared (as in the C++).
    pub(crate) hw: Option<PortKey>,
}

#[derive(Debug)]
pub(crate) struct MsSa {
    pub(crate) dir: Dir,
    pub(crate) sc: ScId,
    pub(crate) an: An,
    /// key, hkey, pn and for XPN salt, ssci, iv: until installed, then wiped.
    pub(crate) par: Params,
    /// As configured; read back from the PHY once installed.
    pub(crate) pn: u64,
    /// Creation order.
    pub(crate) order: u64,
    pub(crate) installed: bool,
    pub(crate) acc: Counters,
}

#[derive(Debug)]
pub(crate) struct MsAclTable {
    pub(crate) dir: Dir,
    pub(crate) port: Option<PortId>,
    pub(crate) fields: BTreeSet<sys::sai_attr_id_t>,
    /// In creation order.
    pub(crate) entries: Vec<AclEntryId>,
}

#[derive(Clone, Debug)]
pub(crate) struct MsAclEntry {
    pub(crate) table: AclTableId,
    pub(crate) priority: u32,
    pub(crate) admin: bool,
    pub(crate) flow_on: bool,
    pub(crate) flow: Option<FlowId>,
    pub(crate) action_on: bool,
    pub(crate) action: i32,
    pub(crate) fields: BTreeMap<sys::sai_attr_id_t, AclField>,
}

/// The MACsec objects of one switch and what its MACsec PHYs hold.
///
/// Not `Sync`-aware: every method takes `&mut self` or `&self`; the caller (the SAI layer)
/// holds its one lock around each call, as the C++ `guarded` does.
pub struct Macsec {
    /// A table was given (`need_table`).
    pub(crate) loaded: bool,
    pub(crate) counters: CounterMode,
    pub(crate) ifg_bits: u32,
    pub(crate) auth_key: AuthKeyPolicy,
    log: Option<Logger>,
    /// Every front-panel port: SAI id to chip port.
    pub(crate) switch_ports: BTreeMap<PortId, PortKey>,
    /// The ports with a MACsec PHY.
    pub(crate) ports: BTreeMap<PortKey, EnginePort>,
    pub(crate) objs: BTreeMap<MacsecId, Dir>,
    pub(crate) mports: BTreeMap<MacsecPortId, MsPort>,
    pub(crate) flows: BTreeMap<FlowId, MsFlow>,
    pub(crate) scs: BTreeMap<ScId, MsSc>,
    pub(crate) sas: BTreeMap<SaId, MsSa>,
    pub(crate) tables: BTreeMap<AclTableId, MsAclTable>,
    pub(crate) entries: BTreeMap<AclEntryId, MsAclEntry>,
    pub(crate) alloc: Alloc,
    ext_alloc: Option<IdAlloc>,
    pub(crate) order: u64,
}

impl fmt::Debug for Macsec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only: the SAs hold key material until they are installed.
        f.debug_struct("Macsec")
            .field("loaded", &self.loaded)
            .field("engine_ports", &self.ports.len())
            .field("macsec_ports", &self.mports.len())
            .field("flows", &self.flows.len())
            .field("scs", &self.scs.len())
            .field("sas", &self.sas.len())
            .finish_non_exhaustive()
    }
}

impl Macsec {
    /// A switch without an external MACsec table: the API is there and every create says
    /// `NOT_SUPPORTED`.
    pub fn without_table(options: Options) -> Self {
        Self {
            loaded: false,
            counters: CounterMode::default(),
            ifg_bits: 0,
            auth_key: options.auth_key,
            log: options.log,
            switch_ports: BTreeMap::new(),
            ports: BTreeMap::new(),
            objs: BTreeMap::new(),
            mports: BTreeMap::new(),
            flows: BTreeMap::new(),
            scs: BTreeMap::new(),
            sas: BTreeMap::new(),
            tables: BTreeMap::new(),
            entries: BTreeMap::new(),
            alloc: Alloc::default(),
            ext_alloc: options.alloc,
            order: 0,
        }
    }

    /// The index of a new object of SAI type `ty` (the caller makes the typed id of it).
    pub(crate) fn new_index(&mut self, ty: sys::sai_object_type_t) -> Result<u64> {
        let Some(alloc) = self.ext_alloc.as_mut() else {
            return Ok(self.alloc.next());
        };
        let raw = alloc(ty).ok_or(Error::Failure)?;
        if raw.type_bits() != ty || (raw.0 >> 48) & 0xff != 0 || raw.0 & ((1 << 48) - 1) == 0 {
            return Err(Error::Failure);
        }
        Ok(raw.0 & ((1 << 48) - 1))
    }

    /// The model of a switch with a table (`ext_macsec_attach`): maps the table's ports
    /// (by first lane) to the switch's, then puts every engine a previous process left on
    /// back into bypass. This process has no MACsec objects yet, so an engine that is not in
    /// bypass (`probe.<dir>`, when the table has it) holds keys nothing here knows: it is
    /// reset and disabled.
    ///
    /// `InvalidParameter` when a table port starts at a lane no switch port starts at.
    pub fn attach(table: &Table, ports: &[SwitchPort], options: Options, runner: &mut dyn SeqRunner) -> Result<Self> {
        let chip: Vec<(Option<PortId>, ChipPort)> = ports
            .iter()
            .map(|p| {
                let port = ChipPort {
                    key: p.key,
                    name: p.name.clone(),
                    lanes: p.lanes.clone(),
                };
                (Some(p.id), port)
            })
            .collect();
        let mut m = Self::attach_with(table, &chip, options, runner)?;
        m.switch_ports = ports.iter().map(|p| (p.id, p.key)).collect();
        Ok(m)
    }

    /// [`Macsec::attach`] for a switch whose SAI port objects do not exist yet (the
    /// library's order: the table is attached while the switch is created, before its ports
    /// are). Until [`Macsec::set_switch_ports`] no SAI port is known: a MACSEC_PORT create
    /// names none, nothing is bound.
    pub fn attach_chip(
        table: &Table,
        ports: &[ChipPort],
        options: Options,
        runner: &mut dyn SeqRunner,
    ) -> Result<Self> {
        let chip: Vec<(Option<PortId>, ChipPort)> = ports.iter().map(|p| (None, p.clone())).collect();
        Self::attach_with(table, &chip, options, runner)
    }

    /// The switch's front-panel ports, SAI id to chip port: replaces what the model knew (the
    /// C++ looks ports up in the live port table at every call; the library calls this
    /// whenever its port table differs from what it last handed over).
    pub fn set_switch_ports(&mut self, ports: &[(PortId, PortKey)]) {
        self.switch_ports = ports.iter().copied().collect();
        for (key, port) in &mut self.ports {
            port.port = ports.iter().rfind(|(_, k)| k == key).map(|(id, _)| *id);
        }
    }

    fn attach_with(
        table: &Table,
        ports: &[(Option<PortId>, ChipPort)],
        options: Options,
        runner: &mut dyn SeqRunner,
    ) -> Result<Self> {
        let mut m = Self::without_table(options);
        m.loaded = true;
        m.counters = table.counters;
        m.ifg_bits = table.ifg_bits;
        let mut engines = table.engines.clone();
        engines.sort_by_key(|e| e.lane); // the C++ walks a map keyed by lane
        for e in engines {
            let Some((id, p)) = ports.iter().rfind(|(_, p)| p.lanes.first() == Some(&e.lane)) else {
                m.log(
                    Level::Error,
                    format_args!("external MACsec table: no port starts at lane {}", e.lane),
                );
                return Err(Error::InvalidParameter);
            };
            let port = EnginePort {
                phy: e.phy,
                name: p.name.clone(),
                port: *id,
                egress: Engine::default(),
                ingress: Engine::default(),
            };
            m.ports.insert(p.key, port);
        }
        let mut left = 0u32;
        let keys: Vec<PortKey> = m.ports.keys().copied().collect();
        for key in keys {
            for dir in Dir::BOTH {
                let probe = seq::probe(dir);
                if !runner.has(&probe) {
                    continue;
                }
                // a failed probe is logged; the port's MACsec calls will fail too
                let Ok(res) = m.run(runner, key, dir, &probe, &Params::new(), false) else {
                    continue;
                };
                if res.u64("bypass") != 0 {
                    continue;
                }
                left += 1;
                let _ = m.run(runner, key, dir, &seq::reset(dir), &Params::new(), true);
                let _ = m.run(runner, key, dir, &seq::disable(dir), &Params::new(), false);
            }
        }
        if left > 0 {
            m.log(
                Level::Notice,
                format_args!(
                    "external MACsec table: {left} engines were left on by a previous process: reset and back in bypass"
                ),
            );
        }
        let (n_ports, n_seqs, ifg) = (m.ports.len(), runner.sequence_names().len(), m.ifg_bits);
        let mode = match m.counters {
            CounterMode::ClearOnRead => "clear on read",
            CounterMode::Absolute => "absolute",
        };
        m.log(
            Level::Notice,
            format_args!(
                "external MACsec table: {n_ports} ports, {n_seqs} sequences, counters {mode}, inter-frame gap {ifg} bit times with MACsec on"
            ),
        );
        Ok(m)
    }

    /// Forgets everything, wiping the keys of SAs not yet installed (`ext_macsec_detach`,
    /// at switch removal). Runs nothing: engines that are on stay on (crate documentation, Q2).
    pub fn detach(&mut self) {
        let log = self.log.take();
        let alloc = self.ext_alloc.take();
        let auth_key = self.auth_key;
        // dropping the SAs wipes their parameters
        *self = Self::without_table(Options { auth_key, log, alloc });
    }

    /// Whether a table is attached (`ext_macsec_active`).
    pub fn active(&self) -> bool {
        self.loaded
    }

    pub(crate) fn log(&mut self, level: Level, text: fmt::Arguments<'_>) {
        if let Some(log) = self.log.as_mut() {
            log(level, &text.to_string());
        }
    }

    pub(crate) fn need_table(&self) -> Result<()> {
        if self.loaded { Ok(()) } else { Err(Error::NotSupported) }
    }

    pub(crate) fn engine(&self, hw: PortKey, dir: Dir) -> Option<&Engine> {
        self.ports.get(&hw).map(|p| p.engine(dir))
    }

    pub(crate) fn engine_mut(&mut self, hw: PortKey, dir: Dir) -> Option<&mut Engine> {
        self.ports.get_mut(&hw).map(|p| p.engine_mut(dir))
    }

    fn port_name(&self, hw: PortKey) -> String {
        self.ports.get(&hw).map(|p| p.name.clone()).unwrap_or_default()
    }

    /// Runs one sequence on the PHY of `hw` and keeps the engine's run statistics.
    /// `missing_ok`: a sequence the table does not have is success (the optional ones).
    pub(crate) fn run(
        &mut self,
        runner: &mut dyn SeqRunner,
        hw: PortKey,
        dir: Dir,
        name: &str,
        params: &Params,
        missing_ok: bool,
    ) -> Result<Results> {
        if !self.ports.contains_key(&hw) {
            return Err(Error::NotSupported);
        }
        let port = self.port_name(hw);
        if !runner.has(name) {
            if missing_ok {
                return Ok(Results::new());
            }
            self.log(
                Level::Error,
                format_args!("port {port}: the external MACsec table has no sequence {name}"),
            );
            return Err(Error::NotSupported);
        }
        let t0 = Instant::now();
        let outcome = runner.run(hw, name, params);
        let dt = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);
        if let Some(engine) = self.engine_mut(hw, dir) {
            engine.runs += 1;
            engine.max_us = engine.max_us.max(dt);
        }
        match outcome {
            Ok(results) => {
                self.log(
                    Level::Info,
                    format_args!("port {port}: external MACsec sequence {name} done ({dt} us)"),
                );
                Ok(results)
            }
            Err(e) => {
                if let Some(engine) = self.engine_mut(hw, dir) {
                    engine.fails += 1;
                    engine.last_fail = format!("{name}: {}", e.why);
                }
                self.log(
                    Level::Error,
                    format_args!("port {port}: external MACsec sequence {name} stopped: {}", e.why),
                );
                Err(e.status)
            }
        }
    }

    /// The first of the candidates the table has, else the last (`pick`).
    fn pick(runner: &dyn SeqRunner, names: Vec<String>) -> String {
        let mut last = String::new();
        for name in names {
            if runner.has(&name) {
                return name;
            }
            last = name;
        }
        last
    }

    /// The MACsec ACL table bound to `port` for `dir`.
    pub(crate) fn table_of_port(&self, port: PortId, dir: Dir) -> Option<AclTableId> {
        self.tables
            .iter()
            .find(|(_, t)| t.port == Some(port) && t.dir == dir)
            .map(|(id, _)| *id)
    }

    /// The flow an enabled entry of the port's MACsec ACL table sends data frames to.
    pub(crate) fn bound_flow(&self, port: PortId, dir: Dir) -> Option<FlowId> {
        let table = self.tables.get(&self.table_of_port(port, dir)?)?;
        table
            .entries
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|e| e.admin && e.flow_on)
            .filter_map(|e| e.flow)
            .find(|flow| self.flows.contains_key(flow))
    }

    /// The port a flow is bound to through an enabled ACL entry (`hw_of_flow`).
    pub(crate) fn hw_of_flow(&self, flow: FlowId) -> Option<PortKey> {
        self.entries
            .values()
            .filter(|e| e.admin && e.flow_on && e.flow == Some(flow))
            .filter_map(|e| self.tables.get(&e.table)?.port)
            .find_map(|port| self.switch_ports.get(&port).copied())
    }

    fn sa_install(&mut self, runner: &mut dyn SeqRunner, hw: PortKey, id: SaId) -> Result<()> {
        let (dir, an, sc_id) = self
            .sas
            .get(&id)
            .map(|sa| (sa.dir, sa.an, sa.sc))
            .ok_or(Error::Failure)?;
        let (cipher, replay, window, sci) = self
            .scs
            .get(&sc_id)
            .map(|sc| (sc.cipher, sc.replay, sc.window, sc.sci))
            .ok_or(Error::Failure)?;
        let first = self.engine(hw, dir).ok_or(Error::Failure)?.installed_count() == 0;
        let names = seq::sa_add_candidates(dir, an, cipher, first, dir == Dir::Ingress && replay);
        let mut par = self.sas.get(&id).map(|sa| sa.par.duplicate()).ok_or(Error::Failure)?;
        par.set(param::SCI, &sci);
        par.set(param::WINDOW, &(if replay { window } else { 0 }).to_le_bytes());
        let name = Self::pick(runner, names);
        let outcome = self.run(runner, hw, dir, &name, &par, false);
        drop(par); // wiped
        outcome?;
        if let Some(sa) = self.sas.get_mut(&id) {
            sa.par.wipe(); // the PHY has the key now
            sa.installed = true;
        }
        if let Some(engine) = self.engine_mut(hw, dir) {
            engine.set_slot(an, Some(id));
            if dir == Dir::Egress {
                engine.tx = Some(id);
            }
        }
        if let Some(sc) = self.scs.get_mut(&sc_id) {
            sc.hw = Some(hw);
        }
        let port = self.port_name(hw);
        self.log(
            Level::Notice,
            format_args!(
                "port {port}: {} SA AN {} installed ({}{})",
                dir.name(),
                an.get(),
                cipher.name(),
                if first { ", first of its SC" } else { "" }
            ),
        );
        Ok(())
    }

    /// Opens or closes the gate (data frames through the SC / dropped). A table without
    /// gate sequences: the model's gate still follows. A failed sequence: logged, the gate
    /// stays as it was.
    fn gate_set(&mut self, runner: &mut dyn SeqRunner, hw: PortKey, dir: Dir, open: bool) {
        match self.engine(hw, dir) {
            Some(engine) if engine.enabled && engine.gate != open => {}
            _ => return,
        }
        let name = if open {
            seq::gate_open(dir)
        } else {
            seq::gate_close(dir)
        };
        if self.run(runner, hw, dir, &name, &Params::new(), true).is_ok() {
            if let Some(engine) = self.engine_mut(hw, dir) {
                engine.gate = open;
            }
            let port = self.port_name(hw);
            self.log(
                Level::Notice,
                format_args!(
                    "port {port}: {} data frames {}",
                    dir.name(),
                    if open {
                        "go through the secure channel"
                    } else {
                        "are dropped"
                    }
                ),
            );
        }
    }

    /// The port whose PHY holds SA `id`. The C++ uses "the port an SA of its SC was last
    /// installed on" instead, which is another port once a flow moved (crate documentation, D1).
    pub(crate) fn holder(&self, id: SaId) -> Option<PortKey> {
        let sa = self.sas.get(&id)?;
        self.ports
            .iter()
            .find(|(_, p)| p.engine(sa.dir).slot(sa.an) == Some(id))
            .map(|(key, _)| *key)
    }

    /// Takes an SA out of the PHY. The model forgets it whether or not the sequence
    /// succeeded (crate documentation, Q1).
    pub(crate) fn sa_uninstall(&mut self, runner: &mut dyn SeqRunner, hw: PortKey, id: SaId) -> Result<()> {
        let Some((dir, an)) = self.sas.get(&id).map(|sa| (sa.dir, sa.an)) else {
            return Ok(());
        };
        let (last, transmitting) = match self.engine(hw, dir) {
            Some(engine) if engine.slot(an) == Some(id) => (engine.installed_count() == 1, engine.tx == Some(id)),
            _ => return Ok(()),
        };
        // Never transmit with, or accept through, what is about to go: close the gate
        // before the last SA and before the transmitting one is removed.
        if last || (dir == Dir::Egress && transmitting) {
            self.gate_set(runner, hw, dir, false);
        }
        let names = if last {
            vec![seq::sa_del_last(dir, an), seq::sa_del(dir, an)]
        } else {
            vec![seq::sa_del(dir, an)]
        };
        let name = Self::pick(runner, names);
        let outcome = self.run(runner, hw, dir, &name, &Params::new(), false);
        if let Some(engine) = self.engine_mut(hw, dir) {
            engine.set_slot(an, None); // gone from the model either way
            if engine.tx == Some(id) {
                engine.tx = None;
            }
        }
        if let Some(sa) = self.sas.get_mut(&id) {
            sa.installed = false;
        }
        outcome.map(|_| ())
    }

    /// Brings the PHY of (`hw`, `dir`) to what the objects say: installs the pending SAs
    /// of the SC on the bound flow, oldest first, then sets the gate. `only`: report the
    /// status of installing this SA (`create_sa`); everything else is logged.
    pub(crate) fn reconcile(
        &mut self,
        runner: &mut dyn SeqRunner,
        hw: PortKey,
        dir: Dir,
        only: Option<SaId>,
    ) -> Result<()> {
        let Some(port) = self.ports.get(&hw) else {
            return Ok(());
        };
        if !port.engine(dir).enabled {
            return Ok(());
        }
        let (port_id, port_name) = (port.port, port.name.clone());
        let flow = port_id.and_then(|p| self.bound_flow(p, dir));
        let mut pending: Vec<(u64, SaId)> = flow.map_or_else(Vec::new, |flow| {
            self.sas
                .iter()
                .filter(|(_, sa)| sa.dir == dir && !sa.installed)
                .filter(|(_, sa)| self.scs.get(&sa.sc).is_some_and(|sc| sc.flow == flow))
                .map(|(id, sa)| (sa.order, *id))
                .collect()
        });
        pending.sort_unstable();
        let mut result = Ok(());
        for (_, id) in pending {
            let Some(an) = self.sas.get(&id).map(|sa| sa.an) else {
                continue;
            };
            let holder = self.engine(hw, dir).and_then(|e| e.slot(an));
            if holder.is_some_and(|other| other != id) {
                // the AN is held by an SA of another SC: one SC per port
                self.log(
                    Level::Error,
                    format_args!(
                        "port {port_name}: {} AN {} is in use by another SA (one secure channel per port and direction)",
                        dir.name(),
                        an.get()
                    ),
                );
                if only == Some(id) {
                    result = Err(Error::InsufficientResources);
                }
                continue;
            }
            let installed = self.sa_install(runner, hw, id);
            if only == Some(id) {
                result = installed;
            }
        }
        let want = flow.is_some()
            && self
                .engine(hw, dir)
                .is_some_and(|e| e.installed_count() > 0 && (dir == Dir::Ingress || e.tx.is_some()));
        self.gate_set(runner, hw, dir, want);
        result
    }

    pub(crate) fn reconcile_port(&mut self, runner: &mut dyn SeqRunner, port: PortId, dir: Dir) {
        if let Some(hw) = self.switch_ports.get(&port).copied() {
            let _ = self.reconcile(runner, hw, dir, None);
        }
    }

    /// Reads the counters of sequence `name` (`poll_counters`): `None` when the table
    /// does not have it.
    pub(crate) fn poll(
        &mut self,
        runner: &mut dyn SeqRunner,
        hw: PortKey,
        dir: Dir,
        name: &str,
    ) -> Result<Option<Results>> {
        if !runner.has(name) {
            return Ok(None);
        }
        self.run(runner, hw, dir, name, &Params::new(), false).map(Some)
    }

    /// Adds what was read to `acc`: accumulating when the PHY clears on read, replacing
    /// when its counters are absolute.
    pub(crate) fn accumulate(mode: CounterMode, acc: &mut Counters, read: &Results) {
        for name in read.names() {
            let value = read.u64(name);
            let slot = acc.entry(name.to_owned()).or_insert(0);
            *slot = match mode {
                CounterMode::ClearOnRead => slot.wrapping_add(value),
                CounterMode::Absolute => value,
            };
        }
    }

    /// Average inter-frame gap (bit times) the port needs while it has a MACSEC_PORT
    /// object; 0 = none (`macsec_port_ifg`). The port code asks this whenever it re-applies
    /// the port's settings (a speed set), so the gap survives that.
    pub fn port_ifg(&self, hw: PortKey) -> u32 {
        if self.loaded && self.ifg_bits != 0 && self.mports.values().any(|m| m.hw == hw) {
            self.ifg_bits
        } else {
            0
        }
    }
}

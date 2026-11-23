//! The diag texts (`openbcm-diag extmacsec`, `extmacsec run ...`), a read-only view of the
//! PHY state for tests and tools, and the model's invariants as a checkable property.
use crate::ids::{An, Dir, PortKey, SaId};
use crate::model::Macsec;
use crate::runner::SeqRunner;
use crate::secret::Params;
use std::fmt::Write as _;
use std::time::Instant;

/// What the model believes the PHY of one port holds, one direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineState {
    /// Out of bypass (a MACSEC_PORT exists).
    pub enabled: bool,
    /// Data frames go through the SC.
    pub gate: bool,
    /// The installed SA per AN.
    pub sa: [Option<SaId>; 4],
    /// Egress: the transmitting SA.
    pub tx: Option<SaId>,
    /// Sequences run.
    pub runs: u64,
    /// Sequences that failed.
    pub fails: u64,
    /// The last failure: `<sequence>: <why>`.
    pub last_fail: String,
}

/// Object counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// MACSEC objects.
    pub macsec: usize,
    /// MACSEC_PORT objects.
    pub ports: usize,
    /// MACSEC_FLOW objects.
    pub flows: usize,
    /// MACSEC_SC objects.
    pub scs: usize,
    /// MACSEC_SA objects.
    pub sas: usize,
    /// MACsec-stage ACL tables.
    pub acl_tables: usize,
    /// Their entries.
    pub acl_entries: usize,
}

impl Macsec {
    /// The state of the engine of (`port`, `dir`); `None` for a port without a MACsec PHY.
    pub fn engine_state(&self, port: PortKey, dir: Dir) -> Option<EngineState> {
        self.engine(port, dir).map(|e| EngineState {
            enabled: e.enabled,
            gate: e.gate,
            sa: An::ALL.map(|an| e.slot(an)),
            tx: e.tx,
            runs: e.runs,
            fails: e.fails,
            last_fail: e.last_fail.clone(),
        })
    }

    /// The ports with a MACsec PHY.
    pub fn engine_ports(&self) -> Vec<PortKey> {
        self.ports.keys().copied().collect()
    }

    /// How many objects of each kind exist.
    pub fn counts(&self) -> Counts {
        Counts {
            macsec: self.objs.len(),
            ports: self.mports.len(),
            flows: self.flows.len(),
            scs: self.scs.len(),
            sas: self.sas.len(),
            acl_tables: self.tables.len(),
            acl_entries: self.entries.len(),
        }
    }

    /// Whether the model still holds the key of SA `id` (it does until the SA is
    /// installed); `None` when there is no such SA.
    pub fn sa_holds_key(&self, id: SaId) -> Option<bool> {
        self.sas
            .get(&id)
            .map(|sa| sa.par.get(crate::secret::param::KEY).is_some())
    }

    /// Whether SA `id` is in a PHY; `None` when there is no such SA.
    pub fn sa_installed(&self, id: SaId) -> Option<bool> {
        self.sas.get(&id).map(|sa| sa.installed)
    }

    /// The text of `openbcm-diag extmacsec` (`ext_macsec_status`), format unchanged.
    pub fn status(&self, runner: &dyn SeqRunner) -> String {
        if !self.loaded {
            return "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n".to_owned();
        }
        let c = self.counts();
        let mut out = format!(
            "extmacsec: {} ports, {} sequences; objects: {} macsec, {} ports, {} flows, {} SCs, {} SAs, {} ACL tables, {} ACL entries\n",
            self.ports.len(),
            runner.sequence_names().len(),
            c.macsec,
            c.ports,
            c.flows,
            c.scs,
            c.sas,
            c.acl_tables,
            c.acl_entries
        );
        out.push_str("port      phy  dir     engine gate   AN0 AN1 AN2 AN3 tx   runs fails  max_us last failure\n");
        let mut bypass = 0usize;
        for port in self.ports.values() {
            for dir in Dir::BOTH {
                let e = port.engine(dir);
                bypass += usize::from(!e.enabled);
                if !e.enabled && e.fails == 0 {
                    continue; // engines in bypass that never failed: nothing to say
                }
                let [a0, a1, a2, a3] = An::ALL.map(|an| if e.slot(an).is_some() { "sa" } else { "-" });
                let tx = An::ALL
                    .iter()
                    .rfind(|an| e.tx.is_some() && e.slot(**an) == e.tx)
                    .map_or_else(|| "-".to_owned(), |an| an.get().to_string());
                let gate = match (e.enabled, e.gate) {
                    (false, _) => "-",
                    (true, true) => "open",
                    (true, false) => "closed",
                };
                // writing to a String cannot fail
                let _ = writeln!(
                    out,
                    "{:<8} {:>4x}  {:<7} {:<6} {:<6} {a0:<3} {a1:<3} {a2:<3} {a3:<3} {:<3} {:>5} {:>5} {:>7} {}",
                    port.name,
                    port.phy,
                    dir.name(),
                    if e.enabled { "on" } else { "bypass" },
                    gate,
                    if dir == Dir::Egress { tx.as_str() } else { "" },
                    e.runs,
                    e.fails,
                    e.max_us,
                    e.last_fail
                );
            }
        }
        let _ = writeln!(out, "engines in bypass: {bypass} of {}", self.ports.len() * 2);
        let pending = self.sas.values().filter(|sa| !sa.installed).count();
        if pending > 0 {
            let _ = writeln!(out, "SAs waiting for their flow to be bound to a port: {pending}");
        }
        out
    }

    /// One sequence by hand, for bring-up (`ext_macsec_diag_run`): the port by its name, the
    /// direction by the sequence name (`.ingress` in it, else egress). The C++ reads the
    /// parameters from a root-only file; here the caller passes them (and owns that check).
    pub fn diag_run(&mut self, runner: &mut dyn SeqRunner, port: &str, name: &str, params: &Params) -> String {
        if !self.loaded {
            return "extmacsec: no external MACsec table\n".to_owned();
        }
        let Some(hw) = self.ports.iter().rfind(|(_, p)| p.name == port).map(|(key, _)| *key) else {
            return format!("extmacsec: no such port in the table: {port}\n");
        };
        let dir = if name.contains(".ingress") {
            Dir::Ingress
        } else {
            Dir::Egress
        };
        let t0 = Instant::now();
        let outcome = self.run(runner, hw, dir, name, params, false);
        let us = t0.elapsed().as_micros();
        let mut out = format!("extmacsec: {port} {name}: ");
        match &outcome {
            Ok(_) => out.push_str("ok"),
            Err(e) => {
                let why = self.engine(hw, dir).map(|e| e.last_fail.clone()).unwrap_or_default();
                let _ = write!(out, "FAILED ({}): {why}", e.status());
            }
        }
        let _ = writeln!(out, ", {us} us");
        if let Ok(results) = outcome {
            for result in results.names() {
                let _ = writeln!(out, "  {result} {}", results.u64(result));
            }
        }
        out
    }

    /// Checks the model's rules from scratch; `Err` describes the first violation. The
    /// property tests call this after every operation.
    ///
    /// - every SA has its SC, every SC its flow; one SC per flow; (SC, AN) is unique
    /// - a slot of an engine names an existing, installed SA of that direction and AN, and
    ///   every installed SA sits in exactly one slot; an installed SA holds no key
    /// - an engine in bypass holds nothing and its gate is closed; an engine is on exactly
    ///   when a MACSEC_PORT exists for it; the transmitting SA is one of the installed ones
    ///   and only an egress engine has one
    /// - an ACL entry's table exists and lists it once; an enabled flow action names an
    ///   existing flow of the table's direction; at most one table per port and direction
    /// - at most one MACSEC object per direction and one MACSEC_PORT per port and direction
    pub fn check_invariants(&self) -> Result<(), String> {
        for (id, sa) in &self.sas {
            let sc = self.scs.get(&sa.sc).ok_or(format!("{id:?} has no SC"))?;
            if sc.dir != sa.dir {
                return Err(format!("{id:?} and its SC differ in direction"));
            }
            if self.sas.values().filter(|o| o.sc == sa.sc && o.an == sa.an).count() != 1 {
                return Err(format!("{id:?}: its (SC, AN) is not unique"));
            }
            let slots = self
                .ports
                .values()
                .flat_map(|p| Dir::BOTH.map(|d| (d, p.engine(d))))
                .filter(|(d, e)| *d == sa.dir && e.slot(sa.an) == Some(*id))
                .count();
            if slots != usize::from(sa.installed) {
                return Err(format!("{id:?}: installed = {}, in {slots} slots", sa.installed));
            }
            if sa.installed && !sa.par.is_empty() {
                return Err(format!("{id:?} is installed and still holds parameters"));
            }
        }
        for (id, sc) in &self.scs {
            let flow = self.flows.get(&sc.flow).ok_or(format!("{id:?} has no flow"))?;
            if flow.dir != sc.dir || self.scs.values().filter(|o| o.flow == sc.flow).count() != 1 {
                return Err(format!("{id:?}: flow of another direction, or shared"));
            }
        }
        for (key, port) in &self.ports {
            for dir in Dir::BOTH {
                let e = port.engine(dir);
                let has_port = self.mports.values().filter(|m| m.hw == *key && m.dir == dir).count();
                if has_port > 1 || (has_port == 1) != e.enabled {
                    return Err(format!(
                        "{key:?} {dir:?}: enabled = {}, {has_port} MACSEC_PORTs",
                        e.enabled
                    ));
                }
                if !e.enabled && (e.gate || e.installed_count() > 0 || e.tx.is_some()) {
                    return Err(format!("{key:?} {dir:?}: in bypass and not empty"));
                }
                for an in An::ALL {
                    if let Some(id) = e.slot(an) {
                        let ok = self
                            .sas
                            .get(&id)
                            .is_some_and(|sa| sa.installed && sa.an == an && sa.dir == dir);
                        if !ok {
                            return Err(format!("{key:?} {dir:?} AN {}: slot names {id:?}", an.get()));
                        }
                    }
                }
                if let Some(tx) = e.tx {
                    if dir != Dir::Egress || !An::ALL.iter().any(|an| e.slot(*an) == Some(tx)) {
                        return Err(format!("{key:?} {dir:?}: transmitting SA {tx:?} is not installed"));
                    }
                }
            }
        }
        self.check_acl_invariants()?;
        for dir in Dir::BOTH {
            if self.objs.values().filter(|d| **d == dir).count() > 1 {
                return Err(format!("two MACSEC objects for {dir:?}"));
            }
        }
        Ok(())
    }

    fn check_acl_invariants(&self) -> Result<(), String> {
        for (id, e) in &self.entries {
            let table = self.tables.get(&e.table).ok_or(format!("{id:?} has no table"))?;
            if table.entries.iter().filter(|x| *x == id).count() != 1 {
                return Err(format!("{id:?} is not listed once by its table"));
            }
            if e.flow_on != e.flow.is_some() {
                return Err(format!("{id:?}: flow action and flow disagree"));
            }
            if let Some(flow) = e.flow {
                if self.flows.get(&flow).map(|f| f.dir) != Some(table.dir) {
                    return Err(format!("{id:?}: its flow is missing or of another direction"));
                }
            }
        }
        for (id, t) in &self.tables {
            if t.entries.iter().any(|e| !self.entries.contains_key(e)) {
                return Err(format!("{id:?} lists a missing entry"));
            }
            if let Some(port) = t.port {
                if self
                    .tables
                    .values()
                    .filter(|o| o.port == Some(port) && o.dir == t.dir)
                    .count()
                    != 1
                {
                    return Err(format!("{id:?}: two tables bound to its port and direction"));
                }
            }
        }
        Ok(())
    }
}

//! The ports a change may run sequences on: what the SAI layer claims before the change runs
//! (`sai-core`'s `extphy`, "Claims"; `sai-extphy`, "SAI calls and the SAI lock"), so that a change
//! never meets a diag transaction on its PHY once it has begun, and never waits for one under the
//! SAI lock. Read from the objects as they are before the change, by the same lookups the change
//! makes; when a lookup fails the change fails before it runs anything, and the answer is
//! [`Reach::Every`] all the same (more than needed is allowed, fewer is a bug).
use crate::ids::{AclEntryId, FlowId, MacsecPortId, PortKey, SaId, ScId};
use crate::model::Macsec;
use crate::value::{Attr, find};
use sai_core::oid::PortId;
use sai_sys as sys;

/// What a change may run sequences on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// It runs none (no table sequence is part of it).
    Nothing,
    /// Sequences of this port only.
    Port(PortKey),
    /// Possibly any port's (a lookup failed).
    Every,
}

/// A change that may run sequences, by the model's ids (the others run none).
#[derive(Clone, Copy, Debug)]
pub enum Change<'a> {
    /// `create_macsec_port`
    CreateMacsecPort(&'a [Attr]),
    /// `remove_macsec_port`
    RemoveMacsecPort(MacsecPortId),
    /// `create_macsec_sa`
    CreateSa(&'a [Attr]),
    /// `remove_macsec_sa`
    RemoveSa(SaId),
    /// `clear_macsec_sa_stats`
    ClearSa(SaId),
    /// `clear_macsec_flow_stats`
    ClearFlow(FlowId),
    /// `macsec_acl_entry_create`
    CreateAclEntry(&'a [Attr]),
    /// `macsec_acl_entry_remove`
    RemoveAclEntry(AclEntryId),
    /// `macsec_acl_entry_set`
    SetAclEntry(AclEntryId),
    /// `INGRESS_MACSEC_ACL` / `EGRESS_MACSEC_ACL` of a port
    PortAcl(PortId),
}

impl Macsec {
    /// The ports `change` may run sequences on (module documentation).
    pub fn reach(&self, change: Change<'_>) -> Reach {
        let found = match change {
            Change::CreateMacsecPort(attrs) => find(attrs, sys::SAI_MACSEC_PORT_ATTR_PORT_ID)
                .and_then(|(i, a)| a.oid(i).ok())
                .and_then(|raw| PortId::try_from(raw).ok())
                .and_then(|port| self.switch_ports.get(&port).copied()),
            Change::RemoveMacsecPort(id) => self.mports.get(&id).map(|m| m.hw),
            Change::CreateSa(attrs) => {
                let sc = find(attrs, sys::SAI_MACSEC_SA_ATTR_SC_ID)
                    .and_then(|(i, a)| a.oid(i).ok())
                    .and_then(|raw| ScId::try_from(raw).ok());
                match sc.and_then(|sc| self.scs.get(&sc)) {
                    // installed now only when the SC's flow is bound to a port (`create_sa`)
                    Some(sc) => return self.hw_of_flow(sc.flow).map_or(Reach::Nothing, Reach::Port),
                    None => None,
                }
            }
            Change::RemoveSa(id) | Change::ClearSa(id) => {
                let Some(sa) = self.sas.get(&id) else {
                    return Reach::Every;
                };
                // `remove_sa`: the holder, else the port its SC was last installed on; a clear
                // reads the holder only
                match self.holder(id).or_else(|| self.scs.get(&sa.sc).and_then(|sc| sc.hw)) {
                    Some(hw) => Some(hw),
                    None => return Reach::Nothing,
                }
            }
            Change::ClearFlow(id) => {
                if !self.flows.contains_key(&id) {
                    return Reach::Every;
                }
                // `get_flow_stats`: the port it is bound to, else its SC's last port
                let hw = self.hw_of_flow(id).or_else(|| {
                    self.scs
                        .values()
                        .filter(|sc| sc.flow == id)
                        .filter_map(|sc| sc.hw)
                        .next_back()
                });
                return hw.map_or(Reach::Nothing, Reach::Port);
            }
            Change::CreateAclEntry(attrs) => {
                let table = find(attrs, sys::SAI_ACL_ENTRY_ATTR_TABLE_ID)
                    .and_then(|(i, a)| a.oid(i).ok())
                    .and_then(|raw| self.acl_table(raw))
                    .and_then(|t| self.tables.get(&t));
                match table {
                    Some(t) => return self.reach_of_port(t.port),
                    None => None,
                }
            }
            Change::RemoveAclEntry(id) | Change::SetAclEntry(id) => {
                match self.entries.get(&id).and_then(|e| self.tables.get(&e.table)) {
                    Some(t) => return self.reach_of_port(t.port),
                    None => None,
                }
            }
            Change::PortAcl(port) => return self.reach_of_port(Some(port)),
        };
        found.map_or(Reach::Every, Reach::Port)
    }

    /// A table's port (`reconcile_port` runs on it); unbound: nothing.
    fn reach_of_port(&self, port: Option<PortId>) -> Reach {
        match port {
            None => Reach::Nothing,
            Some(port) => self
                .switch_ports
                .get(&port)
                .copied()
                .map_or(Reach::Nothing, Reach::Port),
        }
    }
}

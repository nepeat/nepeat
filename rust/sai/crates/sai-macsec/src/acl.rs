//! ACL tables of the MACsec stages and the port attributes that bind them (the glue the C++
//! calls from `sai_acl.cpp` and `sai_port.cpp`). These tables are a model of the PHY's fixed
//! classification and never reach the ASIC; their entries are what ties a MACSEC_FLOW to a
//! port: an enabled entry with `ACTION_MACSEC_FLOW` opens the gate.
use crate::ids::{AclEntryId, AclTableId, Dir, FlowId, MacsecPortId, en};
use crate::model::{Level, Macsec, MsAclEntry, MsAclTable};
use crate::runner::{SeqRunner, seq};
use crate::stats;
use crate::value::{AclParameter, Attr, Value, find, invalid, not_supported};
use sai_core::oid::{PortId, RawOid};
use sai_core::{Error, Result};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};

const TABLE_FIELDS: core::ops::RangeInclusive<sys::sai_attr_id_t> =
    sys::SAI_ACL_TABLE_ATTR_FIELD_START..=sys::SAI_ACL_TABLE_ATTR_FIELD_END;
const ENTRY_FIELDS: core::ops::RangeInclusive<sys::sai_attr_id_t> =
    sys::SAI_ACL_ENTRY_ATTR_FIELD_START..=sys::SAI_ACL_ENTRY_ATTR_FIELD_END;

fn stage_dir(stage: i32) -> Dir {
    if stage == en(sys::SAI_ACL_STAGE_INGRESS_MACSEC) {
        Dir::Ingress
    } else {
        Dir::Egress
    }
}

impl Macsec {
    /// Whether an ACL table create names a MACsec stage, and so belongs here
    /// (`macsec_acl_stage`).
    pub fn is_macsec_stage(attrs: &[Attr]) -> bool {
        find(attrs, sys::SAI_ACL_TABLE_ATTR_ACL_STAGE).is_some_and(|(_, a)| {
            matches!(a.value, Value::Enum(v)
                if v == en(sys::SAI_ACL_STAGE_INGRESS_MACSEC) || v == en(sys::SAI_ACL_STAGE_EGRESS_MACSEC))
        })
    }

    /// The MACsec-stage table with this id, if it is one of this model's (`macsec_acl_table`).
    pub fn acl_table(&self, raw: RawOid) -> Option<AclTableId> {
        AclTableId::try_from(raw).ok().filter(|id| self.tables.contains_key(id))
    }

    /// The entry with this id, if it is an entry of a MACsec-stage table (`macsec_acl_entry`).
    pub fn acl_entry(&self, raw: RawOid) -> Option<AclEntryId> {
        AclEntryId::try_from(raw)
            .ok()
            .filter(|id| self.entries.contains_key(id))
    }

    /// `macsec_acl_table_create`. Accepted fields are what the PHY's fixed classification
    /// can stand for: EAPOL by `EtherType` (the DMAC is not looked at), the SC by its SCI.
    pub fn create_acl_table(&mut self, attrs: &[Attr]) -> Result<AclTableId> {
        self.need_table()?;
        let mut dir = Dir::Egress;
        let mut fields = BTreeSet::new();
        for (i, a) in attrs.iter().enumerate() {
            match a.id {
                sys::SAI_ACL_TABLE_ATTR_ACL_STAGE => dir = stage_dir(a.en(i)?),
                sys::SAI_ACL_TABLE_ATTR_ACL_BIND_POINT_TYPE_LIST => {
                    let Value::I32List(list) = &a.value else {
                        return Err(invalid(i));
                    };
                    if list.iter().any(|b| *b != en(sys::SAI_ACL_BIND_POINT_TYPE_PORT)) {
                        return Err(not_supported(i));
                    }
                }
                sys::SAI_ACL_TABLE_ATTR_ACL_ACTION_TYPE_LIST | sys::SAI_ACL_TABLE_ATTR_SIZE => {}
                sys::SAI_ACL_TABLE_ATTR_FIELD_DST_MAC
                | sys::SAI_ACL_TABLE_ATTR_FIELD_ETHER_TYPE
                | sys::SAI_ACL_TABLE_ATTR_FIELD_MACSEC_SCI => {
                    if a.bool(i)? {
                        fields.insert(a.id);
                    }
                }
                // any other field may be named as long as it is off
                id if TABLE_FIELDS.contains(&id) && !a.bool(i)? => {}
                _ => return Err(not_supported(i)),
            }
        }
        let id = AclTableId::from_index(self.new_index(AclTableId::SAI_TYPE)?);
        let table = MsAclTable {
            dir,
            port: None,
            fields,
            entries: Vec::new(),
        };
        self.tables.insert(id, table);
        Ok(id)
    }

    /// `macsec_acl_table_remove`: refused while it has entries or is bound to a port.
    pub fn remove_acl_table(&mut self, id: AclTableId) -> Result<()> {
        let table = self.tables.get(&id).ok_or(Error::InvalidObjectId)?;
        if !table.entries.is_empty() || table.port.is_some() {
            return Err(Error::ObjectInUse);
        }
        self.tables.remove(&id);
        Ok(())
    }

    /// `macsec_acl_table_get`
    pub fn get_acl_table(&self, id: AclTableId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let table = self.tables.get(&id).ok_or(Error::InvalidObjectId)?;
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_ACL_TABLE_ATTR_ACL_STAGE => Value::Enum(en(match table.dir {
                        Dir::Ingress => sys::SAI_ACL_STAGE_INGRESS_MACSEC,
                        Dir::Egress => sys::SAI_ACL_STAGE_EGRESS_MACSEC,
                    })),
                    id if TABLE_FIELDS.contains(&id) => Value::Bool(table.fields.contains(&id)),
                    sys::SAI_ACL_TABLE_ATTR_ENTRY_LIST => {
                        Value::OidList(table.entries.iter().map(|e| e.raw()).collect())
                    }
                    sys::SAI_ACL_TABLE_ATTR_AVAILABLE_ACL_ENTRY => Value::U32(16),
                    _ => return Err(not_supported(i)),
                })
            })
            .collect()
    }

    /// Applies one attribute to an entry of a table of direction `dir` (`entry_attr`).
    fn entry_attr(&self, e: &mut MsAclEntry, dir: Dir, a: &Attr, i: usize) -> Result<()> {
        match a.id {
            sys::SAI_ACL_ENTRY_ATTR_PRIORITY => e.priority = a.u32(i)?,
            sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE => e.admin = a.bool(i)?,
            sys::SAI_ACL_ENTRY_ATTR_FIELD_DST_MAC
            | sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE
            | sys::SAI_ACL_ENTRY_ATTR_FIELD_MACSEC_SCI => match a.value {
                Value::AclField(Some(field)) => {
                    e.fields.insert(a.id, field);
                }
                Value::AclField(None) => {
                    e.fields.remove(&a.id);
                }
                _ => return Err(invalid(i)),
            },
            sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION => {
                let Value::AclAction {
                    enable,
                    parameter: AclParameter::S32(action),
                } = a.value
                else {
                    return Err(invalid(i));
                };
                let known = action == en(sys::SAI_PACKET_ACTION_FORWARD) || action == en(sys::SAI_PACKET_ACTION_DROP);
                if enable && !known {
                    return Err(not_supported(i));
                }
                e.action_on = enable;
                e.action = action;
            }
            sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW => {
                let Value::AclAction { enable, parameter } = a.value else {
                    return Err(invalid(i));
                };
                let flow = match parameter {
                    AclParameter::Oid(raw) if enable => {
                        let flow = FlowId::try_from(raw).map_err(|_| invalid(i))?;
                        if self.flows.get(&flow).map(|f| f.dir) != Some(dir) {
                            return Err(invalid(i));
                        }
                        Some(flow)
                    }
                    _ if enable => return Err(invalid(i)),
                    _ => None,
                };
                e.flow_on = enable;
                e.flow = flow;
            }
            _ => return Err(not_supported(i)),
        }
        Ok(())
    }

    /// Entries are a model of the PHY's fixed rules; say so when one cannot be honoured
    /// (`entry_note`).
    fn entry_note(&mut self, runner: &dyn SeqRunner, e: &MsAclEntry, dir: Dir) {
        let eapol = e.fields.contains_key(&sys::SAI_ACL_ENTRY_ATTR_FIELD_ETHER_TYPE);
        if eapol && e.action_on && e.action == en(sys::SAI_PACKET_ACTION_DROP) {
            self.log(
                Level::Warn,
                format_args!(
                    "MACsec ACL: an entry that drops an EtherType is not programmed (the PHY passes EAPOL in clear, always)"
                ),
            );
        }
        let forwards = e.admin && e.action_on && e.action == en(sys::SAI_PACKET_ACTION_FORWARD) && !e.flow_on;
        if !eapol && forwards && runner.has(&seq::gate_close(dir)) {
            self.log(
                Level::Warn,
                format_args!(
                    "MACsec ACL: a data entry that forwards in clear is not programmed: data frames are dropped until a secure channel carries them"
                ),
            );
        }
    }

    /// `macsec_acl_entry_create`. `TABLE_ID` must name a table of this model (the caller
    /// routes by [`Macsec::acl_table`]).
    pub fn create_acl_entry(&mut self, runner: &mut dyn SeqRunner, attrs: &[Attr]) -> Result<AclEntryId> {
        let (ti, ta) = find(attrs, sys::SAI_ACL_ENTRY_ATTR_TABLE_ID).ok_or(Error::MandatoryAttributeMissing)?;
        let table = self.acl_table(ta.oid(ti)?).ok_or(invalid(ti))?;
        let (dir, port) = self.tables.get(&table).map(|t| (t.dir, t.port)).ok_or(invalid(ti))?;
        let mut entry = MsAclEntry {
            table,
            priority: 0,
            admin: true,
            flow_on: false,
            flow: None,
            action_on: false,
            action: en(sys::SAI_PACKET_ACTION_FORWARD),
            fields: BTreeMap::new(),
        };
        for (i, a) in attrs.iter().enumerate() {
            if a.id != sys::SAI_ACL_ENTRY_ATTR_TABLE_ID {
                self.entry_attr(&mut entry, dir, a, i)?;
            }
        }
        self.entry_note(runner, &entry, dir);
        let id = AclEntryId::from_index(self.new_index(AclEntryId::SAI_TYPE)?);
        self.entries.insert(id, entry);
        if let Some(t) = self.tables.get_mut(&table) {
            t.entries.push(id);
        }
        if let Some(port) = port {
            self.reconcile_port(runner, port, dir);
        }
        Ok(id)
    }

    /// `macsec_acl_entry_remove`
    pub fn remove_acl_entry(&mut self, runner: &mut dyn SeqRunner, id: AclEntryId) -> Result<()> {
        let entry = self.entries.remove(&id).ok_or(Error::InvalidObjectId)?;
        if let Some(t) = self.tables.get_mut(&entry.table) {
            t.entries.retain(|e| *e != id);
            if let (Some(port), dir) = (t.port, t.dir) {
                self.reconcile_port(runner, port, dir);
            }
        }
        Ok(())
    }

    /// `macsec_acl_entry_set` (with `sai_acl.cpp`'s refusal to move an entry to another table).
    pub fn set_acl_entry(&mut self, runner: &mut dyn SeqRunner, id: AclEntryId, attr: &Attr) -> Result<()> {
        let mut changed = self.entries.get(&id).ok_or(Error::InvalidObjectId)?.clone();
        if attr.id == sys::SAI_ACL_ENTRY_ATTR_TABLE_ID {
            return Err(invalid(0));
        }
        let (dir, port) = self
            .tables
            .get(&changed.table)
            .map(|t| (t.dir, t.port))
            .ok_or(Error::Failure)?;
        self.entry_attr(&mut changed, dir, attr, 0)?;
        self.entry_note(runner, &changed, dir);
        self.entries.insert(id, changed);
        if let Some(port) = port {
            self.reconcile_port(runner, port, dir);
        }
        Ok(())
    }

    /// `macsec_acl_entry_get`
    pub fn get_acl_entry(&self, id: AclEntryId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let e = self.entries.get(&id).ok_or(Error::InvalidObjectId)?;
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_ACL_ENTRY_ATTR_TABLE_ID => Value::Oid(e.table.raw()),
                    sys::SAI_ACL_ENTRY_ATTR_PRIORITY => Value::U32(e.priority),
                    sys::SAI_ACL_ENTRY_ATTR_ADMIN_STATE => Value::Bool(e.admin),
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_PACKET_ACTION => Value::AclAction {
                        enable: e.action_on,
                        parameter: AclParameter::S32(e.action),
                    },
                    sys::SAI_ACL_ENTRY_ATTR_ACTION_MACSEC_FLOW => Value::AclAction {
                        enable: e.flow_on,
                        parameter: AclParameter::Oid(e.flow.map_or(RawOid::NULL, FlowId::raw)),
                    },
                    id => match e.fields.get(&id) {
                        Some(field) => Value::AclField(Some(*field)),
                        None if ENTRY_FIELDS.contains(&id) => Value::AclField(None),
                        None => return Err(not_supported(i)),
                    },
                })
            })
            .collect()
    }

    /// `SAI_PORT_ATTR_INGRESS_MACSEC_ACL` / `EGRESS_MACSEC_ACL` set on `port`
    /// (`port_macsec_set`): binds the table (null: unbinds), then reconciles the port's PHY.
    /// Attribute errors are for index 0; the caller re-indexes (`Error::at_index`).
    pub fn set_port_acl(&mut self, runner: &mut dyn SeqRunner, port: PortId, dir: Dir, table: RawOid) -> Result<()> {
        let hw = self.switch_ports.get(&port).copied();
        let old = self.table_of_port(port, dir);
        if table.is_null() {
            if let Some(t) = old.and_then(|old| self.tables.get_mut(&old)) {
                t.port = None;
            }
        } else {
            let new = self.acl_table(table).ok_or(invalid(0))?;
            let ok = self
                .tables
                .get(&new)
                .is_some_and(|t| t.dir == dir && t.port.is_none_or(|p| p == port));
            if !ok {
                return Err(invalid(0));
            }
            if !hw.is_some_and(|hw| self.ports.contains_key(&hw)) {
                return Err(not_supported(0));
            }
            if let Some(t) = old.filter(|old| *old != new).and_then(|old| self.tables.get_mut(&old)) {
                t.port = None;
            }
            if let Some(t) = self.tables.get_mut(&new) {
                t.port = Some(port);
            }
        }
        if let Some(hw) = hw {
            let _ = self.reconcile(runner, hw, dir, None);
        }
        Ok(())
    }

    /// The MACsec ACL table bound to `port` for `dir`, null when none (`port_macsec_get`).
    pub fn port_acl(&self, port: PortId, dir: Dir) -> RawOid {
        self.table_of_port(port, dir).map_or(RawOid::NULL, AclTableId::raw)
    }

    /// `SAI_PORT_ATTR_MACSEC_PORT_LIST` of `port`.
    pub fn port_macsec_ports(&self, port: PortId) -> Vec<MacsecPortId> {
        self.mports
            .iter()
            .filter(|(_, m)| m.port == port)
            .map(|(id, _)| *id)
            .collect()
    }

    /// `sai_query_stats_capability` for MACSEC_SA and MACSEC_FLOW: the statistics some
    /// `stats-*` sequence of the board table reads, each in mode `SAI_STATS_MODE_READ`.
    /// `NOT_SUPPORTED` for another object type.
    pub fn stats_capability(
        &self,
        runner: &dyn SeqRunner,
        object_type: sys::sai_object_type_t,
    ) -> Result<Vec<sys::sai_stat_id_t>> {
        let table = match object_type {
            sys::SAI_OBJECT_TYPE_MACSEC_SA => stats::SA_STATS,
            sys::SAI_OBJECT_TYPE_MACSEC_FLOW => stats::FLOW_STATS,
            _ => return Err(Error::NotSupported),
        };
        let read: BTreeSet<String> = runner
            .sequence_names()
            .iter()
            .filter(|name| name.starts_with("stats-"))
            .flat_map(|name| runner.result_names(name))
            .collect();
        Ok(table
            .iter()
            .filter(|(_, name)| read.contains(*name))
            .map(|(id, _)| *id)
            .collect())
    }
}

//! The entry points of `sai_macsec_api_t`: MACSEC, MACSEC_PORT, MACSEC_FLOW, MACSEC_SC,
//! MACSEC_SA. Attribute rules and statuses are the C++'s, check by check, in its order.
use crate::hkey::{AuthKeyPolicy, auth_key_matches};
use crate::ids::{An, Cipher, Dir, FlowId, MacsecId, MacsecPortId, SaId, ScId, en};
use crate::model::{Level, Macsec, MsFlow, MsPort, MsSa, MsSc};
use crate::runner::{SeqRunner, seq};
use crate::secret::{Params, Results, Secret, param};
use crate::stats::{self, CounterMode, Counters};
use crate::value::{Attr, Value, find, invalid, not_supported, sci_from_attr, sci_to_attr};
use sai_core::oid::{PortId, RawOid, TypedId};
use sai_core::{Error, Result};
use sai_sys as sys;
use zeroize::Zeroizing;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn dir_value(dir: Dir) -> Value {
    Value::Enum(dir.sai())
}

/// `*_stats_ext`: only the plain read mode exists.
fn read_mode(mode: sys::sai_stats_mode_t) -> Result<()> {
    if mode == sys::SAI_STATS_MODE_READ {
        Ok(())
    } else {
        Err(Error::NotSupported)
    }
}

/// What a `create_sa` call carries, in the C++'s terms.
struct SaArgs<'a> {
    dir: i32,
    sc: RawOid,
    an: An,
    sak: &'a Secret<32>,
    auth: (usize, &'a Secret<16>),
    salt: Option<&'a Secret<12>>,
    ssci: Option<u32>,
    pn: u64,
}

impl<'a> SaArgs<'a> {
    fn parse(attrs: &'a [Attr]) -> Result<Self> {
        let (mut dir, mut sc, mut an, mut sak, mut auth, mut salt, mut ssci) =
            (None, None, None, None, None, None, None);
        let mut pn = 1;
        for (i, a) in attrs.iter().enumerate() {
            match a.id {
                sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION => dir = Some(a.en(i)?),
                sys::SAI_MACSEC_SA_ATTR_SC_ID => sc = Some(a.oid(i)?),
                sys::SAI_MACSEC_SA_ATTR_AN => an = Some(An::new(a.u8(i)?).ok_or(invalid(i))?),
                sys::SAI_MACSEC_SA_ATTR_SAK => match &a.value {
                    Value::Sak(k) => sak = Some(k),
                    _ => return Err(invalid(i)),
                },
                sys::SAI_MACSEC_SA_ATTR_SALT => match &a.value {
                    Value::Salt(s) => salt = Some(s),
                    _ => return Err(invalid(i)),
                },
                sys::SAI_MACSEC_SA_ATTR_AUTH_KEY => match &a.value {
                    Value::AuthKey(k) => auth = Some((i, k)),
                    _ => return Err(invalid(i)),
                },
                sys::SAI_MACSEC_SA_ATTR_MACSEC_SSCI => ssci = Some(a.u32(i)?),
                sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN | sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN => {
                    pn = a.u64(i)?;
                }
                _ => return Err(not_supported(i)),
            }
        }
        match (dir, sc, an, sak, auth) {
            (Some(dir), Some(sc), Some(an), Some(sak), Some(auth)) => Ok(Self {
                dir,
                sc,
                an,
                sak,
                auth,
                salt,
                ssci,
                pn,
            }),
            _ => Err(Error::MandatoryAttributeMissing),
        }
    }

    /// The parameters of the SA's install sequence (all but `sci` and `window`, which are
    /// the SC's at the time of the install).
    fn params(&self, cipher: Cipher, policy: AuthKeyPolicy) -> Result<Params> {
        // SAI: a 128-bit SAK is the last 16 bytes of the 32
        let key = match cipher.key_len() {
            16 => self.sak.expose().get(16..),
            _ => self.sak.expose().get(..),
        }
        .ok_or(Error::Failure)?;
        if policy == AuthKeyPolicy::Verify && !auth_key_matches(key, self.auth.1.expose()) {
            return Err(invalid(self.auth.0));
        }
        let mut par = Params::new();
        par.set(param::KEY, key);
        par.set(param::HKEY, self.auth.1.expose());
        par.set(param::PN, &self.pn.to_le_bytes());
        if cipher.is_xpn() {
            let (Some(salt), Some(ssci)) = (self.salt, self.ssci) else {
                return Err(Error::MandatoryAttributeMissing);
            };
            // the SSCI as a number; on the wire most significant byte first
            let ssci = ssci.to_be_bytes();
            let mut iv = Zeroizing::new(*salt.expose());
            for (byte, s) in iv.iter_mut().zip(ssci) {
                *byte ^= s;
            }
            par.set(param::SALT, salt.expose());
            par.set(param::SSCI, &ssci);
            par.set(param::IV, iv.as_slice());
        }
        Ok(par)
    }
}

/// What the PHY sequences are recorded for: SCI in the `SecTAG`, confidentiality on, no
/// offset (`sc_fixed`). `None`: not one of the fixed attributes.
fn sc_fixed(a: &Attr, i: usize) -> Option<Result<()>> {
    let need = |ok: Result<bool>| match ok {
        Ok(true) => Ok(()),
        Ok(false) => Err(not_supported(i)),
        Err(e) => Err(e),
    };
    match a.id {
        sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE | sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE => {
            Some(need(a.bool(i)))
        }
        sys::SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET | sys::SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET => {
            Some(need(a.u8(i).map(|v| v == 0)))
        }
        sys::SAI_MACSEC_SC_ATTR_USE_ES | sys::SAI_MACSEC_SC_ATTR_USE_SCB => Some(need(a.bool(i).map(|v| !v))),
        _ => None,
    }
}

// ============================================================ MACSEC
impl Macsec {
    /// `create_macsec`: one object per direction; it only answers capability questions.
    pub fn create_macsec(&mut self, attrs: &[Attr]) -> Result<MacsecId> {
        if self.need_table().is_err() {
            self.log(
                Level::Error,
                format_args!("MACsec: no external MACsec table: this switch has no MACsec engine"),
            );
            return Err(Error::NotSupported);
        }
        let (di, d) = find(attrs, sys::SAI_MACSEC_ATTR_DIRECTION).ok_or(Error::MandatoryAttributeMissing)?;
        for (i, a) in attrs.iter().enumerate() {
            match a.id {
                // PHYSICAL_BYPASS: ports without a MACSEC_PORT stay in bypass, which is all there is
                sys::SAI_MACSEC_ATTR_DIRECTION
                | sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE
                | sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE
                | sys::SAI_MACSEC_ATTR_STATS_MODE => {}
                sys::SAI_MACSEC_ATTR_ENABLE_POST if !a.bool(i)? => {}
                _ => return Err(not_supported(i)),
            }
        }
        let dir = Dir::from_sai(d.en(di)?);
        if self.objs.values().any(|o| *o == dir) {
            return Err(Error::ItemAlreadyExists);
        }
        let id = MacsecId::from_index(self.new_index(MacsecId::SAI_TYPE)?);
        self.objs.insert(id, dir);
        Ok(id)
    }

    /// `remove_macsec`: refused while a MACSEC_PORT of its direction exists.
    pub fn remove_macsec(&mut self, id: MacsecId) -> Result<()> {
        let dir = *self.objs.get(&id).ok_or(Error::InvalidObjectId)?;
        if self.mports.values().any(|p| p.dir == dir) {
            return Err(Error::ObjectInUse);
        }
        self.objs.remove(&id);
        Ok(())
    }

    /// `set_macsec_attribute`: PHYSICAL_BYPASS_ENABLE and WARM_BOOT_ENABLE are accepted and
    /// change nothing.
    pub fn set_macsec(&mut self, id: MacsecId, attr: &Attr) -> Result<()> {
        if !self.objs.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        match attr.id {
            sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE | sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE => Ok(()),
            _ => Err(not_supported(0)),
        }
    }

    /// `get_macsec_attribute`. The cipher capabilities are what the board table has
    /// `sa-add.` sequences for.
    pub fn get_macsec(&self, runner: &dyn SeqRunner, id: MacsecId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let dir = *self.objs.get(&id).ok_or(Error::InvalidObjectId)?;
        let has = |want: fn(&str) -> bool| {
            runner
                .sequence_names()
                .iter()
                .any(|n| n.starts_with("sa-add.") && want(n))
        };
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_MACSEC_ATTR_DIRECTION => dir_value(dir),
                    sys::SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC => {
                        Value::Enum(en(sys::SAI_MACSEC_MAX_SECURE_ASSOCIATIONS_PER_SC_FOUR))
                    }
                    sys::SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED => Value::Bool(has(|n| !n.contains("-xpn-"))),
                    sys::SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED => Value::Bool(has(|n| n.contains("-xpn-"))),
                    sys::SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED => Value::Bool(has(|n| !n.contains("-256"))),
                    sys::SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED => Value::Bool(has(|n| n.contains("-256"))),
                    // SCI_IN_INGRESS_MACSEC_ACL false: the SC is found by the PHY (one per
                    // port), not by an SCI match in the ACL
                    sys::SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL
                    | sys::SAI_MACSEC_ATTR_STATS_MODE_READ_CLEAR_SUPPORTED
                    | sys::SAI_MACSEC_ATTR_WARM_BOOT_SUPPORTED
                    | sys::SAI_MACSEC_ATTR_WARM_BOOT_ENABLE
                    | sys::SAI_MACSEC_ATTR_ENABLE_POST
                    | sys::SAI_MACSEC_ATTR_SWITCHING_MODE_CUT_THROUGH_SUPPORTED => Value::Bool(false),
                    sys::SAI_MACSEC_ATTR_STATS_MODE_READ_SUPPORTED
                    | sys::SAI_MACSEC_ATTR_SWITCHING_MODE_STORE_AND_FORWARD_SUPPORTED
                    | sys::SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE => Value::Bool(true),
                    sys::SAI_MACSEC_ATTR_STATS_MODE => Value::Enum(en(sys::SAI_STATS_MODE_READ)),
                    sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_FLOW | sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SC => {
                        Value::U32(count(self.ports.len()))
                    }
                    sys::SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA => Value::U32(count(self.ports.len()).saturating_mul(4)),
                    _ => return Err(not_supported(i)),
                })
            })
            .collect()
    }
}

// ============================================================ MACSEC_PORT
impl Macsec {
    /// `create_macsec_port`: `reset.<dir>` (when the table has it: nothing a previous process
    /// installed may stay valid) then `enable.<dir>` on the port's PHY, the wider inter-frame
    /// gap, then whatever SAs are already waiting for this port.
    pub fn create_macsec_port(&mut self, runner: &mut dyn SeqRunner, attrs: &[Attr]) -> Result<MacsecPortId> {
        self.need_table()?;
        let d = find(attrs, sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION);
        let pa = find(attrs, sys::SAI_MACSEC_PORT_ATTR_PORT_ID);
        let (Some((di, d)), Some((pi, pa))) = (d, pa) else {
            return Err(Error::MandatoryAttributeMissing);
        };
        for (i, a) in attrs.iter().enumerate() {
            match a.id {
                sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION
                | sys::SAI_MACSEC_PORT_ATTR_PORT_ID
                | sys::SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE
                | sys::SAI_MACSEC_PORT_ATTR_STATS_COUNT_MODE => {}
                // no VLAN tag in clear
                sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE | sys::SAI_MACSEC_PORT_ATTR_STAG_ENABLE if !a.bool(i)? => {}
                _ => return Err(not_supported(i)),
            }
        }
        // a front-panel port of this switch (not the CPU port, not another object)
        let port = PortId::try_from(pa.oid(pi)?).map_err(|_| invalid(pi))?;
        let hw = *self.switch_ports.get(&port).ok_or(invalid(pi))?;
        let Some(engine_port) = self.ports.get(&hw) else {
            self.log(
                Level::Error,
                format_args!("MACsec port: the port has no MACsec engine in the external MACsec table"),
            );
            return Err(Error::NotSupported);
        };
        let (name, phy) = (engine_port.name.clone(), engine_port.phy);
        let dir = Dir::from_sai(d.en(di)?);
        if self.mports.values().any(|m| m.port == port && m.dir == dir) {
            return Err(Error::ItemAlreadyExists);
        }
        self.run(runner, hw, dir, &seq::reset(dir), &Params::new(), true)?;
        self.run(runner, hw, dir, &seq::enable(dir), &Params::new(), false)?;
        if let Some(engine) = self.engine_mut(hw, dir) {
            engine.clear();
            // "enable" leaves data frames dropped (when the table has a gate at all)
            engine.enabled = true;
        }
        let id = match self.new_index(MacsecPortId::SAI_TYPE) {
            Ok(index) => MacsecPortId::from_index(index),
            Err(e) => {
                // only an external allocator can refuse (never the C++ one): the engine is on
                // but no object says so; the model goes back to "in bypass"
                if let Some(engine) = self.engine_mut(hw, dir) {
                    engine.clear();
                }
                return Err(e);
            }
        };
        self.mports.insert(id, MsPort { dir, port, hw });
        // room for the SecTAG + ICV the PHY adds to every frame
        if runner.set_ifg(hw, self.port_ifg(hw)).is_err() {
            self.log(
                Level::Error,
                format_args!("port {name}: MACsec is on without the wider inter-frame gap"),
            );
        }
        self.log(
            Level::Notice,
            format_args!("port {name}: MACsec {} on (external PHY 0x{phy:x})", dir.name()),
        );
        let _ = self.reconcile(runner, hw, dir, None);
        Ok(id)
    }

    /// `remove_macsec_port`: SAs still installed are taken out, then `disable.<dir>`. The
    /// object is gone whatever the sequences did; the status is `disable`'s (crate
    /// documentation, Q1).
    pub fn remove_macsec_port(&mut self, runner: &mut dyn SeqRunner, id: MacsecPortId) -> Result<()> {
        let (hw, dir) = self
            .mports
            .get(&id)
            .map(|m| (m.hw, m.dir))
            .ok_or(Error::InvalidObjectId)?;
        // SAs still installed (their objects outlive the port object only if the caller is out of order)
        let installed: Vec<SaId> = self
            .sas
            .iter()
            .filter(|(sa_id, sa)| sa.installed && self.engine(hw, dir).and_then(|e| e.slot(sa.an)) == Some(**sa_id))
            .map(|(sa_id, _)| *sa_id)
            .collect();
        for sa in installed {
            let _ = self.sa_uninstall(runner, hw, sa);
        }
        // SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP (off by default): the engine stays on with its
        // gate closed (the SAs above closed it), so data frames are dropped, not sent in clear
        let outcome = if self.engines_on {
            Ok(Results::new())
        } else {
            self.run(runner, hw, dir, &seq::disable(dir), &Params::new(), false)
        };
        if let Some(engine) = self.engine_mut(hw, dir) {
            engine.clear();
        }
        self.mports.remove(&id);
        let _ = runner.set_ifg(hw, self.port_ifg(hw));
        let name = self.ports.get(&hw).map(|p| p.name.clone()).unwrap_or_default();
        self.log(
            Level::Notice,
            format_args!(
                "port {name}: MACsec {} off{}",
                dir.name(),
                if self.engines_on {
                    " (engine left on: data frames dropped)"
                } else if outcome.is_ok() {
                    ""
                } else {
                    " (the disable sequence FAILED)"
                }
            ),
        );
        outcome.map(|_| ())
    }

    /// `set_macsec_port_attribute`: CTAG / STAG "off" is accepted, nothing else.
    pub fn set_macsec_port(&mut self, id: MacsecPortId, attr: &Attr) -> Result<()> {
        if !self.mports.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        match attr.id {
            sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE | sys::SAI_MACSEC_PORT_ATTR_STAG_ENABLE if !attr.bool(0)? => Ok(()),
            _ => Err(not_supported(0)),
        }
    }

    /// `get_macsec_port_attribute`
    pub fn get_macsec_port(&self, id: MacsecPortId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let m = self.mports.get(&id).ok_or(Error::InvalidObjectId)?;
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_MACSEC_PORT_ATTR_MACSEC_DIRECTION => dir_value(m.dir),
                    sys::SAI_MACSEC_PORT_ATTR_PORT_ID => Value::Oid(m.port.raw()),
                    sys::SAI_MACSEC_PORT_ATTR_CTAG_ENABLE | sys::SAI_MACSEC_PORT_ATTR_STAG_ENABLE => Value::Bool(false),
                    sys::SAI_MACSEC_PORT_ATTR_SWITCH_SWITCHING_MODE => {
                        Value::Enum(en(sys::SAI_SWITCH_SWITCHING_MODE_STORE_AND_FORWARD))
                    }
                    _ => return Err(not_supported(i)),
                })
            })
            .collect()
    }

    /// `get_macsec_port_stats[_ext]`: there are none (the flow carries the counters).
    pub fn get_macsec_port_stats(&self, id: MacsecPortId, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>> {
        if !self.mports.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if ids.is_empty() {
            Ok(Vec::new())
        } else {
            Err(Error::NotSupported)
        }
    }

    /// `clear_macsec_port_stats`: `NOT_SUPPORTED`, whatever the id (as in the C++).
    pub fn clear_macsec_port_stats(&self, _id: MacsecPortId) -> Result<()> {
        Err(Error::NotSupported)
    }
}

// ============================================================ MACSEC_FLOW
impl Macsec {
    /// `create_macsec_flow`
    pub fn create_flow(&mut self, attrs: &[Attr]) -> Result<FlowId> {
        self.need_table()?;
        let (di, d) =
            find(attrs, sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION).ok_or(Error::MandatoryAttributeMissing)?;
        if let Some(i) = attrs.iter().position(|a| {
            a.id != sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION && a.id != sys::SAI_MACSEC_FLOW_ATTR_STATS_COUNT_MODE
        }) {
            return Err(not_supported(i));
        }
        let dir = Dir::from_sai(d.en(di)?);
        let id = FlowId::from_index(self.new_index(FlowId::SAI_TYPE)?);
        self.flows.insert(
            id,
            MsFlow {
                dir,
                acc: Counters::new(),
            },
        );
        Ok(id)
    }

    /// `remove_macsec_flow`: refused while an SC or an ACL entry's flow action names it.
    pub fn remove_flow(&mut self, id: FlowId) -> Result<()> {
        if !self.flows.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        let used =
            self.scs.values().any(|sc| sc.flow == id) || self.entries.values().any(|e| e.flow_on && e.flow == Some(id));
        if used {
            return Err(Error::ObjectInUse);
        }
        self.flows.remove(&id);
        Ok(())
    }

    /// `set_macsec_flow_attribute`: nothing can be set.
    pub fn set_flow(&mut self, id: FlowId, _attr: &Attr) -> Result<()> {
        if self.flows.contains_key(&id) {
            Err(not_supported(0))
        } else {
            Err(Error::InvalidObjectId)
        }
    }

    /// `get_macsec_flow_attribute`
    pub fn get_flow(&self, id: FlowId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let flow = self.flows.get(&id).ok_or(Error::InvalidObjectId)?;
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_MACSEC_FLOW_ATTR_MACSEC_DIRECTION => dir_value(flow.dir),
                    sys::SAI_MACSEC_FLOW_ATTR_SC_LIST => Value::OidList(
                        self.scs
                            .iter()
                            .filter(|(_, sc)| sc.flow == id)
                            .map(|(sc, _)| sc.raw())
                            .collect(),
                    ),
                    sys::SAI_MACSEC_FLOW_ATTR_ACL_ENTRY_LIST => Value::OidList(
                        self.entries
                            .iter()
                            .filter(|(_, e)| e.flow_on && e.flow == Some(id))
                            .map(|(e, _)| e.raw())
                            .collect(),
                    ),
                    _ => return Err(not_supported(i)),
                })
            })
            .collect()
    }

    /// `get_macsec_flow_stats`: reads `stats-flow.<dir>` of the port the flow is bound to
    /// (or, unbound, the port its SC was last installed on) while that engine is on.
    pub fn get_flow_stats(
        &mut self,
        runner: &mut dyn SeqRunner,
        id: FlowId,
        ids: &[sys::sai_stat_id_t],
    ) -> Result<Vec<u64>> {
        let dir = self.flows.get(&id).ok_or(Error::InvalidObjectId)?.dir;
        let hw = self.hw_of_flow(id).or_else(|| {
            // not bound right now: the port its SC was installed on
            self.scs
                .values()
                .filter(|sc| sc.flow == id)
                .filter_map(|sc| sc.hw)
                .next_back()
        });
        if let Some(hw) = hw.filter(|hw| self.engine(*hw, dir).is_some_and(|e| e.enabled)) {
            if let Some(read) = self.poll(runner, hw, dir, &seq::stats_flow(dir))? {
                let mode = self.counters;
                if let Some(flow) = self.flows.get_mut(&id) {
                    Self::accumulate(mode, &mut flow.acc, &read);
                }
            }
        }
        let flow = self.flows.get(&id).ok_or(Error::InvalidObjectId)?;
        Ok(stats::fill(stats::FLOW_STATS, &flow.acc, ids))
    }

    /// `get_macsec_flow_stats_ext`: only `SAI_STATS_MODE_READ`.
    pub fn get_flow_stats_ext(
        &mut self,
        runner: &mut dyn SeqRunner,
        id: FlowId,
        ids: &[sys::sai_stat_id_t],
        mode: sys::sai_stats_mode_t,
    ) -> Result<Vec<u64>> {
        read_mode(mode)?;
        self.get_flow_stats(runner, id, ids)
    }

    /// `clear_macsec_flow_stats`: only when the PHY's counters clear on read (they are read
    /// once more, to empty them).
    pub fn clear_flow_stats(&mut self, runner: &mut dyn SeqRunner, id: FlowId) -> Result<()> {
        if !self.flows.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if self.counters != CounterMode::ClearOnRead {
            return Err(Error::NotSupported);
        }
        let _ = self.get_flow_stats(runner, id, &[sys::SAI_MACSEC_FLOW_STAT_OTHER_ERR]);
        if let Some(flow) = self.flows.get_mut(&id) {
            flow.acc.clear();
        }
        Ok(())
    }
}

// ============================================================ MACSEC_SC
impl Macsec {
    /// `create_macsec_sc`: parameters for its SAs; one SC per flow (one per port and direction).
    pub fn create_sc(&mut self, attrs: &[Attr]) -> Result<ScId> {
        self.need_table()?;
        let (mut dir, mut flow, mut sci) = (None, None, None);
        let (mut cipher, mut replay, mut window) = (Cipher::GcmAes128, false, 0);
        for (i, a) in attrs.iter().enumerate() {
            if let Some(fixed) = sc_fixed(a, i) {
                fixed?;
                continue;
            }
            match a.id {
                sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION => dir = Some(Dir::from_sai(a.en(i)?)),
                sys::SAI_MACSEC_SC_ATTR_FLOW_ID => {
                    let id = FlowId::try_from(a.oid(i)?).map_err(|_| invalid(i))?;
                    if !self.flows.contains_key(&id) {
                        return Err(invalid(i));
                    }
                    flow = Some(id);
                }
                sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI => sci = Some(sci_from_attr(a.u64(i)?)),
                sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE => cipher = Cipher::from_sai(a.en(i)?).ok_or(invalid(i))?,
                sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE => replay = a.bool(i)?,
                sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW => window = a.u32(i)?,
                sys::SAI_MACSEC_SC_ATTR_MACSEC_PORT_ID => {}
                _ => return Err(not_supported(i)),
            }
        }
        let (Some(dir), Some(flow), Some(sci)) = (dir, flow, sci) else {
            return Err(Error::MandatoryAttributeMissing);
        };
        if self.flows.get(&flow).map(|f| f.dir) != Some(dir) {
            return Err(Error::InvalidParameter);
        }
        if self.scs.values().any(|sc| sc.flow == flow) {
            self.log(
                Level::Error,
                format_args!("MACsec SC: its flow already has a secure channel (one per port and direction)"),
            );
            return Err(Error::InsufficientResources);
        }
        let id = ScId::from_index(self.new_index(ScId::SAI_TYPE)?);
        let sc = MsSc {
            dir,
            flow,
            sci,
            cipher,
            replay,
            window,
            hw: None,
        };
        self.scs.insert(id, sc);
        Ok(id)
    }

    /// `remove_macsec_sc`: refused while it has SAs.
    pub fn remove_sc(&mut self, id: ScId) -> Result<()> {
        if !self.scs.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if self.sas.values().any(|sa| sa.sc == id) {
            return Err(Error::ObjectInUse);
        }
        self.scs.remove(&id);
        Ok(())
    }

    /// `set_macsec_sc_attribute`: cipher suite, replay protection and window take effect
    /// for SAs installed from now on, and cannot change under installed SAs.
    pub fn set_sc(&mut self, id: ScId, attr: &Attr) -> Result<()> {
        if !self.scs.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if let Some(fixed) = sc_fixed(attr, 0) {
            return fixed;
        }
        let has_sa = self.sas.values().any(|sa| sa.sc == id && sa.installed);
        // D10 (Q10 fixed): a waiting SA keeps the key bytes, salt and PN cut for the suite it
        // was created under; the suite cannot change under it either.
        let has_waiting = self.sas.values().any(|sa| sa.sc == id && !sa.installed);
        let sc = self.scs.get_mut(&id).ok_or(Error::InvalidObjectId)?;
        match attr.id {
            sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE => {
                let cipher = Cipher::from_sai(attr.en(0)?).ok_or(invalid(0))?;
                if (has_sa || has_waiting) && cipher != sc.cipher {
                    return Err(Error::ObjectInUse);
                }
                sc.cipher = cipher;
            }
            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE => {
                let replay = attr.bool(0)?;
                if has_sa && replay != sc.replay {
                    return Err(Error::ObjectInUse);
                }
                sc.replay = replay;
            }
            sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW => {
                let window = attr.u32(0)?;
                if has_sa && sc.replay && window != sc.window {
                    return Err(Error::ObjectInUse);
                }
                sc.window = window;
            }
            _ => return Err(not_supported(0)),
        }
        Ok(())
    }

    /// `get_macsec_sc_attribute`
    pub fn get_sc(&self, id: ScId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let sc = self.scs.get(&id).ok_or(Error::InvalidObjectId)?;
        ids.iter()
            .enumerate()
            .map(|(i, attr)| {
                Ok(match *attr {
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_DIRECTION => dir_value(sc.dir),
                    sys::SAI_MACSEC_SC_ATTR_FLOW_ID => Value::Oid(sc.flow.raw()),
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_SCI => Value::U64(sci_to_attr(sc.sci)),
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_CIPHER_SUITE => Value::Enum(sc.cipher.sai()),
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_ENABLE => Value::Bool(sc.replay),
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_REPLAY_PROTECTION_WINDOW => Value::U32(sc.window),
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_EXPLICIT_SCI_ENABLE | sys::SAI_MACSEC_SC_ATTR_ENCRYPTION_ENABLE => {
                        Value::Bool(true)
                    }
                    sys::SAI_MACSEC_SC_ATTR_MACSEC_SECTAG_OFFSET | sys::SAI_MACSEC_SC_ATTR_CONFIDENTIALITY_OFFSET => {
                        Value::U8(0)
                    }
                    sys::SAI_MACSEC_SC_ATTR_USE_ES | sys::SAI_MACSEC_SC_ATTR_USE_SCB => Value::Bool(false),
                    sys::SAI_MACSEC_SC_ATTR_ACTIVE_EGRESS_SA_ID => Value::Oid(
                        sc.hw
                            .filter(|_| sc.dir == Dir::Egress)
                            .and_then(|hw| self.engine(hw, Dir::Egress))
                            .and_then(|e| e.tx)
                            .map_or(RawOid::NULL, SaId::raw),
                    ),
                    sys::SAI_MACSEC_SC_ATTR_SA_LIST => Value::OidList(
                        self.sas
                            .iter()
                            .filter(|(_, sa)| sa.sc == id)
                            .map(|(sa, _)| sa.raw())
                            .collect(),
                    ),
                    _ => return Err(not_supported(i)),
                })
            })
            .collect()
    }

    /// `get_macsec_sc_stats[_ext]`: there are none.
    pub fn get_sc_stats(&self, id: ScId, ids: &[sys::sai_stat_id_t]) -> Result<Vec<u64>> {
        if !self.scs.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if ids.is_empty() {
            Ok(Vec::new())
        } else {
            Err(Error::NotSupported)
        }
    }

    /// `clear_macsec_sc_stats`: `NOT_SUPPORTED`, whatever the id (as in the C++).
    pub fn clear_sc_stats(&self, _id: ScId) -> Result<()> {
        Err(Error::NotSupported)
    }
}

// ============================================================ MACSEC_SA
impl Macsec {
    /// `create_macsec_sa`. When the SC's flow is bound to a port whose engine is on, the SA
    /// is installed now (`sa-add...`) and a failed sequence fails the call with nothing kept.
    /// Otherwise it is kept, key included, until the binding appears.
    pub fn create_sa(&mut self, runner: &mut dyn SeqRunner, attrs: &[Attr]) -> Result<SaId> {
        self.need_table()?;
        let args = SaArgs::parse(attrs)?;
        let sc_id = ScId::try_from(args.sc).map_err(|_| Error::InvalidParameter)?;
        let (sc_dir, cipher, flow) = self
            .scs
            .get(&sc_id)
            .map(|sc| (sc.dir, sc.cipher, sc.flow))
            .ok_or(Error::InvalidParameter)?;
        let dir = Dir::from_sai(args.dir);
        if dir != sc_dir {
            return Err(Error::InvalidParameter);
        }
        if self.sas.values().any(|sa| sa.sc == sc_id && sa.an == args.an) {
            return Err(Error::ItemAlreadyExists);
        }
        if cipher.is_xpn() && (args.salt.is_none() || args.ssci.is_none()) {
            return Err(Error::MandatoryAttributeMissing);
        }
        if !cipher.is_xpn() && args.pn > u64::from(u32::MAX) {
            return Err(Error::InvalidParameter);
        }
        let par = args.params(cipher, self.auth_key)?;
        self.order += 1;
        let id = SaId::from_index(self.new_index(SaId::SAI_TYPE)?);
        let sa = MsSa {
            dir,
            sc: sc_id,
            an: args.an,
            par,
            pn: args.pn,
            order: self.order,
            installed: false,
            acc: Counters::new(),
        };
        self.sas.insert(id, sa);
        // Install now if the SC's flow is bound to a port, else when it is (reconcile).
        let bound = self
            .hw_of_flow(flow)
            .filter(|hw| self.engine(*hw, dir).is_some_and(|e| e.enabled));
        if let Some(hw) = bound {
            if let Err(e) = self.reconcile(runner, hw, dir, Some(id)) {
                self.sas.remove(&id); // its parameters are wiped as it is dropped
                return Err(e);
            }
        } else {
            self.log(
                Level::Info,
                format_args!(
                    "MACsec {} SA AN {}: kept until its flow is bound to a port",
                    dir.name(),
                    args.an.get()
                ),
            );
        }
        Ok(id)
    }

    /// `remove_macsec_sa`: `sa-del...` when it is installed (the gate closes first when it
    /// is the last or the transmitting one). The object is gone whatever the sequence did;
    /// the status is the sequence's (crate documentation, Q1).
    pub fn remove_sa(&mut self, runner: &mut dyn SeqRunner, id: SaId) -> Result<()> {
        let (dir, an, sc) = self
            .sas
            .get(&id)
            .map(|sa| (sa.dir, sa.an, sa.sc))
            .ok_or(Error::InvalidObjectId)?;
        let holder = self.holder(id);
        let hw = holder.or_else(|| self.scs.get(&sc).and_then(|sc| sc.hw));
        let mut outcome = Ok(());
        if let Some(hw) = holder {
            outcome = self.sa_uninstall(runner, hw, id);
            let name = self.ports.get(&hw).map(|p| p.name.clone()).unwrap_or_default();
            self.log(
                Level::Notice,
                format_args!(
                    "port {name}: {} SA AN {} removed{}",
                    dir.name(),
                    an.get(),
                    if outcome.is_ok() { "" } else { " (the sequence FAILED)" }
                ),
            );
        }
        self.sas.remove(&id); // wiped as it is dropped
        if let Some(hw) = hw {
            let _ = self.reconcile(runner, hw, dir, None);
        }
        outcome
    }

    /// The packet number of an SA: as configured until it is installed, then read from the
    /// PHY (`pn.<dir>.<an>`, when the table has it), 32 bits for a non-XPN suite.
    fn sa_pn(&mut self, runner: &mut dyn SeqRunner, id: SaId) -> Result<u64> {
        let (dir, an, sc, pn) = self
            .sas
            .get(&id)
            .map(|sa| (sa.dir, sa.an, sa.sc, sa.pn))
            .ok_or(Error::InvalidObjectId)?;
        let (Some(hw), Some(cipher)) = (self.holder(id), self.scs.get(&sc).map(|sc| sc.cipher)) else {
            return Ok(pn);
        };
        let name = seq::pn(dir, an);
        if !runner.has(&name) {
            return Ok(pn);
        }
        let read = self.run(runner, hw, dir, &name, &Params::new(), false)?;
        let mut pn = read.u64(param::PN);
        if !cipher.is_xpn() {
            pn &= u64::from(u32::MAX);
        }
        if let Some(sa) = self.sas.get_mut(&id) {
            sa.pn = pn;
        }
        Ok(pn)
    }

    /// `set_macsec_sa_attribute`: MINIMUM_INGRESS_XPN of an ingress SA. The lowest
    /// acceptable PN of an installed SA moves with the PHY's replay window: a KaY-driven
    /// update is accepted and not written (there is no sequence for it).
    pub fn set_sa(&mut self, id: SaId, attr: &Attr) -> Result<()> {
        let sa = self.sas.get_mut(&id).ok_or(Error::InvalidObjectId)?;
        if attr.id != sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN || sa.dir != Dir::Ingress {
            return Err(not_supported(0));
        }
        let pn = attr.u64(0)?;
        if !sa.installed {
            sa.pn = pn;
            if !sa.par.is_empty() {
                sa.par.set(param::PN, &pn.to_le_bytes());
            }
        }
        Ok(())
    }

    /// `get_macsec_sa_attribute`. SAK, SALT, AUTH_KEY and SSCI are write-only: the key is
    /// not kept once it is in the PHY.
    pub fn get_sa(&mut self, runner: &mut dyn SeqRunner, id: SaId, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let (dir, sc, an) = self
            .sas
            .get(&id)
            .map(|sa| (sa.dir, sa.sc, sa.an))
            .ok_or(Error::InvalidObjectId)?;
        let mut out = Vec::with_capacity(ids.len());
        for (i, attr) in ids.iter().enumerate() {
            out.push(match *attr {
                sys::SAI_MACSEC_SA_ATTR_MACSEC_DIRECTION => dir_value(dir),
                sys::SAI_MACSEC_SA_ATTR_SC_ID => Value::Oid(sc.raw()),
                sys::SAI_MACSEC_SA_ATTR_AN => Value::U8(an.get()),
                // egress: next PN; ingress: lowest acceptable PN
                sys::SAI_MACSEC_SA_ATTR_CURRENT_XPN
                | sys::SAI_MACSEC_SA_ATTR_MINIMUM_INGRESS_XPN
                | sys::SAI_MACSEC_SA_ATTR_CONFIGURED_EGRESS_XPN => Value::U64(self.sa_pn(runner, id)?),
                _ => return Err(not_supported(i)),
            });
        }
        Ok(out)
    }

    /// Reads the SA's counters from the PHY when it is installed (`sa_poll`).
    fn sa_poll(&mut self, runner: &mut dyn SeqRunner, id: SaId) -> Result<()> {
        let (Some((dir, an)), Some(hw)) = (self.sas.get(&id).map(|sa| (sa.dir, sa.an)), self.holder(id)) else {
            return Ok(());
        };
        if let Some(read) = self.poll(runner, hw, dir, &seq::stats_sa(dir, an))? {
            let mode = self.counters;
            if let Some(sa) = self.sas.get_mut(&id) {
                Self::accumulate(mode, &mut sa.acc, &read);
            }
        }
        Ok(())
    }

    /// `get_macsec_sa_stats`
    pub fn get_sa_stats(
        &mut self,
        runner: &mut dyn SeqRunner,
        id: SaId,
        ids: &[sys::sai_stat_id_t],
    ) -> Result<Vec<u64>> {
        if !self.sas.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        self.sa_poll(runner, id)?;
        let sa = self.sas.get(&id).ok_or(Error::InvalidObjectId)?;
        Ok(stats::fill(stats::SA_STATS, &sa.acc, ids))
    }

    /// `get_macsec_sa_stats_ext`: only `SAI_STATS_MODE_READ`.
    pub fn get_sa_stats_ext(
        &mut self,
        runner: &mut dyn SeqRunner,
        id: SaId,
        ids: &[sys::sai_stat_id_t],
        mode: sys::sai_stats_mode_t,
    ) -> Result<Vec<u64>> {
        read_mode(mode)?;
        self.get_sa_stats(runner, id, ids)
    }

    /// `clear_macsec_sa_stats`: only when the PHY's counters clear on read.
    pub fn clear_sa_stats(&mut self, runner: &mut dyn SeqRunner, id: SaId) -> Result<()> {
        if !self.sas.contains_key(&id) {
            return Err(Error::InvalidObjectId);
        }
        if self.counters != CounterMode::ClearOnRead {
            return Err(Error::NotSupported);
        }
        let _ = self.sa_poll(runner, id);
        if let Some(sa) = self.sas.get_mut(&id) {
            sa.acc.clear();
        }
        Ok(())
    }
}

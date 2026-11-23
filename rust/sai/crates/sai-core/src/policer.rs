//! Policers: SDK meters for `CoPP` (a trap group's `POLICER`) and ACL entries
//! (`ACTION_SET_POLICER`). Ports the `POLICER` part of `sai_acl.cpp` (lines 70..=252): the
//! attributes `CoppOrch` sets, the colour actions FORWARD / DROP only, stats `NOT_SUPPORTED` (no
//! per-colour counters).
//!
//! Backend: [`crate::backend::Policer`]. A change of a colour action re-programs `CoPP` and the
//! ACL entries ([`crate::copp::copp_refresh`], [`crate::acl::acl_reprogram`]): the colours are
//! entry actions in the SDK.
//!
//! Registration: [`crate::dispatch::Owner::Acl`] for `POLICER` ([`crate::acl::AclArea`] forwards).
//! The objects are [`Policer`]s in the store ([`crate::acl::AclObject::Policer`]).
use crate::backend::Backend;
use crate::backend::types::{PolicerCfg, PolicerId};
use crate::error::{AttrError, Error, Result};
use crate::log;
use crate::oid::{RawOid, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_sys as sys;

const fn e(enumerator: u32) -> i32 {
    enumerator.cast_signed()
}

/// `SAI_POLICER_ATTR_METER_TYPE` (C++ accepts packets and bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeterType {
    /// `PACKETS` (the C++ default)
    #[default]
    Packets,
    /// `BYTES`
    Bytes,
}

/// `SAI_POLICER_ATTR_MODE` (C++ accepts the two three-colour markers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolicerMode {
    /// `SR_TCM` (the C++ default)
    #[default]
    SrTcm,
    /// `TR_TCM`
    TrTcm,
}

/// `SAI_POLICER_ATTR_COLOR_SOURCE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorSource {
    /// `BLIND`
    Blind,
    /// `AWARE` (the C++ default)
    #[default]
    Aware,
}

/// A colour's packet action (`GREEN` / `YELLOW` / `RED_PACKET_ACTION`): FORWARD or DROP only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorAction {
    /// `FORWARD` (the default)
    #[default]
    Forward,
    /// `DROP`
    Drop,
}

impl ColorAction {
    fn from_sai(v: i32) -> Option<Self> {
        match v {
            x if x == e(sys::SAI_PACKET_ACTION_FORWARD) => Some(Self::Forward),
            x if x == e(sys::SAI_PACKET_ACTION_DROP) => Some(Self::Drop),
            _ => None,
        }
    }

    const fn sai(self) -> i32 {
        match self {
            Self::Forward => e(sys::SAI_PACKET_ACTION_FORWARD),
            Self::Drop => e(sys::SAI_PACKET_ACTION_DROP),
        }
    }

    /// Packets of this colour are dropped.
    pub const fn drops(self) -> bool {
        matches!(self, Self::Drop)
    }
}

/// What a policer is set to (C++ `PolicerObj` without the backend id); the defaults are the C++'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicerSettings {
    /// `METER_TYPE` (create only)
    pub meter: MeterType,
    /// `MODE` (create only)
    pub mode: PolicerMode,
    /// `COLOR_SOURCE` (create only)
    pub color: ColorSource,
    /// `CIR`
    pub cir: u64,
    /// `CBS`
    pub cbs: u64,
    /// `PIR`
    pub pir: u64,
    /// `PBS`
    pub pbs: u64,
    /// `GREEN_PACKET_ACTION`
    pub green: ColorAction,
    /// `YELLOW_PACKET_ACTION`
    pub yellow: ColorAction,
    /// `RED_PACKET_ACTION`
    pub red: ColorAction,
}

impl PolicerSettings {
    /// The SDK meter (C++ `policer_cfg`).
    pub const fn cfg(&self) -> PolicerCfg {
        PolicerCfg {
            tr_tcm: matches!(self.mode, PolicerMode::TrTcm),
            packets: matches!(self.meter, MeterType::Packets),
            color_blind: matches!(self.color, ColorSource::Blind),
            cir: self.cir,
            cbs: self.cbs,
            pir: self.pir,
            pbs: self.pbs,
        }
    }

    const fn mode_name(&self) -> &'static str {
        match self.mode {
            PolicerMode::TrTcm => "tr_tcm",
            PolicerMode::SrTcm => "sr_tcm",
        }
    }

    const fn meter_name(&self) -> &'static str {
        match self.meter {
            MeterType::Packets => "packets",
            MeterType::Bytes => "bytes",
        }
    }
}

/// A policer: its settings and the SDK meter that carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policer {
    /// The attributes.
    pub settings: PolicerSettings,
    /// The backend's meter.
    pub pid: PolicerId,
}

/// What a trap group's policer means for a trap the backend programs itself
/// ([`crate::hostif::trap_spec`], C++ `trap_spec`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrapPolicer {
    /// The backend's meter.
    pub pid: PolicerId,
    /// `YELLOW_PACKET_ACTION` is DROP.
    pub yellow_drop: bool,
    /// `RED_PACKET_ACTION` is DROP.
    pub red_drop: bool,
}

fn not_supported(index: usize) -> Error {
    Error::attr(AttrError::NotSupported, index)
}

fn invalid(index: usize) -> Error {
    Error::attr(AttrError::InvalidValue, index)
}

fn policer_id(raw: RawOid) -> Result<crate::oid::PolicerId> {
    crate::oid::PolicerId::try_from(raw)
}

/// The policer an id names, if it is one (C++ `lookup(ctx().policers, id)`).
pub(crate) fn policer(store: &Store, raw: RawOid) -> Option<&Policer> {
    store.get::<Policer>(policer_id(raw).ok()?).ok()
}

/// Whether `raw` is a live policer (C++ `policer_valid`: trap groups, samplers, ACL entries).
pub(crate) fn policer_valid(store: &Store, raw: RawOid) -> bool {
    policer(store, raw).is_some()
}

/// The backend meter of a policer (`None`: not a policer; the port storm controls).
pub(crate) fn policer_backend_id(store: &Store, raw: RawOid) -> Option<PolicerId> {
    policer(store, raw).map(|p| p.pid)
}

/// A trap group's policer as `trap_program` needs it (`None`: not a policer).
pub(crate) fn trap_policer(store: &Store, raw: RawOid) -> Option<TrapPolicer> {
    policer(store, raw).map(|p| TrapPolicer {
        pid: p.pid,
        yellow_drop: p.settings.yellow.drops(),
        red_drop: p.settings.red.drops(),
    })
}

fn enum_of(attr: &Attr) -> Result<i32> {
    match attr.value {
        Value::Enum(v) | Value::I32(v) => Ok(v),
        _ => Err(Error::Failure),
    }
}

fn u64_of(attr: &Attr) -> Result<u64> {
    match attr.value {
        Value::U64(v) => Ok(v),
        _ => Err(Error::Failure),
    }
}

/// One attribute into the settings, with the C++ statuses (`policer_attr`): create-only ones
/// refused on set, enumerators the SDK has no meter for `NOT_SUPPORTED`, a colour source that is
/// neither `INVALID_ATTR_VALUE`.
fn policer_attr(p: &mut PolicerSettings, attr: &Attr, index: usize, create: bool) -> Result<()> {
    match attr.id {
        sys::SAI_POLICER_ATTR_METER_TYPE => {
            if !create {
                return Err(invalid(index));
            }
            p.meter = match enum_of(attr)? {
                v if v == e(sys::SAI_METER_TYPE_PACKETS) => MeterType::Packets,
                v if v == e(sys::SAI_METER_TYPE_BYTES) => MeterType::Bytes,
                _ => return Err(not_supported(index)),
            };
        }
        sys::SAI_POLICER_ATTR_MODE => {
            if !create {
                return Err(invalid(index));
            }
            p.mode = match enum_of(attr)? {
                v if v == e(sys::SAI_POLICER_MODE_SR_TCM) => PolicerMode::SrTcm,
                v if v == e(sys::SAI_POLICER_MODE_TR_TCM) => PolicerMode::TrTcm,
                _ => return Err(not_supported(index)),
            };
        }
        sys::SAI_POLICER_ATTR_COLOR_SOURCE => {
            if !create {
                return Err(invalid(index));
            }
            p.color = match enum_of(attr)? {
                v if v == e(sys::SAI_POLICER_COLOR_SOURCE_BLIND) => ColorSource::Blind,
                v if v == e(sys::SAI_POLICER_COLOR_SOURCE_AWARE) => ColorSource::Aware,
                _ => return Err(invalid(index)),
            };
        }
        sys::SAI_POLICER_ATTR_CBS => p.cbs = u64_of(attr)?,
        sys::SAI_POLICER_ATTR_CIR => p.cir = u64_of(attr)?,
        sys::SAI_POLICER_ATTR_PBS => p.pbs = u64_of(attr)?,
        sys::SAI_POLICER_ATTR_PIR => p.pir = u64_of(attr)?,
        sys::SAI_POLICER_ATTR_GREEN_PACKET_ACTION
        | sys::SAI_POLICER_ATTR_YELLOW_PACKET_ACTION
        | sys::SAI_POLICER_ATTR_RED_PACKET_ACTION => {
            let action = ColorAction::from_sai(enum_of(attr)?).ok_or(not_supported(index))?;
            match attr.id {
                sys::SAI_POLICER_ATTR_GREEN_PACKET_ACTION => p.green = action,
                sys::SAI_POLICER_ATTR_YELLOW_PACKET_ACTION => p.yellow = action,
                _ => p.red = action,
            }
        }
        _ => return Err(not_supported(index)),
    }
    Ok(())
}

/// C++ `create_policer`: `METER_TYPE` and `MODE` mandatory (checked first), every attribute in
/// order, then the SDK meter; the object only once the backend made it.
pub(crate) fn create<B: Backend>(backend: &mut B, store: &mut Store, attrs: &[Attr]) -> Result<RawOid> {
    let has = |id| attrs.iter().any(|a| a.id == id);
    if !has(sys::SAI_POLICER_ATTR_METER_TYPE) || !has(sys::SAI_POLICER_ATTR_MODE) {
        return Err(Error::MandatoryAttributeMissing);
    }
    let mut s = PolicerSettings::default();
    for (index, attr) in attrs.iter().enumerate() {
        policer_attr(&mut s, attr, index, true)?;
    }
    let pid = backend.policer_create(&s.cfg()).inspect_err(|err| {
        log::error(
            sys::SAI_API_POLICER,
            format_args!(
                "policer ({}, {}, cir {} cbs {} pir {} pbs {}): SDK create failed ({})",
                s.mode_name(),
                s.meter_name(),
                s.cir,
                s.cbs,
                s.pir,
                s.pbs,
                err.status()
            ),
        );
    })?;
    let id = store.insert(Policer { settings: s, pid })?;
    log::notice(
        sys::SAI_API_POLICER,
        format_args!(
            "policer {}: {} {} cir {} cbs {} pir {} pbs {}",
            pid.0,
            s.mode_name(),
            s.meter_name(),
            s.cir,
            s.cbs,
            s.pir,
            s.pbs
        ),
    );
    Ok(id.raw())
}

/// C++ `remove_policer`: in use while a trap group, a port's storm control or an ACL entry
/// names it (the store's references); the SDK meter goes first (`ITEM_NOT_FOUND` counts as gone).
pub(crate) fn remove<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid) -> Result<()> {
    let id = policer_id(raw)?;
    store.ensure_removable::<Policer>(id)?;
    let pid = store.get::<Policer>(id)?.pid;
    match backend.policer_destroy(pid) {
        Ok(()) | Err(Error::ItemNotFound) => {}
        Err(other) => return Err(other),
    }
    store.remove::<Policer>(id).map(drop)
}

/// C++ `set_policer_attribute`: the SDK meter first; a changed colour action re-programs `CoPP`
/// and the ACL entries after the model took it.
pub(crate) fn set<B: Backend>(backend: &mut B, store: &mut Store, raw: RawOid, attr: &Attr) -> Result<()> {
    let id = policer_id(raw)?;
    let before = *store.get::<Policer>(id)?;
    let mut after = before;
    policer_attr(&mut after.settings, attr, 0, false)?;
    let (b, a) = (&before.settings, &after.settings);
    let colors = a.green != b.green || a.yellow != b.yellow || a.red != b.red;
    backend
        .policer_set(before.pid, &after.settings.cfg())
        .map_err(|err| crate::acl::attr_backend_status(err, 0))?;
    store.update::<Policer, _>(id, |p| *p = after)?;
    if colors {
        crate::copp::copp_refresh(backend, store);
        crate::acl::acl_reprogram(backend, store);
    }
    Ok(())
}

/// C++ `get_policer_attribute`.
pub(crate) fn get(store: &Store, raw: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
    let p = store.get::<Policer>(policer_id(raw)?)?.settings;
    Ok(match attr {
        sys::SAI_POLICER_ATTR_METER_TYPE => Value::Enum(match p.meter {
            MeterType::Packets => e(sys::SAI_METER_TYPE_PACKETS),
            MeterType::Bytes => e(sys::SAI_METER_TYPE_BYTES),
        }),
        sys::SAI_POLICER_ATTR_MODE => Value::Enum(match p.mode {
            PolicerMode::SrTcm => e(sys::SAI_POLICER_MODE_SR_TCM),
            PolicerMode::TrTcm => e(sys::SAI_POLICER_MODE_TR_TCM),
        }),
        sys::SAI_POLICER_ATTR_COLOR_SOURCE => Value::Enum(match p.color {
            ColorSource::Blind => e(sys::SAI_POLICER_COLOR_SOURCE_BLIND),
            ColorSource::Aware => e(sys::SAI_POLICER_COLOR_SOURCE_AWARE),
        }),
        sys::SAI_POLICER_ATTR_CBS => Value::U64(p.cbs),
        sys::SAI_POLICER_ATTR_CIR => Value::U64(p.cir),
        sys::SAI_POLICER_ATTR_PBS => Value::U64(p.pbs),
        sys::SAI_POLICER_ATTR_PIR => Value::U64(p.pir),
        sys::SAI_POLICER_ATTR_GREEN_PACKET_ACTION => Value::Enum(p.green.sai()),
        sys::SAI_POLICER_ATTR_YELLOW_PACKET_ACTION => Value::Enum(p.yellow.sai()),
        sys::SAI_POLICER_ATTR_RED_PACKET_ACTION => Value::Enum(p.red.sai()),
        _ => return Err(not_supported(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_the_sdk_meter_are_the_cpp_ones() {
        let s = PolicerSettings::default();
        assert_eq!(s.cfg(), PolicerCfg::default());
        let mut t = s;
        let set = |p: &mut PolicerSettings, id, value| policer_attr(p, &Attr::new(id, value), 2, true);
        set(
            &mut t,
            sys::SAI_POLICER_ATTR_MODE,
            Value::Enum(e(sys::SAI_POLICER_MODE_TR_TCM)),
        )
        .unwrap();
        set(
            &mut t,
            sys::SAI_POLICER_ATTR_COLOR_SOURCE,
            Value::Enum(e(sys::SAI_POLICER_COLOR_SOURCE_BLIND)),
        )
        .unwrap();
        assert!(t.cfg().tr_tcm && t.cfg().color_blind && t.cfg().packets);
        assert_eq!(
            set(
                &mut t,
                sys::SAI_POLICER_ATTR_MODE,
                Value::Enum(e(sys::SAI_POLICER_MODE_STORM_CONTROL))
            ),
            Err(not_supported(2))
        );
        assert_eq!(
            set(&mut t, sys::SAI_POLICER_ATTR_COLOR_SOURCE, Value::Enum(77)),
            Err(invalid(2))
        );
        assert_eq!(
            set(
                &mut t,
                sys::SAI_POLICER_ATTR_RED_PACKET_ACTION,
                Value::Enum(e(sys::SAI_PACKET_ACTION_TRAP))
            ),
            Err(not_supported(2))
        );
        assert_eq!(
            policer_attr(&mut t, &Attr::new(sys::SAI_POLICER_ATTR_MODE, Value::Enum(0)), 0, false),
            Err(invalid(0))
        );
    }
}

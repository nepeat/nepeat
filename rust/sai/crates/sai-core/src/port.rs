//! Ports: attributes, board quirks, counters. Ports `sai_port.cpp` (438 lines).
//!
//! - **Set** ([`set`]): `ADMIN_STATE` (the external PHY's line side comes on before the MAC and
//!   goes off after it, [`crate::extphy::ext_phy_admin`]), `SPEED` (then the board quirks again),
//!   `MTU` 68..=9408 as `frame_max = MTU + 8`, `FEC_MODE` / `AUTO_NEG_MODE` (kept, not
//!   programmed, when a quirk fixes them), `PORT_VLAN_ID`, `LOOPBACK_MODE`, the ACL bindings
//!   ([`crate::acl`]); then, in the C++ order, the `QoS` ([`crate::qos`]), storm-control
//!   ([`crate::hash`]), mirror ([`crate::mirror`]) and `MACsec` ([`crate::macsec`]) handlers;
//!   then the 14 kept-only attributes ([`PORT_STUBS`]); anything else is `ATTR_NOT_SUPPORTED`.
//!   The CPU port takes no attribute.
//! - **Get** ([`get`]): the same order; speed, link and supported speeds are read from the chip.
//! - **Board quirks** ([`PortQuirks`], `SAI_OPENBCM_PORT_{INTERFACE,FEC,AUTONEG,IFG}`): parsed at
//!   switch creation, applied to every port then and again after a speed change
//!   ([`port_quirks_apply`]); the inter-frame gap follows the port's `MACsec` state
//!   ([`port_ifg_apply`]).
//! - **Counters** ([`stats_get`]): the 15 `IF_*` counters through `port_stat_get`, the ones the
//!   backend reads by SAI id in one `port_stats_by_id` call, PFC frames through
//!   `port_pfc_stat_get` (only with `qos_supported`); `READ_AND_CLEAR` and per-counter clear are
//!   `NOT_SUPPORTED` (the SDK has no per-counter clear); [`clear_all_stats`] zeroes all.
//!
//! Registration: `PORT` is [`crate::dispatch::Owner::Core`]: [`crate::sai::Sai`] sends ports
//! here itself (ports are neither created nor removed: `NOT_SUPPORTED`).
use crate::backend::types::{Fec, Loopback, PortInterface, PortStat};
use crate::backend::{Backend, HwPort, Profile, Vid};
use crate::error::{AttrError, Error, Result};
use crate::model::{Port, PortKind, Switch};
use crate::oid::{ObjectType, PortId, RawOid, SwitchId, TypedId};
use crate::store::Store;
use crate::value::{Attr, Value};
use crate::{acl, extphy, hash, keys, log, macsec, mirror, qos};
use sai_sys as sys;

/// The largest MTU (the SDK's frame maximum 9416 minus FCS and VLAN tag).
pub const PORT_MAX_MTU: u32 = 9408;
/// The smallest MTU.
const PORT_MIN_MTU: u32 = 68;
/// What the SDK counts in a frame beyond the SAI MTU (FCS and a VLAN tag).
const FRAME_OVERHEAD: u32 = 8;
/// The Ethernet inter-frame gap, bit times.
const IFG_ETHERNET: u32 = 96;

/// Port attributes accepted, kept and returned without effect (C++ `port_stubs`): id and the
/// default a get answers while unset.
pub const PORT_STUBS: &[(sys::sai_attr_id_t, i64)] = &[
    (
        sys::SAI_PORT_ATTR_MEDIA_TYPE,
        sys::SAI_PORT_MEDIA_TYPE_NOT_PRESENT as i64,
    ),
    (
        sys::SAI_PORT_ATTR_INTERFACE_TYPE,
        sys::SAI_PORT_INTERFACE_TYPE_NONE as i64,
    ),
    (sys::SAI_PORT_ATTR_DEFAULT_VLAN_PRIORITY, 0),
    (sys::SAI_PORT_ATTR_DROP_UNTAGGED, 0),
    (sys::SAI_PORT_ATTR_DROP_TAGGED, 0),
    (sys::SAI_PORT_ATTR_LINK_TRAINING_ENABLE, 0),
    (
        sys::SAI_PORT_ATTR_GLOBAL_FLOW_CONTROL_MODE,
        sys::SAI_PORT_FLOW_CONTROL_MODE_DISABLE as i64,
    ),
    (sys::SAI_PORT_ATTR_PRIORITY_FLOW_CONTROL, 0),
    (sys::SAI_PORT_ATTR_UPDATE_DSCP, 0),
    (sys::SAI_PORT_ATTR_FULL_DUPLEX_MODE, 1),
    (sys::SAI_PORT_ATTR_IPG, IFG_ETHERNET as i64),
    (sys::SAI_PORT_ATTR_PKT_TX_ENABLE, 1),
    (sys::SAI_PORT_ATTR_HOST_TX_SIGNAL_ENABLE, 0),
    (sys::SAI_PORT_ATTR_AUTO_NEG_FEC_MODE_OVERRIDE, 0),
];

/// The value of a kept-only attribute's C++ default, as the metadata's kind (C++
/// `stub_default`). `None` for a kind a stub table does not use.
pub(crate) fn stub_value(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t, def: i64) -> Option<Value> {
    use sai_meta::ValueType;
    let meta = sai_meta::attribute(object_type, attr)?;
    Some(match meta.value_type {
        ValueType::Bool => Value::Bool(def != 0),
        ValueType::U8 => Value::U8(u8::try_from(def).ok()?),
        ValueType::U16 => Value::U16(u16::try_from(def).ok()?),
        ValueType::U32 => Value::U32(u32::try_from(def).ok()?),
        ValueType::U64 => Value::U64(u64::try_from(def).ok()?),
        ValueType::I32 => Value::I32(i32::try_from(def).ok()?),
        ValueType::Enum => Value::Enum(i32::try_from(def).ok()?),
        ValueType::Oid => Value::Oid(RawOid(u64::try_from(def).ok()?)),
        _ => return None,
    })
}

// ------------------------------------------------------------------ quirks

/// An average inter-frame gap in bit times: a multiple of 8 in 64..=504.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ifg(u32);

impl Ifg {
    /// The gap, if valid.
    pub fn new(bits: u32) -> Option<Self> {
        ((64..=504).contains(&bits) && bits.is_multiple_of(8)).then_some(Self(bits))
    }

    /// Bit times.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// Board data the SDK configuration cannot carry (C++ `PortQuirks`), for every front-panel
/// port. `None`: left to the SDK (interface, IFG) or to SAI (FEC, autoneg).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortQuirks {
    /// `SAI_OPENBCM_PORT_INTERFACE`
    pub interface: Option<PortInterface>,
    /// `SAI_OPENBCM_PORT_FEC`: the ASIC FEC is fixed; SAI's `FEC_MODE` is kept only.
    pub fec: Option<Fec>,
    /// `SAI_OPENBCM_PORT_AUTONEG`: fixed; SAI's `AUTO_NEG_MODE` is kept only.
    pub autoneg: Option<bool>,
    /// `SAI_OPENBCM_PORT_IFG`
    pub ifg: Option<Ifg>,
}

impl PortQuirks {
    /// The quirks of the profile (C++ `port_quirks_parse`): a value that does not parse refuses
    /// the switch (`INVALID_PARAMETER`, with the C++ log line); an empty value is no quirk.
    pub fn parse(profile: &Profile) -> Result<Self> {
        let mut q = Self::default();
        let value = keys::get(profile, keys::PORT_INTERFACE);
        if !value.is_empty() {
            q.interface = Some(PortInterface::from_name(value).ok_or_else(|| {
                log::error(
                    sys::SAI_API_SWITCH,
                    format_args!("SAI_OPENBCM_PORT_INTERFACE '{value}': unknown interface type"),
                );
                Error::InvalidParameter
            })?);
        }
        q.fec = match keys::get(profile, keys::PORT_FEC) {
            "" => None,
            "none" => Some(Fec::None),
            "rs" => Some(Fec::Rs),
            "fc" => Some(Fec::Fc),
            other => {
                log::error(
                    sys::SAI_API_SWITCH,
                    format_args!("SAI_OPENBCM_PORT_FEC '{other}': none|rs|fc"),
                );
                return Err(Error::InvalidParameter);
            }
        };
        q.autoneg = match keys::get(profile, keys::PORT_AUTONEG) {
            "" => None,
            "off" => Some(false),
            "on" => Some(true),
            other => {
                log::error(
                    sys::SAI_API_SWITCH,
                    format_args!("SAI_OPENBCM_PORT_AUTONEG '{other}': off|on"),
                );
                return Err(Error::InvalidParameter);
            }
        };
        let value = keys::get(profile, keys::PORT_IFG);
        if !value.is_empty() {
            q.ifg = Some(
                keys::strict_number(value)
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .and_then(Ifg::new)
                    .ok_or_else(|| {
                        log::error(
                            sys::SAI_API_SWITCH,
                            format_args!("SAI_OPENBCM_PORT_IFG '{value}': bit times, a multiple of 8 in 64..504"),
                        );
                        Error::InvalidParameter
                    })?,
            );
        }
        if q.any() {
            log::notice(
                sys::SAI_API_SWITCH,
                format_args!(
                    "port quirks: interface {}, ASIC FEC {}, autoneg {}, IFG {}{}",
                    q.interface.map_or("(SDK)", PortInterface::name),
                    match q.fec {
                        None => "from SAI",
                        Some(Fec::Rs) => "fixed rs",
                        Some(Fec::Fc) => "fixed fc",
                        Some(Fec::None) => "fixed none",
                    },
                    match q.autoneg {
                        None => "from SAI",
                        Some(true) => "fixed on",
                        Some(false) => "fixed off",
                    },
                    q.ifg.map_or_else(|| "(SDK)".to_owned(), |g| g.bits().to_string()),
                    if q.ifg.is_some() { " bits" } else { "" },
                ),
            );
        }
        Ok(q)
    }

    /// Whether any quirk is set.
    pub const fn any(&self) -> bool {
        self.interface.is_some() || self.fec.is_some() || self.autoneg.is_some() || self.ifg.is_some()
    }
}

/// Applies the board quirks to a front-panel port (C++ `port_quirks_apply`), in the order of the
/// diag shell's `port ... an=off if=...`: autoneg, interface, FEC, then the inter-frame gap.
/// Every quirk is tried; a refused one is logged. `false`: one was refused (the switch is
/// then not created).
pub(crate) fn port_quirks_apply<B: Backend + ?Sized>(
    backend: &mut B,
    port: &mut Port,
    quirks: &PortQuirks,
    macsec_ifg: u32,
) -> bool {
    if !matches!(port.kind, PortKind::Front { .. }) {
        return true;
    }
    let mut ok = true;
    let mut check = |what: &str, done: Result<()>| {
        if let Err(e) = done {
            log::error(
                sys::SAI_API_PORT,
                format_args!("port {}: quirk {what} not applied ({})", port.name, e.status()),
            );
            ok = false;
        }
    };
    if let Some(on) = quirks.autoneg {
        check("autoneg", backend.port_autoneg_set(port.hw, on));
    }
    if let Some(interface) = quirks.interface {
        check("interface", backend.port_interface_set(port.hw, interface));
    }
    if let Some(fec) = quirks.fec {
        check("fec", backend.port_fec_set(port.hw, fec));
    }
    let ifg = port_ifg_set(backend, port, quirks, macsec_ifg).is_ok();
    ok && ifg
}

/// The inter-frame gap a port gets (C++ `port_ifg_apply`): the `MACsec` one (`macsec_ifg`, from
/// the external `MACsec` table while the port has a `MACsec` port object), else the quirk's, else
/// back to the Ethernet 96 bit times if the `MACsec` one was programmed, else nothing. A refusal
/// is logged and returned.
fn port_ifg_set<B: Backend + ?Sized>(
    backend: &mut B,
    port: &mut Port,
    quirks: &PortQuirks,
    macsec_ifg: u32,
) -> Result<()> {
    if !matches!(port.kind, PortKind::Front { .. }) {
        return Ok(());
    }
    let want = if macsec_ifg != 0 {
        macsec_ifg
    } else if let Some(ifg) = quirks.ifg {
        ifg.bits()
    } else if port.macsec_ifg {
        IFG_ETHERNET
    } else {
        0
    };
    port.macsec_ifg = macsec_ifg != 0;
    if want == 0 {
        return Ok(());
    }
    backend.port_ifg_set(port.hw, want).inspect_err(|e| {
        log::error(
            sys::SAI_API_PORT,
            format_args!(
                "port {}: inter-frame gap {want} bit times not applied ({})",
                port.name,
                e.status()
            ),
        );
    })
}

/// Re-applies a port's inter-frame gap after its `MACsec` state changed (C++ `port_ifg_apply`,
/// called by the `MACsec` area when a `MACSEC_PORT` comes or goes). Only the gap: re-applying
/// the interface or autoneg quirk to a port that is up would take its link down for a second.
#[allow(
    dead_code,
    reason = "called by the MACsec area (A7) when a MACSEC_PORT comes or goes"
)]
pub(crate) fn port_ifg_apply<B: Backend + ?Sized>(backend: &mut B, store: &mut Store, port: PortId) -> Result<()> {
    let quirks = quirks_of(store);
    let hw = store.get::<Port>(port)?.hw;
    let macsec_ifg = macsec::macsec_port_ifg(store, hw);
    let mut changed = store.get::<Port>(port)?.clone();
    let done = port_ifg_set(backend, &mut changed, &quirks, macsec_ifg);
    let flag = changed.macsec_ifg;
    store.update::<Port, _>(port, |p| p.macsec_ifg = flag)?;
    done
}

/// The switch's board quirks (none before the switch exists).
fn quirks_of(store: &Store) -> PortQuirks {
    store.iter::<Switch>().next().map(|(_, s)| s.quirks).unwrap_or_default()
}

// --------------------------------------------------------------- set / get

/// `SAI_PORT_FEC_MODE_*` as the chip's FEC (C++ `to_fec`, after the range check).
fn fec_of(mode: i32) -> Option<Fec> {
    match u32::try_from(mode).ok()? {
        sys::SAI_PORT_FEC_MODE_NONE => Some(Fec::None),
        sys::SAI_PORT_FEC_MODE_RS => Some(Fec::Rs),
        sys::SAI_PORT_FEC_MODE_FC => Some(Fec::Fc),
        _ => None,
    }
}

/// The chip's FEC as `SAI_PORT_FEC_MODE_*`.
pub(crate) fn fec_mode(fec: Fec) -> i32 {
    match fec {
        Fec::None => sys::SAI_PORT_FEC_MODE_NONE,
        Fec::Rs => sys::SAI_PORT_FEC_MODE_RS,
        Fec::Fc => sys::SAI_PORT_FEC_MODE_FC,
    }
    .cast_signed()
}

/// `SAI_PORT_LOOPBACK_MODE_*` as the chip's loopback (none, MAC, PHY; the remote ones are not).
fn loopback_of(mode: i32) -> Option<Loopback> {
    match u32::try_from(mode).ok()? {
        sys::SAI_PORT_LOOPBACK_MODE_NONE => Some(Loopback::None),
        sys::SAI_PORT_LOOPBACK_MODE_MAC => Some(Loopback::Mac),
        sys::SAI_PORT_LOOPBACK_MODE_PHY => Some(Loopback::Phy),
        _ => None,
    }
}

/// The chip's loopback as `SAI_PORT_LOOPBACK_MODE_*`.
pub(crate) fn loopback_mode(lb: Loopback) -> i32 {
    match lb {
        Loopback::None => sys::SAI_PORT_LOOPBACK_MODE_NONE,
        Loopback::Mac => sys::SAI_PORT_LOOPBACK_MODE_MAC,
        Loopback::Phy => sys::SAI_PORT_LOOPBACK_MODE_PHY,
    }
    .cast_signed()
}

fn invalid() -> Error {
    Error::attr(AttrError::InvalidValue, 0)
}

fn not_supported() -> Error {
    Error::attr(AttrError::NotSupported, 0)
}

/// Sets one attribute of a port that exists (C++ `port_set_one`; errors carry index 0). The
/// backend first, the model after its success.
#[allow(clippy::too_many_lines, reason = "one arm per attribute, in the C++ order")]
pub(crate) fn set<B: Backend>(
    backend: &mut B,
    store: &mut Store,
    switch: SwitchId,
    id: PortId,
    attr: &Attr,
) -> Result<()> {
    let _ = switch;
    let port = store.get::<Port>(id)?;
    if !matches!(port.kind, PortKind::Front { .. }) {
        return Err(not_supported());
    }
    let hw = port.hw;
    let name = port.name.clone();
    match (attr.id, &attr.value) {
        (sys::SAI_PORT_ATTR_ADMIN_STATE, Value::Bool(up)) => {
            let up = *up;
            // the external PHY's PHYs, claimed before anything is changed (`extphy`, "Claims")
            let _claim = extphy::claim_admin(store, hw)?;
            // The external PHY's line side comes on before the MAC and goes off after it.
            if up {
                extphy::ext_phy_admin(backend, store, hw, true);
            }
            let done = backend.port_admin_set(hw, up);
            if if done.is_err() { up } else { !up } {
                extphy::ext_phy_admin(backend, store, hw, false);
            }
            done.map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.admin = up)?;
            log::notice(
                sys::SAI_API_PORT,
                format_args!("port {name} admin {}", if up { "up" } else { "down" }),
            );
            Ok(())
        }
        (sys::SAI_PORT_ATTR_SPEED, Value::U32(mbps)) => {
            if *mbps == 0 {
                return Err(invalid());
            }
            backend.port_speed_set(hw, *mbps).map_err(|e| e.for_attr(0))?;
            // A speed change re-does the MAC / SerDes setup: the board quirks again (a refusal
            // is logged; the speed change itself succeeded).
            let quirks = quirks_of(store);
            let macsec_ifg = macsec::macsec_port_ifg(store, hw);
            store.update::<Port, _>(id, |p| {
                p.speed = *mbps;
                port_quirks_apply(backend, p, &quirks, macsec_ifg)
            })?;
            Ok(())
        }
        (sys::SAI_PORT_ATTR_MTU, Value::U32(mtu)) => {
            if !(PORT_MIN_MTU..=PORT_MAX_MTU).contains(mtu) {
                return Err(invalid());
            }
            // SAI's MTU excludes FCS and VLAN tag; the SDK's limit is the whole frame.
            backend
                .port_frame_max_set(hw, mtu + FRAME_OVERHEAD)
                .map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.mtu = *mtu)
        }
        (sys::SAI_PORT_ATTR_FEC_MODE, Value::Enum(mode)) => {
            let fec = fec_of(*mode).ok_or_else(invalid)?;
            if quirks_of(store).fec.is_some() {
                // FEC is another chip's job on this board: kept, not programmed.
                store.update::<Port, _>(id, |p| p.fec = fec)?;
                log::info(
                    sys::SAI_API_PORT,
                    format_args!("port {name} FEC mode {mode} kept (SAI_OPENBCM_PORT_FEC fixes the ASIC side)"),
                );
                return Ok(());
            }
            backend.port_fec_set(hw, fec).map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.fec = fec)
        }
        (sys::SAI_PORT_ATTR_AUTO_NEG_MODE, Value::Bool(on)) => {
            if quirks_of(store).autoneg.is_some() {
                store.update::<Port, _>(id, |p| p.autoneg = *on)?;
                log::info(
                    sys::SAI_API_PORT,
                    format_args!(
                        "port {name} autoneg {} kept (SAI_OPENBCM_PORT_AUTONEG fixes the ASIC side)",
                        i32::from(*on)
                    ),
                );
                return Ok(());
            }
            backend.port_autoneg_set(hw, *on).map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.autoneg = *on)
        }
        (sys::SAI_PORT_ATTR_PORT_VLAN_ID, Value::U16(vid)) => {
            let vid = Vid::new(*vid).map_err(|_| invalid())?;
            backend.port_pvid_set(hw, vid).map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.pvid = vid)
        }
        (sys::SAI_PORT_ATTR_LOOPBACK_MODE, Value::Enum(mode)) => {
            let lb = loopback_of(*mode).ok_or_else(invalid)?;
            backend.port_loopback_set(hw, lb).map_err(|e| e.for_attr(0))?;
            store.update::<Port, _>(id, |p| p.loopback = lb)
        }
        (sys::SAI_PORT_ATTR_INGRESS_ACL | sys::SAI_PORT_ATTR_EGRESS_ACL, Value::Oid(table)) => acl::acl_bind_set(
            backend,
            store,
            id.raw(),
            attr.id == sys::SAI_PORT_ATTR_INGRESS_ACL,
            *table,
        ),
        _ => {
            if let Some(done) = qos::port_qos_set(backend, store, id, attr) {
                return done;
            }
            if let Some(done) = hash::port_storm_attr_set(backend, store, id, attr) {
                return done;
            }
            if let Some(done) = mirror::port_mirror_set(backend, store, id, attr) {
                return done;
            }
            if let Some(done) = macsec::port_macsec_set(backend, store, id, attr) {
                return done;
            }
            if PORT_STUBS.iter().any(|(stub, _)| *stub == attr.id) {
                store.update::<Port, _>(id, |p| drop(p.kept.insert(attr.id, attr.value.clone())))?;
                log::info(
                    sys::SAI_API_PORT,
                    format_args!("port {name} attr {} stored (not programmed)", attr.id),
                );
                return Ok(());
            }
            log::warn(sys::SAI_API_PORT, format_args!("port attr {} not supported", attr.id));
            Err(not_supported())
        }
    }
}

/// The status of a set that the rim refused before decoding (an attribute the metadata does not
/// let a set carry): what the C++ answers for it, `ATTR_NOT_SUPPORTED` (C++ `port_set_one`'s
/// default and the CPU port's answer).
pub(crate) const fn refused_set() -> Error {
    Error::Attr(AttrError::NotSupported, 0)
}

/// Gets one attribute of a port that exists (C++ `port_get_one`; errors carry index 0).
pub(crate) fn get<B: Backend>(backend: &B, store: &Store, id: PortId, attr: sys::sai_attr_id_t) -> Result<Value> {
    let port = store.get::<Port>(id)?;
    let front = match &port.kind {
        PortKind::Front { .. } => Some(port.hw),
        PortKind::Cpu => None,
    };
    let lanes = match &port.kind {
        PortKind::Front { lanes } => lanes.clone(),
        PortKind::Cpu => Vec::new(),
    };
    Ok(match attr {
        sys::SAI_PORT_ATTR_TYPE => Value::Enum(
            if front.is_some() {
                sys::SAI_PORT_TYPE_LOGICAL
            } else {
                sys::SAI_PORT_TYPE_CPU
            }
            .cast_signed(),
        ),
        sys::SAI_PORT_ATTR_HW_LANE_LIST => Value::U32List(lanes),
        sys::SAI_PORT_ATTR_ADMIN_STATE => Value::Bool(port.admin),
        sys::SAI_PORT_ATTR_OPER_STATUS => {
            let up = match front {
                None => true,
                Some(hw) => backend.port_link_get(hw)?,
            };
            Value::Enum(
                if up {
                    sys::SAI_PORT_OPER_STATUS_UP
                } else {
                    sys::SAI_PORT_OPER_STATUS_DOWN
                }
                .cast_signed(),
            )
        }
        sys::SAI_PORT_ATTR_SPEED | sys::SAI_PORT_ATTR_OPER_SPEED => {
            Value::U32(backend.port_speed_get(front.ok_or_else(not_supported)?)?)
        }
        sys::SAI_PORT_ATTR_SUPPORTED_SPEED => {
            Value::U32List(backend.port_supported_speeds(front.ok_or_else(not_supported)?)?)
        }
        sys::SAI_PORT_ATTR_MTU => Value::U32(port.mtu),
        sys::SAI_PORT_ATTR_FEC_MODE | sys::SAI_PORT_ATTR_OPER_PORT_FEC_MODE => Value::Enum(fec_mode(port.fec)),
        sys::SAI_PORT_ATTR_SUPPORTED_FEC_MODE => Value::I32List([Fec::None, Fec::Rs, Fec::Fc].map(fec_mode).to_vec()),
        sys::SAI_PORT_ATTR_AUTO_NEG_MODE => Value::Bool(port.autoneg),
        sys::SAI_PORT_ATTR_SUPPORTED_AUTO_NEG_MODE => Value::Bool(front.is_some()),
        sys::SAI_PORT_ATTR_PORT_VLAN_ID => Value::U16(port.pvid.get()),
        sys::SAI_PORT_ATTR_LOOPBACK_MODE => Value::Enum(loopback_mode(port.loopback)),
        sys::SAI_PORT_ATTR_QOS_NUMBER_OF_QUEUES => count_of(&children(store, ObjectType::Queue, id))?,
        sys::SAI_PORT_ATTR_QOS_QUEUE_LIST => Value::OidList(children(store, ObjectType::Queue, id)),
        sys::SAI_PORT_ATTR_NUMBER_OF_INGRESS_PRIORITY_GROUPS => {
            count_of(&children(store, ObjectType::IngressPriorityGroup, id))?
        }
        sys::SAI_PORT_ATTR_INGRESS_PRIORITY_GROUP_LIST => {
            Value::OidList(children(store, ObjectType::IngressPriorityGroup, id))
        }
        sys::SAI_PORT_ATTR_INGRESS_ACL | sys::SAI_PORT_ATTR_EGRESS_ACL => Value::Oid(acl::acl_bind_get(
            store,
            id.raw(),
            attr == sys::SAI_PORT_ATTR_INGRESS_ACL,
        )),
        _ => return get_other(backend, store, id, port, attr),
    })
}

/// The handlers of other areas, then the kept attributes, in the C++ order.
fn get_other<B: Backend>(
    backend: &B,
    store: &Store,
    id: PortId,
    port: &Port,
    attr: sys::sai_attr_id_t,
) -> Result<Value> {
    if let Some(value) = qos::port_qos_get(backend, store, id, attr) {
        return value;
    }
    if let Some(value) = hash::port_storm_attr_get(store, id, attr) {
        return value;
    }
    if let Some(value) = mirror::port_mirror_get(backend, store, id, attr) {
        return value;
    }
    if let Some(value) = macsec::port_macsec_get(backend, store, id, attr) {
        return value;
    }
    // The scheduler groups (one per queue) until the QoS area answers them (C++ port_qos_get).
    match attr {
        sys::SAI_PORT_ATTR_QOS_NUMBER_OF_SCHEDULER_GROUPS => {
            return count_of(&children(store, ObjectType::SchedulerGroup, id));
        }
        sys::SAI_PORT_ATTR_QOS_SCHEDULER_GROUP_LIST => {
            return Ok(Value::OidList(children(store, ObjectType::SchedulerGroup, id)));
        }
        _ => {}
    }
    if let Some((_, def)) = PORT_STUBS.iter().find(|(stub, _)| *stub == attr) {
        return match port.kept.get(&attr) {
            Some(value) => Ok(value.clone()),
            None => stub_value(sys::SAI_OBJECT_TYPE_PORT, attr, *def).ok_or(Error::Failure),
        };
    }
    Err(not_supported())
}

fn count_of(list: &[RawOid]) -> Result<Value> {
    u32::try_from(list.len()).map(Value::U32).map_err(|_| Error::Failure)
}

/// The queues, priority groups or scheduler groups of a port: the objects of that type whose
/// port attribute names it, in id order (queue / group index order).
fn children(store: &Store, object_type: ObjectType, port: PortId) -> Vec<RawOid> {
    let attr = match object_type {
        ObjectType::Queue => sys::SAI_QUEUE_ATTR_PORT,
        ObjectType::IngressPriorityGroup => sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT,
        _ => sys::SAI_SCHEDULER_GROUP_ATTR_PORT_ID,
    };
    store
        .bags(object_type)
        .filter(|(_, bag)| bag.attrs.get(&attr) == Some(&Value::Oid(port.raw())))
        .map(|(id, _)| id)
        .collect()
}

// ---------------------------------------------------------------- counters

/// The counter a `SAI_PORT_STAT_IF_*` id names (C++ `map_stat`).
pub fn port_stat(id: sys::sai_stat_id_t) -> Option<PortStat> {
    PortStat::ALL.iter().copied().find(|s| s.sai_id() == id)
}

/// Whether a port counter can be read (C++ `port_stat_supported`): one the backend reads by id,
/// a PFC counter with `QoS`, or one of the 15 `IF_*` counters. Without a backend (no switch):
/// the 15.
pub fn port_stat_supported<B: Backend + ?Sized>(backend: Option<&B>, id: sys::sai_stat_id_t) -> bool {
    if backend.is_some_and(|b| b.port_stat_by_id(id)) {
        return true;
    }
    if qos::port_pfc_stat(id).is_some() {
        return backend.is_some_and(crate::backend::Qos::qos_supported);
    }
    port_stat(id).is_some()
}

/// The port of a counter call: a front-panel port that exists (C++: else `INVALID_OBJECT_ID`).
fn counted_port(store: &Store, oid: RawOid) -> Result<HwPort> {
    let id = PortId::try_from(oid).map_err(|_| Error::InvalidObjectId)?;
    match store.get::<Port>(id) {
        Ok(Port {
            kind: PortKind::Front { .. },
            hw,
            ..
        }) => Ok(*hw),
        _ => Err(Error::InvalidObjectId),
    }
}

/// `get_port_stats` / `get_port_stats_ext` (C++ `port_stats_get`): `clear`
/// (`READ_AND_CLEAR`) is `NOT_SUPPORTED` before anything is looked at; the values in the order
/// of `ids`; the first failure is the answer.
pub(crate) fn stats_get<B: Backend>(
    backend: &B,
    store: &Store,
    oid: RawOid,
    ids: &[sys::sai_stat_id_t],
    clear: bool,
) -> Result<Vec<u64>> {
    if clear {
        return Err(Error::NotSupported); // no per-counter clear in the SDK
    }
    let hw = counted_port(store, oid)?;
    let mut out = vec![0_u64; ids.len()];
    // The counters the backend reads by SAI id: one call for all of them.
    let (at, by_id): (Vec<usize>, Vec<sys::sai_stat_id_t>) = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| backend.port_stat_by_id(**id))
        .map(|(i, id)| (i, *id))
        .unzip();
    if !by_id.is_empty() {
        let values = backend.port_stats_by_id(hw, &by_id)?;
        for (i, value) in at.iter().zip(values) {
            if let Some(slot) = out.get_mut(*i) {
                *slot = value;
            }
        }
    }
    for (slot, id) in out.iter_mut().zip(ids) {
        if !by_id.is_empty() && backend.port_stat_by_id(*id) {
            continue;
        }
        if let Some((prio, tx)) = qos::port_pfc_stat(*id) {
            // PFC frames: a QoS call
            if !backend.qos_supported() {
                return Err(Error::NotSupported);
            }
            *slot = backend.port_pfc_stat_get(hw, prio, tx)?;
            continue;
        }
        *slot = backend.port_stat_get(hw, port_stat(*id).ok_or(Error::NotSupported)?)?;
    }
    Ok(out)
}

/// `clear_port_all_stats`.
pub(crate) fn clear_all_stats<B: Backend>(backend: &mut B, store: &Store, oid: RawOid) -> Result<()> {
    backend.port_stat_clear(counted_port(store, oid)?)
}

/// The counters `sai_query_stats_capability` lists for ports (C++: ids 0..0x200 that
/// [`port_stat_supported`]), each `READ` only.
pub fn stats_capability<B: Backend + ?Sized>(backend: Option<&B>) -> Vec<sys::sai_stat_id_t> {
    (0..0x200).filter(|id| port_stat_supported(backend, *id)).collect()
}

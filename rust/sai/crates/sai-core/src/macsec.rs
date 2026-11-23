//! The `MACsec` seam of the core: the `MACSEC`, `MACSEC_PORT`, `MACSEC_FLOW`, `MACSEC_SC`,
//! `MACSEC_SA` objects and the `MACsec`-stage ACL tables / entries go to a `MACsec` model behind
//! [`MacsecModel`]; the port's inter-frame gap follows `MACsec` state ([`macsec_port_ifg`], asked
//! by [`crate::port::port_ifg_apply`]); the statistics capability of SA and flow. Ports the
//! glue half of `sai_macsec.cpp` / `sai_macsec_rust.cpp` (`macsec_acl_*`, `port_macsec_set` /
//! `get`, `macsec_port_ifg`, `macsec_stats_capability`, `ext_macsec_load` / `attach` /
//! `detach` / `status` / `trace` / `diag_run`).
//!
//! # Why a trait here and the model elsewhere
//!
//! The model is the `sai-macsec` crate (`sai_macsec.cpp`'s port, held to the C++ by its
//! differential tests) and runs the board's table through `sai-macsec-phy` (over `phy-seq`).
//! `sai-macsec` depends on this crate (its ids, errors and statuses are this crate's), so this
//! crate cannot depend on it. The dependency stays one way: this module defines what the core
//! needs ([`MacsecModel`], [`MacsecLoader`]) and what it offers the model during a call
//! ([`MacsecIo`]: the bus under the external-PHY lock, the port's gap); `sai-macsec-phy`'s
//! `native` module implements them, and the rim (`sai-ffi`) hands its loader to the core
//! ([`crate::extphy::Hooks`]). No C ABI and no C++ in between. A core without a loader (tests of
//! other areas) answers every `MACsec` call `NOT_SUPPORTED`.
//!
//! # Key material
//!
//! [`Secret`] (SAK, salt, `AUTH_KEY`: [`crate::value::Value::MacsecSak`] ...) is decoded once by
//! the rim, straight from the caller's union member into the heap; it is wiped on drop, its
//! `Debug` prints the size only, and its equality is constant-time (no early exit on the first
//! differing byte). The model reads the bytes once, when it installs the SA, and never hands
//! them back (the attributes are write-only).
//!
//! # State
//!
//! Per switch, in the store ([`crate::extphy::External`]): the model, the gap each port asked
//! for, and the clear-window hold (`SAI_OPENBCM_MACSEC_HOLD_CLOSED`, designed, off).
use crate::backend::types::{MdioDev, PhyAddr};
use crate::backend::{Backend, HwPort};
use crate::error::{AttrError, Error, Result};
use crate::extphy::{Clock, ExtPhy};
use crate::model::{Port, PortKind};
use crate::oid::{PortId, RawOid, TypedId};
use crate::sai::Sai;
use crate::store::Store;
use crate::value::{Attr, Value};
use sai_meta::AttrMeta;
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use zeroize::Zeroize;

// ------------------------------------------------------------------ key material

/// `N` secret bytes on the heap: SAK (32), salt (12), `AUTH_KEY` (16).
///
/// - Overwritten with zeros when dropped (the `zeroize` crate's volatile writes).
/// - `Debug` shows the size only; there is no `Display`.
/// - `==` compares in constant time (every byte, no early exit).
///
/// Limit: a copy the caller made before handing the bytes over is the caller's to wipe;
/// [`Clone`] makes a second secret, wiped on its own drop.
pub struct Secret<const N: usize>(Box<[u8; N]>);

impl<const N: usize> Secret<N> {
    /// Copies `bytes` (exactly `N` of them) into a new secret.
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != N {
            return None;
        }
        let mut owned = Box::new([0u8; N]);
        owned.copy_from_slice(bytes);
        Some(Self(owned))
    }

    /// The bytes. Named to be easy to find in a review: every use is a place key bytes flow to.
    pub fn expose(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> Clone for Secret<N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<const N: usize> Secret<N> {
    /// Overwrites the bytes with zeros now (what dropping does).
    pub fn wipe(&mut self) {
        self.0.zeroize();
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl<const N: usize> fmt::Debug for Secret<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret<{N}>(redacted)")
    }
}

impl<const N: usize> PartialEq for Secret<N> {
    /// Constant time: every byte is looked at whatever the earlier ones were.
    fn eq(&self, other: &Self) -> bool {
        let diff = self.0.iter().zip(other.0.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b));
        core::hint::black_box(diff) == 0
    }
}

impl<const N: usize> Eq for Secret<N> {}

// ------------------------------------------------------------------ the objects

/// An object type the `MACsec` model owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MacsecObject {
    /// `MACSEC` (one per direction).
    Macsec,
    /// `MACSEC_PORT`
    Port,
    /// `MACSEC_FLOW`
    Flow,
    /// `MACSEC_SC`
    Sc,
    /// `MACSEC_SA`
    Sa,
    /// An `ACL_TABLE` of a `MACsec` stage.
    AclTable,
    /// An `ACL_ENTRY` of such a table.
    AclEntry,
}

impl MacsecObject {
    /// The SAI object type.
    pub const fn sai(self) -> sys::sai_object_type_t {
        match self {
            Self::Macsec => sys::SAI_OBJECT_TYPE_MACSEC,
            Self::Port => sys::SAI_OBJECT_TYPE_MACSEC_PORT,
            Self::Flow => sys::SAI_OBJECT_TYPE_MACSEC_FLOW,
            Self::Sc => sys::SAI_OBJECT_TYPE_MACSEC_SC,
            Self::Sa => sys::SAI_OBJECT_TYPE_MACSEC_SA,
            Self::AclTable => sys::SAI_OBJECT_TYPE_ACL_TABLE,
            Self::AclEntry => sys::SAI_OBJECT_TYPE_ACL_ENTRY,
        }
    }

    /// The five types of the `MACSEC` API (the ACL ones come through the ACL API).
    pub const fn of_macsec_api(object_type: sys::sai_object_type_t) -> Option<Self> {
        match object_type {
            sys::SAI_OBJECT_TYPE_MACSEC => Some(Self::Macsec),
            sys::SAI_OBJECT_TYPE_MACSEC_PORT => Some(Self::Port),
            sys::SAI_OBJECT_TYPE_MACSEC_FLOW => Some(Self::Flow),
            sys::SAI_OBJECT_TYPE_MACSEC_SC => Some(Self::Sc),
            sys::SAI_OBJECT_TYPE_MACSEC_SA => Some(Self::Sa),
            _ => None,
        }
    }
}

/// Whether an object id is of a type the `MACsec` model keeps (not the store): the core's
/// reference checks leave such ids to the model (an ACL entry's `ACTION_MACSEC_FLOW`).
pub(crate) fn held_by_model(id: RawOid) -> bool {
    MacsecObject::of_macsec_api(id.sai_object_type()).is_some()
}

/// The side of a port's `MACsec` ACL (`INGRESS_MACSEC_ACL` / `EGRESS_MACSEC_ACL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AclSide {
    /// `SAI_PORT_ATTR_INGRESS_MACSEC_ACL`
    Ingress,
    /// `SAI_PORT_ATTR_EGRESS_MACSEC_ACL`
    Egress,
}

/// How counters are read (`*_stats` = read, `*_stats_ext` with `READ_AND_CLEAR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsMode {
    /// `SAI_STATS_MODE_READ`
    Read,
    /// `SAI_STATS_MODE_READ_AND_CLEAR`
    ReadAndClear,
}

/// A port of the chip as the table is attached to it (before the SAI port objects exist).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipPortInfo {
    /// The chip's port.
    pub hw: HwPort,
    /// The SDK's name (`ce0`).
    pub name: String,
    /// Its lanes; the table names a port by its first lane.
    pub lanes: Vec<u32>,
}

/// Behaviour that is not in the board table (profile keys, [`crate::extphy`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacsecOptions {
    /// `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP=1` (designed for the owner's decision on the
    /// clear-traffic window, off by default): removing a `MACSEC_PORT` leaves its engine on
    /// (data frames dropped by the closed gate) instead of putting it into bypass, and the
    /// start-up clean-up resets an engine a previous process left on without putting it into
    /// bypass. Off: the C++ behaviour.
    pub engines_on_at_stop: bool,
}

// ------------------------------------------------------------------ what the model gets

/// The clause-45 bus for one sequence run, taken under the external-PHY lock
/// ([`MacsecIo::with_bus`]).
pub trait MacsecBus {
    /// `mdio_c45_read` on the backend.
    fn read(&mut self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16>;
    /// `mdio_c45_write` on the backend.
    fn write(&mut self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()>;
    /// Waits (`D` lines, the interval of a `P`): short waits spun, longer ones slept.
    fn delay_us(&mut self, us: u32);
}

/// What the core offers the model during one of its calls.
pub trait MacsecIo {
    /// Runs `run` with the bus, under the external-PHY table's lock: a sequence never
    /// interleaves with a link poll, an admin sequence or raw MDIO of the diag socket.
    fn with_bus(&mut self, run: &mut dyn FnMut(&mut dyn MacsecBus));
    /// The port's `MACsec` gap changed (`macsec_bits` != 0: it has a `MACSEC_PORT`, 0: no
    /// longer): the port code re-applies its gap (only the gap: C++ `port_ifg_apply`).
    fn set_ifg(&mut self, hw: HwPort, macsec_bits: u32) -> Result<()>;
    /// The id a new `MACsec`-stage ACL table / entry gets: the ACL area's, from the allocator
    /// the other ACL objects use (sai-macsec's D7); `None` for other objects (the model counts).
    fn next_id(&mut self, object: MacsecObject) -> Option<RawOid>;
    /// Whether the poll sequence `name` (`pn.*`, `stats-*` for a get) on `hw` is to be
    /// skipped now: raw MDIO of the diag socket holds the PHY polls off
    /// ([`ExtPhy::macsec_poll_deferred`], bounded). The default never skips.
    fn poll_deferred(&mut self, _hw: HwPort, _name: &str) -> bool {
        false
    }
}

/// The `MACsec` model of one switch (implemented by `sai-macsec-phy`'s `native` module over
/// `sai-macsec`). Statuses, check order and the sequences run are the model's: the core adds
/// nothing but the routing. Attribute errors carry the attribute's index.
pub trait MacsecModel: Send + fmt::Debug {
    /// A table is attached (`ext_macsec_active`).
    fn active(&self) -> bool;
    /// Creates an object.
    fn create(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, attrs: &[Attr]) -> Result<RawOid>;
    /// Removes an object.
    fn remove(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, id: RawOid) -> Result<()>;
    /// Sets one attribute.
    fn set(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, id: RawOid, attr: &Attr) -> Result<()>;
    /// Gets attributes (an SA's packet number is read from its PHY).
    fn get(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>>;
    /// Gets attributes of a `MACsec`-stage ACL table or entry (nothing is run).
    fn get_acl(&self, object: MacsecObject, id: RawOid, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>>;
    /// Reads counters.
    fn stats(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_stat_id_t],
        mode: StatsMode,
    ) -> Result<Vec<u64>>;
    /// Clears counters.
    fn clear_stats(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, id: RawOid) -> Result<()>;
    /// Whether `id` is one of the model's objects of that type.
    fn owns(&self, object: MacsecObject, id: RawOid) -> bool;
    /// The switch's front-panel ports, SAI id to chip port (handed over whenever they change).
    fn set_switch_ports(&mut self, ports: &[(PortId, HwPort)]);
    /// `INGRESS_MACSEC_ACL` / `EGRESS_MACSEC_ACL` set on a port (errors at index 0).
    fn set_port_acl(&mut self, io: &mut dyn MacsecIo, port: PortId, side: AclSide, table: RawOid) -> Result<()>;
    /// The table bound there, null when none.
    fn port_acl(&self, port: PortId, side: AclSide) -> RawOid;
    /// `MACSEC_PORT_LIST` of a port.
    fn port_macsec_ports(&self, port: PortId) -> Vec<RawOid>;
    /// The gap a port needs while it has a `MACSEC_PORT` (0: none).
    fn port_ifg(&self, hw: HwPort) -> u32;
    /// The ports with a `MACsec` engine in the table.
    fn engine_ports(&self) -> Vec<HwPort>;
    /// Whether the port has a `MACSEC_PORT` of either direction.
    fn has_macsec_port(&self, hw: HwPort) -> bool;
    /// `sai_query_stats_capability` for SA and flow.
    fn stats_capability(&self, object: MacsecObject) -> Result<Vec<sys::sai_stat_id_t>>;
    /// The diag socket's `extmacsec` text (never key bytes).
    fn status(&self) -> String;
    /// The diag socket's `extmacsec trace ARGS`.
    fn trace(&mut self, args: &str) -> String;
    /// The diag socket's `extmacsec run PORT SEQ` with the parameters of the root-only file.
    fn diag_run(&mut self, io: &mut dyn MacsecIo, port: &str, seq: &str, params: &[(&str, &[u8])]) -> String;
    /// At switch removal: forgets everything, runs nothing (engines stay on: sai-macsec's Q2).
    fn detach(&mut self);
}

/// A parsed table, attached once the backend is up.
pub trait MacsecPending: Send {
    /// `ext_macsec_attach`: the table's ports by first lane, the start-up clean-up of engines
    /// a previous process left on (sequences run through `io`).
    fn attach(
        self: Box<Self>,
        io: &mut dyn MacsecIo,
        ports: &[ChipPortInfo],
        options: MacsecOptions,
    ) -> Result<Box<dyn MacsecModel>>;
}

/// Makes the model of a switch: given to the core by the rim ([`crate::extphy::Hooks`]).
pub trait MacsecLoader: Send + Sync + fmt::Debug {
    /// `ext_macsec_load`: reads and parses the table at `path`
    /// (`SAI_OPENBCM_EXT_MACSEC_FILE`). Cannot open: `Failure`; not a table:
    /// `InvalidParameter`; both logged as the C++ does.
    fn load(&self, path: &str) -> Result<Box<dyn MacsecPending>>;
    /// The model of a switch without a table (every create `NOT_SUPPORTED`).
    fn without_table(&self) -> Box<dyn MacsecModel>;
}

// ------------------------------------------------------------------ per-switch state

/// The ports held closed until their `MACSEC_PORT` exists
/// (`SAI_OPENBCM_MACSEC_HOLD_CLOSED`): read by the link gates on the backend's link-scan
/// thread, so behind a lock of its own.
#[derive(Clone, Debug, Default)]
pub(crate) struct Hold(Arc<Mutex<BTreeSet<HwPort>>>);

impl Hold {
    pub(crate) fn closed(&self, hw: HwPort) -> bool {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).contains(&hw)
    }

    fn set(&self, closed: BTreeSet<HwPort>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = closed;
    }
}

/// The `MACsec` part of a switch.
#[derive(Debug, Default)]
pub struct MacsecState {
    /// `None`: no loader (this core has no `MACsec` model).
    pub(crate) model: Option<Box<dyn MacsecModel>>,
    /// The gap each port's `MACSEC_PORT`s ask for (what [`macsec_port_ifg`] answers).
    ifg: BTreeMap<HwPort, u32>,
    /// The ports last handed to the model.
    handed: Vec<(PortId, HwPort)>,
    /// The ports of `SAI_OPENBCM_MACSEC_HOLD_CLOSED` (empty: off) and the gates' view.
    pub(crate) hold_ports: BTreeSet<HwPort>,
    pub(crate) hold: Hold,
}

impl MacsecState {
    /// The state of a switch whose model is `model`.
    pub(crate) fn new(model: Option<Box<dyn MacsecModel>>, hold_ports: BTreeSet<HwPort>) -> Self {
        let state = Self {
            model,
            ifg: BTreeMap::new(),
            handed: Vec::new(),
            hold_ports,
            hold: Hold::default(),
        };
        state.refresh_hold();
        state
    }

    /// The gates' view: every held port without a `MACSEC_PORT` is closed.
    fn refresh_hold(&self) {
        let open = |hw: &HwPort| self.model.as_ref().is_some_and(|m| m.has_macsec_port(*hw));
        self.hold
            .set(self.hold_ports.iter().filter(|hw| !open(hw)).copied().collect());
    }
}

/// The bus of one sequence run: the backend's MDIO operations and the clock's waits.
struct BackendBus<'a, B: ?Sized> {
    backend: &'a B,
    clock: &'a dyn Clock,
}

impl<B: Backend + ?Sized> MacsecBus for BackendBus<'_, B> {
    fn read(&mut self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Result<u16> {
        self.backend.mdio_c45_read(phy, dev, reg)
    }
    fn write(&mut self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) -> Result<()> {
        self.backend.mdio_c45_write(phy, dev, reg, val)
    }
    fn delay_us(&mut self, us: u32) {
        self.clock.wait_us(us);
    }
}

/// [`MacsecIo`] over the backend, the store (without the model, taken out for the call) and
/// the external-PHY lock.
pub(crate) struct CoreIo<'a, B: ?Sized> {
    pub(crate) backend: &'a mut B,
    /// `None` while the switch is being created (no port objects yet).
    pub(crate) store: Option<&'a mut Store>,
    pub(crate) phy: ExtPhy,
    pub(crate) clock: Arc<dyn Clock>,
    /// The id the ACL area chose for the table / entry being created.
    pub(crate) alloc: Option<RawOid>,
}

impl<B: Backend + ?Sized> MacsecIo for CoreIo<'_, B> {
    fn with_bus(&mut self, run: &mut dyn FnMut(&mut dyn MacsecBus)) {
        let backend: &B = self.backend;
        let clock = &*self.clock;
        self.phy.locked(|| run(&mut BackendBus { backend, clock }));
    }

    fn set_ifg(&mut self, hw: HwPort, macsec_bits: u32) -> Result<()> {
        let Some(store) = self.store.as_deref_mut() else {
            return Err(Error::Failure);
        };
        if macsec_bits == 0 {
            store.ext_mut().macsec.ifg.remove(&hw);
        } else {
            store.ext_mut().macsec.ifg.insert(hw, macsec_bits);
        }
        let port = PortId::from_index(u64::from(hw.0));
        if store.get::<Port>(port).is_err() {
            return Ok(());
        }
        crate::port::port_ifg_apply(self.backend, store, port)
    }

    fn poll_deferred(&mut self, hw: HwPort, name: &str) -> bool {
        self.phy.macsec_poll_deferred(sai_extphy::ChipPort(hw.0), name)
    }

    fn next_id(&mut self, object: MacsecObject) -> Option<RawOid> {
        match object {
            MacsecObject::AclTable | MacsecObject::AclEntry => self.alloc.take(),
            _ => None,
        }
    }
}

/// The front-panel ports of the store, SAI id to chip port.
fn front_ports(store: &Store) -> Vec<(PortId, HwPort)> {
    store
        .iter::<Port>()
        .filter(|(_, p)| matches!(p.kind, PortKind::Front { .. }))
        .map(|(id, p)| (id, p.hw))
        .collect()
}

/// Runs `call` on the switch's model with the core's io: the model is taken out of the store
/// for the call (the io lends the store to the port code), handed the live port table first
/// (C++ `sync_ports`), and put back after; the hold is refreshed.
pub(crate) fn with_model<B: Backend + ?Sized, T>(
    backend: &mut B,
    store: &mut Store,
    call: impl FnOnce(&mut dyn MacsecModel, &mut dyn MacsecIo) -> Result<T>,
) -> Result<T> {
    with_model_alloc(backend, store, None, call)
}

/// [`with_model`] for a create whose id the caller chose.
fn with_model_alloc<B: Backend + ?Sized, T>(
    backend: &mut B,
    store: &mut Store,
    alloc: Option<RawOid>,
    call: impl FnOnce(&mut dyn MacsecModel, &mut dyn MacsecIo) -> Result<T>,
) -> Result<T> {
    let Some(mut model) = store.ext_mut().macsec.model.take() else {
        return Err(Error::NotSupported);
    };
    let ports = front_ports(store);
    if store.ext().macsec.handed != ports {
        model.set_switch_ports(&ports);
        store.ext_mut().macsec.handed = ports;
    }
    let phy = store.ext().phy.clone();
    let clock = phy.clock();
    let result = {
        let mut io = CoreIo {
            backend,
            store: Some(&mut *store),
            phy,
            clock,
            alloc,
        };
        call(model.as_mut(), &mut io)
    };
    let state = &mut store.ext_mut().macsec;
    state.model = Some(model);
    state.refresh_hold();
    result
}

// ------------------------------------------------------------------ the MACSEC API

impl<B: Backend> Sai<B> {
    fn macsec_parts(&mut self) -> Result<(&mut B, &mut Store)> {
        let cx = self.parts()?;
        Ok((cx.backend, cx.store))
    }

    /// `create_macsec*` (the switch id is the rim's: one switch).
    pub fn macsec_create(&mut self, object: MacsecObject, attrs: &[Attr]) -> Result<RawOid> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.create(io, object, attrs))
    }

    /// `remove_macsec*`
    pub fn macsec_remove(&mut self, object: MacsecObject, id: RawOid) -> Result<()> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.remove(io, object, id))
    }

    /// `set_macsec*_attribute`
    pub fn macsec_set(&mut self, object: MacsecObject, id: RawOid, attr: &Attr) -> Result<()> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.set(io, object, id, attr))
    }

    /// `get_macsec*_attribute`: the values in the order of `ids`.
    pub fn macsec_get(&mut self, object: MacsecObject, id: RawOid, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.get(io, object, id, ids))
    }

    /// `get_macsec*_stats` / `_stats_ext`.
    pub fn macsec_stats(
        &mut self,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_stat_id_t],
        mode: StatsMode,
    ) -> Result<Vec<u64>> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.stats(io, object, id, ids, mode))
    }

    /// `clear_macsec*_stats`
    pub fn macsec_clear_stats(&mut self, object: MacsecObject, id: RawOid) -> Result<()> {
        let (backend, store) = self.macsec_parts()?;
        with_model(backend, store, |m, io| m.clear_stats(io, object, id))
    }

    /// Whether a `MACsec` table is attached (the test port's `ext_macsec_active`).
    pub fn ext_macsec_active(&self) -> bool {
        self.store()
            .and_then(|s| s.ext().macsec.model.as_ref())
            .is_some_and(|m| m.active())
    }
}

// ------------------------------------------------------------------ the diag socket's commands

/// The text without a table (the C++'s).
const NO_TABLE: &str = "extmacsec: no external MACsec table (SAI_OPENBCM_EXT_MACSEC_FILE)\n";

/// The diag socket's `extmacsec` answer (C++ `ext_macsec_status`).
pub fn ext_macsec_status<B: Backend>(sai: &Sai<B>) -> String {
    match sai.store().and_then(|s| s.ext().macsec.model.as_ref()) {
        Some(model) if model.active() => model.status(),
        _ => NO_TABLE.to_owned(),
    }
}

/// The trace's usage line (C++ `ext_macsec_trace`).
const TRACE_USAGE: &str =
    "usage: extmacsec trace [on [all] | off | dump | clear]   (on: sequences that write; on all: every sequence)\n";

/// The diag socket's `extmacsec trace[ ARGS]` (C++ `ext_macsec_trace`); `args` is the text after
/// `extmacsec trace`. Without a switch only the usage check answers (the trace is the model's).
pub fn ext_macsec_trace<B: Backend>(sai: &mut Sai<B>, args: &str) -> String {
    let cmd = args.split_ascii_whitespace().next().unwrap_or_default();
    if !matches!(cmd, "" | "on" | "off" | "dump" | "clear") {
        return TRACE_USAGE.to_owned();
    }
    match sai.macsec_parts() {
        Ok((_, store)) => match store.ext_mut().macsec.model.as_mut() {
            Some(model) => model.trace(args),
            None => "extmacsec trace: off, engine rust (phy-seq), 0 runs, 0 lines kept, 0 dropped\n".to_owned(),
        },
        Err(_) => "extmacsec trace: off, engine rust (phy-seq), 0 runs, 0 lines kept, 0 dropped\n".to_owned(),
    }
}

/// The parameter file of `extmacsec run`: `name hex` pairs; it must exist, belong to root and
/// have no group or other permission bits (C++ `ext_macsec_diag_run`). The bytes are wiped when
/// the result is dropped.
/// The parameters of a diag run, by name; wiped on drop.
type DiagParams = Vec<(String, zeroize::Zeroizing<Vec<u8>>)>;

fn read_param_file(path: &str) -> core::result::Result<DiagParams, ()> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::metadata(path).map_err(|_| ())?;
    if meta.mode() & 0o077 != 0 || meta.uid() != 0 {
        return Err(());
    }
    let text = zeroize::Zeroizing::new(std::fs::read_to_string(path).map_err(|_| ())?);
    let words: Vec<&str> = text.split_ascii_whitespace().collect();
    Ok(words
        .chunks_exact(2)
        .filter_map(|pair| {
            let (name, hex) = (pair.first()?, pair.get(1)?);
            // two hex digits per byte, as the C++ strtoul of each pair (an odd last digit is dropped)
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|d| {
                    core::str::from_utf8(d)
                        .ok()
                        .and_then(|d| u8::from_str_radix(d, 16).ok())
                        .unwrap_or(0)
                })
                .collect();
            Some(((*name).to_owned(), zeroize::Zeroizing::new(bytes)))
        })
        .collect())
}

/// The diag socket's `extmacsec run PORT SEQUENCE [PARAMETER FILE]` (C++ `ext_macsec_diag_run`);
/// `param_file` is empty when none was given.
pub fn ext_macsec_diag_run<B: Backend>(sai: &mut Sai<B>, port: &str, seq: &str, param_file: &str) -> String {
    let Ok((backend, store)) = sai.macsec_parts() else {
        return "extmacsec: no external MACsec table\n".to_owned();
    };
    if !store.ext().macsec.model.as_ref().is_some_and(|m| m.active()) {
        return "extmacsec: no external MACsec table\n".to_owned();
    }
    let params = if param_file.is_empty() {
        Vec::new()
    } else {
        match read_param_file(param_file) {
            Ok(params) => params,
            Err(()) => {
                return "extmacsec: parameter file must exist, belong to root and be mode 0600 or 0400\n".to_owned();
            }
        }
    };
    let lent: Vec<(&str, &[u8])> = params.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    with_model(backend, store, |m, io| Ok(m.diag_run(io, port, seq, &lent))).unwrap_or_default()
}

// ------------------------------------------------------------------ hooks of the port and ACL areas

/// The average inter-frame gap (bit times) the port needs while it has a `MACSEC_PORT`; 0 = none
/// (C++ `macsec_port_ifg`; asked by [`crate::port::port_ifg_apply`] whenever the port's gap is
/// applied, so the gap survives a speed set).
pub(crate) fn macsec_port_ifg(store: &Store, hw: HwPort) -> u32 {
    store.ext().macsec.ifg.get(&hw).copied().unwrap_or(0)
}

/// `SAI_PORT_ATTR_INGRESS_MACSEC_ACL` / `EGRESS_MACSEC_ACL` set on a port (C++
/// `port_macsec_set`): `None` for another attribute. Errors carry index 0.
pub(crate) fn port_macsec_set<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: PortId,
    attr: &Attr,
) -> Option<Result<()>> {
    let side = match attr.id {
        sys::SAI_PORT_ATTR_INGRESS_MACSEC_ACL => AclSide::Ingress,
        sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL => AclSide::Egress,
        _ => return None,
    };
    let Value::Oid(table) = attr.value else {
        return Some(Err(Error::attr(AttrError::InvalidValue, 0)));
    };
    if store.ext().macsec.model.is_none() {
        // no model in this core: as the model without a table (null unbinds nothing)
        return Some(if table.is_null() {
            Ok(())
        } else {
            Err(Error::attr(AttrError::InvalidValue, 0))
        });
    }
    Some(with_model(backend, store, |m, io| m.set_port_acl(io, id, side, table)))
}

/// `INGRESS_MACSEC_ACL`, `EGRESS_MACSEC_ACL`, `MACSEC_PORT_LIST` of a port (C++
/// `port_macsec_get`): `None` for another attribute.
pub(crate) fn port_macsec_get<B: Backend + ?Sized>(
    backend: &B,
    store: &Store,
    id: PortId,
    attr: sys::sai_attr_id_t,
) -> Option<Result<Value>> {
    let _ = backend;
    let model = store.ext().macsec.model.as_ref();
    Some(Ok(match attr {
        sys::SAI_PORT_ATTR_INGRESS_MACSEC_ACL => {
            Value::Oid(model.map_or(RawOid::NULL, |m| m.port_acl(id, AclSide::Ingress)))
        }
        sys::SAI_PORT_ATTR_EGRESS_MACSEC_ACL => {
            Value::Oid(model.map_or(RawOid::NULL, |m| m.port_acl(id, AclSide::Egress)))
        }
        sys::SAI_PORT_ATTR_MACSEC_PORT_LIST => Value::OidList(model.map_or_else(Vec::new, |m| m.port_macsec_ports(id))),
        _ => return None,
    }))
}

/// Whether an `ACL_TABLE` create is for a `MACsec` stage (C++ `macsec_acl_stage`: the
/// `ACL_STAGE` attribute is `INGRESS_MACSEC` or `EGRESS_MACSEC`).
pub fn macsec_acl_stage(attrs: &[Attr]) -> bool {
    attrs
        .iter()
        .find(|a| a.id == sys::SAI_ACL_TABLE_ATTR_ACL_STAGE)
        .is_some_and(|a| {
            matches!(a.value, Value::Enum(s) | Value::I32(s)
                if s == sys::SAI_ACL_STAGE_INGRESS_MACSEC.cast_signed() || s == sys::SAI_ACL_STAGE_EGRESS_MACSEC.cast_signed())
        })
}

/// `macsec_acl_table_create` (the caller checked [`macsec_acl_stage`]): the model creates the
/// table under `id`, the id the ACL area chose (and stores its placeholder under).
pub(crate) fn macsec_acl_table_create<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: RawOid,
    attrs: &[Attr],
) -> Result<()> {
    acl_create(backend, store, MacsecObject::AclTable, id, attrs)
}

/// `macsec_acl_table_remove`
pub(crate) fn macsec_acl_table_remove<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: RawOid,
) -> Result<()> {
    with_model(backend, store, |m, io| m.remove(io, MacsecObject::AclTable, id))
}

/// `macsec_acl_table_get`, one attribute.
pub(crate) fn macsec_acl_table_get<B: Backend + ?Sized>(
    backend: &B,
    store: &Store,
    id: RawOid,
    meta: &AttrMeta,
) -> Result<Value> {
    let _ = backend;
    acl_get(store, MacsecObject::AclTable, id, meta)
}

/// `macsec_acl_entry_create` (the caller checked that `TABLE_ID` is a `MACsec` table).
pub(crate) fn macsec_acl_entry_create<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: RawOid,
    attrs: &[Attr],
) -> Result<()> {
    acl_create(backend, store, MacsecObject::AclEntry, id, attrs)
}

/// `macsec_acl_entry_remove`
pub(crate) fn macsec_acl_entry_remove<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: RawOid,
) -> Result<()> {
    with_model(backend, store, |m, io| m.remove(io, MacsecObject::AclEntry, id))
}

/// `macsec_acl_entry_set`
pub(crate) fn macsec_acl_entry_set<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    id: RawOid,
    attr: &Attr,
) -> Result<()> {
    with_model(backend, store, |m, io| m.set(io, MacsecObject::AclEntry, id, attr))
}

/// `macsec_acl_entry_get`, one attribute.
pub(crate) fn macsec_acl_entry_get<B: Backend + ?Sized>(
    backend: &B,
    store: &Store,
    id: RawOid,
    meta: &AttrMeta,
) -> Result<Value> {
    let _ = backend;
    acl_get(store, MacsecObject::AclEntry, id, meta)
}

fn acl_create<B: Backend + ?Sized>(
    backend: &mut B,
    store: &mut Store,
    object: MacsecObject,
    id: RawOid,
    attrs: &[Attr],
) -> Result<()> {
    let made = with_model_alloc(backend, store, Some(id), |m, io| m.create(io, object, attrs))?;
    if made == id {
        Ok(())
    } else {
        // the model did not take the id it was given (cannot happen with sai-macsec)
        let _ = with_model(backend, store, |m, io| m.remove(io, object, made));
        Err(Error::Failure)
    }
}

fn acl_get(store: &Store, object: MacsecObject, id: RawOid, meta: &AttrMeta) -> Result<Value> {
    let model = store.ext().macsec.model.as_ref().ok_or(Error::InvalidObjectId)?;
    let mut values = model.get_acl(object, id, &[meta.id])?;
    values.pop().ok_or(Error::Failure)
}

/// `sai_query_stats_capability` for `MACSEC_SA` / `MACSEC_FLOW` (C++ `macsec_stats_capability`):
/// the statistics some `stats-*` sequence of the table reads, each `READ`; none without a table.
pub(crate) fn macsec_stats_capability(
    store: Option<&Store>,
    object_type: sys::sai_object_type_t,
) -> Result<Vec<crate::caps::StatCap>> {
    let object = match object_type {
        sys::SAI_OBJECT_TYPE_MACSEC_SA => MacsecObject::Sa,
        sys::SAI_OBJECT_TYPE_MACSEC_FLOW => MacsecObject::Flow,
        _ => return Err(Error::NotSupported),
    };
    let Some(model) = store.and_then(|s| s.ext().macsec.model.as_ref()) else {
        return Ok(Vec::new());
    };
    Ok(model
        .stats_capability(object)?
        .into_iter()
        .map(|id| (id, sys::SAI_STATS_MODE_READ))
        .collect())
}

// ------------------------------------------------------------------ the table's life cycle

/// The model's maker, registered once per process by whoever links one in (the rim registers
/// `sai-macsec-phy`'s; a core without one answers `MACsec` calls `NOT_SUPPORTED`). A
/// constructor, not state: every switch gets its own model from it.
static LOADER: std::sync::OnceLock<&'static dyn MacsecLoader> = std::sync::OnceLock::new();

/// Registers the `MACsec` model's loader (the first registration counts; later ones are
/// ignored: there is one implementation per program).
pub fn register_loader(loader: &'static dyn MacsecLoader) {
    let _ = LOADER.set(loader);
}

fn loader() -> Option<&'static dyn MacsecLoader> {
    LOADER.get().copied()
}

/// A table loaded before the backend starts ([`ext_macsec_load`]).
pub(crate) struct Loaded(Option<Box<dyn MacsecPending>>);

impl fmt::Debug for Loaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Loaded").field(&self.0.is_some()).finish()
    }
}

/// `ext_macsec_load` (before the backend is chosen; also the rim's precheck): reads and parses
/// `SAI_OPENBCM_EXT_MACSEC_FILE`. Cannot open: `FAILURE`, a bad table: `INVALID_PARAMETER` (the
/// loader logs both as the C++). A key in a core without a loader: `NOT_SUPPORTED`.
pub(crate) fn ext_macsec_load(profile: &crate::backend::Profile) -> Result<Loaded> {
    let path = profile.get(crate::extphy::EXT_MACSEC_FILE).unwrap_or_default();
    if path.is_empty() {
        return Ok(Loaded(None));
    }
    let Some(loader) = loader() else {
        crate::log::error(
            sys::SAI_API_SWITCH,
            format_args!(
                "SAI_OPENBCM_EXT_MACSEC_FILE {path}: this library has no MACsec model; refusing to create the switch"
            ),
        );
        return Err(Error::NotSupported);
    };
    loader.load(path).map(|pending| Loaded(Some(pending)))
}

/// `ext_macsec_attach` (after the external PHY table's attach): the model of the switch, the
/// table's ports by first lane, the start-up clean-up; then the clear-window hold's gates
/// (`SAI_OPENBCM_MACSEC_HOLD_CLOSED`, off by default).
pub(crate) fn ext_macsec_attach<B: Backend + ?Sized>(
    backend: &mut B,
    ext: &mut crate::extphy::External,
    loaded: Loaded,
) -> Result<()> {
    let Some(pending) = loaded.0 else {
        if ext.hold_spec != crate::extphy::HoldSpec::Off {
            crate::log::error(
                sys::SAI_API_SWITCH,
                format_args!("{}: no external MACsec table", crate::extphy::MACSEC_HOLD_CLOSED),
            );
            return Err(Error::InvalidParameter);
        }
        ext.macsec = MacsecState::new(loader().map(MacsecLoader::without_table), BTreeSet::new());
        return Ok(());
    };
    let ports: Vec<ChipPortInfo> = backend
        .ports()
        .into_iter()
        .map(|p| ChipPortInfo {
            hw: p.hw,
            name: p.name,
            lanes: p.lanes,
        })
        .collect();
    let phy = ext.phy.clone();
    let clock = phy.clock();
    let model = {
        let mut io = CoreIo {
            backend: &mut *backend,
            store: None,
            phy,
            clock,
            alloc: None,
        };
        pending.attach(&mut io, &ports, ext.options)?
    };
    let engines = model.engine_ports();
    let held: Option<BTreeSet<HwPort>> = match &ext.hold_spec {
        crate::extphy::HoldSpec::Off => Some(BTreeSet::new()),
        crate::extphy::HoldSpec::All => Some(engines.iter().copied().collect()),
        crate::extphy::HoldSpec::Lanes(lanes) => lanes
            .iter()
            .map(|lane| {
                ports
                    .iter()
                    .rfind(|p| p.lanes.first() == Some(lane))
                    .map(|p| p.hw)
                    .filter(|hw| engines.contains(hw))
            })
            .collect(),
    };
    let Some(held) = held else {
        crate::log::error(
            sys::SAI_API_SWITCH,
            format_args!(
                "{}: a lane that is not the first lane of a port of the external MACsec table",
                crate::extphy::MACSEC_HOLD_CLOSED
            ),
        );
        return Err(Error::InvalidParameter);
    };
    ext.macsec = MacsecState::new(Some(model), held.clone());
    for hw in held {
        crate::extphy::register_gate(backend, ext, hw)?;
    }
    Ok(())
}

/// `ext_macsec_detach` (at switch removal and on a failed creation): nothing runs; the engines
/// keep what they hold until the next attach probes them (sai-macsec's Q2). The model and the
/// keys of SAs not yet installed are dropped (and wiped) with the switch's state.
pub(crate) fn ext_macsec_detach<B: Backend + ?Sized>(backend: &mut B, ext: &crate::extphy::External) {
    let _ = (backend, ext);
}

/// The test port's question ([`crate::testing`]): an external `MACsec` table is attached.
#[cfg(feature = "testing")]
#[allow(clippy::unnecessary_wraps, reason = "the test port's signature for every area")]
pub(crate) fn testing_ext_macsec_active<B: Backend>(sai: &Sai<B>) -> crate::testing::AreaAnswer<bool> {
    Ok(sai.ext_macsec_active())
}

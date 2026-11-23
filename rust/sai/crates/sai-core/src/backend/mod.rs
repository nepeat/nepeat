//! What the object model needs from a switch chip: the C++ `ob::Backend`
//! (`sai-openbcm/src/backend.h`) operation for operation (ports are the SDK's logical port
//! numbers, VLANs are 802.1Q VIDs, every call answers with a SAI-level error).
//! Implementations: [`crate::virt`] (in memory), `sai-backend-openbcm` (the SDK glue).
//!
//! - [`Backend`]: the supertrait. It still carries the first slice's 15 operations (`name`,
//!   `init`, ports, VLANs), the capability and sizing queries (`programs`, `limits`,
//!   `port_queues`, ...) and the generic counter methods (`stats_*`, for the counters of objects
//!   no typed operation reads).
//! - [`ops`]: the typed operations, one sub-trait per group of `backend.h` ([`ops::Base`],
//!   [`ops::Shell`], [`ops::Temp`], [`ops::Mdio`], [`ops::PortOps`], [`ops::Lag`], [`ops::L2`],
//!   [`ops::Hostif`], [`ops::L3`], [`ops::Field`], [`ops::Policer`], [`ops::Qos`], [`ops::Mirror`],
//!   [`ops::Sample`], [`ops::Hash`], [`ops::Storm`]); [`types`]: their argument types.
//! - [`recording::Recording`]: a decorator that logs every call in the C++ recording backend's
//!   line format ([`log`]).
//!
//! A backend reports what the chip does on its own (link changes, learned and aged addresses)
//! through the [`EventSink`] it is given at [`Backend::init`]. Sending only queues: it may be
//! called from any thread, including an SDK callback, and never takes a lock of the SAI layer.
//! A per-port [`types::LinkGate`] is the one callback into SAI-side code that a backend makes
//! synchronously; its rules are on the type.
use crate::error::{Error, Result};
use crate::oid::RawOid;
use sai_sys as sys;
use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};

/// One line in the recording decorator, by the kind of the operation's answer:
/// `status(names)`: a status and, on success, outputs (none, one value, or a structure whose
/// fields are the C++ outputs); `value()`: a value instead of a status; `query()` /
/// `query_status(names)`: the same, logged only in full mode (answers fixed for the life of the
/// backend).
macro_rules! record {
    ($slf:ident, $op:ident, status(), [$($a:ident),*]) => {
        record!(@call $slf, $op, false, [$($a),*], super::log::no_outs)
    };
    ($slf:ident, $op:ident, status($name:ident), [$($a:ident),*]) => {
        record!(@call $slf, $op, false, [$($a),*], |v, s| super::log::LogOut::log_out(&super::log::One(v), &[stringify!($name)], s))
    };
    ($slf:ident, $op:ident, outs($($name:ident),+), [$($a:ident),*]) => {
        record!(@call $slf, $op, false, [$($a),*], |v, s| super::log::LogOut::log_out(v, &[$(stringify!($name)),+], s))
    };
    ($slf:ident, $op:ident, query_status($name:ident), [$($a:ident),*]) => {
        record!(@call $slf, $op, true, [$($a),*], |v, s| super::log::LogOut::log_out(&super::log::One(v), &[stringify!($name)], s))
    };
    ($slf:ident, $op:ident, value(), [$($a:ident),*]) => {
        record!(@value $slf, $op, false, [$($a),*])
    };
    ($slf:ident, $op:ident, query(), [$($a:ident),*]) => {
        record!(@value $slf, $op, true, [$($a),*])
    };
    (@call $slf:ident, $op:ident, $query:expr, [$($a:ident),*], $outs:expr) => {{
        #[allow(unused_mut, reason = "an operation without arguments")]
        let mut args = String::new();
        $(super::log::arg(&mut args, stringify!($a), &$a);)*
        let result = $slf.inner.$op($($a),*);
        $slf.record($query, || super::log::call_line(stringify!($op), &args, &result, $outs));
        result
    }};
    (@value $slf:ident, $op:ident, $query:expr, [$($a:ident),*]) => {{
        #[allow(unused_mut, reason = "an operation without arguments")]
        let mut args = String::new();
        $(super::log::arg(&mut args, stringify!($a), &$a);)*
        let value = $slf.inner.$op($($a),*);
        $slf.record($query, || super::log::value_line(stringify!($op), &args, &value));
        value
    }};
}

pub mod log;
pub mod ops;
pub mod recording;
pub mod types;

pub use ops::{
    Base, Field, Hash, Hostif, L2, L3, Lag, Mdio, Mirror, Policer, PortOps, Qos, Sample, Shell, Storm, Temp,
};
pub use recording::{CallLog, Mode, Recording};

/// A chip logical port number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HwPort(pub u16);

/// An 802.1Q VLAN id: 1..=4094 by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Vid(u16);

impl Vid {
    /// The default VLAN.
    pub const DEFAULT: Self = Self(1);

    /// A VLAN id; 0 and 4095 are reserved by 802.1Q and refused.
    pub fn new(vid: u16) -> Result<Self> {
        if (1..=4094).contains(&vid) {
            Ok(Self(vid))
        } else {
            Err(Error::InvalidVlanId)
        }
    }

    /// The number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A front-panel port as the chip came up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortInfo {
    /// Logical port number.
    pub hw: HwPort,
    /// Physical lanes.
    pub lanes: Vec<u32>,
    /// Speed in Mb/s after init.
    pub speed: u32,
    /// The SDK's name (`ce0`).
    pub name: String,
    /// Autonegotiation as the SDK left it after init.
    pub autoneg: bool,
}

/// Values of the SAI profile (`sai.profile`), read by the rim through the service method
/// table before the switch is created.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile(BTreeMap<String, String>);

impl Profile {
    /// A profile from key/value pairs.
    pub fn from_pairs<K: Into<String>, V: Into<String>>(pairs: impl IntoIterator<Item = (K, V)>) -> Self {
        Self(pairs.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }

    /// The value of `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The value of the first of `keys` that is set (a key and its aliases, preferred first).
    pub fn get_any(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|key| self.get(key))
    }

    /// The pairs, keys in byte order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The value of `key` as a number: `default` when absent, `InvalidParameter` when it does
    /// not parse or is outside `range` (a typo must not silently become the default).
    pub fn number(&self, key: &str, default: u32, range: core::ops::RangeInclusive<u32>) -> Result<u32> {
        match self.get(key) {
            None => Ok(default),
            Some(text) => text
                .trim()
                .parse()
                .ok()
                .filter(|n| range.contains(n))
                .ok_or(Error::InvalidParameter),
        }
    }
}

/// Fixed sizes of the chip's tables (what the switch's capacity attributes answer today,
/// through [`Backend::limits`]). The C++ splits this into [`types::ChipLimits`] (fixed) and
/// [`types::TableUsage`] (free entries, [`ops::Base::table_usage`]); this goes with the switch
/// area's port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacities {
    /// Lowest ACL entry priority.
    pub acl_priority_min: u32,
    /// Highest ACL entry priority.
    pub acl_priority_max: u32,
    /// ECMP groups.
    pub ecmp_groups: u32,
    /// Paths per ECMP group.
    pub ecmp_paths: u32,
    /// ECMP member entries in all.
    pub ecmp_members: u32,
    /// Packet buffer in kB.
    pub buffer_kb: u64,
    /// IPv4 routes.
    pub v4_routes: u32,
    /// IPv6 routes.
    pub v6_routes: u32,
    /// Host entries (an IPv6 neighbor takes two).
    pub hosts: u32,
    /// Next hops (egress objects).
    pub next_hops: u32,
    /// MAC table entries.
    pub fdb: u32,
}

/// What a counter read is about, in the chip's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatsObject {
    /// A port.
    Port(HwPort),
    /// An egress queue of a port.
    Queue {
        /// The port.
        port: HwPort,
        /// The queue's index.
        index: u8,
    },
    /// An ingress priority group of a port.
    PriorityGroup {
        /// The port.
        port: HwPort,
        /// The group's index.
        index: u8,
    },
    /// Any other object, by its type and id.
    Other {
        /// The SAI object type.
        object_type: sys::sai_object_type_t,
        /// The object.
        id: RawOid,
    },
}

/// Something the chip did on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// A port's link went up or down (C++ `LinkCallback`).
    Link {
        /// The port.
        hw: HwPort,
        /// The new state.
        up: bool,
    },
    /// The chip learned or aged an address (C++ `FdbCallback`, `ob::FdbEvent`).
    Fdb(types::FdbEvent),
}

/// An observer of every event a sink is sent ([`EventSink::tapped`]).
pub type EventTap = Arc<dyn Fn(&BackendEvent) + Send + Sync>;

/// Where a backend reports events. Cloneable; sending never blocks.
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<BackendEvent>,
    tap: Option<EventTap>,
}

impl core::fmt::Debug for EventSink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventSink")
            .field("tapped", &self.tap.is_some())
            .finish_non_exhaustive()
    }
}

impl EventSink {
    /// A sink and the receiving end the notifier reads.
    pub fn channel() -> (Self, mpsc::Receiver<BackendEvent>) {
        let (tx, rx) = mpsc::channel();
        (Self { tx, tap: None }, rx)
    }

    /// The same sink, with `tap` called (on the sender's thread) for every event before it is
    /// queued: how the recording decorator logs events where they happen. Replaces an earlier
    /// tap.
    #[must_use]
    pub fn tapped(self, tap: impl Fn(&BackendEvent) + Send + Sync + 'static) -> Self {
        Self {
            tx: self.tx,
            tap: Some(Arc::new(tap)),
        }
    }

    /// Queues an event. Events sent after the switch was removed are dropped.
    pub fn send(&self, event: BackendEvent) {
        if let Some(tap) = &self.tap {
            tap(&event);
        }
        // The receiver is gone once the switch is: nothing to tell anyone.
        let _ = self.tx.send(event);
    }
}

impl<B: Backend + ?Sized> Backend for Box<B> {
    fn name(&self) -> &'static str {
        (**self).name()
    }
    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        (**self).init(profile, events)
    }
    fn shutdown(&mut self) {
        (**self).shutdown();
    }
    fn ports(&self) -> Vec<PortInfo> {
        (**self).ports()
    }
    fn cpu_port(&self) -> HwPort {
        (**self).cpu_port()
    }
    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()> {
        (**self).port_admin_set(hw, up)
    }
    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        (**self).port_link_get(hw)
    }
    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        (**self).port_speed_set(hw, mbps)
    }
    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        (**self).port_speed_get(hw)
    }
    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        (**self).port_supported_speeds(hw)
    }
    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        (**self).vlan_create(vid)
    }
    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        (**self).vlan_destroy(vid)
    }
    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        (**self).vlan_port_add(vid, hw, untagged)
    }
    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        (**self).vlan_port_remove(vid, hw)
    }
    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        (**self).vlan_members(vid)
    }
    fn port_queues(&self) -> u8 {
        (**self).port_queues()
    }
    fn port_priority_groups(&self) -> u8 {
        (**self).port_priority_groups()
    }
    fn limits(&self) -> Result<Capacities> {
        (**self).limits()
    }
    fn temperature_reader(&self) -> Option<crate::temp::TempReader> {
        (**self).temperature_reader()
    }
    fn programs(&self, object_type: sys::sai_object_type_t) -> bool {
        (**self).programs(object_type)
    }
    fn stats_supported(&self, object_type: sys::sai_object_type_t) -> Result<Vec<sys::sai_stat_id_t>> {
        (**self).stats_supported(object_type)
    }
    fn stats_get(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>> {
        (**self).stats_get(object, counters, clear)
    }
    fn stats_clear(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t]) -> Result<()> {
        (**self).stats_clear(object, counters)
    }
    #[cfg(feature = "testing")]
    fn as_virtual(&self) -> Option<&crate::virt::VirtualBackend> {
        (**self).as_virtual()
    }
    #[cfg(feature = "testing")]
    fn as_virtual_mut(&mut self) -> Option<&mut crate::virt::VirtualBackend> {
        (**self).as_virtual_mut()
    }
    fn mdio_bus(&self) -> Option<types::MdioBus> {
        (**self).mdio_bus()
    }
    fn diag_mdio_native(&self) -> bool {
        (**self).diag_mdio_native()
    }
}

/// Everything the SAI layer needs from a chip: the typed sub-traits ([`ops`]) and, for now,
/// the first slice's operations and the queries below.
///
/// A backend that has none of a group's operations yet implements the sub-trait with its
/// default bodies (`impl L3 for MyBackend {}`, or [`crate::typed_defaults!`] for several), which
/// answer `NotSupported` as the C++ defaults do.
pub trait Backend:
    Send
    + Base
    + Shell
    + Temp
    + Mdio
    + PortOps
    + Lag
    + L2
    + Hostif
    + L3
    + Field
    + Policer
    + Qos
    + Mirror
    + Sample
    + Hash
    + Storm
{
    /// A name for logs.
    fn name(&self) -> &'static str;
    /// Brings the chip up. Called once per switch creation; on error nothing is left running.
    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()>;
    /// Stops the chip and drops the event sink.
    fn shutdown(&mut self);
    /// The front-panel ports, in chip order.
    fn ports(&self) -> Vec<PortInfo>;
    /// The CPU port's number.
    fn cpu_port(&self) -> HwPort;

    /// Enables or disables a port.
    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()>;
    /// The port's link state.
    fn port_link_get(&self, hw: HwPort) -> Result<bool>;
    /// Sets the speed in Mb/s.
    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()>;
    /// The configured speed in Mb/s.
    fn port_speed_get(&self, hw: HwPort) -> Result<u32>;
    /// The speeds the port can run at.
    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>>;

    /// Creates a VLAN without members.
    fn vlan_create(&mut self, vid: Vid) -> Result<()>;
    /// Destroys a VLAN.
    fn vlan_destroy(&mut self, vid: Vid) -> Result<()>;
    /// Adds a port to a VLAN, or changes its tagging when it is a member already.
    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()>;
    /// Removes a port from a VLAN.
    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()>;
    /// The members of a VLAN with their tagging (true = untagged).
    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>>;

    // ---- Everything below has a default: a backend that does not have the feature says so
    // (`NotSupported`) or reports none of it, and the SAI layer answers accordingly.

    /// Egress queues per front-panel port (the SAI layer makes one QUEUE object and one
    /// scheduler group for each). Default: none.
    fn port_queues(&self) -> u8 {
        0
    }
    /// Ingress priority groups per front-panel port. Default: none.
    fn port_priority_groups(&self) -> u8 {
        0
    }
    /// The chip's table sizes.
    fn limits(&self) -> Result<Capacities> {
        Err(Error::NotSupported)
    }
    /// A temperature read the publisher thread calls without the SAI lock
    /// ([`crate::temp::Publisher`]): [`Temp::asic_temperatures`] on state shared with the
    /// backend. Default: none (no publisher runs).
    fn temperature_reader(&self) -> Option<crate::temp::TempReader> {
        None
    }

    /// Whether objects of `object_type` are programmed by this backend (for
    /// `SAI_OBJECT_TYPE_PORT`: whether the port attributes without an operation of their own can
    /// be set). The capability queries advertise only what this answers yes to ([`crate::caps`]).
    /// Default: no.
    fn programs(&self, _object_type: sys::sai_object_type_t) -> bool {
        false
    }

    /// The counters the chip has for objects of a type.
    fn stats_supported(&self, _object_type: sys::sai_object_type_t) -> Result<Vec<sys::sai_stat_id_t>> {
        Err(Error::NotSupported)
    }
    /// Reads counters, in the order asked; with `clear` they are zeroed after the read.
    fn stats_get(&mut self, _object: StatsObject, _counters: &[sys::sai_stat_id_t], _clear: bool) -> Result<Vec<u64>> {
        Err(Error::NotSupported)
    }
    /// Zeroes counters.
    fn stats_clear(&mut self, _object: StatsObject, _counters: &[sys::sai_stat_id_t]) -> Result<()> {
        Err(Error::NotSupported)
    }

    /// The virtual chip, when this backend is one or wraps one (the test port,
    /// [`crate::testing`]). Default: none.
    #[cfg(feature = "testing")]
    fn as_virtual(&self) -> Option<&crate::virt::VirtualBackend> {
        None
    }
    /// The same, to change (the test port's hooks).
    #[cfg(feature = "testing")]
    fn as_virtual_mut(&mut self) -> Option<&mut crate::virt::VirtualBackend> {
        None
    }

    /// The MDIO master as a handle the ext-PHY runtime keeps ([`types::MdioAccess`]: its link
    /// gates run on the backend's link scan thread). Not a C++ operation and not logged itself;
    /// the accesses through the handle are logged as `mdio_c45_read` / `mdio_c45_write` by the
    /// recording decorator. Default: none (the backend has no MDIO for an external PHY table).
    fn mdio_bus(&self) -> Option<types::MdioBus> {
        None
    }

    /// Whether the backend's diag shell answers `phy raw c45 PHY DEV REG [VALUE]` from the MDIO
    /// master behind [`Backend::mdio_bus`] (the SDK's shell does: `soc_miimc45_read` / `_write`,
    /// the calls of the handle), so that the diag socket may serve such a line itself over the
    /// handle, outside the SAI lock, with the same bus effect and answer (`crate::diag`). Not a C++
    /// operation and not logged. Default yes; the virtual chip only in
    /// [`crate::virt::ShellMdio::Answer`] (its default shell is the C++ fake's echo).
    fn diag_mdio_native(&self) -> bool {
        true
    }
}

/// A chip that has the first slice's operations of `B` ([`LEGACY_CPP_OPS`]) and none of the
/// typed ones: every typed operation answers what its default answers, or the C++'s
/// `NotSupported` for a required one (tests of what the SAI layer does without a feature).
#[derive(Debug)]
pub struct Refusing<B>(pub B);

impl<B: Backend> Backend for Refusing<B> {
    fn name(&self) -> &'static str {
        "refusing"
    }
    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        self.0.init(profile, events)
    }
    fn shutdown(&mut self) {
        self.0.shutdown();
    }
    fn ports(&self) -> Vec<PortInfo> {
        self.0.ports()
    }
    fn cpu_port(&self) -> HwPort {
        self.0.cpu_port()
    }
    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()> {
        self.0.port_admin_set(hw, up)
    }
    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        self.0.port_link_get(hw)
    }
    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        self.0.port_speed_set(hw, mbps)
    }
    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        self.0.port_speed_get(hw)
    }
    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        self.0.port_supported_speeds(hw)
    }
    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        self.0.vlan_create(vid)
    }
    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        self.0.vlan_destroy(vid)
    }
    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        self.0.vlan_port_add(vid, hw, untagged)
    }
    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        self.0.vlan_port_remove(vid, hw)
    }
    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        self.0.vlan_members(vid)
    }
}

/// The C++ `ob::Backend` operations that are still methods of [`Backend`] (with the same names
/// and meaning); [`ops::OP_NAMES`] has the other 106.
pub const LEGACY_CPP_OPS: [&str; 15] = [
    "name",
    "init",
    "shutdown",
    "cpu_port",
    "ports",
    "port_admin_set",
    "port_link_get",
    "port_speed_set",
    "port_speed_get",
    "port_supported_speeds",
    "vlan_create",
    "vlan_destroy",
    "vlan_port_add",
    "vlan_port_remove",
    "vlan_members",
];

/// Implements typed sub-traits ([`crate::backend::ops`]) with their default bodies for a
/// backend that has none of their operations yet: `typed_defaults!(MyBackend: Base, Shell, L3);`.
/// When an area is ported to the backend, its trait leaves the list and gets a real impl.
#[macro_export]
macro_rules! typed_defaults {
    ($ty:ty: $($tr:ident),+ $(,)?) => {
        $(impl $crate::backend::ops::$tr for $ty {})+
    };
}

/// Records the names of the typed operations called (tests: [`ops::OP_NAMES`] is complete).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct OpNames(pub(crate) core::cell::RefCell<Vec<&'static str>>);

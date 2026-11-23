//! What the object model needs from a switch chip: the operations of this slice, in the
//! spirit of the C++ `ob::Backend` (ports are the SDK's logical port numbers, VLANs are
//! 802.1Q VIDs, every call answers with a SAI-level error). Implementations: [`crate::virt`]
//! (in memory); later one over the C SDK glue.
//!
//! A backend reports what the chip does on its own (link changes) through the
//! [`EventSink`] it is given at [`Backend::init`]. Sending only queues: it may be called from
//! any thread, including an SDK callback, and never takes a lock of the SAI layer.
use crate::error::{Error, Result};
use crate::model::EntryKey;
use crate::oid::RawOid;
use crate::value::Attr;
use sai_sys as sys;
use std::collections::BTreeMap;
use std::sync::mpsc;

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

/// Fixed sizes of the chip's tables (what the switch's capacity attributes answer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipLimits {
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
    /// A port's link went up or down.
    Link {
        /// The port.
        hw: HwPort,
        /// The new state.
        up: bool,
    },
}

/// A host interface of type NETDEV on a front-panel port, as the SAI layer resolved it for
/// [`Backend::hostif_create`]: the netdev a KNET-style chip makes for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostifNetdev<'a> {
    /// `SAI_HOSTIF_ATTR_NAME`: 1..=15 bytes (checked by the SAI layer).
    pub name: &'a str,
    /// The port `SAI_HOSTIF_ATTR_OBJ_ID` names.
    pub port: HwPort,
    /// The switch's source MAC address (`SAI_SWITCH_ATTR_SRC_MAC_ADDRESS`), which the C++
    /// library gives every port netdev.
    pub mac: [u8; 6],
    /// `SAI_HOSTIF_ATTR_OPER_STATUS` at creation (default false): the netdev's carrier.
    pub oper_up: bool,
}

/// Where a backend reports events. Cloneable; sending never blocks.
#[derive(Clone, Debug)]
pub struct EventSink(mpsc::Sender<BackendEvent>);

impl EventSink {
    /// A sink and the receiving end the notifier reads.
    pub fn channel() -> (Self, mpsc::Receiver<BackendEvent>) {
        let (tx, rx) = mpsc::channel();
        (Self(tx), rx)
    }

    /// Queues an event. Events sent after the switch was removed are dropped.
    pub fn send(&self, event: BackendEvent) {
        // The receiver is gone once the switch is: nothing to tell anyone.
        let _ = self.0.send(event);
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
    fn limits(&self) -> Result<ChipLimits> {
        (**self).limits()
    }
    fn programs(&self, object_type: sys::sai_object_type_t) -> bool {
        (**self).programs(object_type)
    }
    fn object_create(&mut self, object_type: sys::sai_object_type_t, id: RawOid, attrs: &[Attr]) -> Result<()> {
        (**self).object_create(object_type, id, attrs)
    }
    fn hostif_create(&mut self, id: RawOid, netdev: &HostifNetdev<'_>, attrs: &[Attr]) -> Result<()> {
        (**self).hostif_create(id, netdev, attrs)
    }
    fn object_set(&mut self, object_type: sys::sai_object_type_t, id: RawOid, attr: &Attr) -> Result<()> {
        (**self).object_set(object_type, id, attr)
    }
    fn object_remove(&mut self, object_type: sys::sai_object_type_t, id: RawOid) -> Result<()> {
        (**self).object_remove(object_type, id)
    }
    fn entry_create(&mut self, key: &EntryKey, attrs: &[Attr]) -> Result<()> {
        (**self).entry_create(key, attrs)
    }
    fn entry_set(&mut self, key: &EntryKey, attr: &Attr) -> Result<()> {
        (**self).entry_set(key, attr)
    }
    fn entry_remove(&mut self, key: &EntryKey) -> Result<()> {
        (**self).entry_remove(key)
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
}

/// The chip operations of this slice.
pub trait Backend: Send {
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
    fn limits(&self) -> Result<ChipLimits> {
        Err(Error::NotSupported)
    }

    /// Whether [`Backend::object_create`], [`Backend::object_set`] and
    /// [`Backend::object_remove`] (or the entry operations, for an entry type) can succeed for
    /// objects of `object_type`; for `SAI_OBJECT_TYPE_PORT`: whether the port attributes
    /// without an operation of their own can be set. The capability queries advertise only
    /// what this answers yes to ([`crate::caps`]). Default: no (the defaults below refuse).
    fn programs(&self, _object_type: sys::sai_object_type_t) -> bool {
        false
    }

    /// An object whose whole state is its attributes is about to be created with the id
    /// `id` ([`crate::bag`]): program it, or refuse. Also called for attributes of ports
    /// that have no operation of their own above (`object_set` with a port id).
    fn object_create(&mut self, _object_type: sys::sai_object_type_t, _id: RawOid, _attrs: &[Attr]) -> Result<()> {
        Err(Error::NotSupported)
    }
    /// A host interface of type NETDEV is about to be created: make its netdev, or refuse.
    /// Called instead of [`Backend::object_create`] for these (its later attribute changes and
    /// its removal come through [`Backend::object_set`] and [`Backend::object_remove`] as for
    /// any bag object). Default: [`Backend::object_create`], no netdev.
    fn hostif_create(&mut self, id: RawOid, _netdev: &HostifNetdev<'_>, attrs: &[Attr]) -> Result<()> {
        self.object_create(sys::SAI_OBJECT_TYPE_HOSTIF, id, attrs)
    }
    /// One attribute of such an object is about to change.
    fn object_set(&mut self, _object_type: sys::sai_object_type_t, _id: RawOid, _attr: &Attr) -> Result<()> {
        Err(Error::NotSupported)
    }
    /// Such an object is about to be removed.
    fn object_remove(&mut self, _object_type: sys::sai_object_type_t, _id: RawOid) -> Result<()> {
        Err(Error::NotSupported)
    }
    /// An FDB, neighbor or route entry is about to be created.
    fn entry_create(&mut self, _key: &EntryKey, _attrs: &[Attr]) -> Result<()> {
        Err(Error::NotSupported)
    }
    /// One attribute of an entry is about to change.
    fn entry_set(&mut self, _key: &EntryKey, _attr: &Attr) -> Result<()> {
        Err(Error::NotSupported)
    }
    /// An entry is about to be removed.
    fn entry_remove(&mut self, _key: &EntryKey) -> Result<()> {
        Err(Error::NotSupported)
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
}

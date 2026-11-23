//! The virtual backend: an in-memory chip. Deterministic, no hardware, and it can be told to
//! fail the next call of an operation, which is how the tests check that a failed backend
//! call leaves the object model unchanged.
//!
//! Profile keys: `SAI_RS_VIRTUAL_PORTS` (32), `SAI_RS_VIRTUAL_LANES` (4),
//! `SAI_RS_VIRTUAL_SPEED` (100000), `SAI_RS_VIRTUAL_LINK` (`admin`: link follows admin state;
//! `none`: never up), `SAI_RS_VIRTUAL_ON_HARDWARE` (`1`: start although the host has switch
//! chip devices; also read from the process environment), `SAI_RS_VIRTUAL_NETDEV` (`none`:
//! host interfaces exist in the model only; `tap`: each is also a Linux TAP netdev,
//! [`netdev`]; never on a host with switch chip devices), `SAI_RS_VIRTUAL_WIRE`
//! (`<netif>:<ifname>,...`, tap only: that netdev's frames are carried to and from the
//! host's interface `<ifname>` while the port is up). The C++ fake's keys, renamed.
use crate::backend::{
    Backend, BackendEvent, ChipLimits, EventSink, HostifNetdev, HwPort, PortInfo, Profile, StatsObject, Vid,
};
use crate::error::{Error, Result};
use crate::model::EntryKey;
use crate::oid::RawOid;
use crate::value::{Attr, Value};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod netdev;
pub use netdev::{NetdevMode, parse_wire};

/// The operations [`VirtualBackend::fail_next`] can make fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// [`Backend::init`]
    Init,
    /// [`Backend::port_admin_set`]
    PortAdminSet,
    /// [`Backend::port_speed_set`]
    PortSpeedSet,
    /// [`Backend::vlan_create`]
    VlanCreate,
    /// [`Backend::vlan_destroy`]
    VlanDestroy,
    /// [`Backend::vlan_port_add`]
    VlanPortAdd,
    /// [`Backend::vlan_port_remove`]
    VlanPortRemove,
    /// [`Backend::object_create`]
    ObjectCreate,
    /// [`Backend::object_set`]
    ObjectSet,
    /// [`Backend::object_remove`]
    ObjectRemove,
    /// [`Backend::entry_create`]
    EntryCreate,
    /// [`Backend::entry_set`]
    EntrySet,
    /// [`Backend::entry_remove`]
    EntryRemove,
}

#[derive(Debug)]
struct Port {
    lanes: Vec<u32>,
    speed: u32,
    admin: bool,
    link: bool,
    /// `admin && link`, for the wire threads (which read it without any lock).
    up: Arc<AtomicBool>,
}

impl Port {
    fn sync_up(&self) {
        self.up.store(self.admin && self.link, Ordering::Release);
    }
}

/// A host interface of type NETDEV as the chip has it (the C++ fake's `Netif`).
#[derive(Debug)]
pub struct Netif {
    /// Its name.
    pub name: String,
    /// Its port.
    pub port: HwPort,
    /// The carrier: the host interface's `OPER_STATUS`.
    pub carrier: bool,
    kernel: Option<netdev::Kernel>,
}

impl Netif {
    /// Whether it is a kernel netdev (mode `tap`) or only a model entry.
    pub fn has_kernel_netdev(&self) -> bool {
        self.kernel.is_some()
    }

    /// Whether its wire carries frames (a thread runs).
    pub fn wired(&self) -> bool {
        self.kernel.as_ref().is_some_and(netdev::Kernel::wired)
    }
}

/// The profile keys this backend reads (the rim asks the SAI user for exactly these).
pub const PROFILE_KEYS: [&str; 7] = [
    "SAI_RS_VIRTUAL_PORTS",
    "SAI_RS_VIRTUAL_LANES",
    "SAI_RS_VIRTUAL_SPEED",
    "SAI_RS_VIRTUAL_LINK",
    "SAI_RS_VIRTUAL_ON_HARDWARE",
    "SAI_RS_VIRTUAL_NETDEV",
    "SAI_RS_VIRTUAL_WIRE",
];

/// Device nodes of the Broadcom kernel modules: a host that has them has a real switch chip,
/// and a virtual switch answering "success" there would hide that nothing is programmed.
const HARDWARE_MARKERS: [&str; 2] = ["/dev/linux-user-bde", "/dev/linux-kernel-bde"];
const SPEEDS: [u32; 5] = [10_000, 25_000, 40_000, 50_000, 100_000];
/// Queues and priority groups per port, as the C++ sai-openbcm builds them.
const QUEUES_PER_PORT: u8 = 8;
const PRIORITY_GROUPS_PER_PORT: u8 = 8;
/// Table sizes: the constants of the C++ fake backend (Tomahawk values).
const LIMITS: ChipLimits = ChipLimits {
    acl_priority_min: 0,
    acl_priority_max: 0x7fff_ffff,
    ecmp_groups: 4096,
    ecmp_paths: 64,
    ecmp_members: 16384,
    buffer_kb: 20165 * 4 * 208 / 1024,
    v4_routes: 16384,
    v6_routes: 8192,
    hosts: 16384,
    next_hops: 32768,
    fdb: 32768,
};
/// The counters this chip has, per object type (the C++ fake's sets). They read zero until
/// a test sets them.
const PORT_STATS: [sys::sai_stat_id_t; 15] = [
    sys::SAI_PORT_STAT_IF_IN_OCTETS,
    sys::SAI_PORT_STAT_IF_IN_UCAST_PKTS,
    sys::SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS,
    sys::SAI_PORT_STAT_IF_IN_DISCARDS,
    sys::SAI_PORT_STAT_IF_IN_ERRORS,
    sys::SAI_PORT_STAT_IF_IN_UNKNOWN_PROTOS,
    sys::SAI_PORT_STAT_IF_IN_BROADCAST_PKTS,
    sys::SAI_PORT_STAT_IF_IN_MULTICAST_PKTS,
    sys::SAI_PORT_STAT_IF_OUT_OCTETS,
    sys::SAI_PORT_STAT_IF_OUT_UCAST_PKTS,
    sys::SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS,
    sys::SAI_PORT_STAT_IF_OUT_DISCARDS,
    sys::SAI_PORT_STAT_IF_OUT_ERRORS,
    sys::SAI_PORT_STAT_IF_OUT_BROADCAST_PKTS,
    sys::SAI_PORT_STAT_IF_OUT_MULTICAST_PKTS,
];
const QUEUE_STATS: [sys::sai_stat_id_t; 3] = [
    sys::SAI_QUEUE_STAT_PACKETS,
    sys::SAI_QUEUE_STAT_BYTES,
    sys::SAI_QUEUE_STAT_DROPPED_PACKETS,
];
const PRIORITY_GROUP_STATS: [sys::sai_stat_id_t; 2] = [
    sys::SAI_INGRESS_PRIORITY_GROUP_STAT_PACKETS,
    sys::SAI_INGRESS_PRIORITY_GROUP_STAT_BYTES,
];

fn counters_of(object: StatsObject) -> &'static [sys::sai_stat_id_t] {
    match object {
        StatsObject::Port(_) => &PORT_STATS,
        StatsObject::Queue { .. } => &QUEUE_STATS,
        StatsObject::PriorityGroup { .. } => &PRIORITY_GROUP_STATS,
        StatsObject::Other { .. } => &[],
    }
}

/// The in-memory chip.
#[derive(Debug, Default)]
pub struct VirtualBackend {
    ports: BTreeMap<HwPort, Port>,
    vlans: BTreeMap<Vid, BTreeMap<HwPort, bool>>,
    events: Option<EventSink>,
    link_follows_admin: bool,
    fail: Option<(Op, Error)>,
    /// The attribute-bag objects the chip was told about, with their type.
    objects: BTreeMap<RawOid, sys::sai_object_type_t>,
    /// The entries the chip was told about.
    entries: BTreeSet<EntryKey>,
    /// Counters a test set; the others read zero.
    counters: BTreeMap<(StatsObject, sys::sai_stat_id_t), u64>,
    /// `SAI_RS_VIRTUAL_NETDEV`.
    netdev_mode: NetdevMode,
    /// `SAI_RS_VIRTUAL_WIRE`: netif name -> host interface.
    wire: BTreeMap<String, String>,
    /// The host interfaces of type NETDEV, by object id.
    netifs: BTreeMap<RawOid, Netif>,
}

impl VirtualBackend {
    /// A chip that is not initialised yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next call of `op` fail with `error` (once).
    pub fn fail_next(&mut self, op: Op, error: Error) {
        self.fail = Some((op, error));
    }

    /// Changes a port's link as a cable would, and reports it.
    pub fn inject_link(&mut self, hw: HwPort, up: bool) -> Result<()> {
        self.port(hw)?;
        self.set_link(hw, up);
        Ok(())
    }

    /// The host interfaces of type NETDEV the chip has, by object id. For tests.
    pub fn netifs(&self) -> &BTreeMap<RawOid, Netif> {
        &self.netifs
    }

    /// The netdev mode the profile chose.
    pub fn netdev_mode(&self) -> NetdevMode {
        self.netdev_mode
    }

    /// Whether a port's frames would pass its wire (admin up with link). For tests.
    pub fn port_passes(&self, hw: HwPort) -> Option<bool> {
        self.ports.get(&hw).map(|p| p.up.load(Ordering::Acquire))
    }

    /// The VLANs the chip has, with members and tagging (true = untagged). For tests.
    pub fn vlans(&self) -> &BTreeMap<Vid, BTreeMap<HwPort, bool>> {
        &self.vlans
    }

    /// The attribute-bag objects the chip has (created through [`Backend::object_create`]),
    /// with their type. For tests.
    pub fn objects(&self) -> &BTreeMap<RawOid, sys::sai_object_type_t> {
        &self.objects
    }

    /// The entries the chip has. For tests.
    pub fn entries(&self) -> &BTreeSet<EntryKey> {
        &self.entries
    }

    /// Sets a counter as traffic would. For tests.
    pub fn set_counter(&mut self, object: StatsObject, counter: sys::sai_stat_id_t, value: u64) {
        self.counters.insert((object, counter), value);
    }

    /// Whether a port is enabled. For tests.
    pub fn port_admin(&self, hw: HwPort) -> Option<bool> {
        self.ports.get(&hw).map(|p| p.admin)
    }

    fn check(&mut self, op: Op) -> Result<()> {
        match self.fail {
            Some((failing, error)) if failing == op => {
                self.fail = None;
                Err(error)
            }
            _ => Ok(()),
        }
    }

    fn port(&self, hw: HwPort) -> Result<&Port> {
        self.ports.get(&hw).ok_or(Error::InvalidParameter)
    }

    fn set_link(&mut self, hw: HwPort, up: bool) {
        if let Some(port) = self.ports.get_mut(&hw) {
            if port.link != up {
                port.link = up;
                port.sync_up();
                if let Some(events) = &self.events {
                    events.send(BackendEvent::Link { hw, up });
                }
            }
        }
    }
}

impl Backend for VirtualBackend {
    fn name(&self) -> &'static str {
        "virtual"
    }

    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        self.check(Op::Init)?;
        // The profile key, or the same name in the process environment (how the test
        // suites are run on a switch: their profiles are written for any host).
        let allowed = profile.get("SAI_RS_VIRTUAL_ON_HARDWARE") == Some("1")
            || std::env::var_os("SAI_RS_VIRTUAL_ON_HARDWARE").is_some_and(|v| v == "1");
        if !allowed && HARDWARE_MARKERS.iter().any(|m| Path::new(m).exists()) {
            return Err(Error::Failure);
        }
        let count = profile.number("SAI_RS_VIRTUAL_PORTS", 32, 1..=256)?;
        let lanes = profile.number("SAI_RS_VIRTUAL_LANES", 4, 1..=8)?;
        let speed = profile.number("SAI_RS_VIRTUAL_SPEED", 100_000, 1..=800_000)?;
        self.link_follows_admin = match profile.get("SAI_RS_VIRTUAL_LINK") {
            None | Some("admin") => true,
            Some("none") => false,
            Some(_) => return Err(Error::InvalidParameter),
        };
        let netdev_mode = match profile.get("SAI_RS_VIRTUAL_NETDEV") {
            None | Some("none") => NetdevMode::None,
            Some("tap") => NetdevMode::Tap,
            Some(_) => return Err(Error::InvalidParameter),
        };
        let wire = parse_wire(profile.get("SAI_RS_VIRTUAL_WIRE").unwrap_or("")).ok_or(Error::InvalidParameter)?;
        // Names the kernel refuses are refused here, not at the first host interface.
        if wire
            .iter()
            .any(|(a, b)| !netdev::valid_name(a) || !netdev::valid_name(b))
        {
            return Err(Error::InvalidParameter);
        }
        // A wire needs the TAP.
        if !wire.is_empty() && netdev_mode != NetdevMode::Tap {
            return Err(Error::InvalidParameter);
        }
        if netdev_mode == NetdevMode::Tap {
            // Never next to a real chip, whatever SAI_RS_VIRTUAL_ON_HARDWARE says: its KNET
            // netdevs have the same names.
            if HARDWARE_MARKERS.iter().any(|m| Path::new(m).exists()) {
                netdev::note(format_args!(
                    "SAI_RS_VIRTUAL_NETDEV=tap refused: this host has a switch chip"
                ));
                return Err(Error::Failure);
            }
            if !netdev::tun_available() {
                netdev::note(format_args!(
                    "SAI_RS_VIRTUAL_NETDEV=tap: /dev/net/tun cannot be opened read-write"
                ));
                return Err(Error::Failure);
            }
        }
        self.netdev_mode = netdev_mode;
        self.wire = wire;
        self.netifs.clear();
        self.ports.clear();
        self.vlans.clear();
        self.objects.clear();
        self.entries.clear();
        self.counters.clear();
        for i in 0..count {
            // Logical ports from 1 (0 is the CPU port, as on the Broadcom chips).
            let hw = HwPort(u16::try_from(i + 1).map_err(|_| Error::InvalidParameter)?);
            let first_lane = i * lanes + 1;
            // An SDK leaves ports enabled after init; so does this chip (the SAI layer disables them).
            self.ports.insert(
                hw,
                Port {
                    lanes: (first_lane..first_lane + lanes).collect(),
                    speed,
                    admin: true,
                    link: false,
                    up: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        // The chip's default VLAN: every front-panel port, untagged.
        self.vlans
            .insert(Vid::DEFAULT, self.ports.keys().map(|hw| (*hw, true)).collect());
        self.events = Some(events);
        Ok(())
    }

    fn shutdown(&mut self) {
        // The netdevs first: their wire threads stop before anything else goes.
        self.netifs.clear();
        self.ports.clear();
        self.vlans.clear();
        self.objects.clear();
        self.entries.clear();
        self.counters.clear();
        self.events = None;
        self.fail = None;
    }

    fn ports(&self) -> Vec<PortInfo> {
        self.ports
            .iter()
            .map(|(hw, p)| PortInfo {
                hw: *hw,
                lanes: p.lanes.clone(),
                speed: p.speed,
            })
            .collect()
    }

    fn cpu_port(&self) -> HwPort {
        HwPort(0)
    }

    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()> {
        self.check(Op::PortAdminSet)?;
        let port = self.ports.get_mut(&hw).ok_or(Error::InvalidParameter)?;
        port.admin = up;
        port.sync_up();
        if self.link_follows_admin || !up {
            self.set_link(hw, up);
        }
        Ok(())
    }

    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        self.port(hw).map(|p| p.link)
    }

    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        self.check(Op::PortSpeedSet)?;
        if !SPEEDS.contains(&mbps) {
            return Err(Error::InvalidParameter);
        }
        self.ports.get_mut(&hw).ok_or(Error::InvalidParameter)?.speed = mbps;
        Ok(())
    }

    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        self.port(hw).map(|p| p.speed)
    }

    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        self.port(hw).map(|_| SPEEDS.to_vec())
    }

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        self.check(Op::VlanCreate)?;
        if self.vlans.contains_key(&vid) {
            return Err(Error::ItemAlreadyExists);
        }
        self.vlans.insert(vid, BTreeMap::new());
        Ok(())
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        self.check(Op::VlanDestroy)?;
        self.vlans.remove(&vid).map(|_| ()).ok_or(Error::ItemNotFound)
    }

    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        self.check(Op::VlanPortAdd)?;
        self.port(hw)?;
        self.vlans
            .get_mut(&vid)
            .ok_or(Error::ItemNotFound)?
            .insert(hw, untagged);
        Ok(())
    }

    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        self.check(Op::VlanPortRemove)?;
        self.vlans
            .get_mut(&vid)
            .ok_or(Error::ItemNotFound)?
            .remove(&hw)
            .map(|_| ())
            .ok_or(Error::ItemNotFound)
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        self.vlans
            .get(&vid)
            .map(|m| m.iter().map(|(hw, untagged)| (*hw, *untagged)).collect())
            .ok_or(Error::ItemNotFound)
    }

    fn port_queues(&self) -> u8 {
        QUEUES_PER_PORT
    }

    fn port_priority_groups(&self) -> u8 {
        PRIORITY_GROUPS_PER_PORT
    }

    fn limits(&self) -> Result<ChipLimits> {
        Ok(LIMITS)
    }

    fn programs(&self, _object_type: sys::sai_object_type_t) -> bool {
        true
    }

    // Attribute-bag objects: this chip keeps whatever the SAI layer accepted. It records
    // which objects it has, so that the tests can compare model and chip.
    fn object_create(&mut self, object_type: sys::sai_object_type_t, id: RawOid, _attrs: &[Attr]) -> Result<()> {
        self.check(Op::ObjectCreate)?;
        if self.objects.insert(id, object_type).is_some() {
            return Err(Error::ItemAlreadyExists);
        }
        Ok(())
    }

    fn hostif_create(&mut self, id: RawOid, netdev: &HostifNetdev<'_>, _attrs: &[Attr]) -> Result<()> {
        self.check(Op::ObjectCreate)?;
        let gate = Arc::clone(&self.port(netdev.port)?.up);
        if self.objects.contains_key(&id) || self.netifs.values().any(|n| n.name == netdev.name) {
            return Err(Error::ItemAlreadyExists);
        }
        let kernel = match self.netdev_mode {
            NetdevMode::None => None,
            NetdevMode::Tap => {
                let wire_to = self.wire.get(netdev.name).map(String::as_str);
                let kernel = netdev::Kernel::open(netdev.name, netdev.port, netdev.mac, wire_to, &gate)?;
                // As the C++ SAI layer: the carrier as created, and a failure here is only logged.
                let _ = kernel.set_carrier(netdev.oper_up);
                Some(kernel)
            }
        };
        self.netifs.insert(
            id,
            Netif {
                name: netdev.name.to_owned(),
                port: netdev.port,
                carrier: netdev.oper_up,
                kernel,
            },
        );
        self.objects.insert(id, sys::SAI_OBJECT_TYPE_HOSTIF);
        Ok(())
    }

    fn object_set(&mut self, object_type: sys::sai_object_type_t, id: RawOid, attr: &Attr) -> Result<()> {
        self.check(Op::ObjectSet)?;
        // A netdev's carrier is its host interface's oper status (C++: netif_carrier_set).
        if let (sys::SAI_HOSTIF_ATTR_OPER_STATUS, Value::Bool(up), Some(netif)) =
            (attr.id, &attr.value, self.netifs.get_mut(&id))
        {
            if let Some(kernel) = &netif.kernel {
                kernel.set_carrier(*up)?;
            }
            netif.carrier = *up;
        }
        // Ports and the objects that come with the switch are not created through
        // object_create; everything else must be known.
        let builtin = matches!(
            object_type,
            sys::SAI_OBJECT_TYPE_PORT
                | sys::SAI_OBJECT_TYPE_QUEUE
                | sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP
                | sys::SAI_OBJECT_TYPE_SCHEDULER_GROUP
                | sys::SAI_OBJECT_TYPE_STP
                | sys::SAI_OBJECT_TYPE_HASH
                | sys::SAI_OBJECT_TYPE_VIRTUAL_ROUTER
                | sys::SAI_OBJECT_TYPE_HOSTIF_TRAP_GROUP
        );
        if builtin || self.objects.contains_key(&id) {
            Ok(())
        } else {
            Err(Error::ItemNotFound)
        }
    }

    fn object_remove(&mut self, _object_type: sys::sai_object_type_t, id: RawOid) -> Result<()> {
        self.check(Op::ObjectRemove)?;
        self.objects.remove(&id).ok_or(Error::ItemNotFound)?;
        // Its netdev, if it is a host interface: the wire stops, the TAP goes.
        self.netifs.remove(&id);
        Ok(())
    }

    fn entry_create(&mut self, key: &EntryKey, _attrs: &[Attr]) -> Result<()> {
        self.check(Op::EntryCreate)?;
        if self.entries.insert(*key) {
            Ok(())
        } else {
            Err(Error::ItemAlreadyExists)
        }
    }

    fn entry_set(&mut self, key: &EntryKey, _attr: &Attr) -> Result<()> {
        self.check(Op::EntrySet)?;
        if self.entries.contains(key) {
            Ok(())
        } else {
            Err(Error::ItemNotFound)
        }
    }

    fn entry_remove(&mut self, key: &EntryKey) -> Result<()> {
        self.check(Op::EntryRemove)?;
        if self.entries.remove(key) {
            Ok(())
        } else {
            Err(Error::ItemNotFound)
        }
    }

    fn stats_supported(&self, object_type: sys::sai_object_type_t) -> Result<Vec<sys::sai_stat_id_t>> {
        match object_type {
            sys::SAI_OBJECT_TYPE_PORT => Ok(PORT_STATS.to_vec()),
            sys::SAI_OBJECT_TYPE_QUEUE => Ok(QUEUE_STATS.to_vec()),
            sys::SAI_OBJECT_TYPE_INGRESS_PRIORITY_GROUP => Ok(PRIORITY_GROUP_STATS.to_vec()),
            _ => Err(Error::NotSupported),
        }
    }

    fn stats_get(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>> {
        let known = counters_of(object);
        if known.is_empty() || counters.iter().any(|c| !known.contains(c)) {
            return Err(Error::NotSupported);
        }
        let values = counters
            .iter()
            .map(|c| self.counters.get(&(object, *c)).copied().unwrap_or(0))
            .collect();
        if clear {
            self.stats_clear(object, counters)?;
        }
        Ok(values)
    }

    fn stats_clear(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t]) -> Result<()> {
        let known = counters_of(object);
        if known.is_empty() || counters.iter().any(|c| !known.contains(c)) {
            return Err(Error::NotSupported);
        }
        for counter in counters {
            self.counters.remove(&(object, *counter));
        }
        Ok(())
    }
}

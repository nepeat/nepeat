//! The virtual backend: an in-memory chip with the behaviour of the C++ fake backend
//! (`sai-openbcm/src/fake_backend.{h,cpp}`). Deterministic, no hardware, no threads (except
//! the TAP wires); link and FDB events are queued synchronously from the call that causes them.
//! It can be told to fail calls of an operation ([`VirtualBackend::fail_next`],
//! [`VirtualBackend::fail_nth`]), which is how the tests check that a failed backend call leaves
//! the object model unchanged.
//!
//! Profile keys (each also under the C++ fake's name, `SAI_OPENBCM_FAKE_*`; the
//! `SAI_RS_VIRTUAL_*` name wins when both are set):
//!
//! | key | default | meaning |
//! |---|---|---|
//! | `SAI_RS_VIRTUAL_PORTS` | 32 | front-panel ports, 1..=256 |
//! | `SAI_RS_VIRTUAL_LANES` | 4 | lanes per port, 1..=8 |
//! | `SAI_RS_VIRTUAL_SPEED` | 100000 | initial speed, Mb/s |
//! | `SAI_RS_VIRTUAL_LINK` | `admin` | `admin`: link follows admin state (and loopback); `none`: never up |
//! | `SAI_RS_VIRTUAL_ON_HARDWARE` | | `1`: start although the host has switch chip devices (also from the environment) |
//! | `SAI_RS_VIRTUAL_NETDEV` | `none` | `tap`: each host interface is also a Linux TAP netdev ([`netdev`]) |
//! | `SAI_RS_VIRTUAL_WIRE` | | `<netif>:<ifname>,...` (tap only): that netdev's frames to and from host interface `<ifname>` |
//! | `SAI_RS_VIRTUAL_COMPAT` | `rust` | `cpp`: the C++ fake's port numbers and answers where they differ (below) |
//!
//! Differences from the C++ fake, all kept for the existing Rust tests and listed for the
//! differential: logical ports are `1, 2, 3...` (C++, and here with `COMPAT=cpp`:
//! `1 + i * lanes`); every port supports 10/25/40/50/100G and an unknown speed is
//! `InvalidParameter` (C++ and `cpp`: by lane count, `NotSupported`); VLAN 1 can be destroyed and
//! removing a non-member is `ItemNotFound` (C++ and `cpp`: `ObjectInUse`, success); an unknown
//! port is `InvalidParameter` everywhere (C++: `INVALID_PORT_NUMBER`, which
//! [`crate::Error`] does not have); the CPU port is not listed as a VLAN member (the C++ keeps
//! it as an internal tagged member); a bad `LINK` value is refused (C++: anything but `none`
//! is `admin`); id counters keep counting across re-init (as the C++) but the shared CPU / drop
//! egress objects are made again (the C++ keeps a stale id).
use crate::backend::types::{Dest, FdbChange, FdbEvent, FpEntryId, GateVerdict, LinkGate, Loopback, PortStat};
use crate::backend::{
    Backend, BackendEvent, Capacities, EventSink, HostifNetdev, HwPort, LEGACY_CPP_OPS, PortInfo, Profile, StatsObject,
    Vid, ops,
};
use crate::error::{Error, Result};
use crate::model::EntryKey;
use crate::oid::RawOid;
use crate::value::{Attr, Value};
use sai_sys as sys;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

mod netdev;
mod state;
mod typed;
pub use netdev::{NetdevMode, parse_wire};
pub use state::{
    EgressKind, FpEntry, FpGroup, FpModel, Genl, HwQ, Intf, KnetNetif, KnetNetifs, L2Entry, L2Table, L2xModel, L3Model,
    Lags, MdioModel, MdioOp, MirrorModel, QosModel, Toggles, Traps, VirtPort,
};

/// The operations [`VirtualBackend::fail_next`] can make fail (the first slice's; every other
/// operation by name with [`VirtualBackend::fail_nth`]).
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

impl Op {
    /// The operation's name (as in the call log).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::PortAdminSet => "port_admin_set",
            Self::PortSpeedSet => "port_speed_set",
            Self::VlanCreate => "vlan_create",
            Self::VlanDestroy => "vlan_destroy",
            Self::VlanPortAdd => "vlan_port_add",
            Self::VlanPortRemove => "vlan_port_remove",
            Self::ObjectCreate => "object_create",
            Self::ObjectSet => "object_set",
            Self::ObjectRemove => "object_remove",
            Self::EntryCreate => "entry_create",
            Self::EntrySet => "entry_set",
            Self::EntryRemove => "entry_remove",
        }
    }
}

/// The generic methods of [`Backend`] that can be made to fail by name.
const GENERIC_OPS: [&str; 7] = [
    "object_create",
    "object_set",
    "object_remove",
    "entry_create",
    "entry_set",
    "entry_remove",
    "stats_get",
];

/// Armed failures.
#[derive(Debug, Default)]
struct Faults {
    /// [`VirtualBackend::fail_next`]: one at a time, the next call of that operation.
    next: Option<(&'static str, Error)>,
    /// [`VirtualBackend::fail_nth`]: per operation, calls to let pass first and the error.
    nth: BTreeMap<&'static str, (u32, Error)>,
}

/// How much of the C++ fake's numbering and answers the chip takes on (`SAI_RS_VIRTUAL_COMPAT`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compat {
    /// The Rust virtual chip's own (the default).
    #[default]
    Rust,
    /// The C++ fake's port numbers, speeds and status codes where they differ.
    Cpp,
}

/// A host interface of type NETDEV as the chip has it (the C++ fake's `Netif`; the generic
/// path, [`Backend::hostif_create`]).
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
pub const PROFILE_KEYS: [&str; 15] = [
    "SAI_RS_VIRTUAL_PORTS",
    "SAI_RS_VIRTUAL_LANES",
    "SAI_RS_VIRTUAL_SPEED",
    "SAI_RS_VIRTUAL_LINK",
    "SAI_RS_VIRTUAL_ON_HARDWARE",
    "SAI_RS_VIRTUAL_NETDEV",
    "SAI_RS_VIRTUAL_WIRE",
    "SAI_RS_VIRTUAL_COMPAT",
    "SAI_OPENBCM_FAKE_PORTS",
    "SAI_OPENBCM_FAKE_LANES",
    "SAI_OPENBCM_FAKE_SPEED",
    "SAI_OPENBCM_FAKE_LINK",
    "SAI_OPENBCM_FAKE_ON_HARDWARE",
    "SAI_OPENBCM_FAKE_NETDEV",
    "SAI_OPENBCM_FAKE_WIRE",
];

/// A key and its C++ name.
fn key(name: &str) -> [String; 2] {
    [format!("SAI_RS_VIRTUAL_{name}"), format!("SAI_OPENBCM_FAKE_{name}")]
}

fn get<'a>(profile: &'a Profile, name: &str) -> Option<&'a str> {
    let [rs, cpp] = key(name);
    profile.get_any(&[&rs, &cpp])
}

fn number(profile: &Profile, name: &str, default: u32, range: core::ops::RangeInclusive<u32>) -> Result<u32> {
    let [rs, cpp] = key(name);
    let chosen = if profile.get(&rs).is_some() { rs } else { cpp };
    profile.number(&chosen, default, range)
}

/// Device nodes of the Broadcom kernel modules: a host that has them has a real switch chip,
/// and a virtual switch answering "success" there would hide that nothing is programmed.
const HARDWARE_MARKERS: [&str; 2] = ["/dev/linux-user-bde", "/dev/linux-kernel-bde"];
const SPEEDS: [u32; 5] = [10_000, 25_000, 40_000, 50_000, 100_000];
/// Queues and priority groups per port, as the C++ sai-openbcm builds them.
const QUEUES_PER_PORT: u8 = 8;
const PRIORITY_GROUPS_PER_PORT: u8 = 8;
/// Table sizes: the constants of the C++ fake backend (Tomahawk values).
const LIMITS: Capacities = Capacities {
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

/// The link gates, by port.
#[derive(Default)]
struct Gates(BTreeMap<HwPort, LinkGate>);

impl core::fmt::Debug for Gates {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

/// Counters for the ids the chip hands out. They keep counting across re-init (as the C++).
#[derive(Debug)]
struct NextIds {
    tid: u32,
    netif: u32,
    filter: u32,
    intf: u32,
    station: u32,
    egress: u32,
    ecmp: u32,
    fp_group: u32,
    fp_entry: u32,
    policer: u32,
    /// Shared by `QoS` maps, WRED profiles, mirror sessions, samplers and generic netlink.
    qos: u32,
}

impl Default for NextIds {
    /// The C++ fake's first ids.
    fn default() -> Self {
        Self {
            tid: 1,
            netif: 1,
            filter: 2,
            intf: 1,
            station: 1,
            egress: 100_002,
            ecmp: 200_000,
            fp_group: 1,
            fp_entry: 1,
            policer: 1,
            qos: 1,
        }
    }
}

fn take(counter: &mut u32) -> u32 {
    let id = *counter;
    *counter = counter.wrapping_add(1);
    id
}

/// The in-memory chip.
#[derive(Debug, Default)]
pub struct VirtualBackend {
    ports: BTreeMap<HwPort, VirtPort>,
    vlans: BTreeMap<Vid, BTreeMap<HwPort, bool>>,
    events: Option<EventSink>,
    link_follows_admin: bool,
    compat: Compat,
    faults: Mutex<Faults>,
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
    /// The host interfaces of type NETDEV, by object id (the generic path).
    netifs: BTreeMap<RawOid, Netif>,
    gates: Gates,
    mdio: Arc<Mutex<MdioModel>>,
    shell_log: Vec<String>,
    temps: Vec<i32>,
    temp_fail: bool,
    lags: Lags,
    l2: L2Table,
    age_seconds: u32,
    knet: KnetNetifs,
    traps: Traps,
    l3: L3Model,
    fp: FpModel,
    qos: QosModel,
    mirror: MirrorModel,
    l2x: L2xModel,
    toggles: Toggles,
    next: NextIds,
}

impl VirtualBackend {
    /// A chip that is not initialised yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn faults(&self) -> std::sync::MutexGuard<'_, Faults> {
        self.faults.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes the next call of `op` fail with `error` (once; arming another replaces it).
    pub fn fail_next(&mut self, op: Op, error: Error) {
        self.faults().next = Some((op.name(), error));
    }

    /// Makes the `n`th call (1: the next) of the operation named `op` fail with `error`, once.
    /// `op` is any [`ob::Backend` operation name](crate::backend::ops::OP_NAMES), one of
    /// [`LEGACY_CPP_OPS`], or a generic method; `InvalidParameter` for another name or `n == 0`.
    pub fn fail_nth(&mut self, op: &str, n: u32, error: Error) -> Result<()> {
        let name = ops::OP_NAMES
            .iter()
            .chain(LEGACY_CPP_OPS.iter())
            .chain(GENERIC_OPS.iter())
            .find(|name| **name == op)
            .ok_or(Error::InvalidParameter)?;
        if n == 0 {
            return Err(Error::InvalidParameter);
        }
        self.faults().nth.insert(name, (n - 1, error));
        Ok(())
    }

    /// Fails this call if a fault for `op` is due.
    fn check(&self, op: &'static str) -> Result<()> {
        let mut faults = self.faults();
        if let Some((failing, error)) = faults.next {
            if failing == op {
                faults.next = None;
                return Err(error);
            }
        }
        match faults.nth.get_mut(op) {
            Some((0, error)) => {
                let error = *error;
                faults.nth.remove(op);
                Err(error)
            }
            Some((left, _)) => {
                *left -= 1;
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Changes a port's link as a cable would, and reports it (no gate asked).
    pub fn inject_link(&mut self, hw: HwPort, up: bool) -> Result<()> {
        self.port(hw)?;
        self.set_link(hw, up);
        Ok(())
    }

    /// One link scan pass: every port's link is worked out again (gates asked) and changes are
    /// reported (C++ `linkscan`).
    pub fn linkscan(&mut self) {
        let ports: Vec<HwPort> = self.ports.keys().copied().collect();
        for hw in ports {
            self.update_link(hw);
        }
    }

    /// The chip learned or aged an address on its own: the L2 table changes and the event is
    /// reported (C++ `inject_fdb`).
    pub fn inject_fdb(&mut self, event: FdbEvent) {
        match event.change {
            FdbChange::Learned => {
                self.l2.insert(
                    (event.mac, event.vid),
                    L2Entry {
                        dest: event.hw_port,
                        is_static: false,
                        discard: false,
                    },
                );
            }
            FdbChange::Aged => {
                self.l2.remove(&(event.mac, event.vid));
            }
        }
        if let Some(events) = &self.events {
            events.send(BackendEvent::Fdb(event));
        }
    }

    /// Traffic on a field entry: its counter grows (C++ `fp_hit`).
    pub fn fp_hit(&mut self, eid: FpEntryId, packets: u64, bytes: u64) {
        if let Some(entry) = self.fp.entries.get_mut(&eid) {
            entry.packets = entry.packets.saturating_add(packets);
            entry.bytes = entry.bytes.saturating_add(bytes);
        }
    }

    /// Sets a port counter as traffic would (C++ `set_stat`).
    pub fn set_stat(&mut self, hw: HwPort, stat: PortStat, value: u64) {
        self.counters.insert((StatsObject::Port(hw), stat.sai_id()), value);
    }

    /// Sets a PFC frame counter (C++ `pfc_stats`).
    pub fn set_pfc_stat(&mut self, hw: HwPort, prio: u8, tx: bool, value: u64) {
        self.qos.pfc_stats.insert((hw, prio, tx), value);
    }

    /// The temperature monitors' current values, tenths of a degree C (C++ `temps`).
    pub fn set_temps(&mut self, temps: Vec<i32>) {
        self.temps = temps;
    }

    /// Temperature reads fail (C++ `temp_fail`: the sensor is gone).
    pub fn set_temp_fail(&mut self, fail: bool) {
        self.temp_fail = fail;
    }

    /// The optional models (C++ `qos_on`, `mirror_on`, `l2x_on`), to change before or after init.
    pub fn toggles_mut(&mut self) -> &mut Toggles {
        &mut self.toggles
    }

    /// The optional models' switches.
    pub fn toggles(&self) -> Toggles {
        self.toggles
    }

    /// The MDIO model (registers, log, dead PHYs), behind its own lock: a test or a link gate
    /// may hold the handle.
    pub fn mdio(&self) -> Arc<Mutex<MdioModel>> {
        Arc::clone(&self.mdio)
    }

    /// The diag shell commands run so far (C++ `shell_log`).
    pub fn shell_log(&self) -> &[String] {
        &self.shell_log
    }

    /// A port's state. For tests.
    pub fn port_state(&self, hw: HwPort) -> Option<&VirtPort> {
        self.ports.get(&hw)
    }

    /// The ports that have a link gate.
    pub fn gated_ports(&self) -> Vec<HwPort> {
        self.gates.0.keys().copied().collect()
    }

    /// The LAGs and their members.
    pub fn lags(&self) -> &Lags {
        &self.lags
    }

    /// The L2 table.
    pub fn l2_table(&self) -> &L2Table {
        &self.l2
    }

    /// The aging time (C++ `age_seconds`).
    pub fn age_seconds(&self) -> u32 {
        self.age_seconds
    }

    /// The KNET netifs (the typed path).
    pub fn knet_netifs(&self) -> &KnetNetifs {
        &self.knet
    }

    /// The traps the chip steers.
    pub fn traps(&self) -> &Traps {
        &self.traps
    }

    /// The L3 tables.
    pub fn l3(&self) -> &L3Model {
        &self.l3
    }

    /// The field processor and meters.
    pub fn fp(&self) -> &FpModel {
        &self.fp
    }

    /// The `QoS` model.
    pub fn qos(&self) -> &QosModel {
        &self.qos
    }

    /// Mirror sessions, samplers, generic netlink.
    pub fn mirror(&self) -> &MirrorModel {
        &self.mirror
    }

    /// Hash, storm and flood control.
    pub fn l2x(&self) -> &L2xModel {
        &self.l2x
    }

    /// Which flavour the profile chose.
    pub fn compat(&self) -> Compat {
        self.compat
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

    fn port(&self, hw: HwPort) -> Result<&VirtPort> {
        self.ports.get(&hw).ok_or(Error::InvalidParameter)
    }

    fn port_mut(&mut self, hw: HwPort) -> Result<&mut VirtPort> {
        self.ports.get_mut(&hw).ok_or(Error::InvalidParameter)
    }

    /// A port, or a LAG that exists (C++ `dest_ok`).
    fn dest_ok(&self, dest: Dest) -> bool {
        match dest {
            Dest::Port(hw) => self.ports.contains_key(&hw),
            Dest::Lag(tid) => self.lags.contains_key(&tid),
        }
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

    /// Works out a port's link as the SDK's link scan does (C++ `update_link`): up when enabled
    /// and (in loopback or the link follows admin), unless its gate says down.
    fn update_link(&mut self, hw: HwPort) {
        let Some(port) = self.ports.get(&hw) else { return };
        let mut up = port.admin && (port.lb != Loopback::None || self.link_follows_admin);
        if up && self.gates.0.get(&hw).is_some_and(|gate| gate() == GateVerdict::Down) {
            up = false;
        }
        self.set_link(hw, up);
    }

    /// The speeds a port supports.
    fn speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        let port = self.port(hw)?;
        Ok(match (self.compat, port.lanes.len()) {
            (Compat::Rust, _) => SPEEDS.to_vec(),
            (Compat::Cpp, n) if n >= 4 => vec![40_000, 100_000],
            (Compat::Cpp, 2) => vec![50_000],
            (Compat::Cpp, _) => vec![10_000, 25_000],
        })
    }

    /// Forgets everything the chip holds (not the id counters, not the toggles).
    fn clear(&mut self) {
        // The netdevs first: their wire threads stop before anything else goes.
        self.netifs.clear();
        self.knet.clear();
        self.ports.clear();
        self.vlans.clear();
        self.objects.clear();
        self.entries.clear();
        self.counters.clear();
        self.gates.0.clear();
        *self.mdio.lock().unwrap_or_else(PoisonError::into_inner) = MdioModel::default();
        self.shell_log.clear();
        self.temps.clear();
        self.temp_fail = false;
        self.lags.clear();
        self.l2.clear();
        self.age_seconds = 300;
        self.traps.clear();
        self.l3 = L3Model::default();
        self.fp = FpModel::default();
        self.qos = QosModel::default();
        self.mirror = MirrorModel::default();
        self.l2x = L2xModel::default();
    }

    fn read_profile(&mut self, profile: &Profile) -> Result<(u32, u32, u32)> {
        // The profile key, or the same name in the process environment (how the test
        // suites are run on a switch: their profiles are written for any host).
        let allowed = get(profile, "ON_HARDWARE") == Some("1")
            || std::env::var_os("SAI_RS_VIRTUAL_ON_HARDWARE").is_some_and(|v| v == "1");
        if !allowed && HARDWARE_MARKERS.iter().any(|m| Path::new(m).exists()) {
            return Err(Error::Failure);
        }
        let count = number(profile, "PORTS", 32, 1..=256)?;
        let lanes = number(profile, "LANES", 4, 1..=8)?;
        let speed = number(profile, "SPEED", 100_000, 1..=800_000)?;
        self.link_follows_admin = match get(profile, "LINK") {
            None | Some("admin") => true,
            Some("none") => false,
            Some(_) => return Err(Error::InvalidParameter),
        };
        self.compat = match get(profile, "COMPAT") {
            None | Some("rust") => Compat::Rust,
            Some("cpp") => Compat::Cpp,
            Some(_) => return Err(Error::InvalidParameter),
        };
        let netdev_mode = match get(profile, "NETDEV") {
            None | Some("none") => NetdevMode::None,
            Some("tap") => NetdevMode::Tap,
            Some(_) => return Err(Error::InvalidParameter),
        };
        let wire = parse_wire(get(profile, "WIRE").unwrap_or("")).ok_or(Error::InvalidParameter)?;
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
        Ok((count, lanes, speed))
    }
}

impl Backend for VirtualBackend {
    fn name(&self) -> &'static str {
        "virtual"
    }

    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        self.check("init")?;
        let (count, lanes, speed) = self.read_profile(profile)?;
        self.clear();
        for i in 0..count {
            // Logical ports from 1 (0 is the CPU port, as on the Broadcom chips); the C++ fake
            // leaves gaps like the SDK (one number per lane).
            let number = match self.compat {
                Compat::Rust => i + 1,
                Compat::Cpp => 1 + i * lanes,
            };
            let hw = HwPort(u16::try_from(number).map_err(|_| Error::InvalidParameter)?);
            let first_lane = i * lanes + 1;
            let name = format!("{}{i}", if lanes >= 4 { "ce" } else { "xe" });
            self.ports.insert(
                hw,
                VirtPort::new((first_lane..first_lane + lanes).collect(), speed, name),
            );
        }
        // The chip's default VLAN: every front-panel port, untagged.
        self.vlans
            .insert(Vid::DEFAULT, self.ports.keys().map(|hw| (*hw, true)).collect());
        // Eight monitors, 40.0 C up in steps of 1.5 C (the C++ fake's).
        self.temps = (0..8).map(|i| 400 + 15 * i).collect();
        self.events = Some(events);
        Ok(())
    }

    fn shutdown(&mut self) {
        self.clear();
        self.events = None;
        *self.faults() = Faults::default();
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
        self.check("port_admin_set")?;
        let port = self.port_mut(hw)?;
        port.admin = up;
        port.sync_up();
        self.update_link(hw);
        Ok(())
    }

    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        self.check("port_link_get")?;
        self.port(hw).map(|p| p.link)
    }

    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        self.check("port_speed_set")?;
        if !self.speeds(hw)?.contains(&mbps) {
            return Err(match self.compat {
                Compat::Rust => Error::InvalidParameter,
                Compat::Cpp => Error::NotSupported,
            });
        }
        self.port_mut(hw)?.speed = mbps;
        Ok(())
    }

    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        self.check("port_speed_get")?;
        self.port(hw).map(|p| p.speed)
    }

    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        self.check("port_supported_speeds")?;
        self.speeds(hw)
    }

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        self.check("vlan_create")?;
        if self.vlans.contains_key(&vid) {
            return Err(Error::ItemAlreadyExists);
        }
        self.vlans.insert(vid, BTreeMap::new());
        Ok(())
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        self.check("vlan_destroy")?;
        if !self.vlans.contains_key(&vid) {
            return Err(Error::ItemNotFound);
        }
        // The SDK refuses to destroy the default VLAN.
        if self.compat == Compat::Cpp && vid == Vid::DEFAULT {
            return Err(Error::ObjectInUse);
        }
        self.vlans.remove(&vid);
        Ok(())
    }

    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        self.check("vlan_port_add")?;
        if self.compat == Compat::Cpp && !self.vlans.contains_key(&vid) {
            return Err(Error::ItemNotFound);
        }
        self.port(hw)?;
        self.vlans
            .get_mut(&vid)
            .ok_or(Error::ItemNotFound)?
            .insert(hw, untagged);
        Ok(())
    }

    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        self.check("vlan_port_remove")?;
        let members = self.vlans.get_mut(&vid).ok_or(Error::ItemNotFound)?;
        match (members.remove(&hw), self.compat) {
            (Some(_), _) | (None, Compat::Cpp) => Ok(()),
            (None, Compat::Rust) => Err(Error::ItemNotFound),
        }
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        self.check("vlan_members")?;
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

    fn limits(&self) -> Result<Capacities> {
        Ok(LIMITS)
    }

    fn programs(&self, _object_type: sys::sai_object_type_t) -> bool {
        true
    }

    // Attribute-bag objects: this chip keeps whatever the SAI layer accepted. It records
    // which objects it has, so that the tests can compare model and chip.
    fn object_create(&mut self, object_type: sys::sai_object_type_t, id: RawOid, _attrs: &[Attr]) -> Result<()> {
        self.check("object_create")?;
        if self.objects.insert(id, object_type).is_some() {
            return Err(Error::ItemAlreadyExists);
        }
        Ok(())
    }

    fn hostif_create(&mut self, id: RawOid, netdev: &HostifNetdev<'_>, _attrs: &[Attr]) -> Result<()> {
        self.check("object_create")?;
        let gate = Arc::clone(&self.port(netdev.port)?.up);
        if self.objects.contains_key(&id) || self.netdev_name_taken(netdev.name) {
            return Err(Error::ItemAlreadyExists);
        }
        let kernel = self.open_kernel(netdev.name, netdev.port, netdev.mac, &gate, netdev.oper_up)?;
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
        self.check("object_set")?;
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
        self.check("object_remove")?;
        self.objects.remove(&id).ok_or(Error::ItemNotFound)?;
        // Its netdev, if it is a host interface: the wire stops, the TAP goes.
        self.netifs.remove(&id);
        Ok(())
    }

    fn entry_create(&mut self, key: &EntryKey, _attrs: &[Attr]) -> Result<()> {
        self.check("entry_create")?;
        if self.entries.insert(*key) {
            Ok(())
        } else {
            Err(Error::ItemAlreadyExists)
        }
    }

    fn entry_set(&mut self, key: &EntryKey, _attr: &Attr) -> Result<()> {
        self.check("entry_set")?;
        if self.entries.contains(key) {
            Ok(())
        } else {
            Err(Error::ItemNotFound)
        }
    }

    fn entry_remove(&mut self, key: &EntryKey) -> Result<()> {
        self.check("entry_remove")?;
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
        self.check("stats_get")?;
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

impl VirtualBackend {
    /// Whether a netdev of this name exists (either path).
    fn netdev_name_taken(&self, name: &str) -> bool {
        self.netifs.values().any(|n| n.name == name) || self.knet.values().any(|n| n.name == name)
    }

    /// The kernel side of a netdev in mode `tap`, carrier as given (a carrier failure is only
    /// logged, as the C++ SAI layer does); none otherwise.
    fn open_kernel(
        &self,
        name: &str,
        port: HwPort,
        mac: [u8; 6],
        gate: &Arc<std::sync::atomic::AtomicBool>,
        carrier: bool,
    ) -> Result<Option<netdev::Kernel>> {
        match self.netdev_mode {
            NetdevMode::None => Ok(None),
            NetdevMode::Tap => {
                let wire_to = self.wire.get(name).map(String::as_str);
                let kernel = netdev::Kernel::open(name, port, mac, wire_to, gate)?;
                let _ = kernel.set_carrier(carrier);
                Ok(Some(kernel))
            }
        }
    }

    fn next_qos_id(&mut self) -> u32 {
        take(&mut self.next.qos)
    }
}

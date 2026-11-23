//! The virtual chip's state per group, as the C++ fake has it (`fake_backend.h`: its public
//! maps). Read by tests through the accessors of [`super::VirtualBackend`].
use crate::backend::types::{
    BufferCfg, Dest, EcmpId, EgressId, Fec, FilterId, FloodKind, FpEntryId, FpGroupId, FpRule, GenlId, IntfId,
    LagMember, Learn, Loopback, MacAddr, MdioDev, MirrorCfg, MirrorId, NetifId, PhyAddr, PolicerCfg, PolicerId,
    PortInterface, QosMapEntry, QosMapId, QosMapKind, RouteTarget, SampleId, SchedCfg, StationId, StormKind, TrapKind,
    TrunkId, Vrf, WredCfg, WredId,
};
use crate::backend::{HwPort, Vid};
use crate::value::IpPrefix;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A front-panel port (C++ `FakeBackend::Port`; counters are in the backend's counter map).
#[derive(Debug)]
pub struct VirtPort {
    /// Physical lanes.
    pub lanes: Vec<u32>,
    /// Speed in Mb/s.
    pub speed: u32,
    /// The SDK's name (`ce<n>` for four lanes or more, else `xe<n>`).
    pub name: String,
    /// Enabled.
    pub admin: bool,
    /// Link up.
    pub link: bool,
    /// FEC.
    pub fec: Fec,
    /// Autonegotiation.
    pub autoneg: bool,
    /// `port_interface_set` (`None`: never set).
    pub iface: Option<PortInterface>,
    /// `port_ifg_set` (0: never set).
    pub ifg: u32,
    /// `port_interface_set` + `port_ifg_set` calls.
    pub quirk_calls: u32,
    /// Loopback.
    pub lb: Loopback,
    /// Port VLAN id.
    pub pvid: Vid,
    /// Largest frame.
    pub frame_max: u32,
    /// Learning mode.
    pub learn: Learn,
    /// `admin && link`, for the wire threads (which read it without any lock).
    pub(crate) up: Arc<AtomicBool>,
}

impl VirtPort {
    pub(crate) fn new(lanes: Vec<u32>, speed: u32, name: String) -> Self {
        Self {
            lanes,
            speed,
            name,
            // An SDK leaves ports enabled after init; so does this chip (the SAI layer disables them).
            admin: true,
            link: false,
            fec: Fec::None,
            autoneg: false,
            iface: None,
            ifg: 0,
            quirk_calls: 0,
            lb: Loopback::None,
            pvid: Vid::DEFAULT,
            frame_max: 9412,
            learn: Learn::Hw,
            up: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn sync_up(&self) {
        self.up.store(self.admin && self.link, Ordering::Release);
    }
}

/// One MDIO access (C++ `FakeBackend::MdioOp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdioOp {
    /// `true`: a write.
    pub write: bool,
    /// The PHY.
    pub phy: PhyAddr,
    /// The device.
    pub dev: MdioDev,
    /// The register.
    pub reg: u16,
    /// The value read or written.
    pub val: u16,
}

/// External PHYs: a register file (unset registers read 0), every access logged; accesses to a
/// PHY in `dead` fail. Behind its own lock (the C++ `mdio_mu`): a link gate may do MDIO while
/// a link update runs.
#[derive(Debug, Default)]
pub struct MdioModel {
    /// Register values.
    pub regs: BTreeMap<(PhyAddr, MdioDev, u16), u16>,
    /// Every access, in order.
    pub log: Vec<MdioOp>,
    /// PHYs that do not answer.
    pub dead: BTreeSet<PhyAddr>,
    /// A device model behind the bus (tests: a stateful PHY), asked before the register file.
    pub device: Option<std::sync::Arc<dyn MdioDevice>>,
}

/// A device behind the virtual MDIO master (tests: a PHY's stateful firmware-message interface),
/// called with the MDIO model's lock held (one access at a time, as the SDK's MIIM lock).
pub trait MdioDevice: Send + Sync + std::fmt::Debug {
    /// A read: `Some` answers it, `None` leaves it to the register file.
    fn read(&self, phy: PhyAddr, dev: MdioDev, reg: u16) -> Option<u16>;
    /// A write (the register file keeps the value too).
    fn write(&self, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16);
}

/// An L2 entry (C++ `FakeBackend::L2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct L2Entry {
    /// Where it points.
    pub dest: Dest,
    /// Static.
    pub is_static: bool,
    /// Drop frames to it.
    pub discard: bool,
}

/// A KNET netif (C++ `FakeBackend::Netif`; the typed path, [`crate::backend::ops::Hostif`]).
#[derive(Debug)]
pub struct KnetNetif {
    /// Its name.
    pub name: String,
    /// Its port.
    pub hw: HwPort,
    /// Its VLAN.
    pub vid: Vid,
    /// Its MAC.
    pub mac: MacAddr,
    /// RX keeps the VLAN tag.
    pub keep_tag: bool,
    /// Carrier.
    pub carrier: bool,
    /// Its RX filter.
    pub filter: FilterId,
    pub(crate) kernel: Option<super::netdev::Kernel>,
}

impl KnetNetif {
    /// Whether it is a kernel netdev (mode `tap`).
    pub fn has_kernel_netdev(&self) -> bool {
        self.kernel.is_some()
    }

    /// Whether its wire carries frames (a thread runs; mode `tap` with `WIRE`).
    pub fn wired(&self) -> bool {
        self.kernel.as_ref().is_some_and(super::netdev::Kernel::wired)
    }
}

/// What an egress object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EgressKind {
    /// A next hop.
    Forward {
        /// Its L3 interface.
        intf: IntfId,
        /// The neighbor's MAC.
        dmac: MacAddr,
        /// Where it goes.
        dest: Dest,
        /// The VLAN.
        vid: Vid,
    },
    /// The shared CPU egress.
    Cpu,
    /// The shared drop egress.
    Drop,
}

/// An L3 interface (C++ `FakeBackend::Intf`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intf {
    /// Router MAC.
    pub mac: MacAddr,
    /// Its VLAN.
    pub vid: Vid,
    /// MTU.
    pub mtu: u32,
    /// VRF.
    pub vrf: Vrf,
}

/// The L3 tables (C++ `intfs`, `stations`, `egress`, `hosts`, `routes`, `ecmps`).
#[derive(Debug, Default)]
pub struct L3Model {
    /// L3 interfaces.
    pub intfs: BTreeMap<IntfId, Intf>,
    /// My-station entries.
    pub stations: BTreeMap<StationId, MacAddr>,
    /// Egress objects.
    pub egress: BTreeMap<EgressId, EgressKind>,
    /// Host entries.
    pub hosts: BTreeMap<(Vrf, IpAddr), EgressId>,
    /// Routes.
    pub routes: BTreeMap<(Vrf, IpPrefix), RouteTarget>,
    /// ECMP groups.
    pub ecmps: BTreeMap<EcmpId, Vec<EgressId>>,
    /// The shared CPU egress, once made.
    pub(crate) cpu_egress: Option<EgressId>,
    /// The shared drop egress, once made.
    pub(crate) drop_egress: Option<EgressId>,
}

/// A field group (C++ `FakeBackend::FpGroup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FpGroup {
    /// The qualifier set as a mask (`FpQual::bit`).
    pub qset: u32,
    /// Its priority.
    pub priority: i32,
}

/// A field entry (C++ `FakeBackend::FpEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FpEntry {
    /// Its group.
    pub gid: FpGroupId,
    /// What it does.
    pub rule: FpRule,
    /// Counted packets.
    pub packets: u64,
    /// Counted bytes.
    pub bytes: u64,
}

/// The field processor and meters (C++ `fp_groups`, `fp_entries`, `policers`).
#[derive(Debug, Default)]
pub struct FpModel {
    /// Groups.
    pub groups: BTreeMap<FpGroupId, FpGroup>,
    /// Entries.
    pub entries: BTreeMap<FpEntryId, FpEntry>,
    /// Meters.
    pub policers: BTreeMap<PolicerId, PolicerCfg>,
}

/// A queue or priority group: port and index.
pub type HwQ = (HwPort, u8);

/// The `QoS` model (C++ `qmaps` ... `pfc_stats`; only used when the SAI layer sees
/// `qos_supported`).
#[derive(Debug, Default)]
pub struct QosModel {
    /// Maps.
    pub qmaps: BTreeMap<QosMapId, (QosMapKind, Vec<QosMapEntry>)>,
    /// A port's map per kind.
    pub port_qmap: BTreeMap<(HwPort, QosMapKind), QosMapId>,
    /// Default traffic class per port.
    pub default_tc: BTreeMap<HwPort, u8>,
    /// PFC per port: separate, tx, rx.
    pub pfc: BTreeMap<HwPort, (bool, u8, u8)>,
    /// Queue schedulers (absent: the default).
    pub queue_sched: BTreeMap<HwQ, SchedCfg>,
    /// Port shapers.
    pub port_sched: BTreeMap<HwPort, SchedCfg>,
    /// WRED profiles.
    pub wreds: BTreeMap<WredId, WredCfg>,
    /// A queue's WRED profile.
    pub queue_wred: BTreeMap<HwQ, WredId>,
    /// Queue buffer profiles.
    pub queue_buf: BTreeMap<HwQ, BufferCfg>,
    /// Priority group buffer profiles.
    pub pg_buf: BTreeMap<HwQ, BufferCfg>,
    /// PFC frame counters a test set: (port, priority, tx).
    pub pfc_stats: BTreeMap<(HwPort, u8, bool), u64>,
}

/// A generic-netlink host interface (C++ `FakeBackend::Genl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Genl {
    /// Family name.
    pub name: String,
    /// Multicast group.
    pub mcgrp: String,
    /// The trap types whose packets go there.
    pub traps: BTreeSet<i32>,
}

/// Mirror sessions, sampling, generic netlink (C++ `mirrors` ... `genls`).
#[derive(Debug, Default)]
pub struct MirrorModel {
    /// Sessions.
    pub mirrors: BTreeMap<MirrorId, MirrorCfg>,
    /// A port's sessions per direction (true: ingress).
    pub port_mirrors: BTreeMap<(HwPort, bool), Vec<MirrorId>>,
    /// Samplers: rate.
    pub samples: BTreeMap<SampleId, u32>,
    /// A port's sampler per direction.
    pub port_samples: BTreeMap<(HwPort, bool), SampleId>,
    /// Generic-netlink interfaces.
    pub genls: BTreeMap<GenlId, Genl>,
}

/// Hash, storm and flood control (C++ `hash`, `storm`, `flood`; only with `l2x_on`).
#[derive(Debug, Default)]
pub struct L2xModel {
    /// LAG (true) / ECMP hash: fields and seed.
    pub hash: BTreeMap<bool, (Vec<i32>, u32)>,
    /// Storm policers.
    pub storm: BTreeMap<(HwPort, StormKind), PolicerId>,
    /// VLAN flood control.
    pub flood: BTreeMap<(Vid, FloodKind), bool>,
}

/// The test toggles of the C++ fake: optional models off unless a test turns them on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "three independent switches, as the C++")]
pub struct Toggles {
    /// `qos_supported` answers yes.
    pub qos_on: bool,
    /// `mirror_supported` answers yes; generic netlink works.
    pub mirror_on: bool,
    /// `hash_supported` answers yes; hash, storm and flood control work.
    pub l2x_on: bool,
}

/// The LAGs (C++ `lags`).
pub type Lags = BTreeMap<TrunkId, Vec<LagMember>>;

/// The L2 table (C++ `l2`).
pub type L2Table = BTreeMap<(MacAddr, Vid), L2Entry>;

/// The traps the chip steers (C++ `traps`).
pub type Traps = BTreeMap<TrapKind, bool>;

/// The KNET netifs (C++ `netifs`).
pub type KnetNetifs = BTreeMap<NetifId, KnetNetif>;

/// The temperature monitors (C++ `temps`, `temp_fail`), shared with the reader the backend
/// hands the publisher thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TempModel {
    /// Current values, tenths of a degree C.
    pub(crate) temps: Vec<i32>,
    /// Reads fail (the sensor is gone).
    pub(crate) fail: bool,
}

impl TempModel {
    /// One read (C++ `asic_temperatures`): the values, each peak 0.5 C above.
    pub(crate) fn read(&self) -> crate::error::Result<crate::backend::types::AsicTemps> {
        if self.fail {
            return Err(crate::error::Error::Failure);
        }
        Ok(crate::backend::types::AsicTemps {
            cur: self.temps.clone(),
            peak: self.temps.iter().map(|t| t + 5).collect(),
        })
    }
}

//! The virtual backend: an in-memory chip. Deterministic, no hardware, and it can be told to
//! fail the next call of an operation, which is how the tests check that a failed backend
//! call leaves the object model unchanged.
//!
//! Profile keys: `SAI_RS_VIRTUAL_PORTS` (32), `SAI_RS_VIRTUAL_LANES` (4),
//! `SAI_RS_VIRTUAL_SPEED` (100000), `SAI_RS_VIRTUAL_LINK` (`admin`: link follows admin state;
//! `none`: never up), `SAI_RS_VIRTUAL_ON_HARDWARE` (`1`: start although the host has switch
//! chip devices).
use crate::backend::{Backend, BackendEvent, EventSink, HwPort, PortInfo, Profile, Vid};
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::Path;

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
}

#[derive(Clone, Debug)]
struct Port {
    lanes: Vec<u32>,
    speed: u32,
    admin: bool,
    link: bool,
}

/// The profile keys this backend reads (the rim asks the SAI user for exactly these).
pub const PROFILE_KEYS: [&str; 5] = [
    "SAI_RS_VIRTUAL_PORTS",
    "SAI_RS_VIRTUAL_LANES",
    "SAI_RS_VIRTUAL_SPEED",
    "SAI_RS_VIRTUAL_LINK",
    "SAI_RS_VIRTUAL_ON_HARDWARE",
];

/// Device nodes of the Broadcom kernel modules: a host that has them has a real switch chip,
/// and a virtual switch answering "success" there would hide that nothing is programmed.
const HARDWARE_MARKERS: [&str; 2] = ["/dev/linux-user-bde", "/dev/linux-kernel-bde"];
const SPEEDS: [u32; 5] = [10_000, 25_000, 40_000, 50_000, 100_000];

/// The in-memory chip.
#[derive(Debug, Default)]
pub struct VirtualBackend {
    ports: BTreeMap<HwPort, Port>,
    vlans: BTreeMap<Vid, BTreeMap<HwPort, bool>>,
    events: Option<EventSink>,
    link_follows_admin: bool,
    fail: Option<(Op, Error)>,
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
        let port = self.ports.get_mut(&hw).ok_or(Error::InvalidParameter)?;
        if port.link != up {
            port.link = up;
            if let Some(events) = &self.events {
                events.send(BackendEvent::Link { hw, up });
            }
        }
        Ok(())
    }

    /// The VLANs the chip has, with members and tagging (true = untagged). For tests.
    pub fn vlans(&self) -> &BTreeMap<Vid, BTreeMap<HwPort, bool>> {
        &self.vlans
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
        if profile.get("SAI_RS_VIRTUAL_ON_HARDWARE") != Some("1")
            && HARDWARE_MARKERS.iter().any(|m| Path::new(m).exists())
        {
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
        self.ports.clear();
        self.vlans.clear();
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
        self.ports.clear();
        self.vlans.clear();
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
        self.ports.get_mut(&hw).ok_or(Error::InvalidParameter)?.admin = up;
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
}

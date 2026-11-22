//! What the object model needs from a switch chip: the operations of this slice, in the
//! spirit of the C++ `ob::Backend` (ports are the SDK's logical port numbers, VLANs are
//! 802.1Q VIDs, every call answers with a SAI-level error). Implementations: [`crate::virt`]
//! (in memory); later one over the C SDK glue.
//!
//! A backend reports what the chip does on its own (link changes) through the
//! [`EventSink`] it is given at [`Backend::init`]. Sending only queues: it may be called from
//! any thread, including an SDK callback, and never takes a lock of the SAI layer.
use crate::error::{Error, Result};
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
}

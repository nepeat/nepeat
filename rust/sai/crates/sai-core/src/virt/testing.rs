//! Test hooks of the virtual chip that need its private state (cargo feature `testing`): put
//! entries into its tables or take them out directly, bypassing the SAI layer, as the C++ test
//! port does with the fake backend's public maps (`tests/test_port_cpp.cpp`). Reads go through
//! the public accessors of [`VirtualBackend`].
use super::{L2Entry, VirtPort, VirtualBackend};
use crate::backend::types::{Dest, MacAddr};
use crate::backend::{HwPort, Vid};

impl VirtualBackend {
    /// Every front-panel port's state, by port number.
    pub fn port_states(&self) -> impl Iterator<Item = (HwPort, &VirtPort)> {
        self.ports.iter().map(|(hw, port)| (*hw, port))
    }

    /// Forgets the interface type and IFG last programmed on a port (`quirk_calls` stays);
    /// `false`: no such port.
    pub fn forget_port_quirks(&mut self, hw: HwPort) -> bool {
        let Some(port) = self.ports.get_mut(&hw) else {
            return false;
        };
        port.iface = None;
        port.ifg = 0;
        true
    }

    /// A dynamic L2 entry, straight into the chip's table (replacing one of the same key).
    pub fn insert_l2_directly(&mut self, mac: MacAddr, vid: Vid, dest: Dest) {
        self.l2.insert(
            (mac, vid),
            L2Entry {
                dest,
                is_static: false,
                discard: false,
            },
        );
    }

    /// An L2 entry, straight out of the chip's table.
    pub fn erase_l2_directly(&mut self, mac: MacAddr, vid: Vid) {
        self.l2.remove(&(mac, vid));
    }
}

// SPDX-License-Identifier: GPL-2.0

//! Ethernet addresses (`include/linux/etherdevice.h` helpers, in safe Rust).

/// A 48-bit Ethernet MAC address.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct MacAddr(pub(crate) [u8; 6]);

impl MacAddr {
    /// The all-ones broadcast address.
    pub(crate) const BROADCAST: Self = Self([0xff; 6]);

    /// `is_zero_ether_addr()`.
    pub(crate) fn is_zero(&self) -> bool {
        self.0 == [0; 6]
    }

    /// `is_multicast_ether_addr()`: group bit (bit 0 of the first octet).
    pub(crate) fn is_multicast(&self) -> bool {
        self.0[0] & 1 != 0
    }

    /// `is_valid_ether_addr()`: neither multicast (broadcast included) nor all zero.
    pub(crate) fn is_valid(&self) -> bool {
        !self.is_multicast() && !self.is_zero()
    }

}

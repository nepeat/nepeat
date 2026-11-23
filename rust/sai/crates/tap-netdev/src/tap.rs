//! A TAP netdev, owned.
use crate::name::IfName;
use crate::sys;
use core::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsFd;

/// The clone device.
const TUN_DEVICE: &str = "/dev/net/tun";

/// One TAP netdev. The netdev exists exactly as long as this value: it is not persistent, so
/// closing the file descriptor (drop) removes it.
pub struct Tap {
    file: File,
    name: IfName,
}

impl fmt::Debug for Tap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tap").field("name", &self.name).finish_non_exhaustive()
    }
}

impl Tap {
    /// Whether this process can make TAP netdevs at all: `/dev/net/tun` opens for reading and
    /// writing. Creates nothing.
    pub fn available() -> io::Result<()> {
        OpenOptions::new().read(true).write(true).open(TUN_DEVICE).map(drop)
    }

    /// Creates the TAP netdev `name` (`IFF_TAP | IFF_NO_PI`: frames without a header) with its
    /// carrier off (like a KNET netif until the host interface's oper status is set) and the
    /// MAC address `mac`. Admin state and MTU are left as the kernel makes them (down, 1500).
    /// On error nothing is left behind.
    pub fn create(name: IfName, mac: [u8; 6]) -> io::Result<Self> {
        let flags = libc::c_short::try_from(libc::IFF_TAP | libc::IFF_NO_PI)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // std opens with O_CLOEXEC.
        let file = OpenOptions::new().read(true).write(true).open(TUN_DEVICE)?;
        sys::tun_set_iff(file.as_fd(), &name, flags)?;
        // From here the netdev exists and goes away with `tap` on any error below.
        let tap = Self { file, name };
        sys::tun_set_carrier(tap.file.as_fd(), false)?;
        sys::tun_set_hwaddr(tap.file.as_fd(), &tap.name, mac)?;
        Ok(tap)
    }

    /// Switches the carrier (`TUNSETCARRIER`).
    pub fn set_carrier(&self, up: bool) -> io::Result<()> {
        sys::tun_set_carrier(self.file.as_fd(), up)
    }

    /// The netdev's name.
    pub fn name(&self) -> &IfName {
        &self.name
    }

    /// The file: reading gives one frame the netdev sent, writing one hands a frame to it as
    /// received.
    pub fn file(&self) -> &File {
        &self.file
    }
}

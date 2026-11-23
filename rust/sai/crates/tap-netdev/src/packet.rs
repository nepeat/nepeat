//! An `AF_PACKET` socket on an existing interface.
use crate::name::IfName;
use crate::sys;
use core::fmt;
use std::fs::File;
use std::io;
use std::os::fd::AsFd;

/// A raw packet socket bound to one interface: every protocol, the interface promiscuous while
/// it is open, and frames this host sends on it not read back. Reading gives one frame the
/// interface received; writing one sends it out of the interface.
pub struct PacketSocket {
    file: File,
    interface: IfName,
}

impl fmt::Debug for PacketSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PacketSocket")
            .field("interface", &self.interface)
            .finish_non_exhaustive()
    }
}

impl PacketSocket {
    /// Opens the socket on `interface`, which must exist.
    pub fn bind(interface: IfName) -> io::Result<Self> {
        let index = sys::if_nametoindex(&interface)?;
        let fd = sys::packet_socket()?;
        sys::bind_packet(fd.as_fd(), index)?;
        sys::packet_promisc(fd.as_fd(), index)?;
        sys::packet_ignore_outgoing(fd.as_fd())?;
        Ok(Self {
            file: File::from(fd),
            interface,
        })
    }

    /// The interface.
    pub fn interface(&self) -> &IfName {
        &self.interface
    }

    /// The socket as a file (read: a frame in; write: a frame out).
    pub fn file(&self) -> &File {
        &self.file
    }
}

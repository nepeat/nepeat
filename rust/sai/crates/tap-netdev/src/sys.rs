//! The system calls, and the only unsafe code of the crate. Each function makes one call with
//! arguments it builds itself; what the kernel reads or writes through a pointer is a value
//! of this module's own type, with the size and layout of the kernel's (checked at compile
//! time below), fully initialised (no padding bytes left uninitialised).
#![allow(unsafe_code)]

use crate::name::{IFNAMSIZ, IfName};
use core::mem::{offset_of, size_of};
use libc::{c_char, c_int, c_short, c_uint};
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// `sizeof(struct ifreq)`: 40 on 64-bit targets.
const IFREQ_SIZE: usize = size_of::<libc::ifreq>();
/// Where `ifr_ifru` (the union) starts.
const IFRU_OFFSET: usize = offset_of!(libc::ifreq, ifr_ifru);

/// `struct ifreq` as `TUNSETIFF` reads it: name and `ifr_flags`.
#[repr(C)]
struct IfReqFlags {
    name: [c_char; IFNAMSIZ],
    flags: c_short,
    rest: [u8; IFREQ_SIZE - IFNAMSIZ - size_of::<c_short>()],
}

/// `struct ifreq` as `SIOCSIFHWADDR` reads it: name and `ifr_hwaddr` (a `struct sockaddr`).
#[repr(C)]
struct IfReqHwAddr {
    name: [c_char; IFNAMSIZ],
    family: libc::sa_family_t,
    data: [c_char; 14],
    rest: [u8; IFREQ_SIZE - IFNAMSIZ - size_of::<libc::sockaddr>()],
}

/// `struct packet_mreq` (`linux/if_packet.h`).
#[repr(C)]
struct PacketMreq {
    ifindex: c_int,
    kind: u16,
    alen: u16,
    address: [u8; 8],
}

// The layouts the kernel expects, checked against the C library's definitions.
const _: () = {
    assert!(size_of::<IfReqFlags>() == IFREQ_SIZE);
    assert!(offset_of!(IfReqFlags, flags) == IFRU_OFFSET);
    assert!(size_of::<IfReqHwAddr>() == IFREQ_SIZE);
    assert!(offset_of!(IfReqHwAddr, family) == IFRU_OFFSET);
    assert!(size_of::<libc::sockaddr>() == size_of::<libc::sa_family_t>() + 14);
    assert!(size_of::<PacketMreq>() == 16);
    assert!(IFRU_OFFSET == IFNAMSIZ);
};

/// `ETH_P_ALL`: every protocol (host order; the kernel wants it in network order).
const ETH_P_ALL: u16 = 0x0003;
/// `ARPHRD_ETHER`.
const ARPHRD_ETHER: u16 = 1;

fn check(rc: c_int) -> io::Result<()> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn family(af: c_int) -> io::Result<libc::sa_family_t> {
    libc::sa_family_t::try_from(af).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// `TUNSETIFF`: attaches the (new) tun/tap device `name` with `flags` to `fd`.
pub(crate) fn tun_set_iff(fd: BorrowedFd<'_>, name: &IfName, flags: c_short) -> io::Result<()> {
    let mut req = IfReqFlags {
        name: name.to_c(),
        flags,
        rest: [0; IFREQ_SIZE - IFNAMSIZ - size_of::<c_short>()],
    };
    // SAFETY: TUNSETIFF reads and writes one `struct ifreq` at the pointer; `req` has that
    // size and layout (checked above), is initialised and lives across the call; `fd` is open
    // for the call (a `BorrowedFd`).
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), libc::TUNSETIFF, &raw mut req) };
    check(rc)
}

/// `TUNSETCARRIER`: the tap's carrier on or off.
pub(crate) fn tun_set_carrier(fd: BorrowedFd<'_>, up: bool) -> io::Result<()> {
    let on = c_int::from(up);
    // SAFETY: TUNSETCARRIER reads one `int` at the pointer (get_user); `on` is one and lives
    // across the call; `fd` is open for the call.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), libc::TUNSETCARRIER, &raw const on) };
    check(rc)
}

/// `SIOCSIFHWADDR` on the tun file descriptor itself (the tun driver handles it for its own
/// device): the netdev's MAC address.
pub(crate) fn tun_set_hwaddr(fd: BorrowedFd<'_>, name: &IfName, mac: [u8; 6]) -> io::Result<()> {
    let mut data = [0; 14];
    for (slot, byte) in data.iter_mut().zip(mac) {
        *slot = c_char::from_ne_bytes([byte]);
    }
    let mut req = IfReqHwAddr {
        name: name.to_c(),
        family: ARPHRD_ETHER,
        data,
        rest: [0; IFREQ_SIZE - IFNAMSIZ - size_of::<libc::sockaddr>()],
    };
    // SAFETY: SIOCSIFHWADDR copies one `struct ifreq` from the pointer (and may write it back);
    // `req` has that size and layout (checked above), is initialised and lives across the
    // call; `fd` is open for the call.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), libc::SIOCSIFHWADDR, &raw mut req) };
    check(rc)
}

/// `if_nametoindex(3)`.
pub(crate) fn if_nametoindex(name: &IfName) -> io::Result<c_uint> {
    let c_name = std::ffi::CString::new(name.as_str()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `c_name` is a NUL-terminated string that lives across the call; the function
    // only reads it.
    let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
    if index == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(index)
    }
}

/// `socket(AF_PACKET, SOCK_RAW | SOCK_CLOEXEC, htons(ETH_P_ALL))`.
pub(crate) fn packet_socket() -> io::Result<OwnedFd> {
    // SAFETY: integer arguments only.
    let fd = unsafe {
        libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            c_int::from(ETH_P_ALL.to_be()),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned by socket(2), is open, and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `bind(2)` of a packet socket to an interface, every protocol.
pub(crate) fn bind_packet(fd: BorrowedFd<'_>, ifindex: c_uint) -> io::Result<()> {
    let addr = libc::sockaddr_ll {
        sll_family: family(libc::AF_PACKET)?,
        sll_protocol: ETH_P_ALL.to_be(),
        sll_ifindex: c_int::try_from(ifindex).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 0,
        sll_addr: [0; 8],
    };
    let len = libc::socklen_t::try_from(size_of::<libc::sockaddr_ll>())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: bind reads `len` bytes at the pointer: `addr` is a `sockaddr_ll` of that size
    // (a `sockaddr` of family AF_PACKET), initialised, alive across the call; `fd` is open.
    let rc = unsafe { libc::bind(fd.as_raw_fd(), (&raw const addr).cast::<libc::sockaddr>(), len) };
    check(rc)
}

/// `setsockopt(2)` with a value of type `T` (a plain C value without padding).
fn set_option<T>(fd: BorrowedFd<'_>, level: c_int, name: c_int, value: &T) -> io::Result<()> {
    let len = libc::socklen_t::try_from(size_of::<T>()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: setsockopt reads `len` = size_of::<T>() bytes at the pointer, which is a live
    // `T` borrowed for the call; `fd` is open for the call. Callers pass the C type the
    // option expects (`int`, `struct packet_mreq`).
    let rc = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            level,
            name,
            (&raw const *value).cast::<libc::c_void>(),
            len,
        )
    };
    check(rc)
}

/// `PACKET_ADD_MEMBERSHIP` `PACKET_MR_PROMISC`: the interface stays promiscuous while the
/// socket is open.
pub(crate) fn packet_promisc(fd: BorrowedFd<'_>, ifindex: c_uint) -> io::Result<()> {
    const PACKET_ADD_MEMBERSHIP: c_int = 1;
    const PACKET_MR_PROMISC: u16 = 1;
    let mreq = PacketMreq {
        ifindex: c_int::try_from(ifindex).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?,
        kind: PACKET_MR_PROMISC,
        alen: 0,
        address: [0; 8],
    };
    set_option(fd, libc::SOL_PACKET, PACKET_ADD_MEMBERSHIP, &mreq)
}

/// `PACKET_IGNORE_OUTGOING`: frames this host sends on the interface are not read back.
pub(crate) fn packet_ignore_outgoing(fd: BorrowedFd<'_>) -> io::Result<()> {
    const PACKET_IGNORE_OUTGOING: c_int = 23;
    set_option(fd, libc::SOL_PACKET, PACKET_IGNORE_OUTGOING, &c_int::from(true))
}

/// `poll(2)`; `Interrupted` on EINTR.
pub(crate) fn poll(fds: &mut [libc::pollfd], timeout_ms: c_int) -> io::Result<usize> {
    let n = libc::nfds_t::try_from(fds.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: poll reads and writes `n` = fds.len() `struct pollfd` at the pointer, which is
    // the exclusively borrowed slice; nothing else accesses it during the call.
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), n, timeout_ms) };
    usize::try_from(rc).map_err(|_| io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The request numbers as the kernel headers of each architecture compute them
    /// (_IOW('T', 202, int), _IOW('T', 226, int)): powerpc encodes direction and size
    /// differently from x86, which is why they come from libc, not from here.
    #[test]
    fn ioctl_numbers_are_the_architectures() {
        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(libc::TUNSETIFF, 0x4004_54ca);
            assert_eq!(libc::TUNSETCARRIER, 0x4004_54e2);
        }
        #[cfg(target_arch = "powerpc64")]
        {
            assert_eq!(libc::TUNSETIFF, 0x8004_54ca);
            assert_eq!(libc::TUNSETCARRIER, 0x8004_54e2);
        }
        assert_eq!(libc::SIOCSIFHWADDR, 0x8924);
    }

    #[test]
    fn network_order_fields_are_big_endian_bytes() {
        assert_eq!(ETH_P_ALL.to_be().to_ne_bytes(), [0x00, 0x03]);
        assert_eq!(u16::to_be(0x88b5).to_ne_bytes(), [0x88, 0xb5]);
    }

    #[test]
    fn the_flags_request_puts_the_flags_where_the_union_starts() {
        let req = IfReqFlags {
            name: IfName::new("t").unwrap().to_c(),
            flags: 0x1002,
            rest: [0; IFREQ_SIZE - IFNAMSIZ - size_of::<c_short>()],
        };
        assert_eq!(req.name[0], c_char::from_ne_bytes(*b"t"));
        assert_eq!(offset_of!(IfReqFlags, flags), 16);
        assert_eq!(req.flags, 0x1002);
    }
}

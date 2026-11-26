//! The operating-system rim: raw sockets (`AF_PACKET`, generic netlink), interface lookup,
//! `poll`. The only unsafe code of the crate; every function makes one system call with
//! arguments it builds itself, and what the kernel reads or writes through a pointer is a value
//! of this module's own type (the size and layout of the kernel's, checked below).
#![allow(unsafe_code)]

use core::mem::size_of;
use libc::{c_char, c_int, c_void};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

/// `ETH_P_ALL` (host order).
pub const ETH_P_ALL: u16 = 0x0003;
/// `sll_pkttype` of a frame this host sent (`PACKET_OUTGOING`).
pub const PACKET_OUTGOING: u8 = 4;
/// `ENOBUFS` (a netlink socket overran).
pub const ENOBUFS: i32 = libc::ENOBUFS;
const IFNAMSIZ: usize = 16;
const IFREQ_SIZE: usize = size_of::<libc::ifreq>();

/// `struct ifreq` as plain bytes: the name, then the union (flags at 0, a `struct sockaddr` at 0).
#[repr(C)]
struct IfReq {
    name: [c_char; IFNAMSIZ],
    union: [u8; IFREQ_SIZE - IFNAMSIZ],
}

/// `struct sockaddr_nl` (libc's has a private padding field).
#[repr(C)]
struct SockaddrNl {
    family: u16,
    pad: u16,
    pid: u32,
    groups: u32,
}

const _: () = assert!(size_of::<SockaddrNl>() == size_of::<libc::sockaddr_nl>());
const _: () = assert!(size_of::<IfReq>() == IFREQ_SIZE);
const _: () = assert!(size_of::<libc::sockaddr_ll>() == 20);

fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidInput)
}

fn check(rc: c_int) -> io::Result<()> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn check_len(rc: isize) -> io::Result<usize> {
    usize::try_from(rc).map_err(|_| io::Error::last_os_error())
}

fn raw_socket(domain: c_int, kind: c_int, protocol: c_int) -> io::Result<OwnedFd> {
    // SAFETY: integer arguments only.
    let fd = unsafe { libc::socket(domain, kind | libc::SOCK_CLOEXEC, protocol) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned by socket(2), is open, and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// A socket (packet or netlink).
#[derive(Debug)]
pub struct Sock {
    fd: OwnedFd,
}

impl Sock {
    /// `socket(AF_PACKET, SOCK_RAW, htons(protocol))` (protocol 0: send only).
    pub fn packet(protocol: u16) -> io::Result<Self> {
        raw_socket(libc::AF_PACKET, libc::SOCK_RAW, c_int::from(protocol.to_be())).map(|fd| Self { fd })
    }

    /// `socket(AF_NETLINK, SOCK_RAW, NETLINK_GENERIC)`.
    pub fn netlink_generic() -> io::Result<Self> {
        raw_socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_GENERIC).map(|fd| Self { fd })
    }

    fn bind_raw<T>(&self, addr: &T) -> io::Result<()> {
        let len = libc::socklen_t::try_from(size_of::<T>()).map_err(|_| invalid())?;
        // SAFETY: bind reads `len` = size_of::<T>() bytes at the pointer: `addr` is a live
        // sockaddr_ll / sockaddr_nl of that size; the descriptor is open for the call.
        let rc = unsafe { libc::bind(self.fd.as_raw_fd(), (&raw const *addr).cast::<libc::sockaddr>(), len) };
        check(rc)
    }

    /// Binds a packet socket to an interface and protocol.
    pub fn bind_packet(&self, ifindex: u32, protocol: u16) -> io::Result<()> {
        self.bind_raw(&libc::sockaddr_ll {
            sll_family: u16::try_from(libc::AF_PACKET).map_err(|_| invalid())?,
            sll_protocol: protocol.to_be(),
            sll_ifindex: c_int::try_from(ifindex).map_err(|_| invalid())?,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        })
    }

    /// Binds a netlink socket (kernel-assigned port id, no groups).
    pub fn bind_netlink(&self) -> io::Result<()> {
        self.bind_raw(&SockaddrNl {
            family: u16::try_from(libc::AF_NETLINK).map_err(|_| invalid())?,
            pad: 0,
            pid: 0,
            groups: 0,
        })
    }

    fn set_option<T>(&self, level: c_int, name: c_int, value: &T) -> io::Result<()> {
        let len = libc::socklen_t::try_from(size_of::<T>()).map_err(|_| invalid())?;
        // SAFETY: setsockopt reads `len` = size_of::<T>() bytes at the pointer, a live `T`
        // borrowed for the call (an `int` or a `timeval`, what each option expects); the
        // descriptor is open for the call.
        let rc = unsafe {
            libc::setsockopt(
                self.fd.as_raw_fd(),
                level,
                name,
                (&raw const *value).cast::<c_void>(),
                len,
            )
        };
        check(rc)
    }

    /// `SO_RCVTIMEO`.
    pub fn set_recv_timeout(&self, d: Duration) -> io::Result<()> {
        let tv = libc::timeval {
            tv_sec: libc::time_t::try_from(d.as_secs()).map_err(|_| invalid())?,
            tv_usec: libc::suseconds_t::from(d.subsec_micros()),
        };
        self.set_option(libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv)
    }

    /// `SO_RCVBUF` (the kernel doubles it, capped by `rmem_max`).
    pub fn set_recv_buffer(&self, bytes: i32) -> io::Result<()> {
        self.set_option(libc::SOL_SOCKET, libc::SO_RCVBUF, &bytes)
    }

    /// `SO_RCVBUFFORCE` (needs `CAP_NET_ADMIN`; ignores `rmem_max`).
    pub fn force_recv_buffer(&self, bytes: i32) -> io::Result<()> {
        self.set_option(libc::SOL_SOCKET, libc::SO_RCVBUFFORCE, &bytes)
    }

    /// `NETLINK_ADD_MEMBERSHIP`.
    pub fn join_netlink_group(&self, group: u32) -> io::Result<()> {
        self.set_option(libc::SOL_NETLINK, libc::NETLINK_ADD_MEMBERSHIP, &group)
    }

    /// `sendto` on a packet socket: `frame` out of the interface, link address `dmac`.
    pub fn send_to_packet(&self, ifindex: u32, dmac: [u8; 6], frame: &[u8]) -> io::Result<usize> {
        let mut addr = [0u8; 8];
        for (slot, b) in addr.iter_mut().zip(dmac) {
            *slot = b;
        }
        let to = libc::sockaddr_ll {
            sll_family: u16::try_from(libc::AF_PACKET).map_err(|_| invalid())?,
            sll_protocol: 0,
            sll_ifindex: c_int::try_from(ifindex).map_err(|_| invalid())?,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 6,
            sll_addr: addr,
        };
        let len = libc::socklen_t::try_from(size_of::<libc::sockaddr_ll>()).map_err(|_| invalid())?;
        // SAFETY: sendto reads `frame.len()` bytes at the slice and `len` bytes at the address,
        // a live sockaddr_ll of that size; the descriptor is open for the call.
        let rc = unsafe {
            libc::sendto(
                self.fd.as_raw_fd(),
                frame.as_ptr().cast::<c_void>(),
                frame.len(),
                0,
                (&raw const to).cast::<libc::sockaddr>(),
                len,
            )
        };
        check_len(rc)
    }

    /// `recvfrom` on a packet socket: the length and `sll_pkttype` of one frame (blocks up to the
    /// receive timeout; `dontwait`: `MSG_DONTWAIT`).
    pub fn recv_from_packet(&self, buf: &mut [u8], dontwait: bool) -> io::Result<(usize, u8)> {
        let mut from = libc::sockaddr_ll {
            sll_family: 0,
            sll_protocol: 0,
            sll_ifindex: 0,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        };
        let mut len = libc::socklen_t::try_from(size_of::<libc::sockaddr_ll>()).map_err(|_| invalid())?;
        let flags = if dontwait { libc::MSG_DONTWAIT } else { 0 };
        // SAFETY: recvfrom writes at most `buf.len()` bytes at the exclusively borrowed slice, and
        // at most `len` bytes at `from` (a sockaddr_ll of that size), updating `len`; the
        // descriptor is open for the call.
        let rc = unsafe {
            libc::recvfrom(
                self.fd.as_raw_fd(),
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len(),
                flags,
                (&raw mut from).cast::<libc::sockaddr>(),
                &raw mut len,
            )
        };
        check_len(rc).map(|n| (n, from.sll_pkttype))
    }

    /// `send(fd, buf, len, 0)`.
    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        // SAFETY: send reads `buf.len()` bytes at the slice; the descriptor is open for the call.
        let rc = unsafe { libc::send(self.fd.as_raw_fd(), buf.as_ptr().cast::<c_void>(), buf.len(), 0) };
        check_len(rc)
    }

    /// `recv(fd, buf, len, 0)`.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: recv writes at most `buf.len()` bytes at the exclusively borrowed slice; the
        // descriptor is open for the call.
        let rc = unsafe { libc::recv(self.fd.as_raw_fd(), buf.as_mut_ptr().cast::<c_void>(), buf.len(), 0) };
        check_len(rc)
    }
}

/// `poll(2)` for input on several sockets: which are readable (`POLLIN`), after at most
/// `timeout_ms`; `Ok(None)` when none is ready (timeout or a signal).
pub fn poll_readable(socks: &[&Sock], timeout_ms: i32) -> io::Result<Option<Vec<bool>>> {
    let mut fds: Vec<libc::pollfd> = socks
        .iter()
        .map(|s| libc::pollfd {
            fd: s.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let n = libc::nfds_t::try_from(fds.len()).map_err(|_| invalid())?;
    // SAFETY: poll reads and writes `n` = fds.len() `struct pollfd` at the pointer, the
    // exclusively owned vector; the descriptors stay open (the sockets are borrowed).
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), n, timeout_ms) };
    match usize::try_from(rc) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(fds.iter().map(|f| f.revents & libc::POLLIN != 0).collect())),
        Err(_) => {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// `if_nametoindex(3)`: 0 when there is no such interface.
pub fn if_nametoindex(name: &str) -> u32 {
    let Ok(c) = std::ffi::CString::new(name) else { return 0 };
    // SAFETY: `c` is a NUL-terminated string that lives across the call; it is only read.
    unsafe { libc::if_nametoindex(c.as_ptr()) }
}

/// `if_indextoname(3)`.
pub fn if_indextoname(index: u32) -> Option<String> {
    let mut buf = [0 as c_char; IFNAMSIZ];
    // SAFETY: if_indextoname writes at most IF_NAMESIZE (= 16) bytes at the pointer, which is
    // `buf`, exclusively borrowed for the call.
    let p = unsafe { libc::if_indextoname(index, buf.as_mut_ptr()) };
    if p.is_null() {
        return None;
    }
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A failed step of [`up_and_index`]: what `perror` was given, and why.
#[derive(Debug)]
pub struct IfError {
    /// The `perror` prefix (the interface name, `up`, `hwaddr`).
    pub what: String,
    /// The operating-system error.
    pub source: io::Error,
}

impl core::fmt::Display for IfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.what, crate::cstd::strerror(&self.source))
    }
}

fn ifreq_ioctl(fd: &OwnedFd, request: libc::Ioctl, req: &mut IfReq) -> io::Result<()> {
    // SAFETY: the SIOC[GS]IF{FLAGS,HWADDR} requests read and write one `struct ifreq` at the
    // pointer; `req` has that size and layout (checked above), is initialised and exclusively
    // borrowed for the call; the descriptor is open.
    let rc = unsafe { libc::ioctl(fd.as_raw_fd(), request, &raw mut *req) };
    check(rc)
}

fn named_req(name: &str) -> IfReq {
    let mut req = IfReq {
        name: [0; IFNAMSIZ],
        union: [0; IFREQ_SIZE - IFNAMSIZ],
    };
    // `snprintf(r.ifr_name, IFNAMSIZ, "%s", ifn)`: at most 15 bytes and a NUL
    for (slot, b) in req.name.iter_mut().zip(name.bytes().take(IFNAMSIZ - 1)) {
        *slot = c_char::from_ne_bytes([b]);
    }
    req
}

/// `up_and_index` of the C tools: brings the interface up when it is down and returns its index
/// (0 when the lookup fails, as `if_nametoindex` does) and MAC address.
pub fn up_and_index(name: &str) -> Result<(u32, [u8; 6]), IfError> {
    let err = |what: &str, source| IfError {
        what: what.to_owned(),
        source,
    };
    let fd = raw_socket(libc::AF_INET, libc::SOCK_DGRAM, 0).map_err(|e| err(name, e))?;
    let mut req = named_req(name);
    ifreq_ioctl(&fd, libc::SIOCGIFFLAGS, &mut req).map_err(|e| err(name, e))?;
    let flags = i16::from_ne_bytes([req.union[0], req.union[1]]);
    if flags & i16::try_from(libc::IFF_UP).unwrap_or(1) == 0 {
        let up = (flags | i16::try_from(libc::IFF_UP).unwrap_or(1)).to_ne_bytes();
        req.union[..2].copy_from_slice(&up);
        ifreq_ioctl(&fd, libc::SIOCSIFFLAGS, &mut req).map_err(|e| err("up", e))?;
    }
    let mut req = named_req(name);
    ifreq_ioctl(&fd, libc::SIOCGIFHWADDR, &mut req).map_err(|e| err("hwaddr", e))?;
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&req.union[2..8]);
    Ok((if_nametoindex(name), mac))
}

//! The SDK's own netdevs (`inno_0_<sysport>`, made by libifcs' ipd driver with the init yaml's
//! netdev `auto_create`): renamed and given a MAC when a host interface adopts one, given back at
//! teardown (`tl_hostif.cpp` `rename_netdev`, as the vendor's `knet.py` does). The interface
//! ioctls on an `AF_INET` datagram socket; the sequence (and what is logged) is the backend's.
//!
//! Each call passes one `struct ifreq` of this module's own type (the C library's size and
//! layout, checked at compile time; fully initialised) that lives across the call.
#![allow(unsafe_code)]

use core::ffi::{c_char, c_int};
use core::mem::{offset_of, size_of};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// `IFNAMSIZ`.
const IFNAMSIZ: usize = 16;
/// `sizeof(struct ifreq)`.
const IFREQ_SIZE: usize = size_of::<libc::ifreq>();
/// `ARPHRD_ETHER`.
const ARPHRD_ETHER: u16 = 1;

/// `struct ifreq`: the name, then the union's bytes (`ifr_flags`, `ifr_newname`, `ifr_hwaddr`
/// all start there).
#[repr(C, align(8))]
struct IfReq {
    name: [c_char; IFNAMSIZ],
    ifru: [u8; IFREQ_SIZE - IFNAMSIZ],
}

const _: () = {
    assert!(size_of::<IfReq>() == IFREQ_SIZE);
    assert!(offset_of!(libc::ifreq, ifr_ifru) == IFNAMSIZ);
    assert!(offset_of!(IfReq, ifru) == IFNAMSIZ);
    assert!(size_of::<libc::sockaddr>() == 16);
};

/// A name as `snprintf(buf, IFNAMSIZ, "%s", name)` writes it: at most 15 bytes (up to a NUL), then
/// NUL.
fn c_name(name: &str) -> [c_char; IFNAMSIZ] {
    let mut out = [0 as c_char; IFNAMSIZ];
    for (slot, b) in out
        .iter_mut()
        .zip(name.bytes().take_while(|b| *b != 0).take(IFNAMSIZ - 1))
    {
        *slot = c_char::from_ne_bytes([b]);
    }
    out
}

impl IfReq {
    /// A zeroed request for `name` (`memset` + `snprintf` of the C++).
    fn new(name: &str) -> Self {
        Self {
            name: c_name(name),
            ifru: [0; IFREQ_SIZE - IFNAMSIZ],
        }
    }

    fn put(&mut self, off: usize, bytes: &[u8]) {
        if let Some(dst) = self.ifru.get_mut(off..off + bytes.len()) {
            dst.copy_from_slice(bytes);
        }
    }

    fn flags(&self) -> i16 {
        let mut b = [0u8; 2];
        if let Some(src) = self.ifru.get(..2) {
            b.copy_from_slice(src);
        }
        i16::from_ne_bytes(b)
    }

    fn set_flags(&mut self, flags: i16) {
        self.put(0, &flags.to_ne_bytes());
    }

    /// `ifr_newname` (the union's first 16 bytes).
    fn set_newname(&mut self, name: &str) {
        for (slot, c) in self.ifru.iter_mut().zip(c_name(name)) {
            *slot = c.to_ne_bytes()[0];
        }
    }

    /// `ifr_hwaddr`: `sa_family` = `ARPHRD_ETHER`, `sa_data` = the MAC.
    fn set_hwaddr(&mut self, mac: [u8; 6]) {
        self.put(0, &ARPHRD_ETHER.to_ne_bytes());
        self.put(2, &mac);
    }
}

/// `IFF_UP`.
pub const IFF_UP: i16 = 1;

/// The control socket of the interface ioctls (`socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0)`),
/// closed on drop.
#[derive(Debug)]
pub struct NetdevSocket(OwnedFd);

fn check(rc: c_int) -> io::Result<()> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl NetdevSocket {
    /// Opens it.
    pub fn open() -> io::Result<Self> {
        // SAFETY: socket(2) with constant arguments; no memory is passed.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        check(fd)?;
        // SAFETY: `fd` is the descriptor socket(2) just returned, open and owned by nothing else.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    fn ioctl(&self, request: libc::Ioctl, req: &mut IfReq) -> io::Result<()> {
        // SAFETY: the interface ioctls used here (SIOCGIFFLAGS, SIOCSIFFLAGS, SIOCSIFNAME,
        // SIOCSIFHWADDR) read and may write one `struct ifreq` at the pointer; `req` has its size
        // and layout (checked above), is initialised and lives across the call; the descriptor is
        // open (owned by `self`).
        let rc = unsafe { libc::ioctl(self.0.as_raw_fd(), request, core::ptr::from_mut(req)) };
        check(rc)
    }

    /// `SIOCGIFFLAGS`: the interface's flags (`ENODEV`: no such netdev).
    pub fn flags(&self, name: &str) -> io::Result<i16> {
        let mut r = IfReq::new(name);
        self.ioctl(libc::SIOCGIFFLAGS, &mut r)?;
        Ok(r.flags())
    }

    /// `SIOCSIFFLAGS`.
    pub fn set_flags(&self, name: &str, flags: i16) -> io::Result<()> {
        let mut r = IfReq::new(name);
        r.set_flags(flags);
        self.ioctl(libc::SIOCSIFFLAGS, &mut r)
    }

    /// `SIOCSIFNAME`: `from` becomes `to` (the netdev must be down).
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let mut r = IfReq::new(from);
        r.set_newname(to);
        self.ioctl(libc::SIOCSIFNAME, &mut r)
    }

    /// `SIOCSIFHWADDR` (`ARPHRD_ETHER`).
    pub fn set_hwaddr(&self, name: &str, mac: [u8; 6]) -> io::Result<()> {
        let mut r = IfReq::new(name);
        r.set_hwaddr(mac);
        self.ioctl(libc::SIOCSIFHWADDR, &mut r)
    }
}

/// `strerror(errno)` of an OS error (without std's ` (os error N)`).
pub fn strerror(e: &io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ").and_then(|i| s.get(..i)) {
        Some(t) => t.to_owned(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_laid_out_as_struct_ifreq() {
        let mut r = IfReq::new("inno_0_0001");
        assert_eq!(
            r.name[..12].iter().map(|c| c.to_ne_bytes()[0]).collect::<Vec<_>>(),
            b"inno_0_0001\0"
        );
        r.set_flags(0x1043);
        assert_eq!(r.flags(), 0x1043);
        r.set_newname("Ethernet0");
        assert_eq!(&r.ifru[..10], b"Ethernet0\0");
        r.set_hwaddr([2, 4, 6, 8, 10, 12]);
        let f = ARPHRD_ETHER.to_ne_bytes();
        assert_eq!(r.ifru[..8], [f[0], f[1], 2, 4, 6, 8, 10, 12]);
        // snprintf's cut: 15 bytes and the NUL
        let long = IfReq::new("abcdefghijklmnopqrst");
        assert_eq!(long.name[14].to_ne_bytes()[0], b'o');
        assert_eq!(long.name[15], 0);
    }

    #[test]
    fn a_missing_netdev_is_enodev() {
        let Ok(s) = NetdevSocket::open() else { return }; // no AF_INET in a sandbox: nothing to check
        let e = s.flags("tl-no-such-0").unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ENODEV));
        assert_eq!(strerror(&e), "No such device");
        assert_eq!(strerror(&io::Error::other("x")), "x");
    }
}

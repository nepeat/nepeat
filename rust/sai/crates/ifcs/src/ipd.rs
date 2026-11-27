//! The ipd driver's sFlow netlink ioctls (`tl_sflow.cpp`): `IPD_SET_PARAMS` (the samples' GRE
//! protocol), `IPD_CREATE_NETLINK` / `IPD_DELETE_NETLINK` (`nl_app_type` SFLOW), `IPD_SET_NETDEV` (a
//! sysport's sample rate), on the `/dev/ipd` descriptor libifcs opened in this process (the driver
//! refuses a second open), found through `/proc/self/fd`. A libifcs exporting `tl_ipd_ioctl` (the
//! test mock) gets them instead, as in the C++.
//!
//! The argument layouts are the driver's (`inno_ioctl.h` of ipd 0.16.2, GPL; mrvl-teralynx), restated
//! for interop only, as `tl_sflow.cpp` does; sizes checked at compile time. The driver dispatches on
//! the command's number only and copies its own struct's size from the pointer: each argument here is
//! exactly that size (`IPD_SET_NETDEV`'s 140-byte buffer holds both the 0.12.x (76 B) and the 0.16
//! (140 B) `inno_ioctl_netdev_info_t`, whose rate is the union's first `u32` in both).
//!
//! Unsafe: the descriptor's `ioctl` call, the hook's call, the hook's address as its function type:
//! one operation each.
#![allow(unsafe_code)]

use core::ffi::{c_int, c_ulong, c_void};
use core::mem::{offset_of, size_of};
use core::ptr::NonNull;

use crate::Ifcs;

/// `IPD_SET_NETDEV`'s number.
pub const NR_SET_NETDEV: u32 = 26;
/// `IPD_CREATE_NETLINK`'s number.
pub const NR_CREATE_NETLINK: u32 = 27;
/// `IPD_DELETE_NETLINK`'s number.
pub const NR_DELETE_NETLINK: u32 = 28;
/// `IPD_SET_PARAMS`'s number.
pub const NR_SET_PARAMS: u32 = 31;
/// `inno_ioctl_hdr_t.version`.
pub const IOCTL_VERSION: u16 = 1;
/// `ENODEV` (no ipd device).
pub const ENODEV: c_int = libc::ENODEV;
/// `EBADF` (the descriptor went away: look again).
pub const EBADF: c_int = libc::EBADF;
/// `EINVAL` (a devport without a sysport).
pub const EINVAL: c_int = libc::EINVAL;
/// `NETLINK_GENERIC`.
pub const NETLINK_GENERIC_TYPE: u32 = 16;
/// `INNO_NL_SFLOW`.
pub const APP_SFLOW: u32 = 1;
/// `INNO_ND_INGRESS_SFLOW_SAMPLE_RATE`.
pub const ND_INGRESS_RATE: u16 = 8;
/// `INNO_ND_EGRESS_SFLOW_SAMPLE_RATE`.
pub const ND_EGRESS_RATE: u16 = 9;
/// `IFNAMSIZ + 1`: `inno_ioctl_netlink_t.name`.
pub const NAME_SIZE: usize = 17;
/// `GENL_NAMSIZ`: `inno_ioctl_netlink_t.mcgrp`.
pub const MCGRP_SIZE: usize = 16;
/// `sizeof(inno_ioctl_netdev_info_t)` of ipd 0.16 (the larger layout).
pub const NETDEV_INFO_SIZE: usize = 140;

/// `_IOWR('I', nr, void*)` (asm-generic encoding: `x86_64` and `aarch64`): read-write, the size of a
/// pointer, type `'I'`.
pub const fn cmd(nr: u32) -> c_ulong {
    const WRITE: c_ulong = 1;
    const READ: c_ulong = 2;
    ((READ | WRITE) << 30) | ((size_of::<*mut c_void>() as c_ulong) << 16) | ((b'I' as c_ulong) << 8) | nr as c_ulong
}

/// `inno_ioctl_hdr_t` (8 B).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hdr {
    /// `version` ([`IOCTL_VERSION`]).
    pub version: u16,
    /// `instance`.
    pub instance: u8,
    unused1: u8,
    unused2: u32,
}

impl Hdr {
    /// `hdr()` of `tl_sflow.cpp`: version 1, the rest 0.
    pub const fn new() -> Self {
        Self {
            version: IOCTL_VERSION,
            instance: 0,
            unused1: 0,
            unused2: 0,
        }
    }
}

/// `inno_ioctl_netlink_t` (52 B; the tail padding made explicit, zero).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Netlink {
    /// The header.
    pub hdr: Hdr,
    /// `type` ([`NETLINK_GENERIC_TYPE`]).
    pub kind: u32,
    /// `app` ([`APP_SFLOW`]).
    pub app: u32,
    /// `name`, NUL-terminated.
    pub name: [u8; NAME_SIZE],
    /// `mcgrp`, NUL-terminated.
    pub mcgrp: [u8; MCGRP_SIZE],
    pad: [u8; 3],
}

/// `copy_name` of `tl_sflow.cpp`: zeroed, at most `N - 1` bytes of `s`.
fn c_name<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    for (d, b) in out.iter_mut().zip(s.bytes().take(N.saturating_sub(1))) {
        *d = b;
    }
    out
}

impl Netlink {
    /// The sFlow netlink of a generic-netlink family `name`, multicast group `mcgrp`.
    pub fn sflow(name: &str, mcgrp: &str) -> Self {
        Self {
            hdr: Hdr::new(),
            kind: NETLINK_GENERIC_TYPE,
            app: APP_SFLOW,
            name: c_name(name),
            mcgrp: c_name(mcgrp),
            pad: [0; 3],
        }
    }
}

/// `inno_ioctl_params_t` (12 B).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// The header.
    pub hdr: Hdr,
    /// `shim_gre_proto`.
    pub shim_gre_proto: u16,
    /// `rsvd`.
    pub rsvd: u16,
}

impl Params {
    /// The samples' GRE protocol.
    pub const fn gre(proto: u16) -> Self {
        Self {
            hdr: Hdr::new(),
            shim_gre_proto: proto,
            rsvd: 0,
        }
    }
}

/// `inno_ioctl_netdev_info_t` as a zeroed 140-byte buffer: packed `{hdr, u16 id, u16 sysport, union}`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetdevInfo([u8; NETDEV_INFO_SIZE]);

/// Where the union starts (after the packed header, id and sysport).
const NETDEV_INFO_U: usize = 12;

impl NetdevInfo {
    /// `push_rate`'s buffer: the header, `id` (`ND_INGRESS_RATE` / `ND_EGRESS_RATE`), the sysport's
    /// number, the rate as the union's first `u32`.
    pub fn rate(id: u16, sysport: u16, rate: u32) -> Self {
        let mut b = [0u8; NETDEV_INFO_SIZE];
        let mut put = |off: usize, v: &[u8]| {
            if let Some(d) = b.get_mut(off..off + v.len()) {
                d.copy_from_slice(v);
            }
        };
        put(offset_of!(Hdr, version), &IOCTL_VERSION.to_ne_bytes());
        put(size_of::<Hdr>(), &id.to_ne_bytes());
        put(size_of::<Hdr>() + 2, &sysport.to_ne_bytes());
        put(NETDEV_INFO_U, &rate.to_ne_bytes());
        Self(b)
    }

    /// The bytes.
    pub const fn bytes(&self) -> &[u8; NETDEV_INFO_SIZE] {
        &self.0
    }
}

const _: () = {
    assert!(size_of::<Hdr>() == 8);
    assert!(size_of::<Netlink>() == 52);
    assert!(offset_of!(Netlink, kind) == 8 && offset_of!(Netlink, app) == 12);
    assert!(offset_of!(Netlink, name) == 16 && offset_of!(Netlink, mcgrp) == 33);
    assert!(size_of::<Params>() == 12 && offset_of!(Params, shim_gre_proto) == 8);
    assert!(size_of::<NetdevInfo>() == NETDEV_INFO_SIZE);
};

mod sealed {
    pub trait Sealed {}
}

/// An ipd ioctl argument: one of this module's layouts (nothing else can be passed).
pub trait Arg: sealed::Sealed {}
impl sealed::Sealed for Netlink {}
impl Arg for Netlink {}
impl sealed::Sealed for Params {}
impl Arg for Params {}
impl sealed::Sealed for NetdevInfo {}
impl Arg for NetdevInfo {}

/// `int tl_ipd_ioctl(int fd, unsigned long req, void* arg)` (`rust/sai/tests/teralynx-mock/mock_sflow.c`).
pub(crate) type Hook = unsafe extern "C" fn(c_int, c_ulong, *mut c_void) -> c_int;

/// The hook at `p` (dlsym's answer for `tl_ipd_ioctl` in libifcs).
pub(crate) fn hook_fn(p: NonNull<c_void>) -> Hook {
    // SAFETY: `p` is dlsym's address of tl_ipd_ioctl in libifcs; the only libifcs defining it (the
    // test mock, mock_sflow.c) defines it with this signature, the one tl_sflow.cpp calls it by.
    unsafe { core::mem::transmute::<*mut c_void, Hook>(p.as_ptr()) }
}

/// The descriptor of `/proc/self/fd` whose link names `want`, or `/dev/ipd` / `/dev/ipd0` when
/// `want` is empty (`ipd_ready`: the first one in directory order).
pub fn find_fd(want: &str) -> Option<c_int> {
    let dir = std::fs::read_dir("/proc/self/fd").ok()?;
    for e in dir.flatten() {
        let Ok(target) = std::fs::read_link(e.path()) else {
            continue;
        };
        let t = target.to_string_lossy();
        let hit = if want.is_empty() {
            t == "/dev/ipd" || t == "/dev/ipd0"
        } else {
            t == want
        };
        if hit {
            // atoi of the entry's name
            return e.file_name().to_str().and_then(|n| n.parse().ok());
        }
    }
    None
}

/// `errno` after a failed call (`errno ? errno : EIO`).
fn errno() -> c_int {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(e) if e != 0 => e,
        _ => libc::EIO,
    }
}

impl Ifcs {
    /// `ioctl_(fd, _IOWR('I', nr, void*), arg)`: through the hook when libifcs exports one (the
    /// descriptor is then not looked at), else `ioctl(2)` on `fd`. `Err(errno)` on failure.
    pub fn ipd_ioctl<T: Arg>(&self, fd: c_int, nr: u32, arg: &mut T) -> Result<(), c_int> {
        let p = core::ptr::from_mut(arg).cast::<c_void>();
        let rc = match self.0.ipd_hook {
            // SAFETY: the mock's function (hook_fn); it reads the driver's layout from `p`, which points
            // at one of this module's layouts of that size, borrowed for the call.
            Some(h) => unsafe { h(fd, cmd(nr), p) },
            // SAFETY: ioctl(2) on a descriptor number (a closed one answers EBADF); the ipd driver
            // reads and writes at most its own struct's size at `p`, which is exactly the size of the
            // argument borrowed for the call (module text). As in the C++, the number is the one
            // /proc/self/fd showed as /dev/ipd when it was looked up.
            None => unsafe { libc::ioctl(fd, cmd(nr), p) },
        };
        if rc == 0 { Ok(()) } else { Err(errno()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_layouts_are_the_drivers() {
        assert_eq!(cmd(NR_SET_NETDEV), 0xc008_491a);
        assert_eq!(cmd(NR_SET_PARAMS), 0xc008_491f);
        let n = Netlink::sflow("psample", "packets");
        assert_eq!(&n.name[..8], b"psample\0");
        assert_eq!(&n.mcgrp[..8], b"packets\0");
        assert_eq!((n.hdr.version, n.kind, n.app), (1, 16, 1));
        let long = Netlink::sflow("abcdefghijklmnopqrstuvwxyz", "0123456789abcdefghij");
        assert_eq!(long.name[15], b'p');
        assert_eq!(long.name[16], 0);
        assert_eq!(long.mcgrp[14], b'e');
        assert_eq!(long.mcgrp[15], 0);
        let r = NetdevInfo::rate(ND_INGRESS_RATE, 3, 4096);
        let b = r.bytes();
        assert_eq!(b[0..2], 1u16.to_ne_bytes());
        assert_eq!(b[8..10], 8u16.to_ne_bytes());
        assert_eq!(b[10..12], 3u16.to_ne_bytes());
        assert_eq!(b[12..16], 4096u32.to_ne_bytes());
        assert!(b[16..].iter().all(|x| *x == 0));
        assert_eq!(Params::gre(0x8818).shim_gre_proto, 0x8818);
    }

    #[test]
    fn no_ipd_descriptor_in_a_test_process() {
        assert_eq!(find_fd("/dev/tl-no-such-ipd"), None);
        assert!(find_fd("").is_none());
    }
}

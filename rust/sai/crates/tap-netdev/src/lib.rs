//! Linux TAP netdevs for the Rust SAI's virtual backend, as the C++ sai-openbcm fake backend
//! makes them (`fake_backend.cpp`, `SAI_OPENBCM_FAKE_NETDEV=tap` / `SAI_OPENBCM_FAKE_WIRE`):
//!
//! - [`Tap`]: one TAP netdev (`IFF_TAP | IFF_NO_PI`, not persistent). Created with its carrier
//!   off and the given MAC address; the carrier is switched with [`Tap::set_carrier`]. Dropping
//!   the handle closes the file descriptor, and the kernel removes the netdev with it.
//! - [`PacketSocket`]: an `AF_PACKET` raw socket bound to an existing interface, promiscuous,
//!   not seeing its own transmissions.
//! - [`Wire`]: a thread that carries frames between a TAP and a packet socket in both
//!   directions while a gate is open (the port is up). Dropping it stops and joins the thread.
//!
//! Everything here needs `CAP_NET_ADMIN` (in the network namespace's user namespace) and
//! `/dev/net/tun`; nothing is created unless a caller asks for it.
//!
//! Unsafe code: only in the private module `sys` (the ioctls and socket calls, one per block,
//! each with the reasoning clippy asks for); the crate root denies `unsafe_code` everywhere
//! else. Byte order: `struct ifreq` and friends are built field by field with the C library's
//! types (host order where the kernel ABI is host order); the two network-order fields
//! (`sll_protocol`, the protocol argument of `socket(2)`) are converted with
//! [`u16::to_be`].
#![deny(unsafe_code)]

mod name;
mod packet;
mod sys;
mod tap;
mod wire;

pub use name::{IFNAMSIZ, IfName, NameError};
pub use packet::PacketSocket;
pub use tap::Tap;
pub use wire::{Forwarded, Wire, forward};

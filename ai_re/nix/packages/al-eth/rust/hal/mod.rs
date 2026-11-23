// SPDX-License-Identifier: GPL-2.0

//! al_eth hardware abstraction in safe Rust: the parts of the Annapurna Labs
//! HAL (../../src/al_hal_*.c) and of the Linux glue (../../src/al_eth_main.c)
//! that program the hardware on our boards (DMC-12: RGMII 1G fixed link;
//! CS8210: SGMII and 10G serial). Only `core` is used and there is no unsafe
//! code; the hardware is reached through the traits of [`io`]. Every register
//! access sequence is checked against the vendor C on the build host
//! (../../difftest, README.md in this directory).

#![forbid(unsafe_code)]
// A crate-internal layer: its items are `pub` for the driver modules and the
// host tests of the same crate (so the kernel's unreachable_pub lint does not
// fit), and parts only the CS8210 modes or the tests use are unused on the
// DMC-12 path.
#![allow(unreachable_pub, dead_code)]

#[path = "../board_params.rs"]
pub mod board_params;
pub mod io;
pub mod regs;
pub mod iofic;
pub mod udma;
pub mod ec;
pub mod mac;
pub mod desc;
pub mod ring;
pub mod port;

/// What can go wrong. The C HAL returns -EINVAL/-EPERM/-ETIMEDOUT/-ENOSPC and
/// BUG()s on broken invariants (al_assert); the port returns the matching
/// variant instead and never panics on a value read from the hardware.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// An argument the C HAL rejects or asserts on.
    Invalid,
    /// A mode, revision or feature this port does not implement (on purpose:
    /// none of our boards uses it).
    Unsupported,
    /// A hardware poll did not finish (-ETIMEDOUT / -ETIME).
    Timeout,
    /// No free descriptor (-ENOSPC).
    NoSpace,
    /// The hardware reported something impossible (a reserved UDMA state,
    /// a packet with more descriptors than a packet can have).
    Hardware,
    /// The boot loader left the board parameter registers empty.
    NoBoardParams,
}

pub type Result<T = ()> = core::result::Result<T, Error>;

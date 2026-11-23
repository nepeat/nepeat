//! Safe access to the Broadcom SDK through sai-openbcm's C glue (`sdk_glue.h`, declared by
//! `sdk-glue-sys`), and a pure-Rust fake of that glue for tests. This crate is the only caller of
//! the glue's functions and holds all the `unsafe` of that side: in [`real`]-module calls (one block
//! per C call, each with its `SAFETY` comment), the two callback trampolines' pointer reads and
//! the C fake's test hooks.
//!
//! # Shape
//!
//! * Owners, one per SDK at a time (the process's real glue; each Rust fake chip): [`Attached`]
//!   ([`attach`]: `ob_sdk_attach`, temperatures and the over-temperature power-down only),
//!   [`Sdk`] ([`unit`]: `ob_sdk_init_mdio`, MDIO and temperatures), [`Switch`] ([`switch`]: `ob_sdk_init`,
//!   "init all", everything; derefs to its [`Sdk`]). Each exists only after its bring-up returned
//!   success, so what needs that bring-up is a method on it and cannot be called early. `Send`,
//!   not `Sync`.
//! * Handles for other threads, `Clone + Send + Sync`, dead once their owner is dropped:
//!   [`MdioBus`] ([`mdio`]; what `sai-core`'s `MdioAccess` wraps) and [`TempReader`] ([`temp`]).
//! * One module per area adding methods to [`Switch`]: [`port`], [`vlan`], [`l2`], [`lag`] (trunks),
//!   [`l3`], [`field`], [`policer`], [`netif`] (KNET), [`trap`], [`shell`]; callbacks in [`link`]
//!   (link, L2 events, per-port link gates).
//! * [`Error`] / [`Code`] ([`error`]): the glue's `OB_E_*` codes.
//! * One internal interface, two implementations ([`glue`]): the real glue ([`real`], FFI) and
//!   [`rust_fake`] (feature `rust-fake`: ports, VLANs, L2 table, trunks, L3 tables, field groups /
//!   entries / counters, policers, netifs, MDIO registers, temperatures, shell log, scripted faults
//!   per function, callback injection). `fake` (only when the C fake is linked): its test hooks.
//!
//! # What the glue requires, and who guarantees it
//!
//! | requirement (`sdk_glue.h` / `sdk_glue.c`) | guaranteed by |
//! |---|---|
//! | no `ob_sdk_mdio_*` before `ob_sdk_init_mdio` or `ob_sdk_init` succeeded: after a bare "init soc" the MDIO clock is at its reset value, which hung PHYs on the AS7772 | the MDIO methods are on [`Sdk`] (and [`Switch`]), which only those bring-ups return; [`Attached`] has none; an [`MdioBus`] of a dropped owner never calls (the glue checks again: `OB_E_INIT`) |
//! | clause 45 only; address word at most 0x7ff | [`mdio::Mmd`] holds 1..=31, [`mdio::PHY_MAX`] (the glue checks again) |
//! | buffers of the size passed | slices: the length passed is the slice's ([`real`]) |
//! | strings NUL-terminated and not longer than the glue's buffers | `CString`; a NUL byte or a too long command / netif name is refused before the call |
//! | one bring-up per process at a time; the glue boots its SDK once per process and keeps that config | a process-wide flag: a second owner is refused while one is alive; a later bring-up of the real glue whose config the glue would ignore is refused ([`unit`]) |
//! | port, VLAN, L2, L3, FP, KNET and callback calls need "init all" | they are methods of [`Switch`], which only `ob_sdk_init` returns |
//! | the callback context stays valid while the SDK may call back; no unwinding into C | no memory is handed over and the trampolines catch panics: [`link`] |
//!
//! # Invariants every call of [`real`] relies on
//!
//! * **[I1] a bring-up runs alone.** `ob_sdk_init` / `_init_mdio` / `_attach` are called only with
//!   the glue's claim held and no owner alive, and the handles of earlier owners are dead
//!   (dropping an owner waits for their calls in progress), so no other glue call runs on any
//!   thread.
//! * **[I2] the other calls come from a live owner or handle.** The calls on glue state the C
//!   keeps without a lock (callback pointers, trunk bitmap, cached L2 user entries and special
//!   egress objects, the shell's fd 1) take `&mut` of the owner, so they are serialised; the
//!   handles only read MDIO registers and temperatures.
//! * **[I3] callbacks.** The function pointers handed to the glue are this crate's trampolines
//!   ([`link`]: valid for the process's life, callable with any arguments from any thread, never
//!   unwinding); the context is a key, never dereferenced.
//!
//! The type state, as the compiler holds it:
//!
//! ```compile_fail,E0599
//! // attach only: misc init did not run, so there is no MDIO
//! fn mdio(a: &mut sdk_glue::Attached) {
//!     let _ = a.mdio_c45_read(0x24, sdk_glue::Mmd::new(1).unwrap(), 0);
//! }
//! ```
//!
//! ```compile_fail,E0599
//! // after init_mdio there is no BCM layer: no port calls
//! fn ports(s: &mut sdk_glue::Sdk) {
//!     let _ = s.port_enable_set(1, true);
//! }
//! ```
//!
//! ```compile_fail,E0277
//! // an owner is not Sync: its handles are the way to other threads
//! fn sync<T: Sync>() {}
//! sync::<sdk_glue::Switch>();
//! ```
//!
//! ```compile_fail,E0277
//! // a callback runs on SDK threads: it must be Send + Sync + 'static
//! fn register(sw: &mut sdk_glue::Switch) {
//!     let here = std::rc::Rc::new(0);
//!     let _ = sw.set_link_callback(move |_, _| drop(here.clone()));
//! }
//! ```
//!
//! ```
//! // the handles are what crosses threads
//! fn handles<T: Clone + Send + Sync + 'static>() {}
//! handles::<sdk_glue::MdioBus>();
//! handles::<sdk_glue::TempReader>();
//! ```
//!
//! What cannot be checked here: that the linked library is the glue the declarations were
//! checked against (`sdk_glue_sys::HEADER_CHECK`), and what the SDK does to the hardware.
//!
//! # Linking and the fakes
//!
//! Features `real` and `fake` choose the C glue (see `sdk-glue-sys`); with either, the
//! bring-ups without a chip argument ([`Switch::init`], ...) use it. Feature `rust-fake` adds
//! [`rust_fake`]: [`rust_fake::Chip`] instances, used through [`Switch::init_on`] & co., any number
//! per process and in parallel; and, in a build that links no C glue, the process's own fake,
//! which the plain bring-ups then use, with the hooks of the C fake under the same names
//! ([`fake`] is then [`rust_fake::process`]). Such a build calls no C at all (Miri runs it). The tests
//! of this crate run the [`real`] module against the C fake and every wrapper against the Rust one.
//! The C fake (`sdk-glue-sys/fake/fake_glue.c`) goes once its users have moved (see
//! [`rust_fake`]).

pub mod attach;
pub mod error;
#[cfg(ob_glue_fake)]
pub mod fake;
pub mod field;
mod glue;
pub mod l2;
pub mod l3;
pub mod lag;
pub mod link;
pub mod mdio;
pub mod netif;
pub mod policer;
pub mod port;
#[cfg(not(ob_glue_rust_only))]
mod real;
#[cfg(feature = "rust-fake")]
pub mod rust_fake;
pub mod shell;
pub mod switch;
pub mod temp;
pub mod trap;
pub mod unit;
pub mod vlan;

#[cfg(ob_glue_rust_only)]
pub use rust_fake::process as fake;

pub use attach::Attached;
pub use error::{Code, Error};
pub use field::{FpEntry, FpQual, FpRule, IpType};
pub use l2::Dest;
pub use l3::RouteTarget;
pub use lag::TrunkMember;
pub use link::{CallbackStats, Gate, L2Event};
pub use mdio::{MdioBus, Mmd};
pub use netif::Netif;
pub use policer::PolicerCfg;
pub use port::{Fec, Learn, Loopback, PortInfo, PortStat};
pub use switch::{ChipLimits, Switch, TableUsage};
pub use temp::{TempReader, Temperatures};
pub use trap::TrapKind;
pub use unit::{InitError, Sdk};

/// Which glue this build links: `real`, `fake` (the C fake) or `none`, and `rust-fake` in a build
/// that links none and has the Rust fake (what the plain bring-ups then use).
#[cfg(not(ob_glue_rust_only))]
pub const LINKED: &str = sdk_glue_sys::LINKED;
/// Which glue this build links: `real`, `fake` (the C fake) or `none`, and `rust-fake` in a build
/// that links none and has the Rust fake (what the plain bring-ups then use).
#[cfg(ob_glue_rust_only)]
pub const LINKED: &str = "rust-fake";
/// What the build did to check the declarations against `sdk_glue.h`.
pub const HEADER_CHECK: &str = sdk_glue_sys::HEADER_CHECK;

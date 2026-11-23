//! Safe access to the Broadcom SDK through sai-openbcm's C glue (`sdk_glue.h`, declared by
//! `sdk-glue-sys`). This crate is the only caller of those functions and holds all the
//! `unsafe` of that side: one block per C call, each with its `SAFETY` comment.
//!
//! # Shape
//!
//! * [`Sdk`] ([`unit`]): the owning handle of the one SDK unit of this process. It only exists
//!   after a bring-up function returned success, so everything that needs an initialised SDK
//!   is a method on it and cannot be called early. At most one exists at a time. Not `Sync`.
//! * [`Error`] / [`Code`] ([`error`]): the glue's `OB_E_*` return codes.
//! * One module per area, each adding methods to [`Sdk`]: [`mdio`] (clause-45 access through
//!   the chip's MIIM master), [`temp`] (die temperature monitors).
//! * [`Switch`] ([`switch`]): the handle after the full bring-up ("init all"). It owns an
//!   [`Sdk`] (and derefs to it), and is the only way to what needs the BCM layer: [`port`],
//!   [`vlan`] and the link callback ([`link`]). After [`Sdk::init_mdio`] there is no BCM
//!   layer, and no `Switch`, so those calls cannot be made.
//! * `fake` (only when the fake glue is linked): the fake's test hooks, e.g. a link event
//!   from any thread.
//!
//! # What the glue requires, and who guarantees it
//!
//! | requirement (`sdk_glue.h`) | guaranteed by |
//! |---|---|
//! | no `ob_sdk_mdio_*` before `ob_sdk_init_mdio` (or `ob_sdk_init`) succeeded: after a bare "init soc" the MDIO clock is at its reset value, which hung PHYs on the AS7772 | the methods take an [`Sdk`], which only a successful bring-up returns (the glue checks again: `OB_E_INIT`) |
//! | clause 45 only | [`mdio::Mmd`] holds 1..=31 (the glue checks again: `OB_E_PARAM`) |
//! | `err` / `cur` / `peak` / `val` point to buffers of the size passed | built here from local arrays and their lengths |
//! | strings are NUL-terminated | `CString`; a path with a NUL byte is refused before the call |
//! | one bring-up per process at a time (the glue's state is global, one SDK lock per unit) | a process-wide flag: a second [`Sdk`] is refused while one is alive |
//! | port, VLAN and callback calls need "init all" (`ob_sdk_init`) | they are methods of [`Switch`], which only `ob_sdk_init` returns |
//! | the callback context stays valid while the SDK may call back | no memory is handed over: see [`link`] |
//!
//! What cannot be checked here: that the linked library is the glue the declarations were
//! checked against (`sdk_glue_sys::HEADER_CHECK`), and what the SDK does to the hardware.
//!
//! # Linking
//!
//! Features `real` and `fake` choose the glue (see `sdk-glue-sys`). The tests of this crate
//! run against the fake, so the `unsafe` blocks execute on any host.

pub mod error;
#[cfg(ob_glue_fake)]
pub mod fake;
pub mod link;
pub mod mdio;
pub mod port;
pub mod switch;
pub mod temp;
pub mod unit;
pub mod vlan;

pub use error::{Code, Error};
pub use mdio::Mmd;
pub use port::{Fec, PortInfo};
pub use switch::Switch;
pub use temp::Temperatures;
pub use unit::{InitError, Sdk};

/// Which glue this build links: `real`, `fake` or `none`.
pub const LINKED: &str = sdk_glue_sys::LINKED;
/// What the build did to check the declarations against `sdk_glue.h`.
pub const HEADER_CHECK: &str = sdk_glue_sys::HEADER_CHECK;

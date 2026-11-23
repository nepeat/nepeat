//! The safe core of the Rust SAI. No `unsafe`: C pointers, unions and callbacks stop at
//! `sai-ffi`, which hands this crate decoded values ([`value::Value`]) and gets values back.
//!
//! - [`oid`]: object ids. A raw 64-bit id is decoded once into a typed id ([`oid::PortId`],
//!   [`oid::VlanId`], ...); functions below the entry points only take typed ids.
//! - [`store`]: the object store. It owns every object, counts references between them and
//!   refuses to remove a referenced object or to store a reference to a missing one.
//! - [`attrs`]: argument checking by the SAI metadata (`sai-meta`): unknown, read-only,
//!   duplicate and missing attributes, object ids of the wrong type.
//! - [`backend`]: what the model needs from a switch chip ([`backend::Backend`]), and
//!   [`virt`], the in-memory implementation.
//! - [`bag`]: the object types whose whole state is their attributes, and the entries
//!   (FDB, neighbor, route): one implementation driven by the metadata.
//! - [`caps`]: what the capability queries answer.
//! - [`sai`]: the entry points ([`sai::Sai`]): create, remove, set, get by object type, and
//!   the events a backend raises turned into notifications.
//!
//! Failure rule: an operation calls the backend first and changes the store only after the
//! backend succeeded, so a failed call leaves both as they were.
#![forbid(unsafe_code)]

pub mod attrs;
pub mod backend;
pub mod bag;
pub mod caps;
pub mod error;
pub mod model;
pub mod oid;
pub mod sai;
pub mod store;
pub mod value;
pub mod virt;

pub use error::{Error, Result};

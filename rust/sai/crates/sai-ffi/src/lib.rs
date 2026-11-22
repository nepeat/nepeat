//! `libsai.so`: the C ABI of the Rust SAI, and the only crate of the workspace with
//! hand-written `unsafe`. Everything a C caller hands over is turned into Rust values here
//! and nothing unsafe crosses into `sai-core`.
//!
//! Every entry point has one shape ([`generic`]): check the pointers ([`ptr`]), decode the
//! attributes by the metadata ([`convert`]), call the safe core under the API lock
//! ([`state`]), encode the answer; all inside [`guard::guard`], which turns a panic into
//! `SAI_STATUS_FAILURE`.
//!
//! Where the unsafe code is:
//! - [`ptr`]: the pointer helpers. Each states what the C caller must guarantee.
//! - [`convert`]: reading members of `sai_attribute_value_t` (a union) and writing lists.
//! - [`state`]: calling the profile callbacks of the service method table.
//! - [`notify`]: calling the port state callback.
//! - [`tables`] / [`global`]: the exported functions themselves, which only forward their own
//!   contract to the helpers above.
//!
//! No `static mut`: the state is one `Mutex<Option<..>>`, log levels are atomics, method
//! tables are immutable statics.
// Exported functions take raw pointers by the nature of a C ABI; their contracts are in "# Safety".
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod convert;
mod generic;
pub mod global;
mod guard;
mod log;
mod notify;
mod ptr;
mod state;
mod tables;

/// A SAI status code.
type Status = sai_sys::sai_status_t;

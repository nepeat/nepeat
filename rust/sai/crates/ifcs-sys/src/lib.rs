//! Raw declarations of the IFCS API (the Innovium / Marvell Teralynx 7 SDK, the CS8210's
//! `libifcs.so` 0.12.41), generated at build time by bindgen from the GENERATED headers
//! `ifcs_api.h` / `ifcs_api_qos.h` in `$IFCS_API_INC` (`rust/sai/nix/ifcs-headers.nix`: the
//! sai-teralynx generator over the public ishell 2.0.1 ctypes; README "Teralynx backend"). Nothing
//! here is written by hand and nothing generated is committed: the declarations are derived from
//! vendor material (the owner's publishing question, open), so they exist only in a build.
//!
//! What it has:
//! - the types, structs and enumerators of the header (the anonymous `enum { ... }` blocks give
//!   constants of bindgen's `_bindgen_ty_N` types; object-like macros give typed constants);
//! - the function pointer typedef `<name>_fn` of every `IFCS_SYMBOLS` entry (libifcs is
//!   `dlopen`ed, nothing is linked) and the list itself: [`SYMBOLS`], [`SYMBOL_COUNT`] and the
//!   macro [`for_each_symbol!`] (`$m!((name, name_fn), ...)`);
//! - the unions of the header as **opaque** byte blobs (`ifcs_attr_value_t`, the key maps, the IP
//!   address union): reading a Rust union is `unsafe`, a blob is not; their members all sit at
//!   offset 0 and the `ifcs` crate encodes them by the member structs' checked layouts.
//!
//! The build script checks the layout twice, as `sai-sys` does: bindgen's own size / alignment /
//! offset assertions (compiled into this crate for every target) and the same numbers plus every
//! enumerator and constant re-asserted with `_Static_assert` by the **target's** C compiler
//! (`IFCS_SYS_ABI_CC`; `IFCS_SYS_ABI_CHECK=require` in Nix makes a missing compiler an error).
//! It also asserts that the `IFCS_SYMBOLS` list it read has exactly the number of entries the C
//! preprocessor counts, each with its typedef. [`ABI_CHECK`] says what the second check did.
//!
//! Function-like macros (`IFCS_STATUS_REASON`, `IFCS_HANDLE`, ...) have no bindgen output: the
//! `ifcs` crate implements them and tests them against values the C side produces.
//!
//! Only for the targets the Teralynx SAI runs on or is tested on (64-bit little-endian Linux on
//! `x86_64` and `aarch64`); elsewhere (the AS77xx's ppc64) the crate is empty and needs no header.
#![no_std]
#![forbid(unsafe_code)]
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, missing_docs)]
// Generated code: not held to the workspace's style lints.
#![allow(clippy::all, clippy::pedantic, clippy::restriction)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
include!(concat!(env!("OUT_DIR"), "/symbols.rs"));

/// Outcome of the C-compiler ABI check of this build: `checked <n> assertions with <cc> for <target>`
/// or `skipped: <why>`.
pub const ABI_CHECK: &str = env!("IFCS_SYS_ABI_CHECK_RESULT");

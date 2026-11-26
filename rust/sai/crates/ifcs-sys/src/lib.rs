//! Raw declarations of the IFCS API (the Innovium / Marvell Teralynx 7 SDK, the CS8210's
//! `libifcs.so` 0.12.41), generated at build time by bindgen from the GENERATED headers
//! `ifcs_api.h` / `ifcs_api_qos.h`: the build script writes them with `ifcs-gen` (our templates) from
//! the ctypes file `$IFCS_CTYPES` (the public ishell 2.0.1 one; `$IFCS_OVERRIDES`: another overrides
//! file, empty for none; `$IFCS_API_EXPECT`: a directory whose headers they must equal, byte for
//! byte), or takes them from `$IFCS_API_INC` (README "Teralynx backend"). Nothing
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
//! # By-value structs
//!
//! Every struct `T` of the bindings has a **byte twin** `T_bytes` ([`CBytes`]): a
//! `#[repr(C, align(A))]` newtype over `[u8; size_of::<T>()]` with `T`'s size and alignment (both
//! from bindgen's layout, asserted equal at compile time). A Rust value of `T` may leave its
//! padding bytes undefined (a struct literal, a move or a copy need not keep them), but libifcs
//! copies whole structs into its keys and compares and hashes the raw bytes (padding included):
//! the CS8210 answered NOTFOUND for route and FDB deletes whose keys differed from the create's in
//! padding only (`ai_re/devices/cs8210-32x/evidence/rust-h2/06-ifcs-key-padding.txt`). So the
//! `ifcs` crate keeps every struct it hands libifcs as its twin (zeroed, then the fields written at
//! their offsets) and passes pointers to the twin's bytes as pointers to `T`.
//!
//! The IFCS functions that take a struct **by value** (`ifcs_l2_entry_key_t_mac_l2vni_set`,
//! `ifcs_route_entry_key_t_ip_dest_l3vni_set`: [`BY_VALUE_ARGS`]) are declared, in their `<name>_fn`
//! typedefs, with the twin in that place, so what reaches libifcs is the twin's bytes. ABI
//! equivalence of a by-value `T_bytes` and a by-value `T` (the build refuses any other case):
//! - SysV x86_64: an aggregate of at most 16 bytes is classified per eightbyte; `T` has integer
//!   members only (no float / double: checked), so every eightbyte of it is INTEGER, as every
//!   eightbyte of a `[u8; N]` is: the same general-purpose registers (or, out of them, the same
//!   stack slots). Above 16 bytes both are MEMORY: a copy in the argument area of the same size
//!   and alignment.
//! - AAPCS64: neither is an HFA or HVA (`T` has no floating-point or vector member: checked), so
//!   both are general composites of the same size: up to 16 bytes in the same `x` registers
//!   (alignment up to 8, checked: no even-register rule of 16-byte alignment), above 16 bytes
//!   replaced by a pointer to a caller-made copy of the same size.
//! - Rust lowers a `#[repr(C)]` aggregate by the same rules for `extern "C"` (it classifies the
//!   layout, not the field names), so the twin is passed as the target's C passes `T`.
//! The `ifcs` crate's `tests/abi.rs` proves it on both targets: C callees of the real signatures
//! compare every byte they receive with the expected zero-padded image.
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

/// The byte twin of a C struct of the header (`<struct>_bytes`, "By-value structs"): its size and
/// alignment, every byte a plain `u8`.
pub trait CBytes: Copy {
    /// The struct it is the bytes of.
    type C;
    /// All bytes zero (the C `memset(&s, 0, sizeof s)`).
    const ZERO: Self;
    /// The bytes.
    fn as_bytes(&self) -> &[u8];
    /// The bytes, writable.
    fn as_bytes_mut(&mut self) -> &mut [u8];
    /// A pointer to the bytes as the struct (same size and alignment; casting it is safe, what may be
    /// done with it is the caller's business).
    fn c_ptr(&mut self) -> *mut Self::C {
        core::ptr::from_mut(self).cast()
    }
}

/// Outcome of the C-compiler ABI check of this build: `checked <n> assertions with <cc> for <target>`
/// or `skipped: <why>`.
pub const ABI_CHECK: &str = env!("IFCS_SYS_ABI_CHECK_RESULT");

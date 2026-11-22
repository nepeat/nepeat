//! Raw SAI C ABI, generated at build time by bindgen from the SAI headers in `$SAI_INC`
//! (the SAI revision sairedis is pinned to). Nothing here is written by hand: types,
//! enumerators, method tables and the prototypes of the global `sai_*` functions are the
//! header's. Status codes, which the headers define through a function-like macro bindgen
//! cannot evaluate, come from an enum the build script lets the C front end evaluate
//! (`SAI_STATUS_*` below are that enum's values).
//!
//! The build script checks the layout twice: bindgen's own size / alignment / offset
//! assertions (clang's numbers against rustc's, compiled into this crate for every target)
//! and the same numbers plus every enumerator against the target's C compiler
//! (`_Static_assert`, see `build.rs`). [`ABI_CHECK`] says what the second check did.
//!
//! This crate has no `unsafe` block and no `unsafe impl`. What it has are the generated
//! `unsafe extern "C"` declarations of the global `sai_*` functions (which is why it cannot be
//! `#![forbid(unsafe_code)]`). Using its types is safe; calling through its function pointers
//! and reading its unions is not, and only `sai-ffi` does that.
#![no_std]
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals, missing_docs)]
// Generated code: not held to the workspace's style lints.
#![allow(clippy::all, clippy::pedantic, clippy::restriction)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// Outcome of the C-compiler ABI check of this build: `checked <n> assertions with <cc> for
/// <target>` or `skipped: <why>`.
pub const ABI_CHECK: &str = env!("SAI_SYS_ABI_CHECK_RESULT");

/// The headers' version as `sai_query_api_version` reports it.
pub const SAI_API_VERSION: sai_api_version_t = (SAI_MAJOR as sai_api_version_t) * 10000
    + (SAI_MINOR as sai_api_version_t) * 100
    + SAI_REVISION as sai_api_version_t;

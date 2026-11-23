//! Raw C interface of `sdk_glue.h` (sai-openbcm): the functions through which everything else
//! reaches the Broadcom SDK. `sdk_glue.c` stays C: it is the only file that includes SDK headers
//! and is compiled with the SDK's own defines. Safe wrappers: the `sdk-glue` crate; nothing else
//! should call these.
//!
//! # Generated, never written by hand
//!
//! `build.rs` runs bindgen over `sdk_glue.h` (all functions, the five structs `ob_port_info_t`,
//! `ob_table_usage_t`, `ob_chip_limits_t`, `ob_fp_rule_t`, `ob_policer_cfg_t`, the three
//! callback types, the `OB_E_*` codes, `OB_LAG_DEST_BIT`, the `OB_FPQ_*` qualifier bits and the
//! `OB_FP_*` IP types). The header is required: `SDK_GLUE_INC`, else the monorepo's own copy
//! (`ai_re/nix/packages/sai-openbcm/src`). Then, with the target's C compiler (`SDK_GLUE_SYS_CC`, ...; x86_64 natively, the
//! cross gcc for big-endian ppc64): every size, alignment and field offset bindgen asserted
//! (clang's view, compiled into this crate as `const` assertions) and every `OB_*` constant as a
//! `_Static_assert` against the header, and every function declared again (C refuses an
//! incompatible redeclaration). [`HEADER_CHECK`] says what happened; `SDK_GLUE_SYS_CHECK=require`
//! fails a build without it. The libclang target for ppc64 ELFv2 is spelled as sai-sys spells it.
//!
//! The ints whose meaning is an order the glue hard-codes for its callers (`which` of
//! `ob_sdk_port_stat_get`, `kind` of `ob_sdk_trap_set`, the `mode` of FEC / loopback / learning)
//! are the C enums of the header (`ob_port_stat_t` / `OB_PORTSTAT_*`, `ob_trap_kind_t` /
//! `OB_TRAP_*`, `ob_fec_t`, `ob_loopback_t`, `ob_learn_t`; the C++ `ob::PortStat` etc. of
//! backend.h are checked against them by static assertions there): [`order`] is generated from
//! bindgen's view of them (values 0, 1, ... in order, `*_COUNT` last), and the build compares the
//! counter and trap tables of `sdk_glue.c` with them (lengths, and the LLDP / LACP kinds it takes
//! by name) when `sdk_glue.c` is there ([`ORDER_CHECK`]).
//!
//! # Safe declarations: the owner's rule
//!
//! A glue function is declared `safe fn` in the `unsafe extern "C"` block **only if**
//!
//! * (a) all its arguments and its result are scalars: no pointer, buffer, string, callback or
//!   struct by pointer; checked by `build.rs` on bindgen's signature, a violation stops the build;
//! * (b) its C implementation refuses every out-of-range argument itself before it touches the
//!   SDK (unit attached, the unit "init all" or misc init ran on, port in the chip's valid port
//!   map, vid 1..4094, ids non-negative or -1 for none, enum values in range): in `sdk_glue.c` the
//!   body starts with the `OB_CHECK_*` macros (no call before the first one; checked by
//!   `build.rs` when `sdk_glue.c` is found, required with `SDK_GLUE_SYS_CHECK=require`). An SDK
//!   call that validates the port itself is not enough.
//!
//! The rule is necessary, not sufficient. Five functions with scalar signatures stay `unsafe`
//! because they read or write glue globals other calls write without a lock (callback
//! pointers, the trunk bitmap, the cached L2 user entries): two threads calling them would race
//! in C. Everything else keeps an `unsafe fn` declaration, and `sdk-glue` calls it in a small
//! `unsafe` block with a `// SAFETY:` contract.
//!
//! What `safe` does **not** cover (stated plainly): the glue is not thread-safe against a
//! bring-up. `ob_sdk_init` / `_init_mdio` / `_attach` (unsafe) write the glue's state flags and
//! re-run the SDK's "init all", and a call on another thread at the same time races in the SDK
//! and on those flags. `sdk-glue` rules that out by its types (no handle exists while a
//! bring-up runs, and its MDIO / temperature handles stop calling when their switch is
//! dropped), so the safe declarations are sound for this workspace's only caller; any other
//! caller of this crate has to keep that rule itself.
//!
//! The table below (generated: function, declared, why, with the line numbers of the checks
//! in the `sdk_glue.c` of this build) counts both kinds.
//!
//! # Linking
//!
//! Nothing is linked by default. Feature `real` links the glue and libsdk (`SDK_GLUE_LINK`:
//! whitespace-separated `<path>/lib<name>.a`, `-L<dir>`, `-l<name>`). There is no C fake: tests run
//! on the pure-Rust fake of `sdk-glue` (feature `rust-fake` there), which calls nothing of this
//! crate but its constants and types.
#![no_std]
#![allow(
    missing_docs,
    non_camel_case_types,
    non_upper_case_globals,
    reason = "generated declarations; sdk_glue.h documents them"
)]
#![allow(clippy::all, clippy::pedantic, clippy::restriction, reason = "generated code")]
#![doc = include_str!(concat!(env!("OUT_DIR"), "/declarations.md"))]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// Outcome of the check of the generated declarations against `sdk_glue.h` with the target's C
/// compiler: `checked <n> declarations ... with <cc> for <target> against <header>` or
/// `skipped: <why>`.
pub const HEADER_CHECK: &str = env!("SDK_GLUE_SYS_CHECK_RESULT");

/// The rule for `safe fn` declarations in this build: how many functions, how many declared
/// safe, and whether rule (b) was checked in `sdk_glue.c`.
pub const SAFE_RULE: &str = env!("SDK_GLUE_SYS_RULE");

/// What the build compared between [`order`] (sdk_glue.h) and the tables of `sdk_glue.c`.
pub const ORDER_CHECK: &str = env!("SDK_GLUE_SYS_ORDERS");

/// Which glue this build links: `real` or `none`.
pub const LINKED: &str = env!("SDK_GLUE_SYS_LINKED");

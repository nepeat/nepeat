//! The SAI MACsec object model for a MACsec engine in an external PHY that is driven by
//! named board sequences: a port of `sai_macsec.cpp` of the C++ sai-openbcm
//! (since 2026-11-23 frozen, verbatim, in `ai_re/nix/packages/sai-teralynx/frontend/src`; line
//! numbers below are that file's unless another file is named). No chip knowledge, no bus access, no `unsafe`.
//!
//! # What it is
//!
//! SONiC's MACsecOrch model, as the C++ has it:
//!
//! | object | what it does here |
//! |---|---|
//! | MACSEC (one per direction) | capabilities only |
//! | MACSEC_PORT (port, direction) | `reset.<dir>` (optional) + `enable.<dir>` on the port's PHY; the ASIC's inter-frame gap |
//! | ACL table of a `*_MACSEC` stage, its entries | kept here, never in the ASIC. Bound with the port's `INGRESS/EGRESS_MACSEC_ACL`; an enabled entry with `ACTION_MACSEC_FLOW` is what ties a flow to a port and opens the gate |
//! | MACSEC_FLOW | the counters of the port it is bound to |
//! | MACSEC_SC (flow, SCI, cipher suite) | parameters for its SAs; one SC per flow |
//! | MACSEC_SA (SC, AN, keys, PN) | `sa-add...` / `sa-del...`; the newest egress SA transmits |
//!
//! An SA can be created before its flow is bound to a port (MACsecOrch does that while the
//! port is not enabled yet): it is kept, key included, and installed when the binding
//! appears ([`Macsec`]'s reconcile). Order on the PHY: MACSEC_PORT create = reset + enable
//! (control frames in clear, data dropped); SA = `sa-add[-first]` then `gate-open`; before the
//! last SA or the transmitting egress SA goes, `gate-close`; MACSEC_PORT remove = the SAs
//! still installed, then `disable`.
//!
//! # Shape
//!
//! - [`Macsec`] owns the objects and its picture of what every PHY holds. Entry points take
//!   typed ids ([`ids`]) and typed values ([`value::Value`]) and answer with
//!   `sai_core::Error` (the SAI status). One method per C++ entry point, same checks in the
//!   same order, same statuses. Not thread-aware: the caller holds its lock, as `guarded`
//!   does in the C++.
//! - The effect side is [`SeqRunner`]: "run sequence *name* on the PHY of port *p* with
//!   these parameter bytes", plus the inter-frame gap. Sequence names are built in
//!   [`runner::seq`], parameter names and byte orders are in [`secret::param`]; both are
//!   the contract documented at the top of `ext_macsec.h`. The table parser and executor
//!   are another crate (`phy-seq`); this crate does not depend on it.
//! - Failure rule: runner first, model second. A create whose sequence fails changes
//!   nothing (the run statistics aside). Removals are the exception the C++ makes (Q1).
//! - [`Macsec::check_invariants`] states the model's rules as a checkable property.
//!
//! # Key material
//!
//! SAK, hash key, salt and the XPN nonce live in [`Secret`] / [`Params`]: `Debug` prints
//! sizes only, the bytes are on the heap and are overwritten on drop (the `zeroize` crate).
//! The model keeps an SA's key until the SA is installed, wipes it then, and never reads it
//! back (SAK, SALT, AUTH_KEY, SSCI are write-only). No log line, status text or error text
//! carries parameter bytes; the tests search all three for the test keys.
//! Limits: copies outside these types are not covered (the C caller's attribute list; the
//! runner's own handling of the bytes it is lent), and a moved `Secret` moves only its
//! pointer, but `Value::clone` copies the bytes into a second `Secret` (wiped on its own drop).
//!
//! The hash key: the C++ does no AES. It hands AUTH_KEY to the sequence as `hkey` (Q11).
//! [`AuthKeyPolicy::Verify`] (off by default) recomputes AES-ECB(SAK, 0^128) with the
//! RustCrypto `aes` crate and refuses an SA whose AUTH_KEY differs; see [`hkey`] for
//! the implementation notes (constant-time software path on ppc64, vectors in the tests).
//!
//! # Byte orders (the target is big-endian ppc64; nothing here depends on the host's order
//! except the one place SAI's contract does)
//!
//! - `pn`: 8 bytes, least significant first. `window`: 4 bytes, least significant first.
//! - `ssci`: the attribute is a number; the parameter is its 4 bytes most significant first
//!   (the wire order). `iv` = salt with those 4 bytes XOR-ed into bytes 0..3.
//! - `key`: 16 or 32 bytes by the SC's cipher suite at the time the SA is created; a 128-bit
//!   SAK is the last 16 bytes of SAI's 32.
//! - `sci`: 8 bytes as on the wire = the bytes of the attribute's `u64` in memory order
//!   ([`value::sci_from_attr`], Q13).
//! - Results: 16-bit words, least significant word first, the first four make the number.
//!
//! # Intentional differences from the C++
//!
//! - **D1** An installed SA is looked for on the port whose PHY holds it. The C++ uses
//!   "the port an SA of its SC was last installed on" (`sc.hw`, set at :241, used at :960,
//!   :977, :379). Once a flow moved to another port and got a new SA there, removing an old
//!   SA runs nothing in the C++ and returns success: the key stays in the first port's PHY
//!   and its AN stays taken; its counters and PN are read from the wrong PHY. Shown against
//!   the real C++ in `tests/differential.rs`
//!   (`where_a_flow_moved_the_cpp_leaves_the_key_in_the_phy`). Without a moving flow the two
//!   are the same port.
//! - **D2** Values are typed. A value of another kind than the attribute's is
//!   `INVALID_ATTR_VALUE` at its index (the C++ reads the union member blindly); an object id
//!   of another type is answered like an unknown id of the right type. ACL glue called with
//!   an id that is not a MACsec-stage table / entry answers `INVALID_OBJECT_ID` (the C++
//!   relies on `sai_acl.cpp` to route; its `S().tables[id]` would create an empty object).
//! - **D3** [`AuthKeyPolicy::Verify`] exists (default: the C++ behaviour).
//! - **D4** Log lines go to a callback ([`Options::log`]); the texts are the C++'s except
//!   that a finished run's line has no operation count. Timing (`max_us`) is wall-clock around
//!   the runner call.
//! - **D5** The switch's ports are given once, at [`Macsec::attach`]; the C++ looks ports up
//!   in the live port table at every call. Ports created later (breakout) are unknown here.
//!   A library can close this difference (the C++ one did through sai-macsec-capi, deleted
//!   2026-11-23): the table is attached with [`Macsec::attach_chip`] before the SAI ports
//!   exist, and the library hands its live port table over with [`Macsec::set_switch_ports`]
//!   whenever it changed.
//! - **D6** Not ported, because it is not the model's: reading and parsing the table file
//!   (`ext_macsec_load`: "cannot open" = FAILURE, a parse error = INVALID_PARAMETER, both
//!   refuse to create the switch), the spin-wait `wait_us`, the sequence lock, the root-only
//!   parameter file of `extmacsec run` ([`Macsec::diag_run`] takes [`Params`]). The unused
//!   `kScStubs` table (:711) and `MsFlow::hw` (:54, never written) are dropped.
//! - **D7** Object ids come from the model's own counter (never reused). In the library the
//!   ACL table / entry ids must come from the allocator the other ACL objects use:
//!   [`Options::alloc`] (the deleted sai-macsec-capi passed the C++ `ctx().alloc`).
//! - **D8** List attributes come back as `Vec`s; SAI's buffer convention is the rim's.
//! - **D9** (Q7 fixed, owner's decision 2026-11-23) When the flow a port's data goes to
//!   changes, the SAs of every other SC are taken out of that PHY (`reconcile`: the gate closes
//!   before the last, then `sa-del[-last]`); the new flow's SAs then install as the first of
//!   their SC. The old SAs become "waiting" without their key (wiped at install, as Q3), so
//!   they cannot come back. A flow that goes away (the entry disabled) still leaves its SAs in,
//!   as the C++ (the gate closes). The rule is on the state, not on the step: whenever the port
//!   has a bound flow, installed SAs of an SC on another flow come out. So the switch may come
//!   in two steps (an entry disabled, then another entry, or the same one with another flow,
//!   gives the port a flow): there the C++ opens the gate on the old SC's SA for a flow it does
//!   not belong to (:311). Tests: `d9_a_flow_switch_takes_the_old_scs_sas_out`,
//!   `d9_a_flow_switch_through_a_disabled_entry`; against the C++: `tests/differential.rs`, which
//!   holds the model to the C++ with exactly this edit and D10's (`tests/cpp/listed_fixes.rs`).
//!   SONiC does not reach D9: with `SCI_IN_INGRESS_MACSEC_ACL` false (:457) MACsecOrch gives
//!   every SC of a port and direction the port's one flow, and removes an SC's SAs before its
//!   ACL entry.
//! - **D10** (Q10 fixed, the same decision) The cipher suite of an SC cannot change while it has
//!   SAs waiting for their flow either: `OBJECT_IN_USE`, as for installed ones. Test:
//!   `d10_no_cipher_suite_change_under_waiting_sas`.
//! - [`Options::engines_on_at_stop`] (profile key `SAI_OPENBCM_MACSEC_ENGINES_ON_AT_STOP`,
//!   designed, **off**: the owner decides on the clear-traffic window): removing a MACSEC_PORT
//!   skips `disable` (the engine stays on with its gate closed: data dropped, not sent in
//!   clear), and the start-up clean-up skips it after `reset`. Off, the model is the C++.
//!
//! # Behaviour of the C++ that is questionable or under-specified
//!
//! Ported as it is (the differential test holds the port to it) unless a D above says
//! otherwise; `tests/model.rs` pins each one by its number. None is fixed silently.
//!
//! - **Q1** A removal whose sequence fails still removes the object and returns the failure:
//!   `sa_uninstall` "gone from the model either way" (:270), `remove_macsec_port` erases
//!   after a failed `disable` (:560-562). The PHY may still hold the key / be out of bypass
//!   while the model says otherwise, and the caller has no object left to retry on. The
//!   opposite choice (keep the object) would leave SONiC with an object it believes gone.
//! - **Q2** `ext_macsec_detach` (:1407) runs nothing: at switch removal engines stay on,
//!   keys in them, until the next attach probes them. With a table that has no `probe`
//!   sequence nothing cleans up before the next MACSEC_PORT create (`reset`).
//! - **Q3** An SA whose MACSEC_PORT is removed first is uninstalled (:559) and becomes
//!   "waiting" again, but its key was wiped at install (:237). Every later reconcile tries to
//!   install it with the parameters it has left (:230); the executor refuses (missing
//!   `key`), the failure counter grows, the status text counts it as waiting, for ever.
//! - **Q4** An install that fails when a flow gets bound (not inside `create_macsec_sa`) is
//!   only logged (:299-309); the SA stays waiting with its key and is retried by every later
//!   reconcile of that port. The caller is never told.
//! - **Q5** A gate sequence that fails is only logged (:251). `sa_uninstall` then removes
//!   the last / the transmitting SA although the gate may still be open, against its own
//!   comment ("never transmit with, or accept through, what is about to go", :264).
//! - **Q6** A `probe` that fails at attach is skipped (:1395): that engine is neither reset
//!   nor disabled, whatever a previous process left in it.
//! - **Q7** (fixed in this port: D9) When the flow an entry sends data to changes (or another
//!   entry of the table takes over, at once or after a time without a flow), the SAs of the
//!   previous flow's SC stay in the PHY. The gate condition
//!   (:311) counts every installed SA, so the gate stays open for the new flow with the old
//!   SC's SA; an SA of the new SC on an AN the old one holds is INSUFFICIENT_RESOURCES
//!   (:302).
//! - **Q8** `ACTIVE_EGRESS_SA_ID` (:844) returns the transmitting SA of the port in `sc.hw`
//!   whoever's SA that is; `sc.hw` is never cleared (:241).
//! - **Q9** `CONFIGURED_EGRESS_XPN` reads the live packet number like `CURRENT_XPN`
//!   (:1019-1021), not what was configured.
//! - **Q10** (fixed in this port: D10) The SC's cipher suite may change while SAs wait (only installed SAs fix it,
//!   :801), but a waiting SA keeps the key bytes cut for the suite it was created under
//!   (:917-919): 128 to 256 makes it uninstallable; 256 to 128 installs the *first* 16 bytes
//!   where SAI puts a 128-bit SAK in the last 16; non-XPN to XPN lacks salt / SSCI; XPN to
//!   non-XPN keeps a PN beyond 32 bits that :915 would have refused.
//! - **Q11** AUTH_KEY is trusted to be AES-ECB(SAK, 0^128) (:920). A wrong one installs an SA
//!   that fails every ICV, with nothing to show why (see D3).
//! - **Q12** Flow counters are accumulated per flow object wherever it is bound (`MsFlow::hw`
//!   was meant to track that, :54): with absolute counters a flow that moved jumps to the
//!   other port's values; with clear-on-read ones the two ports' counts are summed.
//! - **Q13** The SCI is the attribute's `u64` in memory order (:758, "as SONiC passes it").
//!   On the big-endian AS7772 that is the wire order of the number and it is verified on
//!   hardware there. On a little-endian host it is the reverse of the number's wire order;
//!   whether SONiC's value is laid out for that was not checked (the unit tests build the
//!   value from bytes, so they pass on both).
//! - **Q14** The statistics capability (:1327) is "some `stats-*` sequence reads a result of
//!   that name", for SA and flow alike: a name only the flow sequence reads would also be
//!   announced for SAs if both tables had it. (No SAI statistic has the same name in both.)
//! - **Q15** `MINIMUM_INGRESS_XPN` set on a waiting SA (:998) is not range-checked for a
//!   non-XPN suite; on an installed SA it is accepted and does nothing (stated in the C++).
//! - **Q16** `clear_*_stats` ignores a failure of the read that empties the PHY's counters
//!   and returns success (:704, :1056): what was not emptied reappears in the next read.
//! - **Q17** `clear_macsec_port_stats` / `clear_macsec_sc_stats` answer NOT_SUPPORTED without
//!   looking at the id (:610, :870).
//! - **Q18** A failed inter-frame gap is only logged (:543): MACsec is on with the narrow
//!   gap, which loses frames under load (the SecTAG + ICV have no room). After a failed
//!   `disable` the gap is restored all the same (:563).
//! - **Q19** Attribute lists with the same attribute twice: MACSEC / MACSEC_PORT / FLOW take
//!   the first (`find_attr`), SC and SA the last (their loops). Kept.
//!
//! # How `libsai.so` serves these objects
//!
//! Natively, no C ABI in between: `sai_core::macsec` is the core's seam (the `MacsecModel` /
//! `MacsecLoader` traits, the secret value types `Value::MacsecSak` / `MacsecSalt` /
//! `MacsecAuthKey` carrying this crate's [`Secret`], the port and ACL hooks, the inter-frame gap);
//! `sai-macsec-phy`'s `native` module implements it over this crate and `phy-seq`; `sai-ffi`
//! decodes the MACSEC API (the keys straight from the union into a [`Secret`]) and registers the
//! loader. (`sai-macsec-capi`, the C interface for the C++ library, was deleted with it on 2026-11-23.)
#![forbid(unsafe_code)]
#![allow(
    clippy::doc_markdown,
    reason = "the vocabulary of this crate (MACsec, MACSEC_PORT, SecTAG, MACsecOrch, EtherType) is prose here, not code: \
              the lint would put half of every sentence in backticks"
)]

mod acl;
pub mod hkey;
pub mod ids;
mod model;
mod objects;
pub mod runner;
pub mod secret;
pub mod stats;
mod status;
pub mod value;

pub use hkey::AuthKeyPolicy;
pub use ids::{AclEntryId, AclTableId, An, Cipher, Dir, FlowId, MacsecId, MacsecPortId, PortKey, SaId, ScId};
pub use model::{ChipPort, EngineAddr, IdAlloc, Level, Logger, Macsec, Options, SwitchPort, Table};
pub use runner::{SeqError, SeqRunner, seq};
pub use secret::{Params, Results, Secret, param};
pub use stats::CounterMode;
pub use status::{Counts, EngineState};
pub use value::{AclField, AclParameter, Attr, Value};

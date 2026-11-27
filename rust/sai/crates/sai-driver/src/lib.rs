//! The Rust half of the C++-vs-Rust frontend differential: op scripts (one SAI operation per
//! line) run against `sai_core::sai::Sai<Recording<VirtualBackend>>`, printing per operation the
//! status, the values read, every backend call (call-log lines, `sai_core::backend::log`) and the
//! notifications raised. Grammar and output are those of sai-openbcm's
//! `tests/frontend_driver.cpp` (its header comment is the reference; `tests/driver-scripts/*.ops`
//! are the scripts); `tests/differential.rs` compares the two drivers' outputs op block by op block
//! ([`compare`]) against an allow-list of the differences still expected.
//!
//! The driver calls the safe core directly, not the C ABI, so it needs no `unsafe`; the rim
//! (`sai-ffi`) is covered by the test-port suite instead. What the rim does on the way, the
//! driver does in a few lines, citing `sai-ffi/src/generic.rs`:
//!
//! - **Attributes** (`decode_one` / `decode_list`): one a call may not carry (create: not an
//!   attribute of the type or read-only; set: not settable) gets the type's C++ status
//!   (`sai_core::dispatch::refused_create`; a set's after the object lookup, `Sai::refused_set` /
//!   `refused_set_entry`); a value member the rim does not decode (`ValueType::Other`):
//!   `ATTR_NOT_SUPPORTED_<i>`; `i` = the position in the call (the switch's three fixed attributes
//!   count). The `MACsec` types: no metadata check (the model's statuses), as the rim. Values are built as [`sai_core::value::Value`] straight from
//!   the script text by the attribute's metadata (`sai-meta`: names, `@type` text, member) and
//!   the driver's kind, which is the C++ driver's (`vt_of` over the same `@type` text). Enum names
//!   come from the SAI headers (`build.rs`, the scan the C++ driver's `driver_names.awk` did) and print
//!   as the C++ `enum_name`: the first enumerator of the value that is not a range marker,
//!   without its type's prefix.
//! - **Object types** (`implemented`): a type the core has no id type for is `NOT_IMPLEMENTED`.
//!   NOT reproduced: the rim's API table (`sai-ffi/src/tables.rs`, `of`): a SAI API it has no
//!   method table for answers `sai_api_query` with failure, and a C caller then gets nothing to
//!   call (the C++ driver prints `NOT_IMPLEMENTED`); this driver reaches every type the core
//!   dispatches.
//! - **Gets** (`encode_all`): every list gets the C++ driver's buffer of 256 elements; a longer
//!   answer is `BUFFER_OVERFLOW`, printed as `<ATTR> count=<n>` for each list attribute.
//! - **Switch creation** (`create_switch`): the profile the backend sees holds only the keys the
//!   rim asks the SAI user for (`sai_core::keys::EXPLICIT`, the diag socket's, the external
//!   tables', `sai_core::virt::PROFILE_KEYS`); `SAI_OPENBCM_BACKEND` `openbcm` (the C++ driver's
//!   recorder over its fake), `fake` or `virtual` (else `FAILURE`); a new core over a new
//!   backend per switch.
//! - **Statuses**: `sai_core::Error` -> `sai_core::backend::log::status_name`, the call log's
//!   names (`SUCCESS`, `INVALID_ATTR_VALUE_0`, ...), as the C++ `ob::sai_status_name`.
//!
//! The profile: the C++ driver's defaults, exactly (`SAI_OPENBCM_BACKEND=openbcm`,
//! `SAI_OPENBCM_FAKE_PORTS=8`, `_LANES=4`, `_SPEED=100000`); the virtual chip numbers ports and
//! answers like the C++ fake (`VirtualBackend::cpp_by_default`), so the `init(profile=...)` lines
//! are equal.
//!
//! Events: the backend queues them on the core's `EventSink`; after each operation the driver
//! takes them off the queue (`try_recv`) and applies each with `Sai::handle_event` (the C++
//! driver waits until the library has processed the events of the operation), so the `be` lines
//! (events included, written where they happen) come first, then the `notify` lines.
//! Notifications print as the C++ driver's (`notify port <name> UP|DOWN`); a variant this
//! driver does not know yet (FDB events, once the core has them) prints as `notify <Debug form>`
//! instead of breaking the build, and the differential shows it.
//!
//! Injections (`inject link|fdb|stat`, `mdio`, `feature`, `linkscan`) go to the driver's
//! [`Hooks`]: by default [`Virtual`], the virtual backend's own hooks; like the C++ test-port hooks
//! they are not logged as calls, but the events they raise are. [`Driver::with_hooks`] runs the
//! scripts on another backend with its hooks, which may also print lines per operation between
//! the echo and the status (sai-backend-openbcm's glue differential: the hardware backend on a
//! recording fake chip, its glue calls). Differences from the C++ hooks: `inject link` on a LAG and on an
//! unknown port do nothing (the C++ fake ignores them too); `inject stat` of a counter the virtual
//! backend lacks, an MDIO device outside 1..=31 and an FDB VLAN outside 1..=4094 are parse errors
//! (the C++ test port aborts or passes them on); `dest_of` knows ports and LAGs only (the C++
//! `tp::backend_id` also answers for VRs, RIFs, policers, ACL tables / entries and `QoS` maps).
//!
//! Object names: `switch`, `cpu`, `port<i>`, `bport<i>`, `vlan1`, `vr0`, `bridge`, `stp0`,
//! `trapgroup0` (bound through the same gets as the C++ driver's), and `<type>#<n>` for an id
//! first seen in an answer, counted per type in order of sight.
#![forbid(unsafe_code)]

pub mod compare;
pub mod driver;
pub mod names;
pub mod text;
pub mod types;
pub mod values;

pub use driver::{Driver, Feature, Hooks, LogMode, OnHardware, Options, Virtual};

//! What `openbcm-mdio-script` tells people and programs about a run: the per-PHY result of a
//! script's verification section ([`Report`]), the classification of a failure
//! ([`FailClass`]), the status files ([`Status`]: `status.json` and `status.txt`), and the
//! things it needs from the system for that (who holds the SDK lock, systemd's status line,
//! MDIO reads through syncd's diag socket: [`DiagBus`]). No SDK code and no knowledge of any
//! PHY: what is checked comes from the script, names for PHYs and registers from a names file
//! ([`Names`]).
//!
//! The safe Rust port of `mdio_report.c` / `mdio_report.h` of the sai-openbcm package. The
//! status files are an interface (the NixOS module's units, `phy-status` and the login message
//! read them), so text is produced byte for byte as the C code produces it, including its
//! buffer limits: output is built as bytes (a label or a path need not be UTF-8), and no JSON
//! library is involved, as in C.
//!
//! # Differences from the C implementation
//!
//! * `status.txt` prints the start time in UTC (`+0000`). C prints local time; `std` has no
//!   safe way to the time zone. The same text wherever the zone is UTC (the appliances).
//! * [`DiagBus`] refuses an answer whose value has no hex digit after `0x`; C's `strtoul`
//!   check lets that through as 0.
//! * [`DiagBus`] takes a [`mdio_script::Dev`], so the C function's "clause 45 only" refusal
//!   cannot be reached.
//! * A names file is read whole (C: `fgets` with 200 bytes); lines are cut the same way.
//! * Paths are not limited to 4096 bytes.
#![forbid(unsafe_code)]

mod class;
mod diag;
mod names;
mod report;
mod status;
mod system;
mod text;

pub use class::{FailClass, classify_run};
pub use diag::DiagBus;
pub use names::{Names, NamesError};
pub use report::{CheckRecord, MAX_CHECKS, MAX_PHYS, PhyReport, PhyState, Report};
pub use status::{Mode, ScriptStatus, State, Status, TempStatus, result_name, text_path};
pub use system::{LockHolder, lock_holder, sd_status};
pub use text::strerror;

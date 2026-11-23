//! `openbcm-mdio-script` in Rust: run MDIO scripts ([`mdio_script`]) through the switch chip's
//! external MDIO master, for PHYs the SDK has no driver for. Run once per boot, before syncd.
//! The port of sai-openbcm's `mdio_script_main.c`: the same command line, messages, status
//! files ([`mdio_report`]) and exit codes, so the binary is a drop-in for the systemd unit.
//!
//! ```text
//! openbcm-mdio-script --check SCRIPT...                         parse only (any build)
//! openbcm-mdio-script --config CONFIG.BCM [options] SCRIPT...   run
//!   --unit N
//!   --status-file FILE.json   machine-readable result, rewritten while it runs and at the end;
//!                             FILE.txt next to it is the same for people
//!   --names FILE              names for PHYs and registers in messages and tables
//!   --hint CLASS=TEXT         this board's "what to do" for a failure class (repeatable)
//!   --temp-out FILE [--temp-prom FILE] [--temp-interval SECONDS]
//!                             publish the ASIC die temperature while the script runs
//! openbcm-mdio-script --verify-only (--diag-socket SOCKET | --config CONFIG.BCM) [--names FILE] [--json] SCRIPT...
//!                             read-only: the scripts' verification sections, every PHY
//! ```
//!
//! Exit: 0 done, 1 usage, 2 a script failed on the chip (poll, I/O, verification), 3 SDK lock
//! busy, 4 SDK init failed, 5 script missing or malformed, 7 stopped by a signal.
//!
//! # Two buses
//!
//! * syncd's diag socket ([`mdio_report::DiagBus`]): reads only, pure safe Rust, any build.
//! * the SDK through the C glue (feature `sdk`: [`glue`], over the `sdk-glue` crate), behind
//!   [`ChipOpener`] / [`Chip`] so that the run path is tested without any C.
//!
//! # What the types rule out
//!
//! * Clause 22: a script cannot express it ([`mdio_script::Dev`]), neither bus can send it.
//! * MDIO before misc init: the only way to a [`Chip`] is [`ChipOpener::open`], which is
//!   `ob_sdk_init_mdio` (init soc + init misc, MDIO clock read back) or nothing.
//! * Running a script that was not parsed completely: a run takes [`mdio_script::Script`]
//!   values, made by the parse pass over every file before anything is opened.
//! * Writes in `--verify-only`: that path runs with [`mdio_script::VerifyMode::Only`] and, on
//!   the diag socket, over a bus that refuses writes.
//!
//! The tool knows no PHY and loads no firmware itself: what is written is what the script says.
//!
//! # Differences from the C tool
//!
//! * A script is read once: the run executes what the parse pass read (C opens the file again
//!   for every pass).
//! * The differences of [`mdio_report`] (UTC in `status.txt`, a stricter diag answer check)
//!   and of [`mdio_script`].
//! * In messages on stderr, a label that is not UTF-8 is shown lossily (the status files keep
//!   its bytes); messages are not cut to C's 600-byte buffer.
//! * `POSIXLY_CORRECT` is not looked at.
//! * A script path that is a directory: "Is a directory" instead of C's "read error".
#![forbid(unsafe_code)]

pub mod app;
pub mod chip;
pub mod cli;
#[cfg(feature = "sdk")]
pub mod glue;
pub mod temp;

pub use app::{Env, run};
pub use chip::{Chip, ChipOpener, OpenError, OpenFailure, Temperatures};

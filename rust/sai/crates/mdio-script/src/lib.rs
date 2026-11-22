//! MDIO script: a list of clause-45 writes, delays, reads and polls for PHYs that have no driver
//! (retimers, `MACsec` PHYs with a vendor-only library). The script is a recording made elsewhere;
//! this crate knows no chip. It is the safe Rust port of `mdio_script.c` / `mdio_script.h` of the
//! sai-openbcm package and has no dependencies beyond `std`.
//!
//! # Format
//!
//! Text, one operation per line, numbers hex unless noted:
//!
//! ```text
//! mdio-script 1                                  first line
//! # comment
//! L <text>                                       label: names the step in messages
//! D <usec, decimal>                              delay
//! W <phy> <dev> <reg> <val>                      write
//! R <phy> <dev> <reg>                            read, value ignored (clear-on-read, FIFO pop)
//! E <phy> <dev> <reg> <val> [<mask>]             read; (v & mask) != val is counted, the first 20 logged
//! P <phy> <dev> <reg> <val> <mask> <tries> <interval>
//!                                                read until (v & mask) == val; the script FAILS after
//!                                                <tries> reads (decimal), <interval> usec (decimal) apart
//! N <phy> <dev> <reg> <val> <mask> <tries> <interval>
//!                                                the same, until (v & mask) != val
//! ```
//!
//! `<phy>` is the bus master's address word. `<dev>` is 1..1f: clause 22 cannot be expressed
//! ([`Dev`]). `P` / `N` with `<tries>` 1 is an assertion.
//!
//! # Verification section
//!
//! The assertions at the very end of a script (after the last `W`, `D`, `R`, `E` and multi-try
//! `P` / `N`; `L` lines and comments between them belong to it) are its read-back verification.
//! [`Script::stats`] says where it starts (`verify_line`). With [`VerifyMode::Report`] a mismatch
//! there does not stop the script: every assertion is read and handed to [`Hooks::verify`], and the
//! run fails at the end with [`RunErrorKind::Verify`]. With [`VerifyMode::Only`] nothing before
//! the section runs: reads only, for a status display or a post-mortem.
//!
//! # Two phases
//!
//! [`Script::parse`] checks the whole text and returns the typed operations; [`Script::run`]
//! executes a parsed script. A [`Script`] value is the proof that every line is well formed, so
//! "run a script only after a parse-only pass succeeded" (a truncated file must not leave a chip
//! half-programmed) is a fact of the types instead of a rule for callers.
//!
//! # Differences from the C implementation
//!
//! * **Nothing runs before a syntax error.** The C interpreter, called with a bus on a file that
//!   was not parsed first, executes the lines before the bad one (and calls `tick` before a line
//!   it then rejects). Here such a file never becomes a [`Script`].
//! * The verification line is not a caller-supplied number: [`VerifyMode`] uses the script's own
//!   section. `verify_only` on a script without one is [`RunErrorKind::NoVerifySection`] (C:
//!   `MDIO_SCRIPT_E_SYNTAX`, which [`RunErrorKind::code`] still returns).
//! * Messages carry no file path (the caller has it) and are not cut to a buffer size.
//! * Labels are kept as bytes, cut to 150 bytes like C; in messages a label that is not UTF-8
//!   (or was cut inside a character) is shown lossily.
//! * A bus value is a `u16`; the C callback could hand back a wider `unsigned`.
//! * [`Hooks::tick`] receives the running statistics (C: the caller keeps a pointer to them).
#![forbid(unsafe_code)]

mod hooks;
mod parse;
mod run;
mod types;

pub use hooks::{Check, Hooks, NoHooks, SpinSleeper};
pub use parse::{LoadError, ParseError, ParseErrorKind, Script};
pub use run::{Failure, RunError, RunErrorKind, RunOptions, VerifyMode};
pub use types::{BusError, DelayUs, Dev, Label, Line, MdioBus, Op, OpLetter, RegAddr, Stats, Tries, Until};

/// `MDIO_SCRIPT_E_FILE`: the script cannot be opened or read.
pub const E_FILE: i32 = -1;
/// `MDIO_SCRIPT_E_SYNTAX`: a bad line.
pub const E_SYNTAX: i32 = -2;
/// `MDIO_SCRIPT_E_POLL`: a `P` / `N` never matched, the chip does not follow the script.
pub const E_POLL: i32 = -3;
/// `MDIO_SCRIPT_E_IO`: a bus read or write failed.
pub const E_IO: i32 = -4;
/// `MDIO_SCRIPT_E_VERIFY`: the script ran to its end; verification assertions failed.
pub const E_VERIFY: i32 = -5;
/// `MDIO_SCRIPT_E_ABORT`: [`Hooks::tick`] asked to stop.
pub const E_ABORT: i32 = -6;

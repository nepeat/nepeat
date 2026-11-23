//! Sequence tables for external PHYs: board data that tells a SAI how to drive a chip the SDK
//! has no driver for, as short clause-45 sequences. This crate knows no chip. It is the safe
//! Rust port of the two table engines of the sai-openbcm package:
//!
//! * the external `MACsec` table, `ext-macsec 1` (`ext_macsec.h`, `sai_extmacsec.cpp`): named
//!   sequences with placeholders for the key, SCI, packet number, ..., and named results;
//! * the external PHY table, `ext-phy 1` (`ext_phy.h`, `sai_extphy.cpp`): per port a link poll
//!   and admin-down / admin-up sequences.
//!
//! # One engine
//!
//! Both formats are lists of the same operations on the same bus, so there is one operation
//! type ([`Op`]) and one executor ([`run`]): ext-phy uses `W R E P` with fixed PHYs and constant
//! values plus `C`; ext-macsec uses `W R E P` with "the port's PHY" and computed values plus `X`
//! and `D`. The two parsers ([`MacsecTable::parse`], [`ExtPhyTable::parse`]) are separate: the
//! formats differ in keywords, limits and in the order things are checked, and a table must get
//! the same verdict and message from this crate as from the C++ parser.
//!
//! # Formats
//!
//! Text, one item per line, numbers hex unless noted; `#` starts a comment line.
//!
//! ```text
//! ext-macsec 1                                  first line
//! counters clear-on-read | absolute             how the PHY's counters behave (default clear-on-read)
//! ifg-bits <decimal>                            inter-frame gap of the ASIC MAC while a port does `MACsec`, 0..2040
//! port <first lane, decimal> <phy>              a port that can do `MACsec` and its PHY's MDIO address word
//! seq <name>                                    starts a sequence (a-z 0-9 . -); shared by all ports
//! W p|<phy> <dev> <reg> <value>                 write ("p" = the PHY of the port the sequence runs for)
//! R p|<phy> <dev> <reg>                         read, value ignored
//! E p|<phy> <dev> <reg> <val> [<mask>]          read; (v & mask) != val stops the sequence (fault)
//! P p|<phy> <dev> <reg> <val> <mask> <tries> <interval>
//!                                               read until (v & mask) == val: <tries> reads (decimal, 1..1000),
//!                                               <interval> usec apart (decimal, 0..100000); never: fault
//! X p|<phy> <dev> <reg> <dest>                  read and keep: $<n> (n 0..15) or <result>[<word 0..7>]
//! D <usec, decimal>                             delay (0..100000)
//! ```
//!
//! `<value>` is `hhhh` (a constant), `$<n>[&<mask>][|<bits>]` (a value kept by `X` in this run,
//! masked, then OR-ed) or `<byte>.<byte>` (high byte, low byte; a byte is `hh` or
//! `<param>[<index>]` with the parameters of [`ParamName`]).
//!
//! ```text
//! ext-phy 1                                     first line
//! poll-ms <decimal>                             line-side poll period, 0..60000 (default 500)
//! up-polls <decimal>                            consecutive good polls before "up", 1..100 (default 2)
//! holdoff-ms <decimal>                          pause after raw MDIO on the diag socket, 0..600000 (default 3000)
//! port <first lane, decimal>                    starts a port
//! link | admin-down | admin-up                  starts a sequence of that port
//! W <phy> <dev> <reg> <val>   R <phy> <dev> <reg>   E <phy> <dev> <reg> <val> [<mask>]
//! P <phy> <dev> <reg> <val> <mask> <tries> <interval>
//! C <phy> <dev> <reg> <val> <mask>              link only: condition (v & mask) == val; all must hold
//! ```
//!
//! `<dev>` is 1..1f in both: clause 22 cannot be expressed ([`Dev`], the type of `mdio-script`).
//!
//! # Use
//!
//! ```
//! use phy_seq::{Bus, Dev, MacsecTable, ParamName, Params};
//!
//! struct Chip(Vec<String>);
//! impl Bus for Chip {
//!     type Error = std::convert::Infallible;
//!     fn read(&mut self, _phy: u16, _dev: Dev, reg: u16) -> Result<u16, Self::Error> {
//!         Ok(if reg == 0x9005 { 0x8001 } else { 0x00aa })
//!     }
//!     fn write(&mut self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), Self::Error> {
//!         self.0.push(format!("{phy:x} {}.{reg:04x} {value:04x}", dev.get()));
//!         Ok(())
//!     }
//!     fn delay_us(&mut self, _us: u32) {}
//! }
//!
//! let table = MacsecTable::parse(
//!     b"ext-macsec 1\nport 1 1e0\nseq sa-add.egress.0.gcm-aes-128\n\
//!       W p 1 a000 key[1].key[0]\nX p 1 9005 $0\nW p 1 9005 $0&7fff\nX p 1 a101 pn[0]\n",
//! )?;
//! let mut params = Params::new();
//! params.set(ParamName::Key, &[0x11, 0x22])?;
//! let phy = table.phy_of_lane(1).ok_or("no such port")?;
//! let mut chip = Chip(Vec::new());
//! // an optional sequence is simply absent
//! assert!(table.sequence("gate-open.egress").is_none());
//! let seq = table.sequence("sa-add.egress.0.gcm-aes-128").ok_or("no such sequence")?;
//! let results = seq.run(&mut chip, phy, &params)?;
//! assert_eq!(chip.0, ["1e0 1.a000 2211", "1e0 1.9005 0001"]);
//! assert_eq!(results.u64("pn"), 0xaa);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Guarantees
//!
//! * A [`MacsecTable`] / [`ExtPhyTable`] value is the proof that the whole text is well formed;
//!   nothing can be run from a text that is not.
//! * Parameters are checked before the first bus access ([`ExecErrorKind::MissingParam`]).
//! * [`Params`] never prints its bytes and wipes them on drop. Error messages hold no parameter
//!   byte. [`Value::is_secret`] tells a tracer which written words are key material.
//! * No `unsafe`, no panics in library code (workspace lints), no byte-order dependence: bus
//!   words are built with shifts (`hi << 8 | lo`) and results are lists of 16-bit words, least
//!   significant first.
//!
//! # Differences from the C++ engines
//!
//! * A bus value is a `u16` and a PHY address word a `u16` (C++: `unsigned`). The table formats
//!   cannot express more; only a caller of the C++ `ext_macsec_exec` could pass a wider PHY.
//! * The results of a run that fails are not delivered (C++ leaves the words read so far in the
//!   caller's map).
//! * [`Params`] holds only the eight parameters a table can name and at most
//!   [`ParamName::max_len`] bytes of each (C++: a map of any names and lengths, of which no more
//!   can be read).
//! * The parsers agree with C++ on every byte string, including the corners of `strtoul` (an
//!   `0x` prefix is accepted; a number word is read up to a NUL byte).
#![forbid(unsafe_code)]

pub mod cli;
mod error;
mod exec;
mod extphy;
mod lex;
mod macsec;
mod op;
mod params;

pub use error::{Format, ParseError, ParseErrorKind};
pub use exec::{Bus, ExecError, ExecErrorKind, Outcome, OverMdio, Results, required_params, run};
pub use extphy::{ExtPhyPort, ExtPhyTable, Section};
pub use macsec::{CounterMode, MacsecTable, Sequence};
pub use op::{
    ByteSrc, Compare, Dest, Dev, Op, OpKind, OpLetter, ParamByte, ParamName, PhySel, Reg, ResultName, Slot, Tries,
    Usec, Value, Word,
};
pub use params::{ParamTooLong, Params};

/// The limits of the two formats.
pub mod limits {
    pub use crate::extphy::MAX_OPS as EXT_PHY_MAX_OPS;
    pub use crate::macsec::MAX_OPS as EXT_MACSEC_MAX_OPS;
}

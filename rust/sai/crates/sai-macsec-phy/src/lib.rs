//! [`sai_macsec::SeqRunner`] over [`phy_seq`]: the model of `sai-macsec` runs the board's
//! external MACsec table (`ext-macsec 1`, `ext_macsec.h`) with the Rust parser and executor on a
//! clause-45 bus. Together the three crates are the whole Rust path from a SAI MACsec object
//! to the MDIO words; `tests/e2e.rs` holds that path to the C++ one (`sai_macsec.cpp` +
//! `sai_extmacsec.cpp` on the fake backend) access by access.
//!
//! What it adds to the two crates, and nothing else:
//!
//! - **Ports**: the table names a PHY by the first lane of its port; [`PhySeqRunner::new`] maps
//!   the switch's ports to PHY address words the way `ext_macsec_attach` does (the last port
//!   whose first lane matches). A table port no switch port starts at is left out here; the
//!   model's [`sai_macsec::Macsec::attach`] refuses such a table.
//! - **Parameters**: `sai_macsec::Params` (by name) become `phy_seq::Params` (by [`phy_seq::ParamName`])
//!   byte for byte, from slice to slice: the bytes go from one wiping store straight into the
//!   other, which is dropped (and wiped) when the run ends. A name no table can use is left
//!   out, bytes beyond what a table can index are cut ([`phy_seq::Params::set_cut`]), as the C++
//!   engines do (they keep them in their map, unread).
//! - **Errors**: an [`phy_seq::ExecError`] becomes a [`SeqError`] with the C++ status (a missing
//!   parameter `INVALID_PARAMETER`, a fault `FAILURE`, a bus error the bus's own status) and the
//!   C++ engine's message (`phy-seq` words it byte for byte when the bus error displays as its
//!   status number, which [`BusStatus`] does).
//! - **The inter-frame gap** is not the PHY's: [`IfgSink`] (the port code of the SAI).
//!
//! # Locking
//!
//! [`SeqRunner::run`] must not interleave with other users of the bus (the C++ runs under the
//! external PHY table's sequence lock). That is the [`phy_seq::Bus`] implementation's: a bus shared
//! with a link poller takes the lock both use for the whole run, e.g. in [`phy_seq::Bus::begin`]
//! of the first operation or around the runner call.
//!
//! # Differences from the C++ (`sai_macsec.cpp` `run()` + `ext_macsec_exec`)
//!
//! - A bus status that `sai_core::Error` cannot express (`SAI_STATUS_NO_MEMORY`,
//!   `SAI_STATUS_INVALID_PORT_NUMBER`: the SDK's `BCM_E_MEMORY` / `BCM_E_PORT`) fails the call with
//!   `SAI_STATUS_FAILURE`; the message still carries the number.
//! - The results of a run that fails are not delivered (phy-seq's rule). The C++ `extmacsec run`
//!   prints the words read before the failure; the model's [`sai_macsec::Macsec::diag_run`] prints
//!   none.
#![forbid(unsafe_code)]
#![allow(
    clippy::doc_markdown,
    reason = "MACsec, MACSEC_PORT and the table's vocabulary are prose here, as in sai-macsec"
)]

use std::collections::BTreeMap;
use std::fmt;

pub mod native;

use phy_seq::{Bus, Dest, ExecError, ExecErrorKind, MacsecTable, OpKind, ParamName};
use sai_core::Error;
use sai_macsec::{CounterMode, EngineAddr, PortKey, Results, SeqError, SeqRunner, SwitchPort, Table};

/// A SAI status a bus master reports for a failed access: what the C++ backends'
/// `mdio_c45_read` / `mdio_c45_write` return. Displays as the number (the C++ message prints it
/// with `%d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusStatus(pub i32);

impl fmt::Display for BusStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl BusStatus {
    /// The status as an error of the SAI call (see [`status_error`]).
    pub fn error(self) -> Error {
        status_error(self.0)
    }
}

/// Every status `sai_core::Error` has a variant for (the attribute ranges aside).
const PLAIN: [Error; 14] = [
    Error::Failure,
    Error::NotSupported,
    Error::NotImplemented,
    Error::InsufficientResources,
    Error::InvalidParameter,
    Error::ItemAlreadyExists,
    Error::ItemNotFound,
    Error::InvalidVlanId,
    Error::Uninitialized,
    Error::TableFull,
    Error::MandatoryAttributeMissing,
    Error::ObjectInUse,
    Error::InvalidObjectType,
    Error::InvalidObjectId,
];

/// The error for a SAI status number; `FAILURE` for one `sai_core::Error` has no variant for
/// (and for `SUCCESS`, which is no error: a bus must not report it as one).
pub fn status_error(status: i32) -> Error {
    PLAIN
        .into_iter()
        .find(|e| e.status() == status)
        .unwrap_or(Error::Failure)
}

/// Where the model's inter-frame gap changes go: the ASIC MAC of the port, which is not on
/// the PHY's bus (`port_ifg_apply` of the C++ port code). See
/// [`SeqRunner::set_ifg`] for the contract.
pub trait IfgSink {
    /// The port has (`macsec_bits` != 0) or no longer has (0) a MACSEC_PORT object.
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError>;
}

impl<F: FnMut(PortKey, u32) -> Result<(), SeqError>> IfgSink for F {
    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError> {
        self(port, macsec_bits)
    }
}

/// An [`IfgSink`] for a board whose table has no `ifg-bits` (the model then always asks for 0
/// and nothing is to be done).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoIfg;

impl IfgSink for NoIfg {
    fn set_ifg(&mut self, _port: PortKey, _macsec_bits: u32) -> Result<(), SeqError> {
        Ok(())
    }
}

/// What the model needs to know of a parsed table besides its sequences
/// (counters, `ifg-bits`, ports).
pub fn model_table(table: &MacsecTable) -> Table {
    Table {
        counters: match table.counters() {
            phy_seq::CounterMode::ClearOnRead => CounterMode::ClearOnRead,
            phy_seq::CounterMode::Absolute => CounterMode::Absolute,
        },
        ifg_bits: u32::from(table.ifg_bits()),
        engines: table
            .ports()
            .map(|(lane, phy)| EngineAddr {
                lane,
                phy: u32::from(phy),
            })
            .collect(),
    }
}

/// The parameters of one run for the executor: every name a table can use, byte for byte.
/// The result wipes its bytes when dropped.
pub fn phy_params(params: &sai_macsec::Params) -> phy_seq::Params {
    let mut out = phy_seq::Params::new();
    for name in params.names() {
        if let (Some(param), Some(bytes)) = (ParamName::from_name(name.as_bytes()), params.get(name)) {
            out.set_cut(param, bytes);
        }
    }
    out
}

/// The executor's results in the model's terms (16-bit words, least significant first).
pub fn model_results(results: &phy_seq::Results) -> Results {
    let mut out = Results::new();
    for (name, words) in results.iter() {
        for (index, word) in words.iter().enumerate() {
            out.set_word(name.as_str(), index, *word);
        }
    }
    out
}

/// A failed run in the model's terms: the status the SAI call fails with and the C++
/// engine's message (no parameter byte: phy-seq's messages hold none).
pub fn seq_error<E: fmt::Display>(error: &ExecError<E>, status_of: impl FnOnce(&E) -> Error) -> SeqError {
    let status = match &error.kind {
        ExecErrorKind::MissingParam(_) => Error::InvalidParameter,
        ExecErrorKind::Bus { error, .. } => status_of(error),
        ExecErrorKind::Mismatch { .. } => Error::Failure,
    };
    SeqError {
        status,
        why: error.to_string(),
    }
}

/// The board's external MACsec table, run by `phy-seq` on bus `B`; the gap goes to `I`.
pub struct PhySeqRunner<B, I = NoIfg> {
    table: MacsecTable,
    names: Vec<String>,
    phy_of: BTreeMap<PortKey, u16>,
    bus: B,
    ifg: I,
}

impl<B, I> fmt::Debug for PhySeqRunner<B, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhySeqRunner")
            .field("sequences", &self.names.len())
            .field("phy_of", &self.phy_of)
            .finish_non_exhaustive()
    }
}

impl<B, I> PhySeqRunner<B, I> {
    /// A runner for the switch's `ports`: each table port is the last switch port whose first
    /// lane is the table's (`ext_macsec_attach`).
    pub fn new(table: MacsecTable, ports: &[SwitchPort], bus: B, ifg: I) -> Self {
        let phy_of = table
            .ports()
            .filter_map(|(lane, phy)| {
                let port = ports.iter().rfind(|p| p.lanes.first() == Some(&lane))?;
                Some((port.key, phy))
            })
            .collect();
        let names = table.sequences().map(|(name, _)| name.to_owned()).collect();
        Self {
            table,
            names,
            phy_of,
            bus,
            ifg,
        }
    }

    /// The table.
    pub fn table(&self) -> &MacsecTable {
        &self.table
    }

    /// The PHY address word of a port, `None` for a port without a MACsec PHY.
    pub fn phy_of(&self, port: PortKey) -> Option<u16> {
        self.phy_of.get(&port).copied()
    }

    /// The bus.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// The bus, to change it between runs (tests: a register the chip would set).
    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    /// The gap sink.
    pub fn ifg(&self) -> &I {
        &self.ifg
    }
}

impl<B, I> SeqRunner for PhySeqRunner<B, I>
where
    B: Bus<Error = BusStatus>,
    I: IfgSink,
{
    fn sequence_names(&self) -> Vec<String> {
        self.names.clone()
    }

    fn has(&self, name: &str) -> bool {
        self.table.sequence(name).is_some()
    }

    fn result_names(&self, name: &str) -> Vec<String> {
        self.table.sequence(name).map_or_else(Vec::new, |seq| {
            seq.ops()
                .iter()
                .filter_map(|op| match &op.kind {
                    OpKind::Keep {
                        dest: Dest::Result { name, .. },
                        ..
                    } => Some(name.as_str().to_owned()),
                    _ => None,
                })
                .collect()
        })
    }

    fn run(&mut self, port: PortKey, name: &str, params: &sai_macsec::Params) -> Result<Results, SeqError> {
        let Some(phy) = self.phy_of(port) else {
            return Err(SeqError {
                status: Error::NotSupported,
                why: format!("port {} has no MACsec PHY in the table", port.0),
            });
        };
        let Some(seq) = self.table.sequence(name) else {
            return Err(SeqError {
                status: Error::NotSupported,
                why: format!("the external MACsec table has no sequence {name}"),
            });
        };
        let par = phy_params(params);
        let outcome = seq.run(&mut self.bus, phy, &par);
        drop(par); // wiped
        outcome
            .map(|results| model_results(&results))
            .map_err(|e| seq_error(&e, |status| status.error()))
    }

    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError> {
        self.ifg.set_ifg(port, macsec_bits)
    }
}

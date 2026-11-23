//! The switch chip's die temperature monitors: methods of [`Sdk`] and [`crate::Attached`], and
//! [`TempReader`], a cloneable handle for a publisher thread.

use std::ffi::c_int;
use std::sync::{Arc, Mutex, PoisonError};

use crate::attach::Attached;
use crate::error::{Error, check};
use crate::glue::Glue;
use crate::unit::{Core, Live, Sdk};

/// The most monitors a read returns (the glue's own limit).
pub const MAX_MONITORS: usize = 16;

/// One read of the monitors, in tenths of a degree C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Temperatures {
    current: [i32; MAX_MONITORS],
    peak: [i32; MAX_MONITORS],
    count: usize,
}

impl Temperatures {
    /// The current value of every monitor.
    pub fn current(&self) -> &[i32] {
        self.current.get(..self.count).unwrap_or(&self.current)
    }

    /// Per monitor, the highest value since the previous read.
    pub fn peak(&self) -> &[i32] {
        self.peak.get(..self.count).unwrap_or(&self.peak)
    }
}

fn read(glue: &Glue, unit: c_int, mu: &Mutex<()>) -> Result<Temperatures, Error> {
    let _one_at_a_time = mu.lock().unwrap_or_else(PoisonError::into_inner);
    let mut current: [c_int; MAX_MONITORS] = [0; MAX_MONITORS];
    let mut peak: [c_int; MAX_MONITORS] = [0; MAX_MONITORS];
    let mut count: c_int = 0;
    check(glue.temperatures(unit, &mut current, &mut peak, &mut count))?;
    Ok(Temperatures {
        current,
        peak,
        count: usize::try_from(count).unwrap_or(0).min(MAX_MONITORS),
    })
}

fn reader(core: &Core) -> TempReader {
    TempReader {
        glue: core.glue.clone(),
        unit: core.unit,
        live: Arc::clone(&core.live),
        mu: Arc::clone(&core.temp_mu),
    }
}

impl Sdk {
    /// Reads the monitors (`ob_sdk_temperatures`). Reading resets the hardware peak; the first
    /// read after a chip reset returns a garbage peak.
    pub fn temperatures(&mut self) -> Result<Temperatures, Error> {
        read(&self.core.glue, self.core.unit, &self.core.temp_mu)
    }

    /// A handle for reading the monitors from another thread: [`TempReader`].
    pub fn temp_reader(&self) -> TempReader {
        reader(&self.core)
    }
}

impl Attached {
    /// Reads the monitors (through the S-channel: Tomahawk family; [`crate::Code::Unavail`] on
    /// other chips). As [`Sdk::temperatures`].
    pub fn temperatures(&mut self) -> Result<Temperatures, Error> {
        read(&self.core.glue, self.core.unit, &self.core.temp_mu)
    }

    /// A handle for reading the monitors from another thread: [`TempReader`].
    pub fn temp_reader(&self) -> TempReader {
        reader(&self.core)
    }
}

/// The temperature monitors as a handle of their own (`Clone + Send + Sync`, reads through
/// `&self`): what a publisher thread uses without the switch (`sai-core`'s `TempReader` closure).
/// Reads from all handles of one owner and the owner itself run one at a time. Works while the
/// owner is alive; afterwards [`crate::Code::Init`] without reaching the glue.
#[derive(Debug, Clone)]
pub struct TempReader {
    glue: Glue,
    unit: c_int,
    live: Arc<Live>,
    mu: Arc<Mutex<()>>,
}

impl TempReader {
    /// Reads the monitors, as [`Sdk::temperatures`].
    pub fn read(&self) -> Result<Temperatures, Error> {
        self.live
            .with(|| read(&self.glue, self.unit, &self.mu))
            .unwrap_or(Err(Error::INIT))
    }
}

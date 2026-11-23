//! The switch chip as the tool needs it: a bring-up for MDIO access, the MDIO bus, the die
//! temperature. Implemented over the SDK glue in [`crate::glue`]; tests implement it in Rust.

use std::ffi::OsStr;

use mdio_script::MdioBus;

/// One read of the die temperature monitors, tenths of a degree C.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Temperatures {
    /// Current value per monitor.
    pub current: Vec<i32>,
    /// Highest value since the previous read, per monitor.
    pub peak: Vec<i32>,
}

/// A chip brought up for MDIO access: it only exists after misc init succeeded.
pub trait Chip: MdioBus {
    /// Reads the monitors; the error is the SDK's text for its code.
    fn temperatures(&mut self) -> Result<Temperatures, String>;
}

/// Which failure class a failed bring-up is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenFailure {
    /// Another process holds the unit's SDK lock.
    Busy,
    /// The SDK config file is missing.
    Config,
    /// The MDIO clock or hold delay on the chip is not as configured.
    MdioClock,
    /// Anything else: attach, init soc, init misc.
    Init,
}

/// A bring-up that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenError {
    /// The class.
    pub failure: OpenFailure,
    /// The reason, one line.
    pub message: String,
    /// The SDK's text for its error code.
    pub code_text: String,
}

/// The way to a [`Chip`].
pub trait ChipOpener {
    /// Brings the SDK up on `unit` with the config file `config` for MDIO access: "init soc" +
    /// "init misc", nothing else. On success: the chip and one line about it and its MDIO clock.
    fn open(&mut self, unit: i32, config: &OsStr) -> Result<(Box<dyn Chip>, String), OpenError>;
}

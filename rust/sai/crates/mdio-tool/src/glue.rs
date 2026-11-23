//! [`Chip`] over the SDK glue (`sdk-glue`): `ob_sdk_init_mdio`, clause-45 access, temperatures.

use std::ffi::OsStr;
use std::path::Path;

use mdio_script::{BusError, Dev, MdioBus};
use sdk_glue::{Code, InitError, Mmd, Sdk};

use crate::chip::{Chip, ChipOpener, OpenError, OpenFailure, Temperatures};

/// Opens the SDK of this process through the glue that is linked in.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlueOpener;

struct GlueChip(Sdk);

impl ChipOpener for GlueOpener {
    fn open(&mut self, unit: i32, config: &OsStr) -> Result<(Box<dyn Chip>, String), OpenError> {
        match Sdk::init_mdio(unit, Path::new(config)) {
            Ok((sdk, line)) => Ok((Box::new(GlueChip(sdk)), line)),
            Err(InitError::Sdk { error, message }) => Err(OpenError {
                failure: match error.code() {
                    Code::Busy => OpenFailure::Busy,
                    Code::Config => OpenFailure::Config,
                    Code::MdioClock => OpenFailure::MdioClock,
                    _ => OpenFailure::Init,
                },
                message,
                code_text: error.message(),
            }),
            Err(other) => Err(OpenError {
                failure: OpenFailure::Init,
                message: other.to_string(),
                code_text: "not attempted".to_owned(),
            }),
        }
    }
}

fn mmd(dev: Dev) -> Result<Mmd, BusError> {
    Mmd::new(dev.get()).ok_or(BusError)
}

impl MdioBus for GlueChip {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.0
            .mdio_c45_read(u32::from(phy), mmd(dev)?, reg)
            .map_err(|_| BusError)
    }

    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError> {
        self.0
            .mdio_c45_write(u32::from(phy), mmd(dev)?, reg, val)
            .map_err(|_| BusError)
    }
}

impl Chip for GlueChip {
    fn temperatures(&mut self) -> Result<Temperatures, String> {
        self.0
            .temperatures()
            .map(|t| Temperatures {
                current: t.current().to_vec(),
                peak: t.peak().to_vec(),
            })
            .map_err(sdk_glue::Error::message)
    }
}

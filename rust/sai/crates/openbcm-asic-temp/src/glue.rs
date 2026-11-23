//! [`Attacher`] / [`Sensor`] over the SDK glue (`sdk-glue`): `ob_sdk_attach`, temperatures, the
//! core-PLL power-down. Which glue: the one this program links (feature `sdk-real`), or
//! `sdk-glue`'s Rust fake chip (feature `sdk-fake`, [`GlueAttacher::on`]).

use sdk_glue::{Attached, Code, InitError};

use crate::{AttachError, Attacher, Reading, Sensor};

/// Attaches through the glue: the process's own ([`GlueAttacher::process`]) or a Rust fake chip.
#[derive(Debug, Default)]
pub struct GlueAttacher {
    #[cfg(feature = "sdk-fake")]
    chip: Option<sdk_glue::rust_fake::Chip>,
}

impl GlueAttacher {
    /// The glue this program links (`Attached::attach`).
    pub fn process() -> Self {
        Self::default()
    }

    /// A Rust fake chip (its scenario: `sdk_glue::rust_fake::Chip::load`).
    #[cfg(feature = "sdk-fake")]
    pub fn on(chip: sdk_glue::rust_fake::Chip) -> Self {
        Self { chip: Some(chip) }
    }

    fn attached(&self, unit: i32) -> Result<(Attached, String), InitError> {
        #[cfg(feature = "sdk-fake")]
        if let Some(chip) = &self.chip {
            return Attached::attach_on(chip, unit);
        }
        Attached::attach(unit)
    }
}

struct GlueSensor(Attached);

impl Attacher for GlueAttacher {
    fn attach(&mut self, unit: i32) -> Result<(Box<dyn Sensor>, String), AttachError> {
        match self.attached(unit) {
            Ok((attached, line)) => Ok((Box::new(GlueSensor(attached)), line)),
            Err(InitError::Sdk { error, message }) => Err(AttachError {
                busy: error.code() == Code::Busy,
                message,
                code_text: error.message(),
            }),
            Err(other) => Err(AttachError {
                busy: false,
                message: other.to_string(),
                code_text: "not attempted".to_owned(),
            }),
        }
    }
}

impl Sensor for GlueSensor {
    fn read(&mut self) -> Result<Reading, String> {
        self.0
            .temperatures()
            .map(|t| Reading {
                current: t.current().to_vec(),
                peak: t.peak().to_vec(),
            })
            .map_err(sdk_glue::Error::message)
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.0.overtemp_shutdown().map_err(sdk_glue::Error::message)
    }
}

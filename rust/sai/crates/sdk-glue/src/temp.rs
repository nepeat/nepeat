//! The switch chip's die temperature monitors.

use std::ffi::c_int;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::unit::Sdk;

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

impl Sdk {
    /// Reads the monitors (`ob_sdk_temperatures`). Reading resets the hardware peak; the first
    /// read after a chip reset returns a garbage peak.
    pub fn temperatures(&mut self) -> Result<Temperatures, Error> {
        let mut current: [c_int; MAX_MONITORS] = [0; MAX_MONITORS];
        let mut peak: [c_int; MAX_MONITORS] = [0; MAX_MONITORS];
        let mut count: c_int = 0;
        let max = c_int::try_from(MAX_MONITORS).unwrap_or(0);
        // SAFETY: self exists, so the SDK is up on this unit, and &mut self keeps a second
        // call of this handle out. current and peak are live locals of MAX_MONITORS ints each
        // and max says so: the glue writes at most max elements to each, one int to count, and
        // keeps none of the pointers.
        let rv = unsafe {
            sys::ob_sdk_temperatures(
                self.raw_unit(),
                current.as_mut_ptr(),
                peak.as_mut_ptr(),
                max,
                &raw mut count,
            )
        };
        check(rv)?;
        Ok(Temperatures {
            current,
            peak,
            count: usize::try_from(count).unwrap_or(0).min(MAX_MONITORS),
        })
    }
}

//! The glue's return codes (`OB_E_*`: the SDK's `BCM_E_*` plus one of the glue's own).

#[cfg(all(ob_glue_linked, not(miri)))]
use std::ffi::CStr;
use std::ffi::c_int;
use std::fmt;

use sdk_glue_sys as sys;

/// What an error code means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    /// `OB_E_INTERNAL`
    Internal,
    /// `OB_E_MEMORY`
    Memory,
    /// `OB_E_UNIT`: no such (attached) unit.
    Unit,
    /// `OB_E_PARAM`
    Param,
    /// `OB_E_EMPTY`
    Empty,
    /// `OB_E_FULL`
    Full,
    /// `OB_E_NOT_FOUND`
    NotFound,
    /// `OB_E_EXISTS`
    Exists,
    /// `OB_E_TIMEOUT`
    Timeout,
    /// `OB_E_BUSY`: from a bring-up, another process holds the unit's SDK lock.
    Busy,
    /// `OB_E_FAIL`
    Fail,
    /// `OB_E_DISABLED`
    Disabled,
    /// `OB_E_BADID`
    BadId,
    /// `OB_E_RESOURCE`
    Resource,
    /// `OB_E_CONFIG`: from a bring-up, the SDK config file is missing.
    Config,
    /// `OB_E_UNAVAIL`
    Unavail,
    /// `OB_E_INIT`: not initialised.
    Init,
    /// `OB_E_PORT`
    Port,
    /// `OB_E_MDIO_CLOCK` (not an SDK code): after misc init the chip's external MDIO clock
    /// divisor or hold delay is not what the config asks for; no MDIO frame was sent.
    MdioClock,
    /// A negative code this crate has no name for.
    Other,
}

/// A failed glue call: the negative code it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Error {
    raw: c_int,
}

impl Error {
    /// `OB_E_INTERNAL`: what this crate reports for an answer of the glue that cannot be.
    pub(crate) const INTERNAL: Self = Self {
        raw: sys::OB_E_INTERNAL,
    };
    /// `OB_E_PARAM`: an argument this crate refuses before calling the glue.
    pub(crate) const PARAM: Self = Self { raw: sys::OB_E_PARAM };
    /// `OB_E_INIT`: a handle whose owner is gone.
    pub(crate) const INIT: Self = Self { raw: sys::OB_E_INIT };
    /// `OB_E_PORT`: a port number outside the glue's `int`.
    pub(crate) const PORT: Self = Self { raw: sys::OB_E_PORT };

    /// `None` for a code that is not an error (0 and above).
    pub const fn from_raw(rv: i32) -> Option<Self> {
        if rv < 0 { Some(Self { raw: rv }) } else { None }
    }

    /// The code as the glue returned it (negative).
    pub const fn raw(self) -> i32 {
        self.raw
    }

    /// What the code means.
    pub const fn code(self) -> Code {
        match self.raw {
            sys::OB_E_INTERNAL => Code::Internal,
            sys::OB_E_MEMORY => Code::Memory,
            sys::OB_E_UNIT => Code::Unit,
            sys::OB_E_PARAM => Code::Param,
            sys::OB_E_EMPTY => Code::Empty,
            sys::OB_E_FULL => Code::Full,
            sys::OB_E_NOT_FOUND => Code::NotFound,
            sys::OB_E_EXISTS => Code::Exists,
            sys::OB_E_TIMEOUT => Code::Timeout,
            sys::OB_E_BUSY => Code::Busy,
            sys::OB_E_FAIL => Code::Fail,
            sys::OB_E_DISABLED => Code::Disabled,
            sys::OB_E_BADID => Code::BadId,
            sys::OB_E_RESOURCE => Code::Resource,
            sys::OB_E_CONFIG => Code::Config,
            sys::OB_E_UNAVAIL => Code::Unavail,
            sys::OB_E_INIT => Code::Init,
            sys::OB_E_PORT => Code::Port,
            sys::OB_E_MDIO_CLOCK => Code::MdioClock,
            _ => Code::Other,
        }
    }

    /// The glue's text for the code (`ob_sdk_errmsg`: the SDK's `bcm_errmsg`). Needs no
    /// initialised SDK. In a build that links no C glue (only the Rust fake), and under Miri,
    /// the same texts from [`sdk_text`] (a copy of the SDK's table).
    pub fn message(self) -> String {
        #[cfg(any(not(ob_glue_linked), miri))]
        {
            sdk_text(self.raw).to_owned()
        }
        #[cfg(all(ob_glue_linked, not(miri)))]
        {
            self.glue_message()
        }
    }

    #[cfg(all(ob_glue_linked, not(miri)))]
    fn glue_message(self) -> String {
        // SAFETY: ob_sdk_errmsg takes any int, reads no state and needs no initialised SDK: it
        // returns the address of a string literal (sdk_glue.c: one literal of its own, or
        // bcm_errmsg, which indexes the SDK's constant message table with a clamped index).
        let text = unsafe { sys::ob_sdk_errmsg(self.raw) };
        if text.is_null() {
            return format!("error {}", self.raw);
        }
        // SAFETY: text is not null (checked above) and, by the contract just stated, points to a
        // NUL-terminated string literal: valid for reads up to its NUL for the whole life of
        // the process and never written.
        let text = unsafe { CStr::from_ptr(text) };
        text.to_string_lossy().into_owned()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

/// The SDK's message for a code (`_shr_errmsg`; the Rust fake's `ob_sdk_errmsg`), and the
/// glue's own for `OB_E_MDIO_CLOCK`. Never compared with the real glue's `ob_sdk_errmsg` offline;
/// mdio-tool's differential compares the texts the tool prints with the C tool's (on a C stub
/// holding the same table).
pub fn sdk_text(rv: i32) -> &'static str {
    const TEXT: [&str; 20] = [
        "Ok",
        "Internal error",
        "Out of memory",
        "Invalid unit",
        "Invalid parameter",
        "Table empty",
        "Table full",
        "Entry not found",
        "Entry exists",
        "Operation timed out",
        "Operation still running",
        "Operation failed",
        "Operation disabled",
        "Invalid identifier",
        "No resources for operation",
        "Invalid configuration",
        "Feature unavailable",
        "Feature not initialized",
        "Invalid port",
        "Unknown error",
    ];
    if rv == sys::OB_E_MDIO_CLOCK {
        return "MDIO clock setup not as configured";
    }
    let index = if rv <= 0 && rv > -19 {
        usize::try_from(-rv).unwrap_or(19)
    } else {
        19
    };
    TEXT.get(index).copied().unwrap_or("Unknown error")
}

/// `Ok(rv)` for a non-negative return code.
pub(crate) fn check(rv: c_int) -> Result<c_int, Error> {
    Error::from_raw(rv).map_or(Ok(rv), Err)
}

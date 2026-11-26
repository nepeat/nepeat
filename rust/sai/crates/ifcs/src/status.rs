//! IFCS status codes (`ifcs_status_t`: the low 5 bits are the `ifcs_reason`) and their SAI
//! errors, exactly as `tl_core.cpp` maps and names them.
#![forbid(unsafe_code)]

use ifcs_sys as sys;
use sai_core::error::Error;

/// A status an IFCS function returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status(pub sys::ifcs_status_t);

/// What the C++ `Core::reason` prints for each reason (`"?"` beyond 16).
const NAMES: [&str; 17] = [
    "SUCCESS",
    "UNINIT",
    "BUSY",
    "CONFIG",
    "MEMORY",
    "PARAM",
    "EXIST",
    "MULTI_EXIST",
    "ENTRY_REPLACED",
    "LENGTH_MISMATCH",
    "NOTFOUND",
    "INVAL",
    "UNSUPPORTED",
    "RESOURCE_FULL",
    "TIMEOUT",
    "DEVICE_ACCESS_ERROR",
    "DRIVER_ERROR",
];

/// A reason as a number: `IFCS_SUCCESS` .. `IFCS_DRIVER_ERROR` (the anonymous enum's values).
pub type Reason = u32;

impl Status {
    /// `IFCS_SUCCESS`.
    pub const SUCCESS: Self = Self(0);

    /// `IFCS_STATUS_REASON(rc)`: the low 5 bits.
    pub const fn reason(self) -> Reason {
        self.0.cast_unsigned() & 31
    }

    /// The reason is `IFCS_SUCCESS`.
    pub const fn is_success(self) -> bool {
        self.reason() == sys::IFCS_SUCCESS
    }

    /// The reason's name as the C++ logs it (`Core::reason`).
    pub fn name(self) -> &'static str {
        reason_name(self.reason())
    }

    /// `Ok` on success, else the SAI error of the reason ([`sai_error`]).
    pub fn ok(self) -> Result<(), Error> {
        if self.is_success() {
            Ok(())
        } else {
            Err(sai_error(self.reason()))
        }
    }
}

/// The C++ `Core::reason` of a reason number.
pub fn reason_name(r: Reason) -> &'static str {
    usize::try_from(r)
        .ok()
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("?")
}

/// `tl_core.cpp` `map_reason` for a failed reason (`IFCS_SUCCESS` is not an error: it maps to
/// `FAILURE` here only if asked, which no caller does).
pub fn sai_error(r: Reason) -> Error {
    match r {
        sys::IFCS_UNINIT => Error::Uninitialized,
        sys::IFCS_BUSY => Error::ObjectInUse,
        sys::IFCS_MEMORY => Error::NoMemory,
        sys::IFCS_PARAM | sys::IFCS_INVAL | sys::IFCS_CONFIG | sys::IFCS_LENGTH_MISMATCH => Error::InvalidParameter,
        sys::IFCS_EXIST | sys::IFCS_MULTI_EXIST => Error::ItemAlreadyExists,
        sys::IFCS_NOTFOUND => Error::ItemNotFound,
        sys::IFCS_UNSUPPORTED => Error::NotSupported,
        sys::IFCS_RESOURCE_FULL => Error::TableFull,
        _ => Error::Failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `map_reason` of `tl_core`.cpp, every reason (the C++ switch transcribed: SUCCESS is handled
    /// by `Status::ok`).
    #[test]
    fn reasons_map_as_tl_core() {
        let want = [
            (1, Error::Uninitialized),
            (2, Error::ObjectInUse),
            (3, Error::InvalidParameter),
            (4, Error::NoMemory),
            (5, Error::InvalidParameter),
            (6, Error::ItemAlreadyExists),
            (7, Error::ItemAlreadyExists),
            (8, Error::Failure), // ENTRY_REPLACED: the C++ default
            (9, Error::InvalidParameter),
            (10, Error::ItemNotFound),
            (11, Error::InvalidParameter),
            (12, Error::NotSupported),
            (13, Error::TableFull),
            (14, Error::Failure),
            (15, Error::Failure),
            (16, Error::Failure),
            (31, Error::Failure),
        ];
        for (r, e) in want {
            assert_eq!(sai_error(r), e, "reason {r}");
            assert_eq!(Status(r.cast_signed()).ok(), Err(e));
        }
        assert_eq!(Status::SUCCESS.ok(), Ok(()));
        // the low 5 bits only (libifcs puts more in the upper bits)
        assert_eq!(Status(0x40 | 0xa).reason(), 10);
        assert!(Status(0x20).is_success());
        assert_eq!(Status(10).name(), "NOTFOUND");
        assert_eq!(reason_name(16), "DRIVER_ERROR");
        assert_eq!(reason_name(17), "?");
    }
}

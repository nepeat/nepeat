//! `OB_E_*` (the SDK's `BCM_E_*`) to SAI errors: `map_rv` of the C++ `openbcm_backend.cpp`,
//! code for code.
//!
//! | glue code | SAI status |
//! |---|---|
//! | `MEMORY` | `NO_MEMORY` |
//! | `UNIT`, `PARAM`, `CONFIG` | `INVALID_PARAMETER` |
//! | `EMPTY`, `NOT_FOUND` | `ITEM_NOT_FOUND` |
//! | `FULL` | `TABLE_FULL` |
//! | `EXISTS` | `ITEM_ALREADY_EXISTS` |
//! | `BUSY` | `OBJECT_IN_USE` |
//! | `BADID` | `INVALID_OBJECT_ID` |
//! | `RESOURCE` | `INSUFFICIENT_RESOURCES` |
//! | `UNAVAIL` | `NOT_SUPPORTED` |
//! | `INIT` | `UNINITIALIZED` |
//! | `PORT` | `INVALID_PORT_NUMBER` |
//! | anything else (`INTERNAL`, `TIMEOUT`, `FAIL`, `DISABLED`, `MDIO_CLOCK`, unnamed) | `FAILURE` |
//!
//! The test `tests/parity.rs` reads the C++ table from the source and compares every code.
use sai_core::error::Error;
use sdk_glue::Code;

/// The SAI error for a failed glue call.
pub const fn sai_error(error: sdk_glue::Error) -> Error {
    match error.code() {
        Code::Memory => Error::NoMemory,
        Code::Unit | Code::Param | Code::Config => Error::InvalidParameter,
        Code::Empty | Code::NotFound => Error::ItemNotFound,
        Code::Full => Error::TableFull,
        Code::Exists => Error::ItemAlreadyExists,
        Code::Busy => Error::ObjectInUse,
        Code::BadId => Error::InvalidObjectId,
        Code::Resource => Error::InsufficientResources,
        Code::Unavail => Error::NotSupported,
        Code::Init => Error::Uninitialized,
        Code::Port => Error::InvalidPortNumber,
        Code::Internal | Code::Timeout | Code::Fail | Code::Disabled | Code::MdioClock | Code::Other => Error::Failure,
    }
}

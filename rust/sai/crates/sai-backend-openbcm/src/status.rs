//! `OB_E_*` (the SDK's `BCM_E_*`) to SAI errors, as `map_rv` of the C++
//! `openbcm_backend.cpp` does.
//!
//! | glue code | C++ | here |
//! |---|---|---|
//! | `MEMORY` | `NO_MEMORY` | `INSUFFICIENT_RESOURCES` (`sai_core::Error` has no `NO_MEMORY`) |
//! | `UNIT`, `PARAM`, `CONFIG` | `INVALID_PARAMETER` | the same |
//! | `EMPTY`, `NOT_FOUND` | `ITEM_NOT_FOUND` | the same |
//! | `FULL` | `TABLE_FULL` | the same |
//! | `EXISTS` | `ITEM_ALREADY_EXISTS` | the same |
//! | `BUSY` | `OBJECT_IN_USE` | the same |
//! | `BADID` | `INVALID_OBJECT_ID` | the same |
//! | `RESOURCE` | `INSUFFICIENT_RESOURCES` | the same |
//! | `UNAVAIL` | `NOT_SUPPORTED` | the same |
//! | `INIT` | `UNINITIALIZED` | the same |
//! | `PORT` | `INVALID_PORT_NUMBER` | `INVALID_PARAMETER` (`sai_core::Error` has no `INVALID_PORT_NUMBER`; what the virtual backend answers for an unknown port) |
//! | anything else | `FAILURE` | the same |
//!
//! The test `tests/parity.rs` reads the C++ table from the source and compares.
use sai_core::error::Error;
use sdk_glue::Code;

/// The SAI error for a failed glue call.
pub fn sai_error(error: sdk_glue::Error) -> Error {
    match error.code() {
        Code::Memory | Code::Resource => Error::InsufficientResources,
        Code::Unit | Code::Param | Code::Config | Code::Port => Error::InvalidParameter,
        Code::Empty | Code::NotFound => Error::ItemNotFound,
        Code::Full => Error::TableFull,
        Code::Exists => Error::ItemAlreadyExists,
        Code::Busy => Error::ObjectInUse,
        Code::BadId => Error::InvalidObjectId,
        Code::Unavail => Error::NotSupported,
        Code::Init => Error::Uninitialized,
        Code::Internal | Code::Timeout | Code::Fail | Code::Disabled | Code::MdioClock | Code::Other => Error::Failure,
    }
}

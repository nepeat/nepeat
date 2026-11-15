//! A small string error type with context, so every failure says where it came from.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Attach context to any displayable error.
pub trait Context<T> {
    fn ctx<S: Into<String>>(self, what: impl FnOnce() -> S) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn ctx<S: Into<String>>(self, what: impl FnOnce() -> S) -> Result<T> {
        self.map_err(|e| Error(format!("{}: {e}", what().into())))
    }
}

#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::error::Error(format!($($t)*))) };
}

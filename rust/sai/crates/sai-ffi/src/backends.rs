//! Which chip the library drives: chosen by `SAI_RS_BACKEND` in the SAI profile when the
//! switch is created. Nothing is chosen implicitly: a library that answers "success" without
//! programming a chip, or one that takes over a chip, must be what the operator asked for.
//!
//! - `virtual`: the in-memory chip of `sai-core` (always built in).
//! - `openbcm`: the Broadcom SDK through the C glue (`sai-backend-openbcm`), only in a
//!   library built with the cargo feature `openbcm`.
use sai_core::backend::Backend;
use sai_core::virt::VirtualBackend;

/// The profile key that names the backend.
pub(crate) const BACKEND_KEY: &str = "SAI_RS_BACKEND";

/// The backends this build of the library has, for the error message.
pub(crate) const AVAILABLE: &str = if cfg!(feature = "openbcm") {
    "\"virtual\", \"openbcm\""
} else {
    "\"virtual\""
};

/// Every profile key a backend of this build reads (the rim asks the SAI user for these).
pub(crate) fn profile_keys() -> Vec<&'static str> {
    let mut keys = vec![BACKEND_KEY];
    keys.extend(sai_core::virt::PROFILE_KEYS);
    #[cfg(feature = "openbcm")]
    keys.extend(sai_backend_openbcm::PROFILE_KEYS);
    keys
}

/// A new, uninitialised backend of that name. Creating one touches nothing: the chip is
/// brought up by `Backend::init` during switch creation.
pub(crate) fn select(name: Option<&str>) -> Option<Box<dyn Backend>> {
    match name {
        Some("virtual") => Some(Box::new(VirtualBackend::new())),
        #[cfg(feature = "openbcm")]
        Some("openbcm") => Some(Box::new(sai_backend_openbcm::OpenbcmBackend::new())),
        _ => None,
    }
}

/// The backend the API holds before a switch exists (never initialised).
pub(crate) fn placeholder() -> Box<dyn Backend> {
    Box::new(VirtualBackend::new())
}

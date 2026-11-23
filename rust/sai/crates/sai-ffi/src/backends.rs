//! Which chip the library drives: chosen when the switch is created by `SAI_RS_BACKEND`, else
//! `SAI_OPENBCM_BACKEND` (the C++ key), as the C++ `pick_backend` chooses
//! ([`sai_core::keys::backend_choice`]): `fake` / `virtual` the in-memory chip, `openbcm` the
//! SDK; no key is `openbcm` in a library that has it and an error in one that does not, so the
//! in-memory chip is never chosen implicitly.
//!
//! - `virtual`: the in-memory chip of `sai-core` (always built in).
//! - `openbcm`: the Broadcom SDK through the C glue (`sai-backend-openbcm`), only in a
//!   library built with the cargo feature `openbcm`.
use sai_core::backend::Backend;
use sai_core::keys::BackendChoice;
use sai_core::virt::VirtualBackend;

/// Every profile key the library asks for one by one besides those the SAI user enumerates:
/// the C++ list ([`sai_core::keys::EXPLICIT`]) and every key a backend of this build reads.
pub(crate) fn profile_keys() -> Vec<&'static str> {
    let mut keys = sai_core::keys::EXPLICIT.to_vec();
    keys.push(sai_diag::SOCKET_KEY); // the diag socket (not a backend's)
    keys.extend(sai_core::extphy::PROFILE_KEYS); // the external PHY / MACsec tables and the clear-window keys
    keys.extend(sai_core::virt::PROFILE_KEYS);
    #[cfg(feature = "openbcm")]
    keys.extend(sai_backend_openbcm::PROFILE_KEYS);
    keys
}

/// A new, uninitialised backend of that kind; `None` for `openbcm` in a library built without
/// it. Creating one touches nothing: the chip is brought up by `Backend::init` during switch
/// creation.
pub(crate) fn select(choice: BackendChoice) -> Option<Box<dyn Backend>> {
    match choice {
        BackendChoice::Virtual => Some(Box::new(VirtualBackend::new())),
        #[cfg(feature = "openbcm")]
        BackendChoice::Openbcm => Some(Box::new(sai_backend_openbcm::OpenbcmBackend::new())),
        #[cfg(not(feature = "openbcm"))]
        BackendChoice::Openbcm => None,
    }
}

/// The backend the API holds before a switch exists (never initialised).
pub(crate) fn placeholder() -> Box<dyn Backend> {
    Box::new(VirtualBackend::new())
}

//! Which chip the library drives: chosen when the switch is created by `SAI_RS_BACKEND`, else
//! `SAI_OPENBCM_BACKEND` (the C++ key), as the C++ `pick_backend` chooses
//! ([`sai_core::keys::backend_choice`]): `fake` / `virtual` the in-memory chip, `openbcm` the
//! SDK; no key is `openbcm` in a library that has it and an error in one that does not, so the
//! in-memory chip is never chosen implicitly.
//!
//! - `virtual`: the in-memory chip of `sai-core` (always built in).
//! - `openbcm`: the Broadcom SDK through the C glue (`sai-backend-openbcm`), only in a
//!   library built with the cargo feature `openbcm`.
//! - With the cargo feature `teralynx` instead, the hardware backend is the Teralynx 7's
//!   (`sai-backend-teralynx`, the CS8210-32X): `SAI_OPENBCM_BACKEND=openbcm` (what the CS8210's
//!   board profile sets, as for the C++ library's "openbcm" slot), `teralynx` and no key all
//!   select it.
use sai_core::backend::{Backend, Profile};
use sai_core::keys::{self, BackendChoice, ChoiceError};
use sai_core::virt::VirtualBackend;

#[cfg(all(feature = "openbcm", feature = "teralynx"))]
compile_error!("sai-ffi: the features \"openbcm\" and \"teralynx\" are two hardware backends; a libsai has one");

/// Whether this library has a hardware backend (then no key selects it).
pub(crate) const HAS_HARDWARE: bool = cfg!(any(feature = "openbcm", feature = "teralynx"));

/// The backend the profile asks for (`keys::backend_choice`); a teralynx library also takes
/// `teralynx` for its hardware backend. Inlined: without the feature it is the call the rim always
/// made (`keys::backend_choice(profile, cfg!(feature = "openbcm"))`).
#[allow(
    clippy::inline_always,
    reason = "keeps a build without the feature as it was (AS77xx byte-identity)"
)]
#[inline(always)]
pub(crate) fn choice(profile: &Profile) -> Result<BackendChoice, ChoiceError> {
    #[cfg(feature = "teralynx")]
    if profile.get_any(&[keys::RS_BACKEND, keys::BACKEND]) == Some("teralynx") {
        return Ok(BackendChoice::Openbcm);
    }
    keys::backend_choice(profile, HAS_HARDWARE)
}

/// Every profile key the library asks for one by one besides those the SAI user enumerates:
/// the C++ list ([`sai_core::keys::EXPLICIT`]) and every key a backend of this build reads.
pub(crate) fn profile_keys() -> Vec<&'static str> {
    let mut keys = sai_core::keys::EXPLICIT.to_vec();
    keys.push(sai_diag::SOCKET_KEY); // the diag socket (not a backend's)
    keys.extend(sai_core::extphy::PROFILE_KEYS); // the external PHY / MACsec tables and the clear-window keys
    keys.extend(sai_core::virt::PROFILE_KEYS);
    #[cfg(feature = "openbcm")]
    keys.extend(sai_backend_openbcm::PROFILE_KEYS);
    #[cfg(feature = "teralynx")]
    keys.extend(sai_backend_teralynx::PROFILE_KEYS);
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
        #[cfg(feature = "teralynx")]
        BackendChoice::Openbcm => Some(Box::new(sai_backend_teralynx::TeralynxBackend::new())),
        #[cfg(not(any(feature = "openbcm", feature = "teralynx")))]
        BackendChoice::Openbcm => None,
    }
}

/// The backend the API holds before a switch exists (never initialised).
pub(crate) fn placeholder() -> Box<dyn Backend> {
    Box::new(VirtualBackend::new())
}

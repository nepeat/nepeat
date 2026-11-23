//! The full switch bring-up (`ob_sdk_init`: "init all" + the optional rc file) and the handle
//! it returns.

use std::ffi::{CString, c_char, c_int};
use std::ops::{Deref, DerefMut};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::unit::{InitError, Sdk};

/// Size of the buffer for the chip name (the C++ backend's `char b[64]`).
const HW_INFO_LEN: usize = 64;

/// The switch chip after the full bring-up: the BCM layer is up, so ports ([`crate::port`]),
/// VLANs ([`crate::vlan`]) and the link callback ([`crate::link`]) can be used. Derefs to the
/// [`Sdk`] it owns (MDIO, temperatures).
///
/// One per process at a time, `Send` but not `Sync`, as [`Sdk`]. Calls that change the chip
/// take `&mut self`; reads take `&self`, which cannot be shared between threads either.
///
/// Dropping it unregisters the link callback, drops the closure, and then shuts the BCM layer
/// down (`ob_sdk_shutdown`). As with the C++ backend the SDK itself stays attached and the
/// process keeps the unit's SDK lock until it ends; a new [`Switch::init`] re-runs "init all".
#[derive(Debug)]
pub struct Switch {
    sdk: Sdk,
    /// A link callback is registered with the SDK ([`crate::link`]).
    pub(crate) link_registered: bool,
}

fn c_path(path: &Path) -> Result<CString, InitError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| InitError::BadPath)
}

impl Switch {
    /// Brings the whole switch up (`ob_sdk_init`): SDK boot and attach, "init all" with the
    /// SDK config file `config`, then the diag-shell script `rc` if one is given, stale KNET
    /// interfaces removed, packet receive started, L3 enabled on the ports.
    /// **This resets the switch chip**; ports are enabled afterwards as "init all" leaves them.
    ///
    /// On success: the handle and the glue's summary line. On failure nothing is registered
    /// and no handle exists, but what the SDK did up to the failing step is not undone (there
    /// is no detach): the process may hold the unit's SDK lock and SDK threads until it ends.
    /// [`InitError::BadPath`]: `config` or `rc` holds a NUL byte; nothing was called.
    pub fn init(unit: i32, config: &Path, rc: Option<&Path>) -> Result<(Self, String), InitError> {
        let config = c_path(config)?;
        // The glue takes "" (or NULL) for "no rc file".
        let rc = rc.map(c_path).transpose()?.unwrap_or_default();
        let (sdk, message) = Sdk::bring_up(unit, true, |err: *mut c_char, errlen: c_int| {
            // SAFETY: config and rc are NUL-terminated C strings that outlive the call and are
            // only read. err points to errlen writable bytes of a live buffer nothing else
            // accesses during the call (Sdk::bring_up); the glue writes at most errlen bytes
            // there (snprintf) and keeps none of the three pointers. bring_up holds the
            // process-wide claim, so no handle exists and no other thread is inside a glue
            // call; ob_sdk_init is the glue's entry point for exactly that state.
            unsafe { sys::ob_sdk_init(unit, config.as_ptr(), rc.as_ptr(), err, errlen) }
        })?;
        Ok((
            Self {
                sdk,
                link_registered: false,
            },
            message,
        ))
    }

    /// The chip's name as the SDK prints it (`ob_sdk_hw_info`), e.g. `BCM56960_B1`.
    pub fn hw_info(&self) -> Result<String, Error> {
        let mut buf = [0u8; HW_INFO_LEN];
        let len = c_int::try_from(buf.len()).unwrap_or(0);
        let ptr: *mut c_char = buf.as_mut_ptr().cast();
        // SAFETY: self exists, so ob_sdk_init succeeded on this unit. ptr points to the len
        // bytes of buf, a live local; the glue writes at most len bytes including the NUL
        // (snprintf with len) and does not keep the pointer.
        let rv = unsafe { sys::ob_sdk_hw_info(self.sdk.raw_unit(), ptr, len) };
        check(rv)?;
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(buf.get(..end).unwrap_or(&buf)).into_owned())
    }

    /// The unit as the glue takes it. For the area modules.
    pub(crate) const fn raw_unit(&self) -> c_int {
        self.sdk.raw_unit()
    }
}

impl Deref for Switch {
    type Target = Sdk;

    fn deref(&self) -> &Sdk {
        &self.sdk
    }
}

impl DerefMut for Switch {
    fn deref_mut(&mut self) -> &mut Sdk {
        &mut self.sdk
    }
}

impl Drop for Switch {
    fn drop(&mut self) {
        // Before the Sdk field is dropped (ob_sdk_shutdown): no callback of this handle
        // outlives it, and the closure (with whatever it owns, e.g. a channel sender) goes.
        self.clear_link_callback();
    }
}

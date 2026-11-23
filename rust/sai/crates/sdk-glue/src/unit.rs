//! The owning handle of the SDK unit, and its bring-up for MDIO access.

use std::cell::Cell;
use std::ffi::{CString, c_char, c_int};
use std::fmt;
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use sdk_glue_sys as sys;

use crate::error::{Error, check};

/// A handle is alive in this process (the glue's state is global).
static OPEN: AtomicBool = AtomicBool::new(false);

/// Size of the message buffer handed to the glue (the C tool's `err[600]`).
const MESSAGE_LEN: usize = 600;

/// The SDK, brought up on one unit. The only way to the methods of [`crate::mdio`] and
/// [`crate::temp`], so none of them can run before a bring-up succeeded.
///
/// One per process at a time: the glue keeps its state in globals and holds the unit's SDK
/// lock (`/run/openbcm-unit<N>.lock`) from the first bring-up to the end of the process.
/// `Send` but not `Sync`: methods that touch the chip take `&mut self`, and a shared
/// reference must not cross threads either.
///
/// Dropping it ends this handle's use of the SDK: `ob_sdk_shutdown` is called when the BCM
/// layer was brought up (the full switch bring-up; not bound yet). After
/// [`Sdk::init_mdio`] there is no BCM layer and nothing to undo: the C tool calls nothing at
/// exit either, the glue keeps the unit attached and the lock held until the process ends.
#[derive(Debug)]
pub struct Sdk {
    unit: c_int,
    /// "init all" ran: callbacks may be registered, [`Drop`] calls `ob_sdk_shutdown`.
    bcm_layer: bool,
    _not_sync: PhantomData<Cell<()>>,
}

/// Why a bring-up did not return a handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    /// Another [`Sdk`] is alive in this process. Nothing was called.
    AlreadyOpen,
    /// The config path holds a NUL byte and cannot be passed to C. Nothing was called.
    BadPath,
    /// The glue refused or failed; `message` is its one line (the reason).
    Sdk {
        /// The code: [`crate::Code::Busy`] (SDK lock held by another process),
        /// [`crate::Code::Config`] (config file missing), [`crate::Code::MdioClock`],
        /// [`crate::Code::Unit`], [`crate::Code::Fail`] (init soc / init misc), ...
        error: Error,
        /// The glue's message.
        message: String,
    },
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyOpen => f.write_str("the SDK is already open in this process"),
            Self::BadPath => f.write_str("the SDK config path contains a NUL byte"),
            Self::Sdk { error, message } => write!(f, "{message} ({error})"),
        }
    }
}

impl std::error::Error for InitError {}

/// Releases the process-wide claim unless a handle took it over.
struct Claim(bool);

impl Claim {
    fn take() -> Option<Self> {
        OPEN.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            // Lazily: a Claim built for a refused attempt would release, when dropped, the
            // claim of the handle that is alive.
            .then(|| Self(true))
    }

    fn keep(mut self) {
        self.0 = false;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.0 {
            OPEN.store(false, Ordering::Release);
        }
    }
}

/// The text the glue left in a message buffer: up to the first NUL.
fn message_text(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(buf.get(..end).unwrap_or(buf)).into_owned()
}

impl Sdk {
    /// Brings the SDK up for MDIO access to PHYs it does not manage (`ob_sdk_init_mdio`):
    /// "init soc" + "init misc" with the SDK config file `config`, nothing else (no BCM layer,
    /// no port enabled). **This resets the switch chip.** Misc init programs the external MDIO
    /// clock and hold delay, which the glue reads back; a difference fails with
    /// [`crate::Code::MdioClock`] before any MDIO frame is sent.
    ///
    /// On success: the handle and the glue's line about the chip and its MDIO clock registers.
    /// The process then holds the unit's SDK lock until it ends.
    pub fn init_mdio(unit: i32, config: &Path) -> Result<(Self, String), InitError> {
        let claim = Claim::take().ok_or(InitError::AlreadyOpen)?;
        let config = CString::new(config.as_os_str().as_bytes()).map_err(|_| InitError::BadPath)?;
        let mut message = [0u8; MESSAGE_LEN];
        let len = c_int::try_from(message.len()).unwrap_or(0);
        let buf: *mut c_char = message.as_mut_ptr().cast();
        // SAFETY: config is a NUL-terminated C string that lives until after the call and is
        // only read. buf points to the len bytes of message, a live local nothing else
        // accesses during the call; the glue writes at most len bytes there (snprintf with
        // errlen) and keeps neither pointer. The claim above means no other handle exists, so
        // no other thread is inside a glue call that needs one; the function itself is the
        // glue's entry point for a process that has not initialised the SDK.
        let rv = unsafe { sys::ob_sdk_init_mdio(unit, config.as_ptr(), buf, len) };
        let text = message_text(&message);
        match check(rv) {
            Ok(_) => {
                claim.keep();
                Ok((
                    Self {
                        unit,
                        bcm_layer: false,
                        _not_sync: PhantomData,
                    },
                    text,
                ))
            }
            Err(error) => Err(InitError::Sdk { error, message: text }),
        }
    }

    /// The shape every bring-up shares, for the area modules (`crate::switch`): takes the
    /// process-wide claim, hands `call` a zeroed message buffer and its length, and turns the
    /// glue's return code into the handle (the claim kept) or the error (the claim released),
    /// with the buffer's text either way. `bcm_layer`: the bring-up runs "init all", so
    /// dropping the handle calls `ob_sdk_shutdown`. `call` is not run when a handle is alive.
    pub(crate) fn bring_up(
        unit: c_int,
        bcm_layer: bool,
        call: impl FnOnce(*mut c_char, c_int) -> c_int,
    ) -> Result<(Self, String), InitError> {
        let claim = Claim::take().ok_or(InitError::AlreadyOpen)?;
        let mut message = [0u8; MESSAGE_LEN];
        let len = c_int::try_from(message.len()).unwrap_or(0);
        let rv = call(message.as_mut_ptr().cast(), len);
        let text = message_text(&message);
        match check(rv) {
            Ok(_) => {
                claim.keep();
                Ok((
                    Self {
                        unit,
                        bcm_layer,
                        _not_sync: PhantomData,
                    },
                    text,
                ))
            }
            Err(error) => Err(InitError::Sdk { error, message: text }),
        }
    }

    /// The unit number.
    pub const fn unit(&self) -> i32 {
        self.unit
    }

    /// The unit as the glue takes it. For the area modules.
    pub(crate) const fn raw_unit(&self) -> c_int {
        self.unit
    }
}

impl Drop for Sdk {
    fn drop(&mut self) {
        if self.bcm_layer {
            // SAFETY: self exists, so the bring-up of this unit succeeded in this process and
            // no other handle is alive; ob_sdk_shutdown takes the unit number only and
            // unregisters the glue's own SDK callbacks.
            unsafe { sys::ob_sdk_shutdown(self.unit) };
        }
        OPEN.store(false, Ordering::Release);
    }
}

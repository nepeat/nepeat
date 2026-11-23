//! The owning handle of the SDK unit ([`Sdk`]), the bring-up for MDIO access, and what every
//! bring-up shares: the one-handle-per-SDK claim, the message buffer, the liveness of the
//! handles a bring-up hands out, the boot rule of the real glue.

use std::cell::Cell;
use std::ffi::{CString, c_int};
use std::fmt;
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::glue::Glue;

/// A handle of the real glue is alive in this process (its state is global).
#[cfg(not(ob_glue_rust_only))]
pub(crate) static REAL_OPEN: AtomicBool = AtomicBool::new(false);

/// Size of the message buffer handed to the glue (the C tool's `err[600]`).
const MESSAGE_LEN: usize = 600;

/// How the real glue's SDK was first booted in this process: it boots once, and later
/// bring-ups reuse that boot whatever they pass (`sdk_glue.c` `sdk_boot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Boot {
    /// `ob_sdk_attach`: booted with no config (`/dev/null`).
    Attach,
    /// `ob_sdk_init` / `ob_sdk_init_mdio` with this config file.
    Config(PathBuf),
}

/// The first successful boot of the real glue in this process.
static REAL_BOOT: Mutex<Option<Boot>> = Mutex::new(None);

/// Why a bring-up asking for `next` would not get what it asks after `first`: the glue keeps
/// the SAL config of its first boot for the life of the process. `None`: no conflict.
pub(crate) fn boot_conflict(first: Option<&Boot>, next: &Boot) -> Option<String> {
    match (first, next) {
        (Some(Boot::Attach), Boot::Config(path)) => Some(format!(
            "the SDK of this process was booted attach-only, without a config: the glue would not load {}",
            path.display()
        )),
        (Some(Boot::Config(first)), Boot::Config(path)) if first != path => Some(format!(
            "the SDK of this process was booted with {}: the glue would ignore {}",
            first.display(),
            path.display()
        )),
        _ => None,
    }
}

/// Whether the handles of one bring-up may still call the glue. Its owner sets it to false
/// when it is dropped, under the write lock: a call in progress on another thread (under the
/// read lock) finishes first, and none starts afterwards.
#[derive(Debug, Default)]
pub(crate) struct Live(RwLock<bool>);

impl Live {
    fn new() -> Arc<Self> {
        Arc::new(Self(RwLock::new(true)))
    }

    /// Runs `call` if the owner is alive (`None` otherwise), with the owner kept alive meanwhile.
    pub(crate) fn with<R>(&self, call: impl FnOnce() -> R) -> Option<R> {
        let alive = self.0.read().unwrap_or_else(PoisonError::into_inner);
        alive.then(call)
    }

    fn kill(&self) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = false;
    }
}

/// What a bring-up brought up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    /// `ob_sdk_attach`: register access, temperatures; no MDIO (misc init did not run).
    Attach,
    /// `ob_sdk_init_mdio`: "init soc" + "init misc"; MDIO and temperatures.
    Mdio,
    /// `ob_sdk_init`: "init all"; everything. Dropping calls `ob_sdk_shutdown`.
    Bcm,
}

/// The state every handle shares: unit, glue, liveness of the handles, the temperature lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) unit: c_int,
    pub(crate) glue: Glue,
    pub(crate) layer: Layer,
    pub(crate) live: Arc<Live>,
    /// Temperature reads, from the owner and its [`crate::TempReader`]s, one at a time (a read
    /// resets the hardware peak; the C++ backend's `temp_mu_`).
    pub(crate) temp_mu: Arc<Mutex<()>>,
}

impl Drop for Core {
    fn drop(&mut self) {
        // The handles first: an MDIO access in progress on another thread finishes, none starts.
        self.live.kill();
        if self.layer == Layer::Bcm {
            // [I2]: the owner of the BCM layer, once, as it goes.
            self.glue.shutdown(self.unit);
        }
        self.glue.open_flag().store(false, Ordering::Release);
    }
}

/// The SDK, brought up on one unit for MDIO access ([`Sdk::init_mdio`]), or the [`Sdk`] inside a
/// [`crate::Switch`]. The only way to the methods of [`crate::mdio`] and [`crate::temp`], so none
/// of them can run before a bring-up that ran misc init succeeded.
///
/// One per SDK at a time: the real glue keeps its state in globals and holds the unit's SDK
/// lock (`/run/openbcm-unit<N>.lock`) from the first bring-up to the end of the process (each
/// Rust fake chip has its own). `Send` but not `Sync`: methods that touch the chip take
/// `&mut self`, and a shared reference must not cross threads either; the handles
/// [`crate::MdioBus`] and [`crate::TempReader`] are the cloneable, `Sync` way.
///
/// Dropping it ends this handle's use of the SDK: its handles stop working, and
/// `ob_sdk_shutdown` is called when the BCM layer was brought up (inside a [`crate::Switch`]).
/// After [`Sdk::init_mdio`] there is no BCM layer and nothing to undo: the C tool calls nothing at
/// exit either, the glue keeps the unit attached and the lock held until the process ends.
#[derive(Debug)]
pub struct Sdk {
    pub(crate) core: Core,
    _not_sync: PhantomData<Cell<()>>,
}

/// Why a bring-up did not return a handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    /// Another handle of the same SDK (the process's, or the fake chip's) is alive. Nothing
    /// was called.
    AlreadyOpen,
    /// The config path holds a NUL byte and cannot be passed to C. Nothing was called.
    BadPath,
    /// The glue refused or failed; `message` is its one line (the reason). Also, with the
    /// glue's code for it and nothing called: a bring-up of the real glue whose config this
    /// process's SDK would ignore ([`crate::Code::Config`]: it was booted attach-only, or with
    /// another config; the glue boots once per process).
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

/// Releases a glue's "a handle is alive" flag unless a handle took it over.
struct Claim<'g>(Option<&'g AtomicBool>);

impl<'g> Claim<'g> {
    fn take(flag: &'g AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            // Lazily: a Claim built for a refused attempt would release, when dropped, the
            // claim of the handle that is alive.
            .then(|| Self(Some(flag)))
    }

    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(flag) = self.0 {
            flag.store(false, Ordering::Release);
        }
    }
}

/// The text the glue left in a message buffer: up to the first NUL.
pub(crate) fn message_text(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(buf.get(..end).unwrap_or(buf)).into_owned()
}

pub(crate) fn c_path(path: &Path) -> Result<CString, InitError> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| InitError::BadPath)
}

/// The shape every bring-up shares: takes the glue's claim, applies the real glue's boot rule,
/// hands `call` a zeroed message buffer, and turns the return code into the core (the claim
/// kept) or the error (the claim released), with the buffer's text either way. `call` is not run
/// when a handle is alive or the boot rule refuses.
pub(crate) fn bring_up(
    glue: Glue,
    unit: c_int,
    layer: Layer,
    boot: Boot,
    call: impl FnOnce(&Glue, &mut [u8]) -> c_int,
) -> Result<(Core, String), InitError> {
    let claim = Claim::take(glue.open_flag()).ok_or(InitError::AlreadyOpen)?;
    let real = glue.is_real_sdk();
    if real {
        let first = REAL_BOOT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(message) = boot_conflict(first.as_ref(), &boot) {
            return Err(InitError::Sdk {
                error: Error::from_raw(sys::OB_E_CONFIG).unwrap_or(Error::INTERNAL),
                message,
            });
        }
    }
    let mut message = [0u8; MESSAGE_LEN];
    // [I1]: the claim is held and no handle of this glue exists, so no other call runs.
    let rv = call(&glue, &mut message);
    let text = message_text(&message);
    match check(rv) {
        Ok(_) => {
            if real {
                REAL_BOOT
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert(boot);
            }
            claim.keep();
            Ok((
                Core {
                    unit,
                    glue,
                    layer,
                    live: Live::new(),
                    temp_mu: Arc::new(Mutex::new(())),
                },
                text,
            ))
        }
        Err(error) => Err(InitError::Sdk { error, message: text }),
    }
}

impl Sdk {
    /// Brings the SDK up for MDIO access to PHYs it does not manage (`ob_sdk_init_mdio`): "init
    /// soc" + "init misc" with the SDK config file `config`, nothing else (no BCM layer, no port
    /// enabled). **This resets the switch chip.** Misc init programs the external MDIO clock and
    /// hold delay, which the glue reads back; a difference fails with [`crate::Code::MdioClock`]
    /// before any MDIO frame is sent.
    ///
    /// On success: the handle and the glue's line about the chip and its MDIO clock registers.
    /// The process then holds the unit's SDK lock until it ends. The glue this program links
    /// (or, in a build with only the feature `rust-fake`, the process's Rust fake).
    pub fn init_mdio(unit: i32, config: &Path) -> Result<(Self, String), InitError> {
        Self::init_mdio_with(Glue::process(), unit, config)
    }

    /// [`Sdk::init_mdio`] on a Rust fake chip: its scenario is the file `config`.
    #[cfg(feature = "rust-fake")]
    pub fn init_mdio_on(chip: &crate::rust_fake::Chip, unit: i32, config: &Path) -> Result<(Self, String), InitError> {
        Self::init_mdio_with(Glue::Fake(chip.clone()), unit, config)
    }

    fn init_mdio_with(glue: Glue, unit: i32, config: &Path) -> Result<(Self, String), InitError> {
        let config_c = c_path(config)?;
        let (core, message) = bring_up(glue, unit, Layer::Mdio, Boot::Config(config.to_path_buf()), |g, err| {
            g.init_mdio(unit, &config_c, err)
        })?;
        Ok((Self::from_core(core), message))
    }

    pub(crate) fn from_core(core: Core) -> Self {
        Self {
            core,
            _not_sync: PhantomData,
        }
    }

    /// The unit number.
    pub const fn unit(&self) -> i32 {
        self.core.unit
    }

    /// The unit as the glue takes it. For the area modules.
    pub(crate) const fn raw_unit(&self) -> c_int {
        self.core.unit
    }

    /// The glue this handle drives. For the area modules.
    pub(crate) const fn glue(&self) -> &Glue {
        &self.core.glue
    }
}

#[cfg(test)]
mod tests {
    use super::{Boot, boot_conflict};
    use std::path::PathBuf;

    #[test]
    fn the_real_glue_boots_once_and_keeps_its_config() {
        let a = Boot::Config(PathBuf::from("/etc/a.bcm"));
        let b = Boot::Config(PathBuf::from("/etc/b.bcm"));
        assert_eq!(boot_conflict(None, &a), None);
        assert_eq!(boot_conflict(None, &Boot::Attach), None);
        assert_eq!(boot_conflict(Some(&a), &a), None);
        assert_eq!(boot_conflict(Some(&a), &Boot::Attach), None);
        assert_eq!(boot_conflict(Some(&Boot::Attach), &Boot::Attach), None);
        assert!(boot_conflict(Some(&a), &b).is_some_and(|m| m.contains("/etc/a.bcm") && m.contains("would ignore")));
        assert!(boot_conflict(Some(&Boot::Attach), &a).is_some_and(|m| m.contains("attach-only")));
    }
}

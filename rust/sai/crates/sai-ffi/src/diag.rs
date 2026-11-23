//! The diag socket (crate `sai-diag`, the port of `sai_diag.cpp`) on the API lock: its commands
//! run on the core under the same mutex as every SAI call, but raw MDIO and `extphy txn` on a
//! switch with external PHY tables, which the switch's `sai_core::diag::DiagPort` serves without
//! it (lock order: API lock, a diag lease, a PHY's slot, the MDIO master's lock). Started by `create_switch` when the
//! profile has `SAI_OPENBCM_DIAG_SOCKET`; stopped by `remove_switch` and `sai_api_uninitialize`,
//! always after the lock was released (the server's thread may be waiting for it).
use crate::state;
use sai_core::backend::Profile;
use sai_diag::{DiagHandler, DiagHost, DiagServer, Level, PhyDiag};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::sync::Arc;

/// The library as the diag server sees it.
struct Host;

impl DiagHost for Host {
    fn with_handler(&self, body: &mut dyn FnMut(Option<&mut dyn DiagHandler>)) {
        let mut ran = false;
        // Uninitialised or poisoned: the closure does not run, and the command finds no switch.
        let _ = state::with_api(|api| {
            ran = true;
            body(Some(&mut api.sai));
            Ok(())
        });
        if !ran {
            body(None);
        }
    }

    fn peer_uid(&self, conn: BorrowedFd<'_>) -> Option<u32> {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let size = size_of::<libc::ucred>();
        let mut len = libc::socklen_t::try_from(size).ok()?;
        // SAFETY: `conn` is an open socket for the whole call (borrowed); `cred` is a writable
        // `ucred` of `len` bytes and `len` a writable `socklen_t`, which is what getsockopt(2)
        // writes for SO_PEERCRED (at most `len` bytes, then the length it wrote).
        let rc = unsafe {
            libc::getsockopt(
                conn.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast(),
                &raw mut len,
            )
        };
        (rc == 0 && usize::try_from(len).ok() == Some(size)).then_some(cred.uid)
    }

    fn log(&self, level: Level, text: &str) {
        sai_core::diag::log(level, text);
    }
}

/// The server of a switch created with `profile`, when the profile asks for one; `phy`: the
/// switch's external-PHY arbitration (`sai_core::diag::phy_diag`), which serves raw MDIO and
/// transactions without the API lock.
pub(crate) fn start(profile: &Profile, phy: Option<Arc<dyn PhyDiag>>) -> Option<DiagServer> {
    sai_core::diag::start(profile, Arc::new(Host), phy)
}

/// The test port's `diag_start(path)`: the server at `path` as `create_switch` starts it with
/// `SAI_OPENBCM_DIAG_SOCKET` (empty: nothing; one already running stays).
#[cfg(feature = "test-port")]
#[allow(dead_code, reason = "for src/test_port.rs (tp::diag_start)")]
pub(crate) fn test_start(path: &str) {
    let _ = state::with_api(|api| {
        if api.diag.is_none() {
            api.diag = start(
                &Profile::from_pairs([(sai_diag::SOCKET_KEY, path)]),
                sai_core::diag::phy_diag(&api.sai),
            );
        }
        Ok(())
    });
}

/// The test port's `diag_stop()`: stops the server and removes its socket, outside the lock.
#[cfg(feature = "test-port")]
#[allow(dead_code, reason = "for src/test_port.rs (tp::diag_stop)")]
pub(crate) fn test_stop() {
    let server = state::with_api(|api| Ok(api.diag.take())).ok().flatten();
    drop(server);
}

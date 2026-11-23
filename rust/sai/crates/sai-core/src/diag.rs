//! The diag socket's commands on the core: the part of `sai_diag.cpp` that runs under the SAI
//! lock. The socket, the thread and the parsing are `sai-diag`; the rim (`sai-ffi`) implements
//! its [`DiagHost`] over the API lock and starts the server at switch creation ([`start`]).
//!
//! - Shell commands go to [`crate::backend::Shell::run_shell_to`]; `phy raw ...` through
//!   [`crate::extphy::ext_phy_foreign_mdio`] (the external PHY table's polls pause around it).
//! - `extphy`, `extmacsec`, `extmacsec trace`, `extmacsec run`: the hooks of
//!   [`crate::extphy`] and [`crate::macsec`].
use crate::backend::{Backend, Profile};
use crate::sai::Sai;
use crate::{extphy, macsec};
use sai_diag::{DiagHandler, DiagHost, DiagServer, Level, MacsecRun, MdioAccess, ShellCommand, ShellFailed};
use sai_sys as sys;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

impl<B: Backend> DiagHandler for Sai<B> {
    fn switch_created(&self) -> bool {
        self.switch_id().is_some()
    }

    fn ext_phy_status(&mut self) -> String {
        extphy::ext_phy_status(self)
    }

    fn ext_macsec_status(&mut self) -> String {
        macsec::ext_macsec_status(self)
    }

    fn ext_macsec_trace(&mut self, args: &str) -> String {
        macsec::ext_macsec_trace(self, args)
    }

    fn ext_macsec_run(&mut self, run: &MacsecRun<'_>) -> String {
        macsec::ext_macsec_diag_run(self, run.port, run.sequence, run.param_file.unwrap_or_default())
    }

    fn shell(&mut self, command: &ShellCommand<'_>, out: BorrowedFd<'_>) -> Result<(), ShellFailed> {
        let done = match command.mdio {
            MdioAccess::Raw => extphy::ext_phy_foreign_mdio(self, |backend| backend.run_shell_to(command.line, out)),
            MdioAccess::Other => self.backend_mut().run_shell_to(command.line, out),
        };
        done.map_err(|e| ShellFailed(e.status()))
    }
}

/// A line of the diag server for the library's log (API `SWITCH`, as the C++ logs it).
pub fn log(level: Level, text: &str) {
    match level {
        Level::Notice => crate::log::notice(sys::SAI_API_SWITCH, format_args!("{text}")),
        Level::Error => crate::log::error(sys::SAI_API_SWITCH, format_args!("{text}")),
    }
}

/// Starts the diag server when the profile asks for one (C++ `diag_start`): nothing without
/// [`sai_diag::SOCKET_KEY`] or with an empty value; a socket that cannot be set up is logged and
/// the switch goes on without it.
pub fn start(profile: &Profile, host: Arc<dyn DiagHost>) -> Option<DiagServer> {
    let path = profile.get(sai_diag::SOCKET_KEY).filter(|p| !p.is_empty())?;
    DiagServer::start(path, host)
        .map_err(|e| log(Level::Error, &e.to_string()))
        .ok()
}

//! The diag socket's commands on the core: the part of `sai_diag.cpp` that runs under the SAI
//! lock, and the external PHYs' raw MDIO and transactions that run without it. The socket, the
//! thread and the parsing are `sai-diag`; the rim (`sai-ffi`) implements its [`DiagHost`] over
//! the API lock and starts the server at switch creation ([`start`]) with this switch's
//! [`DiagPort`] ([`phy_diag`]).
//!
//! - Shell commands go to [`crate::backend::Shell::run_shell_to`].
//! - `phy raw c45 PHY DEV REG [VALUE]` (one PHY, device 1..=31) and `extphy txn ...`: served by
//!   [`DiagPort`] **without the SAI lock**, over the backend's MDIO handle, inside the PHY
//!   arbitration (`sai-extphy`), when the library runs PHY sequences of its own (an external PHY
//!   or `MACsec` table is attached) and the backend's shell would answer it from that bus
//!   ([`crate::backend::Backend::diag_mdio_native`]). The answer is the SDK shell's: a read
//!   `\t0x<reg>: 0x<value>\n` (`%02x` / `%04x`), a write nothing; a failed access
//!   `ERROR: MII Addr <phy>: soc_miimc45_read failed: status <n>` (the SDK prints its own error
//!   text, not the status) and `openbcm-diag: command failed (<n>)`.
//! - Every other `phy raw ...` (clause 22, devad 0, `sbus`, a broadcast address, no table):
//!   through the backend's shell under the SAI lock, within the arbitration
//!   ([`crate::extphy::ext_phy_raw_shell`]).
//! - `extphy`, `extmacsec`, `extmacsec trace`, `extmacsec run`: the hooks of
//!   [`crate::extphy`] and [`crate::macsec`] (`extmacsec run` is refused inside a transaction on
//!   its PHY).
use crate::backend::types::{MdioBus, MdioDev, PhyAddr};
use crate::backend::{Backend, Profile};
use crate::extphy::{DiagTarget, DiagWho, ExtPhy, LeaseScope, TxnRefused};
use crate::sai::Sai;
use crate::{extphy, macsec};
use sai_diag::{
    DiagHandler, DiagHost, DiagServer, Level, MacsecRun, MdioAccess, PhyDiag, RawFailed, RawRequest, RawTarget, Reply,
    ShellCommand, ShellFailed, Txn,
};
use sai_sys as sys;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

fn who(request: &RawRequest<'_>) -> DiagWho {
    request.txn.map_or(DiagWho::Bare, DiagWho::Txn)
}

fn target(request: &RawRequest<'_>) -> DiagTarget {
    match request.target {
        RawTarget::Phy(p) => DiagTarget::Phy(p),
        RawTarget::Global => DiagTarget::Global,
    }
}

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
            MdioAccess::Raw => {
                return self
                    .raw_mdio(&RawRequest::new(command.line, None), out)
                    .map_err(|e| match e {
                        RawFailed::Shell(s) => s,
                        RawFailed::Refused(_) => ShellFailed(sys::SAI_STATUS_OBJECT_IN_USE),
                    });
            }
            MdioAccess::Other => self.backend_mut().run_shell_to(command.line, out),
        };
        done.map_err(|e| ShellFailed(e.status()))
    }

    fn raw_mdio(&mut self, request: &RawRequest<'_>, out: BorrowedFd<'_>) -> Result<(), RawFailed> {
        extphy::ext_phy_raw_shell(self, who(request), target(request), |backend| {
            backend.run_shell_to(request.line, out)
        })
        .map_err(|refused| RawFailed::Refused(refused.to_string()))?
        .map_err(|e| RawFailed::Shell(ShellFailed(e.status())))
    }

    fn ext_phy_txn(&mut self, txn: &Txn<'_>) -> Reply {
        match self.store() {
            Some(store) => txn_reply(&store.ext().phy, txn),
            None => Reply::failed("no switch"),
        }
    }
}

/// The diag socket's view of one switch's external PHYs: the arbitration and the backend's MDIO
/// handle, used by the server **without** the SAI lock ([`phy_diag`]).
#[derive(Debug)]
pub struct DiagPort {
    phy: ExtPhy,
    bus: MdioBus,
    /// Clause-45 raw MDIO is served here (else by the backend's shell, under the SAI lock).
    native: bool,
}

/// The [`DiagPort`] of the switch, for [`start`]: `None` without a switch or an MDIO handle.
pub fn phy_diag<B: Backend>(sai: &Sai<B>) -> Option<Arc<dyn PhyDiag>> {
    let store = sai.store()?;
    let bus = sai.backend().mdio_bus()?;
    Some(Arc::new(DiagPort {
        phy: store.ext().phy.clone(),
        bus,
        native: sai.backend().diag_mdio_native() && extphy::manages_phys(sai),
    }))
}

impl PhyDiag for DiagPort {
    fn raw(&self, request: &RawRequest<'_>) -> Option<Reply> {
        let c45 = request.c45.filter(|_| self.native)?;
        let dev = MdioDev::new(c45.dev)?;
        let phy = PhyAddr(u32::from(c45.phy));
        let access = || match c45.value {
            None => match self.bus.read(phy, dev, c45.reg) {
                Ok(v) => Reply::ok(format!("\t0x{:02x}: 0x{v:04x}\n", c45.reg)),
                Err(e) => Reply {
                    text: format!(
                        "ERROR: MII Addr {}: soc_miimc45_read failed: status {}\n",
                        c45.phy,
                        e.status()
                    ),
                    failure: Some(format!("command failed ({})", e.status())),
                },
            },
            Some(value) => match self.bus.write(phy, dev, c45.reg, value) {
                Ok(()) => Reply::ok(""),
                Err(e) => Reply {
                    text: format!(
                        "ERROR: MII Addr {}: soc_miimc45_write failed: status {}\n",
                        c45.phy,
                        e.status()
                    ),
                    failure: Some(format!("command failed ({})", e.status())),
                },
            },
        };
        Some(
            self.phy
                .diag_access(who(request), target(request), access)
                .unwrap_or_else(|refused| Reply::failed(refused.to_string())),
        )
    }

    fn txn(&self, txn: &Txn<'_>) -> Reply {
        txn_reply(&self.phy, txn)
    }
}

/// `extphy txn begin|renew|end|list` on a switch's arbitration (a raw access is not one).
fn txn_reply(phy: &ExtPhy, txn: &Txn<'_>) -> Reply {
    let failed = |e: TxnRefused| Reply::failed(e.to_string());
    match txn {
        Txn::Begin { phys, ms } => {
            let scope = phys.as_ref().map_or(LeaseScope::All, |p| {
                LeaseScope::Phys(p.iter().copied().collect::<BTreeSet<u16>>())
            });
            phy.txn_begin(scope, *ms).map_or_else(failed, |i| {
                Reply::ok(format!(
                    "txn {} begun: {}, lease {} ms after each access, at most {} ms\n",
                    i.txn, i.scope, i.step_ms, i.max_left_ms
                ))
            })
        }
        Txn::Renew { txn, ms } => phy.txn_renew(*txn, *ms).map_or_else(failed, |i| {
            Reply::ok(format!(
                "txn {} renewed: {} ms left, at most {} ms\n",
                i.txn, i.left_ms, i.max_left_ms
            ))
        }),
        Txn::End { txn } => phy.txn_end(*txn).map_or_else(failed, |e| {
            Reply::ok(format!(
                "txn {} ended: {} accesses in {} ms\n",
                e.txn, e.accesses, e.held_ms
            ))
        }),
        Txn::List => {
            let leases = phy.leases();
            let mut s = format!("extphy txn: {} leases\n", leases.len());
            for l in leases {
                let _ = match l.txn {
                    Some(t) => writeln!(
                        s,
                        "txn {t} {}: {} ms left (at most {} ms), {} accesses",
                        l.scope,
                        l.left_ms,
                        l.max_left_ms.unwrap_or_default(),
                        l.accesses
                    ),
                    None => writeln!(
                        s,
                        "raw MDIO hold-off {}: {} ms left, {} accesses",
                        l.scope, l.left_ms, l.accesses
                    ),
                };
            }
            Reply::ok(s)
        }
        Txn::Raw(_) => Reply::failed("extphy txn: a raw access is served by the raw path"),
        Txn::Usage => Reply::failed(sai_diag::command::TXN_USAGE),
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
/// the switch goes on without it. `phy`: the switch's [`phy_diag`] (raw MDIO and transactions
/// without the SAI lock; `None`: everything under it).
pub fn start(profile: &Profile, host: Arc<dyn DiagHost>, phy: Option<Arc<dyn PhyDiag>>) -> Option<DiagServer> {
    let path = profile.get(sai_diag::SOCKET_KEY).filter(|p| !p.is_empty())?;
    DiagServer::start_with(path, host, phy)
        .map_err(|e| log(Level::Error, &e.to_string()))
        .ok()
}

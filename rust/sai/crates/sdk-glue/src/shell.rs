//! The SDK's diag shell, one command at a time.

use std::os::fd::BorrowedFd;

use crate::error::{Error, check};
use crate::switch::{Switch, c_text};

/// The longest command (the glue copies it into `char buf[1024]` and would cut it short).
pub const SHELL_CMD_MAX: usize = 1023;

impl Switch {
    /// Runs one diag shell command (`ob_sdk_shell`); its output goes to this process's stdout.
    /// `OB_E_PARAM` for a command with a NUL byte or longer than [`SHELL_CMD_MAX`];
    /// [`crate::Code::Fail`] when the shell says the command failed.
    pub fn run_shell(&mut self, cmd: &str) -> Result<(), Error> {
        let cmd = c_text(cmd, SHELL_CMD_MAX)?;
        check(self.glue().shell(self.raw_unit(), &cmd)).map(|_| ())
    }

    /// The same with the shell's output sent to `fd` (the diag socket). The glue points the
    /// **process's** fd 1 at `fd` for the command: whatever any other thread prints to stdout
    /// meanwhile lands there too.
    pub fn run_shell_to(&mut self, cmd: &str, fd: BorrowedFd<'_>) -> Result<(), Error> {
        let cmd = c_text(cmd, SHELL_CMD_MAX)?;
        check(self.glue().shell_to(self.raw_unit(), &cmd, fd)).map(|_| ())
    }
}

//! Host-interface traps the glue knows: LLDP and LACP as exact L2 user entries (to the CPU or
//! discarded, BPDU), the others as switch controls.

use std::ffi::c_int;

pub use sdk_glue_sys::order::TrapKind;

use crate::error::{Error, check};
use crate::switch::Switch;

impl Switch {
    /// Steers `kind` to the CPU (`to_cpu`) or not. Kinds without an SDK control on this path
    /// (`DHCPv6`, STP, other): [`crate::Code::Unavail`].
    pub fn trap_set(&mut self, kind: TrapKind, to_cpu: bool) -> Result<(), Error> {
        check(self.glue().trap_set(self.raw_unit(), kind.raw(), c_int::from(to_cpu))).map(|_| ())
    }
}

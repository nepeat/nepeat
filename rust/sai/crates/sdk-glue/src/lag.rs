//! LAGs as SDK trunks: create, destroy, the member list.

use std::ffi::c_int;

use crate::error::{Error, check};
use crate::port::c_value;
use crate::switch::{Switch, count};

/// The most members of one trunk (the glue's array).
pub const MAX_TRUNK_MEMBERS: usize = 64;

/// A trunk member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrunkMember {
    /// The SDK logical port.
    pub port: i32,
    /// Not in the distribution (hash) set.
    pub egress_disable: bool,
    /// Received as a non-trunk port (not a drop).
    pub ingress_disable: bool,
}

impl Switch {
    /// Creates a trunk (hash on source and destination IP); its id.
    pub fn trunk_create(&mut self) -> Result<u32, Error> {
        let mut tid: c_int = -1;
        check(self.glue().trunk_create(self.raw_unit(), &mut tid))?;
        count(tid)
    }

    /// Destroys trunk `tid`.
    pub fn trunk_destroy(&mut self, tid: u32) -> Result<(), Error> {
        let t = c_value(tid)?;
        check(self.glue().trunk_destroy(self.raw_unit(), t)).map(|_| ())
    }

    /// Replaces trunk `tid`'s members (none: empties it). At most [`MAX_TRUNK_MEMBERS`].
    pub fn trunk_set(&mut self, tid: u32, members: &[TrunkMember]) -> Result<(), Error> {
        let t = c_value(tid)?;
        if members.len() > MAX_TRUNK_MEMBERS {
            return Err(Error::PARAM);
        }
        let ports: Vec<c_int> = members.iter().map(|m| m.port).collect();
        let flags: Vec<c_int> = members
            .iter()
            .map(|m| c_int::from(m.egress_disable) | (c_int::from(m.ingress_disable) << 1))
            .collect();
        check(self.glue().trunk_set(self.raw_unit(), t, &ports, &flags)).map(|_| ())
    }
}

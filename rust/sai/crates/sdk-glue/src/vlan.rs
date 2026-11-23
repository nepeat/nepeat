//! VLANs and their member ports. The CPU port is a tagged member of every VLAN created here
//! (the glue adds it, so floods reach the host) and is never reported as a member.

use std::ffi::c_int;

use crate::error::{Error, check};
use crate::port::MAX_PORTS;
use crate::switch::Switch;

impl Switch {
    /// Creates VLAN `vid` (1..=4094; the glue refuses others) without front-panel members.
    pub fn vlan_create(&mut self, vid: u16) -> Result<(), Error> {
        check(self.glue().vlan_create(self.raw_unit(), c_int::from(vid))).map(|_| ())
    }

    /// Destroys VLAN `vid`.
    pub fn vlan_destroy(&mut self, vid: u16) -> Result<(), Error> {
        check(self.glue().vlan_destroy(self.raw_unit(), c_int::from(vid))).map(|_| ())
    }

    /// Makes `port` a member of `vid`, tagged or untagged; a member's tagging is changed (the glue
    /// removes and re-adds it).
    pub fn vlan_port_add(&mut self, vid: u16, port: i32, untagged: bool) -> Result<(), Error> {
        check(
            self.glue()
                .vlan_port_add(self.raw_unit(), c_int::from(vid), port, c_int::from(untagged)),
        )
        .map(|_| ())
    }

    /// Removes `port` from `vid`.
    pub fn vlan_port_remove(&mut self, vid: u16, port: i32) -> Result<(), Error> {
        check(self.glue().vlan_port_remove(self.raw_unit(), c_int::from(vid), port)).map(|_| ())
    }

    /// The front-panel members of `vid`: (logical port, untagged).
    pub fn vlan_members(&self, vid: u16) -> Result<Vec<(i32, bool)>, Error> {
        let mut ports: Vec<c_int> = vec![0; MAX_PORTS];
        let mut untagged: Vec<c_int> = vec![0; MAX_PORTS];
        let n = usize::try_from(check(self.glue().vlan_members(
            self.raw_unit(),
            c_int::from(vid),
            &mut ports,
            &mut untagged,
        ))?)
        .map_err(|_| Error::INTERNAL)?;
        if n > MAX_PORTS {
            return Err(Error::INTERNAL);
        }
        Ok(ports
            .iter()
            .zip(&untagged)
            .take(n)
            .map(|(port, untagged)| (*port, *untagged != 0))
            .collect())
    }
}

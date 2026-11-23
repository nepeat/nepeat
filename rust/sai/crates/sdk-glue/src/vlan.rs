//! VLANs and their member ports. The CPU port is a tagged member of every VLAN created here
//! (the glue adds it, so floods reach the host) and is never reported as a member.

use std::ffi::c_int;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::port::MAX_PORTS;
use crate::switch::Switch;

impl Switch {
    /// Creates VLAN `vid` without front-panel members.
    pub fn vlan_create(&mut self, vid: u16) -> Result<(), Error> {
        // SAFETY: self exists (BCM layer up), &mut self keeps other calls of this handle out;
        // plain integers.
        let rv = unsafe { sys::ob_sdk_vlan_create(self.raw_unit(), c_int::from(vid)) };
        check(rv).map(|_| ())
    }

    /// Destroys VLAN `vid`.
    pub fn vlan_destroy(&mut self, vid: u16) -> Result<(), Error> {
        // SAFETY: as vlan_create.
        let rv = unsafe { sys::ob_sdk_vlan_destroy(self.raw_unit(), c_int::from(vid)) };
        check(rv).map(|_| ())
    }

    /// Makes `port` a member of `vid`, tagged or untagged; a member's tagging is changed
    /// (the glue removes and re-adds it).
    pub fn vlan_port_add(&mut self, vid: u16, port: i32, untagged: bool) -> Result<(), Error> {
        // SAFETY: as vlan_create.
        let rv = unsafe { sys::ob_sdk_vlan_port_add(self.raw_unit(), c_int::from(vid), port, c_int::from(untagged)) };
        check(rv).map(|_| ())
    }

    /// Removes `port` from `vid`.
    pub fn vlan_port_remove(&mut self, vid: u16, port: i32) -> Result<(), Error> {
        // SAFETY: as vlan_create.
        let rv = unsafe { sys::ob_sdk_vlan_port_remove(self.raw_unit(), c_int::from(vid), port) };
        check(rv).map(|_| ())
    }

    /// The front-panel members of `vid`: (logical port, untagged).
    pub fn vlan_members(&self, vid: u16) -> Result<Vec<(i32, bool)>, Error> {
        let mut ports: Vec<c_int> = vec![0; MAX_PORTS];
        let mut untagged: Vec<c_int> = vec![0; MAX_PORTS];
        let max = c_int::try_from(MAX_PORTS).unwrap_or(0);
        // SAFETY: self exists (BCM layer up). ports and untagged are live Vecs of max ints
        // each; the glue writes at most max elements to each and keeps neither pointer.
        let rv = unsafe {
            sys::ob_sdk_vlan_members(
                self.raw_unit(),
                c_int::from(vid),
                ports.as_mut_ptr(),
                untagged.as_mut_ptr(),
                max,
            )
        };
        let count = usize::try_from(check(rv)?).map_err(|_| Error::INTERNAL)?;
        if count > MAX_PORTS {
            return Err(Error::INTERNAL);
        }
        Ok(ports
            .iter()
            .zip(&untagged)
            .take(count)
            .map(|(port, untagged)| (*port, *untagged != 0))
            .collect())
    }
}

//! L3: router interfaces, my-station (router MAC) entries, egress objects (next hops), hosts,
//! routes, ECMP groups. Addresses are bytes in network order (`std::net`), so nothing here
//! depends on the byte order of the host.

use std::ffi::c_int;
use std::net::IpAddr;

use crate::error::{Error, check};
use crate::l2::{Dest, vid};
use crate::port::c_value;
use crate::switch::{Switch, count};

/// The most members of one ECMP group (the glue's array).
pub const MAX_ECMP_MEMBERS: usize = 256;

/// What a route points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTarget {
    /// An egress object (next hop, or the CPU / drop object).
    Egress(u32),
    /// An ECMP group.
    Ecmp(u32),
}

fn octets(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(a) => a.octets().to_vec(),
        IpAddr::V6(a) => a.octets().to_vec(),
    }
}

fn prefix(addr: IpAddr, mask: IpAddr) -> Result<(Vec<u8>, Vec<u8>), Error> {
    if addr.is_ipv4() != mask.is_ipv4() {
        return Err(Error::PARAM);
    }
    Ok((octets(addr), octets(mask)))
}

fn members(egress: &[u32]) -> Result<Vec<c_int>, Error> {
    if egress.is_empty() || egress.len() > MAX_ECMP_MEMBERS {
        return Err(Error::PARAM);
    }
    egress.iter().map(|e| c_value(*e)).collect()
}

impl Switch {
    /// Creates a router interface on `vid` with router MAC `mac`, `mtu` and VRF `vrf` (the VLAN
    /// gets IPv4 / IPv6 routing on); its id.
    pub fn l3_intf_create(&mut self, mac: [u8; 6], vid: u16, mtu: u32, vrf: u32) -> Result<u32, Error> {
        let (v, m, r) = (self::vid(vid)?, c_value(mtu)?, c_value(vrf)?);
        let mut id: c_int = -1;
        check(self.glue().l3_intf_create(self.raw_unit(), &mac, v, m, r, &mut id))?;
        count(id)
    }

    /// Replaces router interface `id`'s settings.
    pub fn l3_intf_update(&mut self, id: u32, mac: [u8; 6], vid: u16, mtu: u32, vrf: u32) -> Result<(), Error> {
        let (i, v, m, r) = (c_value(id)?, self::vid(vid)?, c_value(mtu)?, c_value(vrf)?);
        check(self.glue().l3_intf_update(self.raw_unit(), i, &mac, v, m, r)).map(|_| ())
    }

    /// Destroys router interface `id`.
    pub fn l3_intf_destroy(&mut self, id: u32) -> Result<(), Error> {
        let i = c_value(id)?;
        check(self.glue().l3_intf_destroy(self.raw_unit(), i)).map(|_| ())
    }

    /// Adds a my-station entry for router MAC `mac` (IPv4, IPv6, ARP terminated); its id.
    pub fn l3_station_add(&mut self, mac: [u8; 6]) -> Result<u32, Error> {
        let mut id: c_int = -1;
        check(self.glue().l3_station_add(self.raw_unit(), &mac, &mut id))?;
        count(id)
    }

    /// Deletes my-station entry `id`.
    pub fn l3_station_delete(&mut self, id: u32) -> Result<(), Error> {
        let i = c_value(id)?;
        check(self.glue().l3_station_delete(self.raw_unit(), i)).map(|_| ())
    }

    /// Creates an egress object (next hop): out of router interface `intf` to `dmac` on `dest` in
    /// `vid`; its id.
    pub fn l3_egress_create(&mut self, intf: u32, dmac: [u8; 6], dest: Dest, vid: u16) -> Result<u32, Error> {
        let (i, d, v) = (c_value(intf)?, dest.raw()?, self::vid(vid)?);
        let mut id: c_int = -1;
        check(self.glue().l3_egress_create(self.raw_unit(), i, &dmac, d, v, &mut id))?;
        count(id)
    }

    /// Replaces egress object `id`.
    pub fn l3_egress_update(&mut self, id: u32, intf: u32, dmac: [u8; 6], dest: Dest, vid: u16) -> Result<(), Error> {
        let (e, i, d, v) = (c_value(id)?, c_value(intf)?, dest.raw()?, self::vid(vid)?);
        check(self.glue().l3_egress_update(self.raw_unit(), e, i, &dmac, d, v)).map(|_| ())
    }

    /// The egress object to the CPU, created once per "init all" and cached by the glue.
    pub fn l3_egress_cpu(&mut self) -> Result<u32, Error> {
        self.egress_special(false)
    }

    /// The egress object that drops, created once per "init all" and cached by the glue.
    pub fn l3_egress_drop(&mut self) -> Result<u32, Error> {
        self.egress_special(true)
    }

    fn egress_special(&mut self, drop: bool) -> Result<u32, Error> {
        let mut id: c_int = -1;
        check(
            self.glue()
                .l3_egress_special(self.raw_unit(), c_int::from(drop), &mut id),
        )?;
        count(id)
    }

    /// Destroys egress object `id` (not the CPU / drop ones: the glue keeps handing those out).
    pub fn l3_egress_destroy(&mut self, id: u32) -> Result<(), Error> {
        let e = c_value(id)?;
        check(self.glue().l3_egress_destroy(self.raw_unit(), e)).map(|_| ())
    }

    /// Adds host `ip` in VRF `vrf` via egress object `egress`.
    pub fn l3_host_add(&mut self, vrf: u32, ip: IpAddr, egress: u32) -> Result<(), Error> {
        let (r, e) = (c_value(vrf)?, c_value(egress)?);
        check(self.glue().l3_host_add(self.raw_unit(), r, &octets(ip), e)).map(|_| ())
    }

    /// Deletes host `ip` in VRF `vrf`.
    pub fn l3_host_delete(&mut self, vrf: u32, ip: IpAddr) -> Result<(), Error> {
        let r = c_value(vrf)?;
        check(self.glue().l3_host_delete(self.raw_unit(), r, &octets(ip))).map(|_| ())
    }

    /// Adds (with `replace`: replaces) the route `addr` / `mask` (same family) in VRF `vrf` to
    /// `target`; `drop`: matching packets are dropped.
    pub fn l3_route_add(
        &mut self,
        vrf: u32,
        addr: IpAddr,
        mask: IpAddr,
        target: RouteTarget,
        drop: bool,
        replace: bool,
    ) -> Result<(), Error> {
        let r = c_value(vrf)?;
        let (a, m) = prefix(addr, mask)?;
        let (id, ecmp) = match target {
            RouteTarget::Egress(id) => (c_value(id)?, 0),
            RouteTarget::Ecmp(id) => (c_value(id)?, 1),
        };
        check(self.glue().l3_route_add(
            self.raw_unit(),
            r,
            &a,
            &m,
            id,
            ecmp,
            c_int::from(drop),
            c_int::from(replace),
        ))
        .map(|_| ())
    }

    /// Deletes the route `addr` / `mask` in VRF `vrf`.
    pub fn l3_route_delete(&mut self, vrf: u32, addr: IpAddr, mask: IpAddr) -> Result<(), Error> {
        let r = c_value(vrf)?;
        let (a, m) = prefix(addr, mask)?;
        check(self.glue().l3_route_delete(self.raw_unit(), r, &a, &m)).map(|_| ())
    }

    /// Creates an ECMP group over 1..=[`MAX_ECMP_MEMBERS`] egress objects; its id.
    pub fn l3_ecmp_create(&mut self, egress: &[u32]) -> Result<u32, Error> {
        let m = members(egress)?;
        let mut id: c_int = -1;
        check(self.glue().l3_ecmp_set(self.raw_unit(), &mut id, 0, &m))?;
        count(id)
    }

    /// Replaces the members of ECMP group `id`.
    pub fn l3_ecmp_update(&mut self, id: u32, egress: &[u32]) -> Result<(), Error> {
        let m = members(egress)?;
        let mut i = c_value(id)?;
        check(self.glue().l3_ecmp_set(self.raw_unit(), &mut i, 1, &m)).map(|_| ())
    }

    /// Destroys ECMP group `id`.
    pub fn l3_ecmp_destroy(&mut self, id: u32) -> Result<(), Error> {
        let i = c_value(id)?;
        check(self.glue().l3_ecmp_destroy(self.raw_unit(), i)).map(|_| ())
    }
}

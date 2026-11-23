//! KNET: kernel network interfaces for host interfaces, each with its RX filter (packets
//! from its port). The carrier of such a netdev is not the glue's (`/proc/bcm/knet/link`).

use std::ffi::c_int;

use crate::error::{Error, check};
use crate::l2::vid;
use crate::port::c_value;
use crate::switch::{Switch, c_text, count};

/// The longest netif name (the SDK's 16-byte name field with its NUL, as `IFNAMSIZ`).
pub const NETIF_NAME_MAX: usize = 15;

/// A netif and its RX filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Netif {
    /// The netif.
    pub netif: u32,
    /// Its filter.
    pub filter: u32,
}

impl Switch {
    /// Creates netdev `name` (at most [`NETIF_NAME_MAX`] bytes: the glue would cut it) sending
    /// out of `port` with MAC `mac`, and its RX filter (`keep_tag`: frames keep their VLAN tag);
    /// if the filter fails the netif is destroyed again.
    pub fn netif_create(
        &mut self,
        name: &str,
        port: i32,
        vid: u16,
        mac: [u8; 6],
        keep_tag: bool,
    ) -> Result<Netif, Error> {
        let name = c_text(name, NETIF_NAME_MAX)?;
        let v = self::vid(vid)?;
        let (mut netif, mut filter): (c_int, c_int) = (-1, -1);
        check(self.glue().netif_create(
            self.raw_unit(),
            &name,
            port,
            v,
            &mac,
            c_int::from(keep_tag),
            &mut netif,
            &mut filter,
        ))?;
        Ok(Netif {
            netif: count(netif)?,
            filter: count(filter)?,
        })
    }

    /// Destroys a filter and / or a netif (`None`: none of that kind).
    pub fn netif_destroy(&mut self, netif: Option<u32>, filter: Option<u32>) -> Result<(), Error> {
        let n = netif.map_or(Ok(-1), c_value)?;
        let f = filter.map_or(Ok(-1), c_value)?;
        check(self.glue().netif_destroy(self.raw_unit(), n, f)).map(|_| ())
    }

    /// Replaces a netif's RX filter by one for `port` with `keep_tag`; `old` is destroyed after the
    /// new one exists. The new filter's id.
    pub fn netif_filter_set(&mut self, netif: u32, port: i32, keep_tag: bool, old: Option<u32>) -> Result<u32, Error> {
        let n = c_value(netif)?;
        let mut filter = old.map_or(Ok(-1), c_value)?;
        check(
            self.glue()
                .netif_filter_set(self.raw_unit(), n, port, c_int::from(keep_tag), &mut filter),
        )?;
        count(filter)
    }
}

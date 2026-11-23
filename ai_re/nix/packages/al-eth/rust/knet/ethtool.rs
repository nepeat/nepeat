// SPDX-License-Identifier: GPL-2.0

//! The ethtool calls a simple NIC driver answers (`include/linux/ethtool.h`).

use super::netdev::{NetDev, Ops};
use core::marker::PhantomData;
use kernel::{bindings, error::from_result, ffi::c_int};

/// `struct ethtool_drvinfo` being filled by [`Ops::get_drvinfo`].
pub(crate) struct DrvInfo<'a>(&'a mut bindings::ethtool_drvinfo);

fn put_str(dst: &mut [kernel::ffi::c_char], s: &str) {
    // Truncate and always NUL-terminate, like strscpy().
    let n = s.len().min(dst.len().saturating_sub(1));
    for (d, b) in dst.iter_mut().zip(s.as_bytes()[..n].iter()) {
        *d = *b as kernel::ffi::c_char;
    }
    if let Some(z) = dst.get_mut(n) {
        *z = 0;
    }
}

impl DrvInfo<'_> {
    pub(crate) fn set_driver(&mut self, s: &str) {
        put_str(&mut self.0.driver, s)
    }
    pub(crate) fn set_version(&mut self, s: &str) {
        put_str(&mut self.0.version, s)
    }
    pub(crate) fn set_bus_info(&mut self, s: &str) {
        put_str(&mut self.0.bus_info, s)
    }
}

/// Port types (`PORT_*`).
#[derive(Clone, Copy)]
pub(crate) enum Port {
    Tp,
    Mii,
    Fibre,
    Other,
}

/// `struct ethtool_link_ksettings` being filled by [`Ops::get_link_ksettings`].
pub(crate) struct LinkKsettings<'a>(&'a mut bindings::ethtool_link_ksettings);

impl LinkKsettings<'_> {
    /// Speed in Mb/s (`SPEED_UNKNOWN` = `u32::MAX`), full duplex or not.
    pub(crate) fn set_speed_duplex(&mut self, speed: u32, full: bool) {
        self.0.base.speed = speed;
        self.0.base.duplex = if full { bindings::DUPLEX_FULL } else { bindings::DUPLEX_HALF } as u8;
    }
    pub(crate) fn set_port(&mut self, port: Port) {
        self.0.base.port = match port {
            Port::Tp => bindings::PORT_TP,
            Port::Mii => bindings::PORT_MII,
            Port::Fibre => bindings::PORT_FIBRE,
            Port::Other => bindings::PORT_OTHER,
        } as u8;
    }
    pub(crate) fn set_autoneg(&mut self, on: bool) {
        self.0.base.autoneg = if on { bindings::AUTONEG_ENABLE } else { bindings::AUTONEG_DISABLE } as u8;
    }
    /// Adds link mode `bit` (`bindings::ethtool_link_mode_bit_indices_*`) to the
    /// supported and advertised sets; out-of-range bits are ignored.
    pub(crate) fn add_link_mode(&mut self, bit: u32) {
        let w = (bit / usize::BITS) as usize;
        let m = 1usize << (bit % usize::BITS);
        let modes = &mut self.0.link_modes;
        if let (Some(s), Some(a)) = (modes.supported.get_mut(w), modes.advertising.get_mut(w)) {
            *s |= m as kernel::ffi::c_ulong;
            *a |= m as kernel::ffi::c_ulong;
        }
    }
}

/// Interrupt coalescing (`ethtool -c/-C`): the subset a driver supports.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Coalesce {
    pub(crate) rx_usecs: u32,
    pub(crate) tx_usecs: u32,
    pub(crate) adaptive_rx: bool,
}

/// What [Coalesce] carries, as `ethtool_ops.supported_coalesce_params`
/// (include/linux/ethtool.h ETHTOOL_COALESCE_RX_USECS BIT(0), _TX_USECS BIT(4),
/// _USE_ADAPTIVE_RX BIT(9): #defines bindgen does not bind).
const COALESCE_PARAMS: u32 = 1 << 0 | 1 << 4 | 1 << 9;

/// Ring sizes (`ethtool -g`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Rings {
    pub(crate) rx_max: u32,
    pub(crate) rx: u32,
    pub(crate) tx_max: u32,
    pub(crate) tx: u32,
}

/// # Safety (of the callbacks)
///
/// Called by the ethtool core with a device registered by `Registration<T>` and
/// exclusive, valid argument structs for the duration of the call (RTNL held).
unsafe extern "C" fn get_drvinfo<T: Ops>(dev: *mut bindings::net_device, info: *mut bindings::ethtool_drvinfo) {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    // SAFETY: see above.
    T::get_drvinfo(d, &mut DrvInfo(unsafe { &mut *info }))
}

unsafe extern "C" fn get_link<T: Ops>(dev: *mut bindings::net_device) -> u32 {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    u32::from(T::get_link(d))
}

unsafe extern "C" fn get_link_ksettings<T: Ops>(
    dev: *mut bindings::net_device,
    ks: *mut bindings::ethtool_link_ksettings,
) -> c_int {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    // SAFETY: see above (the core zeroes `ks` before the call).
    let ks = unsafe { &mut *ks };
    from_result(|| T::get_link_ksettings(d, &mut LinkKsettings(ks)).map(|()| 0))
}

unsafe extern "C" fn get_coalesce<T: Ops>(
    dev: *mut bindings::net_device,
    ec: *mut bindings::ethtool_coalesce,
    _kec: *mut bindings::kernel_ethtool_coalesce,
    _extack: *mut bindings::netlink_ext_ack,
) -> c_int {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    // SAFETY: see above (a struct of u32 fields from the core, zeroed before the call).
    let ec = unsafe { &mut *ec };
    from_result(|| {
        let mut c = Coalesce::default();
        T::get_coalesce(d, &mut c)?;
        ec.rx_coalesce_usecs = c.rx_usecs;
        ec.tx_coalesce_usecs = c.tx_usecs;
        ec.use_adaptive_rx_coalesce = u32::from(c.adaptive_rx);
        Ok(0)
    })
}

unsafe extern "C" fn set_coalesce<T: Ops>(
    dev: *mut bindings::net_device,
    ec: *mut bindings::ethtool_coalesce,
    _kec: *mut bindings::kernel_ethtool_coalesce,
    _extack: *mut bindings::netlink_ext_ack,
) -> c_int {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    // SAFETY: see above (the wanted settings, current ones merged in by the core).
    let ec = unsafe { &*ec };
    let c = Coalesce { rx_usecs: ec.rx_coalesce_usecs, tx_usecs: ec.tx_coalesce_usecs, adaptive_rx: ec.use_adaptive_rx_coalesce != 0 };
    from_result(|| T::set_coalesce(d, &c).map(|()| 0))
}

unsafe extern "C" fn get_ringparam<T: Ops>(
    dev: *mut bindings::net_device,
    r: *mut bindings::ethtool_ringparam,
    _kr: *mut bindings::kernel_ethtool_ringparam,
    _extack: *mut bindings::netlink_ext_ack,
) {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    let mut g = Rings::default();
    T::get_ringparam(d, &mut g);
    // SAFETY: see above (a struct of u32 fields from the core).
    let r = unsafe { &mut *r };
    r.rx_max_pending = g.rx_max;
    r.rx_pending = g.rx;
    r.tx_max_pending = g.tx_max;
    r.tx_pending = g.tx;
}

unsafe extern "C" fn get_sset_count<T: Ops>(dev: *mut bindings::net_device, sset: c_int) -> c_int {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    if sset != bindings::ethtool_stringset_ETH_SS_STATS as c_int {
        return -(bindings::EOPNOTSUPP as c_int);
    }
    T::stats_names(d).len() as c_int
}

unsafe extern "C" fn get_strings<T: Ops>(dev: *mut bindings::net_device, sset: u32, data: *mut u8) {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    if sset != bindings::ethtool_stringset_ETH_SS_STATS {
        return;
    }
    let names = T::stats_names(d);
    let w = bindings::ETH_GSTRING_LEN as usize;
    // SAFETY: the core allocates ETH_GSTRING_LEN bytes per string of the count
    // get_sset_count gave for this stringset (the same names), u8: no alignment.
    let buf = unsafe { core::slice::from_raw_parts_mut(data, names.len() * w) };
    for (dst, name) in buf.chunks_exact_mut(w).zip(names) {
        let n = name.len().min(w - 1);
        dst[..n].copy_from_slice(&name.as_bytes()[..n]);
        dst[n..].fill(0);
    }
}

unsafe extern "C" fn get_ethtool_stats<T: Ops>(dev: *mut bindings::net_device, _s: *mut bindings::ethtool_stats, data: *mut u64) {
    // SAFETY: see above.
    let d = unsafe { NetDev::<T>::from_raw(dev) };
    let n = T::stats_names(d).len();
    // SAFETY: the core passes room for get_sset_count's number of u64 values,
    // from vzalloc (aligned), exclusively ours for the call.
    let out = unsafe { core::slice::from_raw_parts_mut(data, n) };
    T::get_stats(d, out)
}

/// The ethtool ops of `T`.
pub(super) const fn ops<T: Ops>() -> bindings::ethtool_ops {
    let _ = PhantomData::<T>;
    bindings::ethtool_ops {
        get_drvinfo: if T::HAS_GET_DRVINFO { Some(get_drvinfo::<T>) } else { None },
        get_link: if T::HAS_GET_LINK { Some(get_link::<T>) } else { Some(bindings::ethtool_op_get_link) },
        get_link_ksettings: if T::HAS_GET_LINK_KSETTINGS {
            Some(get_link_ksettings::<T>)
        } else {
            None
        },
        supported_coalesce_params: if T::HAS_SET_COALESCE { COALESCE_PARAMS } else { 0 },
        get_coalesce: if T::HAS_GET_COALESCE { Some(get_coalesce::<T>) } else { None },
        set_coalesce: if T::HAS_SET_COALESCE { Some(set_coalesce::<T>) } else { None },
        get_ringparam: if T::HAS_GET_RINGPARAM { Some(get_ringparam::<T>) } else { None },
        get_sset_count: if T::HAS_STATS_NAMES { Some(get_sset_count::<T>) } else { None },
        get_strings: if T::HAS_STATS_NAMES { Some(get_strings::<T>) } else { None },
        get_ethtool_stats: if T::HAS_STATS_NAMES { Some(get_ethtool_stats::<T>) } else { None },
        ..pin_init::zeroed()
    }
}

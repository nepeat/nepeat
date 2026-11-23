//! The full switch bring-up (`ob_sdk_init`: "init all" + the optional rc file), the handle it
//! returns, and the switch-wide calls: chip name, table usage, chip limits, KNET cleanup.

use std::ffi::{CString, c_int};
use std::ops::{Deref, DerefMut};
use std::path::Path;

use sdk_glue_sys as sys;

use crate::error::{Error, check};
use crate::glue::Glue;
use crate::link::Callbacks;
use crate::unit::{Boot, InitError, Layer, Sdk, bring_up, c_path, message_text};

/// Size of the buffer for the chip name (the C++ backend's `char b[64]`).
const HW_INFO_LEN: usize = 64;

/// The switch chip after the full bring-up: the BCM layer is up, so everything of the glue can
/// be used. Derefs to the [`Sdk`] it owns (MDIO, temperatures).
///
/// One per SDK at a time, `Send` but not `Sync`, as [`Sdk`]. Calls that change the chip take
/// `&mut self`; reads take `&self`, which cannot be shared between threads either. For other
/// threads: [`crate::MdioBus`], [`crate::TempReader`], and the callbacks ([`crate::link`]).
///
/// Dropping it removes the port gates and the link / L2 callbacks, drops the closures, stops
/// its handles, and then shuts the BCM layer down (`ob_sdk_shutdown`). As with the C++ backend
/// the SDK itself stays attached and the process keeps the unit's SDK lock until it ends; a new
/// [`Switch::init`] re-runs "init all".
#[derive(Debug)]
pub struct Switch {
    sdk: Sdk,
    pub(crate) cb: Callbacks,
}

/// Free entries of the forwarding tables (`ob_table_usage_t`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names, reason = "the glue's names")]
pub struct TableUsage {
    /// LPM, IPv4 prefixes.
    pub v4_routes_free: u32,
    /// LPM, IPv6 prefixes up to /64.
    pub v6_routes_free: u32,
    /// Host table in IPv4 entries (an IPv6 host takes 2).
    pub hosts_free: u32,
    /// L3 egress objects.
    pub egress_free: u32,
    /// L2 table.
    pub fdb_free: u32,
    /// ECMP groups.
    pub ecmp_groups_free: u32,
    /// ECMP member entries.
    pub ecmp_members_free: u32,
}

/// Fixed properties of the chip (`ob_chip_limits_t`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChipLimits {
    /// Packet buffer in bytes (Tomahawk: MMU cells x cell size).
    pub buffer_bytes: u64,
    /// Lowest field entry priority.
    pub acl_prio_min: u32,
    /// Highest field entry priority.
    pub acl_prio_max: u32,
    /// Host table entries.
    pub max_host: u32,
    /// Route table entries.
    pub max_route: u32,
    /// L3 egress objects.
    pub max_nexthop: u32,
    /// L2 entries.
    pub max_l2: u32,
    /// ECMP groups.
    pub ecmp_groups: u32,
    /// Paths per ECMP group.
    pub ecmp_paths: u32,
    /// ECMP member entries across all groups.
    pub ecmp_members: u32,
    /// Highest VRF id.
    pub vrf_max: u32,
}

/// A count of the glue that cannot be negative.
pub(crate) fn count(value: c_int) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::INTERNAL)
}

/// A size of the glue where a negative one means "unknown": 0.
fn clamp(value: c_int) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

impl Switch {
    /// Brings the whole switch up (`ob_sdk_init`): SDK boot and attach, "init all" with the SDK
    /// config file `config`, then the diag-shell script `rc` if one is given, stale KNET interfaces
    /// removed, packet receive started, L3 enabled on the ports.
    /// **This resets the switch chip**; ports are enabled afterwards as "init all" leaves them.
    ///
    /// On success: the handle and the glue's summary line. On failure nothing is registered
    /// and no handle exists, but what the SDK did up to the failing step is not undone (there
    /// is no detach): the process may hold the unit's SDK lock and SDK threads until it ends.
    /// [`InitError::BadPath`]: `config` or `rc` holds a NUL byte; nothing was called. The glue
    /// this program links (in a build with only the feature `rust-fake`: the process's Rust fake).
    pub fn init(unit: i32, config: &Path, rc: Option<&Path>) -> Result<(Self, String), InitError> {
        Self::init_with(Glue::process(), unit, config, rc)
    }

    /// [`Switch::init`] on a Rust fake chip: its scenario is the file `config`.
    #[cfg(feature = "rust-fake")]
    pub fn init_on(
        chip: &crate::rust_fake::Chip,
        unit: i32,
        config: &Path,
        rc: Option<&Path>,
    ) -> Result<(Self, String), InitError> {
        Self::init_with(Glue::Fake(chip.clone()), unit, config, rc)
    }

    fn init_with(glue: Glue, unit: i32, config: &Path, rc: Option<&Path>) -> Result<(Self, String), InitError> {
        let config_c = c_path(config)?;
        // The glue takes "" (or NULL) for "no rc file".
        let rc = rc.map(c_path).transpose()?.unwrap_or_default();
        let (core, message) = bring_up(glue, unit, Layer::Bcm, Boot::Config(config.to_path_buf()), |g, err| {
            g.init(unit, &config_c, &rc, err)
        })?;
        Ok((
            Self {
                sdk: Sdk::from_core(core),
                cb: Callbacks::new(),
            },
            message,
        ))
    }

    /// The chip's name as the SDK prints it (`ob_sdk_hw_info`), e.g. `BCM56960_B1`.
    pub fn hw_info(&self) -> Result<String, Error> {
        let mut buf = [0u8; HW_INFO_LEN];
        check(self.glue().hw_info(self.raw_unit(), &mut buf))?;
        Ok(message_text(&buf))
    }

    /// Free entries of the forwarding tables (`ob_sdk_table_usage`).
    pub fn table_usage(&self) -> Result<TableUsage, Error> {
        let mut u = sys::ob_table_usage_t {
            v4_routes_free: 0,
            v6_routes_free: 0,
            hosts_free: 0,
            egress_free: 0,
            fdb_free: 0,
            ecmp_groups_free: 0,
            ecmp_members_free: 0,
        };
        check(self.glue().table_usage(self.raw_unit(), &mut u))?;
        Ok(TableUsage {
            v4_routes_free: u.v4_routes_free,
            v6_routes_free: u.v6_routes_free,
            hosts_free: u.hosts_free,
            egress_free: u.egress_free,
            fdb_free: u.fdb_free,
            ecmp_groups_free: u.ecmp_groups_free,
            ecmp_members_free: u.ecmp_members_free,
        })
    }

    /// The chip's limits (`ob_sdk_chip_limits`): Tomahawk only ([`crate::Code::Unavail`] else).
    pub fn chip_limits(&self) -> Result<ChipLimits, Error> {
        let mut l = sys::ob_chip_limits_t {
            buffer_bytes: 0,
            acl_prio_min: 0,
            acl_prio_max: 0,
            max_host: 0,
            max_route: 0,
            max_nexthop: 0,
            max_l2: 0,
            ecmp_groups: 0,
            ecmp_paths: 0,
            ecmp_members: 0,
            vrf_max: 0,
        };
        check(self.glue().chip_limits(self.raw_unit(), &mut l))?;
        Ok(ChipLimits {
            buffer_bytes: l.buffer_bytes,
            acl_prio_min: l.acl_prio_min,
            acl_prio_max: l.acl_prio_max,
            // A negative size (the SDK's "unknown") is 0, as the C++ backend clamps them.
            max_host: clamp(l.max_host),
            max_route: clamp(l.max_route),
            max_nexthop: clamp(l.max_nexthop),
            max_l2: clamp(l.max_l2),
            ecmp_groups: clamp(l.ecmp_groups),
            ecmp_paths: clamp(l.ecmp_paths),
            ecmp_members: clamp(l.ecmp_members),
            vrf_max: clamp(l.vrf_max),
        })
    }

    /// Destroys the KNET netifs and filters left from an earlier SDK process (they survive it),
    /// keeping the SDK's own default filter (`ob_sdk_knet_cleanup`; the bring-up does it once).
    /// The numbers of netifs and filters destroyed.
    pub fn knet_cleanup(&mut self) -> Result<(u32, u32), Error> {
        let (mut netifs, mut filters) = (0, 0);
        check(self.glue().knet_cleanup(self.raw_unit(), &mut netifs, &mut filters))?;
        Ok((count(netifs)?, count(filters)?))
    }

    /// The unit as the glue takes it. For the area modules.
    pub(crate) const fn raw_unit(&self) -> c_int {
        self.sdk.raw_unit()
    }

    /// The glue. For the area modules.
    pub(crate) const fn glue(&self) -> &Glue {
        self.sdk.glue()
    }
}

/// A string for the glue: `OB_E_PARAM` for one with a NUL byte or longer than `max` bytes (the
/// glue would cut it short).
pub(crate) fn c_text(text: &str, max: usize) -> Result<CString, Error> {
    if text.len() > max {
        return Err(Error::PARAM);
    }
    CString::new(text).map_err(|_| Error::PARAM)
}

impl Deref for Switch {
    type Target = Sdk;

    fn deref(&self) -> &Sdk {
        &self.sdk
    }
}

impl DerefMut for Switch {
    fn deref_mut(&mut self) -> &mut Sdk {
        &mut self.sdk
    }
}

impl Drop for Switch {
    fn drop(&mut self) {
        // Before the Sdk field is dropped (its handles stop, then ob_sdk_shutdown): no callback
        // of this handle outlives it, and the closures (with what they own) go.
        self.unregister_all();
    }
}

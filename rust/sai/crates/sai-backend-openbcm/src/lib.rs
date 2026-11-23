//! The hardware backend of the Rust SAI: [`sai_core::backend::Backend`] over the Broadcom SDK,
//! through sai-openbcm's C glue (`sdk_glue.h`) as wrapped by `sdk-glue`. A port of the C++
//! `openbcm_backend.cpp` (`OpenbcmBackend`, 72 overrides): every operation it implements makes
//! the same glue calls, in the same order, with the same arguments; what it does not implement
//! (`QoS`, mirror, sampling, generic netlink, hash, storm and flood control, `trap_program`, the
//! L3 and field-processor extras, counters by id, PFC) stays at the trait defaults
//! (`NotSupported` / `false`), as there. No `unsafe` here: all of it is in `sdk-glue`.
//!
//! # Profile
//!
//! The keys of the C++ backend with the same meaning, so one `sai.profile` serves both
//! (the board quirk keys `SAI_OPENBCM_PORT_*` are the SAI layer's, `sai_core::keys`):
//!
//! | key | meaning |
//! |---|---|
//! | `SAI_INIT_CONFIG_FILE` | the SDK's `config.bcm`. Required and must be a readable file: the SDK would otherwise start with its compiled-in defaults |
//! | `SAI_OPENBCM_RC_FILE` | diag-shell script run after "init all" (LED programs); optional |
//! | `SAI_OPENBCM_UNIT` | SDK unit, default 0. Strict: a value that is not a number in 0..=255 is `INVALID_PARAMETER` (C++: `atoi`, so a typo is unit 0); empty is the default |
//!
//! # What init does (as the C++)
//!
//! 1. Checks the profile (nothing has touched the chip when a value is refused).
//! 2. `ob_sdk_init`: **resets the switch chip** ("init all"), runs the rc file, destroys stale
//!    KNET netifs and filters, starts packet receive, registers its link scan handler.
//! 3. One `ob_sdk_register_callbacks`: link changes ([`BackendEvent::Link`]) and L2 learn / age
//!    events ([`BackendEvent::Fdb`]) are queued on the [`EventSink`] from the SDK's threads;
//!    nothing else happens there.
//! 4. `ob_sdk_l3_egress_special` for the CPU, then the drop egress object: cached here (never
//!    destroyed, handed out by `l3_egress_cpu` / `l3_egress_drop`).
//! 5. `ob_sdk_chip_limits` once, logged only.
//!
//! Nothing here disables, enables or configures a port: the SAI layer disables every
//! front-panel port and applies the board quirks right after init (`sai_core::switch`,
//! `build_objects`, as the C++ `switch_build_objects`).
//!
//! # Differences from the C++ backend
//!
//! The glue-call sequences differ only here (each also in the item's documentation):
//!
//! * **A failed init after `ob_sdk_init` shuts the BCM layer down** (the dropped
//!   `sdk_glue::Switch`: `ob_sdk_shutdown`); the C++ leaves the SDK initialised. A refused `ob_sdk_register_callbacks` is followed by an
//!   `ob_sdk_unregister_callbacks` (the glue keeps the pointers it was given).
//! * **[`Backend::shutdown`]** unregisters a link gate still registered
//!   (`ob_sdk_linkscan_port_gate(.., NULL)` per such port) before `ob_sdk_shutdown`; the C++
//!   calls `ob_sdk_shutdown` alone (its ext-PHY detach, like the Rust one, has removed the gates
//!   by then, so in practice both make the one call; the glue-call differential shows it).
//! * Before init (and after shutdown) every operation answers `UNINITIALIZED` without a glue
//!   call; the C++ calls the glue with unit 0, which answers `OB_E_INIT` (= `UNINITIALIZED`) for
//!   most. [`Backend::cpu_port`] answers port 0 there (C++: -1), [`Backend::ports`] none.
//!   The C++ keeps its field entries, policer tables and CPU / drop egress ids across a
//!   shutdown; here they go with it (a new init starts empty).
//! * A second init while the chip is up is refused (`FAILURE`); the C++ runs `ob_sdk_init` again.
//! * Arguments `sdk-glue` refuses before the glue (`OB_E_PARAM` / `OB_E_PORT` without a call):
//!   numbers above `i32::MAX` where the glue takes an `int` (the C++ casts), a netif name
//!   longer than 15 bytes or a shell command longer than 1023 (the glue cuts them), a NUL
//!   byte in a string (the C++ passes the part before it), an ECMP group with no or more than
//!   256 members, a trunk with more than 64.
//! * Results `sdk-glue` checks: a port with 0 lanes has one lane here (C++: none); a
//!   negative number where the glue promises a count fails the call (`FAILURE`); a port
//!   number that does not fit the SAI's 16 bits is left out of [`Backend::ports`] and its events
//!   are dropped (logged).
//! * L2 events with arguments out of range (VLAN outside 1..=4094, negative port, flags other
//!   than 0 / 1) are dropped by `sdk-glue`'s trampoline; the C++ passes them on.
//! * A field rule with both an IPv4 and an IPv6 address on one side: the C++ keeps what the
//!   last of the two SAI attributes wrote (the IPv4 address overwrites only the first 4 bytes);
//!   here the IPv6 address with the IPv4 address over its first 4 bytes, as when the IPv4
//!   attribute comes last.
//! * Messages go to stderr, one line each (C++: syslog). A failed glue call is logged as the
//!   C++ `RV()` logs it (the call's text and the SDK's message), except `OB_E_UNAVAIL`, which
//!   the C++ logs at info level and this backend not at all.
#![forbid(unsafe_code)]

mod field;
mod ops;
pub mod status;

use sai_core::backend::types::{Dest, EgressId, FdbChange, FdbEvent, MacAddr, TrunkId};
use sai_core::backend::{Backend, BackendEvent, EventSink, HwPort, PortInfo, Profile, Vid};
use sai_core::error::{Error, Result};
use sdk_glue::{Code, InitError, PolicerCfg, Switch};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// `SAI_INIT_CONFIG_FILE` (SAI's own key, `SAI_KEY_INIT_CONFIG_FILE`): the SDK config file.
pub const CONFIG_KEY: &str = "SAI_INIT_CONFIG_FILE";
/// `SAI_OPENBCM_RC_FILE`: diag-shell script run after "init all".
pub const RC_KEY: &str = "SAI_OPENBCM_RC_FILE";
/// `SAI_OPENBCM_UNIT`: the SDK unit, default 0.
pub const UNIT_KEY: &str = "SAI_OPENBCM_UNIT";

/// The profile keys this backend reads (the rim asks the SAI user for exactly these, besides
/// `sai_core::keys::EXPLICIT`, which has them too).
pub const PROFILE_KEYS: [&str; 3] = [CONFIG_KEY, RC_KEY, UNIT_KEY];

/// bcm-knet's procfs file of netdev carriers: `<ifname>=up|down` sets one
/// ([`OpenbcmBackend::with_knet_link`] for another path).
pub const KNET_LINK: &str = "/proc/bcm/knet/link";

/// Which glue this build links: `real` (`sdk_glue.c`, or a C stub in its place), `rust-fake`
/// or `none` (then nothing that uses the backend links).
pub const GLUE: &str = sdk_glue::LINKED;

/// One line on stderr. The library is loaded into syncd, whose stderr is the journal.
pub(crate) fn log(message: &str) {
    // A closed stderr is no reason to fail a SAI call.
    let _ = writeln!(std::io::stderr().lock(), "sai-rs openbcm: {message}");
}

/// A glue call as the C++ `RV(call)` macro handles it: a failure is logged with the call's text
/// (`call`, the C++ source of it) and the SDK's message, except `OB_E_UNAVAIL` (the C++ logs that
/// at info level), and mapped by [`status::sai_error`].
pub(crate) fn rv<T>(call: &str, result: core::result::Result<T, sdk_glue::Error>) -> Result<T> {
    result.map_err(|error| {
        if error.code() != Code::Unavail {
            log(&format!("{call}: {error}"));
        }
        status::sai_error(error)
    })
}

/// A glue call the C++ maps without logging (`return map_rv(rv)`).
pub(crate) fn quiet<T>(result: core::result::Result<T, sdk_glue::Error>) -> Result<T> {
    result.map_err(status::sai_error)
}

/// A field entry this backend installed (C++ `Entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Its counter.
    pub(crate) stat: Option<u32>,
    /// The policer the SAI layer attached.
    pub(crate) policer: Option<u32>,
    /// The copy of that policer the entry meters with, when the SDK refused to share it.
    pub(crate) clone: Option<u32>,
}

/// The chip, up, and what the C++ backend keeps about it.
#[derive(Debug)]
pub(crate) struct Up {
    pub(crate) switch: Switch,
    /// `cpu_egress_` / `drop_egress_`, from init.
    pub(crate) cpu_egress: EgressId,
    pub(crate) drop_egress: EgressId,
    /// `entries_`: field entries by id.
    pub(crate) entries: BTreeMap<u32, Entry>,
    /// `policer_cfg_`: each policer's configuration as the glue got it.
    pub(crate) policer_cfg: BTreeMap<u32, PolicerCfg>,
    /// `policer_users_`: entries attached per policer.
    pub(crate) policer_users: BTreeMap<u32, u32>,
}

/// The backend over the Broadcom SDK. Creating one touches nothing; [`Backend::init`] brings
/// the chip up.
#[derive(Debug)]
pub struct OpenbcmBackend {
    up: Option<Up>,
    knet_link: PathBuf,
    /// The Rust fake chip to drive instead of the glue this program links.
    #[cfg(feature = "fake")]
    chip: Option<sdk_glue::rust_fake::Chip>,
}

impl Default for OpenbcmBackend {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) const fn hw(port: HwPort) -> i32 {
    // u16 into i32: lossless (i32::from is not const)
    port.0 as i32
}

/// The SAI's destination as the glue's.
pub(crate) fn glue_dest(dest: Dest) -> sdk_glue::Dest {
    match dest {
        Dest::Port(port) => sdk_glue::Dest::Port(hw(port)),
        Dest::Lag(tid) => sdk_glue::Dest::Lag(tid.0),
    }
}

/// An L2 event of the glue as the SAI layer's (C++ `on_l2`); `None` for a port number above
/// the SAI's 16 bits or a VLAN id the trampoline let through but `Vid` refuses (none).
fn fdb_event(event: sdk_glue::L2Event) -> Option<FdbEvent> {
    let hw_port = match event.dest {
        sdk_glue::Dest::Port(port) => Dest::Port(HwPort(u16::try_from(port).ok()?)),
        sdk_glue::Dest::Lag(tid) => Dest::Lag(TrunkId(tid)),
    };
    Some(FdbEvent {
        change: if event.learned {
            FdbChange::Learned
        } else {
            FdbChange::Aged
        },
        mac: MacAddr(event.mac),
        vid: Vid::new(event.vid).ok()?,
        hw_port,
    })
}

/// The profile's config file, checked as the C++ does before the SDK is started.
fn config_file(profile: &Profile) -> Result<&Path> {
    let Some(config) = profile.get(CONFIG_KEY).filter(|v| !v.is_empty()) else {
        log(&format!(
            "{CONFIG_KEY} not set in the profile; refusing to start the SDK with compiled-in defaults"
        ));
        return Err(Error::Failure);
    };
    let path = Path::new(config);
    // C++: access(R_OK). Opening it is the same question for a file; a directory opens but is
    // not a config file.
    let readable = path.is_file() && std::fs::File::open(path).is_ok();
    if !readable {
        log(&format!(
            "SDK config file {config} missing or unreadable; refusing to start"
        ));
        return Err(Error::Failure);
    }
    Ok(path)
}

/// `SAI_OPENBCM_UNIT`: absent or empty 0, else a number in 0..=255.
fn unit(profile: &Profile) -> Result<i32> {
    let Some(text) = profile.get(UNIT_KEY).filter(|v| !v.trim().is_empty()) else {
        return Ok(0);
    };
    text.trim().parse::<u8>().map(i32::from).map_err(|_| {
        log(&format!("{UNIT_KEY} '{text}': not a unit number (0..255)"));
        Error::InvalidParameter
    })
}

impl OpenbcmBackend {
    /// A backend that is not initialised, on the glue this program links (with only the feature
    /// `fake`: the process's Rust fake chip, `sdk_glue::rust_fake::process`).
    pub fn new() -> Self {
        Self {
            up: None,
            knet_link: PathBuf::from(KNET_LINK),
            #[cfg(feature = "fake")]
            chip: None,
        }
    }

    /// A backend that is not initialised, on the Rust fake chip `chip` (the profile's
    /// `SAI_INIT_CONFIG_FILE` is then its scenario, `sdk_glue::rust_fake`). Any number of them can
    /// run in one process, each on its own chip: what the tests and the glue-call differential
    /// use. Keep a clone of `chip` to inspect it.
    #[cfg(feature = "fake")]
    pub fn on_chip(chip: sdk_glue::rust_fake::Chip) -> Self {
        Self {
            chip: Some(chip),
            ..Self::new()
        }
    }

    /// The same backend writing netdev carriers ([`sai_core::backend::Hostif::netif_carrier_set`])
    /// to `path` instead of [`KNET_LINK`] (tests).
    #[must_use]
    pub fn with_knet_link(mut self, path: impl Into<PathBuf>) -> Self {
        self.knet_link = path.into();
        self
    }

    /// The chip's name as the SDK prints it (`ob_sdk_hw_info`, e.g. `BCM56960_B1`); `None` before
    /// init. The same as [`sai_core::backend::Base::hardware_info`].
    pub fn hardware_info(&self) -> Option<String> {
        self.up.as_ref().map(|up| up.switch.hw_info().unwrap_or_default())
    }

    pub(crate) fn up(&self) -> Result<&Up> {
        self.up.as_ref().ok_or(Error::Uninitialized)
    }

    pub(crate) fn up_mut(&mut self) -> Result<&mut Up> {
        self.up.as_mut().ok_or(Error::Uninitialized)
    }

    pub(crate) fn switch(&mut self) -> Result<&mut Switch> {
        self.up_mut().map(|up| &mut up.switch)
    }

    pub(crate) fn knet_link(&self) -> &Path {
        &self.knet_link
    }

    /// `ob_sdk_init` on this backend's glue.
    #[cfg_attr(
        not(feature = "fake"),
        allow(clippy::unused_self, reason = "the chip is a field with the feature fake")
    )]
    fn bring_up(
        &self,
        unit: i32,
        config: &Path,
        rc: Option<&Path>,
    ) -> core::result::Result<(Switch, String), InitError> {
        #[cfg(feature = "fake")]
        if let Some(chip) = &self.chip {
            return Switch::init_on(chip, unit, config, rc);
        }
        Switch::init(unit, config, rc)
    }

    /// Steps 3 to 5 of the bring-up on a chip that is up (the C++ order). On an error the caller
    /// drops `switch`, which unregisters and shuts the BCM layer down.
    fn after_sdk_init(mut switch: Switch, events: EventSink) -> Result<Up> {
        let link_events = events.clone();
        rv(
            "ob_sdk_register_callbacks(unit_, &OpenbcmBackend::on_link, &OpenbcmBackend::on_l2, this)",
            switch.set_callbacks(
                // From SDK threads: only queue.
                move |port, up| match u16::try_from(port) {
                    Ok(port) => link_events.send(BackendEvent::Link { hw: HwPort(port), up }),
                    Err(_) => log(&format!("link event of port {port}: out of the SAI's range, dropped")),
                },
                move |event| match fdb_event(event) {
                    Some(event) => events.send(BackendEvent::Fdb(event)),
                    None => log(&format!("L2 event {event:?}: out of the SAI's range, dropped")),
                },
            ),
        )?;
        let cpu = rv("ob_sdk_l3_egress_special(unit_, 0, &cpu)", switch.l3_egress_cpu())?;
        let drop = rv("ob_sdk_l3_egress_special(unit_, 1, &drop)", switch.l3_egress_drop())?;
        if let Ok(lim) = switch.chip_limits() {
            log(&format!(
                "SDK tables: host {}, route {}, egress {}, L2 {}; buffer {} B; ECMP {} groups x {} paths, {} member entries; VRF ids up to {}",
                lim.max_host,
                lim.max_route,
                lim.max_nexthop,
                lim.max_l2,
                lim.buffer_bytes,
                lim.ecmp_groups,
                lim.ecmp_paths,
                lim.ecmp_members,
                lim.vrf_max
            ));
        }
        Ok(Up {
            switch,
            cpu_egress: EgressId(cpu),
            drop_egress: EgressId(drop),
            entries: BTreeMap::new(),
            policer_cfg: BTreeMap::new(),
            policer_users: BTreeMap::new(),
        })
    }
}

impl Backend for OpenbcmBackend {
    fn name(&self) -> &'static str {
        "openbcm"
    }

    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        if self.up.is_some() {
            log("init: the chip is already up");
            return Err(Error::Failure);
        }
        let config = config_file(profile)?;
        let rc = profile.get(RC_KEY).filter(|v| !v.is_empty()).map(Path::new);
        let unit = unit(profile)?;
        log(&format!(
            "SDK init: unit {unit}, config {}, rc {}",
            config.display(),
            rc.map_or_else(|| "(none)".to_owned(), |p| p.display().to_string())
        ));
        let (switch, message) = self.bring_up(unit, config, rc).map_err(|error| match error {
            InitError::Sdk { error, message } => {
                log(&format!("SDK init failed: {message} ({error})"));
                status::sai_error(error)
            }
            InitError::BadPath => {
                log("SDK init failed: a path holds a NUL byte");
                Error::InvalidParameter
            }
            InitError::AlreadyOpen => {
                log("SDK init failed: the SDK is already open in this process");
                Error::Failure
            }
        })?;
        log(&format!("SDK init: {message}"));
        // An error drops the switch inside: callbacks unregistered, BCM layer shut down.
        self.up = Some(Self::after_sdk_init(switch, events)?);
        Ok(())
    }

    fn shutdown(&mut self) {
        // Dropping the switch unregisters the gates and the callbacks (which drops the event
        // sinks the closures own), then calls ob_sdk_shutdown.
        self.up = None;
    }

    fn ports(&self) -> Vec<PortInfo> {
        let Some(up) = &self.up else {
            return Vec::new();
        };
        let raw = match up.switch.ports() {
            Ok(raw) => raw,
            Err(error) => {
                log(&format!("port config: {error}"));
                return Vec::new();
            }
        };
        let mut ports = Vec::with_capacity(raw.len());
        for port in raw {
            let Ok(number) = u16::try_from(port.port) else {
                log(&format!(
                    "port {}: logical number {} is out of the SAI's range",
                    port.name, port.port
                ));
                continue;
            };
            let autoneg = up.switch.port_autoneg_get(port.port).unwrap_or(false);
            ports.push(PortInfo {
                hw: HwPort(number),
                lanes: (0..port.lanes)
                    .map(|lane| port.first_lane.saturating_add(lane))
                    .collect(),
                speed: port.speed,
                name: port.name,
                autoneg,
            });
        }
        ports
    }

    fn cpu_port(&self) -> HwPort {
        // Before init there is no chip to ask; 0 is the CPU port of every chip the SDK runs here.
        self.up
            .as_ref()
            .and_then(|up| u16::try_from(up.switch.cpu_port()).ok())
            .map_or(HwPort(0), HwPort)
    }

    fn port_admin_set(&mut self, port: HwPort, up: bool) -> Result<()> {
        rv(
            "ob_sdk_port_enable_set(unit_, hw, up)",
            self.switch()?.port_enable_set(hw(port), up),
        )
    }

    fn port_link_get(&self, port: HwPort) -> Result<bool> {
        rv(
            "ob_sdk_port_link_get(unit_, hw, &u)",
            self.up()?.switch.port_link_get(hw(port)),
        )
    }

    fn port_speed_set(&mut self, port: HwPort, mbps: u32) -> Result<()> {
        rv(
            "ob_sdk_port_speed_set(unit_, hw, (int)mbps)",
            self.switch()?.port_speed_set(hw(port), mbps),
        )
    }

    fn port_speed_get(&self, port: HwPort) -> Result<u32> {
        rv(
            "ob_sdk_port_speed_get(unit_, hw, &s)",
            self.up()?.switch.port_speed_get(hw(port)),
        )
    }

    fn port_supported_speeds(&self, port: HwPort) -> Result<Vec<u32>> {
        // Not logged, as in the C++ (callers probe).
        quiet(self.up()?.switch.port_speeds(hw(port)))
    }

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        rv("ob_sdk_vlan_create(unit_, vid)", self.switch()?.vlan_create(vid.get()))
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        rv(
            "ob_sdk_vlan_destroy(unit_, vid)",
            self.switch()?.vlan_destroy(vid.get()),
        )
    }

    fn vlan_port_add(&mut self, vid: Vid, port: HwPort, untagged: bool) -> Result<()> {
        rv(
            "ob_sdk_vlan_port_add(unit_, vid, hw, untagged)",
            self.switch()?.vlan_port_add(vid.get(), hw(port), untagged),
        )
    }

    fn vlan_port_remove(&mut self, vid: Vid, port: HwPort) -> Result<()> {
        rv(
            "ob_sdk_vlan_port_remove(unit_, vid, hw)",
            self.switch()?.vlan_port_remove(vid.get(), hw(port)),
        )
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        // Not logged, as in the C++.
        let members = quiet(self.up()?.switch.vlan_members(vid.get()))?;
        members
            .into_iter()
            .map(|(port, untagged)| {
                u16::try_from(port)
                    .map(|p| (HwPort(p), untagged))
                    .map_err(|_| Error::Failure)
            })
            .collect()
    }

    fn temperature_reader(&self) -> Option<sai_core::temp::TempReader> {
        let reader = self.up.as_ref()?.switch.temp_reader();
        Some(std::sync::Arc::new(move || {
            reader.read().map(ops::asic_temps).map_err(status::sai_error)
        }))
    }

    fn mdio_bus(&self) -> Option<sai_core::backend::types::MdioBus> {
        let bus = self.up.as_ref()?.switch.mdio_bus();
        Some(std::sync::Arc::new(ops::Bus(bus)))
    }
}

// What openbcm_backend.cpp does not override: the trait defaults, as there.
sai_core::typed_defaults!(OpenbcmBackend: Qos, Mirror, Sample, Hash, Storm);

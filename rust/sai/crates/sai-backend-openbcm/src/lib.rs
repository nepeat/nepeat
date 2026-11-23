//! The hardware backend of the Rust SAI: [`sai_core::backend::Backend`] over the Broadcom SDK,
//! through sai-openbcm's C glue as wrapped by `sdk-glue`. The counterpart of the C++
//! `openbcm_backend.cpp` for the operations of the slice (switch bring-up, ports, VLANs, link
//! events). No `unsafe` here: all of it is in `sdk-glue`.
//!
//! # Profile
//!
//! The keys of the C++ backend with the same meaning, so one `sai.profile` serves both
//! (the Rust library additionally needs `SAI_RS_BACKEND=openbcm`):
//!
//! | key | meaning |
//! |---|---|
//! | `SAI_INIT_CONFIG_FILE` | the SDK's `config.bcm`. Required and must be a readable file: the SDK would otherwise start with its compiled-in defaults |
//! | `SAI_OPENBCM_RC_FILE` | diag-shell script run after "init all" (LED programs); optional |
//! | `SAI_OPENBCM_UNIT` | SDK unit, default 0 |
//! | `SAI_OPENBCM_PORT_INTERFACE`, `_FEC`, `_AUTONEG`, `_IFG` | fixed settings of every front-panel port ([`quirks`]) |
//!
//! # What init does
//!
//! 1. Reads and checks the profile (nothing has touched the chip when a value is refused).
//! 2. `ob_sdk_init`: **resets the switch chip** ("init all"), runs the rc file.
//! 3. Registers the link callback: link changes are queued on the [`EventSink`] from the
//!    SDK's thread; nothing else happens on that thread.
//! 4. Enumerates the front-panel ports and the CPU port.
//! 5. Disables every front-panel port, then applies the port settings of the profile.
//!    Nothing here enables a port by itself.
//!
//! A failure at any step after 2 unregisters the callback and shuts the BCM layer down again
//! before the error is returned. As in C the SDK stays attached to the process (there is no
//! detach) and the process keeps the unit's SDK lock until it ends.
//!
//! # Differences from the C++ backend
//!
//! Listed in the workspace README ("Hardware backend"); the ones a caller can see:
//! a profile value that does not parse is an error (C++: `atoi`), a port that cannot be
//! disabled or enumerated fails the bring-up (C++: logged), no L2 callback, no L3 egress
//! objects, no temperature files, no diag socket, messages go to stderr (C++: syslog).
#![forbid(unsafe_code)]

pub mod quirks;
pub mod status;

use quirks::PortQuirks;
use sai_core::backend::{Backend, BackendEvent, EventSink, HwPort, PortInfo, Profile, Vid};
use sai_core::error::{Error, Result};
use sdk_glue::{Code, InitError, Switch};
use std::io::Write as _;
use std::path::Path;

/// `SAI_INIT_CONFIG_FILE` (SAI's own key, `SAI_KEY_INIT_CONFIG_FILE`): the SDK config file.
pub const CONFIG_KEY: &str = "SAI_INIT_CONFIG_FILE";
/// `SAI_OPENBCM_RC_FILE`: diag-shell script run after "init all".
pub const RC_KEY: &str = "SAI_OPENBCM_RC_FILE";
/// `SAI_OPENBCM_UNIT`: the SDK unit, default 0.
pub const UNIT_KEY: &str = "SAI_OPENBCM_UNIT";

/// The profile keys this backend reads (the rim asks the SAI user for exactly these).
pub const PROFILE_KEYS: [&str; 7] = [
    CONFIG_KEY,
    RC_KEY,
    UNIT_KEY,
    quirks::INTERFACE_KEY,
    quirks::FEC_KEY,
    quirks::AUTONEG_KEY,
    quirks::IFG_KEY,
];

/// Which glue this build links: `real`, `fake` or `none` (then nothing that uses the backend
/// links).
pub const GLUE: &str = sdk_glue::LINKED;

/// One line on stderr. The library is loaded into syncd, whose stderr is the journal.
pub(crate) fn log(message: &str) {
    // A closed stderr is no reason to fail a SAI call.
    let _ = writeln!(std::io::stderr().lock(), "sai-rs openbcm: {message}");
}

/// The chip, up.
#[derive(Debug)]
struct Up {
    switch: Switch,
    ports: Vec<PortInfo>,
    cpu: HwPort,
    hardware: String,
}

/// The backend over the Broadcom SDK. Creating one touches nothing; [`Backend::init`] brings
/// the chip up.
#[derive(Debug, Default)]
pub struct OpenbcmBackend {
    up: Option<Up>,
}

fn hw(port: HwPort) -> i32 {
    i32::from(port.0)
}

/// Logs a failed glue call as the C++ `RV()` macro does and maps its code.
fn checked<T>(what: &str, result: core::result::Result<T, sdk_glue::Error>) -> Result<T> {
    result.map_err(|error| {
        if error.code() != Code::Unavail {
            log(&format!("{what}: {error}"));
        }
        status::sai_error(error)
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
    let readable = path.is_file() && std::fs::File::open(path).is_ok();
    if !readable {
        log(&format!(
            "SDK config file {config} missing or unreadable; refusing to start"
        ));
        return Err(Error::Failure);
    }
    Ok(path)
}

impl OpenbcmBackend {
    /// A backend that is not initialised.
    pub fn new() -> Self {
        Self::default()
    }

    /// The chip's name as the SDK prints it (e.g. `BCM56960_B1`); `None` before init.
    pub fn hardware_info(&self) -> Option<&str> {
        self.up.as_ref().map(|up| up.hardware.as_str())
    }

    fn up(&self) -> Result<&Up> {
        self.up.as_ref().ok_or(Error::Uninitialized)
    }

    fn switch(&mut self) -> Result<&mut Switch> {
        self.up.as_mut().map(|up| &mut up.switch).ok_or(Error::Uninitialized)
    }

    /// Steps 3 to 5 of the bring-up, on a chip that is up. On an error the caller drops
    /// `switch`, which undoes step 3 and shuts the BCM layer down.
    fn after_sdk_init(switch: &mut Switch, quirks: &PortQuirks, events: EventSink) -> Result<(Vec<PortInfo>, HwPort)> {
        checked(
            "link callback",
            switch.set_link_callback(move |port, up| {
                // From an SDK thread: only queue. A port number outside the SAI's range
                // cannot be one of the enumerated ports.
                if let Ok(port) = u16::try_from(port) {
                    events.send(BackendEvent::Link { hw: HwPort(port), up });
                }
            }),
        )?;
        let cpu = u16::try_from(switch.cpu_port()).map(HwPort).map_err(|_| {
            log("the CPU port's number is out of range");
            Error::Failure
        })?;
        let raw = checked("port config", switch.ports())?;
        if raw.is_empty() {
            log("the SDK reports no front-panel port (is the config.bcm the board's?)");
            return Err(Error::Failure);
        }
        let mut ports = Vec::with_capacity(raw.len());
        for port in &raw {
            let Ok(number) = u16::try_from(port.port) else {
                log(&format!(
                    "port {}: logical number {} is out of range",
                    port.name, port.port
                ));
                return Err(Error::Failure);
            };
            // Front-panel ports start admin down (SAI's default), whatever "init all" or the
            // rc script left; the board settings are applied to a port that is down.
            checked(
                &format!("port {}: disable", port.name),
                switch.port_enable_set(port.port, false),
            )?;
            if let Err((what, error)) = quirks.apply(switch, port.port) {
                log(&format!("port {}: quirk {what} not applied: {error}", port.name));
                return Err(status::sai_error(error));
            }
            let lanes = (0..port.lanes)
                .map(|lane| port.first_lane.saturating_add(lane))
                .collect();
            ports.push(PortInfo {
                hw: HwPort(number),
                lanes,
                speed: port.speed,
            });
        }
        Ok((ports, cpu))
    }
}

// The typed operations are not on this backend yet (G2): their defaults answer NotSupported.
sai_core::typed_defaults!(OpenbcmBackend: Base, Shell, Temp, Mdio, PortOps, Lag, L2, Hostif, L3, Field, Policer, Qos, Mirror, Sample, Hash, Storm);

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
        let unit = profile.number(UNIT_KEY, 0, 0..=255).inspect_err(|_| {
            log(&format!("{UNIT_KEY}: not a unit number"));
        })?;
        let unit = i32::try_from(unit).map_err(|_| Error::InvalidParameter)?;
        let quirks = PortQuirks::parse(profile)?;

        log(&format!(
            "SDK init: unit {unit}, config {}, rc {}",
            config.display(),
            rc.map_or("(none)".into(), |p| p.display().to_string())
        ));
        let (mut switch, message) = Switch::init(unit, config, rc).map_err(|error| {
            log(&format!("SDK init failed: {error}"));
            match error {
                InitError::Sdk { error, .. } => status::sai_error(error),
                InitError::BadPath => Error::InvalidParameter,
                InitError::AlreadyOpen => Error::Failure,
            }
        })?;
        log(&format!("SDK init: {message}"));
        if !quirks.is_empty() {
            log(&format!("port quirks: {quirks}"));
        }
        // An error drops `switch` here: callback unregistered, BCM layer shut down.
        let (ports, cpu) = Self::after_sdk_init(&mut switch, &quirks, events)?;
        let hardware = switch.hw_info().unwrap_or_default();
        log(&format!(
            "{hardware}: {} front-panel ports, CPU port {}",
            ports.len(),
            cpu.0
        ));
        self.up = Some(Up {
            switch,
            ports,
            cpu,
            hardware,
        });
        Ok(())
    }

    fn shutdown(&mut self) {
        // Dropping the switch unregisters the link callback (which drops the event sink the
        // closure owns) and then shuts the BCM layer down.
        self.up = None;
    }

    fn ports(&self) -> Vec<PortInfo> {
        self.up.as_ref().map(|up| up.ports.clone()).unwrap_or_default()
    }

    fn cpu_port(&self) -> HwPort {
        // Before init there is no chip to ask; 0 is the CPU port of every chip the SDK runs here.
        self.up.as_ref().map_or(HwPort(0), |up| up.cpu)
    }

    fn port_admin_set(&mut self, port: HwPort, up: bool) -> Result<()> {
        checked("port enable", self.switch()?.port_enable_set(hw(port), up))
    }

    fn port_link_get(&self, port: HwPort) -> Result<bool> {
        checked("port link", self.up()?.switch.port_link_get(hw(port)))
    }

    fn port_speed_set(&mut self, port: HwPort, mbps: u32) -> Result<()> {
        checked("port speed set", self.switch()?.port_speed_set(hw(port), mbps))
    }

    fn port_speed_get(&self, port: HwPort) -> Result<u32> {
        checked("port speed get", self.up()?.switch.port_speed_get(hw(port)))
    }

    fn port_supported_speeds(&self, port: HwPort) -> Result<Vec<u32>> {
        // Not logged, as in the C++ (callers probe).
        self.up()?.switch.port_speeds(hw(port)).map_err(status::sai_error)
    }

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        checked("vlan create", self.switch()?.vlan_create(vid.get()))
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        checked("vlan destroy", self.switch()?.vlan_destroy(vid.get()))
    }

    fn vlan_port_add(&mut self, vid: Vid, port: HwPort, untagged: bool) -> Result<()> {
        checked(
            "vlan port add",
            self.switch()?.vlan_port_add(vid.get(), hw(port), untagged),
        )
    }

    fn vlan_port_remove(&mut self, vid: Vid, port: HwPort) -> Result<()> {
        checked("vlan port remove", self.switch()?.vlan_port_remove(vid.get(), hw(port)))
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        // Not logged, as in the C++.
        let members = self.up()?.switch.vlan_members(vid.get()).map_err(status::sai_error)?;
        members
            .into_iter()
            .map(|(port, untagged)| {
                u16::try_from(port)
                    .map(|p| (HwPort(p), untagged))
                    .map_err(|_| Error::Failure)
            })
            .collect()
    }
}

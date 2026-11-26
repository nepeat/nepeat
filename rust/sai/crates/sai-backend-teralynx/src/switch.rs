//! Switch (`tl_switch.{h,cpp}`): the init sequence and shutdown, init commands, event callbacks,
//! temperatures, table usage, chip limits, the read-only diag shell.
//!
//! Safety rules (as the C++): no readable `SAI_INIT_CONFIG_FILE`, no port map: refuse to start;
//! every front-panel devport is set admin-down right after node enable, before the init command
//! files run, and the init command interpreter knows only `ifcs set devport` serdes / eq
//! attributes, so `port enable` lines in vendor files never enable a port; shutdown sets every
//! port admin-down before anything else.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ifcs::{Attrs, Callbacks, Handlers, Ifcs, sys};
use sai_core::backend::types::{AsicTemps, ChipLimits, TableUsage};
use sai_core::backend::{BackendEvent, EventSink, HwPort, Profile};
use sai_core::error::{Error, Result};
use sai_core::log;
use sai_sys as ssys;

use crate::acl::AclArea;
use crate::config::{self, InitCmds, ValKind};
use crate::core::{
    CAP_MAC, CAP_V4_HOST, CAP_V4_PREFIX, CAP_V6_PREFIX, CPU_DEVPORT, Core, ECMP_PATHS, NODE, VRF_MAX, speed_from_enum,
};
use crate::counters;
use crate::hostif::HostifArea;
use crate::keys::Settings;
use crate::l2::{self, L2Area};
use crate::l3::L3Area;
use crate::lag::LagArea;
use crate::port::PortArea;
use crate::sflow::SflowArea;

/// The diag shell's `help` (the C++ text, `regs` added).
pub const HELP: &str =
    "commands: ports | devport <id> | temp | copp | acl | counters <devport> | regs | help (read-only)\n";

/// What the post-init register list did at the last init.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegStats {
    /// Written and read back as wanted.
    pub applied: usize,
    /// Read back different under the mask.
    pub mismatched: usize,
    /// An access failed (or libifcs has no register accessors).
    pub failed: usize,
}

/// The SDK up, and what every area keeps about it.
#[derive(Debug)]
pub struct Up {
    /// Shared state.
    pub core: Core,
    /// Ports.
    pub port: PortArea,
    /// L2.
    pub l2: L2Area,
    /// LAG and hash.
    pub lag: LagArea,
    /// L3.
    pub l3: L3Area,
    /// Host interfaces.
    pub hostif: HostifArea,
    /// `sFlow`.
    pub sflow: SflowArea,
    /// ACL / `CoPP`.
    pub acl: AclArea,
    /// Temperatures (shared with the publisher's reader).
    pub temps: Arc<Temps>,
    limits: Mutex<Option<ChipLimits>>,
    /// The board's post-init register list (`SAI_TERALYNX_POST_INIT_REGS`).
    pub regs: Vec<config::RegWrite>,
    /// What applying it did at this init.
    pub reg_stats: RegStats,
    /// The board's diag watch list (`SAI_TERALYNX_DIAG_REGS`): read by `regs`, never written.
    pub watch: Vec<u32>,
    log_cb: Option<Callbacks>,
    event_cb: Option<Callbacks>,
    sdk_running: bool,
    /// `Core::initialized`: init completed.
    pub initialized: bool,
}

fn err(api: ssys::sai_api_t, message: core::fmt::Arguments<'_>) {
    log::error(api, message);
}

/// `SwitchArea::init`'s steps before the SDK: the profile, the files, the run-time config, the
/// library. `Err` is the status the C++ answers (the reason already logged).
#[allow(clippy::type_complexity, reason = "the init inputs in the C++ order, once")]
fn prepare(
    profile: &Profile,
) -> Result<(
    Settings,
    Vec<config::PortSpec>,
    InitCmds,
    (Vec<config::RegWrite>, Vec<u32>),
    String,
    Ifcs,
)> {
    let Some(settings) = Settings::from_profile(profile) else {
        err(
            ssys::SAI_API_SWITCH,
            format_args!("SAI_TERALYNX_NETDEV must be create|adopt"),
        );
        return Err(Error::InvalidParameter);
    };
    let s = &settings;
    if s.config.is_empty() || !config::readable(&s.config) {
        err(
            ssys::SAI_API_SWITCH,
            format_args!(
                "SAI_INIT_CONFIG_FILE '{}' not set or unreadable; refusing to start the SDK",
                s.config
            ),
        );
        return Err(Error::Failure);
    }
    if s.port_map.is_empty() {
        err(
            ssys::SAI_API_SWITCH,
            format_args!("SAI_TERALYNX_PORT_MAP not set; refusing to start"),
        );
        return Err(Error::Failure);
    }
    let specs = config::parse_port_map(&s.port_map).map_err(|e| {
        err(ssys::SAI_API_SWITCH, format_args!("port map: {e}"));
        Error::Failure
    })?;
    let mut ic = InitCmds::default();
    for f in s.init_cmds.split(':').filter(|f| !f.is_empty()) {
        config::parse_init_cmds(f, &mut ic).map_err(|e| {
            err(ssys::SAI_API_SWITCH, format_args!("init commands: {e}"));
            Error::Failure
        })?;
    }
    // The board's post-init register list (no C++ counterpart): refused before the SDK starts.
    let regs = if s.post_init_regs.is_empty() {
        Vec::new()
    } else {
        config::parse_regs(&s.post_init_regs).map_err(|e| {
            err(ssys::SAI_API_SWITCH, format_args!("post-init registers: {e}"));
            Error::Failure
        })?
    };
    // The board's diag watch list (no C++ counterpart): read only, refused like the list above.
    let watch = if s.diag_regs.is_empty() {
        Vec::new()
    } else {
        config::parse_diag_regs(&s.diag_regs).map_err(|e| {
            err(ssys::SAI_API_SWITCH, format_args!("diag registers: {e}"));
            Error::Failure
        })?
    };
    // Runtime config: the generated init yaml, then the board's isg yaml.
    let yaml = config::merged_config(&s.config, &s.append).map_err(|e| {
        err(ssys::SAI_API_SWITCH, format_args!("{e}"));
        Error::Failure
    })?;
    let _ = std::fs::create_dir(&s.run_dir); // EEXIST is fine; the write below reports real errors
    let run_config = format!("{}/config.yaml", s.run_dir);
    config::write_atomic(&run_config, &yaml).map_err(|e| {
        err(ssys::SAI_API_SWITCH, format_args!("{e}"));
        Error::Failure
    })?;
    // learn-ring read guard (the shim, learn-wedge.md): on unless SAI_TERALYNX_IFCS_GUARD=off
    let guard = ifcs::GuardOptions {
        enable: s.guard,
        stats_path: format!("{}/ifcs-guard.prom", s.run_dir),
        ring_size: s.ring_size,
        malloc_limit: s.malloc_limit,
    };
    let lib = ifcs::load(&s.libifcs, &s.trace, &guard).map_err(|e| {
        err(ssys::SAI_API_SWITCH, format_args!("{e}"));
        Error::Failure
    })?;
    Ok((settings, specs, ic, (regs, watch), run_config, lib))
}

/// `tl_switch.cpp` `on_log`: libifcs' log levels on the SAI's.
pub fn log_level(level: i32) -> ssys::sai_log_level_t {
    let level = u32::try_from(level).unwrap_or(0);
    if level <= sys::IFCS_LOG_LEVEL_ERROR {
        ssys::SAI_LOG_LEVEL_ERROR
    } else if level == sys::IFCS_LOG_LEVEL_WARN {
        ssys::SAI_LOG_LEVEL_WARN
    } else if level == sys::IFCS_LOG_LEVEL_INFO {
        ssys::SAI_LOG_LEVEL_INFO
    } else {
        ssys::SAI_LOG_LEVEL_DEBUG
    }
}

impl Up {
    /// `SwitchArea::init`: everything the C++ does at `create_switch`, in its order. On an error
    /// the SDK is deinitialised again where the C++ does it (and nothing is left registered here).
    pub fn init(profile: &Profile, events: EventSink) -> Result<Self> {
        let (settings, specs, ic, (regs, watch), run_config, lib) = prepare(profile)?;
        let mut up = Self {
            core: Core::new(lib, settings, specs),
            port: PortArea::new(),
            l2: L2Area::default(),
            lag: LagArea::default(),
            l3: L3Area::default(),
            hostif: HostifArea::default(),
            sflow: SflowArea::default(),
            acl: AclArea::default(),
            temps: Arc::new(Temps::new(None, 15, 5)),
            limits: Mutex::new(None),
            regs,
            reg_stats: RegStats::default(),
            watch,
            log_cb: None,
            event_cb: None,
            sdk_running: false,
            initialized: false,
        };
        up.hostif.configure(up.core.settings.netdev);
        up.temps = Arc::new(Temps::new(
            Some(up.core.lib.clone()),
            up.core.settings.temp_period,
            up.core.settings.temp_monitors,
        ));
        up.start(&ic, &run_config, events)?;
        Ok(up)
    }

    fn start(&mut self, ic: &InitCmds, run_config: &str, events: EventSink) -> Result<()> {
        let s = &self.core.settings;
        // Exactly the vendor's innovium.user (main): init, node create, enable.
        log::notice(
            ssys::SAI_API_SWITCH,
            format_args!("ifcs_init({run_config}, {}, COLD) via {}", s.log_name, s.libifcs),
        );
        let (Ok(cfg), Ok(name)) = (
            std::ffi::CString::new(run_config),
            std::ffi::CString::new(s.log_name.as_str()),
        ) else {
            err(
                ssys::SAI_API_SWITCH,
                format_args!("ifcs_init failed: a path holds a NUL byte"),
            );
            return Err(Error::InvalidParameter);
        };
        let (rc, nodes) = self.core.lib.ifcs_init(&cfg, &name, sys::IFCS_BOOT_COLD.cast_signed());
        if !rc.is_success() {
            err(ssys::SAI_API_SWITCH, format_args!("ifcs_init failed: {}", rc.name()));
            return Err(ifcs::status::sai_error(rc.reason()));
        }
        self.sdk_running = true;
        if nodes < 1 {
            err(ssys::SAI_API_SWITCH, format_args!("ifcs_init found no switch node"));
            self.deinit();
            return Err(Error::Failure);
        }
        let log_cb = Callbacks::new(Handlers {
            log: Some(Arc::new(|level, msg| {
                log::emit(ssys::SAI_API_SWITCH, log_level(level), format_args!("ifcs: {msg}"));
            })),
            ..Handlers::default()
        });
        if self.core.lib.ifcs_log_function_register(&log_cb).0 != 0 {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!("ifcs_log_function_register failed: SDK messages stay in its own log file"),
            );
        }
        self.log_cb = Some(log_cb);
        let rc = self.core.lib.ifcs_node_create(NODE, &mut Attrs::null());
        if !rc.is_success() {
            err(
                ssys::SAI_API_SWITCH,
                format_args!("ifcs_node_create(0) failed: {}", rc.name()),
            );
            self.deinit();
            return Err(ifcs::status::sai_error(rc.reason()));
        }
        L2Area::pre_enable(&self.core); // node attributes that must precede ENABLE
        self.l3.pre_enable(&self.core); // L3 interface counter modes
        SflowArea::pre_enable(&self.core); // egress sampling, a boot-time node attribute
        let mut en = ifcs::attr::zeroed(0);
        let _ = self.core.lib.ifcs_attr_t_init(&mut en);
        let _ = self.core.lib.ifcs_attr_t_id_set(&mut en, sys::IFCS_NODE_ATTR_ENABLE);
        let _ = self.core.lib.ifcs_attr_t_value_data_set(&mut en, true);
        let rc = self.core.lib.ifcs_node_attr_set(NODE, &mut Attrs::from_raw(en));
        if !rc.is_success() {
            err(ssys::SAI_API_SWITCH, format_args!("node enable failed: {}", rc.name()));
            self.deinit();
            return Err(ifcs::status::sai_error(rc.reason()));
        }
        if let Err(e) = self.post_enable(ic, events) {
            PortArea::all_down(&self.core);
            self.deinit();
            return Err(e);
        }
        self.initialized = true;
        self.temps.initialized.store(true, Ordering::SeqCst);
        log::notice(
            ssys::SAI_API_SWITCH,
            format_args!(
                "IFCS node 0 enabled: {} front-panel devports, {} init commands applied",
                self.core.specs.len(),
                ic.cmds.len()
            ),
        );
        Ok(())
    }

    /// `SwitchArea::post_enable`: sysport handles, ports down, init commands, event callbacks,
    /// the L2 base.
    fn post_enable(&mut self, ic: &InitCmds, events: EventSink) -> Result<()> {
        let mut dps = vec![CPU_DEVPORT];
        dps.extend(self.core.specs.iter().map(|s| s.devport));
        let mut dev_of = std::collections::BTreeMap::new();
        self.core.sysport.clear();
        for &dp in &dps {
            let mut a = Attrs::new().of(sys::IFCS_DEVPORT_ATTR_SYSPORT);
            let (rc, _) = self.core.lib.ifcs_devport_attr_get(NODE, dp, &mut a);
            if !rc.is_success() {
                err(
                    ssys::SAI_API_SWITCH,
                    format_args!(
                        "devport {dp}: no sysport ({}); port map and init config disagree?",
                        rc.name()
                    ),
                );
                return Err(ifcs::status::sai_error(rc.reason()));
            }
            self.core.sysport.insert(dp, a.handle_at(0));
            dev_of.insert(a.handle_at(0), dp);
        }
        self.core.dev_of_sysport = Arc::new(dev_of);
        PortArea::all_down(&self.core);
        self.port.configure(&self.core); // link cache: every port down from here on
        let (mut ok, mut bad) = (0u32, 0u32);
        for c in &ic.cmds {
            if self.apply_cmd(c) {
                ok += 1;
            } else {
                bad += 1;
            }
        }
        for (kind, n) in &ic.skipped {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!(
                    "init commands: skipped {n} '{kind}' line(s) (only 'ifcs set devport' serdes/eq attributes are applied)"
                ),
            );
        }
        if bad > 0 {
            err(
                ssys::SAI_API_SWITCH,
                format_args!("init commands: {bad} of {} failed (see above)", ic.cmds.len()),
            );
        }
        log::notice(ssys::SAI_API_SWITCH, format_args!("init commands: {ok} applied"));
        // the board's register list, where the vendor's init files have their `pci write` lines
        self.apply_regs();

        // The events: libifcs' threads only translate and queue (no IFCS call, no SAI lock).
        let front: Arc<Vec<u32>> = Arc::new(self.core.specs.iter().map(|s| s.devport).collect());
        let cache = self.port.link_cache();
        let link_events = events.clone();
        let map = Arc::clone(&self.lag.dest_map);
        let dev_of = Arc::clone(&self.core.dev_of_sysport);
        let cb = Callbacks::new(Handlers {
            link: Some(Arc::new(move |e| {
                if !front.contains(&e.devport) {
                    return;
                }
                cache.event(e.devport, e.up);
                if let Ok(hw) = u16::try_from(e.devport) {
                    link_events.send(BackendEvent::Link {
                        hw: HwPort(hw),
                        up: e.up,
                    });
                }
            })),
            l2: Some(Arc::new(move |e| {
                if let Some(ev) = l2::fdb_event(&e, |h| l2::dest_of_handle(&map, &dev_of, h)) {
                    events.send(BackendEvent::Fdb(ev));
                }
            })),
            log: None,
        });
        self.core.call(
            self.core.lib.ifcs_linkscan_event_register_notify(NODE, &cb),
            "linkscan register",
        )?;
        let registered = self.core.call(
            self.core.lib.ifcs_l2_event_register_notify(NODE, &cb),
            "l2 event register",
        );
        self.event_cb = Some(cb);
        registered?;
        self.l2.base_init(&self.core, &mut self.lag, &dps)
    }

    /// The post-init register list (`SAI_TERALYNX_POST_INIT_REGS`, no C++ counterpart): per entry,
    /// in order, through libifcs' own `node_pci_reg_read32` / `_write32` (the vendor CLI's
    /// `pci read` / `pci write`; no second mapping of the BAR): read, `(old & !mask) | value`
    /// written, read back and compared under the mask. A failed access or a mismatch is an error
    /// line and counted ([`RegStats`], the diag shell's `regs`), and the init goes on: the list is
    /// board setup the switching does not depend on (the CS8210's LED stream), and a switch that
    /// forwards with dark LEDs is better than none.
    fn apply_regs(&mut self) {
        let mut st = RegStats::default();
        let n = self.regs.len();
        if n == 0 {
            self.reg_stats = st;
            return;
        }
        let lib = &self.core.lib;
        if !lib.has_reg_access() {
            err(
                ssys::SAI_API_SWITCH,
                format_args!(
                    "post-init registers: libifcs exports no node_pci_reg_read32/_write32: {n} writes NOT applied"
                ),
            );
            st.failed = n;
            self.reg_stats = st;
            return;
        }
        for r in &self.regs {
            let old = match lib.reg_read32(NODE, r.offset) {
                Ok(v) => v,
                Err(rc) => {
                    err(
                        ssys::SAI_API_SWITCH,
                        format_args!("{}: read of {:#x} failed ({rc}): not written", r.at, r.offset),
                    );
                    st.failed += 1;
                    continue;
                }
            };
            let new = (old & !r.mask) | r.value;
            if let Err(rc) = lib.reg_write32(NODE, r.offset, new) {
                err(
                    ssys::SAI_API_SWITCH,
                    format_args!("{}: write {new:#x} to {:#x} failed ({rc})", r.at, r.offset),
                );
                st.failed += 1;
                continue;
            }
            match lib.reg_read32(NODE, r.offset) {
                Ok(back) if back & r.mask == r.value => st.applied += 1,
                Ok(back) => {
                    err(
                        ssys::SAI_API_SWITCH,
                        format_args!(
                            "{}: {:#x} reads {back:#x} after writing {new:#x} (mask {:#x}): mismatch",
                            r.at, r.offset, r.mask
                        ),
                    );
                    st.mismatched += 1;
                }
                Err(rc) => {
                    err(
                        ssys::SAI_API_SWITCH,
                        format_args!("{}: read-back of {:#x} failed ({rc})", r.at, r.offset),
                    );
                    st.failed += 1;
                }
            }
        }
        log::notice(
            ssys::SAI_API_SWITCH,
            format_args!(
                "post-init registers: {} of {n} written and read back ({} mismatched, {} failed)",
                st.applied, st.mismatched, st.failed
            ),
        );
        self.reg_stats = st;
    }

    /// `SwitchArea::apply_cmd`: one init command, typed as the vendor CLI's setter does.
    fn apply_cmd(&self, c: &config::DevportCmd) -> bool {
        let a = Attrs::new();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the C++ casts: the parser checked each value's range"
        )]
        let a = match c.kind {
            ValKind::U32 => a.u32(c.attr, c.values.first().copied().unwrap_or(0) as u32),
            ValKind::I8List => a.i8_list(c.attr, c.values.iter().map(|&v| v as i8).collect()),
            ValKind::U8List => a.u8_list(c.attr, c.values.iter().map(|&v| v as u8).collect()),
            ValKind::U32List => a.u32_list(c.attr, c.values.iter().map(|&v| v as u32).collect()),
        };
        let mut a = a;
        let rc = self.core.lib.ifcs_devport_attr_set(NODE, c.devport, &mut a);
        if !rc.is_success() {
            err(
                ssys::SAI_API_SWITCH,
                format_args!("{}: devport {} {}: {}", c.at, c.devport, c.name, rc.name()),
            );
            return false;
        }
        true
    }

    /// `SwitchArea::deinit`.
    fn deinit(&mut self) {
        if !self.sdk_running {
            return;
        }
        let rc = self.core.lib.ifcs_deinit(sys::IFCS_SHUTDOWN_TYPE_COLD.cast_signed());
        if !rc.is_success() && rc.reason() != sys::IFCS_UNINIT {
            err(ssys::SAI_API_SWITCH, format_args!("ifcs_deinit: {}", rc.name()));
        }
        self.sdk_running = false;
    }

    /// `IfcsBackend::shutdown`: `SflowArea::shutdown`, then `SwitchArea::shutdown`: ports first
    /// (a dead or half-torn-down SDK must not leave links up), callbacks off, netdevs, the ACL
    /// state, `ifcs_deinit(COLD)` (or not, with `SAI_TERALYNX_SHUTDOWN=ports-down`).
    pub fn shutdown(&mut self) {
        self.sflow.shutdown();
        if !self.sdk_running {
            return;
        }
        PortArea::all_down(&self.core);
        let _ = self.core.lib.ifcs_linkscan_event_unregister_notify(NODE);
        let _ = self.core.lib.ifcs_l2_event_unregister_notify(NODE);
        self.hostif.teardown_all();
        self.acl.forget();
        self.event_cb = None; // c_.link = c_.fdb = nullptr
        if self.core.settings.deinit_on_shutdown {
            self.deinit();
        } else {
            log::notice(
                ssys::SAI_API_SWITCH,
                format_args!("SAI_TERALYNX_SHUTDOWN=ports-down: ports down, IFCS left initialised"),
            );
        }
        self.sdk_running = false;
        self.initialized = false;
        self.temps.initialized.store(false, Ordering::SeqCst);
    }

    /// `SwitchArea::table_usage`: prefixes / hosts from the L3 area's counts against the documented
    /// capacities, next hops / L2 / ECMP from libifcs' usage calls.
    pub fn table_usage(&self) -> Result<TableUsage> {
        let n = self.l3.counts();
        let c = &self.core;
        let used = n.v4_prefixes.saturating_add(n.v6_prefixes.saturating_mul(2));
        let v4_routes_free = CAP_V4_PREFIX.saturating_sub(used);
        let hused = n.v4_hosts.saturating_add(n.v6_hosts.saturating_mul(2));
        let mut us = sys::ifcs_usage_t {
            max: 0,
            current: 0,
            dirty: 0,
        };
        let rc = c.lib.ifcs_nexthop_usage_get(NODE, &mut Attrs::null(), &mut us);
        let egress_free = c.usage_free(rc, &us, 0, "nexthop");
        let mut us = sys::ifcs_usage_t {
            max: 0,
            current: 0,
            dirty: 0,
        };
        let rc = c.lib.ifcs_l2_entry_usage_get(NODE, &mut Attrs::null(), &mut us);
        let fdb_free = c.usage_free(rc, &us, CAP_MAC, "l2_entry");
        let mut us = sys::ifcs_usage_t {
            max: 0,
            current: 0,
            dirty: 0,
        };
        let rc = c.lib.ifcs_ecmp_usage_get(NODE, &mut Attrs::null(), &mut us);
        let ecmp_groups_free = c.usage_free(rc, &us, 0, "ecmp");
        let l = self.chip_limits()?;
        Ok(TableUsage {
            v4_routes_free,
            v6_routes_free: CAP_V6_PREFIX.min(v4_routes_free / 2),
            hosts_free: CAP_V4_HOST.saturating_sub(hused),
            egress_free,
            fdb_free,
            ecmp_groups_free,
            ecmp_members_free: l.ecmp_members.saturating_sub(n.ecmp_members),
        })
    }

    /// `SwitchArea::chip_limits`: node ACL priority range and ECMP groups, read once.
    pub fn chip_limits(&self) -> Result<ChipLimits> {
        let mut cached = self.limits.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(l) = *cached {
            return Ok(l);
        }
        let mut l = ChipLimits::default();
        let mut a = Attrs::new()
            .of(sys::IFCS_NODE_ATTR_ACL_ENTRY_MINIMUM_PRIORITY)
            .of(sys::IFCS_NODE_ATTR_ACL_ENTRY_MAXIMUM_PRIORITY)
            .of(sys::IFCS_NODE_ATTR_NUMBER_OF_ECMP_GROUPS);
        let (rc, _) = self.core.lib.ifcs_node_attr_get(NODE, &mut a);
        if rc.is_success() {
            l.acl_prio_min = a.u32_at(0);
            l.acl_prio_max = a.u32_at(1);
            l.ecmp_groups = a.u32_at(2);
        } else {
            log::warn(
                ssys::SAI_API_SWITCH,
                format_args!("node limits (ACL priority, ECMP groups) unreadable; reporting 0"),
            );
        }
        l.ecmp_paths = ECMP_PATHS;
        l.ecmp_members = l.ecmp_groups.wrapping_mul(ECMP_PATHS);
        l.vrf_max = VRF_MAX;
        l.buffer_bytes = 0; // not known from this SDK: SWITCH TOTAL_BUFFER_SIZE reads 0
        *cached = Some(l);
        Ok(l)
    }

    /// `SwitchArea::shell`: the read-only diag commands; those of areas not ported yet say so.
    pub fn shell(&self, cmd: &str) -> (Result<()>, String) {
        let t: Vec<&str> = cmd.split_whitespace().collect();
        let first = t.first().copied();
        if first.is_none() || first == Some("help") {
            return (
                Ok(()),
                "commands: ports | devport <id> | temp | copp | acl | counters <devport> | help (read-only)\n"
                    .to_owned(),
            );
        }
        if !self.initialized {
            return (Err(Error::Uninitialized), "SDK not initialised\n".to_owned());
        }
        match (first, t.len()) {
            (Some("temp"), _) => match self.temps.read() {
                Err(e) => (Err(e), "temperature read failed\n".to_owned()),
                Ok(c) => {
                    let mut out = String::new();
                    for (i, v) in c.cur.iter().enumerate() {
                        let _ = writeln!(out, "monitor {i}: {:.1} C", f64::from(*v) / 10.0);
                    }
                    (Ok(()), out)
                }
            },
            (Some(w @ ("copp" | "acl")), _) => (
                Err(Error::NotSupported),
                format!(
                    "{w}: not ported yet (G2: the {} area of sai-backend-teralynx)\n",
                    if w == "copp" { "host interface" } else { "ACL" }
                ),
            ),
            (Some("regs"), 1) => {
                let s = &self.reg_stats;
                let mut out = format!(
                    "post-init registers: {} listed, {} applied, {} mismatched, {} failed\n",
                    self.regs.len(),
                    s.applied,
                    s.mismatched,
                    s.failed
                );
                for r in &self.regs {
                    let now = self.core.lib.reg_read32(NODE, r.offset);
                    let _ = match now {
                        Ok(v) => writeln!(
                            out,
                            "{:#07x} mask {:#010x} want {:#010x} now {v:#010x} {}",
                            r.offset,
                            r.mask,
                            r.value,
                            if v & r.mask == r.value { "ok" } else { "DIFFERS" }
                        ),
                        Err(rc) => writeln!(out, "{:#07x}: read failed ({rc})", r.offset),
                    };
                }
                // the board's watch list, read now (under the SAI lock, as every IFCS call here);
                // the only other offsets this command reads, none of them from the socket
                let _ = writeln!(out, "watch registers: {} (SAI_TERALYNX_DIAG_REGS)", self.watch.len());
                for &off in &self.watch {
                    let _ = match self.core.lib.reg_read32(NODE, off) {
                        Ok(v) => writeln!(out, "{off:#07x} = {v:#010x}"),
                        Err(rc) => writeln!(out, "{off:#07x}: read failed ({rc})"),
                    };
                }
                (Ok(()), out)
            }
            (Some("counters"), 2) => {
                let hw = t.get(1).map_or(0, |a| crate::keys::atoi(a));
                (Ok(()), counters::counters_report(&self.core, hw))
            }
            (Some("ports"), _) | (Some("devport"), 2) => {
                let mut out = String::new();
                for s in &self.core.specs {
                    if first == Some("devport") && t.get(1).is_some_and(|id| *id != s.devport.to_string()) {
                        continue;
                    }
                    let g = |id, def| self.core.dp_get_u32(s.devport, id).unwrap_or(def);
                    let admin = g(sys::IFCS_DEVPORT_ATTR_ADMIN_STATE, 9);
                    let link = g(sys::IFCS_DEVPORT_ATTR_LINK_STATUS, 9);
                    let speed = g(sys::IFCS_DEVPORT_ATTR_SPEED, 0);
                    let fec = g(sys::IFCS_DEVPORT_ATTR_FEC_MODE, 9);
                    let an = g(sys::IFCS_DEVPORT_ATTR_AUTO_NEG, 9);
                    let sp = self.core.sysport.get(&s.devport).map_or(0, |h| h.0);
                    let _ = writeln!(
                        out,
                        "devport {}: admin {admin} link {} speed {} fec {fec} autoneg {an} sysport 0x{sp:x}",
                        s.devport,
                        if link == 1 { "up" } else { "down" },
                        speed_from_enum(speed)
                    );
                }
                if out.is_empty() {
                    return (Err(Error::InvalidParameter), "no such devport\n".to_owned());
                }
                (Ok(()), out)
            }
            _ => (Err(Error::NotSupported), format!("not supported: {cmd}\n")),
        }
    }
}

/// The temperature monitors (`SwitchArea::asic_temperatures`): one monitor per read, round-robin,
/// at most one read per `SAI_TERALYNX_TEMP_PERIOD` once every monitor was read, the answer from the
/// cache in between (a read holds libifcs' lock ~400 ms on the chip). Its own mutex: the
/// publisher calls it without the SAI lock, as the C++ (`temp_mu_`).
#[derive(Debug)]
pub struct Temps {
    lib: Option<Ifcs>,
    period: u32,
    monitors: usize,
    /// `Core::initialized` for the reader.
    pub initialized: AtomicBool,
    state: Mutex<TempState>,
}

#[derive(Debug, Default)]
struct TempState {
    next: usize,
    reads: usize,
    valid: [bool; 5],
    tenths: [i32; 5],
    when: Option<Instant>,
}

const MONITORS: [u32; 5] = [
    sys::IFCS_NODE_ATTR_MASTER_LAST_TEMP,
    sys::IFCS_NODE_ATTR_REMOTE0_LAST_TEMP,
    sys::IFCS_NODE_ATTR_REMOTE1_LAST_TEMP,
    sys::IFCS_NODE_ATTR_REMOTE2_LAST_TEMP,
    sys::IFCS_NODE_ATTR_REMOTE3_LAST_TEMP,
];

/// A monitor's reading as the C++ keeps it: the master always, a remote only within -40..150 C;
/// tenths rounded half away from zero (`lround(t * 10)`).
pub fn tenths(master: bool, t: f32) -> Option<i32> {
    let valid = t.is_finite() && (master || (-40.0..=150.0).contains(&t));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lround of a finite reading in tenths of a degree"
    )]
    valid.then(|| (t * 10.0).round() as i32)
}

impl Temps {
    /// Monitors read through `lib` (none: every read fails).
    pub fn new(lib: Option<Ifcs>, period: u32, monitors: usize) -> Self {
        Self {
            lib,
            period,
            monitors: monitors.clamp(1, 5),
            initialized: AtomicBool::new(false),
            state: Mutex::new(TempState::default()),
        }
    }

    /// `asic_temperatures`: current = peak = the valid monitors.
    pub fn read(&self) -> Result<AsicTemps> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(Error::Uninitialized);
        }
        let lib = self.lib.as_ref().ok_or(Error::Uninitialized)?;
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let mut due = st.reads == 0
            || self.period == 0
            || st
                .when
                .is_none_or(|w| now.duration_since(w) >= Duration::from_secs(u64::from(self.period)));
        // Until every monitor was read once, read the next one on each call.
        if st.reads < self.monitors {
            due = true;
        }
        if due {
            let i = st.next;
            let mut a = Attrs::new().of(MONITORS.get(i).copied().unwrap_or(sys::IFCS_NODE_ATTR_MASTER_LAST_TEMP));
            let (rc, _) = lib.ifcs_node_attr_get(NODE, &mut a);
            rc.ok()?;
            let t = tenths(i == 0, a.flt_at(0));
            let st = &mut *st;
            if let (Some(v), Some(x)) = (st.valid.get_mut(i), st.tenths.get_mut(i)) {
                *v = t.is_some();
                *x = t.unwrap_or(0);
            }
            st.next = (st.next + 1) % self.monitors;
            st.when = Some(now);
            st.reads += 1;
        }
        let cur: Vec<i32> = (0..self.monitors)
            .filter(|&i| st.valid.get(i).copied().unwrap_or(false))
            .filter_map(|i| st.tenths.get(i).copied())
            .collect();
        Ok(AsicTemps { peak: cur.clone(), cur })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_validity_and_rounding_as_the_cpp() {
        assert_eq!(tenths(true, 55.5), Some(555));
        assert_eq!(tenths(false, 53.55), Some(536)); // 535.5 rounds away from zero
        assert_eq!(tenths(false, -300.0), None);
        assert_eq!(tenths(true, -300.0), Some(-3000)); // the master is not range-checked
        assert_eq!(tenths(true, f32::NAN), None);
        assert_eq!(tenths(false, 150.0), Some(1500));
        assert_eq!(Temps::new(None, 15, 5).read(), Err(Error::Uninitialized));
    }

    #[test]
    fn log_levels_as_on_log() {
        assert_eq!(log_level(0), ssys::SAI_LOG_LEVEL_ERROR);
        assert_eq!(log_level(2), ssys::SAI_LOG_LEVEL_ERROR);
        assert_eq!(log_level(3), ssys::SAI_LOG_LEVEL_WARN);
        assert_eq!(log_level(4), ssys::SAI_LOG_LEVEL_INFO);
        assert_eq!(log_level(5), ssys::SAI_LOG_LEVEL_DEBUG);
        assert_eq!(log_level(-1), ssys::SAI_LOG_LEVEL_ERROR);
    }
}

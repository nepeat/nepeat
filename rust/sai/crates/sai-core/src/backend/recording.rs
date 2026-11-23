//! A backend decorator that forwards every call and writes one line per call in the C++
//! recording backend's format ([`super::log`]): the call log the C++-vs-Rust differential
//! compares. Events the backend raises are logged where they happen (before the line of the call
//! that caused them), through a tap on the [`EventSink`] the decorator hands the backend at
//! [`Backend::init`].
//!
//! Not logged: the queries whose answer is fixed for the life of the backend (`name`, `is_fake`,
//! `cpu_port`, `hardware_info`, `chip_limits` and the capability probes) unless the mode is
//! [`Mode::Full`]; and the generic methods of [`Backend`] (`object_create`, `entry_*`,
//! `stats_*`, `programs`, `limits`, `port_queues`, ...), which have no C++ counterpart and go
//! away area by area.
use super::log;
use super::{Backend, Capacities, EventSink, HostifNetdev, HwPort, PortInfo, Profile, StatsObject, Vid};
use crate::error::Result;
use crate::model::EntryKey;
use crate::oid::RawOid;
use crate::value::Attr;
use sai_sys as sys;
use std::sync::{Arc, Mutex, PoisonError};

/// What is logged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Every call except the fixed queries (the C++ default).
    #[default]
    Normal,
    /// Every call (C++ `RecordingBackend::Mode::Full`, driver option `--queries`).
    Full,
}

/// The lines written so far; shared, so it can be read while the decorator sits inside a
/// [`crate::sai::Sai`].
#[derive(Clone, Debug, Default)]
pub struct CallLog(Arc<Mutex<Vec<String>>>);

impl CallLog {
    fn lines_mut(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        // A panic while a line was being pushed leaves whole lines behind: keep them.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Appends a line.
    pub fn push(&self, line: String) {
        self.lines_mut().push(line);
    }

    /// The lines so far.
    pub fn lines(&self) -> Vec<String> {
        self.lines_mut().clone()
    }

    /// The lines so far, removed from the log.
    pub fn take(&self) -> Vec<String> {
        core::mem::take(&mut *self.lines_mut())
    }

    /// The log as text: each line ends with `\n`.
    pub fn text(&self) -> String {
        self.lines_mut().iter().fold(String::new(), |mut text, line| {
            text.push_str(line);
            text.push('\n');
            text
        })
    }
}

/// The decorator.
#[derive(Debug)]
pub struct Recording<B> {
    pub(super) inner: B,
    log: CallLog,
    mode: Mode,
}

impl<B> Recording<B> {
    /// `inner`, logged into a new [`CallLog`].
    pub fn new(inner: B, mode: Mode) -> Self {
        Self {
            inner,
            log: CallLog::default(),
            mode,
        }
    }

    /// The log (a handle: it stays readable after the decorator was moved).
    pub fn log(&self) -> CallLog {
        self.log.clone()
    }

    /// The backend.
    pub fn inner(&self) -> &B {
        &self.inner
    }

    /// The backend, mutably (tests: fault injection, link changes; not logged).
    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    /// The backend back.
    pub fn into_inner(self) -> B {
        self.inner
    }

    /// Writes a call's line unless it is a query and the mode is normal.
    pub(super) fn record(&self, query: bool, line: impl FnOnce() -> String) {
        if !query || self.mode == Mode::Full {
            self.log.push(line());
        }
    }
}

impl<B: Backend> Backend for Recording<B> {
    fn name(&self) -> &'static str {
        record!(self, name, query(), [])
    }

    fn init(&mut self, profile: &Profile, events: EventSink) -> Result<()> {
        let mut args = String::new();
        log::arg(&mut args, "profile", profile);
        let lines = self.log.clone();
        let events = events.tapped(move |event| lines.push(log::event_line(event)));
        let result = self.inner.init(profile, events);
        self.record(false, || log::call_line("init", &args, &result, log::no_outs));
        result
    }

    fn shutdown(&mut self) {
        self.inner.shutdown();
        self.record(false, || "shutdown() -> void".to_owned());
    }

    fn ports(&self) -> Vec<PortInfo> {
        record!(self, ports, value(), [])
    }

    fn cpu_port(&self) -> HwPort {
        record!(self, cpu_port, query(), [])
    }

    fn port_admin_set(&mut self, hw: HwPort, up: bool) -> Result<()> {
        record!(self, port_admin_set, status(), [hw, up])
    }

    fn port_link_get(&self, hw: HwPort) -> Result<bool> {
        record!(self, port_link_get, status(up), [hw])
    }

    fn port_speed_set(&mut self, hw: HwPort, mbps: u32) -> Result<()> {
        record!(self, port_speed_set, status(), [hw, mbps])
    }

    fn port_speed_get(&self, hw: HwPort) -> Result<u32> {
        record!(self, port_speed_get, status(mbps), [hw])
    }

    fn port_supported_speeds(&self, hw: HwPort) -> Result<Vec<u32>> {
        record!(self, port_supported_speeds, status(out), [hw])
    }

    fn vlan_create(&mut self, vid: Vid) -> Result<()> {
        record!(self, vlan_create, status(), [vid])
    }

    fn vlan_destroy(&mut self, vid: Vid) -> Result<()> {
        record!(self, vlan_destroy, status(), [vid])
    }

    fn vlan_port_add(&mut self, vid: Vid, hw: HwPort, untagged: bool) -> Result<()> {
        record!(self, vlan_port_add, status(), [vid, hw, untagged])
    }

    fn vlan_port_remove(&mut self, vid: Vid, hw: HwPort) -> Result<()> {
        record!(self, vlan_port_remove, status(), [vid, hw])
    }

    fn vlan_members(&self, vid: Vid) -> Result<Vec<(HwPort, bool)>> {
        record!(self, vlan_members, status(out), [vid])
    }

    // ---- the generic methods: forwarded, not logged (no C++ counterpart)

    fn port_queues(&self) -> u8 {
        self.inner.port_queues()
    }
    fn port_priority_groups(&self) -> u8 {
        self.inner.port_priority_groups()
    }
    fn limits(&self) -> Result<Capacities> {
        self.inner.limits()
    }
    fn programs(&self, object_type: sys::sai_object_type_t) -> bool {
        self.inner.programs(object_type)
    }
    fn object_create(&mut self, object_type: sys::sai_object_type_t, id: RawOid, attrs: &[Attr]) -> Result<()> {
        self.inner.object_create(object_type, id, attrs)
    }
    fn hostif_create(&mut self, id: RawOid, netdev: &HostifNetdev<'_>, attrs: &[Attr]) -> Result<()> {
        self.inner.hostif_create(id, netdev, attrs)
    }
    fn object_set(&mut self, object_type: sys::sai_object_type_t, id: RawOid, attr: &Attr) -> Result<()> {
        self.inner.object_set(object_type, id, attr)
    }
    fn object_remove(&mut self, object_type: sys::sai_object_type_t, id: RawOid) -> Result<()> {
        self.inner.object_remove(object_type, id)
    }
    fn entry_create(&mut self, key: &EntryKey, attrs: &[Attr]) -> Result<()> {
        self.inner.entry_create(key, attrs)
    }
    fn entry_set(&mut self, key: &EntryKey, attr: &Attr) -> Result<()> {
        self.inner.entry_set(key, attr)
    }
    fn entry_remove(&mut self, key: &EntryKey) -> Result<()> {
        self.inner.entry_remove(key)
    }
    fn stats_supported(&self, object_type: sys::sai_object_type_t) -> Result<Vec<sys::sai_stat_id_t>> {
        self.inner.stats_supported(object_type)
    }
    fn stats_get(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t], clear: bool) -> Result<Vec<u64>> {
        self.inner.stats_get(object, counters, clear)
    }
    fn stats_clear(&mut self, object: StatsObject, counters: &[sys::sai_stat_id_t]) -> Result<()> {
        self.inner.stats_clear(object, counters)
    }
}

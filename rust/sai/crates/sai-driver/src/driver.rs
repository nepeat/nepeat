//! The driver: one script line at a time against `Sai<Recording<VirtualBackend>>`, with the C++
//! `frontend_driver`'s grammar and output (see the crate documentation for what differs and why).
use crate::text::{self, ParseError, Parsed, fail, hexnum, num, parse_ip, parse_mac, parse_prefix, unquote, words};
use crate::types::{self, ObjType, Shape};
use crate::values::{Kind, LIST_CAPACITY, Names, is_list, kind_of, list_count, value_in, value_out};
use sai_core::Error;
use sai_core::backend::log::status_name;
use sai_core::backend::types::{Dest, FdbChange, FdbEvent, MacAddr, MdioDev, PhyAddr, PortStat};
use sai_core::backend::{Backend, BackendEvent, CallLog, HwPort, Mode, Profile, Recording, Vid};
use sai_core::error::AttrError;
use sai_core::macsec::MacsecObject;
use sai_core::model::{EntryKey, Lag, Port};
use sai_core::oid::{LagId, ObjectType, PortId, RawOid, TypedId};
use sai_core::sai::{Notification, Sai};
use sai_core::value::{Attr, IpPrefix, Pointer, Value};
use sai_core::virt::VirtualBackend;
use sai_meta::{AttrMeta, ValueType};
use sai_sys as sys;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::PoisonError;
use std::sync::mpsc::Receiver;

/// The core the driver runs (the rim's `Sai<Box<dyn Backend>>` with the backend chosen): by
/// default the virtual one.
type Core<B> = Sai<Recording<B>>;

/// What a driver runs on besides the core: where the backend of each switch comes from, and what
/// the script's test hooks (`inject`, `mdio`, `feature`, `linkscan`) do to it. [`Virtual`] is the
/// default (the virtual backend, as the C++ driver's fake); another backend comes with its own
/// (sai-backend-openbcm's glue differential: the hardware backend over the Rust fake chip, the
/// hooks on that chip). Like the C++ test-port hooks, the hooks are not backend calls: the
/// `be` lines show only what they cause.
pub trait Hooks<B> {
    /// The backend of a new switch, for the profile's `SAI_OPENBCM_BACKEND` (`None` when this
    /// driver has no such backend: the switch create fails with `FAILURE`).
    fn backend(&mut self, selected: &str) -> Option<B>;
    /// A backend that is never initialised: the core's before the first switch.
    fn idle(&mut self) -> B;
    /// `inject link <port|lag> up|down`: the destination's link changes.
    fn link(&mut self, be: &mut B, dest: Dest, up: bool);
    /// `linkscan`: one link scan pass.
    fn linkscan(&mut self, be: &mut B);
    /// `inject fdb ...`: hardware learned or aged an entry.
    fn fdb(&mut self, be: &mut B, event: FdbEvent);
    /// `inject stat <port> <STAT> <value>`: the value the backend reports for a counter.
    fn stat(&mut self, be: &mut B, hw: HwPort, stat: PortStat, value: u64);
    /// `mdio <phy> <dev> <reg> <value>`: a register of the backend's MDIO bus.
    fn mdio(&mut self, be: &mut B, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16);
    /// `feature qos|mirror|l2 on|off`: an optional group of the backend.
    fn feature(&mut self, be: &mut B, feature: Feature, on: bool);
    /// Lines to print for the operation just run, after its echo line and before its status
    /// (each indented by two blanks); by default none.
    fn lines(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// The groups `feature` switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// `qos`.
    Qos,
    /// `mirror`.
    Mirror,
    /// `l2` (hash, storm control, flood control).
    L2,
}

impl Feature {
    /// The script's word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Qos => "qos",
            Self::Mirror => "mirror",
            Self::L2 => "l2",
        }
    }
}

/// The default hooks: the virtual backend, numbering ports and answering like the C++ fake
/// (`VirtualBackend::cpp_by_default`), for `openbcm` (the C++ driver's recorder over its fake),
/// `fake` and `virtual`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Virtual;

impl Hooks<VirtualBackend> for Virtual {
    fn backend(&mut self, selected: &str) -> Option<VirtualBackend> {
        matches!(selected, "openbcm" | "fake" | "virtual").then(VirtualBackend::cpp_by_default)
    }

    fn idle(&mut self) -> VirtualBackend {
        VirtualBackend::new()
    }

    fn link(&mut self, be: &mut VirtualBackend, dest: Dest, up: bool) {
        // A LAG's destination is no port: the C++ fake finds no port of that number and
        // ignores it; so does this (an unknown port too: inject_link's error).
        if let Dest::Port(hw) = dest {
            let _ = be.inject_link(hw, up);
        }
    }

    fn linkscan(&mut self, be: &mut VirtualBackend) {
        be.linkscan();
    }

    fn fdb(&mut self, be: &mut VirtualBackend, event: FdbEvent) {
        be.inject_fdb(event);
    }

    fn stat(&mut self, be: &mut VirtualBackend, hw: HwPort, stat: PortStat, value: u64) {
        be.set_stat(hw, stat, value);
    }

    fn mdio(&mut self, be: &mut VirtualBackend, phy: PhyAddr, dev: MdioDev, reg: u16, val: u16) {
        let mdio = be.mdio();
        mdio.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .regs
            .insert((phy, dev, reg), val);
    }

    fn feature(&mut self, be: &mut VirtualBackend, feature: Feature, on: bool) {
        let toggles = be.toggles_mut();
        match feature {
            Feature::Qos => toggles.qos_on = on,
            Feature::Mirror => toggles.mirror_on = on,
            Feature::L2 => toggles.l2x_on = on,
        }
    }
}

/// The profile key the C++ driver's backend factory reads (`SAI_OPENBCM_BACKEND`).
const BACKEND_KEY: &str = sai_core::keys::BACKEND;

/// How the driver is started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The call log in full mode (`--queries`: the constant-answer queries too).
    pub log: LogMode,
    /// The host has a switch chip (`SAI_RS_VIRTUAL_ON_HARDWARE=1` in the environment): the
    /// virtual backend is told so (`SAI_OPENBCM_FAKE_ON_HARDWARE=1`), as the C++ driver tells its fake.
    pub on_hardware: OnHardware,
}

/// The call log's mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogMode {
    /// Every backend call but the constant-answer queries (the default).
    #[default]
    Changes,
    /// Every backend call (`--queries`).
    Full,
}

/// Whether the host has a switch chip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnHardware {
    /// No (the default).
    #[default]
    No,
    /// Yes: the virtual backend must be told it may run here.
    Yes,
}

/// What an operation printed: the status and the indented result lines.
struct Out {
    status: String,
    lines: Vec<String>,
}

impl Out {
    fn status(status: String) -> Self {
        Self {
            status,
            lines: Vec::new(),
        }
    }
    fn of<T>(result: &sai_core::Result<T>) -> Self {
        Self::status(status_name(result))
    }
    fn success() -> Self {
        Self::status("SUCCESS".to_owned())
    }
}

/// An attribute as the script gave it: its metadata, the driver's kind and the decoded value
/// (`None`: no Rust form, see [`value_in`]).
struct ScriptAttr {
    meta: &'static AttrMeta,
    value: Option<Value>,
}

/// Which call an attribute list goes to (the rim's `Call`, `sai-ffi/src/generic.rs`).
#[derive(Clone, Copy)]
enum Call {
    Create,
    Set,
}

/// The script runner, on the virtual backend unless made with [`Driver::with_hooks`].
pub struct Driver<B: Backend = VirtualBackend> {
    mode: Mode,
    profile: BTreeMap<String, String>,
    sai: Core<B>,
    log: CallLog,
    initialized: bool,
    switch: RawOid,
    events: Option<Receiver<BackendEvent>>,
    names: Names,
    hooks: Box<dyn Hooks<B>>,
}

impl<B: Backend> std::fmt::Debug for Driver<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver")
            .field("mode", &self.mode)
            .field("profile", &self.profile)
            .field("initialized", &self.initialized)
            .field("switch", &self.switch)
            .field("names", &self.names)
            .finish_non_exhaustive()
    }
}

impl Driver<VirtualBackend> {
    /// A driver with the C++ driver's profile defaults (`SAI_OPENBCM_BACKEND=openbcm`: the C++
    /// driver's recorder over its fake, here the recorder over the virtual chip, which numbers
    /// ports and answers like the C++ fake: [`VirtualBackend::cpp_by_default`]).
    pub fn new(options: Options) -> Self {
        Self::with_hooks(options, Box::new(Virtual))
    }
}

impl<B: Backend> Driver<B> {
    /// A driver whose switches run on the backends `hooks` makes, with the same profile defaults.
    pub fn with_hooks(options: Options, mut hooks: Box<dyn Hooks<B>>) -> Self {
        let mut profile: BTreeMap<String, String> = [
            (BACKEND_KEY, "openbcm"),
            ("SAI_OPENBCM_FAKE_PORTS", "8"),
            ("SAI_OPENBCM_FAKE_LANES", "4"),
            ("SAI_OPENBCM_FAKE_SPEED", "100000"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        if options.on_hardware == OnHardware::Yes {
            profile.insert("SAI_OPENBCM_FAKE_ON_HARDWARE".to_owned(), "1".to_owned());
        }
        let mode = match options.log {
            LogMode::Changes => Mode::Normal,
            LogMode::Full => Mode::Full,
        };
        // the MACsec model the rim links (sai-ffi state.rs)
        sai_core::macsec::register_loader(&sai_macsec_phy::native::LOADER);
        // the rim's API before a switch: a core over a backend that was never initialised
        let backend = Recording::new(hooks.idle(), mode);
        let log = backend.log();
        Self {
            mode,
            profile,
            sai: Sai::new(backend),
            log,
            initialized: false,
            switch: RawOid::NULL,
            events: None,
            names: Names::default(),
            hooks,
        }
    }

    /// Runs a whole script; the output text.
    pub fn run_script(&mut self, script: &str) -> String {
        script.lines().filter_map(|l| self.run_line(l)).collect()
    }

    /// Runs one script line; its output (every line ends with `\n`), or `None` for a blank line
    /// or a comment.
    pub fn run_line(&mut self, line: &str) -> Option<String> {
        let w = words(line);
        if w.first().is_none_or(|first| first.starts_with('#')) {
            return None;
        }
        let mut text = format!("> {}\n", w.join(" "));
        let out = match self.run(&w) {
            Ok(out) => out,
            Err(ParseError(why)) => {
                for l in self.hooks.lines() {
                    let _ = writeln!(text, "  {l}");
                }
                let _ = writeln!(text, "= PARSE_ERROR {why}");
                let _ = self.log.take();
                return Some(text);
            }
        };
        // The C++ driver waits until the library processed the events the operation raised (not
        // after remove_switch: the queue goes with the switch); here they are applied in order now.
        let mut notes = Vec::new();
        if self.initialized && w.first().is_some_and(|op| op != "remove_switch") {
            if let Some(events) = &self.events {
                while let Ok(event) = events.try_recv() {
                    notes.extend(self.sai.handle_event(event));
                }
            }
        }
        // what the hooks show for the operation (e.g. the SDK glue calls), between the echo and
        // the status: where the C++ driver's glue stub prints them as they happen
        for l in self.hooks.lines() {
            let _ = writeln!(text, "  {l}");
        }
        let _ = writeln!(text, "= {}", out.status);
        for l in &out.lines {
            let _ = writeln!(text, "  {l}");
        }
        for l in self.log.take() {
            let _ = writeln!(text, "  be {l}");
        }
        for n in &notes {
            let note = self.note(n);
            let _ = writeln!(text, "  {note}");
        }
        Some(text)
    }

    /// A notification as the C++ driver prints it.
    fn note(&mut self, n: &Notification) -> String {
        match n {
            Notification::PortOperStatus { port, up } => {
                format!(
                    "notify port {} {}",
                    self.names.name(port.raw()),
                    if *up { "UP" } else { "DOWN" }
                )
            }
            Notification::Fdb {
                kind,
                mac,
                vlan,
                bridge_port,
                ..
            } => {
                let event = match kind.sai() {
                    sys::SAI_FDB_EVENT_LEARNED => "LEARNED",
                    sys::SAI_FDB_EVENT_AGED => "AGED",
                    sys::SAI_FDB_EVENT_MOVE => "MOVE",
                    sys::SAI_FDB_EVENT_FLUSHED => "FLUSHED",
                    _ => "?",
                };
                let bv = self.names.name(vlan.raw());
                let bp = self.names.name(bridge_port.map_or(RawOid::NULL, TypedId::raw));
                format!("notify fdb {event} {}@{bv} bridge_port={bp}", text::mac_str(mac))
            }
            // A variant this driver does not know yet prints generically instead of breaking the
            // build; the differential then shows it.
            #[allow(
                unreachable_patterns,
                clippy::match_wildcard_for_single_variants,
                reason = "Notification grows with the areas"
            )]
            other => format!("notify {other:?}"),
        }
    }

    fn run(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        match word(0) {
            "profile" => {
                need(w, 2)?;
                let Some((key, value)) = word(1).split_once('=') else {
                    return fail("expected <KEY>=<VALUE>");
                };
                self.profile.insert(key.to_owned(), unquote(value).to_owned());
                Ok(Out::success())
            }
            "switch" => self.switch_op(w),
            "remove_switch" => {
                let result = self.sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, self.switch);
                self.names.forget();
                self.switch = RawOid::NULL;
                self.events = None;
                Ok(Out::of(&result))
            }
            "create" => self.create_op(w),
            "remove" => self.remove_op(w),
            "set" => self.set_op(w),
            "get" => self.get_op(w),
            "stats" | "clear" => self.stats_op(w),
            "flush" => {
                let attrs = self.attrs_in(types::FDB_FLUSH, w, 1)?;
                let result = rim_decode(types::FDB_FLUSH.ot, &attrs, Call::Create)
                    .and_then(|a| self.sai.flush_fdb(self.switch, &a));
                Ok(Out::of(&result))
            }
            "inject" => self.inject_op(w),
            "mdio" => self.mdio_op(w),
            "feature" => self.feature_op(w),
            "linkscan" => {
                self.hooks.linkscan(self.sai.backend_mut().inner_mut());
                Ok(Out::success())
            }
            op => fail(format!("unknown operation '{op}'")),
        }
    }

    fn remove_op(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        need(w, 3)?;
        let ty = need_type(w, 1)?;
        if ty.shape == Shape::Entry {
            let key = self.key_of(ty, word(2))?;
            return Ok(Out::of(&self.sai.remove_entry(self.switch, &key)));
        }
        if !ty.has_create() {
            return fail(format!("{} has no remove", ty.name));
        }
        let oid = self.names.oid(word(2))?;
        Ok(Out::of(&implemented(ty.ot).and_then(|()| self.sai.remove(ty.ot, oid))))
    }

    fn set_op(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        need(w, 4)?;
        if w.len() != 4 {
            return fail("set takes one attribute");
        }
        let ty = need_type(w, 1)?;
        let attrs = self.attrs_in(ty, w, 3)?;
        if ty.shape == Shape::Entry {
            let key = self.key_of(ty, word(2))?;
            let result = match rim_decode(ty.ot, &attrs, Call::Set) {
                // an attribute a set may not carry: the C++ answer, after the entry lookup
                Err(_) if !settable(ty.ot, &attrs) => Err(attrs.first().map_or(Error::InvalidParameter, |a| {
                    self.sai.refused_set_entry(self.switch, &key, a.meta.id)
                })),
                other => other
                    .and_then(|a| a.first().cloned().ok_or(Error::InvalidParameter))
                    .and_then(|a| self.sai.set_entry(self.switch, &key, &a)),
            };
            return Ok(Out::of(&result));
        }
        let oid = self.names.oid(word(2))?;
        let result = implemented(ty.ot).and_then(|()| match rim_decode(ty.ot, &attrs, Call::Set) {
            // an attribute a set may not carry: the type's C++ answer, after the object lookup
            // (the rim's `Sai::refused_set`); the metadata's refusal where it defines none
            Err(e) if !settable(ty.ot, &attrs) => Err(attrs
                .first()
                .and_then(|a| self.sai.refused_set(ty.ot, oid, a.meta.id))
                .unwrap_or(e)),
            other => other
                .and_then(|a| a.first().cloned().ok_or(Error::InvalidParameter))
                .and_then(|a| self.sai.set(ty.ot, oid, &a)),
        });
        Ok(Out::of(&result))
    }

    fn stats_op(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        need(w, 4)?;
        let ty = need_type(w, 1)?;
        if !ty.has_stats() {
            return fail(format!("{} has no counters here", ty.name));
        }
        let ids = w.iter().skip(3).map(|s| stat_of(ty, s)).collect::<Parsed<Vec<_>>>()?;
        let oid = self.names.oid(word(2))?;
        if word(0) == "clear" {
            return Ok(Out::of(
                &implemented(ty.ot).and_then(|()| self.sai.stats_clear(ty.ot, oid, &ids)),
            ));
        }
        let result = implemented(ty.ot).and_then(|()| self.sai.stats_get(ty.ot, oid, &ids, false));
        let mut out = Out::of(&result);
        if let Ok(values) = result {
            out.lines = w.iter().skip(3).zip(values).map(|(s, v)| format!("{s}={v}")).collect();
        }
        Ok(out)
    }

    fn mdio_op(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        need(w, 5)?;
        let (phy, dev, reg, val) = (hexnum(word(1))?, hexnum(word(2))?, hexnum(word(3))?, hexnum(word(4))?);
        let dev = u8::try_from(dev)
            .ok()
            .and_then(MdioDev::new)
            .ok_or_else(|| ParseError(format!("bad MDIO device '{}'", word(2))))?;
        let (phy, reg, val) = (low32(phy), low16(reg), low16(val));
        self.hooks
            .mdio(self.sai.backend_mut().inner_mut(), PhyAddr(phy), dev, reg, val);
        Ok(Out::success())
    }

    fn feature_op(&mut self, w: &[String]) -> Parsed<Out> {
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        need(w, 3)?;
        let feature = match word(1) {
            "qos" => Feature::Qos,
            "mirror" => Feature::Mirror,
            "l2" => Feature::L2,
            _ => return fail("feature qos | mirror | l2"),
        };
        let on = match word(2) {
            "on" => true,
            "off" => false,
            _ => return fail("expected on / off"),
        };
        self.hooks.feature(self.sai.backend_mut().inner_mut(), feature, on);
        Ok(Out::success())
    }

    fn switch_op(&mut self, w: &[String]) -> Parsed<Out> {
        // sai_api_initialize the first time (the rim's always succeeds)
        self.initialized = true;
        let ty = types::named("switch").ok_or_else(|| ParseError("no switch type".into()))?;
        let script = self.attrs_in(ty, w, 1)?;
        let mut attrs: Vec<ScriptAttr> = [
            (sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true)),
            // the driver prints notifications itself; the core only keeps the pointers
            (
                sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY,
                Value::Pointer(Pointer::from_addr(1)),
            ),
            (
                sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY,
                Value::Pointer(Pointer::from_addr(2)),
            ),
        ]
        .into_iter()
        .filter_map(|(id, value)| {
            sai_meta::attribute(ty.ot, id).map(|meta| ScriptAttr {
                meta,
                value: Some(value),
            })
        })
        .collect();
        attrs.extend(script);
        let result = self.create_switch(&attrs);
        let ok = result.is_ok();
        let out = Out::of(&result);
        if ok {
            self.bind_switch_names();
        }
        Ok(out)
    }

    /// The rim's `create_switch` (`sai-ffi/src/generic.rs`): decode, read the profile keys it
    /// knows, select the backend, a new core over it, the switch, the event queue, and the
    /// front-panel port count for its log line (a `ports()` call).
    fn create_switch(&mut self, attrs: &[ScriptAttr]) -> sai_core::Result<()> {
        let attrs = rim_decode(sys::SAI_OBJECT_TYPE_SWITCH, attrs, Call::Create)?;
        // the keys the rim asks the SAI user for (sai-ffi backends.rs profile_keys)
        let mut keys = sai_core::keys::EXPLICIT.to_vec();
        keys.push(sai_diag::SOCKET_KEY);
        keys.extend(sai_core::extphy::PROFILE_KEYS);
        keys.extend(sai_core::virt::PROFILE_KEYS);
        let profile = Profile::from_pairs(
            keys.iter()
                .filter_map(|k| self.profile.get(*k).map(|v| ((*k).to_owned(), v.clone())))
                .collect::<Vec<_>>(),
        );
        // the C++ driver's factory: openbcm is the recorder over the fake (the in-memory chip
        // here too, with the default hooks); anything else the library would have to have
        if self.sai.switch_id().is_some() {
            return Err(Error::ItemAlreadyExists);
        }
        let Some(inner) = self.hooks.backend(profile.get(BACKEND_KEY).unwrap_or_default()) else {
            return Err(Error::Failure);
        };
        let backend = Recording::new(inner, self.mode);
        self.log = backend.log();
        self.sai = Sai::new(backend);
        let id = self.sai.create_switch(&attrs, &profile)?;
        self.events = self.sai.take_events();
        self.switch = id.raw();
        Ok(())
    }

    /// C++ `bind_switch_names`: switch, cpu, vlan1, vr0, bridge, stp0, trapgroup0, port<i> and
    /// bport<i>, through the same get calls.
    fn bind_switch_names(&mut self) {
        let sw = self.switch;
        self.names.bind("switch", sw);
        let get1 = |sai: &Core<B>, ot, oid, attr| match sai.get(ot, oid, &[attr]).as_deref() {
            Ok([v]) => Some(v.clone()),
            _ => None,
        };
        for (attr, name) in [
            (sys::SAI_SWITCH_ATTR_CPU_PORT, "cpu"),
            (sys::SAI_SWITCH_ATTR_DEFAULT_VLAN_ID, "vlan1"),
            (sys::SAI_SWITCH_ATTR_DEFAULT_VIRTUAL_ROUTER_ID, "vr0"),
            (sys::SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID, "bridge"),
            (sys::SAI_SWITCH_ATTR_DEFAULT_STP_INST_ID, "stp0"),
            (sys::SAI_SWITCH_ATTR_DEFAULT_TRAP_GROUP, "trapgroup0"),
        ] {
            if let Some(Value::Oid(o)) = get1(&self.sai, sys::SAI_OBJECT_TYPE_SWITCH, sw, attr) {
                if !o.is_null() {
                    self.names.bind(name, o);
                }
            }
        }
        // the C++ driver's buffers: 256 ports, 512 bridge ports
        let Some(Value::OidList(ports)) = get1(
            &self.sai,
            sys::SAI_OBJECT_TYPE_SWITCH,
            sw,
            sys::SAI_SWITCH_ATTR_PORT_LIST,
        ) else {
            return;
        };
        if ports.len() > LIST_CAPACITY {
            return;
        }
        for (i, p) in ports.iter().enumerate() {
            self.names.bind(&format!("port{i}"), *p);
        }
        if !self.names.contains("bridge") {
            return;
        }
        let Ok(bridge) = self.names.oid("bridge") else {
            return;
        };
        let Some(Value::OidList(bports)) = get1(
            &self.sai,
            sys::SAI_OBJECT_TYPE_BRIDGE,
            bridge,
            sys::SAI_BRIDGE_ATTR_PORT_LIST,
        ) else {
            return;
        };
        if bports.len() > 2 * LIST_CAPACITY {
            return;
        }
        for bp in bports {
            if let Some(Value::Oid(p)) = get1(
                &self.sai,
                sys::SAI_OBJECT_TYPE_BRIDGE_PORT,
                bp,
                sys::SAI_BRIDGE_PORT_ATTR_PORT_ID,
            ) {
                for (i, port) in ports.iter().enumerate() {
                    if *port == p {
                        self.names.bind(&format!("bport{i}"), bp);
                    }
                }
            }
        }
    }

    fn create_op(&mut self, w: &[String]) -> Parsed<Out> {
        need(w, 3)?;
        let ty = need_type(w, 1)?;
        let name = w.get(2).map_or("", String::as_str);
        if ty.shape == Shape::Entry {
            let key = self.key_of(ty, name)?;
            let attrs = self.attrs_in(ty, w, 3)?;
            let result =
                rim_decode(ty.ot, &attrs, Call::Create).and_then(|a| self.sai.create_entry(self.switch, key, &a));
            return Ok(Out::of(&result));
        }
        if !ty.has_create() {
            return fail(format!("{} has no create", ty.name));
        }
        if !valid_name(name) {
            return fail(format!("bad name '{name}'"));
        }
        if self.names.contains(name) {
            return fail(format!("name '{name}' is bound"));
        }
        let attrs = self.attrs_in(ty, w, 3)?;
        let result = implemented(ty.ot)
            .and_then(|()| rim_decode(ty.ot, &attrs, Call::Create))
            .and_then(|a| self.sai.create(ty.ot, self.switch, &a));
        if let Ok(id) = result {
            self.names.bind(name, id);
        }
        Ok(Out::of(&result))
    }

    fn get_op(&mut self, w: &[String]) -> Parsed<Out> {
        need(w, 4)?;
        let ty = need_type(w, 1)?;
        let asked = w
            .iter()
            .skip(3)
            .map(|s| attr_of(ty, s).map(|(meta, kind)| (s.as_str(), meta, kind)))
            .collect::<Parsed<Vec<_>>>()?;
        let ids: Vec<sys::sai_attr_id_t> = asked.iter().map(|(_, m, _)| m.id).collect();
        let result = if ty.shape == Shape::Entry {
            let key = self.key_of(ty, w.get(2).map_or("", String::as_str))?;
            self.sai.get_entry(self.switch, &key, &ids)
        } else {
            let oid = self.names.oid(w.get(2).map_or("", String::as_str))?;
            implemented(ty.ot).and_then(|()| self.sai.get_any(ty.ot, oid, &ids))
        };
        let values = match result {
            Ok(values) => values,
            Err(e) => return Ok(Out::of(&Err::<(), _>(e))),
        };
        // The rim's encode_all: every value is written; a list longer than the buffer the C++
        // driver gives (256) makes the call BUFFER_OVERFLOW.
        let overflow = values.iter().any(|v| list_count(v).is_some_and(|n| n > LIST_CAPACITY));
        if overflow {
            let mut out = Out::of(&Err::<(), _>(Error::BufferOverflow));
            for ((short, _, kind), v) in asked.iter().zip(&values) {
                if is_list(*kind) {
                    out.lines
                        .push(format!("{short} count={}", list_count(v).unwrap_or(LIST_CAPACITY)));
                }
            }
            return Ok(out);
        }
        let mut out = Out::success();
        for ((short, _, kind), v) in asked.iter().zip(&values) {
            out.lines
                .push(format!("{short}={}", value_out(*kind, v, &mut self.names)));
        }
        Ok(out)
    }

    fn inject_op(&mut self, w: &[String]) -> Parsed<Out> {
        need(w, 2)?;
        let word = |i: usize| w.get(i).map_or("", String::as_str);
        match word(1) {
            "link" => {
                need(w, 4)?;
                let up = match word(3) {
                    "up" => true,
                    "down" => false,
                    _ => return fail("expected up / down"),
                };
                let dest = self.dest_of(word(2))?;
                self.hooks.link(self.sai.backend_mut().inner_mut(), dest, up);
            }
            "fdb" => {
                need(w, 6)?;
                let change = match word(2) {
                    "learned" => FdbChange::Learned,
                    "aged" => FdbChange::Aged,
                    _ => return fail("expected learned / aged"),
                };
                let mac = parse_mac(word(3))?;
                let vid =
                    Vid::new(low16(num(word(4))?)).map_err(|_| ParseError(format!("bad VLAN id '{}'", word(4))))?;
                let hw_port = self.dest_of(word(5))?;
                self.hooks.fdb(
                    self.sai.backend_mut().inner_mut(),
                    FdbEvent {
                        change,
                        mac: MacAddr(mac),
                        vid,
                        hw_port,
                    },
                );
            }
            "stat" => {
                need(w, 5)?;
                let Dest::Port(hw) = self.dest_of(word(2))? else {
                    return fail(format!("'{}' is no port", word(2)));
                };
                let port = types::named("port").ok_or_else(|| ParseError("no port type".into()))?;
                let id = stat_of(port, word(3))?;
                let stat = PortStat::from_sai_id(id)
                    .ok_or_else(|| ParseError(format!("the virtual backend has no counter {}", word(3))))?;
                #[allow(clippy::cast_sign_loss, reason = "the C driver's (uint64_t)num()")]
                let value = num(word(4))? as u64;
                self.hooks.stat(self.sai.backend_mut().inner_mut(), hw, stat, value);
            }
            _ => return fail("inject link | fdb | stat"),
        }
        Ok(Out::success())
    }

    /// The backend's destination for a port or LAG (C++ `dest_of`: `tp::backend_id`, LAG -> `lag_dest`).
    fn dest_of(&self, name: &str) -> Parsed<Dest> {
        let oid = self.names.oid(name)?;
        let none = || ParseError(format!("'{name}' has no backend id"));
        let store = self.sai.store().ok_or_else(none)?;
        match ObjectType::from_sai(oid.sai_object_type()) {
            Some(ObjectType::Port) => {
                let id = PortId::try_from(oid).map_err(|_| none())?;
                Ok(Dest::Port(store.get::<Port>(id).map_err(|_| none())?.hw))
            }
            Some(ObjectType::Lag) => {
                let id = LagId::try_from(oid).map_err(|_| none())?;
                Ok(Dest::Lag(store.get::<Lag>(id).map_err(|_| none())?.tid))
            }
            _ => Err(none()),
        }
    }

    /// An entry's key: `<mac>@<bv>`, `<ip>@<rif>`, `<prefix>@<vr>` (C++ `key_of`).
    fn key_of(&self, ty: ObjType, s: &str) -> Parsed<EntryKey> {
        let Some((a, b)) = s.rsplit_once('@') else {
            return fail("expected <key>@<name>");
        };
        Ok(match ty.ot {
            sys::SAI_OBJECT_TYPE_FDB_ENTRY => {
                let mac = parse_mac(a)?;
                EntryKey::Fdb {
                    bv_id: self.names.oid(b)?,
                    mac,
                }
            }
            sys::SAI_OBJECT_TYPE_NEIGHBOR_ENTRY => {
                let ip = parse_ip(a)?;
                EntryKey::Neighbor {
                    rif: self.names.oid(b)?,
                    ip,
                }
            }
            _ => {
                let (addr, mask) = parse_prefix(a)?;
                let prefix = IpPrefix::new(addr, mask).ok_or_else(|| ParseError(format!("bad prefix '{a}'")))?;
                EntryKey::Route {
                    vr: self.names.oid(b)?,
                    prefix,
                }
            }
        })
    }

    /// The attributes `w[from..]` of a create / set / flush (C++ `attrs_in`).
    fn attrs_in(&self, ty: ObjType, w: &[String], from: usize) -> Parsed<Vec<ScriptAttr>> {
        w.iter()
            .skip(from)
            .map(|word| {
                let Some((short, text)) = word.split_once('=') else {
                    return fail(format!("expected <ATTR>=<value>, got '{word}'"));
                };
                let (meta, kind) = attr_of(ty, short)?;
                let value = value_in(kind, meta.value_type, text, &self.names)?;
                if let Some(v) = &value {
                    let undecoded = matches!(meta.value_type, ValueType::Other | ValueType::AclCapability);
                    if !undecoded && v.value_type() != meta.value_type {
                        return fail(format!(
                            "driver: {} is {kind:?} to the script but travels as {:?}",
                            meta.name, meta.value_type
                        ));
                    }
                }
                Ok(ScriptAttr { meta, value })
            })
            .collect()
    }
}

/// The rim's `implemented`: an object type neither the core nor the `MACsec` model has is
/// `NOT_IMPLEMENTED`.
fn implemented(ot: sys::sai_object_type_t) -> sai_core::Result<()> {
    if ObjectType::from_sai(ot).is_some() || MacsecObject::of_macsec_api(ot).is_some() {
        Ok(())
    } else {
        Err(Error::NotImplemented)
    }
}

/// Whether the metadata lets a set carry the (one) attribute.
fn settable(ot: sys::sai_object_type_t, attrs: &[ScriptAttr]) -> bool {
    attrs
        .first()
        .and_then(|a| sai_meta::attribute(ot, a.meta.id))
        .is_some_and(|m| m.flags.settable())
}

/// What the rim does to each attribute before the core sees it (`sai-ffi/src/generic.rs`,
/// `decode_one` / `decode_list`): an attribute a create may not carry (not an attribute of the
/// type, read-only): the type's C++ answer (`sai_core::dispatch::refused_create`); one a set may
/// not carry: `UNKNOWN_ATTRIBUTE_0` / `INVALID_ATTRIBUTE_0`, which the caller replaces by the
/// type's C++ answer (`Sai::refused_set`); a member the rim does not decode:
/// `ATTR_NOT_SUPPORTED_<i>` (`convert::decode`); `i` = position.
fn rim_decode(ot: sys::sai_object_type_t, attrs: &[ScriptAttr], call: Call) -> sai_core::Result<Vec<Attr>> {
    if MacsecObject::of_macsec_api(ot).is_some() {
        return Ok(attrs.iter().map(|a| macsec_decode(ot, a, call)).collect());
    }
    attrs
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let meta = sai_meta::attribute(ot, a.meta.id);
            let allowed = match call {
                Call::Create => meta.is_some_and(|m| m.flags.creatable()),
                Call::Set => meta.is_some_and(|m| m.flags.settable()),
            };
            let Some(meta) = meta.filter(|_| allowed) else {
                return Err(match (call, meta) {
                    (Call::Create, _) => sai_core::dispatch::refused_create(ot, a.meta.id, i),
                    (Call::Set, None) => Error::attr(AttrError::Unknown, i),
                    (Call::Set, Some(_)) => Error::attr(AttrError::Invalid, i),
                });
            };
            match (&a.value, meta.value_type) {
                (_, ValueType::Other | ValueType::AclCapability) | (None, _) => {
                    Err(Error::attr(AttrError::NotSupported, i))
                }
                (Some(v), _) => Ok(Attr::new(meta.id, v.clone())),
            }
        })
        .collect()
}

/// The rim's `decode_macsec` (`sai-ffi/src/generic.rs`): no metadata check (the `MACsec` model
/// answers as the C++); an attribute the call may carry in a member the model uses is passed on,
/// anything else as a placeholder the model refuses by its id.
fn macsec_decode(ot: sys::sai_object_type_t, a: &ScriptAttr, call: Call) -> Attr {
    let usable = sai_meta::attribute(ot, a.meta.id).is_some_and(|m| {
        (match call {
            Call::Create => m.flags.creatable(),
            Call::Set => m.flags.settable(),
        }) && matches!(
            m.value_type,
            ValueType::Bool
                | ValueType::U8
                | ValueType::U32
                | ValueType::U64
                | ValueType::Enum
                | ValueType::I32
                | ValueType::Oid
                | ValueType::OidList
                | ValueType::I32List
                | ValueType::MacsecSak
                | ValueType::MacsecSalt
                | ValueType::MacsecAuthKey
        )
    });
    match &a.value {
        Some(v) if usable => Attr::new(a.meta.id, v.clone()),
        _ => Attr::new(a.meta.id, Value::U32(0)),
    }
}

fn need(w: &[String], n: usize) -> Parsed<()> {
    if w.len() < n { fail("too few words") } else { Ok(()) }
}

fn need_type(w: &[String], i: usize) -> Parsed<ObjType> {
    let Some(name) = w.get(i) else {
        return fail("missing <type>");
    };
    types::named(name).ok_or_else(|| ParseError(format!("unknown type '{name}'")))
}

/// A name for a new object: `[a-z][a-z0-9_.#]*`, not `null` (C++ `valid_name`).
fn valid_name(n: &str) -> bool {
    n.starts_with(|c: char| c.is_ascii_lowercase())
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '#'))
        && n != "null"
}

/// The metadata and kind of `SAI_<TYPE>_ATTR_<short>` (C++ `attr_of`). The C++ name table also
/// has the alias enumerators (`SAI_SWITCH_ATTR_PORT_NUMBER = SAI_SWITCH_ATTR_NUMBER_OF_ACTIVE_PORTS`,
/// `@ignore`d, which `sai-meta` leaves out): a name `sai-meta` lacks is looked up as an enumerator
/// of `sai_<type>_attr_t` (range markers excluded) and taken by its id.
fn attr_of(ty: ObjType, short: &str) -> Parsed<(&'static AttrMeta, Kind)> {
    let full = format!("SAI_{}_ATTR_{short}", ty.upper());
    let attrs = sai_meta::attributes(ty.ot);
    let alias = || {
        let id = crate::names::enum_value(&format!("sai_{}_attr_t", ty.name), &full)
            .filter(|_| !crate::names::is_marker(&full))?;
        attrs.iter().find(|a| i64::from(a.id) == id)
    };
    let Some(meta) = attrs.iter().find(|a| a.name == full).or_else(alias) else {
        return fail(format!("no attribute {full}"));
    };
    let kind =
        kind_of(meta.type_name).ok_or_else(|| ParseError(format!("{full}: unsupported type '{}'", meta.type_name)))?;
    Ok((meta, kind))
}

/// `SAI_<TYPE>_STAT_<short>` of `sai_<type>_stat_t` (C++ `stat_of`).
fn stat_of(ty: ObjType, short: &str) -> Parsed<sys::sai_stat_id_t> {
    crate::names::enum_value(
        &format!("sai_{}_stat_t", ty.name),
        &format!("SAI_{}_STAT_{short}", ty.upper()),
    )
    .and_then(|v| sys::sai_stat_id_t::try_from(v).ok())
    .ok_or_else(|| ParseError(format!("no counter {short} of {}", ty.name)))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the C driver's (unsigned) / (uint16_t)"
)]
const fn low32(v: i64) -> u32 {
    v as u32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the C driver's (uint16_t)"
)]
const fn low16(v: i64) -> u16 {
    v as u16
}

//! The `MACsec` model natively in the Rust library: `sai_core::macsec`'s seam ([`MacsecModel`],
//! [`MacsecLoader`]) implemented over `sai-macsec` (the objects) and `phy-seq` (the board's
//! table, run on the bus the core lends for each call, under the external-PHY lock). No C ABI and
//! no C++ in between; the rim registers [`LOADER`] with the core.
//!
//! What this module adds to [`crate::PhySeqRunner`]'s path, and nothing else:
//!
//! - **The bus of a call.** The model's calls take a [`SeqRunner`]; here one is made per call
//!   ([`CallRunner`]) around the core's [`MacsecIo`]: a sequence runs inside
//!   [`MacsecIo::with_bus`] (the backend's `mdio_c45_read` / `_write`, so the recording backend
//!   logs every access), the gap goes to [`MacsecIo::set_ifg`] (the port code's
//!   `port_ifg_apply`).
//! - **Ids.** `MACsec`-stage ACL tables and entries get the id the ACL area chose
//!   ([`MacsecIo::next_id`], sai-macsec's D7); the `MACsec` objects count by themselves.
//! - **Values.** The core's attribute values to the model's and back, by attribute: the key
//!   material is the same [`Secret`] type on both sides (cloned into the model's attribute: the
//!   clone is wiped on its own drop); ACL fields / actions as the ACL area decodes them.
//! - **The trace** of `extmacsec trace` (`sai_extmacsec.cpp`): the bus operations of sequence runs,
//!   key words as `****`, read values not shown, the last 50000 lines. Per switch here (the C++
//!   keeps one per process).
use crate::{BusStatus, model_results, model_table, phy_params, seq_error};
use phy_seq::{Dest, MacsecTable, Op, OpKind};
use sai_core::backend::HwPort;
use sai_core::backend::types::{MdioDev, PhyAddr};
use sai_core::macsec::{
    AclSide, ChipPortInfo, MacsecBus, MacsecIo, MacsecLoader, MacsecModel, MacsecObject, MacsecOptions, MacsecPending,
    Secret, StatsMode,
};
use sai_core::oid::{PortId, RawOid};
use sai_core::value::{AclAction as CoreAction, AclField as CoreField, AclFieldData, AclKind, AclParam, FieldMatch};
use sai_core::{Error, Result};
use sai_macsec::{
    AclEntryId, AclField, AclParameter, AclTableId, Attr, ChipPort, Dir, FlowId, Level, Macsec, MacsecId, MacsecPortId,
    Options, Params, PortKey, Results, SaId, ScId, SeqError, SeqRunner, Value,
};
use sai_sys as sys;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex, PoisonError};

/// The loader the rim registers ([`sai_core::macsec::register_loader`]).
pub static LOADER: NativeLoader = NativeLoader;

/// Reads and parses `SAI_OPENBCM_EXT_MACSEC_FILE` and makes the switch's model.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeLoader;

fn log_switch(text: &str) {
    sai_core::log::error(sys::SAI_API_SWITCH, format_args!("{text}"));
}

impl MacsecLoader for NativeLoader {
    fn load(&self, path: &str) -> Result<Box<dyn MacsecPending>> {
        let Ok(text) = std::fs::read(path) else {
            log_switch(&format!(
                "SAI_OPENBCM_EXT_MACSEC_FILE {path}: cannot open; refusing to create the switch"
            ));
            return Err(Error::Failure);
        };
        match MacsecTable::parse(&text) {
            Ok(table) => Ok(Box::new(Pending(table))),
            Err(e) => {
                let mut msg = Vec::new();
                e.write_to(&mut msg);
                log_switch(&format!(
                    "SAI_OPENBCM_EXT_MACSEC_FILE {path}: {}; refusing to create the switch",
                    String::from_utf8_lossy(&msg)
                ));
                Err(Error::InvalidParameter)
            }
        }
    }

    fn without_table(&self) -> Box<dyn MacsecModel> {
        let alloc = AllocCell::default();
        Box::new(Native {
            model: Macsec::without_table(options(&alloc, MacsecOptions::default())),
            engine: None,
            trace: Trace::default(),
            alloc,
        })
    }
}

/// A parsed table, waiting for the backend.
struct Pending(MacsecTable);

impl MacsecPending for Pending {
    fn attach(
        self: Box<Self>,
        io: &mut dyn MacsecIo,
        ports: &[ChipPortInfo],
        macsec: MacsecOptions,
    ) -> Result<Box<dyn MacsecModel>> {
        let engine = Engine::new(self.0, ports);
        let chip: Vec<ChipPort> = ports
            .iter()
            .map(|p| ChipPort {
                key: key_of(p.hw),
                name: p.name.clone(),
                lanes: p.lanes.clone(),
            })
            .collect();
        let alloc = AllocCell::default();
        let mut trace = Trace::default();
        let model = {
            let mut runner = CallRunner {
                engine: Some(&engine),
                io,
                trace: &mut trace,
            };
            Macsec::attach_chip(&model_table(&engine.table), &chip, options(&alloc, macsec), &mut runner)?
        };
        Ok(Box::new(Native {
            model,
            engine: Some(engine),
            trace,
            alloc,
        }))
    }
}

fn key_of(hw: HwPort) -> PortKey {
    PortKey(u32::from(hw.0))
}

fn hw_of(key: PortKey) -> Option<HwPort> {
    u16::try_from(key.0).ok().map(HwPort)
}

/// The id the ACL area chose for the table / entry being created.
type AllocCell = Arc<Mutex<Option<RawOid>>>;

/// The API a model's log line belongs to (the C++ `OB_*` calls of `sai_macsec.cpp`).
fn api_of(text: &str) -> sys::sai_api_t {
    if text.starts_with("external MACsec table:") {
        sys::SAI_API_SWITCH
    } else if text.starts_with("MACsec ACL:") {
        sys::SAI_API_ACL
    } else {
        sys::SAI_API_MACSEC
    }
}

/// The model's options: its log into the library's, ACL ids from the ACL area, the rest counted.
fn options(alloc: &AllocCell, macsec: MacsecOptions) -> Options {
    let cell = Arc::clone(alloc);
    let mut next: u64 = 0;
    Options {
        log: Some(Box::new(|level: Level, text: &str| {
            let level = match level {
                Level::Error => sys::SAI_LOG_LEVEL_ERROR,
                Level::Warn => sys::SAI_LOG_LEVEL_WARN,
                Level::Notice => sys::SAI_LOG_LEVEL_NOTICE,
                Level::Info => sys::SAI_LOG_LEVEL_INFO,
            };
            sai_core::log::emit(api_of(text), level, format_args!("{text}"));
        })),
        alloc: Some(Box::new(move |object_type| {
            if object_type == sys::SAI_OBJECT_TYPE_ACL_TABLE || object_type == sys::SAI_OBJECT_TYPE_ACL_ENTRY {
                return cell.lock().unwrap_or_else(PoisonError::into_inner).take();
            }
            next += 1;
            Some(RawOid((u64::from(object_type) << 56) | next))
        })),
        engines_on_at_stop: macsec.engines_on_at_stop,
        ..Options::default()
    }
}

/// The table and where its ports' PHYs are.
struct Engine {
    table: MacsecTable,
    names: Vec<String>,
    phy_of: BTreeMap<PortKey, u16>,
}

impl Engine {
    /// Each table port is the last chip port whose first lane is the table's (`ext_macsec_attach`).
    fn new(table: MacsecTable, ports: &[ChipPortInfo]) -> Self {
        let phy_of = table
            .ports()
            .filter_map(|(lane, phy)| {
                let port = ports.iter().rfind(|p| p.lanes.first() == Some(&lane))?;
                Some((key_of(port.hw), phy))
            })
            .collect();
        let names = table.sequences().map(|(name, _)| name.to_owned()).collect();
        Self { table, names, phy_of }
    }

    fn result_names(&self, name: &str) -> Vec<String> {
        self.table.sequence(name).map_or_else(Vec::new, |seq| {
            seq.ops()
                .iter()
                .filter_map(|op| match &op.kind {
                    OpKind::Keep {
                        dest: Dest::Result { name, .. },
                        ..
                    } => Some(name.as_str().to_owned()),
                    _ => None,
                })
                .collect()
        })
    }
}

// ------------------------------------------------------------------ the trace

/// The most lines kept (the oldest go first).
const TRACE_LINES: usize = 50_000;

/// What is traced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TraceMode {
    /// Nothing (the default).
    #[default]
    Off,
    /// Sequences that write.
    Writes,
    /// Every sequence.
    All,
}

#[derive(Debug, Default)]
struct Trace {
    mode: TraceMode,
    lines: VecDeque<String>,
    runs: u64,
    dropped: u64,
}

impl Trace {
    fn wants(&self, ops: &[Op]) -> bool {
        match self.mode {
            TraceMode::Off => false,
            TraceMode::Writes => ops.iter().any(|o| matches!(o.kind, OpKind::Write { .. })),
            TraceMode::All => true,
        }
    }

    fn keep(&mut self, phy: u16, ops: &[Op], lines: Vec<String>, status: i32) {
        self.runs += 1;
        self.lines.push_back(format!(
            "# run {}: phy 0x{phy:x}, {} operations from line {}",
            self.runs,
            ops.len(),
            ops.first().map_or(0, |o| o.line)
        ));
        self.lines.extend(lines);
        self.lines.push_back(format!("= {status}"));
        while self.lines.len() > TRACE_LINES {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }

    /// `extmacsec trace [on [all] | off | dump | clear]` (C++ `ext_macsec_trace`).
    fn command(&mut self, args: &str) -> String {
        let mut words = args.split_ascii_whitespace();
        let (cmd, arg) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
        match cmd {
            "on" => {
                self.mode = if arg == "all" {
                    TraceMode::All
                } else {
                    TraceMode::Writes
                }
            }
            "off" => self.mode = TraceMode::Off,
            "" | "dump" => {}
            "clear" => {
                self.lines.clear();
                self.runs = 0;
                self.dropped = 0;
            }
            _ => {
                return "usage: extmacsec trace [on [all] | off | dump | clear]   (on: sequences that write; on all: every sequence)\n"
                    .to_owned();
            }
        }
        let mode = match self.mode {
            TraceMode::Off => "off",
            TraceMode::Writes => "on (sequences that write)",
            TraceMode::All => "on (all sequences)",
        };
        let mut s = format!(
            "extmacsec trace: {mode}, engine rust (phy-seq), {} runs, {} lines kept, {} dropped\n",
            self.runs,
            self.lines.len(),
            self.dropped
        );
        if cmd == "dump" {
            for line in &self.lines {
                s.push_str(line);
                s.push('\n');
            }
        }
        s
    }
}

// ------------------------------------------------------------------ the runner of one call

/// phy-seq's bus over the core's, with the trace.
struct CoreBus<'a> {
    bus: &'a mut dyn MacsecBus,
    /// The traced lines of this run (`None`: not traced).
    lines: Option<Vec<String>>,
    /// The operation the next accesses belong to.
    op: Option<&'a Op>,
    ops: &'a [Op],
}

fn dev_of(dev: phy_seq::Dev) -> core::result::Result<MdioDev, BusStatus> {
    MdioDev::new(dev.get()).ok_or(BusStatus(Error::InvalidParameter.status()))
}

impl phy_seq::Bus for CoreBus<'_> {
    type Error = BusStatus;

    fn read(&mut self, phy: u16, dev: phy_seq::Dev, reg: u16) -> core::result::Result<u16, BusStatus> {
        if let Some(lines) = self.lines.as_mut() {
            lines.push(format!("R {phy:03x} {:x} {reg:04x}", dev.get()));
        }
        self.bus
            .read(PhyAddr(u32::from(phy)), dev_of(dev)?, reg)
            .map_err(|e| BusStatus(e.status()))
    }

    fn write(&mut self, phy: u16, dev: phy_seq::Dev, reg: u16, value: u16) -> core::result::Result<(), BusStatus> {
        if let Some(lines) = self.lines.as_mut() {
            // a word made of key material is never recorded
            let secret = self
                .op
                .is_some_and(|op| matches!(&op.kind, OpKind::Write { value, .. } if value.is_secret()));
            lines.push(if secret {
                format!("W {phy:03x} {:x} {reg:04x} ****", dev.get())
            } else {
                format!("W {phy:03x} {:x} {reg:04x} {value:04x}", dev.get())
            });
        }
        self.bus
            .write(PhyAddr(u32::from(phy)), dev_of(dev)?, reg, value)
            .map_err(|e| BusStatus(e.status()))
    }

    fn delay_us(&mut self, us: u32) {
        if let Some(lines) = self.lines.as_mut() {
            lines.push(format!("D {us}"));
        }
        self.bus.delay_us(us);
    }

    fn begin(&mut self, index: usize, _op: &Op) {
        self.op = self.ops.get(index);
    }
}

/// The model's [`SeqRunner`] for one call: the table, the core's io, the trace.
struct CallRunner<'a> {
    engine: Option<&'a Engine>,
    io: &'a mut dyn MacsecIo,
    trace: &'a mut Trace,
}

impl SeqRunner for CallRunner<'_> {
    fn sequence_names(&self) -> Vec<String> {
        self.engine.map_or_else(Vec::new, |e| e.names.clone())
    }

    fn has(&self, name: &str) -> bool {
        self.engine.is_some_and(|e| e.table.sequence(name).is_some())
    }

    fn result_names(&self, name: &str) -> Vec<String> {
        self.engine.map_or_else(Vec::new, |e| e.result_names(name))
    }

    fn run(&mut self, port: PortKey, name: &str, params: &Params) -> core::result::Result<Results, SeqError> {
        let Some(engine) = self.engine else {
            return Err(SeqError {
                status: Error::NotSupported,
                why: "no external MACsec table".to_owned(),
            });
        };
        let Some(&phy) = engine.phy_of.get(&port) else {
            return Err(SeqError {
                status: Error::NotSupported,
                why: format!("port {} has no MACsec PHY in the table", port.0),
            });
        };
        let Some(seq) = engine.table.sequence(name) else {
            return Err(SeqError {
                status: Error::NotSupported,
                why: format!("the external MACsec table has no sequence {name}"),
            });
        };
        let ops = seq.ops();
        let traced = self.trace.wants(ops);
        let par = phy_params(params);
        let mut outcome = None;
        let mut lines = None;
        self.io.with_bus(&mut |bus| {
            let mut on = CoreBus {
                bus,
                lines: traced.then(Vec::new),
                op: None,
                ops,
            };
            outcome = Some(seq.run(&mut on, phy, &par));
            lines = on.lines;
        });
        drop(par); // wiped
        let outcome = outcome
            .ok_or_else(|| SeqError {
                status: Error::Failure,
                why: "the bus was not lent".to_owned(),
            })?
            .map(|results| model_results(&results))
            .map_err(|e| seq_error(&e, |status| status.error()));
        if let Some(lines) = lines {
            let status = outcome
                .as_ref()
                .map_or_else(|e| e.status.status(), |_| sys::SAI_STATUS_SUCCESS);
            self.trace.keep(phy, ops, lines, status);
        }
        outcome
    }

    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> core::result::Result<(), SeqError> {
        let hw = hw_of(port).ok_or(SeqError {
            status: Error::InvalidParameter,
            why: format!("port {} is not a chip port", port.0),
        })?;
        self.io.set_ifg(hw, macsec_bits).map_err(|e| SeqError {
            status: e,
            why: format!("inter-frame gap not applied ({})", e.status()),
        })
    }

    fn poll_deferred(&mut self, port: PortKey, name: &str) -> bool {
        hw_of(port).is_some_and(|hw| self.io.poll_deferred(hw, name))
    }
}

/// The table for questions that run nothing (status text, capability).
struct TableView<'a>(Option<&'a Engine>);

impl SeqRunner for TableView<'_> {
    fn sequence_names(&self) -> Vec<String> {
        self.0.map_or_else(Vec::new, |e| e.names.clone())
    }
    fn has(&self, name: &str) -> bool {
        self.0.is_some_and(|e| e.table.sequence(name).is_some())
    }
    fn result_names(&self, name: &str) -> Vec<String> {
        self.0.map_or_else(Vec::new, |e| e.result_names(name))
    }
    fn run(&mut self, _port: PortKey, _name: &str, _params: &Params) -> core::result::Result<Results, SeqError> {
        Err(SeqError {
            status: Error::Failure,
            why: "no bus outside a call".to_owned(),
        })
    }
    fn set_ifg(&mut self, _port: PortKey, _macsec_bits: u32) -> core::result::Result<(), SeqError> {
        Ok(())
    }
}

// ------------------------------------------------------------------ values

/// A core attribute as the model takes it. Values of a kind the model has no use for become a
/// placeholder it refuses by the attribute's id (as the C++ does).
fn to_model(a: &sai_core::value::Attr) -> Attr {
    use sai_core::value::Value as V;
    let value = match &a.value {
        V::Bool(b) => Value::Bool(*b),
        V::U8(x) => Value::U8(*x),
        V::U32(x) => Value::U32(*x),
        V::U64(x) => Value::U64(*x),
        V::Enum(x) | V::I32(x) => Value::Enum(*x),
        V::Oid(o) => Value::Oid(*o),
        V::OidList(l) => Value::OidList(l.clone()),
        V::I32List(l) => Value::I32List(l.clone()),
        V::MacsecSak(s) => Value::Sak(s.clone()),
        V::MacsecSalt(s) => Value::Salt(s.clone()),
        V::MacsecAuthKey(s) => Value::AuthKey(s.clone()),
        V::AclField(CoreField::Enabled(AclFieldData::Mac(m))) => Value::AclField(Some(AclField::Mac {
            data: m.data,
            mask: m.mask,
        })),
        V::AclField(CoreField::Enabled(AclFieldData::U16(m))) => Value::AclField(Some(AclField::U16 {
            data: m.data,
            mask: m.mask,
        })),
        V::AclField(CoreField::Enabled(AclFieldData::U64(m))) => Value::AclField(Some(AclField::U64 {
            data: m.data,
            mask: m.mask,
        })),
        V::AclField(_) => Value::AclField(None),
        V::AclAction(CoreAction::Enabled(p)) => Value::AclAction {
            enable: true,
            parameter: match p {
                AclParam::Oid(o) => AclParameter::Oid(*o),
                AclParam::Enum(x) | AclParam::I32(x) => AclParameter::S32(*x),
                _ => AclParameter::S32(0),
            },
        },
        V::AclAction(CoreAction::Disabled(kind)) => Value::AclAction {
            enable: false,
            parameter: if *kind == AclKind::Oid {
                AclParameter::Oid(RawOid::NULL)
            } else {
                AclParameter::S32(0)
            },
        },
        _ => Value::U32(0),
    };
    Attr::new(a.id, value)
}

fn to_model_all(attrs: &[sai_core::value::Attr]) -> Vec<Attr> {
    attrs.iter().map(to_model).collect()
}

/// The member an ACL field / action of `attr` travels in (the header's `@type`).
fn acl_kind(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> AclKind {
    match sai_meta::attribute(object_type, attr).map(|m| m.value_type) {
        Some(sai_meta::ValueType::AclField(kind) | sai_meta::ValueType::AclAction(kind)) => kind,
        _ => AclKind::Bool,
    }
}

/// A value the model answered, as the core encodes it. Key material never comes back.
fn to_core(object: MacsecObject, attr: sys::sai_attr_id_t, value: Value) -> Result<sai_core::value::Value> {
    use sai_core::value::Value as V;
    let kind = || acl_kind(object.sai(), attr);
    Ok(match value {
        Value::Bool(b) => V::Bool(b),
        Value::U8(x) => V::U8(x),
        Value::U32(x) => V::U32(x),
        Value::U64(x) => V::U64(x),
        Value::Enum(x) => V::Enum(x),
        Value::Oid(o) => V::Oid(o),
        Value::OidList(l) => V::OidList(l),
        Value::I32List(l) => V::I32List(l),
        Value::AclField(Some(AclField::Mac { data, mask })) => {
            V::AclField(CoreField::Enabled(AclFieldData::Mac(FieldMatch { data, mask })))
        }
        Value::AclField(Some(AclField::U16 { data, mask })) => {
            V::AclField(CoreField::Enabled(AclFieldData::U16(FieldMatch { data, mask })))
        }
        Value::AclField(Some(AclField::U64 { data, mask })) => {
            V::AclField(CoreField::Enabled(AclFieldData::U64(FieldMatch { data, mask })))
        }
        Value::AclField(None) => V::AclField(CoreField::Disabled(kind())),
        Value::AclAction { enable: false, .. } => V::AclAction(CoreAction::Disabled(kind())),
        Value::AclAction {
            parameter: AclParameter::Oid(o),
            ..
        } => V::AclAction(CoreAction::Enabled(AclParam::Oid(o))),
        Value::AclAction {
            parameter: AclParameter::S32(x),
            ..
        } => V::AclAction(CoreAction::Enabled(if kind() == AclKind::I32 {
            AclParam::I32(x)
        } else {
            AclParam::Enum(x)
        })),
        Value::Sak(_) | Value::Salt(_) | Value::AuthKey(_) => return Err(Error::Failure),
    })
}

fn to_core_all(
    object: MacsecObject,
    ids: &[sys::sai_attr_id_t],
    values: Vec<Value>,
) -> Result<Vec<sai_core::value::Value>> {
    ids.iter()
        .zip(values)
        .enumerate()
        .map(|(i, (attr, v))| to_core(object, *attr, v).map_err(|e| e.at_index(i)))
        .collect()
}

/// The id as the typed id of its type; another type is "no such object" (the C++ looks it up
/// in that type's map).
fn typed<T: TryFrom<RawOid>>(id: RawOid) -> Result<T> {
    T::try_from(id).map_err(|_| Error::InvalidObjectId)
}

const fn dir_of(side: AclSide) -> Dir {
    match side {
        AclSide::Ingress => Dir::Ingress,
        AclSide::Egress => Dir::Egress,
    }
}

// ------------------------------------------------------------------ the model

/// The `MACsec` model of one switch with its table.
struct Native {
    model: Macsec,
    engine: Option<Engine>,
    trace: Trace,
    alloc: AllocCell,
}

impl fmt::Debug for Native {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Native")
            .field("model", &self.model)
            .field("sequences", &self.engine.as_ref().map_or(0, |e| e.names.len()))
            .finish_non_exhaustive()
    }
}

impl Native {
    /// The model and a runner for one call (split borrows).
    fn parts<'a>(&'a mut self, io: &'a mut dyn MacsecIo) -> (&'a mut Macsec, CallRunner<'a>) {
        let runner = CallRunner {
            engine: self.engine.as_ref(),
            io,
            trace: &mut self.trace,
        };
        (&mut self.model, runner)
    }
}

impl MacsecModel for Native {
    fn active(&self) -> bool {
        self.model.active()
    }

    fn create(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        attrs: &[sai_core::value::Attr],
    ) -> Result<RawOid> {
        let attrs = to_model_all(attrs);
        *self.alloc.lock().unwrap_or_else(PoisonError::into_inner) = io.next_id(object);
        let alloc = Arc::clone(&self.alloc);
        let (m, mut r) = self.parts(io);
        let made: Result<RawOid> = match object {
            MacsecObject::Macsec => m.create_macsec(&attrs).map(Into::into),
            MacsecObject::Port => m.create_macsec_port(&mut r, &attrs).map(Into::into),
            MacsecObject::Flow => m.create_flow(&attrs).map(Into::into),
            MacsecObject::Sc => m.create_sc(&attrs).map(Into::into),
            MacsecObject::Sa => m.create_sa(&mut r, &attrs).map(Into::into),
            MacsecObject::AclTable => m.create_acl_table(&attrs).map(Into::into),
            MacsecObject::AclEntry => m.create_acl_entry(&mut r, &attrs).map(Into::into),
        };
        *alloc.lock().unwrap_or_else(PoisonError::into_inner) = None;
        made
    }

    fn remove(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, id: RawOid) -> Result<()> {
        let (m, mut r) = self.parts(io);
        match object {
            MacsecObject::Macsec => m.remove_macsec(typed::<MacsecId>(id)?),
            MacsecObject::Port => m.remove_macsec_port(&mut r, typed::<MacsecPortId>(id)?),
            MacsecObject::Flow => m.remove_flow(typed::<FlowId>(id)?),
            MacsecObject::Sc => m.remove_sc(typed::<ScId>(id)?),
            MacsecObject::Sa => m.remove_sa(&mut r, typed::<SaId>(id)?),
            MacsecObject::AclTable => m.remove_acl_table(typed::<AclTableId>(id)?),
            MacsecObject::AclEntry => m.remove_acl_entry(&mut r, typed::<AclEntryId>(id)?),
        }
    }

    fn set(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        id: RawOid,
        attr: &sai_core::value::Attr,
    ) -> Result<()> {
        let a = to_model(attr);
        let (m, mut r) = self.parts(io);
        match object {
            MacsecObject::Macsec => m.set_macsec(typed::<MacsecId>(id)?, &a),
            MacsecObject::Port => m.set_macsec_port(typed::<MacsecPortId>(id)?, &a),
            MacsecObject::Flow => m.set_flow(typed::<FlowId>(id)?, &a),
            MacsecObject::Sc => m.set_sc(typed::<ScId>(id)?, &a),
            MacsecObject::Sa => m.set_sa(typed::<SaId>(id)?, &a),
            MacsecObject::AclEntry => m.set_acl_entry(&mut r, typed::<AclEntryId>(id)?, &a),
            MacsecObject::AclTable => Err(Error::InvalidObjectType),
        }
    }

    fn get(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<sai_core::value::Value>> {
        let values = {
            let (m, mut r) = self.parts(io);
            match object {
                MacsecObject::Macsec => m.get_macsec(&r, typed::<MacsecId>(id)?, ids),
                MacsecObject::Port => m.get_macsec_port(typed::<MacsecPortId>(id)?, ids),
                MacsecObject::Flow => m.get_flow(typed::<FlowId>(id)?, ids),
                MacsecObject::Sc => m.get_sc(typed::<ScId>(id)?, ids),
                MacsecObject::Sa => m.get_sa(&mut r, typed::<SaId>(id)?, ids),
                MacsecObject::AclTable => m.get_acl_table(typed::<AclTableId>(id)?, ids),
                MacsecObject::AclEntry => m.get_acl_entry(typed::<AclEntryId>(id)?, ids),
            }?
        };
        to_core_all(object, ids, values)
    }

    fn get_acl(
        &self,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<sai_core::value::Value>> {
        let values = match object {
            MacsecObject::AclTable => self.model.get_acl_table(typed::<AclTableId>(id)?, ids),
            MacsecObject::AclEntry => self.model.get_acl_entry(typed::<AclEntryId>(id)?, ids),
            _ => Err(Error::InvalidObjectType),
        }?;
        to_core_all(object, ids, values)
    }

    fn stats(
        &mut self,
        io: &mut dyn MacsecIo,
        object: MacsecObject,
        id: RawOid,
        ids: &[sys::sai_stat_id_t],
        mode: StatsMode,
    ) -> Result<Vec<u64>> {
        let mode = match mode {
            StatsMode::Read => sys::SAI_STATS_MODE_READ,
            StatsMode::ReadAndClear => sys::SAI_STATS_MODE_READ_AND_CLEAR,
        };
        // no counters on these: the C++ answers by the count alone (nothing is written)
        let probe = ids.get(..ids.len().min(1)).unwrap_or_default();
        let (m, mut r) = self.parts(io);
        match object {
            MacsecObject::Port => m.get_macsec_port_stats(typed(id)?, probe).map(|_| Vec::new()),
            MacsecObject::Sc => m.get_sc_stats(typed(id)?, probe).map(|_| Vec::new()),
            MacsecObject::Flow => m.get_flow_stats_ext(&mut r, typed(id)?, ids, mode),
            MacsecObject::Sa => m.get_sa_stats_ext(&mut r, typed(id)?, ids, mode),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn clear_stats(&mut self, io: &mut dyn MacsecIo, object: MacsecObject, id: RawOid) -> Result<()> {
        let (m, mut r) = self.parts(io);
        match object {
            MacsecObject::Flow => m.clear_flow_stats(&mut r, typed(id)?),
            MacsecObject::Sa => m.clear_sa_stats(&mut r, typed(id)?),
            MacsecObject::Port => m.clear_macsec_port_stats(typed(id)?),
            MacsecObject::Sc => m.clear_sc_stats(typed(id)?),
            _ => Err(Error::InvalidObjectType),
        }
    }

    fn owns(&self, object: MacsecObject, id: RawOid) -> bool {
        match object {
            MacsecObject::AclTable => self.model.acl_table(id).is_some(),
            MacsecObject::AclEntry => self.model.acl_entry(id).is_some(),
            _ => false,
        }
    }

    fn set_switch_ports(&mut self, ports: &[(PortId, HwPort)]) {
        let pairs: Vec<(PortId, PortKey)> = ports.iter().map(|(id, hw)| (*id, key_of(*hw))).collect();
        self.model.set_switch_ports(&pairs);
    }

    fn set_port_acl(&mut self, io: &mut dyn MacsecIo, port: PortId, side: AclSide, table: RawOid) -> Result<()> {
        let (m, mut r) = self.parts(io);
        m.set_port_acl(&mut r, port, dir_of(side), table)
    }

    fn port_acl(&self, port: PortId, side: AclSide) -> RawOid {
        self.model.port_acl(port, dir_of(side))
    }

    fn port_macsec_ports(&self, port: PortId) -> Vec<RawOid> {
        self.model.port_macsec_ports(port).iter().map(|p| p.raw()).collect()
    }

    fn port_ifg(&self, hw: HwPort) -> u32 {
        self.model.port_ifg(key_of(hw))
    }

    fn engine_ports(&self) -> Vec<HwPort> {
        self.model.engine_ports().into_iter().filter_map(hw_of).collect()
    }

    fn has_macsec_port(&self, hw: HwPort) -> bool {
        self.model.has_macsec_port(key_of(hw))
    }

    fn stats_capability(&self, object: MacsecObject) -> Result<Vec<sys::sai_stat_id_t>> {
        self.model
            .stats_capability(&TableView(self.engine.as_ref()), object.sai())
    }

    fn status(&self) -> String {
        self.model.status(&TableView(self.engine.as_ref()))
    }

    fn trace(&mut self, args: &str) -> String {
        self.trace.command(args)
    }

    fn diag_run(&mut self, io: &mut dyn MacsecIo, port: &str, seq: &str, params: &[(&str, &[u8])]) -> String {
        let mut par = Params::new();
        for (name, bytes) in params {
            par.set(name, bytes); // copied into wiped storage
        }
        let (m, mut r) = self.parts(io);
        let out = m.diag_run(&mut r, port, seq, &par);
        drop(par);
        out
    }

    fn detach(&mut self) {
        self.model.detach();
    }
}

/// Keeps the import of `Secret` meaningful for readers: the core and the model share the type.
#[allow(dead_code, reason = "a compile-time check, never called")]
fn secrets_are_one_type(s: Secret<32>) -> sai_macsec::Secret<32> {
    s
}

/// Unused import guard for `Write` (status helpers may append).
#[allow(dead_code, reason = "a compile-time check, never called")]
fn writes(s: &mut String) {
    let _ = write!(s, "");
}

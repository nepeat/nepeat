//! Shared by the `QoS`, mirror and hash tests (agent A6): a switch on the virtual backend with the
//! C++ fake's port numbering (8 ports of 4 lanes: port `i` is chip port `1 + 4 i`), behind the
//! recording decorator, so a test can check what the chip has and which calls it got.
#![allow(dead_code, missing_docs, unreachable_pub, clippy::unwrap_used, clippy::panic)]
use sai_core::backend::{CallLog, HwPort, Mode, Profile, Recording};
use sai_core::model::Port;
use sai_core::oid::{PortId, RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::VirtualBackend;
use sai_core::{Error, Result};
use sai_sys as sys;

pub const SWITCH: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_SWITCH;
pub const PORT: sys::sai_object_type_t = sys::SAI_OBJECT_TYPE_PORT;

pub struct T {
    pub sai: Sai<Recording<VirtualBackend>>,
    pub switch: RawOid,
    pub log: CallLog,
    /// The front-panel ports, in `PORT_LIST` order.
    pub ports: Vec<RawOid>,
}

pub fn e(v: u32) -> Value {
    Value::Enum(v.cast_signed())
}

pub fn a(id: sys::sai_attr_id_t, value: Value) -> Attr {
    Attr::new(id, value)
}

pub fn oid(v: Value) -> RawOid {
    match v {
        Value::Oid(o) => o,
        other => panic!("not an object id: {other:?}"),
    }
}

pub fn list(v: Value) -> Vec<RawOid> {
    match v {
        Value::OidList(l) => l,
        other => panic!("not an object list: {other:?}"),
    }
}

impl T {
    pub fn new() -> Self {
        let backend = Recording::new(VirtualBackend::new(), Mode::Normal);
        let log = backend.log();
        let mut sai = Sai::new(backend);
        let profile = Profile::from_pairs([
            ("SAI_RS_VIRTUAL_PORTS", "8".to_owned()),
            ("SAI_RS_VIRTUAL_COMPAT", "cpp".to_owned()),
        ]);
        let switch = sai
            .create_switch(&[a(sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))], &profile)
            .unwrap()
            .raw();
        let mut t = Self {
            sai,
            switch,
            log,
            ports: Vec::new(),
        };
        t.ports = list(t.get(SWITCH, switch, sys::SAI_SWITCH_ATTR_PORT_LIST).unwrap());
        t.log.take();
        t
    }

    pub fn virt(&self) -> &VirtualBackend {
        self.sai.backend().inner()
    }

    pub fn virt_mut(&mut self) -> &mut VirtualBackend {
        self.sai.backend_mut().inner_mut()
    }

    pub fn qos(&mut self, on: bool) {
        self.virt_mut().toggles_mut().qos_on = on;
    }

    pub fn mirror(&mut self, on: bool) {
        self.virt_mut().toggles_mut().mirror_on = on;
    }

    pub fn l2x(&mut self, on: bool) {
        self.virt_mut().toggles_mut().l2x_on = on;
    }

    pub fn create(&mut self, t: sys::sai_object_type_t, attrs: &[Attr]) -> Result<RawOid> {
        self.sai.create(t, self.switch, attrs)
    }

    pub fn remove(&mut self, t: sys::sai_object_type_t, id: RawOid) -> Result<()> {
        self.sai.remove(t, id)
    }

    pub fn set(&mut self, t: sys::sai_object_type_t, id: RawOid, attr: sys::sai_attr_id_t, value: Value) -> Result<()> {
        self.sai.set(t, id, &Attr::new(attr, value))
    }

    pub fn get(&self, t: sys::sai_object_type_t, id: RawOid, attr: sys::sai_attr_id_t) -> Result<Value> {
        self.sai.get(t, id, &[attr]).map(|mut v| v.remove(0))
    }

    pub fn port_set(&mut self, port: RawOid, attr: sys::sai_attr_id_t, value: Value) -> Result<()> {
        self.set(PORT, port, attr, value)
    }

    pub fn port_list(&self, port: RawOid, attr: sys::sai_attr_id_t) -> Vec<RawOid> {
        list(self.get(PORT, port, attr).unwrap())
    }

    /// The chip port of a port.
    pub fn hw(&self, port: RawOid) -> HwPort {
        self.sai
            .store()
            .unwrap()
            .get::<Port>(PortId::try_from(port).unwrap())
            .unwrap()
            .hw
    }

    /// The store's own rules hold.
    pub fn store_ok(&self) {
        self.sai.store().unwrap().check_invariants().unwrap();
    }

    /// The calls since the last look.
    pub fn calls(&self) -> Vec<String> {
        self.log.take()
    }
}

/// The attribute error of a set (index 0).
pub fn attr(kind: sai_core::error::AttrError) -> Error {
    Error::attr(kind, 0)
}

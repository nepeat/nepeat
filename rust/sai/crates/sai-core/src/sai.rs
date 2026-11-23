//! The entry points of the core: create, remove, set and get by object type, switch
//! creation (the second stage after API initialisation) and backend events.
//!
//! Shape of every operation: check the call against the metadata ([`crate::attrs`]), decode
//! raw ids into typed ids and resolve them in the store, call the backend, and only then
//! change the store.
use crate::attrs;
use crate::backend::{Backend, BackendEvent, EventSink, HwPort, Profile, StatsObject};
use crate::bag;
use crate::caps::{self, AttrCapability};
use crate::dispatch::{self, Cx};
use crate::error::{Error, Result};
use crate::log;
use crate::macsec::{MacsecObject, StatsMode};
use crate::model::{EntryKey, Port, PortKind, Switch};
use crate::oid::{AnyId, BridgePortId, ObjectType, PortId, RawOid, SwitchId, TypedId, VlanId};
use crate::store::Store;
use crate::value::{Attr, Pointer, Value};
use sai_sys as sys;
use std::sync::mpsc;

/// Something to tell the SAI user. The rim delivers it through the callback the switch
/// was given, outside every lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notification {
    /// A port's operational status changed.
    PortOperStatus {
        /// The port.
        port: PortId,
        /// Up or down.
        up: bool,
    },
    /// The chip learned, moved or aged an address (`SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY`;
    /// [`crate::l2::fdb_event`]).
    Fdb {
        /// The switch (the entry's `switch_id`).
        switch: SwitchId,
        /// What happened.
        kind: crate::l2::FdbEventKind,
        /// The address.
        mac: [u8; 6],
        /// Its VLAN (the entry's `bv_id`).
        vlan: VlanId,
        /// The bridge port it is on (none: aged from an entry without one, seen where none is).
        bridge_port: Option<BridgePortId>,
    },
}

#[derive(Debug)]
struct Running {
    id: SwitchId,
    store: Store,
    events: Option<mpsc::Receiver<BackendEvent>>,
    /// The temperature publisher ([`crate::switch`], [`crate::temp`]).
    temp: Option<crate::temp::Publisher>,
}

/// The SAI core over a backend. Created at API initialisation; holds a switch between
/// [`Sai::create_switch`] and its removal.
#[derive(Debug)]
pub struct Sai<B: Backend> {
    backend: B,
    switch: Option<Running>,
}

impl<B: Backend> Drop for Sai<B> {
    /// A switch still up when the core goes away (API uninitialised without removing it) is
    /// shut down: the chip must not be left running without its owner.
    fn drop(&mut self) {
        if let Some(running) = self.switch.take() {
            crate::switch::remove(&mut self.backend, &running.store, running.temp);
        }
    }
}

impl<B: Backend> Sai<B> {
    /// The core with no switch yet.
    pub fn new(backend: B) -> Self {
        Self { backend, switch: None }
    }

    /// `sai_query_attribute_capability` ([`caps::attribute`]: the C++ table, whatever the
    /// backend).
    pub fn attribute_capability(
        &self,
        object_type: sys::sai_object_type_t,
        attr: sys::sai_attr_id_t,
    ) -> AttrCapability {
        caps::attribute(object_type, attr)
    }

    /// `sai_query_attribute_enum_values_capability` ([`caps::enum_values`]).
    pub fn enum_values_capability(
        &self,
        object_type: sys::sai_object_type_t,
        attr: sys::sai_attr_id_t,
    ) -> Result<Vec<i32>> {
        caps::enum_values(object_type, attr)
    }

    /// The backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// The backend, mutably (tests: fault injection, link changes).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// The object store of the switch, when there is one.
    pub fn store(&self) -> Option<&Store> {
        self.switch.as_ref().map(|r| &r.store)
    }

    /// The switch's id, when there is one.
    pub fn switch_id(&self) -> Option<SwitchId> {
        self.switch.as_ref().map(|r| r.id)
    }

    /// The receiving end of the backend's events, once per switch (for the notifier thread).
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<BackendEvent>> {
        self.switch.as_mut().and_then(|r| r.events.take())
    }

    /// A callback the switch was given (`SAI_SWITCH_ATTR_*_NOTIFY`), null when unset.
    pub fn switch_callback(&self, attr: sys::sai_attr_id_t) -> Pointer {
        let Some(running) = &self.switch else {
            return Pointer::NULL;
        };
        match running.store.get::<Switch>(running.id).map(|s| s.kept.get(&attr)) {
            Ok(Some(Value::Pointer(p))) => *p,
            _ => Pointer::NULL,
        }
    }

    /// Creates the switch: brings the backend up and builds the objects every switch has
    /// (ports, CPU port, default bridge with one bridge port per port, VLAN 1 with the
    /// chip's default membership). Front-panel ports are disabled: SAI's default admin state
    /// is down, and a port must only pass traffic once the user enabled it.
    pub fn create_switch(&mut self, attrs: &[Attr], profile: &Profile) -> Result<SwitchId> {
        if self.switch.is_some() {
            // C++: "only one switch is supported"
            log::error(sys::SAI_API_SWITCH, format_args!("only one switch is supported"));
            return Err(Error::InsufficientResources);
        }
        let (sink, events) = EventSink::channel();
        let created = crate::switch::create(&mut self.backend, attrs, profile, sink)?;
        let id = created.id;
        self.switch = Some(Running {
            id,
            store: created.store,
            events: Some(events),
            temp: created.temp,
        });
        Ok(id)
    }

    /// Creates an object of a type other than the switch (a `MACsec` type: the `MACsec` model's,
    /// which does not look at the switch id, as the C++).
    pub fn create(&mut self, object_type: sys::sai_object_type_t, switch: RawOid, attrs: &[Attr]) -> Result<RawOid> {
        if let Some(object) = MacsecObject::of_macsec_api(object_type) {
            return self.macsec_create(object, attrs);
        }
        let object_type = ObjectType::from_sai(object_type).ok_or(Error::NotImplemented)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        // The C++ does not look at the switch id of a create; this library answers an id that is
        // not its switch as the C++ answers every unknown id.
        if SwitchId::try_from(switch).ok() != Some(running.id) {
            return Err(Error::InvalidObjectId);
        }
        let store = &mut running.store;
        match object_type {
            ObjectType::Switch => Err(Error::InvalidParameter),
            // Ports come from the chip configuration (no breakout yet).
            ObjectType::Port => {
                crate::log::error(
                    sys::SAI_API_PORT,
                    format_args!("dynamic port creation (breakout) is not supported; ports come from the SDK config"),
                );
                Err(Error::NotSupported)
            }
            other => dispatch::create(
                Cx {
                    backend,
                    store,
                    switch: running.id,
                },
                other,
                attrs,
            ),
        }
    }

    /// The backend and the store of the switch, for an operation that changes both.
    pub(crate) fn parts(&mut self) -> Result<Cx<'_, B>> {
        match self {
            Self {
                backend,
                switch: Some(running),
            } => Ok(Cx {
                backend,
                store: &mut running.store,
                switch: running.id,
            }),
            _ => Err(Error::Uninitialized),
        }
    }

    /// Checks the switch id an entry's key carries.
    fn check_switch(&self, switch: RawOid) -> Result<()> {
        let running = self.switch.as_ref().ok_or(Error::Uninitialized)?;
        if SwitchId::try_from(switch).ok() == Some(running.id) {
            Ok(())
        } else {
            Err(Error::InvalidObjectId)
        }
    }

    /// Creates an FDB, neighbor or route entry. `switch` is the switch id of its key.
    pub fn create_entry(&mut self, switch: RawOid, key: EntryKey, attrs: &[Attr]) -> Result<()> {
        self.check_switch(switch)?;
        dispatch::create_entry(self.parts()?, key, attrs)
    }

    /// Removes an entry.
    pub fn remove_entry(&mut self, switch: RawOid, key: &EntryKey) -> Result<()> {
        self.check_switch(switch)?;
        dispatch::remove_entry(self.parts()?, key)
    }

    /// What a set of an entry answers that carries an attribute the metadata does not let a set
    /// carry (the rim refuses those before decoding the value): the C++ `set_fdb_entry_attribute`
    /// / `set_neighbor_entry_attribute` / `set_route_entry_attribute` look the entry up
    /// (`ITEM_NOT_FOUND`), then answer `attr_not_supported(0)` (`sai_l2.cpp:649`, `sai_l3.cpp:636` /
    /// :847).
    pub fn refused_set_entry(&self, switch: RawOid, key: &EntryKey, attr: sys::sai_attr_id_t) -> Error {
        let _ = attr;
        match self.get_entry(switch, key, &[]) {
            Err(e) => e,
            Ok(_) => Error::attr(crate::error::AttrError::NotSupported, 0),
        }
    }

    /// Sets one attribute of an entry.
    pub fn set_entry(&mut self, switch: RawOid, key: &EntryKey, attr: &Attr) -> Result<()> {
        self.check_switch(switch)?;
        let settable = sai_meta::attribute(key.entry_type().sai(), attr.id).is_some_and(|meta| meta.flags.settable());
        if !settable {
            return Err(self.refused_set_entry(switch, key, attr.id));
        }
        dispatch::set_entry(self.parts()?, key, attr)
    }

    /// Gets attributes of an entry, in the order of `ids`.
    pub fn get_entry(&self, switch: RawOid, key: &EntryKey, ids: &[sys::sai_attr_id_t]) -> Result<Vec<Value>> {
        self.check_switch(switch)?;
        let running = self.switch.as_ref().ok_or(Error::Uninitialized)?;
        dispatch::get_entry(&self.backend, &running.store, key, ids)
    }

    /// `flush_fdb_entries`: removes the FDB entries matching the attributes and returns
    /// their keys.
    pub fn flush_fdb(&mut self, switch: RawOid, attrs: &[Attr]) -> Result<Vec<EntryKey>> {
        self.check_switch(switch)?;
        dispatch::flush_fdb(self.parts()?, attrs)
    }

    /// Reads counters of an object, in the order of `counters`; with `clear` they are zeroed
    /// after the read. A counter id the object type does not have is `InvalidParameter`; one
    /// the chip does not have is the backend's answer (`NotSupported`).
    pub fn stats_get(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
        clear: bool,
    ) -> Result<Vec<u64>> {
        if let Some(object) = MacsecObject::of_macsec_api(object_type) {
            // the C++ glue: flow and SA counters in READ mode only; a MACSEC_PORT / SC answers
            // by the count alone, whatever the mode
            if clear && matches!(object, MacsecObject::Flow | MacsecObject::Sa) {
                return Err(Error::NotSupported);
            }
            return self.macsec_stats(object, oid, counters, StatsMode::Read);
        }
        if object_type == sys::SAI_OBJECT_TYPE_PORT {
            let running = self.switch.as_ref().ok_or(Error::Uninitialized)?;
            return crate::port::stats_get(&self.backend, &running.store, oid, counters, clear);
        }
        if object_type == sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE {
            let Self { backend, switch } = self;
            let store = &switch.as_ref().ok_or(Error::Uninitialized)?.store;
            return crate::l3::rif_stats(backend, store, oid, counters, clear);
        }
        if let Self {
            backend,
            switch: Some(running),
        } = self
        {
            if let Some(result) = crate::qos::stats_get(backend, &running.store, object_type, oid, counters, clear) {
                return result;
            }
        }
        let object = self.stats_object(object_type, oid, counters)?;
        self.backend.stats_get(object, counters, clear)
    }

    /// Zeroes counters of an object.
    pub fn stats_clear(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
    ) -> Result<()> {
        match MacsecObject::of_macsec_api(object_type) {
            // a MACSEC_PORT / SC: NOT_SUPPORTED without looking at the id (the C++ glue, sai-macsec
            // Q17); flow and SA: every counter, whatever was asked
            Some(MacsecObject::Port | MacsecObject::Sc) => return Err(Error::NotSupported),
            Some(object) => return self.macsec_clear_stats(object, oid),
            None => {}
        }
        if object_type == sys::SAI_OBJECT_TYPE_PORT {
            // C++ clear_port_stats: no per-counter clear in the SDK API.
            return Err(Error::NotSupported);
        }
        if object_type == sys::SAI_OBJECT_TYPE_ROUTER_INTERFACE {
            // C++ clear_rif_stats: a read-and-clear.
            return self.stats_get(object_type, oid, counters, true).map(|_| ());
        }
        if let Self {
            backend,
            switch: Some(running),
        } = self
        {
            if let Some(result) = crate::qos::stats_clear(backend, &running.store, object_type, oid, counters) {
                return result;
            }
        }
        let object = self.stats_object(object_type, oid, counters)?;
        self.backend.stats_clear(object, counters)
    }

    /// `sai_query_stats_capability` ([`caps::stats_capability`]): the counters of an object type
    /// with their modes; answered with or without a switch, as the C++ answers.
    pub fn stats_capability(&self, object_type: sys::sai_object_type_t) -> Result<Vec<caps::StatCap>> {
        caps::stats_capability(Some(&self.backend), self.store(), object_type)
    }

    /// `clear_port_all_stats` ([`crate::port::clear_all_stats`]).
    pub fn clear_port_all_stats(&mut self, oid: RawOid) -> Result<()> {
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        crate::port::clear_all_stats(backend, &running.store, oid)
    }

    /// The text of `sai_dbg_generate_dump` ([`crate::dump::text`]).
    pub fn dump_text(&self) -> String {
        crate::dump::text(&self.backend, self.store(), self.switch_id())
    }

    /// What a set answers that carries an attribute the metadata does not let a set carry
    /// (create-only, read-only, or no attribute of the type; the rim refuses those before
    /// decoding the value): the object checks first, then what the type's C++ `set_*` function
    /// answers ([`crate::dispatch::Area::refused_set`]). `None`: the metadata's refusal stands.
    pub fn refused_set(
        &self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        attr: sys::sai_attr_id_t,
    ) -> Option<Error> {
        Some(match self.resolve(object_type, oid) {
            Err(e) => e,
            Ok(AnyId::Switch(_)) => crate::switch::refused_set(attr),
            Ok(AnyId::Port(_)) => crate::port::refused_set(),
            Ok(id) => return dispatch::refused_set(id.object_type(), attr),
        })
    }

    /// Sets of the switch and of ports ([`crate::switch::set`], [`crate::port::set`]): an
    /// attribute the metadata does not let a set carry gets the C++ answer.
    fn set_switch_or_port(&mut self, id: AnyId, attr: &Attr) -> Result<()> {
        let settable = sai_meta::attribute(id.object_type().sai(), attr.id)
            .is_some_and(|meta| meta.flags.settable() && meta.value_type == attr.value.value_type());
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        match id {
            AnyId::Switch(switch) if settable => crate::switch::set(backend, &mut running.store, switch, attr),
            AnyId::Switch(_) => Err(crate::switch::refused_set(attr.id)),
            AnyId::Port(port) if settable => {
                // An object id must name a live object, whoever keeps the attribute.
                bag::check_references(&running.store, core::slice::from_ref(attr))?;
                crate::port::set(backend, &mut running.store, running.id, port, attr)
            }
            _ => Err(crate::port::refused_set()),
        }
    }

    /// Resolves the object of a counter call into the chip's terms.
    fn stats_object(
        &self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        counters: &[sys::sai_stat_id_t],
    ) -> Result<StatsObject> {
        let id = self.resolve(object_type, oid)?;
        let known = sai_meta::stat_ids(object_type);
        if known.is_empty() {
            return Err(Error::NotSupported);
        }
        if counters.iter().any(|c| !known.contains(&c.cast_signed())) {
            return Err(Error::InvalidParameter);
        }
        let store = self.store().ok_or(Error::Uninitialized)?;
        let of_port = |bag_id: RawOid, port_attr, index_attr| -> Result<(HwPort, u8)> {
            let bag = store.bag(bag_id)?;
            match (bag.attrs.get(&port_attr), bag.attrs.get(&index_attr)) {
                (Some(Value::Oid(port)), Some(Value::U8(index))) => {
                    Ok((store.get::<Port>(PortId::try_from(*port)?)?.hw, *index))
                }
                _ => Err(Error::Failure),
            }
        };
        Ok(match id {
            // The CPU port has no counters.
            AnyId::Port(port) => StatsObject::Port(front_port(store.get::<Port>(port)?).ok_or(Error::NotSupported)?),
            AnyId::Queue(queue) => {
                let (port, index) = of_port(queue.raw(), sys::SAI_QUEUE_ATTR_PORT, sys::SAI_QUEUE_ATTR_INDEX)?;
                StatsObject::Queue { port, index }
            }
            AnyId::IngressPriorityGroup(group) => {
                let (port, index) = of_port(
                    group.raw(),
                    sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_PORT,
                    sys::SAI_INGRESS_PRIORITY_GROUP_ATTR_INDEX,
                )?;
                StatsObject::PriorityGroup { port, index }
            }
            other => StatsObject::Other {
                object_type,
                id: other.raw(),
            },
        })
    }

    /// Removes an object. Removing the switch shuts the backend down.
    pub fn remove(&mut self, object_type: sys::sai_object_type_t, oid: RawOid) -> Result<()> {
        if let Some(object) = MacsecObject::of_macsec_api(object_type) {
            return self.macsec_remove(object, oid);
        }
        // The C++ answers these without looking the id up.
        if object_type == sys::SAI_OBJECT_TYPE_HASH {
            return Err(Error::NotSupported); // sai_hash.cpp:74 remove_hash
        }
        let id = self.resolve(object_type, oid)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        let store = &mut running.store;
        match id {
            AnyId::Switch(_) => {
                if let Some(running) = self.switch.take() {
                    crate::switch::remove(backend, &running.store, running.temp);
                }
                Ok(())
            }
            AnyId::Port(_) => {
                crate::log::error(sys::SAI_API_PORT, format_args!("port removal is not supported"));
                Err(Error::NotSupported)
            }
            other => dispatch::remove(
                Cx {
                    backend,
                    store,
                    switch: running.id,
                },
                other.object_type(),
                other.raw(),
            ),
        }
    }

    /// Sets one attribute.
    pub fn set(&mut self, object_type: sys::sai_object_type_t, oid: RawOid, attr: &Attr) -> Result<()> {
        if let Some(object) = MacsecObject::of_macsec_api(object_type) {
            return self.macsec_set(object, oid, attr);
        }
        let id = self.resolve(object_type, oid)?;
        if matches!(id, AnyId::Switch(_) | AnyId::Port(_)) {
            return self.set_switch_or_port(id, attr);
        }
        let settable = sai_meta::attribute(object_type, attr.id).is_some_and(|meta| meta.flags.settable());
        if !settable {
            let refused = attrs::check_set(object_type, attr).err().unwrap_or(Error::Failure);
            return Err(self.refused_set(object_type, oid, attr.id).unwrap_or(refused));
        }
        attrs::check_set(id.object_type().sai(), attr)?;
        let Self {
            backend,
            switch: Some(running),
        } = self
        else {
            return Err(Error::Uninitialized);
        };
        let store = &mut running.store;
        // An object id must name a live object, whoever keeps the attribute.
        bag::check_references(store, core::slice::from_ref(attr))?;
        // The metadata check passed, so the value has its attribute's kind.
        dispatch::set(
            Cx {
                backend,
                store,
                switch: running.id,
            },
            id.object_type(),
            id.raw(),
            attr,
        )
    }

    /// Gets attributes. The values come back in the order of `ids`; an error names the
    /// position of the attribute that failed.
    pub fn get(
        &self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        let id = self.resolve(object_type, oid)?;
        let Some(running) = &self.switch else {
            return Err(Error::Uninitialized);
        };
        let values: Vec<Value> = match id {
            // The C++ answers an attribute it does not know ATTR_NOT_SUPPORTED.
            AnyId::Switch(switch) => ids
                .iter()
                .enumerate()
                .map(|(index, attr)| {
                    crate::switch::get(&self.backend, &running.store, switch, *attr).map_err(|e| e.at_index(index))
                })
                .collect::<Result<_>>()?,
            AnyId::Port(port) => ids
                .iter()
                .enumerate()
                .map(|(index, attr)| {
                    crate::port::get(&self.backend, &running.store, port, *attr).map_err(|e| e.at_index(index))
                })
                .collect::<Result<_>>()?,
            other => dispatch::get_all(&self.backend, &running.store, other.object_type(), other.raw(), ids)?,
        };
        // An answer of the wrong kind would be encoded into the wrong union member.
        for (attr, value) in ids.iter().zip(&values) {
            match sai_meta::attribute(object_type, *attr) {
                Some(meta) if meta.value_type == value.value_type() => {}
                _ => return Err(Error::Failure),
            }
        }
        Ok(values)
    }

    /// Gets attributes of an object of any type, the `MACsec` model's included (it may run a PHY
    /// sequence, hence `&mut`): [`Sai::get`] or [`Sai::macsec_get`].
    pub fn get_any(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        ids: &[sys::sai_attr_id_t],
    ) -> Result<Vec<Value>> {
        match MacsecObject::of_macsec_api(object_type) {
            Some(object) => self.macsec_get(object, oid, ids),
            None => self.get(object_type, oid, ids),
        }
    }

    /// The object lookup of a call whose pointers the rim found unusable: the C++ looks the
    /// object up first (`INVALID_OBJECT_ID`), then answers `pointer` (`sai_l3.cpp:158`, ...).
    /// A `MACsec` flow's set never looks at its attribute (`sai_macsec.cpp:640`): the model answers.
    pub fn bad_pointer(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        set: bool,
        pointer: Error,
    ) -> Error {
        let looked_up = match MacsecObject::of_macsec_api(object_type) {
            Some(MacsecObject::Flow) if set => {
                return self
                    .macsec_set(MacsecObject::Flow, oid, &Attr::new(0, Value::U32(0)))
                    .err()
                    .unwrap_or(pointer);
            }
            Some(object) => self.macsec_get(object, oid, &[]).map(drop),
            None => self.resolve(object_type, oid).map(drop),
        };
        looked_up.err().unwrap_or(pointer)
    }

    /// The `MACsec` counters of a call whose pointers the rim found unusable: a `MACSEC_PORT` / `SC`
    /// answers by the count alone (the C++ glue), so the model is asked for no counter; other
    /// types: [`Sai::bad_pointer`].
    pub fn bad_stats_pointer(
        &mut self,
        object_type: sys::sai_object_type_t,
        oid: RawOid,
        pointer: Error,
    ) -> Result<()> {
        match MacsecObject::of_macsec_api(object_type) {
            Some(object @ (MacsecObject::Port | MacsecObject::Sc)) => {
                self.macsec_stats(object, oid, &[], StatsMode::Read).map(drop)
            }
            _ => Err(self.bad_pointer(object_type, oid, false, pointer)),
        }
    }

    /// Applies an event the backend raised and returns what the user is to be told.
    pub fn handle_event(&mut self, event: BackendEvent) -> Vec<Notification> {
        let Some(running) = &mut self.switch else {
            return Vec::new();
        };
        match event {
            BackendEvent::Fdb(event) => crate::l2::fdb_event(&mut running.store, event),
            BackendEvent::Link { hw, up } => {
                let port = PortId::from_index(u64::from(hw.0));
                match running
                    .store
                    .update::<Port, _>(port, |p| (core::mem::replace(&mut p.oper_up, up), p.name.clone()))
                {
                    Ok((was, name)) if was != up => {
                        crate::log::notice(
                            sys::SAI_API_PORT,
                            format_args!("port {name} (hw {}) oper {}", hw.0, if up { "up" } else { "down" }),
                        );
                        vec![Notification::PortOperStatus { port, up }]
                    }
                    _ => Vec::new(),
                }
            }
        }
    }

    /// Decodes a raw id for an operation on `object_type`: the id's type must be that type
    /// and the object must exist.
    fn resolve(&self, object_type: sys::sai_object_type_t, oid: RawOid) -> Result<AnyId> {
        let expected = ObjectType::from_sai(object_type).ok_or(Error::NotImplemented)?;
        let Some(running) = &self.switch else {
            return Err(Error::Uninitialized);
        };
        // The C++ looks an id up in the table of its type: an id of another type (or of no type)
        // is no such object (INVALID_OBJECT_ID), for every type.
        let id = AnyId::decode(oid).map_err(|_| Error::InvalidObjectId)?;
        if id.object_type() != expected {
            return Err(Error::InvalidObjectId);
        }
        if running.store.contains(oid) {
            Ok(id)
        } else {
            Err(Error::InvalidObjectId)
        }
    }
}

/// The chip port of a front-panel port; `None` for the CPU port.
fn front_port(port: &Port) -> Option<HwPort> {
    matches!(port.kind, PortKind::Front { .. }).then_some(port.hw)
}

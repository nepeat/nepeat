//! The one shape of every object entry point: check pointers, decode by the metadata, call
//! the safe core under the lock, encode. The exported trampolines ([`crate::tables`]) only
//! name the object type.
use crate::Status;
use crate::convert::EntryStruct;
use crate::log;
use crate::{convert, notify, ptr, state};
use sai_core::error::{AttrError, Error};
use sai_core::macsec::MacsecObject;
use sai_core::model::EntryKey;
use sai_core::oid::{ObjectType, RawOid, TypedId};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_meta::ValueType;
use sai_sys as sys;

use crate::backends;
use sai_core::backend::{Backend, Profile};
use sai_core::keys::{self, BackendChoice, ChoiceError};

/// An object type of the core or of the `MACsec` model (its five API types); anything else is
/// `NOT_IMPLEMENTED`.
fn implemented(object_type: sys::sai_object_type_t) -> Result<(), Status> {
    if ObjectType::from_sai(object_type).is_some() || MacsecObject::of_macsec_api(object_type).is_some() {
        Ok(())
    } else {
        Err(sys::SAI_STATUS_NOT_IMPLEMENTED)
    }
}

/// Whether the metadata lets a set carry the attribute.
fn settable(object_type: sys::sai_object_type_t, attr: sys::sai_attr_id_t) -> bool {
    sai_meta::attribute(object_type, attr).is_some_and(|meta| meta.flags.settable())
}

/// Which kind of call an attribute arrived in.
#[derive(Clone, Copy)]
enum Call {
    Create,
    Set,
}

/// Decodes the attribute list of a create call.
///
/// # Safety
/// As the SAI headers require of the caller of a create function: `list` points to `count`
/// attributes (or `count` is zero), each with its id set and with the value member that SAI
/// defines for that id initialised, lists included; nothing writes to them during the call.
unsafe fn decode_list(
    object_type: sys::sai_object_type_t,
    list: *const sys::sai_attribute_t,
    count: u32,
) -> Result<Vec<Attr>, Status> {
    // SAFETY: this function's contract.
    let raw = unsafe { ptr::in_slice(list, count) }?;
    raw.iter()
        .enumerate()
        .map(|(index, attr)| decode_one(object_type, attr, Call::Create).map_err(|e| e.at_index(index).status()))
        .collect()
}

/// Decodes one attribute of a create or set call. An attribute the call may not carry
/// (read-only, create-only in a set) is refused before its value is looked at: a caller that
/// passes one has no reason to have filled the value in, and a list value is a pointer.
fn decode_one(object_type: sys::sai_object_type_t, attr: &sys::sai_attribute_t, call: Call) -> Result<Attr, Error> {
    if MacsecObject::of_macsec_api(object_type).is_some() {
        return decode_macsec(object_type, attr, call);
    }
    let meta = sai_meta::attribute(object_type, attr.id);
    let allowed = match call {
        Call::Create => meta.is_some_and(|m| m.flags.creatable()),
        Call::Set => meta.is_some_and(|m| m.flags.settable()),
    };
    let Some(meta) = meta.filter(|_| allowed) else {
        return Err(match (call, meta) {
            // the type's C++ create answer (the set's is asked by the caller, after the lookup)
            (Call::Create, _) => sai_core::dispatch::refused_create(object_type, attr.id, 0),
            (Call::Set, None) => Error::attr(AttrError::Unknown, 0),
            (Call::Set, Some(_)) => Error::attr(AttrError::Invalid, 0),
        });
    };
    // SAFETY: `attr` comes from a slice or reference made by `ptr` under the SAI caller's
    // contract (see `decode_list`, `set`): the member SAI defines for this attribute id is
    // initialised, and `meta.value_type` is that member, taken from the SAI headers.
    let value = unsafe { convert::decode(meta.value_type, &attr.value) }?;
    Ok(Attr::new(attr.id, value))
}

/// One attribute of a `MACsec` create or set: every check and status is the `MACsec` model's, in
/// the C++'s order (`sai_macsec.cpp`), so no metadata check refuses anything here. An attribute
/// the call may carry is decoded by its member (the key material into a `Secret`, by
/// [`convert::decode`]); any other (read-only, unknown, a member the model has no use for) is a
/// placeholder the model refuses by its id, its value never looked at.
fn decode_macsec(object_type: sys::sai_object_type_t, attr: &sys::sai_attribute_t, call: Call) -> Result<Attr, Error> {
    let member = sai_meta::attribute(object_type, attr.id)
        .filter(|meta| match call {
            Call::Create => meta.flags.creatable(),
            Call::Set => meta.flags.settable(),
        })
        .map(|meta| meta.value_type)
        .filter(|vt| {
            matches!(
                vt,
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
    let value = match member {
        // SAFETY: `attr` comes from a slice or reference made by `ptr` under the SAI caller's
        // contract (`decode_list`, `set`): the member SAI defines for this attribute id is
        // initialised, and `vt` is that member, taken from the SAI headers.
        Some(vt) => unsafe { convert::decode(vt, &attr.value) }?,
        None => Value::U32(0),
    };
    Ok(Attr::new(attr.id, value))
}

/// `create_switch`: the second stage of bring-up. The C++ order of checks: the API is
/// initialised; the id pointer, and the list when there is a count; no switch exists yet; the
/// attributes; the profile's board keys; the backend the profile names; its bring-up.
///
/// # Safety
/// As `sai_create_switch_fn` in the SAI headers: `switch_id` is valid for a write of an object
/// id; `attr_list` / `attr_count` as for [`decode_list`]. Callback attributes hold functions of
/// the type SAI defines for them, callable until the switch is removed.
pub(crate) unsafe fn create_switch(
    switch_id: *mut sys::sai_object_id_t,
    attr_count: u32,
    attr_list: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    let services = state::with_api(|api| Ok(api.services))?;
    // Every pointer is checked before anything is created: a switch that exists but whose id
    // could not be handed back would be lost to the caller.
    // SAFETY: this function's contract.
    let out = unsafe { ptr::out(switch_id) }?;
    if attr_count != 0 && attr_list.is_null() {
        return Err(sys::SAI_STATUS_INVALID_PARAMETER);
    }
    state::with_api(|api| only_switch(&api.sai))?;
    // SAFETY: this function's contract.
    let attrs = unsafe { decode_list(sys::SAI_OBJECT_TYPE_SWITCH, attr_list, attr_count) }?;
    let profile_id = attrs
        .iter()
        .find_map(|a| match a {
            Attr {
                id: sys::SAI_SWITCH_ATTR_SWITCH_PROFILE_ID,
                value: Value::U32(id),
            } => Some(*id),
            _ => None,
        })
        .unwrap_or(0);

    // The profile is read through the SAI user's callbacks: outside the lock.
    let profile = services.profile(profile_id, &backends::profile_keys());
    // A bad attribute or board key refuses the switch before a backend is chosen (C++ order).
    sai_core::switch::precheck(&attrs, &profile).map_err(Error::status)?;
    let backend = choose_backend(&profile)?;
    // "initializing backend", "init failed" and "switch created" are the core's lines
    // (sai_core::switch::create), as the C++ create_switch logs them.

    let id = state::with_api(|api| {
        only_switch(&api.sai)?;
        // No switch: the core holds a backend that was never initialised, or one that was
        // shut down with its switch. The chosen one takes its place.
        api.sai = Sai::new(backend);
        let id = api.sai.create_switch(&attrs, &profile).map_err(Error::status)?;
        api.generation += 1;
        let started = api
            .sai
            .take_events()
            .ok_or(sys::SAI_STATUS_FAILURE)
            .and_then(|events| notify::spawn(events, api.generation));
        match started {
            Ok(handle) => {
                api.notifier = Some(handle);
                if api.diag.is_none() {
                    api.diag = crate::diag::start(&profile);
                }
                Ok(id)
            }
            Err(status) => {
                // No thread, no notifications: not a switch to hand out.
                let _ = api.sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, id.raw());
                Err(status)
            }
        }
    })?;
    out.write(id.raw().0);
    Ok(())
}

/// `SAI_STATUS_INSUFFICIENT_RESOURCES` while a switch exists (the C++ answer: one switch at most).
fn only_switch(sai: &Sai<Box<dyn Backend>>) -> Result<(), Status> {
    if sai.switch_id().is_some() {
        log::emit(
            sys::SAI_API_SWITCH,
            sys::SAI_LOG_LEVEL_ERROR,
            format_args!("only one switch is supported"),
        );
        return Err(sys::SAI_STATUS_INSUFFICIENT_RESOURCES); // C++: one switch
    }
    Ok(())
}

/// The backend the profile names (the C++ `pick_backend`, with its log lines). The in-memory
/// chip is never chosen implicitly: on a real switch a library that "succeeds" without
/// programming anything must be something the operator asked for.
fn choose_backend(profile: &Profile) -> Result<Box<dyn Backend>, Status> {
    let choice = keys::backend_choice(profile, cfg!(feature = "openbcm")).map_err(|e| {
        match &e {
            ChoiceError::NoneAsked => log::emit(
                sys::SAI_API_SWITCH,
                sys::SAI_LOG_LEVEL_ERROR,
                format_args!(
                    "this libsai has no openbcm backend (fake-only build); set SAI_OPENBCM_BACKEND=fake in \
                     the profile to use the in-memory model"
                ),
            ),
            ChoiceError::Unknown(name) => log::emit(
                sys::SAI_API_SWITCH,
                sys::SAI_LOG_LEVEL_ERROR,
                format_args!("unknown SAI_OPENBCM_BACKEND '{name}' (fake|openbcm)"),
            ),
        }
        e.error().status()
    })?;
    let backend = backends::select(choice).ok_or_else(|| {
        log::emit(
            sys::SAI_API_SWITCH,
            sys::SAI_LOG_LEVEL_ERROR,
            format_args!("SAI_OPENBCM_BACKEND=openbcm but this libsai was built without the SDK"),
        );
        sys::SAI_STATUS_FAILURE
    })?;
    if choice == BackendChoice::Virtual {
        log::emit(
            sys::SAI_API_SWITCH,
            sys::SAI_LOG_LEVEL_WARN,
            format_args!("FAKE backend selected: nothing is programmed into any switch ASIC"),
        );
    }
    Ok(backend)
}

/// Create for every object type but the switch.
///
/// # Safety
/// As the `sai_create_*_fn` types in the SAI headers: `oid` is valid for a write of an object
/// id; `attr_list` / `attr_count` as for [`decode_list`].
pub(crate) unsafe fn create(
    object_type: sys::sai_object_type_t,
    oid: *mut sys::sai_object_id_t,
    switch_id: sys::sai_object_id_t,
    attr_count: u32,
    attr_list: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let out = unsafe { ptr::out(oid) }?;
    implemented(object_type)?;
    // SAFETY: this function's contract.
    let attrs = unsafe { decode_list(object_type, attr_list, attr_count) }?;
    let id = state::with_api(|api| {
        api.sai
            .create(object_type, RawOid(switch_id), &attrs)
            .map_err(Error::status)
    })?;
    out.write(id.0);
    Ok(())
}

/// Remove, for every object type. Removing the switch also ends its notifier thread.
pub(crate) fn remove(object_type: sys::sai_object_type_t, oid: sys::sai_object_id_t) -> Result<(), Status> {
    implemented(object_type)?;
    let (notifier, diag) = state::with_api(|api| {
        api.sai.remove(object_type, RawOid(oid)).map_err(Error::status)?;
        Ok(if object_type == sys::SAI_OBJECT_TYPE_SWITCH {
            (api.notifier.take(), api.diag.take())
        } else {
            (None, None)
        })
    })?;
    drop(diag); // stops the diag server outside the lock: its thread may be waiting for it
    state::join(notifier); // outside the lock: the thread may be waiting for it
    Ok(())
}

/// Set, for every object type.
///
/// # Safety
/// As the `sai_set_*_attribute_fn` types in the SAI headers: `attr` points to one attribute
/// with its id set and the value member SAI defines for that id initialised (lists
/// included), not written to during the call.
pub(crate) unsafe fn set(
    object_type: sys::sai_object_type_t,
    oid: sys::sai_object_id_t,
    attr: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    implemented(object_type)?;
    // SAFETY: this function's contract.
    let attr = match unsafe { ptr::in_ref(attr) } {
        Ok(attr) => attr,
        Err(status) => return Err(bad_pointer(object_type, oid, true, status)),
    };
    let attr = match decode_one(object_type, attr, Call::Set) {
        Ok(attr) => attr,
        // An attribute a set may not carry: each type answers it as its C++ set function does
        // (after the object lookup); where the C++ defines nothing, the metadata's refusal.
        Err(e) if !settable(object_type, attr.id) => {
            let refused = state::with_api(|api| Ok(api.sai.refused_set(object_type, RawOid(oid), attr.id)))?;
            return Err(refused.unwrap_or(e).status());
        }
        Err(e) => return Err(e.status()),
    };
    state::with_api(|api| api.sai.set(object_type, RawOid(oid), &attr).map_err(Error::status))
}

/// Get, for every object type. Every attribute is encoded even after a list did not fit, so
/// one retry with the returned counts is enough; the status is then
/// `SAI_STATUS_BUFFER_OVERFLOW`.
///
/// # Safety
/// As the `sai_get_*_attribute_fn` types in the SAI headers: `attr_list` points to
/// `attr_count` attributes (or the count is zero), valid for reads and writes and not accessed
/// otherwise during the call, each with its id set; for an attribute whose value is a list,
/// the list's count is the capacity of the buffer its pointer names (valid for that many
/// writes), or zero.
pub(crate) unsafe fn get(
    object_type: sys::sai_object_type_t,
    oid: sys::sai_object_id_t,
    attr_count: u32,
    attr_list: *mut sys::sai_attribute_t,
) -> Result<(), Status> {
    implemented(object_type)?;
    // SAFETY: this function's contract. (A union tolerates the uninitialised bytes a C caller
    // leaves in value members it did not set.)
    let attrs = match unsafe { ptr::inout_slice(attr_list, attr_count) } {
        Ok(attrs) => attrs,
        Err(status) => return Err(bad_pointer(object_type, oid, false, status)),
    };
    let ids: Vec<sys::sai_attr_id_t> = attrs.iter().map(|a| a.id).collect();
    let values = state::with_api(|api| api.sai.get_any(object_type, RawOid(oid), &ids).map_err(Error::status))?;
    // SAFETY: the core returns values of the kind SAI defines for each requested id (it
    // checks), so a list result goes to the list member this function's contract covers.
    unsafe { encode_all(attrs, &values) }
}

/// The status of a set or get whose attribute pointer is unusable: the C++ looks the object up
/// first (`Sai::bad_pointer`), but for ACL tables and entries, which check the pointer first
/// (`sai_acl.cpp:648`, :1125).
fn bad_pointer(object_type: sys::sai_object_type_t, oid: sys::sai_object_id_t, set: bool, pointer: Status) -> Status {
    if matches!(
        object_type,
        sys::SAI_OBJECT_TYPE_ACL_TABLE | sys::SAI_OBJECT_TYPE_ACL_ENTRY
    ) {
        return pointer;
    }
    state::with_api(|api| {
        Ok(api
            .sai
            .bad_pointer(object_type, RawOid(oid), set, status_error(pointer))
            .status())
    })
    .unwrap_or_else(|uninitialized| uninitialized)
}

/// A pointer status as the core's error (the helpers in [`ptr`] answer one of these).
fn status_error(status: Status) -> Error {
    match status {
        sys::SAI_STATUS_BUFFER_OVERFLOW => Error::BufferOverflow,
        sys::SAI_STATUS_INVALID_PARAMETER => Error::InvalidParameter,
        _ => Error::Failure,
    }
}

/// Encodes the results of a get into the caller's attributes. Every attribute is encoded
/// even after a list did not fit.
///
/// # Safety
/// For each value that is a list, the caller of the SAI function initialised the list member
/// of the attribute at the same position that carries it: [`convert::encode`]'s contract.
unsafe fn encode_all(attrs: &mut [sys::sai_attribute_t], values: &[Value]) -> Result<(), Status> {
    let mut overflow = false;
    for (slot, value) in attrs.iter_mut().zip(values) {
        // SAFETY: this function's contract.
        match unsafe { convert::encode(value, &mut slot.value) } {
            Ok(()) => {}
            Err(sys::SAI_STATUS_BUFFER_OVERFLOW) => overflow = true,
            Err(status) => return Err(status),
        }
    }
    if overflow {
        Err(sys::SAI_STATUS_BUFFER_OVERFLOW)
    } else {
        Ok(())
    }
}

/// Reads counters, for every object type; `mode` is `SAI_STATS_MODE_READ` or
/// `SAI_STATS_MODE_READ_AND_CLEAR`. The pointers are checked before anything is read or
/// cleared.
///
/// # Safety
/// As the `sai_get_*_stats_fn` / `sai_get_*_stats_ext_fn` types in the SAI headers:
/// `counter_ids` points to `number_of_counters` ids, not written to during the call, and
/// `counters` is valid for that many writes and not accessed otherwise (or the count is zero).
pub(crate) unsafe fn get_stats(
    object_type: sys::sai_object_type_t,
    oid: sys::sai_object_id_t,
    number_of_counters: u32,
    counter_ids: *const sys::sai_stat_id_t,
    mode: sys::sai_stats_mode_t,
    counters: *mut u64,
) -> Result<(), Status> {
    implemented(object_type)?;
    let macsec = MacsecObject::of_macsec_api(object_type);
    let clear = match mode {
        sys::SAI_STATS_MODE_READ => false,
        // the C++ MACsec glue: a MACSEC_PORT / SC answers by the count alone, whatever the mode;
        // flow and SA counters in READ mode only (the core answers NOT_SUPPORTED)
        _ if matches!(macsec, Some(MacsecObject::Port | MacsecObject::Sc)) => false,
        _ if macsec.is_some() => true,
        // C++ get_port_stats_ext: any mode but READ, before the object is looked at
        _ if object_type == sys::SAI_OBJECT_TYPE_PORT => return Err(sys::SAI_STATUS_NOT_SUPPORTED),
        sys::SAI_STATS_MODE_READ_AND_CLEAR => true,
        _ => return Err(sys::SAI_STATUS_INVALID_PARAMETER),
    };
    // SAFETY: this function's contract.
    let ids = unsafe { ptr::in_slice(counter_ids, number_of_counters) };
    // SAFETY: this function's contract.
    let out = unsafe { ptr::out_slice(counters, number_of_counters) };
    let (ids, out) = match (ids, out) {
        (Ok(ids), Ok(out)) => (ids, out),
        (Err(status), _) | (_, Err(status)) if macsec.is_some() => {
            return state::with_api(|api| {
                api.sai
                    .bad_stats_pointer(object_type, RawOid(oid), status_error(status))
                    .map_err(Error::status)
            });
        }
        (Err(status), _) | (_, Err(status)) => return Err(status),
    };
    let values = state::with_api(|api| {
        api.sai
            .stats_get(object_type, RawOid(oid), ids, clear)
            .map_err(Error::status)
    })?;
    out.write(&values)
}

/// Zeroes counters, for every object type.
///
/// # Safety
/// As the `sai_clear_*_stats_fn` types in the SAI headers: `counter_ids` points to
/// `number_of_counters` ids, not written to during the call (or the count is zero).
pub(crate) unsafe fn clear_stats(
    object_type: sys::sai_object_type_t,
    oid: sys::sai_object_id_t,
    number_of_counters: u32,
    counter_ids: *const sys::sai_stat_id_t,
) -> Result<(), Status> {
    implemented(object_type)?;
    // The MACsec glue clears every counter of a flow or SA, whatever was asked, and never reads
    // the ids (the core answers).
    let ids = if MacsecObject::of_macsec_api(object_type).is_some() {
        &[]
    } else {
        // SAFETY: this function's contract.
        unsafe { ptr::in_slice(counter_ids, number_of_counters) }?
    };
    state::with_api(|api| {
        api.sai
            .stats_clear(object_type, RawOid(oid), ids)
            .map_err(Error::status)
    })
}

/// The switch id and the key out of an entry struct.
///
/// # Safety
/// `entry` is null or points to an entry struct the C caller filled in (for an IP address in
/// it: the member its family names), not written to during the call.
unsafe fn entry_key<E: EntryStruct>(entry: *const E) -> Result<(RawOid, EntryKey), Status> {
    // SAFETY: this function's contract.
    let entry = unsafe { ptr::in_ref(entry) }?;
    // SAFETY: this function's contract is `EntryStruct::key`'s.
    unsafe { entry.key() }.map_err(Error::status)
}

/// Create, for the object types whose key is a struct.
///
/// # Safety
/// As the `sai_create_*_entry_fn` types in the SAI headers: `entry` as for [`entry_key`];
/// `attr_list` / `attr_count` as for [`decode_list`].
pub(crate) unsafe fn entry_create<E: EntryStruct>(
    entry: *const E,
    attr_count: u32,
    attr_list: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let (switch, key) = unsafe { entry_key(entry) }?;
    // SAFETY: this function's contract.
    let attrs = unsafe { decode_list(E::OBJECT_TYPE, attr_list, attr_count) }?;
    state::with_api(|api| api.sai.create_entry(switch, key, &attrs).map_err(Error::status))
}

/// Remove, for the object types whose key is a struct.
///
/// # Safety
/// `entry` as for [`entry_key`].
pub(crate) unsafe fn entry_remove<E: EntryStruct>(entry: *const E) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let (switch, key) = unsafe { entry_key(entry) }?;
    state::with_api(|api| api.sai.remove_entry(switch, &key).map_err(Error::status))
}

/// Set, for the object types whose key is a struct.
///
/// # Safety
/// `entry` as for [`entry_key`]; `attr` as for [`set`].
pub(crate) unsafe fn entry_set<E: EntryStruct>(
    entry: *const E,
    attr: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let (switch, key) = unsafe { entry_key(entry) }?;
    // SAFETY: this function's contract.
    let attr = unsafe { ptr::in_ref(attr) }?;
    let attr = match decode_one(E::OBJECT_TYPE, attr, Call::Set) {
        Ok(attr) => attr,
        // an attribute a set may not carry: the C++ answer, after its entry lookup
        Err(_) if !settable(E::OBJECT_TYPE, attr.id) => {
            return Err(state::with_api(|api| Ok(api.sai.refused_set_entry(switch, &key, attr.id)))?.status());
        }
        Err(e) => return Err(e.status()),
    };
    state::with_api(|api| api.sai.set_entry(switch, &key, &attr).map_err(Error::status))
}

/// Get, for the object types whose key is a struct.
///
/// # Safety
/// `entry` as for [`entry_key`]; `attr_list` / `attr_count` as for [`get`].
pub(crate) unsafe fn entry_get<E: EntryStruct>(
    entry: *const E,
    attr_count: u32,
    attr_list: *mut sys::sai_attribute_t,
) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let (switch, key) = unsafe { entry_key(entry) }?;
    // SAFETY: this function's contract.
    let attrs = unsafe { ptr::inout_slice(attr_list, attr_count) }?;
    let ids: Vec<sys::sai_attr_id_t> = attrs.iter().map(|a| a.id).collect();
    let values = state::with_api(|api| api.sai.get_entry(switch, &key, &ids).map_err(Error::status))?;
    // SAFETY: the core returns values of the kind SAI defines for each requested id, so a
    // list result goes to the list member this function's contract covers.
    unsafe { encode_all(attrs, &values) }
}

/// `flush_fdb_entries`.
///
/// # Safety
/// As `sai_flush_fdb_entries_fn` in the SAI headers: `attr_list` / `attr_count` as for
/// [`decode_list`].
pub(crate) unsafe fn flush_fdb(
    switch_id: sys::sai_object_id_t,
    attr_count: u32,
    attr_list: *const sys::sai_attribute_t,
) -> Result<(), Status> {
    // SAFETY: this function's contract.
    let attrs = unsafe { decode_list(sys::SAI_OBJECT_TYPE_FDB_FLUSH, attr_list, attr_count) }?;
    state::with_api(|api| {
        api.sai
            .flush_fdb(RawOid(switch_id), &attrs)
            .map(|_| ())
            .map_err(Error::status)
    })
}

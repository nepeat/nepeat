//! The one shape of every object entry point: check pointers, decode by the metadata, call
//! the safe core under the lock, encode. The exported trampolines ([`crate::tables`]) only
//! name the object type.
use crate::Status;
use crate::log;
use crate::{convert, notify, ptr, state};
use sai_core::error::{AttrError, Error};
use sai_core::oid::{ObjectType, RawOid, TypedId};
use sai_core::value::{Attr, Value};
use sai_sys as sys;

/// The profile keys the library reads, besides the backend's own.
const BACKEND_KEY: &str = "SAI_RS_BACKEND";

fn implemented(object_type: sys::sai_object_type_t) -> Result<(), Status> {
    ObjectType::from_sai(object_type)
        .map(|_| ())
        .ok_or(sys::SAI_STATUS_NOT_IMPLEMENTED)
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
    let meta = sai_meta::attribute(object_type, attr.id).ok_or(Error::attr(AttrError::Unknown, 0))?;
    let allowed = match call {
        Call::Create => meta.flags.creatable(),
        Call::Set => meta.flags.settable(),
    };
    if !allowed {
        return Err(Error::attr(AttrError::Invalid, 0));
    }
    // SAFETY: `attr` comes from a slice or reference made by `ptr` under the SAI caller's
    // contract (see `decode_list`, `set`): the member SAI defines for this attribute id is
    // initialised, and `meta.value_type` is that member, taken from the SAI headers.
    let value = unsafe { convert::decode(meta.value_type, &attr.value) }?;
    Ok(Attr::new(attr.id, value))
}

/// `create_switch`: the second stage of bring-up.
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
    // Every pointer is checked before anything is created: a switch that exists but whose id
    // could not be handed back would be lost to the caller.
    // SAFETY: this function's contract.
    let out = unsafe { ptr::out(switch_id) }?;
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
    let services = state::with_api(|api| Ok(api.services))?;
    let keys: Vec<&str> = core::iter::once(BACKEND_KEY)
        .chain(sai_core::virt::PROFILE_KEYS)
        .collect();
    let profile = services.profile(profile_id, &keys);
    // The virtual switch is never chosen implicitly: on a real switch a library that
    // "succeeds" without programming anything must be something the operator asked for.
    if profile.get(BACKEND_KEY) != Some("virtual") {
        log::emit(
            sys::SAI_API_SWITCH,
            sys::SAI_LOG_LEVEL_ERROR,
            format_args!(
                "{BACKEND_KEY}={:?} in the SAI profile: only \"virtual\" exists, and it must be asked for",
                profile.get(BACKEND_KEY)
            ),
        );
        return Err(sys::SAI_STATUS_FAILURE);
    }

    let id = state::with_api(|api| {
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
                Ok(id)
            }
            Err(status) => {
                // No thread, no notifications: not a switch to hand out.
                let _ = api.sai.remove(sys::SAI_OBJECT_TYPE_SWITCH, id.raw());
                Err(status)
            }
        }
    })?;
    log::emit(
        sys::SAI_API_SWITCH,
        sys::SAI_LOG_LEVEL_WARN,
        format_args!("VIRTUAL backend selected: no hardware is programmed"),
    );
    out.write(id.raw().0);
    Ok(())
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
    let notifier = state::with_api(|api| {
        api.sai.remove(object_type, RawOid(oid)).map_err(Error::status)?;
        Ok(if object_type == sys::SAI_OBJECT_TYPE_SWITCH {
            api.notifier.take()
        } else {
            None
        })
    })?;
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
    let attr = unsafe { ptr::in_ref(attr) }?;
    let attr = decode_one(object_type, attr, Call::Set).map_err(Error::status)?;
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
    let attrs = unsafe { ptr::inout_slice(attr_list, attr_count) }?;
    let ids: Vec<sys::sai_attr_id_t> = attrs.iter().map(|a| a.id).collect();
    let values = state::with_api(|api| api.sai.get(object_type, RawOid(oid), &ids).map_err(Error::status))?;
    let mut overflow = false;
    for (slot, value) in attrs.iter_mut().zip(&values) {
        // SAFETY: the core returns values of the kind SAI defines for each requested id (it
        // checks), so a list result goes to the list member this function's contract covers.
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

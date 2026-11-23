//! The exported `sai_*` functions (`sai.h`, `saiobject.h`): the symbols of `libsai.so`.
use crate::Status;
use crate::guard::{guard, guard_value};
use crate::{log, ptr, state, tables};
use core::ffi::{c_char, c_void};
use sai_core::caps;
use sai_core::error::Error;
use sai_core::oid::RawOid;
use sai_sys as sys;

const GLOBAL: sys::sai_api_t = sys::SAI_API_UNSPECIFIED;

/// First stage of bring-up: remembers the SAI user's service method table. No hardware is
/// touched before a switch is created.
///
/// # Safety
/// `services` is null or points to a service method table whose function pointers are null
/// or stay callable, from any thread, with the signatures of the SAI headers until
/// `sai_api_uninitialize` returns; `profile_get_value` returns null or a NUL-terminated string
/// valid at least until the next call of a table function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_api_initialize(flags: u64, services: *const sys::sai_service_method_table_t) -> Status {
    guard(GLOBAL, || {
        if flags != 0 {
            return Err(sys::SAI_STATUS_INVALID_PARAMETER);
        }
        // SAFETY: this function's contract.
        let table = unsafe { ptr::in_ref(services) }?;
        // SAFETY: this function's contract is `Services::new`'s.
        let services = unsafe { state::Services::new(table) };
        state::initialize(services)
    })
}

/// Ends the API: removes a switch that still exists, stops its notifier thread and forgets
/// everything, also after a panic left the state unusable.
#[unsafe(no_mangle)]
pub extern "C" fn sai_api_uninitialize() -> Status {
    guard(GLOBAL, state::uninitialize)
}

/// Hands out the method table of one API. The table is constant: the caller must not write
/// to it (the C type is not `const`).
///
/// # Safety
/// `api_method_table` is null or valid for a write of a pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_api_query(api: sys::sai_api_t, api_method_table: *mut *mut c_void) -> Status {
    guard(GLOBAL, || {
        // SAFETY: this function's contract.
        let out = unsafe { ptr::out(api_method_table) }?;
        state::with_api(|_| Ok(()))?; // SAI_STATUS_UNINITIALIZED before sai_api_initialize
        let table: *const c_void = match tables::of(api) {
            Some(table) => table,
            None if api == sys::SAI_API_UNSPECIFIED || api >= sys::SAI_API_MAX => {
                return Err(sys::SAI_STATUS_INVALID_PARAMETER);
            }
            None => return Err(sys::SAI_STATUS_NOT_IMPLEMENTED),
        };
        out.write(table.cast_mut());
        Ok(())
    })
}

/// Sets the log level of one API.
#[unsafe(no_mangle)]
pub extern "C" fn sai_log_set(api: sys::sai_api_t, log_level: sys::sai_log_level_t) -> Status {
    guard(GLOBAL, || {
        if log::set_level(api, log_level) {
            Ok(())
        } else {
            Err(sys::SAI_STATUS_INVALID_PARAMETER)
        }
    })
}

/// The object type an id's bits name; `SAI_OBJECT_TYPE_NULL` when they name none. Decodes
/// only: the object need not exist.
#[unsafe(no_mangle)]
pub extern "C" fn sai_object_type_query(object_id: sys::sai_object_id_t) -> sys::sai_object_type_t {
    guard_value(sys::SAI_OBJECT_TYPE_NULL, || RawOid(object_id).sai_object_type())
}

/// The switch an id belongs to; the null id when the id names no object type.
#[unsafe(no_mangle)]
pub extern "C" fn sai_switch_id_query(object_id: sys::sai_object_id_t) -> sys::sai_object_id_t {
    guard_value(0, || RawOid(object_id).switch_id().0)
}

/// The version of the SAI headers this library was generated from.
///
/// # Safety
/// `version` is null or valid for a write of a `sai_api_version_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_query_api_version(version: *mut sys::sai_api_version_t) -> Status {
    guard(GLOBAL, || {
        // SAFETY: this function's contract.
        unsafe { ptr::out(version) }?.write(sys::SAI_API_VERSION);
        Ok(())
    })
}

/// Which of create, set and get an attribute is implemented for (the table of
/// `sai_core::caps`, the C++ sai-openbcm's). An attribute that is not in the table is
/// answered with all three false, and success.
///
/// # Safety
/// `attr_capability` is null or valid for a write of a `sai_attr_capability_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_query_attribute_capability(
    _switch_id: sys::sai_object_id_t,
    object_type: sys::sai_object_type_t,
    attr_id: sys::sai_attr_id_t,
    attr_capability: *mut sys::sai_attr_capability_t,
) -> Status {
    guard(GLOBAL, || {
        // SAFETY: this function's contract.
        let out = unsafe { ptr::out(attr_capability) }?;
        // Cut to what the backend programs; before sai_api_initialize, the table itself.
        let capability = state::with_api(|api| Ok(api.sai.attribute_capability(object_type, attr_id)))
            .unwrap_or_else(|_| caps::attribute(object_type, attr_id));
        out.write(sys::sai_attr_capability_t {
            create_implemented: capability.create,
            set_implemented: capability.set,
            get_implemented: capability.get,
        });
        Ok(())
    })
}

/// The enumerators of an attribute that are implemented, with SAI's list convention (count
/// in: capacity; out: needed). `SAI_STATUS_NOT_SUPPORTED` for attributes without a list.
///
/// # Safety
/// `enum_values_capability` is null or points to a list whose count is the capacity of the
/// buffer its pointer names (valid for that many writes), or zero; nothing else accesses
/// either during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_query_attribute_enum_values_capability(
    _switch_id: sys::sai_object_id_t,
    object_type: sys::sai_object_type_t,
    attr_id: sys::sai_attr_id_t,
    enum_values_capability: *mut sys::sai_s32_list_t,
) -> Status {
    guard(GLOBAL, || {
        // SAFETY: this function's contract.
        let list = unsafe { ptr::inout_slice(enum_values_capability, 1) }?
            .first_mut()
            .ok_or(sys::SAI_STATUS_INVALID_PARAMETER)?;
        let values = state::with_api(|api| {
            api.sai
                .enum_values_capability(object_type, attr_id)
                .map_err(Error::status)
        })?;
        // SAFETY: this function's contract is `write_list`'s for the list's count and buffer.
        unsafe { ptr::write_list(&mut list.count, list.list, &values) }
    })
}

/// The counters the chip has for objects of a type, each readable and clearable, with SAI's
/// list convention.
///
/// # Safety
/// `stats_capability` is null or points to a list whose count is the capacity of the buffer
/// its pointer names (valid for that many writes), or zero; nothing else accesses either
/// during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sai_query_stats_capability(
    _switch_id: sys::sai_object_id_t,
    object_type: sys::sai_object_type_t,
    stats_capability: *mut sys::sai_stat_capability_list_t,
) -> Status {
    guard(GLOBAL, || {
        // SAFETY: this function's contract.
        let list = unsafe { ptr::inout_slice(stats_capability, 1) }?
            .first_mut()
            .ok_or(sys::SAI_STATUS_INVALID_PARAMETER)?;
        let counters = state::with_api(|api| api.sai.stats_capability(object_type).map_err(Error::status))?;
        let items: Vec<sys::sai_stat_capability_t> = counters
            .into_iter()
            .map(|stat_enum| sys::sai_stat_capability_t {
                stat_enum,
                stat_modes: sys::SAI_STATS_MODE_READ | sys::SAI_STATS_MODE_READ_AND_CLEAR,
            })
            .collect();
        // SAFETY: this function's contract is `write_list`'s for the list's count and buffer.
        unsafe { ptr::write_list(&mut list.count, list.list, &items) }
    })
}

/// Entry points that exist so that SAI users link, and that answer one fixed status
/// without looking at their arguments.
macro_rules! fixed_status {
    ($status:ident: $($(#[$doc:meta])* fn $name:ident($($arg:ident: $ty:ty),* $(,)?);)*) => {$(
        $(#[$doc])*
        ///
        /// Answers one fixed status (see the source) and reads or writes nothing through its
        /// pointer arguments.
        #[unsafe(no_mangle)]
        pub extern "C" fn $name($(_: $ty),*) -> Status {
            sys::$status
        }
    )*};
}

fixed_status! { SAI_STATUS_NOT_IMPLEMENTED:
    /// `sai_dbg_generate_dump`
    fn sai_dbg_generate_dump(dump_file_name: *const c_char);
    /// `sai_bulk_get_attribute`
    fn sai_bulk_get_attribute(
        switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, object_count: u32, object_key: *const sys::sai_object_key_t,
        attr_count: *mut u32, attr_list: *mut *mut sys::sai_attribute_t, object_statuses: *mut Status,
    );
    /// `sai_query_object_stage`
    fn sai_query_object_stage(
        switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, attr_count: u32, attr_list: *const sys::sai_attribute_t,
        stage: *mut sys::sai_object_stage_t,
    );
}

// As the C++ sai-openbcm answers them: syncd and orchagent take "not supported" as "ask
// the switch attributes instead" (CRM) or "walk the objects yourself" (discovery).
fixed_status! { SAI_STATUS_NOT_SUPPORTED:
    /// `sai_object_type_get_availability`
    fn sai_object_type_get_availability(
        switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, attr_count: u32, attr_list: *const sys::sai_attribute_t, count: *mut u64,
    );
    /// `sai_get_maximum_attribute_count`
    fn sai_get_maximum_attribute_count(switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, count: *mut u32);
    /// `sai_get_object_count`
    fn sai_get_object_count(switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, count: *mut u32);
    /// `sai_get_object_key`
    fn sai_get_object_key(
        switch_id: sys::sai_object_id_t, object_type: sys::sai_object_type_t, object_count: *mut u32, object_list: *mut sys::sai_object_key_t,
    );
}

/// Compile-time check that every exported function has exactly the prototype the SAI headers
/// declare (`sai_sys`'s declarations are generated from them): each pair must coerce to one
/// function pointer type. Never called.
#[allow(dead_code)]
fn prototypes_match_the_headers() {
    fn same<T>(_: T, _: T) {}
    same::<unsafe extern "C" fn(_, _) -> _>(sys::sai_api_initialize, sai_api_initialize);
    same::<unsafe extern "C" fn() -> _>(sys::sai_api_uninitialize, sai_api_uninitialize);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::sai_api_query, sai_api_query);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::sai_log_set, sai_log_set);
    same::<unsafe extern "C" fn(_) -> _>(sys::sai_object_type_query, sai_object_type_query);
    same::<unsafe extern "C" fn(_) -> _>(sys::sai_switch_id_query, sai_switch_id_query);
    same::<unsafe extern "C" fn(_) -> _>(sys::sai_query_api_version, sai_query_api_version);
    same::<unsafe extern "C" fn(_) -> _>(sys::sai_dbg_generate_dump, sai_dbg_generate_dump);
    same::<unsafe extern "C" fn(_, _, _, _, _) -> _>(
        sys::sai_object_type_get_availability,
        sai_object_type_get_availability,
    );
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::sai_get_maximum_attribute_count, sai_get_maximum_attribute_count);
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::sai_get_object_count, sai_get_object_count);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(sys::sai_get_object_key, sai_get_object_key);
    same::<unsafe extern "C" fn(_, _, _, _, _, _, _) -> _>(sys::sai_bulk_get_attribute, sai_bulk_get_attribute);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(sys::sai_query_attribute_capability, sai_query_attribute_capability);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(
        sys::sai_query_attribute_enum_values_capability,
        sai_query_attribute_enum_values_capability,
    );
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::sai_query_stats_capability, sai_query_stats_capability);
    same::<unsafe extern "C" fn(_, _, _, _, _) -> _>(sys::sai_query_object_stage, sai_query_object_stage);
}

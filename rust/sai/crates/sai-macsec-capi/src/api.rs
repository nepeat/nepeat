//! The exported functions (`include/sai_macsec_rs.h`). Each one: check the pointers, decode,
//! call the model under the state's lock, encode, all inside `guard` (no panic crosses into C).
#![allow(unsafe_code, reason = "the C-ABI rim: see the crate documentation")]

use crate::attr::{decode, encode};
use crate::ptr::{fill_list, in_ref, in_slice, inout_ref, inout_slice, out};
use crate::state::{CRunner, STATE, State, options};
use crate::sys;
use sai_core::Error;
use sai_core::oid::{PortId, RawOid};
use sai_macsec::{
    AclEntryId, AclTableId, Attr, ChipPort, CounterMode, Dir, EngineAddr, FlowId, Macsec, MacsecId, MacsecPortId,
    Params, PortKey, SaId, ScId, Table, Value,
};
use sai_sys as sai;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::PoisonError;

const OK: i32 = sai::SAI_STATUS_SUCCESS;
const FAILURE: i32 = sai::SAI_STATUS_FAILURE;
const INVALID_PARAMETER: i32 = sai::SAI_STATUS_INVALID_PARAMETER;

thread_local! {
    /// This thread is inside an entry point (a callback calling back in would deadlock).
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Leaves the "inside" mark however the entry point ends.
struct Inside;

impl Inside {
    fn enter() -> Option<Self> {
        INSIDE.with(|i| if i.replace(true) { None } else { Some(Self) })
    }
}

impl Drop for Inside {
    fn drop(&mut self) {
        INSIDE.with(|i| i.set(false));
    }
}

/// A panic becomes `fallback`; it never unwinds into C.
fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

/// Runs `f` on the state under its lock. `None` when called from inside a callback of this
/// library, or when a panic poisoned the state (every call fails until a detach).
fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    let _inside = Inside::enter()?;
    let mut lock = STATE.lock().ok()?;
    let state = lock.get_or_insert_with(State::none);
    Some(f(state))
}

/// `with_state` for a status-returning body.
fn status_of(body: impl FnOnce(&mut State) -> i32) -> i32 {
    guard(FAILURE, || with_state(body).unwrap_or(FAILURE))
}

fn st(r: Result<(), Error>) -> i32 {
    r.map_or_else(Error::status, |()| OK)
}

fn len(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The id as the typed id of its object type; an id of another type is "no such object" (the
/// C++ looks it up in that type's map).
fn typed<T: TryFrom<RawOid>>(id: sai::sai_object_id_t) -> Result<T, Error> {
    T::try_from(RawOid(id)).map_err(|_| Error::InvalidObjectId)
}

fn decode_all(object_type: sai::sai_object_type_t, attrs: &[sai::sai_attribute_t]) -> Result<Vec<Attr>, Error> {
    attrs
        .iter()
        .enumerate()
        .map(|(i, a)| decode(object_type, a, i))
        .collect()
}

/// Whether the object exists (a get of nothing: no sequence runs), as the status of a lookup.
fn exists(state: &mut State, object_type: sai::sai_object_type_t, id: sai::sai_object_id_t) -> Result<(), Error> {
    get_values(state, object_type, id, &[]).map(|_| ())
}

fn get_values(
    state: &mut State,
    object_type: sai::sai_object_type_t,
    id: sai::sai_object_id_t,
    ids: &[sai::sai_attr_id_t],
) -> Result<Vec<Value>, Error> {
    let (m, r) = (&mut state.model, &mut state.runner);
    match object_type {
        sai::SAI_OBJECT_TYPE_MACSEC => m.get_macsec(r, typed::<MacsecId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_MACSEC_PORT => m.get_macsec_port(typed::<MacsecPortId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_MACSEC_FLOW => m.get_flow(typed::<FlowId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_MACSEC_SC => m.get_sc(typed::<ScId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_MACSEC_SA => m.get_sa(r, typed::<SaId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_ACL_TABLE => m.get_acl_table(typed::<AclTableId>(id)?, ids),
        sai::SAI_OBJECT_TYPE_ACL_ENTRY => m.get_acl_entry(typed::<AclEntryId>(id)?, ids),
        _ => Err(Error::InvalidObjectType),
    }
}

/// `OB_RS_MACSEC_ABI` of the library.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_abi() -> u32 {
    sys::OB_RS_MACSEC_ABI
}

/// Builds the model of the switch (`ext_macsec_attach`).
///
/// # Safety
/// `callbacks` points to an `ob_rs_callbacks` of functions with the header's signatures (they
/// stay valid while the model lives); each array points to its count of initialised elements,
/// and every name / results pointer in them to its length of bytes (the header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_attach(
    callbacks: *const sys::ob_rs_callbacks,
    loaded: u32,
    clear_on_read: u32,
    ifg_bits: u32,
    engines: *const sys::ob_rs_engine,
    n_engines: usize,
    seqs: *const sys::ob_rs_seq,
    n_seqs: usize,
    ports: *const sys::ob_rs_chip_port,
    n_ports: usize,
) -> i32 {
    guard(FAILURE, || {
        // SAFETY: the caller's contract for each pointer.
        let (Some(cb), Some(engines), Some(seqs), Some(ports)) = (
            unsafe { in_ref(callbacks) },
            unsafe { in_slice(engines, n_engines) },
            unsafe { in_slice(seqs, n_seqs) },
            unsafe { in_slice(ports, n_ports) },
        ) else {
            return INVALID_PARAMETER;
        };
        let text = |p: *const c_char, n: usize| -> Option<String> {
            // SAFETY: n bytes at p (the caller's contract for the names in the arrays).
            let bytes = unsafe { in_slice(p.cast::<u8>(), n) }?;
            std::str::from_utf8(bytes).ok().map(str::to_owned)
        };
        let mut runner = CRunner {
            cb: Some(*cb),
            names: Vec::with_capacity(seqs.len()),
            results: BTreeMap::new(),
        };
        for s in seqs {
            let (Some(name), Some(results)) = (text(s.name, s.name_len), text(s.results, s.results_len)) else {
                return INVALID_PARAMETER;
            };
            let results = results
                .split('\n')
                .filter(|r| !r.is_empty())
                .map(str::to_owned)
                .collect();
            runner.names.push(name.clone());
            runner.results.insert(name, results);
        }
        let mut chip = Vec::with_capacity(ports.len());
        for p in ports {
            let Some(name) = text(p.name, p.name_len) else {
                return INVALID_PARAMETER;
            };
            let lanes = if p.has_lane != 0 {
                vec![p.first_lane]
            } else {
                Vec::new()
            };
            chip.push(ChipPort {
                key: PortKey(p.hw),
                name,
                lanes,
            });
        }
        let table = Table {
            counters: if clear_on_read != 0 {
                CounterMode::ClearOnRead
            } else {
                CounterMode::Absolute
            },
            ifg_bits,
            engines: engines
                .iter()
                .map(|e| EngineAddr {
                    lane: e.lane,
                    phy: e.phy,
                })
                .collect(),
        };
        let Some(_inside) = Inside::enter() else {
            return FAILURE;
        };
        // a new switch: whatever a panic left behind is replaced
        let mut lock = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        *lock = None; // the old model's keys are wiped as it is dropped
        STATE.clear_poison();
        let model = if loaded == 0 {
            Ok(Macsec::without_table(options(*cb)))
        } else {
            Macsec::attach_chip(&table, &chip, options(*cb), &mut runner)
        };
        match model {
            Ok(model) => {
                *lock = Some(State { model, runner });
                OK
            }
            Err(e) => e.status(),
        }
    })
}

/// Forgets the model (keys wiped as it is dropped); runs nothing; clears a contained panic.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_detach() {
    guard((), || {
        let Some(_inside) = Inside::enter() else {
            return;
        };
        let mut lock = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        *lock = None; // keys wiped as the model is dropped; engines left as they are (Q2)
        STATE.clear_poison();
    });
}

/// The switch's front-panel SAI ports.
///
/// # Safety
/// `ports` points to `n` initialised elements (or `n` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_ports(ports: *const sys::ob_rs_switch_port, n: usize) {
    guard((), || {
        // SAFETY: the caller's contract.
        let Some(ports) = (unsafe { in_slice(ports, n) }) else {
            return;
        };
        let pairs: Vec<(PortId, PortKey)> = ports
            .iter()
            .filter_map(|p| Some((PortId::try_from(RawOid(p.oid)).ok()?, PortKey(p.hw))))
            .collect();
        let _ = with_state(|s| s.model.set_switch_ports(&pairs));
    });
}

/// `create_*` of the object types of the header.
///
/// # Safety
/// `id` is valid for a write; `attrs` points to `n` initialised attributes whose SAI-defined
/// members are initialised (lists: their count of elements).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_create(
    object_type: sai::sai_object_type_t,
    id: *mut sai::sai_object_id_t,
    n: u32,
    attrs: *const sai::sai_attribute_t,
) -> i32 {
    // SAFETY: the caller's contract.
    let attrs = unsafe { in_slice(attrs, len(n)) };
    let (false, Some(attrs)) = (id.is_null(), attrs) else {
        return INVALID_PARAMETER;
    };
    status_of(|s| {
        let attrs = match decode_all(object_type, attrs) {
            Ok(a) => a,
            Err(e) => return e.status(),
        };
        let (m, r) = (&mut s.model, &mut s.runner);
        let made: Result<RawOid, Error> = match object_type {
            sai::SAI_OBJECT_TYPE_MACSEC => m.create_macsec(&attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_MACSEC_PORT => m.create_macsec_port(r, &attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_MACSEC_FLOW => m.create_flow(&attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_MACSEC_SC => m.create_sc(&attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_MACSEC_SA => m.create_sa(r, &attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_ACL_TABLE => m.create_acl_table(&attrs).map(Into::into),
            sai::SAI_OBJECT_TYPE_ACL_ENTRY => m.create_acl_entry(r, &attrs).map(Into::into),
            _ => Err(Error::InvalidObjectType),
        };
        match made {
            Ok(raw) => {
                // SAFETY: id is valid for a write (the caller's contract; non-null checked).
                let written = unsafe { out(id, raw.0) };
                if written { OK } else { INVALID_PARAMETER }
            }
            Err(e) => e.status(),
        }
    })
}

/// `remove_*` of the object types of the header.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_remove(object_type: sai::sai_object_type_t, id: sai::sai_object_id_t) -> i32 {
    status_of(|s| {
        let (m, r) = (&mut s.model, &mut s.runner);
        st(match object_type {
            sai::SAI_OBJECT_TYPE_MACSEC => typed(id).and_then(|i| m.remove_macsec(i)),
            sai::SAI_OBJECT_TYPE_MACSEC_PORT => typed(id).and_then(|i| m.remove_macsec_port(r, i)),
            sai::SAI_OBJECT_TYPE_MACSEC_FLOW => typed(id).and_then(|i| m.remove_flow(i)),
            sai::SAI_OBJECT_TYPE_MACSEC_SC => typed(id).and_then(|i| m.remove_sc(i)),
            sai::SAI_OBJECT_TYPE_MACSEC_SA => typed(id).and_then(|i| m.remove_sa(r, i)),
            sai::SAI_OBJECT_TYPE_ACL_TABLE => typed(id).and_then(|i| m.remove_acl_table(i)),
            sai::SAI_OBJECT_TYPE_ACL_ENTRY => typed(id).and_then(|i| m.remove_acl_entry(r, i)),
            _ => Err(Error::InvalidObjectType),
        })
    })
}

/// `set_*_attribute`.
///
/// # Safety
/// A non-null `attr` points to an initialised attribute (its SAI-defined member).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_set(
    object_type: sai::sai_object_type_t,
    id: sai::sai_object_id_t,
    attr: *const sai::sai_attribute_t,
) -> i32 {
    // SAFETY: the caller's contract.
    let attr = unsafe { in_ref(attr) };
    status_of(|s| {
        let a = match attr.map(|a| decode(object_type, a, 0)) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return e.status(),
            // the C++ answers a flow's set without looking at the attribute
            None if object_type == sai::SAI_OBJECT_TYPE_MACSEC_FLOW => Attr::new(0, Value::U32(0)),
            None => return st(exists(s, object_type, id).and(Err(Error::InvalidParameter))),
        };
        let (m, r) = (&mut s.model, &mut s.runner);
        st(match object_type {
            sai::SAI_OBJECT_TYPE_MACSEC => typed(id).and_then(|i| m.set_macsec(i, &a)),
            sai::SAI_OBJECT_TYPE_MACSEC_PORT => typed(id).and_then(|i| m.set_macsec_port(i, &a)),
            sai::SAI_OBJECT_TYPE_MACSEC_FLOW => typed(id).and_then(|i| m.set_flow(i, &a)),
            sai::SAI_OBJECT_TYPE_MACSEC_SC => typed(id).and_then(|i| m.set_sc(i, &a)),
            sai::SAI_OBJECT_TYPE_MACSEC_SA => typed(id).and_then(|i| m.set_sa(i, &a)),
            sai::SAI_OBJECT_TYPE_ACL_ENTRY => typed(id).and_then(|i| m.set_acl_entry(r, i, &a)),
            _ => Err(Error::InvalidObjectType),
        })
    })
}

/// `get_*_attribute`.
///
/// # Safety
/// `attrs` points to `n` attributes valid for reading and writing (their ids set; list
/// members: the caller's capacity and buffer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_get(
    object_type: sai::sai_object_type_t,
    id: sai::sai_object_id_t,
    n: u32,
    attrs: *mut sai::sai_attribute_t,
) -> i32 {
    // SAFETY: the caller's contract.
    let attrs = unsafe { inout_slice(attrs, len(n)) };
    status_of(|s| {
        let Some(attrs) = attrs else {
            // the C++ looks the object up first
            return st(exists(s, object_type, id).and(Err(Error::InvalidParameter)));
        };
        let ids: Vec<sai::sai_attr_id_t> = attrs.iter().map(|a| a.id).collect();
        let values = match get_values(s, object_type, id, &ids) {
            Ok(v) => v,
            Err(e) => return e.status(),
        };
        for (value, a) in values.iter().zip(attrs.iter_mut()) {
            if let Err(status) = encode(value, a) {
                return status;
            }
        }
        OK
    })
}

/// `get_*_stats` (the plain read).
///
/// # Safety
/// `ids` points to `n` initialised ids and `out` to `n` writable counters (or `n` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_stats(
    object_type: sai::sai_object_type_t,
    id: sai::sai_object_id_t,
    n: u32,
    ids: *const sai::sai_stat_id_t,
    counters: *mut u64,
) -> i32 {
    // SAFETY: the caller's contract.
    let ids = unsafe { in_slice(ids, len(n)) };
    // SAFETY: the caller's contract.
    let counters = unsafe { inout_slice(counters, len(n)) };
    status_of(|s| {
        let (m, r) = (&mut s.model, &mut s.runner);
        let values = match (object_type, ids, counters) {
            // no counters: the C++ answers by the count alone
            (sai::SAI_OBJECT_TYPE_MACSEC_PORT, ..) => {
                let dummy = vec![0; len(n).min(1)];
                return st(typed(id).and_then(|i| m.get_macsec_port_stats(i, &dummy).map(|_| ())));
            }
            (sai::SAI_OBJECT_TYPE_MACSEC_SC, ..) => {
                let dummy = vec![0; len(n).min(1)];
                return st(typed(id).and_then(|i| m.get_sc_stats(i, &dummy).map(|_| ())));
            }
            (sai::SAI_OBJECT_TYPE_MACSEC_FLOW | sai::SAI_OBJECT_TYPE_MACSEC_SA, Some(ids), Some(counters)) => {
                let values = if object_type == sai::SAI_OBJECT_TYPE_MACSEC_FLOW {
                    typed(id).and_then(|i| m.get_flow_stats(r, i, ids))
                } else {
                    typed(id).and_then(|i| m.get_sa_stats(r, i, ids))
                };
                match values {
                    Ok(v) => (v, counters),
                    Err(e) => return e.status(),
                }
            }
            (sai::SAI_OBJECT_TYPE_MACSEC_FLOW | sai::SAI_OBJECT_TYPE_MACSEC_SA, ..) => {
                return st(exists(s, object_type, id).and(Err(Error::InvalidParameter)));
            }
            _ => return Error::InvalidObjectType.status(),
        };
        let (values, counters) = values;
        for (c, v) in counters.iter_mut().zip(values) {
            *c = v;
        }
        OK
    })
}

/// `clear_*_stats`.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_clear_stats(object_type: sai::sai_object_type_t, id: sai::sai_object_id_t) -> i32 {
    status_of(|s| {
        let (m, r) = (&mut s.model, &mut s.runner);
        st(match object_type {
            sai::SAI_OBJECT_TYPE_MACSEC_FLOW => typed(id).and_then(|i| m.clear_flow_stats(r, i)),
            sai::SAI_OBJECT_TYPE_MACSEC_SA => typed(id).and_then(|i| m.clear_sa_stats(r, i)),
            sai::SAI_OBJECT_TYPE_MACSEC_PORT => typed(id).and_then(|i| m.clear_macsec_port_stats(i)),
            sai::SAI_OBJECT_TYPE_MACSEC_SC => typed(id).and_then(|i| m.clear_sc_stats(i)),
            _ => Err(Error::InvalidObjectType),
        })
    })
}

/// 1 when `id` is a MACsec-stage ACL table / entry of the model.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_owns(object_type: sai::sai_object_type_t, id: sai::sai_object_id_t) -> u32 {
    guard(0, || {
        with_state(|s| match object_type {
            sai::SAI_OBJECT_TYPE_ACL_TABLE => s.model.acl_table(RawOid(id)).is_some(),
            sai::SAI_OBJECT_TYPE_ACL_ENTRY => s.model.acl_entry(RawOid(id)).is_some(),
            _ => false,
        })
        .map_or(0, u32::from)
    })
}

fn dir(ingress: u32) -> Dir {
    if ingress == 0 { Dir::Egress } else { Dir::Ingress }
}

/// `PORT_ATTR_{INGRESS,EGRESS}_MACSEC_ACL` set; errors for attribute `index`.
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_port_acl_set(
    port: sai::sai_object_id_t,
    ingress: u32,
    table: sai::sai_object_id_t,
    index: u32,
) -> i32 {
    status_of(|s| {
        let Ok(port) = PortId::try_from(RawOid(port)) else {
            return FAILURE;
        };
        st(s.model
            .set_port_acl(&mut s.runner, port, dir(ingress), RawOid(table))
            .map_err(|e| e.at_index(len(index))))
    })
}

/// The MACsec ACL table bound to `port` (null: none).
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_port_acl(port: sai::sai_object_id_t, ingress: u32) -> sai::sai_object_id_t {
    guard(0, || {
        let Ok(port) = PortId::try_from(RawOid(port)) else {
            return 0;
        };
        with_state(|s| s.model.port_acl(port, dir(ingress)).0).unwrap_or(0)
    })
}

/// `PORT_ATTR_MACSEC_PORT_LIST`.
///
/// # Safety
/// `list` points to an initialised `sai_object_list_t` whose buffer has room for its count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_port_list(port: sai::sai_object_id_t, list: *mut sai::sai_object_list_t) -> i32 {
    // SAFETY: the caller's contract.
    let Some(list) = (unsafe { inout_ref(list) }) else {
        return INVALID_PARAMETER;
    };
    status_of(|s| {
        let items: Vec<sai::sai_object_id_t> = PortId::try_from(RawOid(port))
            .map(|p| s.model.port_macsec_ports(p).iter().map(|m| m.raw().0).collect())
            .unwrap_or_default();
        // SAFETY: the buffer has room for list.count elements (the caller's contract).
        match unsafe { fill_list(&mut list.count, list.list, &items) } {
            Ok(()) => OK,
            Err(status) => status,
        }
    })
}

/// The inter-frame gap chip port `hw` needs for MACsec (0: none).
#[unsafe(no_mangle)]
pub extern "C" fn ob_rs_macsec_port_ifg(hw: u32) -> u32 {
    guard(0, || with_state(|s| s.model.port_ifg(PortKey(hw))).unwrap_or(0))
}

/// `sai_query_stats_capability` for MACSEC_SA / MACSEC_FLOW.
///
/// # Safety
/// `out` points to an initialised `sai_stat_capability_list_t` whose buffer has room for its count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_stats_capability(
    object_type: sai::sai_object_type_t,
    out: *mut sai::sai_stat_capability_list_t,
) -> i32 {
    // SAFETY: the caller's contract.
    let Some(list) = (unsafe { inout_ref(out) }) else {
        return INVALID_PARAMETER;
    };
    status_of(|s| {
        let ids = match s.model.stats_capability(&s.runner, object_type) {
            Ok(ids) => ids,
            Err(e) => return e.status(),
        };
        let items: Vec<sai::sai_stat_capability_t> = ids
            .into_iter()
            .map(|stat_enum| sai::sai_stat_capability_t {
                stat_enum,
                stat_modes: sai::SAI_STATS_MODE_READ,
            })
            .collect();
        // SAFETY: the buffer has room for list.count elements (the caller's contract).
        match unsafe { fill_list(&mut list.count, list.list, &items) } {
            Ok(()) => OK,
            Err(status) => status,
        }
    })
}

/// Hands `text` to the caller's text callback.
fn give(text: &str, sink: sys::ob_rs_text_fn, ctx: *mut c_void) {
    if let Some(sink) = sink {
        // SAFETY: a function of the caller with the header's signature, called with its own ctx
        // during the call that passed them; the text is text.len() bytes, alive for the call.
        unsafe { sink(ctx, text.as_ptr().cast(), text.len()) };
    }
}

/// The status text, to `sink(ctx, ...)`.
///
/// # Safety
/// `sink` is null or a function with the header's signature that may be called with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_status(sink: sys::ob_rs_text_fn, ctx: *mut c_void) {
    guard((), || {
        let text = with_state(|s| s.model.status(&s.runner)).unwrap_or_else(|| {
            "extmacsec: the MACsec model is not available (a failed call: see the log)\n".to_owned()
        });
        give(&text, sink, ctx);
    });
}

/// One sequence by hand (`extmacsec run`), its text to `sink(ctx, ...)`.
///
/// # Safety
/// `port` / `seq` point to their length of bytes; `params` to `n_params` parameters whose
/// pointers point to their lengths of bytes; `sink` as for `ob_rs_macsec_status`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ob_rs_macsec_diag_run(
    port: *const c_char,
    port_len: usize,
    seq: *const c_char,
    seq_len: usize,
    params: *const sys::ob_rs_param,
    n_params: usize,
    sink: sys::ob_rs_text_fn,
    ctx: *mut c_void,
) {
    guard((), || {
        let text = |p: *const c_char, n: usize| -> Option<String> {
            // SAFETY: n bytes at p (the caller's contract).
            let bytes = unsafe { in_slice(p.cast::<u8>(), n) }?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        };
        // SAFETY: the caller's contract.
        let lent = unsafe { in_slice(params, n_params) };
        let (Some(port), Some(seq), Some(lent)) = (text(port, port_len), text(seq, seq_len), lent) else {
            give("extmacsec: bad arguments\n", sink, ctx);
            return;
        };
        let mut par = Params::new();
        for p in lent {
            // SAFETY: the parameter's name and bytes (the caller's contract).
            let (Some(name), Some(bytes)) = (text(p.name, p.name_len), unsafe { in_slice(p.bytes, p.len) }) else {
                continue;
            };
            par.set(&name, bytes); // copied into wiped storage
        }
        let out = with_state(|s| s.model.diag_run(&mut s.runner, &port, &seq, &par))
            .unwrap_or_else(|| "extmacsec: the MACsec model is not available\n".to_owned());
        drop(par);
        give(&out, sink, ctx);
    });
}

/// Never called: the functions above against the prototypes of `sai_macsec_rs.h` (`sys` holds
/// bindgen's declarations). Each pair must coerce to one function pointer type, so a header and
/// an implementation that disagree do not compile.
#[allow(dead_code)]
fn prototypes_match_the_header() {
    fn same<T>(_: T, _: T) {}
    same::<unsafe extern "C" fn() -> _>(sys::ob_rs_macsec_abi, ob_rs_macsec_abi);
    same::<unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _) -> _>(sys::ob_rs_macsec_attach, ob_rs_macsec_attach);
    same::<unsafe extern "C" fn()>(sys::ob_rs_macsec_detach, ob_rs_macsec_detach);
    same::<unsafe extern "C" fn(_, _)>(sys::ob_rs_macsec_ports, ob_rs_macsec_ports);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(sys::ob_rs_macsec_create, ob_rs_macsec_create);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_remove, ob_rs_macsec_remove);
    same::<unsafe extern "C" fn(_, _, _) -> _>(sys::ob_rs_macsec_set, ob_rs_macsec_set);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(sys::ob_rs_macsec_get, ob_rs_macsec_get);
    same::<unsafe extern "C" fn(_, _, _, _, _) -> _>(sys::ob_rs_macsec_stats, ob_rs_macsec_stats);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_clear_stats, ob_rs_macsec_clear_stats);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_owns, ob_rs_macsec_owns);
    same::<unsafe extern "C" fn(_, _, _, _) -> _>(sys::ob_rs_macsec_port_acl_set, ob_rs_macsec_port_acl_set);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_port_acl, ob_rs_macsec_port_acl);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_port_list, ob_rs_macsec_port_list);
    same::<unsafe extern "C" fn(_) -> _>(sys::ob_rs_macsec_port_ifg, ob_rs_macsec_port_ifg);
    same::<unsafe extern "C" fn(_, _) -> _>(sys::ob_rs_macsec_stats_capability, ob_rs_macsec_stats_capability);
    same::<unsafe extern "C" fn(_, _)>(sys::ob_rs_macsec_status, ob_rs_macsec_status);
    same::<unsafe extern "C" fn(_, _, _, _, _, _, _, _)>(sys::ob_rs_macsec_diag_run, ob_rs_macsec_diag_run);
}

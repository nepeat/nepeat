//! Notifications. Backends only queue events ([`sai_core::backend::EventSink`]). One thread
//! per switch takes them off the queue, applies each to the model under the API lock, and
//! calls the SAI user's callback after releasing the lock. Removing the switch closes the
//! queue and waits for this thread, so no callback runs after `remove_switch` returned.
use crate::Status;
use crate::state;
use sai_core::backend::BackendEvent;
use sai_core::oid::TypedId;
use sai_core::sai::Notification;
use sai_core::value::Pointer;
use sai_sys as sys;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

/// Backend events the notifier has finished: applied (or dropped as another switch's) and their
/// callback returned. Monotonic; read by the test port (`events_delivered`).
pub(crate) static EVENTS_DELIVERED: AtomicU64 = AtomicU64::new(0);

/// Starts the notifier of the switch created as number `generation`.
pub(crate) fn spawn(events: Receiver<BackendEvent>, generation: u64) -> Result<JoinHandle<()>, Status> {
    std::thread::Builder::new()
        .name("sai-notify".into())
        .spawn(move || run(&events, generation))
        .map_err(|_| sys::SAI_STATUS_INSUFFICIENT_RESOURCES)
}

fn run(events: &Receiver<BackendEvent>, generation: u64) {
    // Ends when the backend dropped its sink (switch removed, API uninitialised).
    while let Ok(event) = events.recv() {
        let outcome = state::with_api(|api| {
            if api.generation != generation {
                return Ok(None); // an event of a switch that is gone
            }
            let notifications = api.sai.handle_event(event);
            Ok(Some((
                notifications,
                api.sai.switch_callback(sys::SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY),
                api.sai.switch_callback(sys::SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY),
            )))
        });
        // The lock is released here; the callback may call into the library.
        match outcome {
            Ok(Some((notifications, port_state, fdb))) => {
                port_state_changes(port_state, &notifications);
                fdb_events(fdb, &notifications);
            }
            Ok(None) => {}
            Err(_) => return, // uninitialised or poisoned: nobody to tell
        }
        EVENTS_DELIVERED.fetch_add(1, Ordering::Release);
    }
}

/// The C type of `SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY`'s value.
type Callback = unsafe extern "C" fn(u32, *const sys::sai_port_oper_status_notification_t);
const _: () = assert!(size_of::<Callback>() == size_of::<*const ()>());

fn port_state_changes(callback: Pointer, notifications: &[Notification]) {
    let data: Vec<sys::sai_port_oper_status_notification_t> = notifications
        .iter()
        .filter_map(|n| match n {
            Notification::PortOperStatus { port, up } => Some(sys::sai_port_oper_status_notification_t {
                port_id: port.raw().0,
                port_state: if *up {
                    sys::SAI_PORT_OPER_STATUS_UP
                } else {
                    sys::SAI_PORT_OPER_STATUS_DOWN
                },
                port_error_status: sys::SAI_PORT_ERROR_STATUS_CLEAR,
            }),
            Notification::Fdb { .. } => None,
        })
        .collect();
    let Ok(count) = u32::try_from(data.len()) else { return };
    if callback.is_null() || count == 0 {
        return;
    }
    let address: *const () = core::ptr::with_exposed_provenance(callback.addr());
    // SAFETY: `address` is the non-null value the SAI user stored in
    // SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY, which the SAI headers define as a
    // sai_port_state_change_notification_fn: a function of exactly this signature. Function
    // and data pointers have the same size here (asserted above).
    let function = unsafe { core::mem::transmute::<*const (), Callback>(address) };
    // SAFETY: the SAI contract keeps the callback callable while the switch exists, and the
    // switch's removal waits for this thread; `data` is a live array of `count` elements.
    unsafe { function(count, data.as_ptr()) };
}

/// The C type of `SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY`'s value.
type FdbCallback = unsafe extern "C" fn(u32, *const sys::sai_fdb_event_notification_data_t);
const _: () = assert!(size_of::<FdbCallback>() == size_of::<*const ()>());

/// One call of the FDB event callback per event, with the attributes the C++ sends
/// (`sai_switch.cpp` `deliver_fdb`): `TYPE` dynamic, `BRIDGE_PORT_ID`, `PACKET_ACTION` forward.
fn fdb_events(callback: Pointer, notifications: &[Notification]) {
    if callback.is_null() {
        return;
    }
    for notification in notifications {
        let Notification::Fdb {
            switch,
            kind,
            mac,
            vlan,
            bridge_port,
        } = notification
        else {
            continue;
        };
        let mut attrs = [
            sys::sai_attribute_t {
                id: sys::SAI_FDB_ENTRY_ATTR_TYPE,
                value: sys::sai_attribute_value_t {
                    s32: sys::SAI_FDB_ENTRY_TYPE_DYNAMIC.cast_signed(),
                },
            },
            sys::sai_attribute_t {
                id: sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID,
                value: sys::sai_attribute_value_t {
                    oid: bridge_port.map_or(0, |bp| bp.raw().0),
                },
            },
            sys::sai_attribute_t {
                id: sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION,
                value: sys::sai_attribute_value_t {
                    s32: sys::SAI_PACKET_ACTION_FORWARD.cast_signed(),
                },
            },
        ];
        let Ok(attr_count) = u32::try_from(attrs.len()) else {
            return;
        };
        let data = sys::sai_fdb_event_notification_data_t {
            event_type: kind.sai(),
            fdb_entry: sys::sai_fdb_entry_t {
                switch_id: switch.raw().0,
                mac_address: *mac,
                bv_id: vlan.raw().0,
            },
            attr_count,
            attr: attrs.as_mut_ptr(),
        };
        let address: *const () = core::ptr::with_exposed_provenance(callback.addr());
        // SAFETY: `address` is the non-null value the SAI user stored in
        // SAI_SWITCH_ATTR_FDB_EVENT_NOTIFY, which the SAI headers define as a
        // sai_fdb_event_notification_fn: a function of exactly this signature. Function and data
        // pointers have the same size here (asserted above).
        let function = unsafe { core::mem::transmute::<*const (), FdbCallback>(address) };
        // SAFETY: the SAI contract keeps the callback callable while the switch exists, and the
        // switch's removal waits for this thread; `data` is one live element whose `attr` points
        // to `attr_count` live attributes, all valid until the call returns.
        unsafe { function(1, &raw const data) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sai_core::l2::FdbEventKind;
    use sai_core::oid::{BridgePortId, PortId, SwitchId, VlanId};
    use std::sync::Mutex;

    /// One callback's view: count, event, entry (switch, MAC, bv id), attributes as (id, value).
    type Seen = (u32, sys::sai_fdb_event_t, u64, [u8; 6], u64, Vec<(u32, u64)>);
    static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

    extern "C" fn record(count: u32, data: *const sys::sai_fdb_event_notification_data_t) {
        // SAFETY: `fdb_events` passes one live element for the duration of the call.
        let d = unsafe { &*data };
        let n = usize::try_from(d.attr_count).unwrap_or(0);
        // SAFETY: `attr` points to `attr_count` live attributes for the duration of the call.
        let attrs = unsafe { core::slice::from_raw_parts(d.attr, n) };
        let values = attrs
            .iter()
            .map(|a| {
                let value = if a.id == sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID {
                    // SAFETY: `fdb_events` writes the `oid` member of the bridge port attribute.
                    unsafe { a.value.oid }
                } else {
                    // SAFETY: and the `s32` member of the other two.
                    u64::from(unsafe { a.value.s32 }.cast_unsigned())
                };
                (a.id, value)
            })
            .collect();
        let entry = d.fdb_entry;
        if let Ok(mut seen) = SEEN.lock() {
            seen.push((
                count,
                d.event_type,
                entry.switch_id,
                entry.mac_address,
                entry.bv_id,
                values,
            ));
        }
    }

    #[test]
    fn fdb_events_reach_the_callback_one_by_one_with_the_cpp_attributes() {
        let switch = SwitchId::from_index(1);
        let vlan = VlanId::from_index(100);
        let bridge_port = BridgePortId::from_index(7);
        let notifications = [
            Notification::Fdb {
                switch,
                kind: FdbEventKind::Learned,
                mac: [0, 0xaa, 0xbb, 0xcc, 0xdd, 1],
                vlan,
                bridge_port: Some(bridge_port),
            },
            Notification::PortOperStatus {
                port: PortId::from_index(1),
                up: true,
            },
            Notification::Fdb {
                switch,
                kind: FdbEventKind::Aged,
                mac: [2; 6],
                vlan,
                bridge_port: None,
            },
        ];
        // no callback: nothing happens
        fdb_events(Pointer::NULL, &notifications);
        let callback: extern "C" fn(u32, *const sys::sai_fdb_event_notification_data_t) = record;
        fdb_events(Pointer::from_addr(callback as usize), &notifications);
        let seen = SEEN.lock().map(|s| s.clone()).unwrap_or_default();
        let attrs = |bp: u64| {
            vec![
                (sys::SAI_FDB_ENTRY_ATTR_TYPE, u64::from(sys::SAI_FDB_ENTRY_TYPE_DYNAMIC)),
                (sys::SAI_FDB_ENTRY_ATTR_BRIDGE_PORT_ID, bp),
                (
                    sys::SAI_FDB_ENTRY_ATTR_PACKET_ACTION,
                    u64::from(sys::SAI_PACKET_ACTION_FORWARD),
                ),
            ]
        };
        assert_eq!(
            seen,
            [
                (
                    1,
                    sys::SAI_FDB_EVENT_LEARNED,
                    switch.raw().0,
                    [0, 0xaa, 0xbb, 0xcc, 0xdd, 1],
                    vlan.raw().0,
                    attrs(bridge_port.raw().0)
                ),
                (
                    1,
                    sys::SAI_FDB_EVENT_AGED,
                    switch.raw().0,
                    [2; 6],
                    vlan.raw().0,
                    attrs(0)
                ),
            ]
        );
    }
}

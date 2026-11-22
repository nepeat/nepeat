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
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

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
            )))
        });
        // The lock is released here; the callback may call into the library.
        match outcome {
            Ok(Some((notifications, callback))) => port_state_changes(callback, &notifications),
            Ok(None) => {}
            Err(_) => return, // uninitialised or poisoned: nobody to tell
        }
    }
}

/// The C type of `SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY`'s value.
type Callback = unsafe extern "C" fn(u32, *const sys::sai_port_oper_status_notification_t);
const _: () = assert!(size_of::<Callback>() == size_of::<*const ()>());

fn port_state_changes(callback: Pointer, notifications: &[Notification]) {
    let data: Vec<sys::sai_port_oper_status_notification_t> = notifications
        .iter()
        .map(|n| match n {
            Notification::PortOperStatus { port, up } => sys::sai_port_oper_status_notification_t {
                port_id: port.raw().0,
                port_state: if *up {
                    sys::SAI_PORT_OPER_STATUS_UP
                } else {
                    sys::SAI_PORT_OPER_STATUS_DOWN
                },
                port_error_status: sys::SAI_PORT_ERROR_STATUS_CLEAR,
            },
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

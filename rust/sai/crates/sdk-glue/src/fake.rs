//! Test hooks of the fake glue (`sdk-glue-sys/fake/fake_glue.c`). This module exists only in
//! a build that links the fake. All of them may be called from any thread, with or without a
//! [`crate::Switch`]; none touches hardware.

use std::ffi::c_int;

use sdk_glue_sys as sys;

fn query(what: c_int, arg: c_int) -> c_int {
    // SAFETY: ob_fake_query takes two plain integers and reads the fake's own state (the
    // callback pointer under the fake's lock, counters and port fields as plain ints).
    unsafe { sys::ob_fake_query(what, arg) }
}

/// A cable event: sets `port`'s link in the fake and calls the registered link callback on
/// the calling thread. True when a callback was called (a switch is up, the port exists, the
/// state changed and a callback is registered).
pub fn link_event(port: i32, up: bool) -> bool {
    // SAFETY: plain integers. The fake calls the callback registered through
    // Switch::set_link_callback, if any: a safe function of this crate that may be called
    // from any thread with any arguments (crate::link).
    let called = unsafe { sys::ob_fake_link_event(port, c_int::from(up)) };
    called != 0
}

/// Whether a link callback is registered with the fake.
pub fn link_callback_registered() -> bool {
    query(0, 0) != 0
}

/// Whether the BCM layer is up: a full bring-up succeeded and no shutdown followed.
pub fn bcm_layer_up() -> bool {
    query(1, 0) != 0
}

/// `ob_sdk_shutdown` calls since the last bring-up attempt that did something.
pub fn shutdown_calls() -> i32 {
    query(2, 0)
}

/// `ob_sdk_unregister_callbacks` calls since the last bring-up (the one inside a shutdown
/// included).
pub fn unregister_calls() -> i32 {
    query(3, 0)
}

/// `ob_sdk_register_callbacks` calls since the last bring-up.
pub fn register_calls() -> i32 {
    query(5, 0)
}

/// Whether `port` is enabled; `None` for a port the scenario does not have.
pub fn port_enabled(port: i32) -> Option<bool> {
    match query(4, port) {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// The board settings last written to `port`: autoneg (0 / 1), FEC mode (0 none, 1 RS, 2 FC),
/// inter-frame gap in bits, each -1 while never set, and whether an interface type was set.
/// `None` for a port the scenario does not have.
pub fn port_settings(port: i32) -> Option<(i32, i32, i32, bool)> {
    if query(4, port) < 0 {
        return None;
    }
    Some((query(6, port), query(7, port), query(8, port), query(9, port) == 1))
}

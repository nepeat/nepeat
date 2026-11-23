//! In a process that never brought the SDK up, the glue itself refuses MDIO access. The safe
//! API cannot even express that call (it needs an `Sdk`), so this test goes below it. Its own
//! test binary: the glue's "initialised" state lasts for the life of a process.
#![allow(clippy::unwrap_used, reason = "test code")]

use std::ffi::c_uint;

#[test]
fn the_glue_refuses_mdio_before_any_bring_up() {
    let mut val: c_uint = 0x5a5a;
    // SAFETY: plain integers and a pointer to a live local; the glue writes at most one
    // unsigned through it. No bring-up happened in this process, which is the point.
    let rv = unsafe { sdk_glue_sys::ob_sdk_mdio_c45_read(0, 0x24, 1, 0x8b00, &raw mut val) };
    assert_eq!(rv, sdk_glue_sys::OB_E_INIT);
    // A safe declaration (scalars, checked by the glue: sdk-glue-sys's rule).
    let rv = sdk_glue_sys::ob_sdk_mdio_c45_write(0, 0x24, 1, 0x8b00, 1);
    assert_eq!(rv, sdk_glue_sys::OB_E_INIT);
    assert_eq!(sdk_glue::LINKED, "fake");
    let e = sdk_glue::Error::from_raw(rv).unwrap();
    assert_eq!(e.code(), sdk_glue::Code::Init);
    assert_eq!(e.message(), "Feature not initialized");
}

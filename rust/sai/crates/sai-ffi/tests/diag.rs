//! The diag socket through the C ABI: `create_switch` starts it from the profile key
//! `SAI_OPENBCM_DIAG_SOCKET` (with the real `SO_PEERCRED` check), commands run under the API
//! lock, `remove_switch` stops the thread and removes the socket, and `sai_api_uninitialize`
//! does so for a switch that was never removed. One test function: the library's state is global.
#![allow(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block,
    clippy::missing_safety_doc,
    clippy::unwrap_used,
    clippy::panic,
    missing_docs
)]
// Every unsafe block here is a C caller keeping the SAI contract.
use core::ffi::{c_char, c_void};
use sai as _; // link the library: it defines the sai_* symbols declared in sai-sys
use sai_sys::*;
use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;

static SOCKET: OnceLock<CString> = OnceLock::new();

unsafe extern "C" fn profile_get_value(_profile: sai_switch_profile_id_t, variable: *const c_char) -> *const c_char {
    let key = unsafe { CStr::from_ptr(variable) };
    match key.to_bytes() {
        b"SAI_RS_BACKEND" => c"virtual".as_ptr(),
        b"SAI_RS_VIRTUAL_PORTS" => c"4".as_ptr(),
        b"SAI_OPENBCM_DIAG_SOCKET" => SOCKET.get().map_or(ptr::null(), |s| s.as_ptr()),
        _ => ptr::null(),
    }
}

fn ask(path: &Path, line: &str) -> String {
    let Ok(mut s) = UnixStream::connect(path) else {
        return "(no connect)".into();
    };
    s.write_all(line.as_bytes()).unwrap();
    s.shutdown(Shutdown::Write).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

unsafe fn switch_api() -> &'static sai_switch_api_t {
    let mut p: *mut c_void = ptr::null_mut();
    assert_eq!(unsafe { sai_api_query(SAI_API_SWITCH, &raw mut p) }, SAI_STATUS_SUCCESS);
    unsafe { p.cast::<sai_switch_api_t>().as_ref() }.unwrap()
}

unsafe fn create() -> sai_object_id_t {
    let api = unsafe { switch_api() };
    let mut attr = sai_attribute_t {
        id: SAI_SWITCH_ATTR_INIT_SWITCH,
        value: sai_attribute_value_t { u64_: 0 },
    };
    attr.value.booldata = true;
    let mut id = 0;
    assert_eq!(
        unsafe { api.create_switch.unwrap()(&raw mut id, 1, &raw const attr) },
        SAI_STATUS_SUCCESS
    );
    id
}

#[test]
#[cfg_attr(miri, ignore = "sockets and threads")]
fn create_switch_starts_it_and_remove_switch_stops_it() {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("sai-ffi-diag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("diag.sock");
    SOCKET.set(CString::new(path.to_str().unwrap()).unwrap()).unwrap();
    let services = sai_service_method_table_t {
        profile_get_value: Some(profile_get_value),
        profile_get_next_value: None,
    };
    unsafe {
        assert_eq!(sai_api_initialize(0, &raw const services), SAI_STATUS_SUCCESS);
        let id = create();
        assert!(path.exists());
        // our own uid is the socket owner's: SO_PEERCRED lets us in
        assert_eq!(ask(&path, "l2 station show\n"), "fake: l2 station show\n");
        assert_eq!(
            ask(&path, "extphy\n"),
            "extphy: no external PHY table (SAI_OPENBCM_EXT_PHY_FILE)\n"
        );
        assert!(ask(&path, "reset\n").starts_with("openbcm-diag: refused"));
        // SAI calls and diag commands from several threads at once: one lock, no deadlock
        std::thread::scope(|scope| {
            for i in 0..4 {
                let path = &path;
                scope.spawn(move || {
                    for j in 0..10 {
                        assert_eq!(ask(path, &format!("ps {i} {j}\n")), format!("fake: ps {i} {j}\n"));
                    }
                });
            }
            for _ in 0..50 {
                let mut attr = sai_attribute_t {
                    id: SAI_SWITCH_ATTR_PORT_NUMBER,
                    value: sai_attribute_value_t { u64_: 0 },
                };
                let api = switch_api();
                assert_eq!(
                    api.get_switch_attribute.unwrap()(id, 1, &raw mut attr),
                    SAI_STATUS_SUCCESS
                );
                assert_eq!(attr.value.u32_, 4);
            }
        });
        assert_eq!(switch_api().remove_switch.unwrap()(id), SAI_STATUS_SUCCESS);
        assert!(!path.exists());
        assert_eq!(ask(&path, "ps\n"), "(no connect)");
        // created again: a new server; uninitialize without removing the switch stops it
        let _id = create();
        assert_eq!(ask(&path, "ps\n"), "fake: ps\n");
        // a silent client does not keep uninitialize waiting for more than its timeout
        let _silent = UnixStream::connect(&path).unwrap();
        assert_eq!(sai_api_uninitialize(), SAI_STATUS_SUCCESS);
        assert!(!path.exists());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

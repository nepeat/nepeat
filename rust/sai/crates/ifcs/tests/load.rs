//! The loader on stand-ins built by `build.rs`: every symbol resolved, a missing one named, the
//! C++'s refusals and their texts, the tracer looked up first. No shim next to the test
//! executable: the guard says it is not installed (stderr), the load goes on (as the C++).
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
#![allow(clippy::unwrap_used, clippy::panic)]

use ifcs::{GuardOptions, load};

fn fixture(path: Option<&'static str>) -> Option<&'static str> {
    if path.is_none() {
        eprintln!("ifcs: no C compiler at build time: the loader fixture tests are SKIPPED");
    }
    path
}

/// One test: libraries stay loaded (`RTLD_GLOBAL`), so the order matters.
#[test]
fn loads_as_ifcs_lib_cpp() {
    let (Some(full), Some(missing), Some(trace), Some(nobind)) = (
        fixture(option_env!("IFCS_STUB_FULL")),
        fixture(option_env!("IFCS_STUB_MISSING")),
        fixture(option_env!("IFCS_TRACE_STUB")),
        fixture(option_env!("IFCS_TRACE_NOBIND")),
    ) else {
        return;
    };
    let off = GuardOptions {
        enable: false,
        ..GuardOptions::default()
    };
    assert_eq!(
        load("relative/libifcs.so", "", &off).unwrap_err(),
        "SAI_TERALYNX_LIBIFCS must be an absolute path (got 'relative/libifcs.so')"
    );
    assert_eq!(
        load("", "", &off).unwrap_err(),
        "SAI_TERALYNX_LIBIFCS must be an absolute path (got '')"
    );
    let e = load("/nonexistent/libifcs.so", "", &off).unwrap_err();
    assert!(
        e.starts_with("dlopen /nonexistent/libifcs.so: /nonexistent/libifcs.so: cannot open shared object file"),
        "{e}"
    );
    assert_eq!(
        load(missing, "", &off).unwrap_err(),
        format!("{missing}: missing symbol ifcs_deinit (wrong libifcs version?)")
    );
    // the guard asked for without a shim: not installed, the load succeeds
    let lib = load(full, "", &GuardOptions::default()).unwrap();
    assert_eq!(lib.path(), std::path::Path::new(full));
    assert_eq!(ifcs::SYMBOL_COUNT, ifcs_sys::SYMBOLS.len());
    assert!(!lib.has_ipd_ioctl_hook());
    // the tracer: bound to libifcs, its functions first
    let e = load(full, nobind, &off).unwrap_err();
    assert_eq!(e, format!("SAI_TERALYNX_IFCS_TRACE {nobind}: no ifcs_trace_bind"));
    let e = load(full, "/nonexistent/trace.so", &off).unwrap_err();
    assert!(
        e.starts_with("SAI_TERALYNX_IFCS_TRACE /nonexistent/trace.so: /nonexistent/trace.so: cannot open"),
        "{e}"
    );
    let traced = load(full, trace, &off).unwrap();
    assert_eq!(traced.ifcs_deinit(2), ifcs::Status(42));
}

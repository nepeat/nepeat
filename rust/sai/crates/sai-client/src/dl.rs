//! `dlopen` / `dlsym` / `dlerror`, each with its contract. The handle is never closed.
#![allow(unsafe_code)]

use core::ffi::{CStr, c_void};
use core::ptr::NonNull;
use std::ffi::CString;

/// A `dlopen` handle (never closed: the library starts threads).
#[derive(Debug)]
pub(crate) struct Library(NonNull<c_void>);

fn last_error() -> String {
    // SAFETY: dlerror has no preconditions; it returns NULL or a NUL-terminated message valid until
    // the next dl* call of this thread, which comes after the copy below.
    let p = unsafe { libc::dlerror() };
    if p.is_null() {
        return "unknown error".to_owned();
    }
    // SAFETY: non-null: a NUL-terminated string (above), copied at once.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

impl Library {
    /// `dlopen(path, RTLD_NOW | RTLD_GLOBAL)`.
    pub(crate) fn open(path: &str) -> Result<Self, String> {
        let c = CString::new(path).map_err(|_| format!("{path}: a NUL in the path"))?;
        // SAFETY: a NUL-terminated path; dlopen runs the library's constructors (the libsai this
        // program exists to drive).
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
        NonNull::new(h)
            .map(Self)
            .ok_or_else(|| format!("dlopen {path}: {}", last_error()))
    }

    /// `dlsym(handle, name)`.
    fn sym(&self, name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: a live handle (never closed) and a NUL-terminated name; dlsym only reads both.
        NonNull::new(unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) })
    }

    /// A function of the library as the function pointer type `F` the SAI header gives it.
    /// `F` must be that `unsafe extern "C" fn` type (the only callers: `ffi.rs`, with the header's
    /// prototypes).
    pub(crate) fn function<F: Copy>(&self, name: &CStr) -> Option<F> {
        const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
        let p = self.sym(name)?;
        // SAFETY: `F` is a function pointer type of the same size as a data pointer (asserted), and
        // `p` is the address of the C function of that name whose prototype `F` is (the callers name
        // the SAI header's global functions with their header prototypes).
        Some(unsafe { core::mem::transmute_copy::<NonNull<c_void>, F>(&p) })
    }
}

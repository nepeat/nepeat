//! The dynamic loader calls of the loader (`dlopen`, `dlsym`, `dlerror`), each with its contract.
//! A [`Library`] is a handle that is never closed (libifcs starts threads; the C++ never
//! `dlclose`s either).
#![allow(unsafe_code)]

use core::ffi::{CStr, c_int, c_void};
use core::ptr::NonNull;

/// A `dlopen` handle. Not `Send`: it lives only inside one load.
#[derive(Debug)]
pub(crate) struct Library(NonNull<c_void>);

/// `dlerror()`'s message, if there is one (also clears it).
pub(crate) fn last_error() -> Option<String> {
    // SAFETY: dlerror has no preconditions; it returns NULL or a NUL-terminated thread-local
    // message valid until the next dl* call of this thread, which happens after the copy below.
    let p = unsafe { libc::dlerror() };
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null: a NUL-terminated string (above), copied at once.
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// `dlopen(path, flags)`; the error is `dlerror()`'s message.
pub(crate) fn open(path: &CStr, flags: c_int) -> Result<Library, Option<String>> {
    // SAFETY: a NUL-terminated path; dlopen runs the object's constructors (libifcs', the shim's,
    // the tracer's: code this loader exists to load).
    let h = unsafe { libc::dlopen(path.as_ptr(), flags) };
    NonNull::new(h).map(Library).ok_or_else(last_error)
}

impl Library {
    /// `dlsym(handle, name)`: the address, or `None`.
    pub(crate) fn sym(&self, name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: a live handle (never closed) and a NUL-terminated name; dlsym only reads both.
        NonNull::new(unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) })
    }

    /// The raw handle, for `ifcs_trace_bind` (it takes libifcs' handle).
    pub(crate) fn raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

/// `dlsym(RTLD_DEFAULT, name)`: the global scope's binding of `name`.
pub(crate) fn global_sym(name: &CStr) -> Option<NonNull<c_void>> {
    // SAFETY: RTLD_DEFAULT is a valid pseudo-handle, the name NUL-terminated.
    NonNull::new(unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) })
}

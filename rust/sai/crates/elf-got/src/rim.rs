//! The unsafe operations of the redirect, each in its own function with its contract. Nothing
//! here decides anything: [`crate::redirect`] checks the slot (mapped, aligned, holding an
//! expected address) before it reads or writes through [`Slot`].
#![allow(unsafe_code)]

use std::ffi::{CStr, c_void};
use std::path::PathBuf;

/// A GOT slot the caller found inside a readable mapping of the object, aligned for a pointer:
/// only [`crate::redirect`] makes one (after [`crate::maps::containing`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot(usize);

impl Slot {
    /// A slot at `addr`. The caller has checked that `[addr, addr + size_of::<usize>())` is inside
    /// a readable mapping of this process and `addr` is aligned for `usize`.
    pub(crate) const fn checked(addr: usize) -> Self {
        Self(addr)
    }

    /// The address it holds.
    pub(crate) fn read(self) -> usize {
        // SAFETY: `Slot::checked`'s contract: the address is aligned and inside a readable
        // mapping that stays mapped (a loaded object's GOT is never unmapped while it is loaded,
        // and libifcs is never dlclose'd). Volatile: the slot is memory the dynamic linker owns.
        unsafe { std::ptr::read_volatile(std::ptr::with_exposed_provenance::<usize>(self.0)) }
    }

    /// Writes `value` into it. The caller has made its page writable.
    pub(crate) fn write(self, value: usize) {
        // SAFETY: as `read`, and the caller made the page writable (`protect`) or found it so; the
        // slot is a GOT entry, a pointer-sized word nothing in Rust holds a reference to. Done
        // before the object's threads start (the IFCS loader redirects before ifcs_init).
        unsafe { std::ptr::write_volatile(std::ptr::with_exposed_provenance_mut::<usize>(self.0), value) }
    }
}

/// The memory page size.
pub(crate) fn page_size() -> usize {
    // SAFETY: sysconf has no preconditions; _SC_PAGESIZE is a valid name.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(n).ok().filter(|&n| n.is_power_of_two()).unwrap_or(4096)
}

/// Sets the protection of the page at `page` (page-aligned) to `read` / `write` / `exec`.
pub(crate) fn protect(page: usize, len: usize, read: bool, write: bool, exec: bool) -> std::io::Result<()> {
    let prot = (if read { libc::PROT_READ } else { 0 })
        | (if write { libc::PROT_WRITE } else { 0 })
        | (if exec { libc::PROT_EXEC } else { 0 });
    // SAFETY: mprotect on a page-aligned range the caller found mapped in /proc/self/maps; it
    // changes protection only (no Rust reference points into a GOT page), and the caller puts the
    // original protection back after its write.
    let rc = unsafe { libc::mprotect(std::ptr::with_exposed_provenance_mut::<c_void>(page), len, prot) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The object an address belongs to: its file name and the start of its mapping (`dladdr`'s
/// `dli_fname` and `dli_fbase`). `None` when the address is in no loaded object.
pub(crate) fn object_of(addr: usize) -> Option<(PathBuf, usize)> {
    let mut info = libc::Dl_info {
        dli_fname: std::ptr::null(),
        dli_fbase: std::ptr::null_mut(),
        dli_sname: std::ptr::null(),
        dli_saddr: std::ptr::null_mut(),
    };
    // SAFETY: dladdr reads only its first argument's value (it is never dereferenced) and writes
    // the Dl_info it is given, which lives for the call.
    let found = unsafe { libc::dladdr(std::ptr::with_exposed_provenance::<c_void>(addr), &raw mut info) };
    if found == 0 || info.dli_fname.is_null() {
        return None;
    }
    // SAFETY: on success dli_fname is the NUL-terminated name of a loaded object (glibc: the
    // link map's l_name, alive while the object is loaded); it is copied at once.
    let name = unsafe { CStr::from_ptr(info.dli_fname) };
    let name = <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(name.to_bytes());
    Some((PathBuf::from(name), info.dli_fbase.addr()))
}

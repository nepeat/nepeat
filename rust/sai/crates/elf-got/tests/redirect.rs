//! The redirect on a real shared object (`tests/c/got_fixture.c`, built by `build.rs` for the target):
//! loaded with `RTLD_NOW` as the IFCS loader loads libifcs, its PLT call and its GOT address load of
//! `got_target` redirected to a Rust function and back.
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
#![allow(unsafe_code, clippy::unwrap_used)]

use elf_got::{Object, SlotKind};
use std::ffi::{CString, c_int, c_void};
use std::path::Path;

type Fn1 = extern "C" fn(c_int) -> c_int;
type FnPtr = extern "C" fn() -> Fn1;

extern "C" fn replacement(x: c_int) -> c_int {
    x + 100
}

struct Fixture {
    path: &'static str,
    target: usize,
    addr_target: usize,
    call: Fn1,
    ptr: FnPtr,
}

fn sym(handle: *mut c_void, name: &str) -> *mut c_void {
    let name = CString::new(name).unwrap();
    // SAFETY: handle is a live dlopen handle (never closed), name is NUL-terminated.
    let p = unsafe { libc::dlsym(handle, name.as_ptr()) };
    assert!(!p.is_null(), "dlsym");
    p
}

fn fixture() -> Option<Fixture> {
    let Some(path) = option_env!("ELF_GOT_FIXTURE") else {
        eprintln!("elf-got: no C compiler at build time: the fixture tests are SKIPPED");
        return None;
    };
    let c = CString::new(path).unwrap();
    // SAFETY: a path; RTLD_NOW binds every slot before dlopen returns (as the IFCS loader does).
    let handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    assert!(!handle.is_null());
    let target = sym(handle, "got_target").addr();
    let addr_target = sym(handle, "got_addr_target").addr();
    // SAFETY: the fixture defines got_call as int(int) and got_addr as int (*(void))(int).
    let call = unsafe { std::mem::transmute::<*mut c_void, Fn1>(sym(handle, "got_call")) };
    // SAFETY: as above.
    let ptr = unsafe { std::mem::transmute::<*mut c_void, FnPtr>(sym(handle, "got_addr")) };
    Some(Fixture {
        path,
        target,
        addr_target,
        call,
        ptr,
    })
}

/// One test: the fixture's slots are process-wide state.
#[test]
fn redirects_plt_and_got_slots_and_back() {
    let Some(f) = fixture() else { return };
    let obj = Object::open(Path::new(f.path), f.target).unwrap();
    let plt = obj.slots("got_target").unwrap();
    let got = obj.slots("got_addr_target").unwrap();
    assert!(
        !plt.is_empty() && plt.iter().all(|s| s.kind == SlotKind::JumpSlot),
        "{plt:?}"
    );
    assert!(
        !got.is_empty() && got.iter().all(|s| s.kind == SlotKind::GlobDat),
        "{got:?}"
    );
    assert_eq!((f.call)(1), 4);
    assert_eq!(((f.ptr)())(1), 3);
    let new = replacement as Fn1 as usize;

    for (symbol, slots, original) in [("got_target", &plt, f.target), ("got_addr_target", &got, f.addr_target)] {
        // an unexpected slot value: nothing written
        let r = obj.redirect(symbol, new, &[]).unwrap();
        assert_eq!((r.written, r.already, r.refused.len()), (0, 0, slots.len()), "{symbol}");
        assert!(r.refused.iter().all(|&(_, held)| held == original));
        // the bound address expected: every slot rewritten
        let r = obj.redirect(symbol, new, &[original]).unwrap();
        assert_eq!((r.written, r.already, r.refused.len()), (slots.len(), 0, 0), "{symbol}");
        assert_eq!(r.changed(), slots.len());
        // a second redirect finds them done
        let r = obj.redirect(symbol, new, &[original]).unwrap();
        assert_eq!((r.written, r.already), (0, slots.len()));
    }
    assert_eq!((f.call)(1), 202);
    assert_eq!(((f.ptr)())(1), 101);

    // the protection is as before: the RELRO page of the GLOB_DAT slot is read-only again, the
    // .got.plt page of the JUMP_SLOT writable
    let maps = elf_got::maps::current().unwrap();
    let mapping = |s: &elf_got::ImportSlot| {
        elf_got::maps::containing(&maps, usize::try_from(s.vaddr).unwrap() + obj.bias(), 8).unwrap()
    };
    assert!(
        got.iter()
            .all(|s| mapping(s).read && !mapping(s).write && !mapping(s).exec)
    );
    assert!(plt.iter().all(|s| mapping(s).read && mapping(s).write));

    // and back
    assert_eq!(obj.redirect("got_target", f.target, &[new]).unwrap().written, plt.len());
    assert_eq!(
        obj.redirect("got_addr_target", f.addr_target, &[new]).unwrap().written,
        got.len()
    );
    assert_eq!((f.call)(1), 4);
    assert_eq!(((f.ptr)())(1), 3);

    // a symbol the object does not import, another object's path, an address in no object
    assert_eq!(obj.redirect("no_such_symbol", 1, &[]).unwrap().changed(), 0);
    assert!(matches!(
        Object::open(Path::new("/nonexistent/libx.so"), f.target),
        Err(elf_got::Error::OtherObject(_))
    ));
    assert!(matches!(
        Object::open(Path::new(f.path), 0),
        Err(elf_got::Error::NotLoaded)
    ));
}

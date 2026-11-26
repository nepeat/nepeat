//! The loader: `ifcs_lib.cpp` `ifcs_lib_load`, step for step, with its error texts and its stderr
//! lines (the C++ writes them with `fprintf(stderr, ...)`, not through the SAI log; so does this).
#![forbid(unsafe_code)]

use core::ffi::c_void;
use core::ptr::NonNull;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::dl::{self, Library};
use crate::table::Table;
use crate::{Ifcs, Inner, ipd, shim};

/// The learn-ring read guard's settings (`ifcs_lib.h` `GuardOptions`; profile keys
/// `SAI_TERALYNX_IFCS_GUARD`, `_LEARN_RING_SIZE`, `_IM_MALLOC_LIMIT_MIB`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardOptions {
    /// Install it.
    pub enable: bool,
    /// Its Prometheus text file, empty for none.
    pub stats_path: String,
    /// Learn ring entries (the deployed init yaml: 2048).
    pub ring_size: u32,
    /// `im_malloc` above this is refused; 0 = no limit.
    pub malloc_limit: u64,
}

impl Default for GuardOptions {
    fn default() -> Self {
        Self {
            enable: true,
            stats_path: String::new(),
            ring_size: 0x800,
            malloc_limit: 1 << 30,
        }
    }
}

/// The shim's file name, next to `libsai.so` (`ifcs_lib.cpp`).
pub const SHIM: &str = "libsai-teralynx-shim.so";

/// `ifcs_lib_load`'s process-wide lock (`static std::mutex mu`).
static LOAD: Mutex<()> = Mutex::new(());

/// Something inside this library, for `dladdr` (`tl_anchor`): in `libsai.so` it names `libsai.so`.
static ANCHOR: u8 = 0;

fn cstring(s: &str) -> Option<CString> {
    CString::new(s).ok()
}

fn sym(lib: &Library, name: &str) -> Option<NonNull<c_void>> {
    cstring(name).and_then(|n| lib.sym(&n))
}

/// The addresses a slot of `symbol` may hold before a redirect: the global scope's binding and the
/// object's own scope's (an `RTLD_NOW` object's slots were bound to one of them).
fn bindings(lib: &Library, symbol: &str) -> Vec<usize> {
    let mut v = Vec::new();
    if let Some(n) = cstring(symbol) {
        v.extend(dl::global_sym(&n).map(|p| p.addr().get()));
        v.extend(lib.sym(&n).map(|p| p.addr().get()));
    }
    v
}

/// `redirect_cb` for one symbol: the slots pointing at `target` afterwards (the C++ count); a
/// failure or a refused slot is said on stderr (no C++ counterpart: the C++ writes every slot).
fn redirect(obj: Option<&elf_got::Object>, path: &str, lib: &Library, symbol: &str, target: NonNull<c_void>) -> usize {
    let Some(obj) = obj else { return 0 };
    match obj.redirect(symbol, target.addr().get(), &bindings(lib, symbol)) {
        Ok(r) => {
            for (slot, held) in &r.refused {
                eprintln!(
                    "sai-teralynx: {path}: GOT slot {slot:#x} of {symbol} holds {held:#x}, not an address {symbol} binds to: left alone"
                );
            }
            r.changed()
        }
        Err(e) => {
            eprintln!("sai-teralynx: GOT redirect of {symbol} in {path}: {e}");
            0
        }
    }
}

/// `dlopen(path, RTLD_NOW | RTLD_GLOBAL)` once per call (libifcs is never closed) and every
/// `IFCS_SYMBOLS` entry resolved, as `ifcs_lib_load`: the shim next to `libsai.so` first, then
/// libifcs, the memcpy redirect, the guard, the tracer, the table (tracer first). The error is the
/// C++'s message.
pub fn load(path: &str, trace: &str, guard: &GuardOptions) -> Result<Ifcs, String> {
    let _serial = LOAD.lock().unwrap_or_else(PoisonError::into_inner);
    if !path.starts_with('/') {
        return Err(format!("SAI_TERALYNX_LIBIFCS must be an absolute path (got '{path}')"));
    }
    let flags = libc::RTLD_NOW | libc::RTLD_GLOBAL;
    // libifcs needs CPython symbols and a Device-memory-safe memcpy (the shim), next to libsai.
    let shim = elf_got::object_of(core::ptr::addr_of!(ANCHOR).addr())
        .map(|(file, _)| file.with_file_name(SHIM))
        .and_then(|p| cstring(p.to_str()?))
        .and_then(|p| dl::open(&p, flags).ok()); // missing: libifcs' dlopen names the symbol
    let c_path = cstring(path).ok_or_else(|| format!("dlopen {path}: unknown error"))?;
    let h =
        dl::open(&c_path, flags).map_err(|e| format!("dlopen {path}: {}", e.as_deref().unwrap_or("unknown error")))?;
    // libifcs read from its file once for all redirects (elf-got); found through one of its symbols
    let object =
        sym(&h, "ifcs_init").and_then(
            |anchor| match elf_got::Object::open(Path::new(path), anchor.addr().get()) {
                Ok(o) => Some(o),
                Err(e) => {
                    eprintln!("sai-teralynx: GOT redirects in {path}: {e}");
                    None
                }
            },
        );
    if let Some(shim) = &shim {
        if let Some(f) = sym(shim, "tl_devmem_memcpy") {
            if redirect(object.as_ref(), path, &h, "memcpy", f) == 0 {
                // e.g. the test mock; the real libifcs.so imports it
                eprintln!("sai-teralynx: {path} has no memcpy import to redirect");
            }
        }
    }
    // The learn-ring read guard (ifcs_guard.c): before ifcs_init starts the learn thread.
    if guard.enable {
        install_guard(shim.as_ref(), &h, object.as_ref(), path, guard);
    }
    let tracer = if trace.is_empty() {
        None
    } else {
        // GLOBAL: the SAI script runner finds ifcs_trace_mark
        let th = cstring(trace).ok_or(None).and_then(|t| dl::open(&t, flags));
        let bind = th.as_ref().ok().and_then(|t| sym(t, "ifcs_trace_bind"));
        let Some(bind) = bind else {
            let why = match th {
                Ok(_) => "no ifcs_trace_bind".to_owned(),
                Err(e) => e.unwrap_or_else(|| "dlopen failed".to_owned()),
            };
            return Err(format!("SAI_TERALYNX_IFCS_TRACE {trace}: {why}"));
        };
        shim::trace_bind(bind, h.raw());
        eprintln!("sai-teralynx: IFCS calls traced by {trace}");
        th.ok()
    };
    let table = Table::resolve(|name| {
        let _ = dl::last_error(); // dlerror() before each, as the C++
        tracer.as_ref().and_then(|t| sym(t, name)).or_else(|| sym(&h, name))
    })
    .map_err(|missing| format!("{path}: missing symbol {missing} (wrong libifcs version?)"))?;
    // tl_sflow.cpp's test hook (the mock's ipd ioctl stand-in), libifcs' own scope only
    let ipd_hook = sym(&h, "tl_ipd_ioctl").map(ipd::hook_fn);
    // libifcs' own BAR register accessors (exported; the learn thread's PLT targets), for the
    // board's post-init register list: called directly, never through the GOT the guard took
    let regs = match (sym(&h, "node_pci_reg_read32"), sym(&h, "node_pci_reg_write32")) {
        (Some(rd), Some(wr)) => Some(shim::reg_fns(rd, wr)),
        _ => None,
    };
    Ok(Ifcs(Arc::new(Inner {
        t: table,
        path: PathBuf::from(path),
        ipd_hook,
        regs,
    })))
}

/// The guard's install sequence of `ifcs_lib_load` with its three stderr lines.
fn install_guard(shim: Option<&Library>, h: &Library, object: Option<&elf_got::Object>, path: &str, g: &GuardOptions) {
    let init = shim.and_then(|s| sym(s, "tl_ifcs_guard_init"));
    let done = shim.and_then(|s| sym(s, "tl_ifcs_guard_installed"));
    let rd = sym(h, "node_pci_reg_read32");
    let wr = sym(h, "node_pci_reg_write32");
    let (Some(init), Some(done), Some(shim)) = (init, done, shim) else {
        eprintln!("sai-teralynx: no libsai-teralynx-shim.so guard: libifcs' learn-ring reads are NOT guarded");
        return;
    };
    let stats = cstring(&g.stats_path).unwrap_or_default();
    let installed = match (rd, wr) {
        (Some(rd), Some(wr)) => shim::guard_init(init, rd, wr, &stats, g.ring_size, g.malloc_limit) == 0,
        _ => false,
    };
    if !installed {
        eprintln!("sai-teralynx: {path} exports no node_pci_reg_read32/_write32: learn-ring guard not installed");
        shim::guard_installed(done, false);
        return;
    }
    let targets = (
        sym(shim, "tl_ifcs_reg_read32"),
        sym(shim, "tl_ifcs_reg_write32"),
        sym(shim, "tl_ifcs_im_malloc"),
    );
    let (mut rr, mut rw, mut rm) = (0, 0, 0);
    if let (Some(fr), Some(fw), Some(fm)) = targets {
        rr = redirect(object, path, h, "node_pci_reg_read32", fr);
        rw = redirect(object, path, h, "node_pci_reg_write32", fw);
        rm = redirect(object, path, h, "im_malloc", fm);
    }
    shim::guard_installed(done, rr > 0);
    eprintln!(
        "sai-teralynx: learn-ring guard {}: GOT entries node_pci_reg_read32 {rr}, node_pci_reg_write32 {rw}, im_malloc {rm} (ring {}, im_malloc limit {} B, stats {})",
        if rr > 0 {
            "active"
        } else {
            "NOT active (no node_pci_reg_read32 import)"
        },
        g.ring_size,
        g.malloc_limit,
        if g.stats_path.is_empty() {
            "none"
        } else {
            g.stats_path.as_str()
        }
    );
}

/// The path of the library this code is linked into (`libsai.so` in a deployment): where the
/// loader looks for the shim.
pub fn own_object() -> Option<PathBuf> {
    elf_got::object_of(core::ptr::addr_of!(ANCHOR).addr()).map(|(p, _)| p)
}

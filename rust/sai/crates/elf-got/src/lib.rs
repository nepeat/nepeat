//! GOT redirect: point the imports of one function by an already loaded ELF shared object at
//! another function, by rewriting its `JUMP_SLOT` / `GLOB_DAT` GOT entries. The Teralynx
//! loader's `redirect_cb` (`src/ifcs_lib.cpp` of the C++ sai-teralynx, deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0) in Rust: libifcs'
//! `memcpy` goes to the shim's Device-memory-safe copy, its `node_pci_reg_read32` / `_write32` /
//! `im_malloc` to the learn-ring guard (README "Teralynx backend").
//!
//! # How
//!
//! 1. **Which object, where**: `dladdr` of an address inside it (any symbol `dlsym` gave) names its
//!    file and the start of its mapping; that file must be the path the caller loaded. The load
//!    bias is the mapping start less the first `PT_LOAD`'s page-aligned virtual address (glibc's
//!    `l_map_start - mapstart`).
//! 2. **Which slots**: the ELF is read **from the file** in safe code ([`elf`]): `PT_DYNAMIC`,
//!    `DT_JMPREL` + `DT_RELA`, `DT_SYMTAB` / `DT_STRTAB`; every relocation of the symbol of type
//!    `R_X86_64_JUMP_SLOT` / `_GLOB_DAT` or `R_AARCH64_JUMP_SLOT` / `_GLOB_DAT` (the running
//!    machine's) is a slot at bias + `r_offset`. Why the file and not the in-memory dynamic section
//!    (`dl_iterate_phdr`, as the C++ does): every read of loaded memory is an unsafe dereference of a
//!    pointer the dynamic linker computed; the file is bytes, read with bounds checks, so the
//!    parser has no unsafe at all and a malformed input is an error. Only the slot itself is read
//!    from memory.
//! 3. **Check before writing** (the C++ writes unconditionally): each slot must lie inside a
//!    mapping of this process (`/proc/self/maps`, [`maps`]), be pointer-aligned, and **hold an
//!    expected address**: one of `expected` (the caller passes what `dlsym` answers for the symbol
//!    in the global scope and in the object's own scope: where an `RTLD_NOW` object's slot was
//!    bound) or `target` already (a second redirect, e.g. a second `create_switch` in one
//!    process: counted as `already`, not written again). A slot holding anything else is not
//!    touched and is reported ([`Redirected::refused`]).
//! 4. **The rim** ([`rim`], 6 unsafe blocks): `dladdr`, reading its name, `sysconf`, the volatile
//!    slot read and write, `mprotect`. A read-only page (RELRO) is made writable for the write and
//!    given its protection back afterwards (the C++ leaves it writable).
//!
//! Only for 64-bit little-endian ELF on `x86_64` and `aarch64` Linux (the Teralynx SAI's
//! targets); elsewhere the crate is empty.
#![deny(unsafe_code)]
#![cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]

pub mod elf;
pub mod maps;
mod rim;

use core::fmt;
use std::path::{Path, PathBuf};

pub use elf::{Elf, ElfError, ImportSlot, Machine, SlotKind};

/// What a redirect did with each slot of the symbol.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Redirected {
    /// Slots rewritten to the target now.
    pub written: usize,
    /// Slots that held the target already (an earlier redirect in this process).
    pub already: usize,
    /// Slots left alone because they held neither an expected address nor the target:
    /// `(slot address, what it held)`.
    pub refused: Vec<(usize, usize)>,
}

impl Redirected {
    /// The slots that point at the target after the call (written now or before): the number the
    /// C++ `redirect_cb` counts as "changed" (it rewrites a slot that already pointed there).
    pub const fn changed(&self) -> usize {
        self.written + self.already
    }
}

/// Why a redirect could not be done (no slot was written).
#[derive(Debug)]
pub enum Error {
    /// The anchor address is in no loaded object.
    NotLoaded,
    /// The anchor address belongs to another file than the one named.
    OtherObject(PathBuf),
    /// The object's file could not be read.
    Read(std::io::Error),
    /// The file is not an ELF object this crate reads.
    Elf(ElfError),
    /// `/proc/self/maps` could not be read, or a slot is not inside a mapping.
    Unmapped(usize),
    /// A slot address is not pointer-aligned.
    Misaligned(usize),
    /// `mprotect` refused.
    Protect(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLoaded => f.write_str("the address is in no loaded object"),
            Self::OtherObject(p) => write!(f, "the address belongs to {}", p.display()),
            Self::Read(e) => write!(f, "cannot read the object's file: {e}"),
            Self::Elf(e) => write!(f, "{e}"),
            Self::Unmapped(a) => write!(f, "GOT slot {a:#x} is not mapped"),
            Self::Misaligned(a) => write!(f, "GOT slot {a:#x} is not aligned"),
            Self::Protect(e) => write!(f, "mprotect: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// The loaded object an address belongs to: its file name (as it was given to `dlopen`) and the
/// start of its mapping (`dladdr`'s `dli_fname` / `dli_fbase`). The IFCS loader finds the
/// directory of `libsai.so` with it (the shim sits next to it).
pub fn object_of(addr: usize) -> Option<(PathBuf, usize)> {
    rim::object_of(addr)
}

/// A loaded shared object, read from its file: the redirects of several symbols share one read.
#[derive(Debug)]
pub struct Object {
    path: PathBuf,
    bias: usize,
    data: Vec<u8>,
}

impl Object {
    /// The loaded object `path` (as it was given to `dlopen`), found through `anchor`: the address
    /// of anything inside it (a `dlsym` answer for one of its own symbols).
    pub fn open(path: &Path, anchor: usize) -> Result<Self, Error> {
        let (name, map_start) = rim::object_of(anchor).ok_or(Error::NotLoaded)?;
        if name != path {
            return Err(Error::OtherObject(name));
        }
        let data = std::fs::read(path).map_err(Error::Read)?;
        let first = Elf::parse(&data, Machine::NATIVE)
            .map_err(Error::Elf)?
            .first_load_vaddr();
        let page = rim::page_size();
        let link_start =
            usize::try_from(first).map_err(|_| Error::Elf(ElfError::Malformed("load address")))? & !(page - 1);
        let bias = map_start
            .checked_sub(link_start)
            .ok_or(Error::Elf(ElfError::Malformed("load address")))?;
        Ok(Self {
            path: path.to_owned(),
            bias,
            data,
        })
    }

    /// The object's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The load bias (`l_addr`): a link-time address plus this is where it is in memory.
    pub const fn bias(&self) -> usize {
        self.bias
    }

    /// The GOT slots of `symbol` (link-time addresses and kinds).
    pub fn slots(&self, symbol: &str) -> Result<Vec<ImportSlot>, Error> {
        Elf::parse(&self.data, Machine::NATIVE)
            .and_then(|e| e.import_slots(symbol))
            .map_err(Error::Elf)
    }

    /// Points every GOT slot of `symbol` that holds one of `expected` at `target` (the module's
    /// steps 2 to 4). An error before any slot was written leaves every slot as it was; an error
    /// in the middle (`mprotect` refusing) leaves the slots before it rewritten.
    pub fn redirect(&self, symbol: &str, target: usize, expected: &[usize]) -> Result<Redirected, Error> {
        let slots = self.slots(symbol)?;
        let mut out = Redirected::default();
        if slots.is_empty() {
            return Ok(out);
        }
        let maps = maps::current().map_err(|_| Error::Unmapped(0))?;
        let page = rim::page_size();
        let word = core::mem::size_of::<usize>();
        for s in slots {
            let addr = usize::try_from(s.vaddr)
                .ok()
                .and_then(|v| v.checked_add(self.bias))
                .ok_or(Error::Unmapped(0))?;
            if addr % word != 0 {
                return Err(Error::Misaligned(addr));
            }
            let mapping = maps::containing(&maps, addr, word)
                .filter(|m| m.read)
                .ok_or(Error::Unmapped(addr))?;
            let slot = rim::Slot::checked(addr);
            let now = slot.read();
            if now == target {
                out.already += 1;
                continue;
            }
            if !expected.contains(&now) {
                out.refused.push((addr, now));
                continue;
            }
            if mapping.write {
                slot.write(target);
            } else {
                let start = addr & !(page - 1);
                rim::protect(start, page, true, true, mapping.exec).map_err(Error::Protect)?;
                slot.write(target);
                rim::protect(start, page, mapping.read, false, mapping.exec).map_err(Error::Protect)?;
            }
            out.written += 1;
        }
        Ok(out)
    }
}

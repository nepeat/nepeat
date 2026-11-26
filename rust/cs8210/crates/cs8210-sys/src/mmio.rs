//! A PCI BAR mapped from sysfs (`/sys/bus/pci/devices/<bdf>/resource0`): 32-bit
//! volatile loads and stores, bounds- and alignment-checked.
//!
//! One access is one aligned 32-bit `LDR`/`STR` (`read_volatile` / `write_volatile` of a
//! `u32`): device registers that only answer full-width accesses need exactly that (the
//! I2C FPGA's vendor driver uses `readl` / `writel`). Which offsets may be touched is
//! the caller's business (the FPGA register model in `cs8210-optics` allow-lists them);
//! this type only refuses what would leave the mapping.
#![allow(unsafe_code)]

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::ptr::NonNull;

/// An access the mapping cannot make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmioError {
    /// Not 4-aligned, or not inside the mapping.
    OutOfRange {
        /// byte offset asked for
        offset: usize,
        /// mapping length
        len: usize,
    },
    /// A store on a read-only mapping.
    ReadOnly,
}

impl fmt::Display for MmioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { offset, len } => {
                write!(
                    f,
                    "BAR offset {offset:#x} misaligned or outside the {len:#x}-byte mapping"
                )
            }
            Self::ReadOnly => f.write_str("store on a read-only BAR mapping"),
        }
    }
}

impl std::error::Error for MmioError {}

/// A shared mapping of the first `len` bytes of a BAR resource file.
///
/// Not `Send` / `Sync` (it holds a raw pointer): one thread owns the device.
pub struct Mmio {
    base: NonNull<u8>,
    len: usize,
    writable: bool,
    _file: File,
}

impl fmt::Debug for Mmio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mmio")
            .field("len", &self.len)
            .field("writable", &self.writable)
            .finish_non_exhaustive()
    }
}

impl Mmio {
    /// Map `len` bytes (a multiple of the 4 KiB page) of `path` from offset 0, shared,
    /// `O_SYNC` (uncached: sysfs maps PCI resources as device memory anyway).
    pub fn map(path: &Path, len: usize, writable: bool) -> io::Result<Self> {
        if len == 0 || !len.is_multiple_of(4096) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mapping length must be whole pages",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(writable)
            .custom_flags(libc::O_SYNC)
            .open(path)?;
        let prot = libc::PROT_READ | if writable { libc::PROT_WRITE } else { 0 };
        // SAFETY: a new shared mapping at an address the kernel picks (null hint, no
        // MAP_FIXED), so it replaces nothing of this process; `file` is open for the call
        // and is kept alive (and the mapping released in Drop) for as long as `Self` lives.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, libc::MAP_SHARED, file.as_raw_fd(), 0) };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let base = NonNull::new(p.cast::<u8>()).ok_or_else(|| io::Error::other("mmap returned null"))?;
        Ok(Self {
            base,
            len,
            writable,
            _file: file,
        })
    }

    /// Mapped length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Never true (a mapping has at least one page); for clippy's `len_without_is_empty`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn word(&self, offset: usize) -> Result<*mut u32, MmioError> {
        let inside = offset.checked_add(4).is_some_and(|end| end <= self.len);
        if !offset.is_multiple_of(4) || !inside {
            return Err(MmioError::OutOfRange { offset, len: self.len });
        }
        // The base is page-aligned (mmap) and offset is a multiple of 4 (checked above),
        // so the u32 pointer is aligned although the base pointer's type is u8.
        #[allow(clippy::cast_ptr_alignment)]
        Ok(self.base.as_ptr().wrapping_add(offset).cast::<u32>())
    }

    /// One 32-bit load at `offset`.
    pub fn read32(&self, offset: usize) -> Result<u32, MmioError> {
        let p = self.word(offset)?;
        // SAFETY: `p` lies inside the live mapping (checked by `word`) and is 4-aligned
        // (page-aligned base + a multiple of 4); a volatile load is never elided, merged or
        // split, so the device sees exactly one 32-bit read.
        Ok(unsafe { p.read_volatile() })
    }

    /// One 32-bit store at `offset` (writable mappings only).
    pub fn write32(&self, offset: usize, value: u32) -> Result<(), MmioError> {
        if !self.writable {
            return Err(MmioError::ReadOnly);
        }
        let p = self.word(offset)?;
        // SAFETY: as in `read32`; the mapping was made with PROT_WRITE (checked above) and
        // no Rust reference points into it, so the store aliases nothing the compiler tracks.
        unsafe { p.write_volatile(value) };
        Ok(())
    }
}

impl Drop for Mmio {
    fn drop(&mut self) {
        // SAFETY: `base`/`len` are exactly what mmap returned; nothing can use the mapping
        // after this (the only accessors borrow `self`).
        unsafe { libc::munmap(self.base.as_ptr().cast(), self.len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file_of(words: &[u32]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cs8210-mmio-{}-{}", std::process::id(), words.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("resource0");
        let mut f = File::create(&p).unwrap();
        let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        bytes.resize(4096, 0);
        f.write_all(&bytes).unwrap();
        p
    }

    #[test]
    fn loads_and_stores_words() {
        let p = file_of(&[0x0102_0304, 0xdead_beef]);
        let m = Mmio::map(&p, 4096, true).unwrap();
        assert_eq!(m.read32(4).unwrap(), 0xdead_beef);
        m.write32(8, 0x1234_5678).unwrap();
        assert_eq!(m.read32(8).unwrap(), 0x1234_5678);
        drop(m);
        let back = std::fs::read(&p).unwrap();
        assert_eq!(&back[8..12], &0x1234_5678u32.to_le_bytes());
    }

    #[test]
    fn refuses_outside_and_misaligned() {
        let p = file_of(&[1, 2, 3, 4, 5]);
        let m = Mmio::map(&p, 4096, false).unwrap();
        assert!(matches!(m.read32(2), Err(MmioError::OutOfRange { .. })));
        assert!(matches!(m.read32(4096), Err(MmioError::OutOfRange { .. })));
        assert!(matches!(m.read32(4093), Err(MmioError::OutOfRange { .. })));
        assert!(m.read32(4092).is_ok());
        assert_eq!(m.write32(0, 1), Err(MmioError::ReadOnly));
        assert!(Mmio::map(&p, 100, false).is_err());
    }
}

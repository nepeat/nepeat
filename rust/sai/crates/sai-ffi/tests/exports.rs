//! The symbols `libsai.so` exports: exactly the 17 `sai_*` functions of `src/global.rs`, and
//! with the feature `test-port` also `sai_rs_test_query`. Precisely: the set of FUNC symbols in
//! `.dynsym` that the library defines (section index not `SHN_UNDEF`), binds GLOBAL or WEAK, with
//! default or protected visibility, and whose name starts with `sai`, equals that list. The other
//! exported functions (Rust runtime internals, if any) are listed in the summary, not judged.
//!
//! Reads the ELF with the small reader below (64-bit, either byte order: the test also runs on
//! big-endian ppc64, where the target bundle has no binutils). Finds the library like
//! `tests/c_abi.rs`; without one it skips with a line to `SAI_RS_TEST_SUMMARY`.
// Test code: a failed unwrap, index or panic is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "tests")]
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The exports of `src/global.rs`.
const SAI_EXPORTS: [&str; 17] = [
    "sai_api_initialize",
    "sai_api_uninitialize",
    "sai_api_query",
    "sai_log_set",
    "sai_object_type_query",
    "sai_switch_id_query",
    "sai_query_api_version",
    "sai_dbg_generate_dump",
    "sai_bulk_get_attribute",
    "sai_query_object_stage",
    "sai_object_type_get_availability",
    "sai_get_maximum_attribute_count",
    "sai_get_object_count",
    "sai_get_object_key",
    "sai_query_attribute_capability",
    "sai_query_attribute_enum_values_capability",
    "sai_query_stats_capability",
];

/// The test-only export (`src/test_port.rs`).
const TEST_EXPORT: &str = "sai_rs_test_query";

/// A line for whoever runs the suite: on stderr, and appended to `$SAI_RS_TEST_SUMMARY`.
fn summary(line: &str) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("SAI_RS_TEST_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The cdylib cargo built for this test run (as `tests/c_abi.rs` finds it).
fn libsai() -> Option<PathBuf> {
    if let Some(lib) = std::env::var_os("SAI_FFI_LIBSAI") {
        return Some(PathBuf::from(lib));
    }
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    [Some(deps), deps.parent()]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("libsai.so"))
        .find(|p| p.is_file())
}

/// An ELF64 file of either byte order, read with bounds checks.
struct Elf<'a> {
    data: &'a [u8],
    big: bool,
}

impl<'a> Elf<'a> {
    fn new(data: &'a [u8]) -> Result<Self, String> {
        if data.get(..4) != Some(b"\x7fELF") {
            return Err("not an ELF file".into());
        }
        if data.get(4) != Some(&2) {
            return Err("not a 64-bit ELF file".into());
        }
        let big = match data.get(5) {
            Some(1) => false,
            Some(2) => true,
            other => return Err(format!("bad byte order {other:?}")),
        };
        Ok(Self { data, big })
    }

    fn bytes<const N: usize>(&self, at: u64) -> Result<[u8; N], String> {
        let start = usize::try_from(at).map_err(|e| e.to_string())?;
        self.data
            .get(start..start.checked_add(N).ok_or("offset overflow")?)
            .and_then(|s| <[u8; N]>::try_from(s).ok())
            .ok_or_else(|| format!("read of {N} bytes at {at:#x} past the end"))
    }

    fn u8(&self, at: u64) -> Result<u8, String> {
        Ok(self.bytes::<1>(at)?[0])
    }

    fn u16(&self, at: u64) -> Result<u16, String> {
        let b = self.bytes(at)?;
        Ok(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }

    fn u32(&self, at: u64) -> Result<u32, String> {
        let b = self.bytes(at)?;
        Ok(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }

    fn u64(&self, at: u64) -> Result<u64, String> {
        let b = self.bytes(at)?;
        Ok(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }

    /// A NUL-terminated name at `at`.
    fn name(&self, at: u64) -> Result<String, String> {
        let start = usize::try_from(at).map_err(|e| e.to_string())?;
        let rest = self.data.get(start..).ok_or("name past the end")?;
        let end = rest.iter().position(|b| *b == 0).ok_or("unterminated name")?;
        Ok(String::from_utf8_lossy(rest.get(..end).ok_or("name")?).into_owned())
    }

    /// Section `index`: (type, offset, size, link, entry size).
    fn section(&self, index: u64) -> Result<(u32, u64, u64, u32, u64), String> {
        let shoff = self.u64(0x28)?;
        let shentsize = u64::from(self.u16(0x3a)?);
        let at = shoff + index * shentsize;
        Ok((
            self.u32(at + 4)?,
            self.u64(at + 24)?,
            self.u64(at + 32)?,
            self.u32(at + 40)?,
            self.u64(at + 56)?,
        ))
    }

    /// Every function the file exports: defined, GLOBAL or WEAK, default or protected visibility.
    fn exported_functions(&self) -> Result<BTreeSet<String>, String> {
        const SHT_DYNSYM: u32 = 11;
        let shnum = u64::from(self.u16(0x3c)?);
        let dynsym = (0..shnum)
            .map(|i| self.section(i))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|s| s.0 == SHT_DYNSYM)
            .ok_or("no .dynsym")?;
        let (_, offset, size, link, entsize) = dynsym;
        if entsize != 24 {
            return Err(format!("symbol size {entsize}"));
        }
        let (_, strtab, ..) = self.section(u64::from(link))?;
        let mut names = BTreeSet::new();
        for i in 0..size / entsize {
            let sym = offset + i * entsize;
            let info = self.u8(sym + 4)?;
            let other = self.u8(sym + 5)?;
            let shndx = self.u16(sym + 6)?;
            let (bind, kind, visibility) = (info >> 4, info & 0xf, other & 0x3);
            let defined = shndx != 0;
            let exported = matches!(bind, 1 | 2) && matches!(visibility, 0 | 3);
            if kind == 2 && defined && exported {
                names.insert(self.name(strtab + u64::from(self.u32(sym)?))?);
            }
        }
        Ok(names)
    }
}

#[test]
fn the_library_exports_exactly_the_sai_functions() {
    let Some(lib) = libsai() else {
        summary("sai-ffi exports: SKIPPED, libsai.so not found (set SAI_FFI_LIBSAI)");
        return;
    };
    let data = std::fs::read(&lib).unwrap_or_else(|e| panic!("{}: {e}", lib.display()));
    let exported = Elf::new(&data).and_then(|elf| elf.exported_functions()).unwrap();
    let sai: BTreeSet<&str> = exported
        .iter()
        .map(String::as_str)
        .filter(|n| n.starts_with("sai"))
        .collect();
    let mut expected: BTreeSet<&str> = SAI_EXPORTS.into_iter().collect();
    if cfg!(feature = "test-port") {
        expected.insert(TEST_EXPORT);
    }
    assert_eq!(sai, expected, "{}", lib.display());
    let others: Vec<&String> = exported.iter().filter(|n| !n.starts_with("sai")).collect();
    summary(&format!(
        "sai-ffi exports: {} sai* functions as expected (test-port {}), {} other exported functions{}{:?}",
        sai.len(),
        if cfg!(feature = "test-port") { "on" } else { "off" },
        others.len(),
        if others.is_empty() { "" } else { ": " },
        others
    ));
}

#[test]
fn the_reader_reads_both_byte_orders() {
    // A minimal ELF64 header in both orders: magic, class, data; shnum at 0x3c.
    for (order, big) in [(1u8, false), (2u8, true)] {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2;
        data[5] = order;
        let shnum: u16 = 0x1234;
        data[0x3c..0x3e].copy_from_slice(&if big { shnum.to_be_bytes() } else { shnum.to_le_bytes() });
        let elf = Elf::new(&data).unwrap();
        assert_eq!(elf.u16(0x3c).unwrap(), 0x1234);
        assert!(elf.u64(60).is_err(), "reads stop at the end");
        assert_eq!(elf.exported_functions().unwrap_err(), "no .dynsym");
    }
    assert!(Elf::new(b"\x7fELF\x01\x01").is_err());
    assert!(Elf::new(b"MZ").is_err());
}

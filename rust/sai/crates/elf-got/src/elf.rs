//! A reader of what the GOT redirect needs from a 64-bit little-endian ELF shared object: its
//! program headers, the dynamic section, the relocation tables `DT_JMPREL` / `DT_RELA` and the
//! dynamic symbol and string tables. Bounds-checked reads of a byte slice: no `unsafe`, any
//! malformed or truncated input is an [`ElfError`], never a panic.
#![forbid(unsafe_code)]

use core::fmt;

/// The machine a relocation table is read for (each has its own relocation type numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// `EM_X86_64`: `R_X86_64_GLOB_DAT` 6, `R_X86_64_JUMP_SLOT` 7.
    X86_64,
    /// `EM_AARCH64`: `R_AARCH64_GLOB_DAT` 1025, `R_AARCH64_JUMP_SLOT` 1026.
    Aarch64,
}

impl Machine {
    /// The machine this program runs on.
    #[cfg(target_arch = "x86_64")]
    pub const NATIVE: Self = Self::X86_64;
    /// The machine this program runs on.
    #[cfg(target_arch = "aarch64")]
    pub const NATIVE: Self = Self::Aarch64;

    /// `e_machine`.
    pub const fn e_machine(self) -> u16 {
        match self {
            Self::X86_64 => 62,
            Self::Aarch64 => 183,
        }
    }

    /// The relocation type of a GOT slot an import is bound through, if `r_type` is one.
    pub const fn slot_kind(self, r_type: u32) -> Option<SlotKind> {
        match (self, r_type) {
            (Self::X86_64, 7) | (Self::Aarch64, 1026) => Some(SlotKind::JumpSlot),
            (Self::X86_64, 6) | (Self::Aarch64, 1025) => Some(SlotKind::GlobDat),
            _ => None,
        }
    }
}

/// How a GOT slot is bound (both hold the symbol's address once the object is relocated with
/// `RTLD_NOW`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A PLT slot (`R_*_JUMP_SLOT`, in `DT_JMPREL`): the calls through the PLT.
    JumpSlot,
    /// An address load (`R_*_GLOB_DAT`, in `DT_RELA`): `&symbol` and `-fno-plt` calls.
    GlobDat,
}

/// A GOT slot of an import: where it is, as an address of the object's link-time layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportSlot {
    /// `r_offset`: the slot's link-time virtual address (add the load bias).
    pub vaddr: u64,
    /// Its relocation type.
    pub kind: SlotKind,
}

/// Why an object could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElfError {
    /// Not an ELF file, or not 64-bit little-endian.
    NotElf64Le,
    /// An ELF file for another machine (`e_machine`).
    WrongMachine(u16),
    /// A table or entry outside the file, or an entry size this reader does not know.
    Malformed(&'static str),
    /// No `PT_DYNAMIC`, or no symbol / string table in it.
    NoDynamic,
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotElf64Le => f.write_str("not a 64-bit little-endian ELF file"),
            Self::WrongMachine(m) => write!(f, "ELF machine {m} is not this program's"),
            Self::Malformed(what) => write!(f, "malformed ELF: {what}"),
            Self::NoDynamic => f.write_str("no dynamic section with symbol and string tables"),
        }
    }
}

impl std::error::Error for ElfError {}

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: u64 = 0;
const DT_PLTRELSZ: u64 = 2;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_RELAENT: u64 = 9;
const DT_STRSZ: u64 = 10;
const DT_SYMENT: u64 = 11;
const DT_PLTREL: u64 = 20;
const DT_JMPREL: u64 = 23;
const RELA_SIZE: u64 = 24;
const SYM_SIZE: u64 = 24;
const DYN_SIZE: u64 = 16;
const PHDR_SIZE: u64 = 56;

fn bytes<'a>(data: &'a [u8], off: u64, len: u64, what: &'static str) -> Result<&'a [u8], ElfError> {
    let start = usize::try_from(off).map_err(|_| ElfError::Malformed(what))?;
    let len = usize::try_from(len).map_err(|_| ElfError::Malformed(what))?;
    let end = start.checked_add(len).ok_or(ElfError::Malformed(what))?;
    data.get(start..end).ok_or(ElfError::Malformed(what))
}

fn u16_at(data: &[u8], off: u64) -> Result<u16, ElfError> {
    let b = bytes(data, off, 2, "u16 field")?;
    Ok(u16::from_le_bytes(
        b.try_into().map_err(|_| ElfError::Malformed("u16 field"))?,
    ))
}

fn u32_at(data: &[u8], off: u64) -> Result<u32, ElfError> {
    let b = bytes(data, off, 4, "u32 field")?;
    Ok(u32::from_le_bytes(
        b.try_into().map_err(|_| ElfError::Malformed("u32 field"))?,
    ))
}

fn u64_at(data: &[u8], off: u64) -> Result<u64, ElfError> {
    let b = bytes(data, off, 8, "u64 field")?;
    Ok(u64::from_le_bytes(
        b.try_into().map_err(|_| ElfError::Malformed("u64 field"))?,
    ))
}

/// A `PT_LOAD` segment: file bytes `[offset, offset + filesz)` at `[vaddr, vaddr + memsz)`.
#[derive(Clone, Copy, Debug)]
struct Load {
    offset: u64,
    vaddr: u64,
    filesz: u64,
}

/// A relocation table (`DT_JMPREL` + `DT_PLTRELSZ`, `DT_RELA` + `DT_RELASZ`), as file offsets.
#[derive(Clone, Copy, Debug)]
struct Table {
    offset: u64,
    size: u64,
}

/// The parts of a shared object the redirect reads, over its file's bytes.
#[derive(Debug)]
pub struct Elf<'a> {
    data: &'a [u8],
    machine: Machine,
    first_load_vaddr: u64,
    symtab: u64,
    strtab: u64,
    strsz: u64,
    tables: Vec<Table>,
}

impl<'a> Elf<'a> {
    /// Reads the headers and the dynamic section of `data` (a whole ELF file) for `machine`.
    pub fn parse(data: &'a [u8], machine: Machine) -> Result<Self, ElfError> {
        if data.get(0..4) != Some(b"\x7fELF".as_slice()) || data.get(4) != Some(&2) || data.get(5) != Some(&1) {
            return Err(ElfError::NotElf64Le);
        }
        let e_machine = u16_at(data, 0x12)?;
        if e_machine != machine.e_machine() {
            return Err(ElfError::WrongMachine(e_machine));
        }
        let phoff = u64_at(data, 0x20)?;
        let phentsize = u64::from(u16_at(data, 0x36)?);
        let phnum = u64::from(u16_at(data, 0x38)?);
        if phentsize != PHDR_SIZE {
            return Err(ElfError::Malformed("program header size"));
        }
        let mut loads = Vec::new();
        let mut dynamic = None;
        for i in 0..phnum {
            let ph = phoff
                .checked_add(i * PHDR_SIZE)
                .ok_or(ElfError::Malformed("program header"))?;
            let p_type = u32_at(data, ph)?;
            let (offset, vaddr, filesz) = (u64_at(data, ph + 8)?, u64_at(data, ph + 16)?, u64_at(data, ph + 32)?);
            match p_type {
                PT_LOAD => loads.push(Load { offset, vaddr, filesz }),
                PT_DYNAMIC => dynamic = Some((offset, filesz)),
                _ => {}
            }
        }
        let first_load_vaddr = loads
            .iter()
            .map(|l| l.vaddr)
            .min()
            .ok_or(ElfError::Malformed("no PT_LOAD"))?;
        let (dyn_off, dyn_size) = dynamic.ok_or(ElfError::NoDynamic)?;
        let to_offset = |vaddr: u64| -> Result<u64, ElfError> {
            loads
                .iter()
                .find(|l| vaddr >= l.vaddr && vaddr - l.vaddr < l.filesz)
                .map(|l| l.offset + (vaddr - l.vaddr))
                .ok_or(ElfError::Malformed("address outside the file's segments"))
        };
        let (mut symtab, mut strtab, mut strsz) = (None, None, None);
        let (mut jmprel, mut pltrelsz, mut rela, mut relasz) = (None, 0, None, 0);
        let (mut relaent, mut syment, mut pltrel) = (RELA_SIZE, SYM_SIZE, DT_RELA);
        for i in 0..dyn_size / DYN_SIZE {
            let d = dyn_off + i * DYN_SIZE;
            let (tag, val) = (u64_at(data, d)?, u64_at(data, d + 8)?);
            match tag {
                DT_NULL => break,
                DT_SYMTAB => symtab = Some(val),
                DT_STRTAB => strtab = Some(val),
                DT_STRSZ => strsz = Some(val),
                DT_JMPREL => jmprel = Some(val),
                DT_PLTRELSZ => pltrelsz = val,
                DT_RELA => rela = Some(val),
                DT_RELASZ => relasz = val,
                DT_RELAENT => relaent = val,
                DT_SYMENT => syment = val,
                DT_PLTREL => pltrel = val,
                _ => {}
            }
        }
        if relaent != RELA_SIZE || syment != SYM_SIZE {
            return Err(ElfError::Malformed("relocation or symbol entry size"));
        }
        if jmprel.is_some() && pltrel != DT_RELA {
            return Err(ElfError::Malformed("DT_PLTREL is not DT_RELA"));
        }
        let (Some(symtab), Some(strtab), Some(strsz)) = (symtab, strtab, strsz) else {
            return Err(ElfError::NoDynamic);
        };
        let mut tables = Vec::new();
        for (vaddr, size) in [(jmprel, pltrelsz), (rela, relasz)] {
            if let Some(vaddr) = vaddr.filter(|_| size > 0) {
                let offset = to_offset(vaddr)?;
                bytes(data, offset, size, "relocation table")?;
                tables.push(Table { offset, size });
            }
        }
        let strtab = to_offset(strtab)?;
        bytes(data, strtab, strsz, "string table")?;
        Ok(Self {
            data,
            machine,
            first_load_vaddr,
            symtab: to_offset(symtab)?,
            strtab,
            strsz,
            tables,
        })
    }

    /// The lowest `PT_LOAD` virtual address: with the mapping's start it gives the load bias.
    pub const fn first_load_vaddr(&self) -> u64 {
        self.first_load_vaddr
    }

    fn symbol_name(&self, index: u64) -> Result<&'a [u8], ElfError> {
        let st_name = u64::from(u32_at(self.data, self.symtab + index * SYM_SIZE)?);
        if st_name >= self.strsz {
            return Err(ElfError::Malformed("symbol name outside the string table"));
        }
        let rest = bytes(self.data, self.strtab + st_name, self.strsz - st_name, "symbol name")?;
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(ElfError::Malformed("unterminated symbol name"))?;
        rest.get(..end).ok_or(ElfError::Malformed("symbol name"))
    }

    /// Every `JUMP_SLOT` / `GLOB_DAT` relocation of `symbol` in `DT_JMPREL` and `DT_RELA`, in
    /// table order (the slots the dynamic linker bound to the symbol's address).
    pub fn import_slots(&self, symbol: &str) -> Result<Vec<ImportSlot>, ElfError> {
        let mut out = Vec::new();
        for t in &self.tables {
            for k in 0..t.size / RELA_SIZE {
                let e = t.offset + k * RELA_SIZE;
                let (r_offset, r_info) = (u64_at(self.data, e)?, u64_at(self.data, e + 8)?);
                // ELF64_R_TYPE / ELF64_R_SYM; the type is the low 32 bits (truncation intended)
                #[allow(clippy::cast_possible_truncation, reason = "ELF64_R_TYPE is the low 32 bits")]
                let Some(kind) = self.machine.slot_kind(r_info as u32) else {
                    continue;
                };
                let sym = r_info >> 32;
                if sym != 0 && self.symbol_name(sym)? == symbol.as_bytes() {
                    out.push(ImportSlot { vaddr: r_offset, kind });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal shared object as the linker lays one out: header, 2 program headers (one
    /// `PT_LOAD` at vaddr 0 covering the file, `PT_DYNAMIC`), dynamic section, symbol and string
    /// tables, a `JMPREL` table (2 entries) and a `RELA` table (2 entries).
    fn synthetic(machine: Machine, jump: u32, glob: u32) -> Vec<u8> {
        let mut f = vec![0u8; 0x400];
        let put = |f: &mut Vec<u8>, off: usize, b: &[u8]| f[off..off + b.len()].copy_from_slice(b);
        put(&mut f, 0, b"\x7fELF\x02\x01\x01");
        put(&mut f, 0x12, &machine.e_machine().to_le_bytes());
        put(&mut f, 0x20, &0x40u64.to_le_bytes());
        put(&mut f, 0x36, &56u16.to_le_bytes());
        put(&mut f, 0x38, &2u16.to_le_bytes());
        // PT_LOAD: offset 0, vaddr 0, filesz 0x400
        put(&mut f, 0x40, &1u32.to_le_bytes());
        put(&mut f, 0x40 + 32, &0x400u64.to_le_bytes());
        // PT_DYNAMIC at 0x100, 12 entries
        put(&mut f, 0x78, &2u32.to_le_bytes());
        put(&mut f, 0x78 + 8, &0x100u64.to_le_bytes());
        put(&mut f, 0x78 + 16, &0x100u64.to_le_bytes());
        put(&mut f, 0x78 + 32, &(12u64 * 16).to_le_bytes());
        let dynamic = [
            (DT_SYMTAB, 0x200u64),
            (DT_STRTAB, 0x2a0),
            (DT_STRSZ, 0x20),
            (DT_SYMENT, 24),
            (DT_JMPREL, 0x300),
            (DT_PLTRELSZ, 48),
            (DT_PLTREL, DT_RELA),
            (DT_RELA, 0x340),
            (DT_RELASZ, 48),
            (DT_RELAENT, 24),
            (DT_NULL, 0),
        ];
        for (i, (tag, val)) in dynamic.iter().enumerate() {
            put(&mut f, 0x100 + i * 16, &tag.to_le_bytes());
            put(&mut f, 0x108 + i * 16, &val.to_le_bytes());
        }
        // symbols: 0 null, 1 "memcpy" (strtab 1), 2 "other" (strtab 8)
        put(&mut f, 0x200 + 24, &1u32.to_le_bytes());
        put(&mut f, 0x200 + 48, &8u32.to_le_bytes());
        put(&mut f, 0x2a0, b"\0memcpy\0other\0");
        let rela = |f: &mut Vec<u8>, at: usize, off: u64, sym: u64, ty: u32| {
            put(f, at, &off.to_le_bytes());
            put(f, at + 8, &((sym << 32) | u64::from(ty)).to_le_bytes());
        };
        rela(&mut f, 0x300, 0x3f0, 1, jump);
        rela(&mut f, 0x318, 0x3f8, 2, jump);
        rela(&mut f, 0x340, 0x3e8, 1, glob);
        rela(&mut f, 0x358, 0x3e0, 1, 8); // R_*_RELATIVE-like: not a slot
        f
    }

    #[test]
    fn finds_both_slot_kinds_per_machine() {
        for (m, j, g) in [(Machine::X86_64, 7, 6), (Machine::Aarch64, 1026, 1025)] {
            let f = synthetic(m, j, g);
            let elf = Elf::parse(&f, m).unwrap();
            assert_eq!(elf.first_load_vaddr(), 0);
            assert_eq!(
                elf.import_slots("memcpy").unwrap(),
                vec![
                    ImportSlot {
                        vaddr: 0x3f0,
                        kind: SlotKind::JumpSlot
                    },
                    ImportSlot {
                        vaddr: 0x3e8,
                        kind: SlotKind::GlobDat
                    },
                ]
            );
            assert_eq!(elf.import_slots("other").unwrap().len(), 1);
            assert_eq!(elf.import_slots("missing").unwrap(), vec![]);
        }
    }

    #[test]
    fn relocation_types_are_the_machines() {
        // the x86_64 numbers read for aarch64 are no slots, and the other way round
        let f = synthetic(Machine::Aarch64, 7, 6);
        assert_eq!(
            Elf::parse(&f, Machine::Aarch64)
                .unwrap()
                .import_slots("memcpy")
                .unwrap(),
            vec![]
        );
        assert_eq!(Machine::X86_64.slot_kind(1026), None);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let f = synthetic(Machine::X86_64, 7, 6);
        assert_eq!(
            Elf::parse(&f, Machine::Aarch64).unwrap_err(),
            ElfError::WrongMachine(62)
        );
        assert_eq!(
            Elf::parse(b"\x7fELF\x01\x01", Machine::X86_64).unwrap_err(),
            ElfError::NotElf64Le
        );
        assert_eq!(Elf::parse(b"", Machine::X86_64).unwrap_err(), ElfError::NotElf64Le);
        // every truncation of a valid file is an error, never a panic
        for len in 0..f.len() {
            let cut = f.get(..len).unwrap();
            let r = Elf::parse(cut, Machine::X86_64).and_then(|e| e.import_slots("memcpy"));
            assert!(r.is_err() || len >= 0x370, "length {len}");
        }
        // a symbol name pointing outside the string table
        let mut bad = f.clone();
        bad[0x200 + 24..0x200 + 28].copy_from_slice(&0x1000u32.to_le_bytes());
        assert!(
            Elf::parse(&bad, Machine::X86_64)
                .unwrap()
                .import_slots("memcpy")
                .is_err()
        );
    }
}

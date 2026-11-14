# TN48M AP806 BootROM dump (2026-11-12)

Read-only dump of the Armada 7040 (AP806-B0) BootROM from running NixOS.
Nothing was written anywhere. Binary: `dumps/ap806-bootrom.bin` (gitignored,
Marvell's code: keep it out of the public repo).

## where it lives

TF-A says AP "IO window 0" is reserved for the BootROM
(`drivers/marvell/io_win.c`: "index 0 dedicated for BootROM"). Live registers:

```
IO_WIN (RFU) 0xF06F0000: win0 ALR = 0x0000fff1  -> enabled, base 0xFFF00000
CCU          0xF0004000: win1 0xF2000000-0xFFFFFFFF -> IO target (leads to IO_WIN)
```

So the ROM is at **0xFFF00000-0xFFFFFFFF** (1 MiB window), and it is still
readable from the non-secure world (Linux at EL2, `/dev/mem`) after boot.

## how it was dumped

`tools/devmem.py` (mmap of `/dev/mem`, 32-bit reads; the stock NixOS kernel has
STRICT_DEVMEM, which still allows non-RAM addresses), run with the python3
already in the box's store:

```
python3 devmem.py words 0xf06f0000 36          # window registers
python3 devmem.py dump  0xfff00000 0x100000 rom.bin
```

Two full reads were identical. The window holds a **64 KiB image mirrored 16
times**; `dumps/ap806-bootrom.bin` is the first 64 KiB.

```
sha256 ap806-bootrom.bin (64 KiB)      c79632ea306ff2896a620d39e66ba1dea1132f0c474bd4166162f33dde1ca787
sha256 whole 1 MiB window (16 mirrors) 1f1ba65845cc52be3694ecef9743f6568d688a74d5a5ed3a88cfdf9f657ce5c8
```

## first look (not audited yet)

- AArch64. `0x0: b 0x400` + NOPs; `0x400: b 0x40c`, then words `0x00000203`
  (= the "BootROM - 2.03" banner version) and `0x00000624`; `0x40c` sets
  `VBAR_EL3`/`CPTR_EL3` and starts init.
- Layout: code/data 0x0-0x878f, zeros to 0xb000, more to 0xe518, zeros to
  0xfffc, last word `0x0280d8ab` (checksum or signature word, unverified).
- Strings match the boot log and the efuse findings: `BootROM - `,
  `Starting AP IOROM `, `Secure boot from `, and the secure-boot error set
  (`Error reading KAK key / CSK index / BOX ID / FLASH ID`, `Wrong KAK key`,
  `Bad CSK array signature`, `Bad header signature`, `Wrong Flash ID`,
  `Wrong Box ID`, AES image-decrypt errors, `MCL failed to verify image
  signature`). Boot sources: SPI NOR/NAND, device-bus NOR, NFC NAND, SD/eMMC,
  UART.
- **There is a debug monitor in the ROM**: prompt `bootrom>> ` with
  `r <addr>`, `w <addr> <val>`, `j <addr>`, `t` (try UART boot), `h/?`.
  Whether it can be reached at all on a secure-boot-fused part is the first
  audit question. Not tried.
- `Error: Failed boot attempt ... Trying Uart...` shows a UART fallback path
  after failed boots; how it behaves under secure boot is also unaudited.
- The CP110 has its own ROM ("Starting CP-0 IOROM 1.07" in the boot log). Not
  located or dumped.

## structure found so far (static, host only)

Helpers: `tools/rom_dis.py` (annotated disassembly), `tools/rom_consts.py`
(constants built with mov/movk). Offsets are file offsets (add 0xFFFF0000).

- The image has two parts: the main ROM (0x0-0x878f) and an **"AP IOROM"
  blob at 0xb000**: header `d010b002` magic, size 0x4fec, code offset 0x10,
  32-bit additive checksum, then code. It holds the boot-device drivers
  (SPI, SD/eMMC) and the "Starting AP IOROM" / "Secure boot from" strings.
- The loader at 0x179c takes the IOROM header **pointer from register
  0xF06F4720**, checks the magic, copies the code to SRAM **0xFFE0C000**,
  sums it against the header checksum, and calls it there with a command
  number in w0 (0-3) and a context at 0xFFE08000. ROM state lives in SRAM at
  0xFFE04000; another SRAM area is 0xFFE1C000.
- The IOROM's 0xd548 programs IO window 1 with target 4; the caller at 0x191c
  passes base 0xF8000000, size 16 MiB (looks like a memory-mapped boot flash
  window; not confirmed).
- Other registers the ROM reads: 0xF06F4724, 0xF06F4730, 0xF06F4734,
  0xF06F00A0/A4, efuse control 0xF06F8008.

## CP110 IOROM: not located

"Starting CP-0 IOROM 1.07" is not a string in the AP ROM, so the CP110 has
its own blob, presumably with the same `d010b002` header. Where it is mapped
is unknown. Not dumped.

## gotcha: a /dev/mem read hung the SoC

2026-11-12: reading 0xF06F4700 (16 words) from NixOS through `devmem.py` hung
the whole box: no panic, no serial, no ping, and the watchdog did not fire.
Recovered with a lab-plug power cycle; ext4 came back clean. The earlier reads
(0xF06F0000, 0xF0004000, 0xFFF00000) were fine. **Do not read the
0xF06F47xx block from Linux.**

## next

Disassemble (Ghidra/objdump, base 0xFFFF0000 or 0xFFF00000: mirrors) and trace:
header parsing (prolog, extension walk, size/offset checks), the secure-boot
decision (efuse rows 37-39), the `bootrom>>` entry condition, and the UART path.
Static analysis only; anything run on the box stays read-only.

# Boot ROM, UART boot and stage3's console keys

Static analysis only (2026-11-15). Nothing was run on hardware for this file. Sources:
- stage2 A/B and stage3 in the scratch Ghidra project (rebuild as in
  [spi-uboot-replacement.md](spi-uboot-replacement.md)); the factory U-Boot in `ghidra/dmc12.gpr`.
- The Annapurna HAL from the MikroTik GPL drop, already in the repo:
  `devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/` (called `HAL/`
  below). It also holds `pcie/src/al_hal_pcie.c`, the al_pcie HAL that spi-uboot-replacement.md §9 lists as missing.
- Hardware values from `inventory/u01/spi-uboot-regs.txt` and `spi-recovery-queries-private.txt`.

*Inferred* means read from code shape or the HAL, never observed.

## Verdict
- **`u` works by construction, and `p` already ran the same code.** `p`, `e` and `u` all end
  in one "force app 5" tail. That tail prints only when the BOOT_MODE app is not 5 already, and
  on u01 it is 5, so `u` is silent today. The only untested piece is the compare `(c & 0xef) == 0x65`.
  The `e` key exercises that compare and prints something (§1).
- **The boot ROM is at 0xfbff0000, 16 KiB** (alias 0xfd8a0000). Dump it with the commands in §2.
  **Everything below about the ROM waits for that dump.**
- **UART boot is set by straps only, and the factory stage2s cancel it.**
  - Boot device = `boot_strap` bits 17:15 (PBS 0xfd8a8110, read-only latch). u01 reads 7 = SPI mode 0.
  - Both stage2 builds set `SPI_LOAD_ENFORCE` for every boot device except NAND. That turns off the
    "UART recovery!" path in stage3 and in U-Boot.
  - No software flag survives stage2. So today there is **no zero-write route** from a reset to
    "ROM → stage2 → stage3 → our app over UART". The two candidates in §4 both depend on what the ROM does.
- No `uartboot.py` yet: the ROM protocol is only inferred (XMODEM), and the tool would be a guess.

## 1. Stage3 console keys (main 0x010285a8)
- **When stage3 polls.** One non-blocking poll of UART0 (`LSR & 1`, 0x01003f08) at 0x0102888c.
  - It comes after the BOOT_MODE/DT/APCEA/SERDES_25G_FW lookups and before the app lookup.
  - While characters are waiting it reads them one at a time (0x01003e78) and polls again, so a
    burst queued in the FIFO is drained.
  - The window is the time from stage3's UART init to that poll. On u01, keys at 50 ms from the
    banner reached it.
- **Keys** (all other bytes are dropped):

  | key | effect | message (log level 4) |
  | --- | --- | --- |
  | `p` (0x70) | skip APCEA (`0x0105a3e8 = 1`), then the force-U-Boot tail | "User request for skipping APCEA!" (once) |
  | `e` (0x65) | keep early APCEA (`0x0105a48c = 1`: no APCEA load, no "Stopping early APCEA"), then the tail | "User request for keeping early APCEA!" (once) |
  | `u` (0x75) | the tail only | none of its own |
  | tail (0x01028964) | if app (`0x0105d268`) ≠ 5: print, then app = 5, poll again | "User request for loading U-Boot!" |

- **After the keys.** The app lookup is `find_id_with_fallback(app | inst<<28)`. A missing ID prints
  "App not found in TOC" and falls back to 5.
- **No side effect tells `u` from no key while app = 5.** The app variable is 5 either way.
- **What can be checked now, with no writes:**
  - Send `e` and expect its message: that proves the `(c & 0xef) == 0x65` compare, the same one `u` takes.
  - Read stage3's state back from the factory prompt (§5). Stage3's .bss at 0x0105xxxx sits below
    U-Boot's load address, and *inferred* it survives.
  - `e` changes nothing on u01: CS0 has no APCEA object, and no log shows an early APCEA.
- **Smallest test of the app ≠ 5 branch (one 4 KiB SPI write; owner decision):**
  - Write a wrapped BOOT_MODE object at 0x340000 with app `0x10000005` (UBOOT instance 1, which is
    byte-identical to instance 0).
  - With no key, stage3 boots UBOOT 1. With `u`, it prints the message and boots UBOOT 0. **Both
    outcomes boot the same factory U-Boot.**
  - Confirm at the prompt: `md.l 0x0105d268 1` reads `10000005` without the key, `00000005` with it.
  - Revert with `sf erase 0x340000 0x1000`.
  - The RAM-image route in §4 would make even that write unnecessary.

## 2. Boot ROM: location and dump
**Evidence.** In `HAL/platform/alpine_v2/include/al_hal_iomap.h`:
- `AL_PBS_INT_MEM_BOOT_ROM_BASE = 0xfbff0000`, and the SRAM follows at
  `AL_PBS_INT_MEM_SRAM_BASE = 0xfbff4000`. That is the SRAM we already read (the stage2 offset
  word, shared data at 0xfbff4180, the agent at 0xfbff4200).
- `AL_PBS_BOOT_ROM_BASE = 0xfd8a0000` is a PBS alias, and its SRAM alias is at 0xfd8a4000.
- `BOOT_ROM_NAND_READ_FUNC_PTR 0xffff0021` and `BOOT_ROM_XMODEM_RECIEVE_FUNC_PTR 0xffff12f9`
  (Thumb): the CPU also sees the ROM at the high vectors, 0xffff0000.
- **Size: 16 KiB.** *Inferred* from the SRAM starting at +0x4000, and both function pointers fall below it.

No stage2, stage3 or agent code calls into the ROM (no references to 0xfbff0000–0xffffffff
other than the SRAM). So the ROM's flow has to come from the dump.

**Commands** (factory `ALPINE_DB>`; every one only reads). Use `./console.py --timeout 60 --out <file> '<cmd>'`.
The tmux history limit is 2000 lines and `md.l … 0x400` prints 256 lines, so dump in 0x400-word pieces.
```
md.l 0xfbff0000 0x10      # sanity: ARM vectors (e.g. e59ff0xx / eaxxxxxx), not bad0add1 or all-zero
md.l 0xfd8a0000 0x10      # alias: expect the same words
cmp.l 0xfbff0000 0xfd8a0000 0x1000
crc32 0xfbff0000 0x4000
md.l 0xfbff0000 0x400     # -> inventory/u01/bootrom-md-0.txt
md.l 0xfbff1000 0x400     # -> bootrom-md-1.txt
md.l 0xfbff2000 0x400     # -> bootrom-md-2.txt
md.l 0xfbff3000 0x400     # -> bootrom-md-3.txt
md.l 0xfbff3ff0 8         # boundary: the last ROM words, then SRAM fbff4000 (= 00000000 f2200020 ...)
```
Offline: `./md2bin.py dumps/u01/bootrom.bin inventory/u01/bootrom-md-*.txt` prints a CRC32, which must
match the `crc32` line. Optional, last of all and may abort: `md.l 0xffff0000 0x10` (the
high-vector alias).

The ROM is Annapurna code. Keep `bootrom.bin` out of git, like the other vendor blobs: list it in
`dumps/u01/SHA256SUMS`.

**Risks.**
- An unclaimed PBS address reads `bad0add1` (seen at 0xfd8a8094 and 0xfd800004), not an SError.
- If the ROM is hidden after boot, expect all `bad0add1` or zeros. Report that, and don't retry other ranges.
- A fault at 0xffff0000 is a U-Boot "Synchronous Abort" followed by a reset or hang. Recover with a
  power-cycle. Nothing in the flash is touched.
- OTP "Boot rom debug: enabled" *may* be the thing that keeps it readable.

## 3. UART boot: straps, flags, protocol
**(a) Straps.**
- `dev_info` decodes PBS `boot_strap` (0xfd8a8110). The layout is `HAL/pbs/src/al_hal_bootstrap_map.h`, and
  stage2's own parser (0xf2206f08) matches it.
- u01 reads `0x03bfdef5`. Bits 17:15 = **7**: SPI mode 0, as `dev_info` shows.

| bits 17:15 | boot device | ROM UART baud |
| --- | --- | --- |
| 0 | UART CLI (ROM command line) | (stage3: "boot device isnt supported", uses SPI) |
| 1 | UART | 2 000 000 |
| 2 | NAND 8-bit | |
| 4 | UART | 115 200 |
| 5 | SPI mode 3 | |
| 6 | UART | 1 000 000 |
| 7 | SPI mode 0 (u01) | |

- **Physically:** all three strap bits read 1 today. UART at 115200 means pulling bits 15 and 16 low
  (two strap resistors on the K2 card); UART CLI means all three low.
- Which balls and resistors those are needs the Alpine V2 datasheet or tracing. Neither is in the repo.
- The register is a read-only latch ("Log"). OTP allows UART boot and UART CLI.

**(b) Software trigger.** Shared data is at 0xfbff4180 (`al_general_shared_data`).
- **The stage2 flags.** The `stage2_shared_data` magic is at **0xfbff41c0** and its flags at
  **0xfbff41c1**:
  - bit0 `XMODEM_LOAD_ENFORCE`
  - bit1 `SPI_LOAD_ENFORCE`
  - bit2 `NITRO_MODE`
  - bit7 `UBOOT_USER_BOOT_ABORT`

  Readers require magic > 0x3a. Source: `HAL/services/shared_mem/al_general_shared_data.[ch]`,
  plus stage3 0x010257c0 and stage2 0xf2208034.
- **The decision, made the same way in stage3 (0x01028950) and U-Boot (`board_init`, 0x011023a0):**
  - `SPI_LOAD_ENFORCE` → read the flash by SPI.
  - Otherwise, if the strap says UART or `XMODEM_LOAD_ENFORCE` is set → the "UART recovery" mode:
    - flash offset X is read from **DRAM 0x01800000 + X** (stage3 0x01027fcc, U-Boot 0x01101bb4);
    - stage3 prints "UART recovery!";
    - U-Boot sets `bootcmd=echo recovery boot command!`.
- **Stage2 wipes the flags and sets the SPI one.** Both stage2 builds re-init the shared data on
  every boot (0xf220c19c). Then they call `flag_set(SPI_LOAD_ENFORCE, 1)` whenever the boot device
  isn't NAND (A: 0xf220c230; B: 0xf221135c).
  - So **nothing written before a reset survives into stage3**, whether by `mw`, the uC or a warm
    reset (unless the ROM skips stage2, §4).
  - It also means a UART-booted factory stage2 just reads the rest from SPI.
- **No stage2 on either chip fills 0x01800000.** There is no XMODEM code in them. The stage3/U-Boot
  recovery path needs a stage2 that does.
- **Image size.** 0x01800000 lies just below stage3's DRAM clear, which starts at 0x02000000. So the
  objects an app load needs must sit below flash offset 0x800000. IPXE (0x2c0000) and BOOT_MODE
  (0x340000) do.
- **Other levers** (an I2C preload EEPROM entry, the uC, the 0xfbff4000 forced-instance word):
  all need the ROM dump. Any of them would be a write to something boot-critical.

**(c) Protocol (*inferred*).**
- The ROM receives over **XMODEM**: there is a ROM XMODEM receive entry in the V2 iomap, and the
  factory U-Boot's own recovery scripts use `loady`.
- The ROM baud comes from the strap. What it receives is most likely the `S2`-headed stage2, loaded
  to SRAM 0xf2200000, the same bytes as SPI offset 0.
- UART CLI mode is a ROM command line.

The dump settles the details: block size, CRC vs checksum, what the ROM checks and the CLI commands.
No host-side Annapurna tool is in any dump or GPL drop in the repo.

## 4. Zero-write test plan (gated on the ROM dump)
Do these now, no writes: the §5 reads, the `e` key, the ROM dump.

Then build a **RAM flash image** on the host:
- a copy of the first 8 MiB of `mtd0.bin`;
- our u-boot wrapped (`alboot.py wrap --id 0x110 --load 0x08000000`) in IPXE 0 at 0x2c0000;
- a BOOT_MODE object at 0x340000 with app 0x110.

The SPI flash is never touched. It is the image both routes below feed to stage3. It also tests `u`
for real: with the key held, stage3 must print "User request for loading U-Boot!" and boot the
factory UBOOT 0 out of the same image.

**Route A, no strap: re-enter stage3 from the factory U-Boot.**
1. TFTP the image to 0x01800000, then `cp.b` a clean stage3 from it to 0x01000000, and zero the
   stage3 .bss (0x01059275–0x0105e000).
2. `mw.b 0xfbff41c1 0x01` (XMODEM on, SPI off), then `dcache off`.
3. Warm-reset CPU0 into AArch32 at 0x01000000. Stage3 writes SCR/MVBAR/CNTFRQ, so it must start at
   AArch32 EL3. The AArch64 EL2 U-Boot can't just `go` to it.
- Stage3 itself warm-resets cores this way: resume address at NB+0x2048/0x204c, AArch64 select at
  NB+(core+0x20)*0x100+4, reset at NB+0x24 (0x01023e9c, 0x01023da0).
- **Gate.** An AArch32 reset starts at the ROM's vectors (0xffff0000), so this works only if the ROM
  jumps to the resume address on a core warm reset. That is the first thing to read in the dump. If
  it does, everything from stage3 on is the production binary. The differences: the reader is a
  memcpy instead of SPI, and the stage2 run is skipped (the cold boot already ran it).

**Route B, UART strap (a hardware change on a spare): the real ROM UART boot.**
- Send a stage2 over XMODEM. It must be stage2 A **patched in RAM** so that, after DDR init, it calls
  the ROM's XMODEM receive into 0x01800000 and sets bit0 instead of bit1. Then send the RAM image.
- This needs the ROM function's address and ABI from the dump. Every patch is to bytes in RAM only.
- It is also the only real no-programmer unbrick, because it doesn't depend on anything in SPI.

**The host-side tool** (`uartboot.py`, once the protocol is read from the ROM) needs **exclusive
access to the serial port**. XMODEM is a binary handshake with tight timeouts, and tmux `send-keys`
can't carry it.
- Plan: stop tio (kill the `serial-dmc12` window), run `sx`/`sb` (lrzsz, add it to `flake.nix`) or
  an equivalent pure-stdlib termios script on the tty.
- Then restart tio. Its log file has a gap for the transfer.

**What can go wrong.**
- The SRAM/shared-data writes (Route A) and a bad reset address only hang the box. A power-cycle
  recovers, because the SPI flash is untouched.
- A strap change on the only working unit is a solder job. Do it on a spare.
- If the ROM is unreadable, both routes stay hypotheses. Then the fallback is the one-write
  BOOT_MODE test in §1.

## 5. Read-only commands for the main agent
At `ALPINE_DB>` after a normal cold boot (save to `inventory/u01/stage3-state.txt`; no identifiers expected):
```
md.b 0xfbff41c0 4        # expect: magic > 3a, flags 02 (SPI_LOAD_ENFORCE)
md.l 0x0105c8f4 1        # stage3 shared-data ptr: fbff4180 (proves stage3 .bss survived)
md.l 0x0105cacc 1        # stage3 reader: 01027f5c = SPI (01027fcc = RAM/UART, 01027fe8 = NAND)
md.b 0x0105caf0 1        # stage3 boot device: 03 = SPI mode 0
md.l 0x0105d260 1        # UART recovery flag: 0
md.l 0x0105d268 1        # app ID used: 00000005
md.l 0x0105a3e8 1        # skip-APCEA flag: 1 only if `p` was seen this boot
md.l 0x0105a48c 1        # keep-early-APCEA flag: 1 only if `e` was seen
md.l 0x0105d254 2        # active / fallback instance: 0 0
```
Then one cold boot with `e` sent the same way `p` was (16× at 50 ms from the stage3 banner):
- expect "User request for keeping early APCEA!", then a normal factory boot;
- `md.l 0x0105a48c 1` should then read 1.

Then the ROM dump in §2.

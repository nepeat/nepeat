# CS8210 UART recovery: boot ROM 3.1.0, stage2/3, entry routes, host tool

Offline work, 2026-11-15/16 (no hardware touched). The ROM dump (`analysis/boot-rom/rom-fbff0000.bin`,
64 KiB, crc32 `d1a98647`, sha256 `7e1ec7ef…`) is vendor code and stays git-ignored; this file holds
summaries and addresses only. Ghidra project `analysis/boot-rom/ghidra/` (ARM:LE:32:v8 at 0xffff0000,
seeded with every Thumb pointer; query outputs `analysis/boot-rom/q*.txt`). Stage2/3 references use the
projects in `analysis/boot-chain/` ([boot-chain.md](boot-chain.md)). HAL = the MikroTik GPL HAL in
`analysis/research/mikrotik-added/drivers/soc/alpine/HAL/`. V2 reference:
[../dmc-12-48r-as0/uart-boot.md](../dmc-12-48r-as0/uart-boot.md).
Marks: **[code]** read in the disassembly · **[emu]** the real ROM code run in Unicorn
(`scripts/rom_emu.py`) · **[hw]** seen on this unit · **[inf]** inferred. ROM addresses are the
0xffff0000 alias; Thumb functions are listed at their even address.

## Verdict
- **No console key or magic byte reaches the ROM's UART boot or CLI.** UART RX is read only by
  `xmodemReceive` (0xffff1484, via 0xffff1398) and the CLI's getc (0xffff20cc, from 0xffff2334) [code].
  The only ways in: the boot-device straps, or no valid S2 image at any of the 16 SPI offsets.
- **The ROM has a UART CLI** (strap 0; V2 had none): rd/wr/rdr/wrr/vrr, xmodem to any address, go
  (A32 or AArch64), gpio_set, I2C EEPROM read/write, quit (continues with SPI boot) [code, emu].
- **UART boot** (strap 4 at 115200, 1 at 2 Mbaud, 6 at 1 Mbaud, or automatic after 16 × 4 failed SPI
  tries): XMODEM, 128-byte blocks, CRC16 or sum, into 0xff000000 (LLC as SRAM), max 0x78000 B on this
  unit's straps, then **A32 jump to 0xff000020 with no header or sum check** (non-secure) [code, emu].
- **What it can fix:** a broken stage2 only. The factory stage2 sent over UART sets SPI_LOAD_ENFORCE and
  reads TOC/DT/STG3/U-Boot from SPI again (0xff00f5a4), as on V2. A bad TOC2, BOOT_MODE, STG3 or app is
  not reachable this way (stage2 at 0 stays valid, so the ROM never falls back); that needs a V3
  Route B (patched stage2, §4, not built) or a programmer.
- **Route R works in emulation**: from the vendor U-Boot, a CPU0 warm reset into AArch32 lands in our
  stub through the ROM's own resume vector, which replays the ROM's init and enters its CLI. RAM only,
  no strap, no SPI write; a hardware run is the parent's call (§7, step 4).
- Host tool `scripts/uart_recovery.py` + `scripts/rom_emu.py selftest`: **ALL PASS** (6 tests, real ROM
  code, through a pty and a consoled attach simulation).

## 1. Boot ROM 3.1.0
**Identity:** "Annapurna Labs Boot ROM version: 3.1.0", CVOS 5762119, HAL d0d4a0a, Mar 22 2017. Linked at
0xffff0000: 717 literal words are 0xffff…, and the HAL's `BOOT_ROM_XMODEM_RECIEVE_FUNC_PTR` for V3
(0xffff1485, `platform/alpine_v3/include/al_hal_iomap.h`) is this ROM's `xmodemReceive` [code]. The
0xfbff0000 copy is the same ROM (hardware check: §7 step 2).

**Reset (0xffff0000, A32, every core):** CPU index from MPIDR; table 0xffffaa40 gives that core's ANPA
`resume_addr_l` (CPU0 **0xf0410030**; cluster n at 0xf0400000 + n·0x80000, core k at +0x10000·(k+1)).
Non-zero → `mov pc` there at once, no stack, MMU off. Else SP = 0xfbff0000 + index·0x2000000 (CPU0:
0xfbff0000, growing down into PBS SRAM) and main 0xffff045c [code, emu].

**main (0xffff045c):** SCTLR (0xffff0338: MMU off, I-cache on); state block 0xfbfe0000..0xfbfe019c
cleared, first 0x18 B from 0xfffff2ad (word 0 = SPI offset, +8 = debug level); `al_bootstrap_parse`
(0xffff7e84) of PBS 0xfd8a8110/0x278 into 0xfbfe0164; GPIO 1 and 4 made outputs if the debug strap is on
(0xffff0498); time init; UART-CLI forces debug on (0xffff04d2); OTP ROM_DBG_DIS check (0xffff19b4); debug
UART (0xffff17b4); banner (0xffff0740); OTP decode (0xffff1814); "Boot configuration" (0xffff81b8, the
same printer as U-Boot's `dev_info`); PBS window 0xf3000000 (0xffff8fd4); LLC as SRAM at 0xff000000 when
the LLC strap is on (0xffff0680); stack to 0xff080000 and 0xffff03b8: MMU on if dcache; boot device 4 →
MMU off, CLI (0xffff0ad8), then boot device := SPI mode 0; then the load loop 0xffff0b5c / 0xffff12c4.

**1.1 Boot device (straps bits 17:15, 0xffff7e84; same as HAL `al_bootstrap_boot_device_get`):**

| bits 17:15 | ROM device | | bits 17:15 | ROM device |
| --- | --- | --- | --- | --- |
| 0 | **UART CLI**, 115200 | | 4 | **UART XMODEM, 115200** |
| 1 | UART XMODEM, 2,000,000 | | 5 | SPI mode 3 |
| 2 | NAND | | 6 | UART XMODEM, 1,000,000 |
| 3 | invalid: assert, fatal | | 7 | **SPI mode 0 (this unit, `dev_info`)** [hw] |

Other bits used: 18 debug (0 = prints on), 30 dcache off, 31 LLC off, 13:12 preload, 23:22 + strap2
bit 0 SPI-master mode. `dev_info` on this unit decodes to e.g. 0x047fa6ef (`rom_emu.py` STRAP_UNIT; the
emulated "Boot configuration" then matches `dev_info` line by line [emu, hw]); the real word is §7 step 2.
**Where the strap pins are is unknown**: `boot_strap` is a read-only latch ("Reflects the bootstrap
value", `al_hal_pbs_regs.h` 0x110); no override register in PBS, no CPLD register known
([board-control.md](board-control.md) lists none).

**1.2 OTP (0xffff1814, 0xffff19b4; `al_otp_storage_layout_v3.h` word 120/121):** secure boot (bit 4) →
0xfbfe0049; per-source disables SNOR (6), NAND (8), UART (9), UART CLI (10) are enforced **only when
secure** (0xffff0b5c, 0xffff03b8); ROM_DBG_DIS (bit 5) forces prints off unless DBG_EN (word 121 = 1).
This unit: all clear [hw: `otp show`], so nothing in OTP blocks UART boot or the CLI.

**1.3 Stage2 load (Stage2LoadLoadAndExecute 0xffff0b5c):**
- SPI/NAND: header 0x20 at offset `*0xfbfe0000`; sum16(0..0x1d) == u16 @0x1e and size (w3) ≠ 0; w4 = −1 →
  load base+0x20 (size limited), else load = exec = w4 (no limit); payload + u16 sum16 checked; flags
  byte +2 bit 1 → AArch64 entry (RVBAR + core reset, 0xffff1284), else A32 `mov pc` (0xffff0270).
  The factory S2 (exec 0xff000020, flags 0) passes: "Load stg2 from 3:00000000 … Checksum ok … Exec
  stg2 @ff000020 (00)!" [emu, mtd0 image].
- UART (0xffff0b5c, `cVar1 == 1`): UART re-init at the strap baud, `xmodemReceive(base, max)`; base
  0xff000000 with LLC (else 0xfbfe0200); max **0x78000** with dcache (page table at 0xff078000),
  0x7f800 without, 0xf600 without LLC. Return ≥ 0 and non-secure → jump base+0x20 (A32), nothing checked.
  The image is "S2 header + payload + sum" as on SPI: `uart_recovery.py s2` cuts it from mtd0
  (0x1b09b B, crc32 9deb81e7).
- Retry (`Recovery`, 0xffff12c4): 4 tries per offset (`*0xfbfe002c`), offset += 0x20000 for 16 offsets
  (0..0x1e0000), then boot device := UART (with the strap's baud field, here 115200) and prints move to
  UART1 (0xffff17b4); UART gets 4 tries, then fatal. No timeout-based fallback anywhere.
- Fatal (0xffff1334): "Unrecoverable Error - Boot ROM Debug printout end"; debug off → spin; debug on →
  GPIO 4 / GPIO 1 toggled every 500 ms (0xfd887040 / 0xfd887008). Nothing resets the box.

**1.4 "Boot ROM debug":** OTP says enabled (ROM_DBG_DIS = 0), so prints happen when strap bit 18 = 0 or in
the UART CLI. They are: banner, OTP summary (`Secure boot 0`, `DBG dis`, …, `puk# 0`), Boot
configuration, memory map, "Load stg2 from <dev>:<off>", "Read header @…", "Calculating checksum…",
"Checksum ok", "xmodemReceive ret …", "Exec stg2 @… (flags)!", errors as "Error! <func>:<line>". This
unit has bit 18 = 1 (`debug_mode 0`): a silent ROM, which is why the console starts at stage2 [hw].

## 2. The UART CLI (0xffff0ad8, table 0xffffaf84: 12 × {name, help, usage, fn})
| cmd | args (hex, **no 0x**) | does | fn |
| --- | --- | --- | --- |
| help | [cmd] | list / usage | 0xffff2534 |
| rd / wr | addr / addr val | 32-bit read / write | 0xffff26b4 / 0xffff2840 |
| rdr / wrr / vrr | addr bytes / + pattern const\|inc\|alt a1 a2 | range read / fill / verify | 0xffff2704 / 0xffff282c / 0xffff2836 |
| quit | | leave the CLI; the ROM goes on with SPI mode 0 boot | 0xffff2520 |
| gpio_set | gpio val | **drives a SoC GPIO** (dir + data) | 0xffff09e8 |
| xmodem | addr | `xmodemReceive(addr, 0x1000000)`, prints "%08x bytes received" ×5 | 0xffff0a78 |
| go | addr 0\|1 | 0: A32 `mov pc`; 1: AArch64 via RVBAR + core-0 power-on reset | 0xffff09b4 |
| eeprom_rd / eeprom_wr | i2c alen n / i2c alen src n | I2C EEPROM read / **write** | 0xffff07cc / 0xffff0888 |

Line editor (0xffff2334): prompt "> ", CR executes, BS and ^C work, ≤ 0x50 chars, ≤ 16 words. **LF is
kept as a character** (send CR only). Numbers are bare hex (0xffff2648); "0x…" is a syntax error [emu].
**rdr/wrr/vrr counts must be multiples of 4** (the loop is `count -= 4` until 0). A "reset" string exists
but no table points at it. In the CLI the ROM runs MMU off; DRAM is not initialised at cold boot, so
`xmodem` targets the LLC SRAM (0xff000000, set up before the CLI when LLC is on): `xmodem ff000000` +
`go ff000020 0` is a manual UART boot.

## 3. XMODEM receiver (0xffff1484)
Menie-style, as V2 [code, emu]: sends 'C' every 2 s up to 60 times (then NAK 60 times, then 3 × CAN, −2);
128-byte SOH only (STX → −4); `n`/`~n` checked, duplicate of the previous block ACKed and dropped; CRC16
(CCITT 0x1021, init 0, big-endian) or 8-bit sum as negotiated; 1 s per byte; 25 bad packets → −3; CAN CAN
→ −1; before every NAK and before the EOT ACK it drains the line until 1.5 s of silence; data past `max`
is dropped silently. One UART-boot try is ~4 min, four tries ~16 min.

## 4. After the ROM: stage2, stage3, U-Boot
- **Stage2** (main 0xff00f01c): NAND reader if the ROM's boot device is 0, else the SPI reader; instance
  from `*0xfbfe0000` (0xff00b59c: no STG2 TOC entry at that offset → error print, instance 0; after the
  automatic fallback the word is 0x1e0000 [emu], so expect that print); for any non-NAND device it sets
  SPI_LOAD_ENFORCE (0xff00f5a4 → 0xff00ae90(…, 1)). It never calls the ROM's XMODEM (no 0xffff1485 literal).
- **Stage3** (0x01035d54…): ROM boot device 1 (UART) → RAM reader (0x01035844, flash offset X at DRAM
  0x01800000 + X) unless SPI_LOAD_ENFORCE; SPI → SPI reader unless SPI_LOAD_ENFORCE is clear and
  XMODEM_LOAD_ENFORCE set, which prints "UART recovery!" (0x01035eb0). U-Boot follows the same flags
  ("echo recovery boot command!"). So the "UART recovery reader" is a RAM reader that **nothing fills
  over UART** on the factory chain.
- **V3 Route B (design only):** patch the UART-sent stage2's 0xff00f5a4 body to call the ROM's
  `xmodemReceive(0x01800000, 0x800000)` (DRAM is up by then) and set XMODEM_LOAD_ENFORCE instead; stage3
  and U-Boot then run from a RAM flash image. Unverified: that the ROM's UART/time state (0xfbfe0078..0xb4)
  survives stage2 until that call. This is what would unbrick a bad TOC2/BOOT_MODE (boot-chain.md §7).
- Vendor U-Boot has `loadx`/`loady`/`loadb`; its env scripts `bootupdspiy`/`dtupdspiy`/… do `loady` then
  **`sf erase`/`sf write`**: never run them.

## 5. Entry routes, least invasive first
| route | needs | writes | proves / gives |
| --- | --- | --- | --- |
| U-Boot `loadx` | the vendor prompt | DRAM | the consoled byte path and the sender (§7 step 3) |
| **Route R** | the vendor prompt; ends in a power cycle | DRAM, CPU0 reset | the ROM CLI + its XMODEM on this unit (step 4) |
| JTAG | a probe and the header (not located) | none | anything; OTP: SoC JTAG and CPU debug enabled [hw] |
| strap 0 / 4 (+ bit 18 = 0) | finding and reworking the strap resistors | board rework | real CLI / UART boot from power-on |
| stage2 invalid at all 16 offsets | an SPI write | **SPI** | automatic UART boot (not a test method; it is what a bad stage2 write leads to) |
| 1.8 V programmer | clip and plan | SPI | everything |

**Route R in detail** (`uart_recovery.py route-r build|plan|disasm`; blob 0x2cc B, crc32 `d5ef1e13`, at
0x01700000): an AArch64 trampoline (EL2) copies a 0x60-B EL3 stub to 0x1000 (CPU0 RVBAR, write-once,
set by stage3), cleans D-cache 0x01000000–0x02000000, writes ANPA `cpus_software_reset` 0xf0400024 =
0x00010013 (core 0 power-on reset, WFI mask as the DMC-12 needed). CPU0 restarts AArch64 EL3 at 0x1000:
'#', `resume_addr` 0xf0410030 = 0x01700180 (secure write), `RMR_EL3` = 2 + WFI (warm reset to AArch32,
the DMC-12 recipe). The ROM's reset vector jumps to the A32 stub: 'R', SP 0x01f00000, then main's init
calls (SCTLR, state block, straps, time, boot device := UART CLI, debug UART, banner, OTP, Boot
configuration) and the CLI; `quit` → 'Q', WFI. Skipped on purpose: the GPIO 1/4 debug outputs, PBS
window, LLC-SRAM, MMU. Emulated: trampoline and EL3 stub up to `msr rmr_el3` (Unicorn AArch64: the
exact MMIO writes), and the reset vector → stub → real ROM CLI with `help`, `rd`, a 64 KiB `xmodem` into
DRAM, `rdr`, `quit` [emu]. Not emulated: the resets themselves.

## 6. Host tool and emulator
- `scripts/uart_recovery.py`: `s2`, `xmodem` (`--first 'loadx …'` types the receiver command first),
  `rom-cli` (refuses wr/wrr/gpio_set/eeprom_wr/go without `--allow-write`, 0x prefixes, counts not ×4),
  `testfile`, `route-r`. Transports: `--tty` (exclusive), `--consoled HOST:PORT` =
  `ssh -T -e none -o BatchMode=yes HOST attach PORT`, `--cmd`. Reuses the DMC-12 `uartboot.py` sender
  logic (CRC16, block framing, holders check).
- **consoled is usable for XMODEM** [code: `rust/consoled`]: the daemon's port is raw with IXON/IXOFF
  off unless the port is configured Xonxoff (check `info cs8210`); the attach client copies port
  bytes to stdout unchanged and works without a pty (piped input passes through). Its input side still
  parses the prefix keys **C-b (0x02) and ^] (0x1d)**: the tool doubles both (a doubled prefix is the
  literal key). `--take` sends prefix-`t`. The attach replays the scrollback first: the tool drains until
  0.8 s of silence and accepts only a 'C'/NAK that arrives on its own. `[line] …` notices are ignored.
  ssh needs root's key (`--ssh 'sudo ssh'` or run with sudo).
- `scripts/rom_emu.py`: the ROM in Unicorn 2.1.4 (Cortex-A15 model, in the devshell), UART0 on a pty,
  UART1 to stderr, strap word per mode, OTP zero, hooks listed in its docstring. `run --mode
  uart|cli|spi|spi-blank|route-r` for manual play; `consoled-sim PTY` mimics an attach (replay, prefix
  rules, a warning on any unescaped prefix). `selftest` (≈25 s): Route R AArch64 writes; strap-4 UART
  boot of the real S2 over a tty; blank SPI → fallback → XMODEM through consoled-sim; SPI boot from the
  mtd0 image; strap 0 CLI → quit → fallback → stage2; Route R CLI session. 0 retries in every transfer.

## 7. Hardware test plan (zero SPI writes; the parent runs it)
Vendor prompt: `systemctl reboot` on NixOS, `123⏎` within the 3 s window. `P` = `sudo python3
scripts/uart_recovery.py`, `C` = `--consoled console@10.60.2.10:cs8210 --take`.
0. Host: `nix develop -c python3 scripts/rom_emu.py selftest` → `ALL PASS`. `ssh console@10.60.2.10 info
   cs8210` → 115200 8N1, flow none.
1. Read-only, vendor prompt: `md.l 0xfd8a8110 1` → bits 17:15 = 7, bit 18 = 1 (expect ≈ 0x047fa6ef; any
   other boot device means §1.1 is wrong); `md.l 0xfd8a8278 1` → bit 0 = 0.
2. `md.l 0xffff0000 4` → the same 4 words as `md.l 0xfbff0000 4`, and `crc32 0xffff0000 0x10000` →
   `d1a98647` (alias proven; low risk: if U-Boot's MMU map ends below 0xffff0000 it aborts,
   power-cycle). `uc_get_recovery` (uC GPIO read) → note the value.
3. Byte path, DRAM only: `P testfile /tmp/t.bin` (crc32 `671f0733`), then `P xmodem C --first 'loadx
   0x01800000' /tmp/t.bin` → tool `sent 0x10000 B in 512 blocks (crc), 0 retries`, U-Boot `## Total Size
   = 0x00010000 = 65536 Bytes`; `crc32 0x01800000 0x10000` → `671f0733`. Retries = line trouble; a crc
   mismatch = an escaping bug (stop).
4. **Route R** (owner OK: resets CPU0 into the ROM; RAM only; ends with a power cycle).
   a. Optional reads, **may abort U-Boot** if these ANPA registers are secure-only (then power-cycle and
      skip): `md.l 0xf0410048 2` → `00001000 00000000`; `md.l 0xf0410008 1` → 1; `md.l 0xf0410030 2`;
      `md.l 0x1000 4` (non-zero: the agent we overwrite).
   b. `P route-r build`, `P xmodem C --first 'loadx 0x01700000' analysis/boot-rom/route-r.bin`;
      `crc32 0x01700000 0x2cc` → `d5ef1e13`.
   c. `go 0x01700000` → `#` then `R`, the banner "Annapurna Labs Boot ROM version: 3.1.0", the OTP lines,
      "Boot configuration:" (= `dev_info` but boot_device UART CLI, debug_mode 1) and `> `.
      Nothing: the reset write was ignored at EL2 or RVBAR ≠ 0x1000. `#` only: no warm reset to AArch32 or
      the resume address is not honoured. `R` only: a ROM init call faulted or the NB timer is stopped.
   d. `P rom-cli C help 'rd fbfe0164' 'rdr fbfe0164 38'` → 12 commands; `[FBFE0164] = 77359400` (2 GHz).
   e. `P rom-cli C --load 1800000 /tmp/t.bin 'rdr 1800000 10'` → "00010000 bytes received" ×5, `Ok`, words
      `CDD89118 1E4110C3 7873C27E 35C961A6`. Then `P rom-cli C quit` → `Q`.
   f. Power-cycle (`lab-power 1 off`, 10 s, `on`) → normal boot. Do not `go` anything else; never
      `wr`/`gpio_set`/`eeprom_wr` (unknown wiring; the I2C EEPROM may be the preload source).
5. Not zero-write, owner decisions: locating the strap resistors (bits 15–18) for a real strap-0/4
   test; a JTAG header.

## 8. V2 (ROM 2.3, DMC-12) vs V3 (ROM 3.1.0, CS8210)
| | V2 | V3 |
| --- | --- | --- |
| size, debug libs | 16 KiB | 64 KiB, mbedtls RSA, NAND ONFI |
| straps 0/1/6 | error | UART CLI / UART 2 Mbaud / UART 1 Mbaud |
| S2 load / UART max | 0xf2200000 / 0x68000 | 0xff000000 (LLC SRAM) / 0x78000 (dcache on) |
| ROM state | 0xf2000000 (= 0xfbff4000) | PBS SRAM 0xfbfe0000; bootstrap struct 0xfbfe0164 |
| AArch32 resume reg | NB 0xf0072028 | ANPA per-core 0xf0410030 |
| CPU0 reset | NB 0xf0070024 = 0x00010013 | ANPA 0xf0400024, same value format |
| UART-boot prints | UART1 | UART1 (moved there for XMODEM) |
| same | 16 offsets × 4 tries → UART; Menie receiver; no UART-image check when non-secure; stage2 sets SPI_LOAD_ENFORCE | |

## 9. Open
- Strap pin location (and whether any CPLD drives them); JTAG header.
- Route R on hardware: whether NS EL2 can write ANPA `cpus_software_reset`; whether the NB timer the ROM
  uses (source 1, 0xf0071008) runs after U-Boot.
- V3 Route B (§4) if a TOC2/BOOT_MODE write is ever planned without a programmer.

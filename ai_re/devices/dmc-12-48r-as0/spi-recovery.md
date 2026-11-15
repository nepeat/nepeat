# SPI NOR recovery and safety paths (before replacing the boot U-Boot)

Static analysis only (2026-11-15). Nothing was run on hardware for this file. Sources:
`dumps/u01/mtd{0,1}.bin.zst` (sha256 in `dumps/u01/SHA256SUMS`), the factory U-Boot in
`ghidra/dmc12.gpr`, and stage2/stage3 of CS0 instance 0, imported in a scratch Ghidra project
(ARM/Thumb, stage2 at 0xf2200000, stage3 at 0x01000000; to rebuild it use
`tools/ghidra/import-raw.sh <img> <dir> <name> <base> ARM:LE:32:v7`). Function addresses
below refer to those images. **Inferred** means read from code shape and never observed.
The boot-chain and U-Boot ABI work is in `u-boot-port.md`; this file covers only what
protects a unit when a flash goes wrong.

## Verdict
**No-go for writing the CS0 `UBOOT` object until rung 0 below is done.** The A/B layout
does **not** give automatic recovery today:
- Each stage loads only its own instance.
- A bad object makes stage2 spin forever and stage3 go without an app.
- No stage arms a watchdog, and nothing flips the instance.

So with a bad instance-0 U-Boot, the only proven way back is an external programmer.
Nothing on the U-Boot flash path is one-way: no OTP, no signing, and no write-protect bit
is in the way. But the next step needs the flash package, a 1.8 V programmer and the
answers to the read-only questions below.

## Hardware answers (u01, 2026-11-15; raw: `inventory/u01/spi-recovery-queries-private.txt`)
- **OTP (direct = shadow): secure boot disabled, all 4 public-key hashes zero and unlocked,
  minimal SW version 0, JTAG + SoC JTAG + SPI debug + CPU debug enabled, boot from SPI/NAND/UART
  enabled, UART CLI enabled.** No signature can be enforced. Boot-ROM UART boot is a candidate
  no-programmer recovery path (untested; ladder step 7).
- `dev_info`: boot_device SPI(M0), i2c_preload 400K from 0x57, cpu 2000 / ddr 1066 / sb 500 MHz,
  ref 100 MHz, cores 1x4, dcache + llc disabled at U-Boot time.
- SRAM boot word `0xfbff4000` = `00000000` (then `f2200020 f2200020 fd883000`); `0xfbff4100` holds
  `ff39a514 bffffdfd 0400fffb` + the string "v2sil-39-rc1".
- `flash_contents_obj_validate` STG3/UBOOT/DT × instance 0/1: all rc=0 (silent on success; a bad ID
  prints "al_flash_obj_validate failed!", rc=1).
- uC: recovery disabled, GPIO 2[2] dir 1 (output), value 0; watchdog enabled, 10 s, reset count 0.
  Env: boot_instance_active=0, non_active=1.
- SPI status (Linux, `spi-status.py`, read opcodes only): both chips RDID c22538, SR 00, CR 07,
  SCUR 00: **unprotected**, no BP bits, SRWD 0, TB/WPSEL/LDSO 0 (none of the one-way bits set).

## 1. What selects CS1 (mtd1)
- The boot ROM boots from SPI master SS0. SS1 is a MUIO alternate function
  (`if_spim_a_ss_1`). Only firmware allocates it: the U-Boot/stage3 `pinctrl_init`, in
  both DTs. Linux and the factory U-Boot address the two chips separately (spi0.0 /
  spi0.1). The U-Boot TOC backend reads CS0 only.
- **uC GPIO port 2 bit 2 is the only candidate** for the select. `do_uc_get_recovery`
  (0x01106754) and `do_uc_set_recovery` (0x011067c8) are plain uC GPIO read/write
  (`uc 0x0909/0x0a0a`, port 2, bit 2). The messages call it "SPI flash"
  ("Failed to set SPI flash to %d").
  - No stage reads it at boot. So the select is pure hardware.
  - *Inferred:* a CS0/CS1 swap mux in front of the boot ROM. Unknown: whether the uC
    keeps the value across power cycles, and whether it acts at once or only at the next
    SoC reset.
  - `uc_get_flash_booted_from` sends nothing ("Unsupported command for dev id 1").
- **No strap or boot-device change selects CS1.** Straps choose SPI, NAND or UART, not a
  chip select.
- **What CS1 holds:** generic Annapurna al-boot 2.44 (Jul 2018).
  - The same TOC as CS0 (0x80000, byte-identical). Stage2 at 0x0, 0x20000 and 0x40000
    (one image three times).
  - A generic **K2C-AB** DT (`alpine_v2_k2c_ab`) and APCEA data.
  - Everything from 0xb7e120 on is **0x00**: env, redundant env, EEPROM objects. So
    U-Boot 2.44 runs on its compiled-in default env.
- **Would CS1 boot this board to a prompt? Unproven, and the risks are concrete.**
  - UART: same (ttyS0 @0xfd883000). DDR: the 2.44 DT has **no** `board-cfg/dram/channel0`
    and no SPD location. 2.44 stage2 gets them from "early init" data, *inferred* to be
    the I2C preloader EEPROM. If that guess is wrong: "no valid dimms!" and a hang in
    stage2. The 2.60 DMC-12 DT points at SPD 0x57 offset 0x440.
  - SerDes: PCIe x8 on groups 0 and 1, so no SATA and no SSD. PCIe port 0 is set up as an
    **endpoint** (`ep-ports = <0>`), and `if_pci_ep_int_a` is pinmuxed.
  - `gpio_init` drives SoC GPIO 23–41 low as outputs (37 excepted) and 34 high. GPIO 41
    is the **CPLD interrupt input** (PL061 @0xfd897000 line 1), so the SoC would fight the
    CPLD. GPIO 22, the CPLD-JTAG-related line that the DMC-12 DT drives high, is left alone.
  - U-Boot 2.44 has **no `uc_*` commands**, so it can't set the select back.
  - Its `UBOOT_SCRIPT` (the same one as CS0's) finds no `boot_app_toc`. It then sets
    `bootcmd` = `al_pcie_ext; … bootm BOOT_APP`, sets `bootdelay 0`, and runs
    **`saveenv`** into the env of the chip that booted. Then it autoboots the K2
    `BOOT_APP`.
- **Verdict on CS1:** a last-resort experiment, not a recovery path. Use it only if
  `uc_set_recovery` turns out to be volatile (a power cycle clears it). Then, from a CS1
  U-Boot prompt (abort at bootdelay 0), the DMC-12 chip would be on SS1. *Unverified:*
  whether 2.44 `sf probe 0:1` reaches it. Never test it on the only working unit.

## 2. A/B instances: what really happens
- The boot ROM leaves a word at SRAM **0xfbff4000**:
  - bit0 = 0: the offset it loaded stage2 from.
  - bit0 = 1: a forced pair, active = bits 15:8 and fallback = bits 23:16.
  - Stage2 (0xf2208718), stage3 (0x0102610c) and U-Boot (0x011376ac) all decode it the
    same way. For an offset they look up the TOC entry with ID 1/0xc/0xd at that offset;
    its instance becomes **both** active and fallback.
- `al_flash_toc_find_id_with_fallback` (U-Boot 0x01137534) uses the fallback only when an
  object ID is **missing from the TOC**. It is never used when an object fails validation.
- **Stage2** (main 0xf220bd88) loads DT n, then `PRE_BOOT_V2` n, else `STG3` n (header +
  data checksum). On any failure it prints and **spins forever** (`while(1)`). It never
  resets and never tries the other instance.
- **Stage3** (0x010285a8) loads `BOOT_MODE` (absent on both chips → app 5 = U-Boot), then
  DT n, then the app (n). On an app header failure it prints "failed (app, instance n)" and
  goes on without an app. Keys during its window:
  - `p` skips APCEA.
  - `e`/`u` load U-Boot instead of the BOOT_MODE app. That doesn't help when U-Boot n is
    the broken object.
- **`boot_instance_active/_non_active`** are set by U-Boot at every boot (0x01102f54) from
  the decoded word. They are not state: they were in `printenv` only because a `saveenv`
  captured them. Nothing counts boots, and nothing flips the instance.
- **The only instance switch is in the boot ROM:**
  - `flash_contents_instance_invalidate N` zeroes byte 0 (`S`) of instance N's stage2.
    It works by a read, a 4 KiB erase and a rewrite of that sector.
  - `…revalidate` writes `S` back.
  - *Inferred:* the ROM then takes the next valid `S2` (0x40000). The stage2 header
    carries a sum8 header checksum (byte 0x1e = sum of 0x00–0x1d, checked on all 5
    copies), and the image ends in `sum16(payload)` plus footer `ca47 ec43 f6ce`. So the
    ROM probably validates and may fall through on a bad stage2. That is not proven (the
    boot ROM is not dumped).
- **CS0 instance 1 is not a twin.** `stg2.1` is a generic 2.60.4 release, not the DMC-12
  `stage2_loader_k2 v2.60.3`. `dt.1` is the generic K2C-AB DT: same DDR block, but the
  SerDes, PCIe and `gpio_init` hazards of section 1. `stg3`, `uboot` and `uboot_script` are
  identical in both instances, and `bootapp` differs. Instance 1 has no env of its own:
  U-Boot falls back to the instance-0 env.
- **The one cheap A/B lever** is the forced pair at 0xfbff4000 (bit0 = 1), which every
  stage honours. 2.44 even names a helper `…_active_instance_force_temporary`. If the boot
  ROM keeps that SRAM word across a warm reset, then a one-shot test of instance 1 gets a
  cold-boot fallback to instance 0 for free. That is an **experiment, not read-only**
  (rung 3 below).

## 3. Watchdogs
- **SoC sp805:** no stage arms it. Stage2 uses wdt0 only for an intentional reset
  (`__wd0_board_reset`, 0xf22018b4: CTRL←2, LOAD←0). Stage3 and the factory U-Boot hold
  no sp805 literal; `reset` does the same immediate trick. So a hung stage hangs forever.
  Our upstream U-Boot must keep `WATCHDOG_AUTOSTART` off unless a reset could actually
  lead somewhere else.
- **uC watchdog** ("enabled, 10 s, reset count 0"): nobody pets it. Stage2 sends only uC
  0x0101 (reset SoC), 0x1e1e (secure-state mirror), 0x1f1f (physical presence), 0x3535
  and 0xa9a9 (I2C / TPM access) and 0xcaca3737. Stage3 and U-Boot send no watchdog command
  either. Since it never fires, it must watch something we don't drive, or be armed only
  by an event. *Inferred:* the Nitro/host or mgmt-I2C flow.
- Even if it fired, the reset would re-enter the **same chip and the same instance**.
  That's a loop, not a fallback. Turning it into a fallback would need the uC firmware to
  flip GPIO 2.2 on expiry, and we have no evidence of that. Not usable.
- The CPLD WDT flag (0x2a) only fails the fans safe. It never resets the board (the box
  stayed at an interrupted `ALPINE_DB>`, which skips `clear_wdt_flag`, for minutes; PROGRESS).

## 4. Secure boot, OTP and one-way operations
- **uC "NOT-SECURE" is a mirror.** On every boot, stage2's TPM/physical-presence flow
  (0xf220d380) sends `uc_set_secure(<SoC secure state>)` (0xf220d514 → uC 0x1e1e) and
  `uc_set_pp`. The real state is the SoC OTP: `secure-boot-enable`, PUK hashes, minimum SW
  version. Settle it with `otp show direct`/`shadow` and `dev_info` ("- Secure boot:",
  "- Minimal SW version:").
- **Signatures are not enforced on SPI objects.** *Inferred* from the evidence: no
  signature objects in either TOC, unsigned generic and custom builds side by side, and
  `NOT-SECURE`. The new U-Boot only needs a valid al-boot object header and data checksum
  (the sibling's work).
- **One-way: never.**
  - SoC `otp` (every subcommand except `show`): the help text itself says "irreversible!".
    It covers secure-boot-enable, boot-from-SPI/UART/NAND-disable, JTAG/SPI-debug-disable,
    PUK hash, **min-sw-ver** (anti-rollback: it would brick older images),
    otpr-override-by-pld-disable and tz-enable.
  - uC: `uc_set_secure`, `uc_set_nitro_mode`, `uc_update` (bootloader!), `uc_fru_*`,
    `uc_set_pp`, `uc_sdr_set_fw_update`.
  - Flash chip: **WPSEL** (opcode 68h, OTP: switches to individual-block lock with every
    block locked at power-up), the **TB** bit (config register, OTP) and **LDSO** (WRSCUR
    2Fh). Also SRWD=1 if WP# is tied low: a hardware lock that only a rework undoes.
- **Boot-critical, but not one-way.** The **I2C preloader EEPROM** (24c64 @ I2C 0x57;
  `pld_i2c_addr=57`; the SPD lives at 0x440 in the same part).
  - Offset 0 holds a boot-ROM preload script (magic `0x1c361c36`) that writes PLLs, PBS
    MUIO, SPI clock divider (0xfd882014←0xe) and a record at SRAM 0xfbff4100. Preload is
    on: U-Boot prints "I2C Preload Disabled!" otherwise, and it doesn't.
  - Writers to avoid: `eepromupd*`, `eeprom_per_device save`, and **stage2 itself**.
    Stage2 rewrites the EEPROM from a valid SPI `EEPROM` object (0x700) at
    0xfc0000/0xfc1000 ("Updating EEPROM content"). Those objects are 0xff on CS0 and 0x00
    on CS1. **Any CS0 image we write must keep them blank.**
- **None of the above is needed to flash U-Boot.**

## 5. Write protection
- Status-register BP3:0 and SRWD are non-volatile and default to 0. The factory U-Boot's
  `sf probe` (0x01165ab0) **always sends WRSR 0x00**: it clears BP and SRWD, then sets QE
  for Macronix only when the read op is quad (not here). It runs at every boot ("SF:
  Detected" in board init). So BP protection can't be in effect on CS0 unless SRWD=1 with
  WP# low. Then WRSR is ignored, and `flash_contents_obj_update` / `sf erase` would fail
  with "dev erase failed".
- **Even a "read-only" `sf probe 0:1` writes CS1's status register.** It's harmless (it
  clears BP), but it is a write.
- Mainline Linux: the mx25u12835f entry has no `SPI_NOR_HAS_LOCK` and dw-apb-ssi is
  single-lane. So the probe writes nothing. Our DTS also marks both partitions
  `read-only`.
- The WP#/SIO2 wiring is unknown (`if_nor_wp` is the parallel-NOR pin).
- Read-only check: `spi-status.py` (Linux). It rebinds spi0.N to spidev and sends only
  RDID/RDSR/RDCR/RDSCUR. Expect `c22538`, SR 00, TB 0, WPSEL 0, LDSO 0.

## 6. External programming
- **Chip:** MX25U12835F, **1.65–2.0 V only**. Datasheet PM1728 lists 8-WSON (6×5 or 8×6
  mm) and 16-SOP 300 mil; later revisions add a 200 mil 8-SOP (`M2I`). The marking suffix
  decides the tool:
  - `M2I`: SOIC-8 clip.
  - `MI`: 16-pin SOIC clip.
  - `Z2I` / `ZNI`: WSON, which a clip can't grip. Use a pogo-pin WSON probe, or hot air
    plus a socket.
- **Where:** on the K2 SoC card next to the Alpine, two identical Macronix parts (CS0 and
  CS1), plus the Infineon SLB9670 SPI TPM on CS2 and the uC. Photos are needed; none
  exist in the repo yet.
- **Programmer:** it must drive 1.8 V I/O, not just 1.8 V VCC. Options: a Dediprog
  SF100/SF600, an FT2232H board with VIO = 1.8 V (e.g. Tigard), or a CH341A only with a
  real level-shifting 1.8 V adapter (the cheap resistor adapters overdrive the I/O).
  Software: flashrom (`-c MX25U12835F`). Before any write, read the chip twice and
  compare it with `mtd0.bin` / `mtd1.bin`.
- **In circuit:** SCLK/MOSI/MISO are shared by both flashes, the TPM and the SoC. The SoC
  must not drive the bus, and powering the 1.8 V rail from the programmer back-feeds the
  SoC and the uC. *Unknown:* whether anything can hold the SoC in reset (uC 0x0101 only
  pulses it; CPLD reg 0x28/0x29 too). Assume chip-off or a powered board with the SoC
  held in reset, and settle that with a photo and a rail measurement first.
- **JTAG:** OTP has `jtag-soc-disable` / `jtag-debug-disable` (state unknown until
  `otp show`). No header has been identified. The CPLD's MachXO3 JTAG is bit-banged from
  SoC GPIO 22 (`cpldupd`); it is not the SoC's JTAG.
- **UART boot:** the boot ROM, stage3 ("UART recovery!") and U-Boot (reads objects from
  RAM +0x1800000; `bootcmd=echo recovery boot command!`) all support a UART boot device.
  It is chosen by straps or a stage2 shared-data flag, not by anything we can set from
  software today. It would be a real unbrick path if the K2 card's boot-device strap
  resistors can be found.
- **Restore from software (works only while a good loader is still running):**
  - Factory U-Boot: `tftpboot 0x08000000 mtd0.bin; sf probe 0:0; sf update 0x08000000 0 0x1000000; sf read 0x0a000000 0 0x1000000; cmp.b 0x08000000 0x0a000000 0x1000000`.
    `sf update` skips identical sectors. Note: `bootupdspi` erases from 0 for `$filesize`:
    never.
  - Linux: a DTB variant without `read-only`, then `flashcp -v mtd0.bin /dev/mtd0`.
  - Neither path helps once a broken U-Boot is the thing that boots. So **verify by
    readback before any reset**. Keep the factory U-Boot (or Linux) running until
    `cmp.b`/CRC matches the staged image.

## 7. Recovery ladder (least → most invasive)
| # | rung | preconditions | status |
| --- | --- | --- | --- |
| 0 | **Before any write:** fresh `sf read` of CS0 cmp'd with `mtd0.bin`; `spi-status.py`; `otp show`; `dev_info`; chip markings and photos; a 1.8 V programmer read of a CS0 chip (ideally on a second unit) | owner, hardware in hand | **not done** |
| 1 | Don't reboot: stage the new object and the old one in RAM, write, read back, `cmp.b`; on mismatch rewrite the old object from RAM (`sf update`) | a factory U-Boot or Linux with a writable mtd still running | standard, not rehearsed |
| 2 | Bad U-Boot that still prints and reads keys: its own prompt → restore from TFTP as in §6 | the new U-Boot has net + `sf` | depends on the new U-Boot |
| 3 | One-shot test of instance 1: from the factory prompt `mw.l 0xfbff4000 0x00010101; reset` (warm). A cold boot then falls back to instance 0 | §2 lever confirmed (SRAM word survives warm reset) and stg2/dt instance 1 judged safe for this board | **experiment, untested**: SRAM write + reset, no flash write |
| 4 | Invalidate instance 0's stage2 so the ROM boots instance 1; revalidate from the instance-1 U-Boot | ROM fallback proven; instance 1 holds a good stg2 + DMC-12 DT + U-Boot (writes to 0x40000 and 0x350000 first) | inferred; a 4 KiB rewrite of sector 0 |
| 5 | `uc_set_recovery 1` → CS1 boots (generic 2.44) → abort at bootdelay 0 → `sf probe 0:1` → rewrite the DMC-12 chip | the select is volatile; CS1 boots DDR; owner accepts the GPIO 41 contention and the CS1 env `saveenv` | inferred, risky |
| 6 | External 1.8 V programmer on the bad chip (clip or pogo, SoC quiet), write `mtd0.bin` back | package known, programmer, SoC held off the bus or chip removed | standard technique, nothing rehearsed |
| 7 | UART boot via the boot-device strap | strap located on the K2 card | inferred from code only |

Only rungs 1 and 6 are ordinary engineering. Rungs 3–5 and 7 rest on the inferences above.

## Read-only commands for the main agent
At `ALPINE_DB>` after `./powercycle_to_uboot.py` (save to `inventory/u01/`; files with
identifiers go to `*-private.txt`):
1. `md.l 0xfbff4000 4` (expect bit0 = 0, offset 0) and `md.l 0xfbff4100 8` (preloader record).
2. `dev_info` (secure boot, boot device, preload, boot-from-UART, minimal SW version).
3. `otp show direct` and `otp show shadow` (only `show`; every other `otp` subcommand
   burns fuses).
4. `uc_get_recovery`, `uc_gpio_get_dir 2 2`, `uc_gpio_read 2 2`, `uc_watchdog_get_status`.
5. `printenv boot_instance_active boot_instance_non_active` (expect 0 / 1).
6. `flash_contents_obj_validate` for STG3, UBOOT and DT, instances 0 and 1 (CRC check
   only; usage from `help flash_contents_obj_validate`).

Linux (NixOS, root): `./spi-status.py` (both chips).

Physical (owner): photos of both sides of the K2 SoC card; the marking and suffix of
both Macronix parts; whether a JTAG/debug header or boot-strap resistors are labelled;
the 1.8 V rail test point.

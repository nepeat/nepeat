# Annapurna Alpine V3 boot chain: notes for other cards

Cross-device notes for Alpine V3 (16× Cortex-A72, "K2X") boards, from the CS8210-32X
([devices/cs8210-32x/boot-chain.md](devices/cs8210-32x/boot-chain.md): addresses, offsets, evidence). It
extends the V2 reference [alpine-v2-boot-chain.md](alpine-v2-boot-chain.md); everything not listed here
behaved as on V2. HAL = the Annapurna HAL 2.6 in the MikroTik GPL drop
(`devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL/`, `platform/alpine_v3/`).
Marks as in the V2 file: **[code]**, **[hw]**, **[inf]**.

## 1. Same as V2
- Object format, TOC format and search (16 offsets × 128 KiB from 0, first TOC whose magic, header sum and
  entry sum pass), byte sums only, object IDs (plus the V3 SerDes firmware IDs 0x510/0x520/0x530, loaded by
  stage3 to 0x01120000/0x01130000 and downloaded to the SerDes complexes) [code]. `alboot.py list` reads V3 images.
- Stage roles: ROM → stage2 (A32, DDR, TPM, measured boot) → stage3 (A32 CVOS: BOOT_MODE, DT, SerDes,
  keys, DRAM clear, app) → AArch64 agent at DRAM 0x1000 → app at its header's load/exec [code].
- Stage3 logic, including its traps [code]: BOOT_MODE word +4 = app ID; one console poll with `p`/`e`/`u`
  forcing app 5; a valid BOOT_MODE header with a bad data sum skips the poll and the app lookup (hang);
  a missing app falls back to UBOOT (5); a bad app header takes the raw path at flash 0x40000.
- No authentication in stage2/stage3 3.65.1 [code]: the PUK/SIG names exist only in the ID table; stage3
  links no OTP code. OTP secure boot (shadow word 120 bit 4) only steers stage2's TPM/uC flow.
- Measured boot into the TPM (PCR 0 objects, PCR 1 env) [code]; the uC gets SET_SECURE(OTP) each boot.

## 2. Different on V3
| item | V2 | V3 |
| --- | --- | --- |
| stage2 load / exec | 0xf2200000 / +0x20 | 0xff000000 / 0xff000020 [code] |
| ROM state word (stage2 offset, bit0 forced pair) | SRAM 0xfbff4000 | PBS int mem 0xfbfe0000 (`STAGE2_ACTUAL_OFFSET_ADDR_IN_PBS_SRAM` 0) [code] |
| app exec / APCEA handoff words | 0xfbff40f8 / 0xfbff40fc | 0xfbfe00f8 / 0xfbfe00fc [code] |
| SRAM agent, cpu-resume, shared data, dev info | 0xfbff4200, 0xfbff4120, 0xfbff4180, 0xfbff4100 | 0xfbfe0200, 0xfbfe00c0, shared params 0xfbfe0150, general shared data 0xfbfe0180, dev info 0xfbfe01c0 (HAL iomap) |
| boot ROM | 0xfbff0000, 16 KiB, ver 2.3 | 0xfbff0000 (PBS int mem + 0x10000), XMODEM receiver at 0xffff1485 (HAL); not dumped |
| SPI read path in stages | SPI mmap | mmap at 0xf8000000 + offset [code]: keep objects below 16 MiB [inf] |
| app load window refused by stage3 | 0xf2200000-0xf22fffff | 0xf3000000-0xf33fffff (ADV_ETH SRAM) [code] |
| DT lookup | ID 2 | ID 2 with a "flavour" in bits 23:16 from uC GET_DT_FLAVOUR (default 0); APCEA_CFG and PXE flavours likewise [code] |
| uC flavour queries | none | 0xBABA8F8F/9191/9494; a uC that answers 0xCACAFCFC makes the HAL print "Command is not supported in secure-mode." even with OTP and uC NOT-SECURE (harmless) [code+hw] |
| bad app header (raw path) | runs whatever sits at 0x40000 | on a TOC-at-0x40000 layout it reads the TOC magic as a size, prints "Invalid application size!" and spins [code] |
| preload | I2C EEPROM (0x57/0x50) | also SPI-master and SPI-slave preload; the CS8210 uses SPI-slave (`dev_info`) [hw] |
| OTP layout | v2 | `al_otp_storage_layout_v3.h`: 4 PUK hashes, PUK index, TZ, SoC JTAG, per-boot-source secure-boot disables (SNOR/NAND/UART), RMA debug |
| vendor U-Boot | 2015.07 | 2018.05 (`bootsecure`, `otp` writes need a password; `flash_contents_*`, `uc_*`, `dev_info` as on V2) |

## 3. Adding an app without touching the factory objects
- If the vendor TOC has no spare slot (CS8210), **write a second TOC at an earlier probe offset**
  (0x20000 when stage2 ends below it and the sector is blank): it shadows the vendor TOC for stage2, stage3
  and U-Boot, a torn write fails the sums and is skipped, and erasing it restores the vendor TOC. List the
  vendor entries unchanged plus BOOT_MODE and the new app, write the new objects first and the TOC last.
- Keep UBOOT 5 instance 0 as the factory escape (`u` key) and the factory SPI writer.
- Look for orphaned objects inside oversized vendor slots (the CS8210 has a reference UBOOT_SCRIPT, IPXE and
  U-Boot tail inside UBOOT's max): never point a TOC at them; UBOOT_SCRIPT in particular can run `saveenv`.
- Rehearsal in RAM (V2 Route A) uses the stage3 "UART recovery" reader (flash offset X at DRAM 0x01800000 + X),
  which V3 keeps [code]; the CPU-reset recipe and SRAM addresses must be redone for V3 (not done).

## 4. Identify a V3 card (read-only)
As V2 §9, plus: S2 exec 0xff000020; `stage2_loader vX` without a k2/ep suffix (build path names the product,
e.g. `NetOS-k2x-n-stageloaders`); SerDes FW objects 0x510/0x520/0x530; `dev_info` preload source; read
`0xfbfe0000`, not 0xfbff4000, for the stage2 offset. Read PBS/ROM addresses only from U-Boot (`md` returns
0xbad0add1 for holes), never via `/dev/mem` on mainline (SError panic).

## 5. Open
- V3 ROM: offsets probed, S2 checks, XMODEM fallback (dump 0xfbff0000 from U-Boot).
- Handoff EL/registers into the app (agent `agent_wakeup v3.53` not disassembled; V2: EL2, MMU off, x0 = 0).
- V3 strap map (boot device bits; HAL `al_hal_bootstrap` V3 has the same parser as V2 plus the SPI preloads).

## 6. Boot ROM 3.1.0 and UART recovery (CS8210 dump; answers the first and third items of §5)
Detail and evidence: [devices/cs8210-32x/uart-recovery.md](devices/cs8210-32x/uart-recovery.md).
- 64 KiB, linked at the 0xffff0000 alias (HAL `BOOT_ROM_XMODEM_RECIEVE_FUNC_PTR` 0xffff1485 is its receiver).
  Reset: per-core ANPA `resume_addr` (CPU0 0xf0410030) non-zero → jump there first (V2: NB 0xf0072028).
- Straps bits 17:15: 0 UART CLI, 1 UART 2 Mbaud, 2 NAND, 3 invalid, 4 UART 115200, 5 SPI mode 3, 6 UART
  1 Mbaud, 7 SPI mode 0; bit 18 = 0 debug prints. No console key or magic byte; no override register.
- S2 checks as V2 (header sum16, size ≠ 0, payload sum16; flags +2 bit 1 = AArch64 entry). 16 offsets ×
  4 tries, then UART XMODEM by itself (Menie, 128 B, CRC/sum) into 0xff000000 (LLC SRAM), max 0x78000 with
  dcache, entered at +0x20 unchecked when non-secure. UART-boot prints go to UART1.
- New on V3: a UART CLI (rd/wr/rdr/wrr/vrr, xmodem, go A32/AArch64, gpio_set, I2C EEPROM rd/wr, quit →
  SPI boot); numbers are bare hex, CR only. Per-source OTP disables bite only in secure mode.
- Stage2 sets SPI_LOAD_ENFORCE for any non-NAND device, so a UART-sent factory stage2 fixes only a bad
  stage2. From a running vendor U-Boot the CLI is reachable with a CPU0 warm reset through the resume
  register ("Route R", emulated; `scripts/uart_recovery.py route-r`).

# CS8210-32X boot chain (boot SPI flash, offline dissection)

Static analysis of the read-only SPI dumps, 2026-11-15. Nothing was written. Device-independent
Alpine V3 notes: [../../alpine-v3-boot-chain.md](../../alpine-v3-boot-chain.md). The V2 reference
this builds on: [../../alpine-v2-boot-chain.md](../../alpine-v2-boot-chain.md) and the DMC-12 files it links.

**Sources.** `images/spi-2026-11-15/mtd0..4` (mtd0 sha256 `a22ff482…1897f6`, same as 11-09; only the env
differs). Assembled 32 MiB image + carved objects + three Ghidra projects (stg2 A32 @0xff000000, stg3
A32 @0x01000000, U-Boot A64 @0x01100000; `tools/ghidra/import-raw.sh` + `seed_functions.py`, queries
with `devices/dmc-12-48r-as0/ghidra/scripts/gdec.py`) in `analysis/boot-chain/` (git-ignored). Live:
`serial-consoled-20261115-boot.txt`, `serial-20261109-182401.txt`, and the vendor-prompt OTP/TOC read
(`otp-readout-20261115.txt`, main agent). Map: `scripts/spi_freemap.py images/spi-2026-11-15`.
Marks: **[code]** read in the disassembly · **[hw]** seen on this unit · **[inf]** inferred.

## Verdict
- **No signature anywhere in the chain, and OTP secure boot is off [hw].** Stage2 is unsigned (no RSA blob
  after its payload [code]); stage3 v3.65.1 validates objects with byte sums only and has no OTP or
  PUK/SIG code [code]; the TOC has no PUK/SIG objects. `otp show direct/shadow`: secure boot disabled,
  4 PUK hashes zero, min SW ver 0; `uc_get_secure`: NOT-SECURE [hw].
- **No spare app slot, no BOOT_MODE, one instance of everything.** The app is always UBOOT 0. The
  console keys exist but change nothing today.
- **Best SPI layout (scheme A'): a second TOC at 0x20000** (blank sector; the TOC search probes 0x0,
  0x20000, 0x40000… and takes the first valid one) listing the 9 factory entries + BOOT_MODE + a new
  app in blank flash. Factory UBOOT 0 and the factory TOC at 0x40000 stay untouched. A torn TOC2 fails
  its sums and the search falls through to 0x40000; erasing sector 0x20000 undoes everything (§7).
- Safest of all: keep chainloading from the factory U-Boot (scheme C, works today, no SPI write).

## 1. Flash layout (spi0.0, mx25u25635f 32 MiB, 1.8 V)
| range | Linux mtd | TOC object (max) | used / content |
| --- | --- | --- | --- |
| 0x000000 | mtd0 uboot (0x380000) | STG2 (0x41000) | S2 + payload 0x1b079 + sum + STG2 hdr, ends 0x1b0e3 |
| 0x01c000-0x040000 | | | **blank 144 KiB** |
| 0x040000 | | **TOC** (9 entries, 0x138 B) | rest of the sector blank |
| 0x041000 | | DT 0x2 (0xf000) | board config FDT 0xb50d B, `board-cfg id` CS8210 |
| 0x050000 / 0x05c000 / 0x068000 | | 0x510 / 0x520 / 0x530 | SerDes PCIe FW 0x57d0, non-PCIe FW 0x7570, SBus master FW 0x22a4 (header id field 0, not checked) |
| 0x070000 | | STG3 (0x120000) | 0x7c539 B, ends 0xec585; **0x0ed000-0x190000 blank 652 KiB** |
| 0x190000 | | UBOOT 5 (0x1f0000) | 0xe8040 B, ends 0x27808c (load = exec 0x01100000) |
| 0x278090-0x380000 | | (inside UBOOT max) | **orphans** of an Annapurna reference image, not in the TOC (never run): tail of the generic al-boot v3.65.1 release U-Boot (HAL build paths `…/alpine-v3-al-boot-v3.65.1-build/…`, strings 0x279479-0x2ba8b8; longer than Celestica's, which overwrote its head) to 0x2c9000, UBOOT_SCRIPT 3.65.1 @0x2ce000 (valid), IPXE 1.4.1 @0x2e0000 (valid), BOOT_APP 6.9.4 @0x360000 (truncated); blank 0x2f9000-0x360000 (412 KiB) |
| 0x380000 | mtd1 uboot-env | UBOOT_ENV 7 | U-Boot env, 16 KiB, **non-redundant** (CRC32 over 0x4..0x4000) |
| 0x3c0000 | mtd2 "unused" | UBOOT_ENV_RED 8 | stale AArch64 code, not an env (CRC bad; U-Boot ignores it [inf]) |
| 0x400000 | mtd3 board_eeprom (64 KiB) | | all zero |
| 0x410000 | mtd4 onie (0x1bf0000) | | ONIE FIT 0x19a85a0 B (ends 0x1db85a0); blank tail 0x1db9000-0x2000000 (2.3 MiB), **erased by any ONIE update** (`uploadonie` erases the whole 0x1bf0000) |

The ROM-probe offsets 0x20000…0x1e0000 hold no other S2 image: there is **no stage2 B** (V2 had one).

## 2. Stages
| stage | where / format | load, entry | checks | how it finds the next |
| --- | --- | --- | --- | --- |
| Boot ROM | on-die, not dumped (HAL V3: 0xfbff0000, XMODEM receiver @0xffff1485) | | S2 sum16s [inf from V2] | S2 at SPI 0 |
| stage2 `stage2_loader v3.65.1` (k2x-n build) | S2 header (`S2`, w3 = size 0x1b079, w4 = exec 0xff000020, u16 sum @0x1e = 0x02ce), payload, u16 sum, al-boot STG2 hdr at 0x1b09b (size = 0x1b09b). No RSA blob | ROM loads to 0xff000000, enters 0xff000020 (A32) | none on itself | TOC search, instance from SRAM 0xfbfe0000, DT (ID 2, flavour bits 23:16), PRE_BOOT_V2 (0xd, absent) else STG3 → 0x01000000 (main 0xff00f01c) |
| stage3 3.65.1 (CVOS, Nov 20 2020) | al-boot object | 0x01000000 (A32) | header sum + magic, data sum (0x0102f0cc / 0x0102f200) | BOOT_MODE, DT, 0x510/0x520 FW, APCEA, keys, app (stg3_board_init 0x01035cbc, exec 0x0103706c) |
| AArch64 agent `agent_wakeup v3.53` | inside stage3 | RVBAR CPU0 = 0x1000 | | ERET to the app [inf: V2 = EL2, x0 = 0] |
| U-Boot 2018.05-alpine_db-v3.65.1.015 (ONIE 2017.11.015) | UBOOT 5 | 0x01100000, AArch64 | | env → bootcmd (§6) |

**Stage2 flow [code]:** clocks/bootstrap → TOC search (16 × 0x20000 from 0) → instance (word at SRAM
0xfbfe0000; bit0 = 0 → STG2 TOC entry at that offset gives the instance, here 0) → uC flavour queries +
DT read (0xff00f9d4) → memory BIST/repair → secure-early flow (TPM init + DT early measurement, only if
OTP secure boot) → DDR → TPM/uC flow, SET_SECURE(OTP value) to the uC (0xff010b98) → measured boot
(`Performing measurement...`, SHA of every TOC object into the TPM, 0xff01072c) → STG3. **Any failure
(TOC search, TOC/STG3 header or data sum) prints and spins forever; there is no fallback** [code].

**Stage3 handoff [code]:** loads refuse 0xf3000000-0xf33fffff (ADV_ETH SRAM); a bad app header takes the
legacy raw path: size word at flash 0x40000 = TOC magic 0x070c070c > 1 MiB → `Invalid application size!`
and a spin (on V2 it ran stage2 B as AArch64 code). Then SPI CS/state reset, `Executing application...`,
exec → SRAM 0xfbfe00f8, APCEA flag → 0xfbfe00fc, RVBAR CPU0 = 0x1000, CPU0 reset, WFI.

## 3. Secure boot and the "secure-mode" message
- What decides it: OTP shadow word 120 bit 4 (SECURE_BOOT_EN, `al_otp_storage_layout_v3.h`), read by
  stage2 with `al_otp_storage_db_read(direct = false)` into `db.secure_boot_enabled` (0xff02bff4,
  getter 0xff010d80). Stage2 uses it only for the TPM measurement flow and to mirror it to the uC
  (SET_SECURE 0xBABA1E1E). Nothing gates the STG3 load on it [code]. Stage3 has no OTP code at all.
- **The app path [code]:** BOOT_MODE, DT and app go through `al_flash_obj_header_read_and_validate`
  (0x0102f0cc: read 0x48 B, byte-sum 0x00..0x43 == @0x44, magic 0x000b9ec7, memcpy) and
  `al_flash_obj_data_load` (0x0102f200: read data, read u32, byte-sum compare). They call only the flash
  reader and memcpy; `Loading application`/`Executing application` (0x0103706c) add the address-window
  check. No crypto/RSA call, nothing reads an OTP or secure flag. Stage3's crypto HAL (FIPS SHA test, SA
  asserts) is not on this path.
- **`secure_boot_early_flow`** (0xff010984, stage2, after the DT read and memory repair, before DDR): reads the
  OTP shadow once (0xff010d44); only if `secure_boot_enabled`: TPM init + DT early measurement. With OTP
  off it does nothing else. **After DDR** (0xff0109cc): uC GET_SECURE (0xBABA2121); a reply 0x5eca…
  selects the key-rotation flow (TPM activation, uC IS_EXIT_SECURE 0xBABA4747, optional TPM force-clear,
  `STG2 secure mode` vs uC state 0x5ecaa006/0x5ecaa002), any other reply (here: no `uC secure mode` print)
  the legacy flow (0xff0108cc): TPM init, physical presence, SET_SECURE(OTP value), then the measurement.
  Neither flow can stop the STG3 load; on errors it extends junk instead.
- **Measured boot** (0xff01072c): for each TOC entry, env objects (7/8) hash 0x10000 raw bytes, other objects
  are validated and loaded to DRAM 0x10000 and hashed (STG2 fails the object-header check and is skipped);
  32-byte digest, extended into **PCR 1 for the env and PCR 0 for everything else**. On a TPM-flow error
  (`Failing measurement on purpose`) PCRs 0..15 get a junk digest (0xff0106ac). The factory U-Boot extends
  too and fails (`TPM extend failed` at `bootm`, boot log). Any SPI or env change moves PCR 0/1: relevant only
  to secrets sealed to them (none known on NixOS).
- `Command is not supported in secure-mode.` is the HAL's generic print in `al_uc_cmd_adv`
  (`al_uc_cmds.c`) for the uC response **0xCACAFCFC** (`AL_UC_RESPONSE_SECURE_NOT_SUPPORTED`). It answers
  stage2's three optional flavour queries GET_DT_FLAVOUR 0xBABA8F8F, GET_APCEA_CFG_FLAVOUR 0xBABA9191,
  GET_PXE_FLAVOUR 0xBABA9494 (0xff010bb8/be0/c08), sent before the DT read; the first one after a cold
  start times out (uC still booting). On error the default flavour 0 is kept, so the DT lookup is plain ID 2.
  It says nothing about the SoC: OTP is off and the uC reports NOT-SECURE [hw]. *Inferred:* this uC
  firmware rejects (does not implement) the flavour opcodes with that code. Benign.
- **Read-only commands that settle it** (factory prompt, stop autoboot with `123` Enter): `otp show shadow`,
  `otp show direct` (`- Secure boot: …`, `- Public keys:`, `- Minimal SW version:`), `dev_info`,
  `uc_get_secure` (`… Secure state is NOT-SECURE`), `uc_get_secure_log`. Every other `otp` subcommand
  burns fuses ("irreversible!", most need a password): never.

## 4. DDR
- Source: the DT object `/soc/board-cfg/dram` [code: `alpine_dtobj.py`]. Two channels; per channel dimm0
  SPD at I2C **0x52** (2-byte offset 0), dimm1 SPD 0x54 / 0x55 (enabled in the DT, but the log shows only
  0x52 per channel [hw], so dimm1 is skipped [inf]); one impedance set per channel
  (`single_dimm_single_rank_per_dimm`: dram_odt 6, dram_rout 7, phy_odt 0x38, phy_rout 0x22, vrefdq 0x14,
  …); `init-pat` 0xf, POST enabled; no CRC/parity. No preload-EEPROM SPD path is used (0x57 is blank,
  `dev_info`: spis_preload, not i2c).
- Log [hw]: OTP memory repair not needed, BIST pass, ECC static poisoning at 0x1e0000/0x1e1100, 4 GB per
  channel, 8 GB total. Stage3 clears and compares 0x2000000-0xc0000000, 0x2_0000_0000-0x2_4000_0000,
  0x1_0000_0000-0x2_0000_0000.

## 5. Apps, keys, A/B, recovery
- **BOOT_MODE [code]:** stage3 looks up ID 0 (instance 0); absent (here) or bad header → app 5. Size > 0x40
  → `Boot mode too big!` → app 5. Valid header + bad data sum → `al_flash_obj_data_load failed (boot mode)!`
  → **skips the key poll and the app lookup** and takes the raw path → hang: **the DMC-12 trap exists on V3**.
  Data word +4 is the app ID (data at 0x01081884, app 0x01081888). Instance bits in the app ID survive the OR.
- **Any ID in the TOC can be the app** (5 UBOOT, 0x110 IPXE, 0x200 BOOT_APP, 0x1000+ user…); a missing one
  prints `App not found in TOC` and falls back to 5. Stage3 also reads the headers of 0x200/5/0x100/0x110
  only to put their versions into the general shared data (0x01039c10: harmless).
- **Keys [code]:** one poll of UART0 (0x01036134) after the BOOT_MODE/DT/APCEA lookups, i.e. between
  `agent_wakeup v3.53` and `Loading DT to 01100000`; it drains every queued byte. `p` skip APCEA, `e` keep
  early APCEA, and `p`/`e`/`u` all force app 5 (`User request for loading U-Boot!` when the app was ≠ 5).
  Today (no BOOT_MODE, no APCEA) they do nothing visible except `p`'s message. DMC-12 timing that worked:
  send every 20 ms from stage2's `Executing next at 0x01000000!`.
- **A/B:** one instance only; stage2 B does not exist; no watchdog-driven fallback [code].
  `flash_contents_instance_invalidate 0` would leave the ROM with no S2 at all: never.
- **UART / RAM paths [code]:** stage3 and U-Boot keep the "UART recovery" reader (flash offset X read from
  DRAM 0x01800000 + X) when stage2 left SPI_LOAD_ENFORCE clear and XMODEM_LOAD_ENFORCE set, or the boot
  strap says UART. The V3 ROM has an XMODEM receiver (HAL). So DMC-12 Route A (production stage3 from a RAM
  image) and Route B (patched stage2 over XMODEM) are possible in principle; **neither is ported**: the V3
  SRAM layout (0xfbfe0000), the CPU reset registers and the ROM behaviour must be redone. Strap pins: unknown.
- **What protects the unit today:** nothing but an external 1.8 V programmer for a broken TOC/STG2/STG3/UBOOT.

## 6. Env and what boots NixOS (mtd1, `analysis/boot-chain/envdump.py`, keys redacted)
- 114 variables; between 11-09 and 11-15 only `nos_bootcmd` changed (`echo` → `scsi init; ext4load scsi
  0:2 0x07000000 /boot/boot.scr; source 0x07000000`). The env holds ONIE's dropbear private host keys: never
  commit it.
- `bootcmd = run boot_diagos; run check_boot_reason; run nos_bootcmd; run onie_bootcmd`, `bootdelay 3`
  (stop: `123` Enter). DIAGOS only with `onie_boot_reason=diag`; any other boot reason → ONIE; else
  `nos_bootcmd` → SSD p2 `/boot/boot.scr` → "CS8210 NixOS bootmenu" → kernel + uImage initrd + DTB
  @0x10000000 (`serial-consoled-20261115-boot.txt`). If that fails, ONIE from SPI 0x410000 (`bootm`).
- No UBOOT_SCRIPT in the TOC (`U-Boot script not found in TOC!`), so no script-driven `saveenv`. Keep the
  orphaned UBOOT_SCRIPT at 0x2ce000 out of any TOC: it is the reference "boot_app_toc" updater.
- U-Boot takes the env location from the TOC (IDs 7/8, 0x011020e4), with the non-redundant format.

## 7. Where a replacement U-Boot goes
What stage3 accepts [code]: an al-boot object (0x48 B header, magic 0x000b9ec7, header byte sum @0x44,
`size/load/exec` u64 @0x28/0x30/0x38; data; u32 data byte sum); load anywhere in DRAM except
0xf3000000-0xf33fffff and what it overwrites itself (stage3 at 0x01000000; DT/FW scratch at
0x01100000-0x0113xxxx are used before the app load, so 0x01100000 is fine, as the factory U-Boot shows);
exec = AArch64 entry. Size limit = the TOC entry's max. Wrap with `alboot.py wrap --id <ID> --load …`.
Keep every object below 16 MiB (stage3 reads through the SPI mmap at 0xf8000000 + offset [code]).

**A' (recommended SPI scheme): TOC2 at 0x20000.**
- TOC2 = the 9 factory entries verbatim + BOOT_MODE (ID 0) at 0x30000 (max 0x1000, blank) + the new app,
  e.g. IPXE 0x110 (or UBOOT instance 1, app ID 0x10000005), in blank flash: **0x0f0000-0x190000 (640 KiB,
  blank today, inside STG3's declared max)** or, for more room, 0x280000-0x380000 (1 MiB, after erasing the
  orphans; then give UBOOT 0 max 0xf0000 in TOC2).
- Why it is safe [code]: stage2 (0xff00b3ec/0xff00b340), stage3 (0x0102ed48) and U-Boot (0x01134b78) all
  search 16 × 0x20000 from 0 and take the first TOC that validates; stage2's validator (read) rejects a bad
  magic, header sum or entry sum, and stage3/U-Boot call the same HAL routine (bodies not re-read). Order: write app → verify; write
  BOOT_MODE → verify; write TOC2 last → verify. Until TOC2 is valid nothing references the new objects; a
  torn TOC2 is skipped. Undo = `sf erase 0x20000 0x1000`.
- Escape: with BOOT_MODE ≠ 5, `u` (or `p`/`e`) in the stage3 poll boots factory UBOOT 0. A bad app header or
  data hangs the box, but the next boot with `u` gets out. A **bad BOOT_MODE data sum hangs every boot with
  no escape** (§5): verify BOOT_MODE byte for byte before TOC2 exists. The factory `flash_contents_*`
  commands then operate on TOC2. `flash_contents_boot_mode_default_app_set` needs an existing valid
  BOOT_MODE (V2 [code]); write the first one as a wrapped object.
- Unverified: that a TOC2 at 0x20000 never looks like an S2 image to the V3 ROM (its word 3 is 0, which the
  V2 ROM rejects); only matters if stage2 at 0 were bad. Measured-boot PCRs change (no boot risk).
- Rehearsal: a Route A port for V3 (RAM image of flash 0..0x800000 incl. TOC2) would test the exact layout
  with no write; not built.

**B (replace UBOOT 0 at 0x190000, max 0x1f0000).** Preconditions: a 1.8 V programmer and a clip/pogo plan
for the flash (package unknown), a programmer read of the chip first. Risk: no escape at all (`u` loads
the same slot); a bad object hangs every boot; it also removes the factory tooling (`flash_contents_*`,
`otp`, `uc_*`) and the ONIE/DIAGOS env logic.

**C (chainload from the SSD, status quo).** The factory U-Boot runs `nos_bootcmd` → `boot.scr`, which can
load our U-Boot into DRAM and jump to it (DMC-12 did this first). No SPI write, nothing can brick. Costs:
the factory U-Boot always runs first (~3 s autoboot window plus its init; the fans stay loud until Linux
either way), our U-Boot inherits a warm state (PCIe/SerDes up), and `bootm` has a size limit (the 71 MB
kernel did not fit; a U-Boot does).

## 8. Read-only checks for the main agent (factory prompt)
- `flash_contents_toc_print` (expect the 9 entries of §1), `flash_contents_obj_validate UBOOT 0` (silent rc 0).
- `md.l 0xfbfe0000 4`: expect word 0 = `00000000` (stage2 from offset 0). `md.l 0xfbfe00f8 2`: expect
  `01100000 00000000`.
- Boot ROM dump for the V3 Route B: `md.l 0xfbff0000 0x40` first (expect code, not `bad0add1`), then
  `crc32 0xfbff0000 0x10000` and an `md.b` capture saved under `analysis/` (vendor code, git-ignored).
  U-Boot only; never `/dev/mem` from mainline Linux (SError panic, PROGRESS mistakes).
- Key test, no write: send `p` every 20 ms from `Executing next at 0x01000000!`; expect
  `User request for skipping APCEA!` between `agent_wakeup v3.53` and `Loading DT to 01100000`.

## 9. Compared with the DMC-12 (Alpine V2)
| point | DMC-12 (V2, 2.60.x) | CS8210 (V3, 3.65.1) |
| --- | --- | --- |
| stage2 | A32/Thumb @0xf2200000, exec +0x20; two instances (A k2 build, B generic ep), A signed blob present | A32 @0xff000000, exec 0xff000020; one instance, no RSA blob |
| ROM state / handoff SRAM | 0xfbff4000 / 0xfbff40f8 | 0xfbfe0000 / 0xfbfe00f8 (PBS int mem) |
| TOC | 0x80000 | 0x40000, 9 entries; same format and search |
| objects | APCEA, IPXE ×2, BOOT_APP, UBOOT_SCRIPT, EEPROM, env ×2 (redundant) | DT, SerDes FW 0x510/0x520/0x530, STG3, UBOOT, env (non-redundant; ENV_RED slot holds junk). No BOOT_MODE/IPXE/APCEA/script/EEPROM |
| DT lookup | ID 2 | ID 2 with a uC-supplied flavour in bits 23:16 (default 0) |
| DDR | DT dram or preload-EEPROM SPD | DT dram, SPD 0x52 on both channels, impedance in DT |
| preload | I2C EEPROM 0x57 | SPI-slave preload (`dev_info`); 0x57 blank |
| signatures | none checked; OTP off | none checked; OTP off [hw] |
| uC | mirror of OTP secure state, `NOT-SECURE` | same mirror; additionally answers flavour queries with 0xCACAFCFC |
| keys `p`/`e`/`u` | proven on hw | same code; untested on hw |
| bad app header | raw path ran stage2 B as AArch64 (silent death) | raw path reads TOC magic as size → `Invalid application size!` spin |
| BOOT_MODE data trap | hang, no escape | same |
| recovery | ROM instance fallback to stage2 B, XMODEM proven from ROM code, Route A proven | no stage2 B; ROM not dumped; Route A/B not ported |
| spare app slots | IPXE ×2 blank | none; TOC2 at 0x20000 instead |

## 10. Open
- V3 boot ROM: dump it read-only (§8) to confirm the S2 checks, the probe offsets and the XMODEM fallback.
- Handoff EL and registers on V3 (agent not disassembled; V2: EL2, MMU off, x0 = 0).
- Where the boot-device straps are, and whether the uC can select another flash (no `uc_*_recovery` use seen).
- Why the stage2 banner printed twice on the very first AC power-on of 11-09 (as on the DMC-12's first boot).
- `alboot.py` gaps for V3 (owner of `nix/packages/alboot`): list objects beyond a single mtd as "outside
  image" rather than `blank(ff)`; non-redundant env (4-byte header); `find_toc` should skip a TOC whose
  entry sum fails and report every valid TOC (a TOC2 shadows 0x40000); names for 0x510/0x520/0x530; an id
  mismatch is not an error for stage3; a `toc` builder for scheme A'.

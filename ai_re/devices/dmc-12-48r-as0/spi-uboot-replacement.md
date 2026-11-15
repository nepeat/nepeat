# Replacing the boot U-Boot in SPI NOR: boot chain, object format, plan

Static analysis only (2026-11-15). Nothing was written to flash. Hardware facts quoted from
the read-only answers in [spi-recovery.md](spi-recovery.md) ("Hardware answers")
(the sibling file for recovery, watchdogs, OTP, CS1 and programmers; not repeated here).

**Sources.** `dumps/u01/mtd{0,1}.bin.zst`. Stage2 A (`stage2_loader_k2 v2.60.3`, the one
u01 boots) and B, as ARM/Thumb at 0xf2200000. Stage3 2.60.4 (A32) at 0x01000000. Its AArch64
`aarch64_resume_agent` at 0x1000. The factory U-Boot is in `ghidra/dmc12.gpr`. To rebuild
the scratch project: carve the objects with `alboot.py`, then run
`tools/ghidra/import-raw.sh <img> <dir> stg <base> ARM:LE:32:v8` and
`tools/ghidra/seed_functions.py … entry=<base+0x20 for stg2, base for stg3>`. The agent is
`AARCH64:LE:64:v8A` at 0x1000. Addresses below are in those images. *Inferred* means read
from code shape and never observed.

## Verdict
**Feasible, and no signature or OTP is in the way.** But don't overwrite `UBOOT` instance 0.
Install our U-Boot as a new app object in a **blank slot** (IPXE, 512 KiB, instances 0
and 1), then point the blank **BOOT_MODE** object at it. Holding `u` on the console during
stage3 then loads the untouched factory U-Boot (§6). The port itself needs three pieces
that only the factory U-Boot does today:
- the external PCIe link (USB serial ports);
- the MUIO pinmux (SPI CS1/CS2, UART1);
- the CCU writes that `bootm` makes.

The image is our existing u-boot.bin, linked at 0x08000000.

## 1. Object format (`alboot.py`, output `inventory/u01/alboot-objects.txt`)
Every checksum is a plain 32-bit **byte sum**. There is no CRC and no hash.
`alboot.py wrap` rebuilt the factory UBOOT object byte-identical to flash 0x1c0000.

- **TOC** (`al_flash_toc_validate`, stage3 0x01025d98 / stage2 0xf22084b8). Found by
  probing 16 offsets 128 KiB apart from 0 (`al_flash_toc_search`); both chips have it at
  **0x80000**.
  - Header, 0x14 B: magic `0x070c070c`, rsvd, `count` @+8, rsvd, and @+0x10 the byte sum of
    bytes 0..0xf.
  - `count` entries of 0x20 B: `id | instance<<28`, `name[8]`, `device` (0 = current),
    `offset`, `max_size`, two reserved words.
  - Then a u32: the byte sum of all the entry bytes.
- **Object header**, 0x48 B (`al_flash_obj_header_read_and_validate`, 0x01026230):

  | off | field | notes |
  | --- | --- | --- |
  | 0x00 | magic `0x000b9ec7` | |
  | 0x04 | reserved | 0 |
  | 0x08 | object ID | no instance bits; not checked against the TOC |
  | 0x0c–0x14 | version major/minor/sub (u32 each) | e.g. 2.60.4; the DMC-12 DT and stg2 A say 0.0.0 |
  | 0x18 | description char[16] | "N/A" |
  | 0x28, 0x30, 0x38 | size, load, exec (u64) | |
  | 0x40 | flags | 0 everywhere |
  | 0x44 | header checksum | byte sum of 0x00..0x43 |

  The data follows at +0x48, then a u32 byte sum of the data
  (`al_flash_obj_data_load`, 0x0102636c).
- **STG2 / PRE_BOOT / PRE_BOOT_V2** (ID 1/0xc/0xd) put the boot-ROM image first:
  - **`S2` header** (0x20 B): word 3 = payload size, word 4 = exec (0xf2200020; the image
    is loaded at 0xf2200000). Words 1, 2, 5, 6 are 0. A u16 at 0x1e = byte sum of
    0x00..0x1d.
  - Then the payload and a u16 byte sum of it.
  - Then, on signed images, a **0x200 B RSA-2048 blob**: 0x100 identical in every build (a
    public key) + 0x100 build-specific (a signature).
  - Last, an al-boot STG2 header whose `size` = everything before it, with no data sum.
  - mtd1's instance 1 has no blob. The ROM-side checks are *inferred*: the ROM is not
    dumped.
- **CS0 instance A is not a mirror of B.** stg2 A is a 0x17447 B `k2` build. Past its end,
  up to 0x202bb, the sector still holds the tail of an older `stage2_loader_ep` 2.60.4
  image (dead bytes). stg2 B is that generic EP build. dt.1 is the generic K2C-AB DT.
- Env objects are raw U-Boot env: 0x4000 B, redundant (flag byte), CRC32 OK. The active
  copy is 0xfe0000 (flag 1).
- Blank on CS0: BOOT_MODE, 2ND_TOC, IPXE ×2, APCEA ×2, BOOT_APP_CFG ×2, EEPROM ×2.

## 2. Who loads what
1. **Boot ROM** loads `S2` from SPI CS0 offset 0 into SRAM 0xf2200000.
   - It leaves **0xfbff4000** = `00000000` on u01 (bit0 = 0: "loaded from offset 0").
   - Then `f2200020 f2200020 fd883000` follow.
2. **Stage2** (main 0xf220bd88): TOC search, then the instance from 0xfbff4000
   (0xf2208718). Offset 0 matches the TOC's STG2 entry 0, so active = fallback = 0.
   - Loads DT 0 (`dt_read_from_flash`, 0xf220c644) and applies its `gpio-list-s2` (absent here;
     0xf220c818), then DDR init and the TPM flow.
   - Then **measured boot** (0xf220d1a4, "Performing measurement"), see §5.
   - Then PRE_BOOT_V2 0, else STG3 0: header + data sum, load 0x01000000.
     `Executing next at` jumps there (0xf220bcc8).
   - Any failure prints and **spins**.
3. **Stage3** (A32, main 0x010296b4):
   - Copies the AArch64 agent (stg3 +0x48ccc, 0x6dc9 B) to **DRAM 0x1000**, then runs
     `stg3_board_init` (0x010285a8):
   - **BOOT_MODE** (ID 0, instance 0 only). If valid, its data word +4 is the app ID (data ≤
     0x40 B). If blank (as on u01), the app is **5 = UBOOT**.
   - DT instance n, APCEA (0x100) n, SERDES_25G_FW (0x500) n.
   - **Console keys**: one non-blocking check of the UART0 RX FIFO, then the app lookup.
     `p` = skip APCEA (it prints "User request for skipping APCEA!"). `e`/`u` = force app 5
     (it prints "User request for loading U-Boot!" only when the app wasn't 5 already).
     Stage3 keeps reading for as long as characters keep arriving.
   - **App** = `find_id_with_fallback(app | n<<28)` (0x01025f68). If the app isn't in the
     TOC, it falls back to app 5. Any instance bits in the BOOT_MODE app ID survive the OR:
     `0x10000110` = IPXE instance 1.
   - Then `Loading DT to 01100000`, the DT-driven init (§4), dram clear/compare and the
     trace move, all before the app is loaded.
   - **Exec** (0x010292f8): `Loading application to <load>`. Loads refused:
     0xf2200000–0xf22fffff. Then caches/MMU off and CCU/NB writes (§3); sets
     `0xfbff40f8 = exec`, `0xfbff40fc = APCEA flag`; sets CPU0's RVBAR to 0x1000 with the
     AArch64 bit (0x01023e9c), core warm reset (0x01023da0), WFI.
   - Instance n is the stage2 instance, **for every object**. Nothing tries the other
     instance on a checksum failure.
4. **On a bad app object** (correcting [spi-recovery.md](spi-recovery.md)'s "goes on
   without an app"):
   - **Bad data checksum:** "al_flash_obj_data_load failed", then a spin.
   - **Bad header:** stage3 takes its **legacy raw path**. It reads a size word at flash
     0x40000, and if it is ≤ 1 MiB it copies from 0x40004 to 0x01100000 and executes it
     (0x0102958c–0x01029624).
   - On this flash 0x40000 is stg2 B's `S2` header (0x3253). So stage3 prints `Loading
     application from 00040000 … (12883 bytes)` and AArch64 runs A32 code. It dies
     silently at EL2.
   - The same happens when stage3 finds no TOC (and the DT then comes from 0xc0000).
5. **Agent** (EL3, 0x1000 → 0x5930 → 0x5f58): installs the SMC handlers and the GIC.
   - SMC handlers: PSCI 0x84xxxxxx, SiP 0x82000000–04.
   - GIC secure-side init: SRE_EL3 = 0xf, PMR 0xff, redistributor woken, SGIs/PPIs in group 1 NS.
   - Sets CNTFRQ_EL0 and CPUECTLR.SMPEN. The secondaries park in WFI.
   - ERETs with **SCR_EL3 = 0x535** (NS, HCE, RW, FIQ to EL3) and **SPSR = 0x3c9**
     (EL2h, DAIF masked) into a stub that calls the app (0x5634).
6. **Factory U-Boot** (`board_init` 0x01102334): its own TOC parse, then DT n into
   `fdtaddr`, the env and `UBOOT_SCRIPT`, then the init listed in §4.

**Why the stage2/stage3 banner shows twice in `serial-20261115-initial-boot.txt`:** the
first pass dies silently right after `Trace final` (factory `board_init`, before `NAND:`).
Nothing prints "Resetting the board!", and every firmware-initiated reset on this path
prints it first (`__wd0_board_reset`, `reset`). So it was an external reset (CPLD, uC or
PSU).
- It happened only on that very first power-up after transport. Twenty later boots in
  `serial-20261115-025824.txt`, two power cycles in `-021915.txt`: one pass each.
- So it is not a firmware "reset once" step.
- *Inferred:* uC/CPLD power-up sequencing on a cold AC start. Question 5 below would tell.
- It changes nothing here, except that writes must never sit in early boot.

## 3. Handoff ABI: stage3 → app (what our U-Boot sees)
- **AArch64, EL2 (non-secure), SPSR 0x3c9, MMU/D-cache/I-cache off** (stage3 turns them off
  before the core reset; the EL2 regime is untouched).
- **x0 = 0** (the per-CPU context ID; PSCI `cpu_on` uses the same slot), x1–x3 = 0. **No
  DT is passed.** "Loading DT to 01100000" is stage3's own board config. The app then
  overwrites it at the same address.
- PC = header `exec`, image at header `load`. Any DRAM address works, except stage2's
  SRAM window.
- Already done:
  - DDR trained, ECC'd, cleared 0x02000000–0xc0000000 and 0x2_0000_0000–0x2_4000_0000.
  - SerDes groups (group0 pcie_g3x4, group1 sata), the internal PCIe unit adapters.
  - **al_eth board params** for port 1 (0x01028fc4, "al_eth_board_params_store"). That
    settles question 1 of uboot-vendor-commands.md.
  - The trace buffer at 0x02004000, general shared data, the GIC (secure side).
  - CNTFRQ. PSCI v0.1 stays resident at EL3 (`cpu_on` 0x84000003, `cpu_off` 0x84000002)
    in DRAM 0x1000–~0xa000: keep 0x0 +1 MiB reserved.
- **Undone before exec:**
  - CCU 0xf0094000 ← 0 (cluster snoop) and 0xf0090004 ← 0; NB 0xf00700d0 bit 30 cleared.
    The factory `bootm` re-sets 0x4000←1 and 0x4←7 (live after it: 0xc0000001 / 7).
  - SPI master 0xfd882000 and 0xfd8be100: +0xf4 ← 0xf.
- The sp805 is not armed (spi-recovery.md §3).

## 4. What our U-Boot must do in place of UBOOT
The DMC-12 DT (dt.0 `board-cfg`) has no `sgpo_init`, no `i2c_pld/gpio` expander, no switch
and no `rc-manual-init`. So the factory U-Boot's own init boils down to:

| # | item | factory does it in | needed? |
| --- | --- | --- | --- |
| 1 | **External PCIe port 0 RC** (0xfd800000 regs, ECAM 0xfb600000, mem 0xc0010000): port config gen1 x1, link start + wait (`pci_init_board_external`, same code as `al_pcie_ext`) | U-Boot init (`PCIE_0: Link up 2.5GT/s x1`) | **yes**: the Pericom bridge → OHCI/EHCI → all 48 ports. Mainline `pcie-al` assumes the link is up. **Built and RAM-tested on u01** (patch 0022, Annapurna HAL 3.4 verbatim; teardown + our init, then NixOS on that link, [u-boot-port.md](u-boot-port.md#self-init-build-pcie-muio-ccu-read-only-spi-ram-tested-on-u01)) |
| 2 | **MUIO pinmux**: `if_uart_1`, `if_spim_a_ss_1`, `if_spim_a_ss_2` (PBS 0xfd8a8000) | `dt_based_init_pinctrl` | **yes** for SPI CS1 (mtd1) and CS2 (SPI TPM); UART1 (LCD) optional. **Built**: DT-driven replay of those 4 pins (0025/0026) |
| 3 | CCU 0xf0094000←1, 0xf0090004←7 | factory `bootm` (DT `io_coherency`) | recommended in `board_prep_linux`, so Linux sees today's tested state. Our Linux DTS is non-coherent, so it is not strictly needed. **Built** (0025) |
| 4 | `gpio_init` (GPIO 22 high) | the factory U-Boot only: k2 stage2 reads `gpio-list-s2`, which the DT lacks (0xf220c818) | **yes**, if GPIO 22 matters (CPLD JTAG-related); one `gpio` write. **Built** (0027/0028, not yet run) |
| 5 | Fans/WDT flag (CPLD 0x2a←0, 0x50←0x2a), LED | env preamble | optional: fans stay at full speed (fail-safe) until NixOS fancontrol. Could go in `board_late_init` |
| 6 | DN0 SATA SerDes fix (`serdes wr 1 p0 pma 21 2 0 5` if 0xfb600000 = 0x8241104c) | env preamble | per unit, **only after item 1** (it reads the external PCIe ID). u01 has none |
| 7 | MAC into EC entry 0 | factory al_eth | done: patch 0018 (EEPROM MAC); params are stage3's |
| 8 | uC FRU copy, thermal sensor power-up (0xfd860a00) | `misc_init_r` / `board_init` | no; thermal only if Linux gets `al-thermal` |
| 9 | `UBOOT_SCRIPT`, factory env | `misc_init_r` | **must not**: our U-Boot ignores both, which also removes the BOOT_APP reroute hazard on our path |
| 10 | DT clock fixups, disable unused `pcie-external` | `ft_board_setup` | no: our DT has fixed clocks |

## 5. A `dmc12-spi` variant (changes to the series)
- **Link/load: no change.** Keep `TEXT_BASE 0x08000000`, PIE, `INIT_SP_RELATIVE`. Wrap
  with `alboot.py wrap --id 0x110 --load 0x08000000` (or `--id 5`).
  - u-boot.bin is 470432 B today: it fits IPXE's 0x80000 slot with 52 KiB to spare, and
    UBOOT's 0xe0000.
  - The first PIE fixup-free run is then byte-for-byte the chainload-tested binary.
  - 0x01100000 is the proven fallback load address. *Unverified:* that stage3's A32 MMU
    map reaches 0x08000000 for its memcpy (the dram clear covers it).
- **Entry:** `save_boot_params` (0014) already handles MMU off, and x0 = 0 is only printed.
  Expect `Entry: EL2, MMU+caches were off, x0=0`.
- **DT:** keep the built-in `OF_SEPARATE` DT, not stage3's DT object (that one is a vendor
  board-config tree, not a Linux DT).
- **Env: keep `ENV_IS_NOWHERE`.** Never `0xfd0000`/`0xfe0000`. That env (0x4000 B,
  redundant, holds `boot_app_toc`) must stay intact for the factory U-Boot that the `u`
  escape boots. If persistence is ever wanted, use a file on the SSD.
- **SPI flash:** built read-only (patch 0024, `#uboot-dmc12-selfinit`): `sf probe`/`sf read`
  only, no write/erase/lock code, no SR write at probe (unlike the factory `sf probe`,
  spi-recovery.md §5), non-read SPI commands refused. For proving the address map with
  `sf read` + `crc32` against the backups. Writes stay with the factory U-Boot (§6) or
  Linux mtd.
- **Built** (2026-11-15; items 1–3 pass the u01 RAM test, chainloaded after the factory U-Boot): items 1–3 and the IPXE
  object (`#uboot-dmc12-stage3app`, 4404 B left in the slot); details, tests and the RAM
  test steps in [u-boot-port.md](u-boot-port.md#self-init-build-pcie-muio-ccu-read-only-spi-ram-tested-on-u01).
  Item 5 (fans/WDT flag) is not done.

## 6. Signatures, secure boot, TPM
- **No object is authenticated.** The flash-contents library has the code ("Authenticating
  PUK", "Comparing PUK hash to efuse", `SIG_`/`PUK0-2` IDs), but no code in stage2 A/B or
  stage3 references those strings. The only checks are the byte sums.
- The ROM-level RSA blob covers stage2 only, which we don't touch. OTP on u01: secure boot
  off, PUK hashes zero, min SW version 0 (spi-recovery.md).
- **On a secure-fused unit** a UBOOT change would still pass these stage2/3 builds. But
  such a unit would likely ship a different stage2 that does call the auth code.
- **Per-unit check:** `alboot.py list` must show no unknown object IDs (SIG/PUK objects)
  and `otp show` secure boot off. Otherwise stop.
- **Measured boot:** stage2 SHA-1-hashes every valid TOC object except STG2, including both
  env sectors, and extends a TPM PCR. "Failing measurement on purpose" extends 0..15 with
  junk. A new U-Boot (and every env edit, already done by reimage) changes the PCRs. That
  only matters for secrets sealed by the factory OS. It is no boot risk.

## 7. Flashing concept (owner decision; implemented as `spi-upgrade.py`, with `sf erase`/`sf write`)
The factory `flash_contents_obj_update <OBJ> <inst> <addr> <size>` (0x1104ecc → 0x011043a4):
- Looks the slot up in the TOC and refuses anything over `max_size`.
- Erases only the 4 KiB sectors it covers (it keeps the partial head/tail sectors), then
  writes the bytes **as given**: no header, no checksum, no verify. So the input must be
  the wrapped `.fimg`.
- For STG2/PRE_BOOT it writes byte 0 last, so a torn write invalidates the `S2`. Other
  objects get no such care.
- `flash_contents_obj_validate <OBJ> <inst>` re-checks header + data sum (rc 0, silent).
- `flash_contents_boot_mode_default_app_set` **fails on a blank BOOT_MODE**: it loads the
  old object first (0x01104d14). So the first BOOT_MODE must be a wrapped object too:
  0x40 B data, word +4 = app ID.

**Proposed order** (each step only after the previous one is checked; the AS7760 rules
apply: stage old and new in RAM, CRC both, `cmp.b` flash against the backup before
erasing, read back before any reset, a scripted flasher with `--dry-run`, an independent
review first):
0. **No write.** Prove the escape: power-cycle while sending `p` continuously; expect
   "User request for skipping APCEA!" between "agent_wakeup" and "Loading DT". If it
   doesn't appear, the plan stops here.
1. `sf read` CS0 → `cmp.b` with `mtd0.bin`. TFTP the `.fimg`, then `crc32`.
   `flash_contents_obj_update IPXE 0 <addr> <size>`, then `flash_contents_obj_validate
   IPXE 0` and an `sf read` + `cmp.b` of the slot.
   - Nothing boots IPXE yet: a bad write here is harmless.
   - The blank IPXE 1 at 0x580000 can hold a second copy for our own A/B.
2. Write BOOT_MODE (app 0x110): the first 4 KiB sector of its 64 KiB slot at 0x340000. A torn write
   leaves it invalid, and stage3 then boots the factory UBOOT. Validate + `cmp.b`.
3. Cold boot. Stage3 loads our U-Boot from IPXE 0.
   - **Bad object or hang:** power-cycle while sending `u`. Stage3 loads the factory
     UBOOT 0, which is untouched.
   - Then `flash_contents_boot_mode_default_app_set UBOOT` (BOOT_MODE is valid now), or
     `sf erase 0x340000 0x1000`.
4. Later updates go to the inactive IPXE instance, then flip BOOT_MODE (0x110 ↔ 0x10000110).
   **Never write UBOOT 0, STG2, STG3, DT or the TOC.**

Writing UBOOT 0 directly (the AS7760 way) would remove the escape: a bad header leads
into the raw-0x40000 crash, and recovery is a 1.8 V programmer only.

## 8. Read-only questions for the main agent
At `ALPINE_DB>` right after `./powercycle_to_uboot.py` (before any `bootm`/`dhcp`):
1. `md.l 0xf0090004 1; md.l 0xf0094000 1; md.l 0xf00700d0 1`: the CCU state the factory
   U-Boot got (expect 0 / 0 / bit 30 clear), versus 7 / 0xc0000001 after `bootm`.
2. `md.l 0xfd8a8000 0x80` (PBS/MUIO, for the pinmux replay) and `md.l 0xfd8b4000 0x10`
   (SGPO). Also the GPIO 22 bank: `gpio status` if available. Save the output to
   `inventory/u01/` (no identifiers expected; check first).
3. `md.l 0xfbff40f8 2`: expect `01100000 00000000` (the exec handoff).
4. `md.l 0xfd800000 0x40` (external PCIe port 0 after link-up, for the HAL port) and
   `pci 1` (bus 1).
5. Double pass: one more cold start after **≥ 2 min AC off**, capturing with timestamps
   (`tio -t`). Does the second pass recur? Then `uc_watchdog_get_status` and CPLD
   `i2c dev 4; i2c md 0x68 0x2a 1`.
6. Step 0 of §7 (send `p` during stage3). Does stage3 see the key? (No flash write.)

## 8a. Answers (u01, 2026-11-15; raw: `inventory/u01/spi-uboot-regs.txt`)
- **Step 0 passed:** `p` sent during stage3 printed "User request for skipping APCEA!", so stage3
  reads the console and the `u` escape is reachable. Timing on this cold boot: stage2 at 2.1 s, stage3 at
  3.3 s, app load at 4.2 s after power-on; a 50 ms key repeat from the stage3 banner was enough.
- **One pass** after 130 s with AC off (no double stage2/U-Boot): the double banner seen before is
  not a regular cold-boot event.
- CCU at the factory prompt, before any bootm: `0xf0090004 = 0`, `0xf0094000 = 0xc0000000`,
  `0xf00700d0 = 0x001d0102`. After the factory bootm (our chainload): 7 and `0xc0000001`. So an SPI
  U-Boot must do the CCU writes itself.
- `0xfbff40f8 = 01100000 00000000` (stage3's app address, as predicted).
- External PCIe port 0 is already up at the factory prompt: `pci 1` lists 12d8:400c (after a CRS
  wait), i.e. the factory U-Boot's init links it. This is the main work for an SPI build.
- MUIO/SGPO/PCIe-controller register dumps are saved (`md.l 0xfd8a8000 0x80`, `0xfd8b4000 0x10`,
  `0xfd800000 0x40`) for replay.
- CPLD WDT flag `0x2a = 0x01` after a true cold boot (0x03 only after a CPLD reset); uC watchdog
  still reset count 0.

## 9. Open questions / risks
- The al_pcie RC init works from a torn-down port on u01 (RAM test), but always after
  the factory U-Boot had already brought the port up once since power-on; a cold port
  straight from stage3 is untested until the IPXE object boots.
- IPXE's 512 KiB caps U-Boot growth. Fallback: UBOOT 1 with app `0x10000005`, which also
  changes what the instance-1 boot runs.
- That the `u` key reaches stage3 (UART FIFO timing) is untested; step 0 covers it.
- Stage3's DRAM map at 0x08000000 is unverified, so keep 0x01100000 ready.
- Nobody pets the uC watchdog (10 s); it has never fired. Watch for it at our `=>` with
  no factory code before it.

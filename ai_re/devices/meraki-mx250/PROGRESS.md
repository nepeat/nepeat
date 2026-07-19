# Meraki MX250 — RE

Goal: custom firmware / root recon. **Reality check: this unit has a real hardware root of trust + signed verified
boot** (opposite of the MS320) — see assessment below.

Entry point. Break out into siblings (`uart.md`, `flash.md`, `secureboot.md`, `research.md`) when this grows.

## Latest status
- **2026-07-18**: Full boot log captured (`bootlog.txt`, 1472 lines) + board photos. Architecture fully mapped:
  x86 Xeon-D, **coreboot → U-Boot verified boot → signed FIT (ECDSA P-384) → Linux 4.4**, Cisco **Trust Anchor
  Module (TAm)** hardware secure boot, locked `<Meraki>` console. OS/rootfs lives on a **USB-attached disk** (/dev/sda).
- **Key strategic finding**: secure boot makes *running* custom firmware hard, BUT the rootfs is on a pluggable
  USB disk → **binary analysis is easy here** (the very thing that was blocked on the MS320). See assessment.
- **2026-07-19 hardware session**: dumped **U118** (8 MiB, FPGA/TAm opaque) and **U15** (4 MiB, Intel ME+GbE) with
  CH341A + SOIC-8 clip, each verified by double-read. **U49** confirmed **crypto/auth (undumpable)** — tried both
  flashrom (SPI, not found) and ch341eeprom (I²C, all-0x00 at every addr/size). Remaining: **U97** (coreboot/U-Boot —
  needs SOP-16 clip) and the **USB storage** (Phison, OS image — USB-tap project). Storage is soldered (Phison
  PS2251-03 + Toshiba NAND). Devshell gained `flashrom` + `ch341eeprom`.
- **2026-07-19 analysis**: firmware-analysis subagents running on `u118-dump1.bin` (FPGA/TAm) and `u15-dump1.bin`
  (Intel ME/GbE) — stock vs Cisco-custom + anything interesting. Findings → `u118-analysis.md` / `u15-analysis.md`.

## Identity (from `bootlog.txt`)
- **Model**: Cisco Meraki **MX250**, internal codename **"Monsters"** (`MERAKI_BOARD=monsters`, "Cisco Monsters/Monsters").
- **Platform class**: `board:x86-gen2`, assembly `600-56020` / PCB `630-56030-D`, coreboot board "Intel Camelback Mountain".
- **CPU**: Intel **Xeon D-1530 @ 2.40 GHz** (Broadwell-DE, `_BDX-DE_`, 4 cores, family 6 model 0x56).
- **RAM**: 8 GiB (U-Boot sees 4 GiB per channel).
- **BIOS**: **coreboot** — `BIOS meraki-monsters 08/13/2021`, ACPI tables tagged COREBOOT.
- **Kernel**: **Linux 4.4.177-meraki**, OpenEmbedded build, gcc 9.5, `#1 SMP Wed May 29 2024`, build tag `wired-18-2-2`.
- **Same physical unit** (confirmed by user): board-photo SN `Q3SW-T3T3-DSGK`/MAC `…C8` vs boot-log SN
  `Q2SW-T3T3-DSCR`/MAC `…C9` are just different label fields (assembly vs PCB) + adjacent-port MACs on one device.

## Boot chain (hardware root of trust — this is the wall)
1. **Cisco Trust Anchor Module (TAm) + FPGA secure-boot core.** Log shows `FPGA: v0026`, `SecureBoot: R03…`,
   `SB Core: F01113R18…`, `Microloader: MA1011R06…`, a `----SecureBoot Registers----` block (`boot_ok:1`,
   `boot_check_count_*`, `boot1_cs_key_type/index`, `boot2_cs_*`), and `Reading whitelist from TAM` → `whitelist.bin`
   (1236 bytes). This is Cisco's ACT2/TAm hardware anchor that verifies the boot images before x86 runs.
2. **coreboot** (x86 BIOS) → **U-Boot 2015.04** payload.
3. U-Boot converts the TAm whitelist to a **signature fdt** listing allowed keys:
   `x86-gen2-RT-SECP384R1_1-rel`, `x86-gen2-RT-RSA3072_1-rel`, plus `BL_LDWM`, `OD`, `AP` keys (multi-role PKI).
4. U-Boot loads the **FIT image** (58 MB, read from USB storage) and **cryptographically verifies** it:
   `Verifying Hash Integrity ... sha384,secp384r1:x86-gen2-RT-SECP384R1_1-rel+ OK` — that's an **ECDSA P-384**
   signature, not a mere hash. kernel/setup/ramdisk subimages each carry sha1 integrity hashes inside the signed FIT.
5. **Linux 4.4.177-meraki** boots; rootfs = signed **squashfs** ramdisk (read-only). Then mounts the USB disk.
6. Reaches the locked **`<Meraki>` console**: `WARNING! THIS CONSOLE IS LOGGED! UNAUTHORIZED ACCESS FORBIDDEN!`
   (same locked click CLI family as the MS320). Full router dataplane comes up: Click (`merakiclick`, `wired_brain`),
   BIRD/AutoVPN, Snort, NBAR2, AnyConnect/client VPN.

## Storage
- **SATA: all 4 ports time out** — no SATA disk present. AHCI controller idle.
- OS/boot is on a **USB mass-storage device** (`usb-storage 3-1:1.0` → `/dev/sda`), i.e. an internal USB DOM/module:
  - `/dev/sda1` → `/boot` (ext4) — holds the signed FIT image.
  - `/dev/sda2` → `/storage` (ext4) — writable config/state.
- Implication: **the disk is pluggable USB mass storage → imageable with any USB reader** (no exotic clip).

## U118 dump — DONE (2026-07-18)
First hardware dump succeeded. CH341A + SOIC-8 clip, in-circuit, `flashrom -c W25Q64JV-.Q`. Two reads **identical**
→ trustworthy. Canonical file: `u118-dump1.bin` (8388608 bytes), sha256 `f6825c73…60c8155e`. (A second read `u118-dump2.bin`
was byte-identical → verification passed; the dup was removed to keep the repo lean.)
- **Content is opaque / high-entropy**, NOT readable coreboot: 80% erased (0xff); the ~20% (1.64 MiB) of real data
  is **entropy 7.876 bits/byte, only 35.6% printable, zero coreboot/U-Boot/Meraki strings**, binwalk finds nothing.
  → compressed/encrypted/**FPGA-bitstream**-like. Header starts `00 10 30 00 26 00 00 f0 3c 00 26 00`.
- Data sits in discrete regions: `0x000000-0x020000`, `0x120000-0x160000`, `0x170000-0x1b0000`, `0x200000-0x230000`,
  `0x280000-0x2a0000`, `0x300000-0x3b0000`, `0x3c0000-0x480000`.
- **Interpretation**: U118 most likely holds the **FPGA secure-boot core / Microloader / TAm images** (bitstream +
  signed blobs — matches the log's `SB Core`, `Microloader`, `FPGA v0026`) or otherwise encrypted platform data. It
  is **not** the directly-reversible x86 coreboot. The readable **coreboot + U-Boot payload is likely on U97** (16 MiB
  SOIC-16) — needs a SOP-16 clip (not owned yet). Dump preserved for later diff / bitstream analysis.

## Flash chips (photo `flash-chips-U97-U118.jpg`) — both Winbond, both had paint dots (probed before)
| Ref | Part | Size | Package | Likely role |
|-----|------|------|---------|-------------|
| **U118** | Winbond **W25Q64JV** | 8 MiB | **SOIC-8** | **dumped** → opaque high-entropy (FPGA secure-boot/TAm/encrypted), not coreboot |
| **U97** | Winbond **W25Q128JV** | 16 MiB | **SOIC-16** | likely coreboot + U-Boot payload (main platform fw) — not yet dumped |
(Note U-Boot said `SPI: ICH SPI: Cannot find device` — it didn't map the SPI via the ICH at that stage; the flashes
are still physically the platform fw store. Confirm role by dumping.)

## Storage device IDENTIFIED (photos IMG_8017) — it's a soldered-on USB thumb drive
The `/dev/sda` USB mass-storage is built from two soldered chips:
- **U54 = Phison `PS2251-03-Q`** (`PS2251-03-Q UO1817C S7F37188`) — a **USB 2.0 flash-drive controller** ("PS2303").
- **U55 = Toshiba `TH58NVG3S0HTA00`** (TSOP-48) — raw **NAND** (~1 GB / 8 Gb SLC-ish), orange paint dot.
- **Test points `TP145 / TP146 / TP147`** sit right next to U54 → likely the factory USB/programming taps (candidates
  for D+/D-/CLK). This is exactly a UFD (USB flash drive) reflowed onto the mainboard = the OS medium.

### ⚠️ How to read it — through the controller, NOT the raw NAND
- **Do NOT clip/desolder the Toshiba NAND (U55).** Phison controllers **scramble** the NAND (proprietary XOR
  scrambling + BCH ECC + FTL wear-leveling/remap). A raw NAND dump is unreconstructable without reversing Phison's
  FTL. Wasted effort.
- **Read it as USB, via the Phison controller.** Two routes:
  1. **Tap the Phison's USB lines** (D+/D-/VBUS/GND) → USB-A plug → laptop mounts it as a normal drive → `dd`/ddrescue.
     Power ONLY the UFD (board off) so the SoC host doesn't contend. Check `TP145-147` + nearby power first; need the
     PS2251-03 QFP pinout to map D+/D-. **This is the practical way to get the full ~1 GB image.**
  2. **Interrupt U-Boot on serial** — U-Boot already talks to this UFD (it read the 58 MB FIT from it). Good for
     exploration, but exfil is hard: U-Boot reports `Net: No ethernet found` (no TFTP) and serial @115200 ≈ 11 KB/s
     (~25 h for 1 GB). Only useful if U-Boot can `usb write` to a spare external stick.

## U15 dump — DONE (2026-07-19) — Intel platform firmware
CH341A + SOIC-8 clip (first attempt was clip 180°-reversed → `No device found`; flipped → OK). Two reads **identical**.
Canonical file `u15-dump1.bin` (4194304 B), sha256 `52239e6c…bd1c6bff` (second read was byte-identical → verified, dup
removed). Entropy 4.76 (structured, NOT encrypted).
- **CORRECTION (per `u15-analysis.md`)**: there is **NO Intel flash descriptor** on U15 (no `5A A5 F0 0F`, no `$FPT`,
  no `$MN2`). So U15 = **Intel SPS (Server Platform Services) ME firmware for Broadwell-DE + Intel X552/X557 10 GbE
  PXE option ROM** — *not* a descriptor region. Descriptor/BIOS/BootGuard live elsewhere (likely U97). GbE NVM is
  FF-erased → no MAC stored here. Only ~720 KiB of 4 MiB used. Content markers:
  - **Intel Management Engine (ME/CSME)** firmware — ThreadX (`tx_application_define`), `bl_phase_prepare_fw_update`/
    `_mng`/`_proxy`/`_host_if`, `FWSWSync_AcquireSemaphore`, `Reset_Init2ndStage`, `Versions_Init` (`./Main.c`).
  - **Intel GbE NVM + Boot Agent XE PXE option ROM** — `Intel(R) Boot Agent XE v2.3.58`, `PXE 2.1 Build 092`,
    `Processor D-1500 Gigabit LANx`, iSCSI/CHAP boot strings.
  - `Copyright (C) 1997-2015, Intel Corporation`.
- **Why it matters**: this is where **Intel BootGuard** config + the ME live → relevant to the secure-boot picture
  (is BootGuard enabled? key manifest?). Analyze with UEFITool / intelmetool / me_cleaner (read-only). Not encrypted.

## U49 — tried both protocols, undumpable (2026-07-19)
- **SPI (flashrom)**: never detected across many clip attempts → not SPI.
- **I²C (ch341eeprom)**: added to devshell; U49 "reads" but returns **all `0x00`** at every 24Cxx size (256 B → 64 KiB)
  and at **every address 0x50–0x57**. A blank EEPROM reads `0xFF`, not `0x00` → this is **no valid ACK**, i.e. it does
  not behave as a 24Cxx.
- **Conclusion**: U49 is almost certainly an **Atmel crypto/auth chip (ATSHA204A / ATECC-class)** — I²C but with a
  protected command protocol at a non-standard address, holding hardware-locked keys. **Not dumpable** by any simple
  reader (and even reached, its secrets don't read out). Dead end; stop here. (All-zero scratch dumps were deleted.)

## Full flash/chip inventory (photos IMG_8015/8017/8020/8022/8023)
| Ref | Part | Pkg | Role (inferred) | Status |
|-----|------|-----|------------------|--------|
| U118 | Winbond W25Q64JV | SOIC-8 | FPGA secure-boot/TAm or encrypted blob | **dumped**, opaque |
| U97 | Winbond W25Q128JV | SOIC-16 | coreboot + U-Boot payload (main platform fw) | not dumped (needs SOP-16) |
| U15 | Winbond W25Q32JV (4 MiB) | SOIC-8 | **Intel SPS (ME) for BDX-DE + X552 10G PXE ROM** (no descriptor) | **dumped** ✅ (sha256 52239e6c…) |
| U49 | Atmel `ATMLH751…` 8-pin | SOIC-8 | **crypto/auth (ATSHA/ATECC-class), not plain EEPROM** | undumpable — see below |
| U54 | **Phison PS2251-03** | QFP | **USB flash controller** (→ /dev/sda) | target for OS image |
| U55 | **Toshiba TH58NVG3S0HTA00** | TSOP-48 | **NAND behind Phison** (~1 GB) | do NOT read raw |
Also seen (non-storage): Marvell 88E1548/88E60xx PHYs, NXP PCA9548A I²C muxes, TI/PW555 logic, U58 8-pin.

## Reading the USB storage (it's SOLDERED — not a pluggable module)
The `/dev/sda` device is a **soldered** USB mass-storage part (eUSB/USB-DOM module or eMMC-behind-USB-bridge), so
"pull and image it" doesn't apply. But it enumerates as **plain USB mass storage**, which opens non-invasive routes.
Ranked by invasiveness (need a close-up photo of the storage chip to pick concretely):
1. **Interrupt U-Boot on serial** (free, zero solder): power on, mash keys — if the prod U-Boot drops to a prompt,
   `usb start` → read the disk → dump over tftp/USB. Likely disabled in a verified-boot production build, but costs
   nothing to try first.
2. **Tap the module's USB lines** (D+/D-/VBUS/GND) to a USB-A plug → mount on the laptop as a normal drive → `dd`.
   Exploits that it's literally USB (no NAND-controller knowledge needed). Power ONLY the module (board off) or the
   host CPU will also enumerate/contend. Needs finding the 4 lines (a header or test points near the USB PHY).
3. **In-circuit read of the storage chip**: if it's an eMMC, find CLK/CMD/DAT0/VCC/VCCQ/GND pads and read with an
   eMMC reader while holding the CPU in reset. Chip-specific.
4. **Desolder** the module/chip and read externally (eMMC→SD/USB adapter, or reflow a DOM onto a USB port). Most
   reliable, most invasive.
Need next: a **close-up photo of the storage device** + the area around it (look for a small BGA/flash IC near the
USB PHY, a Swissbit/ATP/Samsung/SanDisk mark, or a 2x5 "USB" header) to identify the exact part and method.

## Assessment — MX250 vs MS320
- **MS320**: no secure boot (CRC32/SHA1) → software-patchable, but proprietary binaries were unobtainable without a
  flash dump.
- **MX250**: **real hardware secure boot** (Cisco TAm/FPGA + U-Boot ECDSA-P384/RSA-3072 verified FIT). *Running*
  unsigned firmware is genuinely hard — editing the USB disk's FIT fails U-Boot's signature check; patching
  coreboot/U-Boot on SPI (U97) would fail the TAm/FPGA secure-boot check. Breaking it needs a verified-boot parsing
  bug, an FPGA/TAm vuln (cf. "thrangrycat" CVE-2019-1649 on Cisco Trust Anchor), or the signing keys.
- **BUT the analysis door is wide open here**: the rootfs sits on **pluggable USB storage**, so we can image it with a
  plain USB reader and get **all the Meraki userspace binaries** (`serial_logincheck`, click, `wired_brain`, etc.) —
  exactly the Ghidra reversing that was *blocked* on the MS320. Best first move, needs no clip.

## Next actions
1. **Image /dev/sda** — locate the internal USB storage module, pull it, dump with a USB reader (`dd`/ddrescue).
   Read-only. This yields the signed FIT + squashfs rootfs + `/storage` config for offline analysis.
2. Extract the squashfs rootfs; inventory the Meraki binaries; reverse the console lock (`serial_logincheck`) and
   look for a runtime/console foothold — now feasible offline.
3. Dump U97 (SOIC-16) / U118 (SOIC-8) to study coreboot + U-Boot verified-boot and the TAm interface (needs clips).
4. Research prior art: WatchMySys "Breaking secure boot on Meraki Z3/Go GX20", Cisco TAm/thrangrycat, Meraki MX RCE.

## Artifacts
- `bootlog.txt` — full serial boot capture (1472 lines).
- `board-overview.jpg`, `flash-chips-U97-U118.jpg`.

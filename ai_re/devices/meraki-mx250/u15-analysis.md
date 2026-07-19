# U15 SPI Flash Analysis — Cisco Meraki MX250

**File:** `u15-dump1.bin` (4 MiB, Winbond W25Q32JV, read from SPI chip U15)
**Verified duplicate:** `u15-dump2.bin` — MD5 identical (`0f6ad86e9f4cdf5aae4371b272ef5e68`), so the read is trustworthy.
**Platform:** Intel Xeon D-1530 (Broadwell-DE / "BDX"), coreboot board "monsters" / x86-gen2.
**Method:** read-only. `strings`, `xxd`, `python3` manual struct parsing, `me_cleaner -c` (check mode). Original never modified; a scratchpad copy was used for me_cleaner.

---

## Executive summary / verdict

The parent hypothesis — "Intel Flash Descriptor at 0x0, ME region, GbE region, $FPT partitions" — is **DISPROVEN**. This chip does **not** carry a standard Intel descriptor+ME+GbE layout.

What U15 actually contains is a **raw, unwrapped Intel firmware image** with **no Flash Descriptor and no `$FPT` partition table**:

1. A **ThreadX-based Broadwell-DE server-management firmware** — Intel **SPS (Server Platform Services)** bring-up code plus a large board-management/network driver stack (BMC interface, Cortina 10G PHY / SFP / I2C management, GbE NVM task).
2. An **Intel X552/X557 10 GbE PXE/UNDI option ROM** ("Intel(R) Boot Agent XE v2.3.58", Copyright 1997-2015).
3. Only the first ~0xB0000 (≈720 KiB) is used; 0xC0000–0xFFFFF is zero-filled; 0x100000–0x3FFFFF is erased (0xFF).

Because there is no descriptor and no FPT, the standard "is the descriptor locked / what's the ME SKU / manufacturing-mode" questions **cannot be answered from this chip** — those structures are simply not present here. `me_cleaner -c` returns **"Unknown image"**, corroborating this.

---

## 1. Flash Descriptor — ABSENT

- Required signature `5A A5 F0 0F` (FLVALSIG) at offset **0x10** is **not present**. Bytes at 0x10 are `3C 01 00 01`. A whole-file scan for `5A A5 F0 0F` returns **zero hits**.
- First 0x10 bytes are `60 05 08 00 FF FF 00 10 FF FF 88 80 10 01 34 01` — this is code/vector data, not a descriptor header (a real descriptor has 0x00–0x0F reserved/FF and FLVALSIG at 0x10).
- Consequently there is **no region map (FLREG0-4), no FLMSTR master-access table, no PCH straps**. Descriptor lock status is **N/A — there is no descriptor on this chip**.
- The companion 16 MiB chip `u118-dump1.bin` also has **no** `5A A5 F0 0F` at 0x10 (`FF FF FF FF`) and no `$FPT`; its head is `00 10 30 00 26 00 00 F0 …` (coreboot/FMAP-style, out of scope here). So neither dumped chip exposes a classic Intel descriptor at offset 0.

**Implication:** whatever host-write protection exists is not governed by a descriptor on U15. U15 reads as a dedicated firmware payload rather than a host-visible descriptor region.

## 2. Intel ME / CSME / SPS version & SKU

- This is Intel **SPS (Server Platform Services)** firmware for **Broadwell-DE**, not client ME 1.5MB/5MB. Evidence:
  - `BDX FW ver ` (Broadwell-DE firmware version printf template) @ 0x9E9C.
  - ME/SPS bring-up (BUP) strings @ ~0x99xx–0x9Bxx: `tx_application_define` (ThreadX), `Reset_Init2ndStage`, `Versions_Init`, `GENERAL_Init`/`GENERAL_InitSecondStage`, `FWSWSync_AcquireSemaphore`, `LTR_Int`, and the SPS phase sequence `bl_phase_prepare_fw_update` / `_mng` / `_proxy` / `_host_if` / `bl_finalize`, `./Main.c`.
  - Server-management stack: `./BMCIn.c`, `BMCIN_init`, `EVENT_BMCIN`, `THREAD_BMCIn`, `./NetHostIn.c`, `./NVMTask.c`, `NVMTask_Enable` — SPS Node-Manager / BMC / GbE-NVM functionality.
- **Exact numeric version is NOT recoverable from this dump.** There is **no `$MN2`/`$MAN` manifest**, no `$FPT`, no `$VER`, no version string — the running version is filled at runtime into the `BDX FW ver ` template. Whole-file scans for `$FPT, FTPR, NFTP, MFS, $MN2, $CPD, $MME, DLMP, FTUP, PMCP, ROMB, $SKU, $UEP` all return **none**.
- SKU: server SPS (BDX). No consumer/corporate distinction applies. High-entropy blocks at 0x90000–0xB0000 (entropy 7.1–8.0) are compressed firmware/code modules.

## 3. Intel BootGuard — NOT DETERMINABLE FROM THIS CHIP

- No BootGuard structures anywhere: scans for `__KEYM`/`KEYM` (Key Manifest), `__ACBP`/`ACBP` (Boot Policy), `IBBS`, `OEMP`, `BPMH`, `RSAK` all return **none**.
- BootGuard enablement lives in the ME **FPFs** and descriptor **PCH straps** — neither is present on U15 (no ME FPT, no descriptor). So BootGuard status **cannot be assessed here**; it would have to come from the ME region / FPFs on the primary SPI. **No positive evidence of BootGuard in this dump.**

## 4. GbE region — Intel X552 10 GbE, NO MAC stored

- **Option ROM** at **0x88000**: PCI ROM signature `55 AA`, size `0x28`×512 = **20480 bytes**.
- **PCIR** (@ ~0x88040): vendor `0x8086` (Intel), device **`0x15AB`** = Intel **X552/X557-AT 10GBASE-T** (the Xeon-D integrated 10 GbE). UNDI/`!PXE` PXE base-code present.
- Strings: `Initializing Intel(R) Boot Agent XE v2.3.58` (@0x880AF), `IBA XE Slot 0000 v2358`, `PXE 2.1 Build 092 (WfM 2.0)`, `Copyright (C) 1997-2015, Intel Corporation`, `Processor D-1500 Gigabit LANx` (@0xE0F, 0x4E0F). "XE" = 10-Gigabit Boot Agent.
- **MAC address: NONE extractable.** The GbE NVM region is empty — 0x80000–0x87FFF is all `0xFF`, and the option-ROM MAC template is blank (`MAC Address 000000000000 PBA Number 000000-000`, `PCI ID 0000/0000/0000/0000/00`). The apparent "00:18:0A" (Meraki OUI) hit at 0x52F72 is **coincidental code bytes inside the SPS image**, not a real MAC (surrounding bytes are repeating instruction patterns). No usable MAC is stored on this chip.

## 5. Cisco / Meraki customization

- **No ASCII "Cisco", "Meraki", or "monsters" strings** anywhere in the image.
- No OEM/vendor manifest (`OEMP` none), no custom named partition (no FPT at all).
- However the firmware is clearly a **customized OEM management build**, not a stock reference image: heavy custom driver/logger stack — per-module logger channels `NIC.*`, `PHY.*`, `LINK.*`, `HCI.*`, `NVM.*`, `TD.*`, `NP.*`, `LOG_INIT_DRV.*`; **Cortina** 10G PHY management (`CORTINA_TIMER`, `SFP_TIMER_NAME`, `LPLU_TIMER`), a hand-rolled **`i2c_bitbang` driver** (`./I2C_bitbang_driver.c`), timers `TIMER_KEREM`/`TIMER_VPD`/`TIMER_WD`, `PowerStateDriver`, `PTIMER_CORE_RESET`. This is consistent with a Meraki/OEM board-specific SPS + NIC-management image built on standard Intel SPS + Intel X552 components — but with **no overt vendor branding** in the strings.

## 6. Other notable findings / security relevance

- **Manufacturing mode / debug flags: not determinable** — requires the ME FPT/FPFs, absent here.
- **No descriptor => no host-write lockdown expressed on this chip.** If U15 is directly SPI-programmable, nothing in the image itself enforces read/write protection (protection, if any, is external — flash WP pin / platform).
- Only ~18% of the 4 MiB chip is populated; the large erased tail (0x100000–0x3FFFFF = 0xFF, ~3 MiB) is spare/unused space.
- No RSA manifest/signature container (`$MN2`) is present, so image integrity/signing of the SPS payload cannot be verified from this dump alone.
- Entropy map (per 64 KiB): 0x00000–0x0FFFF ≈4.8 (tables/vectors); 0x10000–0x4FFFF ≈5.85 (code); 0x50000–0x6FFFF sparse/zeros; 0x70000 erased; **0x80000–0x8FFFF** ≈4.2 (PXE option ROM, 50% FF); **0x90000–0xB0000** ≈7.1–8.0 (compressed SPS modules); 0xC0000–0xFFFFF zeros; 0x100000+ all 0xFF.

---

## Key offsets (quick reference)

| Offset | Content |
|--------|---------|
| 0x00000 | Image start (code/vectors `60 05 08 00 …`) — NOT a descriptor |
| 0x00E0F | `Processor D-1500 Gigabit LANx` |
| 0x099xx | SPS/ME BUP bring-up strings (`Main.c`, `Reset_Init2ndStage`, `Versions_Init`, `bl_phase_*`) |
| 0x09E9C | `BDX FW ver ` (Broadwell-DE) |
| 0x09E58 | `EVENT_BMCIN` / BMC + NetHostIn + NVMTask driver strings |
| 0x0B45A | `iSCSIPrimary` / `iSCSISecondary` |
| 0x0B510+ | `i2c_bitbang_*` driver strings; Cortina/SFP/LPLU timers |
| 0x80000 | X552 10 GbE PXE/UNDI option ROM (`55 AA`, 20 KiB); vendor 8086 device 15AB; Boot Agent XE v2.3.58 |
| 0x90000–0xB0000 | Compressed SPS firmware modules (entropy ~7–8) |
| 0xC0000–0xFFFFF | Zero-filled |
| 0x100000–0x3FFFFF | Erased (0xFF), unused |

**No:** `5A A5 F0 0F`, `$FPT`, `FTPR`, `$MN2`, `KEYM`, `ACBP`, `IBBS`, `OEMP` anywhere in the image.

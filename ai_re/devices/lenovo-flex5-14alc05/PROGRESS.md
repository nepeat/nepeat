# Lenovo IdeaPad Flex 5-14ALC05 — BIOS un-brick

Recovering a laptop bricked by a bad in-Windows BIOS update. Goal: reflash the
SPI BIOS chip with a good image via external programmer.

**STATUS: ✅ RECOVERED (2026-07-20). Reflashed, verified OK, and the laptop
POSTs/boots. Serial + activation preserved. Case closed.**

## Latest status (2026-07-20)

- **Identified** the board + BIOS chip from board photos.
- **Got the official good firmware**: Lenovo GJCN34WW (latest, Dec 2023),
  extracted the UEFI capsule `GJCN34WW.cap`. See [firmware/](firmware/).
  (User's own `~/Downloads/gjcn34ww.exe` is byte-identical, sha256 `7196427b…`.)
- **Dumping the patient chip in-circuit** via SOIC-8 clip → SOIC8-DIP8 adapter →
  TL866II+ ZIF socket, profile `W25Q128JW1.8V@SOIC8`. Contact good after reseat
  (`-z` pin test passes; `-D` → Chip ID `0xEF6018 OK`).
- **In-circuit read is slightly flaky**: reads #1 vs #2 differ in only
  **7719 bytes, all inside two 4KB blocks — `0x36000` and `0xd53000`**
  (0.046%). Everything else (EFS, PSP/BIOS dirs, all FF fill) reads identically.
  Plan: take 5+ reads, per-byte **majority vote** to recover those two blocks.
- Patient dump is a valid raw image: **EFS `0x55AA55AA` at `0x20000`**,
  `$PSP`@0x53000 / `$BHD`@0x5be44 / `2PSP`@0x51000, `_FVH` volumes present →
  correct base for the merge.

### Read log
- 10 in-circuit reads captured (`dump/orig1..10.bin`). In-circuit flakiness =
  a few random 4KB blocks glitch per read (0x36000, 0x22d000, 0xb74000,
  0xbe1000, 0xd53000, 0xf7b000). Reconstructed `dump/merged_vote.bin` by
  per-byte **majority vote** across all reads (every disagreeing byte had a
  clear >=3/5 majority; 0 ambiguous).

## Recovery image build (2026-07-20)

Method confirmed by two independent research passes + local psptool analysis:
**build from the official capsule wholesale, splice back only patient-unique,
non-PSP windows.** Never mix patient PSP with capsule BIOS.

- **Virgin base** = `GJCN34WW.cap[0x320 : 0x320+0x1000000]` (16MB). Strip length
  0x320 is the value that lands the AMD EFS `0x55AA55AA` exactly at `0x20000`
  (same as the patient dump). Verified.
- **psptool** (`pip install psptool`) on the virgin base: complete, verified
  boot chain — PSP_FW_BOOT_LOADER / recovery BL / SMU / ABL / security policy /
  public keys all present, sha256_ok. **0 of 67 entries** are FF-stubbed where
  the patient has firmware → capsule is self-bootable. (Entry type 0x22
  `TOKEN_UNLOCK` @0x94000 is FF — normal/empty on a non-PSP-unlocked machine.)
- **Preserved from patient** (all verified byte-stable across 10 reads):
  - DMI/identity `0x5EA000–0x5EDFFF` — serial `R911MBZ5`, MTM `82HU002YUS`,
    UUID, `IdeaPad Flex 5 14ALC05`. Dedicated sector: only first 0x400 bytes are
    identity, rest FF (== virgin). This carries the digital-entitlement Windows
    activation. **No MSDM key exists** in either image (activation is digital,
    not OEM-key), so the `0x617000` "just for test" placeholder key is cosmetic.
  - Key/EVSA `0x617000–0x6170BF` — matched to the forum recipe (cosmetic here).
  - **PSP NVRAM** `PSP_NV_DATA @0x549000` (0x20000) + `PSP_NVRAM @0x569000`
    (0x40000) — the capsule ships these empty (100% FF); patient has real
    fTPM/secure-storage state. Preserving = fTPM/BitLocker survive (user chose
    to keep). Both stable across 10 reads.

Candidate images (`dump/`):
- `RECOVERY_A_identity_only.bin` sha256 `ef5f64d9…` — NVRAM blanked (clean-slate)
- `RECOVERY_B_keep_nvram.bin`    sha256 `9571fc4a…` — **chosen**; fTPM preserved

### Flash
- Target: `W25Q128JW1.8V@SOIC8` on TL866II+, in-circuit, `-u` (unprotect) +
  default erase→write→verify. Pin test passed, ID `0xEF6018` immediately before.
- Result: **`Verification OK` (exit 0)** on 2026-07-20. `Protect off...OK`, chip
  read-back matches the flashed image. Chip now holds GJCN34WW + preserved
  identity + fTPM NVRAM. Pending: reassemble + power-on smoke test.

## Device identity

- **Model**: IdeaPad Flex 5-14ALC05, **machine type 82HU**.
- **SoC**: AMD Ryzen 7 5700U (Lucienne, Zen2). AMD platform → PSP-based boot,
  **no Intel ME/descriptor** in the usual sense.
- **Motherboard FRU**: `5B21B84992`. ODM board **LC56-14A SVT / 203021-1**
  (silk `455.0MD02.0001`). Repair-forum dumps are keyed to this board string.
- **Keyboard FRU** (other label): `5CB0Y85490`.

## BIOS SPI flash

- **Chip**: Winbond **W25Q128JW** (marking `25Q128JWSIQ`), SOIC-8.
- **Size**: 128 Mbit = **16 MB** (16,777,216 bytes).
- **Voltage**: **1.8 V** (`JW` family). Location silk = **`SKT1 / BIOS1`**.
- Programmer: **XGecu/TL866II+** (on bench, driven by `minipro`). The TL866II+
  *does* support 1.8 V VCC, so it can read/write this part. Pick the **1.8 V
  `W25Q128JW`** profile in minipro, NOT the 3.3 V `JV` profile.
    - `minipro -L | grep -i 25Q128` to get the exact device name.
    - Read twice + `cmp` to prove a clean contact before trusting anything.

## The brick

Well-documented failure mode for this model: Lenovo pushed a BIOS update
(GJCN2x family) via Windows Update; interruption corrupts the BIOS and bricks
to a black screen. USB crisis recovery generally does NOT work for this brick →
external programmer is the realistic fix.

## Recovery plan

The capsule is **not** a 1:1 SPI image and must not be blind-flashed:

1. It starts with a UEFI firmware volume (`_FVH` @ 0x28), contains AMD PSP/BIOS
   directories (`$PSP`/`$BHD`/`2PSP`), but the AMD EFS `0x55AA55AA` is **not** at
   a bootable flash offset (0x20000 / 0xFA0000 are blank) → layout is for the
   Insyde in-OS flasher, not the raw chip.
2. `install.bat` calls `SctWinFlash64.exe … /sd /sn …` — the flasher **reads the
   serial/DMI + board vars off the live chip** and re-injects them. So the
   `.cap` deliberately omits machine-unique data (serial, UUID, LAN MAC).

Therefore the plan is a **merge**, not a blind write:

1. **Dump the patient chip** (full 16 MB), twice, `cmp` to confirm. Keep as
   `dump/orig.bin` — it holds the correct flash layout + serial/UUID/MAC.
2. Diff patient dump vs the GJCN34WW payload to locate the corrupted region
   (a bad update usually trashes only the BIOS body, leaving PSP + the first
   region + machine data intact).
3. Rebuild a flashable 16 MB using the **patient dump as the base** (correct
   descriptor/EFS/layout/serial) and overlay the **good BIOS/PSP bodies** from
   the capsule where corrupted.
4. Flash + verify with minipro; reassemble.

Fallback if the patient dump is too far gone: use a full known-good SPI dump
from the repair forums (Vinafix / Badcaps / dr-bios — board `LC56-14A SVT
203021-1`), then splice the patient's serial/UUID/MAC back in.

## Firmware provenance (see firmware/, git-ignored)

- `gjcn34ww.exe` — Lenovo official installer (Inno Setup wrapper).
  - Source: `https://download.lenovo.com/consumer/mobiles/gjcn34ww.exe`
  - sha256 `7196427b9afe6362edaf0ad77e5d0c4ca3f1903406ca4ea8e6ce0d333e9f74e9`
- `GJCN34WW.cap` — UEFI capsule extracted from it (17,713,600 B; 0x50 capsule
  header, then ~16 MB payload, then a ~936 KB Insyde flash-driver blob).
  - sha256 `71f7ff65a072a67f74c8cc015708ae7cfa8c5a8e89cf5ce8a323476e82ba794f`
- Extract with: `innoextract gjcn34ww.exe` → `code$GetExtractPath/GJCN34WW.cap`.

## Tooling notes

- `minipro` added to the RE devshell flake (drives the TL866II+).
- `innoextract` pulled ad-hoc via `nix shell nixpkgs#innoextract` to unwrap the
  Lenovo installer. Consider adding to flake if we do more Lenovo firmware.
- UEFI capsule/region surgery will want `UEFITool` / `uefi-firmware-parser` —
  not yet in the flake; add when we start the merge.

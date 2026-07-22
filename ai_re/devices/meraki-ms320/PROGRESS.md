# Meraki MS320 — Custom Firmware

Goal: run custom / unsigned firmware on a Cisco Meraki MS320-48 switch.

Entry point for this project. When this file gets large, break detail into siblings
(`uart.md`, `bootloader.md`, `flash-dump.md`, `firmware-build.md`) and keep this as index + latest status.

## Latest status
- **2026-07-20 (in-circuit SPI attempt — FAILED, U10 desolder required)**: Tried
  to dump U10 (MX25L12845E, SO16) in-circuit with the **TL866II+ + SOP-16 clip**
  (`minipro -p MX25L12845E@SOP16`). `-z` pin test **passed** every time, but
  `-D` returned **`0xFFFFFF`** (bus held/rail sag) across multiple reseats, and a
  later attempt tripped **`Overcurrent protection!`** — the TL866II+ can't
  energize this 48-port switch's 3.3V rail through one clip pin, and with power
  applied the VCore-III SoC contends the shared SPI bus. Both failure modes hit
  = in-circuit is not viable here (matches the caution in `hardware-flash.md`).
  **Next: desolder U10** (hot air), read in a SOP16→DIP16 adapter in the ZIF.
  JEDEC target `0xC22018`.
- **2026-07-18**: UART serial contact made (`/dev/cu.usbserial-0001 @115200`, tmux session `serial`, logging to
  `serial-*.log`). Unit is already booted and sitting at the `<Meraki>` prompt. It is a **locked-down CLI**, not a
  shell: any unknown command returns `UNRECOGNIZED COMMAND LOGGED TO CLOUD SERVERS.` and `?`/`help` give nothing.
  This is the `merakiclick` restricted console. **Note: this console phones home** — further blind command poking
  is logged to Meraki cloud. Stopped poking pending a decision on direction.
- **2026-07-18 (research done)**: Community prior art surveyed — see `research.md`. Big wins and one blocker:
  - **No secure boot.** Boot chain is integrity-checked only: ROM→boot1 = **CRC32**, LinuxLoader→NAND kernel = **SHA1**.
    No signatures/PKI/fuses. Custom firmware is fully feasible.
  - **Ready-made builder**: `halmartin/meraki-builder` explicitly supports `MS320-24(P)` and `MS320-48(LP|FP)`.
  - **Root method** (from equivalent MS220-8P): patch `bootsh` in the stage-1 initramfs to exec a static MIPS BusyBox
    instead of `kexec`, recompute CRC32; console lock is `serial_logincheck` (manufacturing-mode gated).
  - **Blocker**: stock firmware has **no runtime shell and no TFTP/network recovery**, so the *first* patched image
    must be written with a **hardware SPI programmer** on the NOR (`MX25L12805D` SOIC-8). No documented software-only
    foothold. → The "no programmer yet" answer is the real gate. Recommend a CH341A + SOIC-8 clip (or Pi + flashrom).
  - Next: do all offline prep now (build image, prep patched `boot1`) so flashing is quick once a programmer arrives.
- **2026-07-18 (can we reverse the lock without hardware? — NO)**: Tried to obtain `serial_logincheck`/`odm` for
  Ghidra analysis. They're proprietary and exist **only in a real flash dump**: meraki-builder patches around them
  (`sed serial_logincheck→/bin/ash`) and its `extract.sh` needs a NAND dump you supply; the GPL repo has no rootfs;
  **no public MS2xx dump exists** (searched GitHub + web). So getting the binaries needs the hardware flasher — the
  thing we wanted to skip → chicken-and-egg. Ghidra analysis is viable but only *after* the first hardware dump. Only
  hardware-free probe left is the live Local Status Page (likely patched on 2020 fw). See `research.md` verdict.

---

## Identity
- **Model**: Meraki MS320-48 (48-port managed switch). Board name in firmware: `elemental`.
- **MAC (this unit)**: `00:18:0A:CC:52:D0`
- **SoC**: Microsemi/Vitesse **VCORE-III**, CPU **MIPS 24KEc** rev `01019654`, ~275 BogoMIPS.
  (VCORE-III = Vitesse Jaguar/Serval switch SoC family. Confirm exact part from chip markings on teardown — likely VSC74xx "Jaguar-1".)
- **RAM**: 128 MiB (`memory: 07ff0000 @ 0`, `mem=134152192`).
- **Watchdog**: VCORE-III WDT, 30 s. Relevant — a wedged custom image will be reset in 30 s.

## Flash layout
Two chips. Both confirmed in bootlog (see `bootlog.txt`).

**SPI NOR — Macronix `MX25L128` (marked `...45E`), 16 MiB** (`m25p80 spi0.1`), board ref **`U10`**. This is the chip
to clip for a hardware dump. **Package is 16-SOP (SO16), NOT SOIC-8 → need a SOP-16 clip.** Pinout + CH341A wiring +
gotchas in `hardware-flash.md`. Holds bootloaders + boot kernels, A/B redundant:
| Offset | Size | Name | Purpose (inferred) |
|---|---|---|---|
| 0x000000 | 0x40000 (256K) | loader1 | 1st-stage LinuxLoader (A) |
| 0x040000 | 0x3c0000 (3.75M) | boot1 | boot kernel image (A) |
| 0x400000 | 0x40000 | loader2 | 1st-stage LinuxLoader (B) |
| 0x440000 | 0x3c0000 | boot2 | boot kernel image (B) |
| 0x800000 | 0x80000 (512K) | rsvd | reserved |
| 0x880000 | 0x600000 (6M) | bootubi | UBI vol → boot config |
| 0xe80000 | 0x40000 | conf | config |
| 0xec0000 | 0x100000 (1M) | stackconf | stacking config |
| 0xfc0000 | 0x40000 | syslog | persisted syslog |

**NAND — Micron `MT29F1G08ABADAWP`, 128 MiB SLC** (page 2048, OOB 64). One big UBI (`gen_nand.0`),
12 user volumes; vol 4 = `storage` UBIFS. Main firmware + runtime FS live here.

## Boot flow (this is the crux for custom firmware)
There is **no U-Boot / RedBoot and no interactive bootloader prompt**. The design is a *boot-kernel* chain:

1. `LinuxLoader built Nov 12 2014` — tiny 1st stage in `loader1`. Brings up PLL/SPI/DRAM, no user interrupt.
2. Loads a **boot Linux kernel** from `boot1` (`Linux 3.18.x-meraki-elemental`) whose userspace is **`bootsh`**
   (`Made it into bootsh`, `bootsh build switch-12-...-rel-zoology`). bootsh mounts UBI and selects/loads the
   **main firmware kernel** from NAND, then boots into the real OS (2nd `Linux version` block in the log = main image).
3. Main OS prints the banner `WARNING! THIS CONSOLE IS LOGGED!` and reaches a `<Meraki>` prompt.
   Board string: `boot 40 build switch-12-202008242327-G4db7ce45-rel-zoology board elemental`.
   Reset button honored: `Quick boot reason lookup: reset_button`.

Implication: getting custom firmware to run means one of:
- **A. Replace `boot1`/`boot2` kernel** on SPI NOR with our own — *if* LinuxLoader does not verify a signature. Unknown yet.
- **B. Replace the main firmware image in NAND UBI** — *if* bootsh doesn't verify it. Unknown yet.
- **C. Get a shell** at the `<Meraki>` prompt (or via a bootsh escape) to inspect verification + flash from the device.
- **D. Hardware route**: clip/desolder the SPI NOR (SOIC-8, mx25l12805d) and read/write with a CH341A/flashrom.
  This is the fallback that bypasses any software gate and also gives us a full backup first.

## Open questions
- ~~Is boot1 signature-checked?~~ **Answered**: CRC32 only (ROM→boot1), SHA1 (LinuxLoader→NAND kernel). No secure boot.
- ~~What is reachable at `<Meraki>`?~~ **Answered**: locked `merakiclick`/`serial_logincheck` CLI, cloud-logged.
- ~~Can we interrupt the boot-kernel?~~ Not needed; the route is patch-and-reflash, not interrupt.
- Confirm the **exact initramfs offset/length inside our `boot1`** (MS220-8P was `0x339604`/`349124` — will differ).
- ~~Confirm NOR chip + package~~ **Answered**: `U10`, Macronix MX25L128 (…45E), **16-SOP** (need SOP-16 clip), JEDEC
  `C2 2018`. See `hardware-flash.md`.
- Whether an in-circuit clip read works on this board or the NOR must be desoldered (SoC shares the SPI bus).

## Plan / next actions
Path chosen: **software patch (bootsh→shell) + one-time hardware SPI flash**, using `meraki-builder`. Sequence:
1. ~~Serial contact~~ **done** — locked CLI confirmed.
2. ~~Research community work~~ **done** — see `research.md`. No secure boot; MS320 supported by `meraki-builder`.
3. **Acquire a SPI programmer** (CH341A + SOIC-8 clip, or Raspberry Pi + flashrom). Gating item — user has none yet.
4. **Offline prep while waiting** (no hardware needed):
   - Clone/build `halmartin/meraki-builder` for `MS320-48` in the devshell; produce `loader1`, kernel, `squashfs`.
   - Build a static **MIPS32r2 24kec LE BusyBox** for the `bootsh`→shell patch.
   - Script the carve/patch/reassemble of `boot1` (initramfs swap, recompute CRC32; SHA1 for stage-2).
5. **When programmer arrives**: clip the NOR, **`flashrom -r` a full backup first** (mandatory), then write the
   patched image, boot to the stage-1 shell, and flash the rest on-device (`nandwrite /dev/mtd12`, etc.).

## References
- `bootlog.txt` — full UART boot capture (verbatim, 327 lines).
- `research.md` — community prior art, exact techniques, and source links.

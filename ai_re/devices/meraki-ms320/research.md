# MS320 — Community RE research (2026-07-18)

Prior art on Meraki MS boot-kernel switches. The MS320 shares the Jaguar-1 architecture and boot chain with the
MS220-24/48, so the well-documented MS220 work applies almost directly.

## Headline: no secure boot
The whole boot chain uses only **integrity checks, no cryptographic signatures** (per the MS220-8P rooting writeup,
same VCore-III / boot-kernel design):
- **VCore-III ROM loader → `boot1` (stage-1 in NOR)**: **CRC32 only** (32-bit word 16 bytes into the header). A
  deliberate CRC failure makes the ROM jump to the `loader2`/`boot2` B-slot.
- **stage-1 LinuxLoader → main kernel (NAND)**: **SHA1 only**. Bad hash prints `meraki_part_mips: SHA1 doesn't match`.
- **No PKI, no fuses, no secure boot.** Recalculate CRC32/SHA1 after patching and the device runs arbitrary code.

This means custom firmware is fully achievable — the only gate is *getting the first modified image onto the flash*.

## Ready-made build system: `halmartin/meraki-builder`
- Explicitly supports **`MS320-24(P)` and `MS320-48(LP|FP)`** — our exact model.
- Produces the three NOR pieces: `nor/bin/loader1` (RedBoot), a kernel (separate repo), and
  `output/images/rootfs.squashfs` → `nor/bin/squashfs`. Buildroot-based (latest stable buildroot), kernel 3.18.x.
- README does **not** cover on-device flashing — that's the writeups below.

## Root technique (MS220-8P, directly portable)
Console is locked by `serial_logincheck` except in **manufacturing mode** (flag checked against `/storage/config`).
The bypass patches the boot chain instead of enabling manufacturing mode:
1. Extract `boot1` from NOR; locate the **XZ-compressed cpio initramfs** inside it (on the MS220-8P: offset
   `0x339604`, length `349124`). `dd` out the pre-ramdisk head, `xz -d | cpio -i` the ramdisk.
2. The initramfs holds static `bootsh` + `kexec`. Replace the `kexec ...` path with a shell. Naive hex-edit of the
   `kexec -f %s --reuse-cmdline` string fails (`/bin` isn't mounted that early); instead drop a static **BusyBox for
   MIPS32r2 24kec little-endian** (`-Wa,-mips32r2,-march=24kec,-mtune=24kec -mel -EL`) and an `/init` that execs it.
3. Reassemble: `cat boot1-pre modified.xz boot1-post > boot1-patched`, then **recompute the CRC32** in the header.
   (For stage-2 kernel mods, recompute the **SHA1** instead.)
4. Alternatively bypass the manufacturing-mode check by editing the **stage-2 initramfs** directly.

## The catch (matters for us): initial write needs a hardware programmer
- The stock firmware is locked with **no runtime shell and no TFTP/network recovery**. So the *first* patched image
  must be written with a **hardware SPI programmer** clipped to the NOR chip (`MX25L12805D`, SOIC-8):
  `flashrom -p linux_spi:dev=/dev/spidev0.0,spispeed=600 -c MX25L12835F -w dump-patched.dat` (Raspberry Pi example;
  a CH341A works too). Serial console (jumper 4, 115200) is used to watch/kexec, not to write flash.
- **Once you have the stage-1 shell**, further flashing is on-device, e.g. NAND main image:
  `nandwrite -p /dev/mtd12 /firmware.bin`. NOR can then be rewritten from the device too.

## Not the path: mainline OpenWrt
OpenWrt's `mscc` target only covers **Ocelot** (VSC7513/14) and **Luton10/26** VCore-III parts — **not Jaguar-1**
(VSC746x / LynX, our SoC). So mainline OpenWrt won't boot the MS320; `meraki-builder` (buildroot) is the real path.

## Takeaway for our plan
The earlier "no programmer yet" answer is the actual blocker: there is **no documented software-only foothold** on
the locked stock firmware — bootstrapping custom firmware needs a **one-time hardware SPI flash** of the 16 MiB NOR.
Everything else (building the image, preparing patched `boot1`, BusyBox toolchain) can be done offline first so the
flash is quick once a CH341A/Pi is available.

## Locked console & software-foothold angle (can we skip the programmer?)
Question: is there a way past the `<Meraki>` lock or a known bug, avoiding the hardware flasher? Findings:

- **The lock = `serial_logincheck`** (the getty on `ttyS0`, also the `mf` account's shell). It only drops to a real
  shell / accepts `odm` commands when the device is in **manufacturing or RMA mode**. In normal mode it rejects
  everything (`UNRECOGNIZED COMMAND...`). Per Leo, the "LOGGED TO CLOUD" banner is a **bluff — it doesn't actually
  log**. No public console-only bypass exists.
- **Manufacturing mode** is a flag set via `/usr/bin/board_data_config` (`mfg_done`) / `/storage/config`. Setting it
  needs a shell → chicken-and-egg. No documented way to flip it from the locked console.
- **`odm` utility** (`/usr/bin/odm`) — a script that does diagnostics *including writing new firmware*. Nobody has
  published what, if anything, the locked prompt exposes of it. This is the one **un-enumerated live attack surface**,
  but it's likely mfg-mode-gated too.
- **Network bug — CVE-2018-0284** (Local Status Page privilege escalation, CVSS **8.8**): authenticated local-status
  user can inject config → **interactive elevated session**, no hardware. Affects MS. **BUT fixed in MS 9.37 / MS
  10.20 (2018).** Our unit is the **`switch-12` / 2020 (`rel-zoology`)** branch — far past the fix, so almost
  certainly patched. Only relevant if the unit were downgraded.
- **Salesforce "Meraki RCE" series** (Alberto Garcia Illera): real multi-part research, but MX-appliance-focused and
  reported/fixed by Cisco. Not an MS-switch console bug.

**Bottom line**: no public, working, hardware-free root for a 2020-firmware MS320. Every documented root of this
device class went through the SPI flasher. The only *unexplored* software paths are original research:
1. **Static-analyze `serial_logincheck` + `odm`** (pull from a firmware image / meraki-builder rootfs) in Ghidra for a
   parser bug reachable **before** the mfg-mode check — i.e. a command the locked prompt accepts that mishandles input.
2. **Enumerate the Local Status Page** web UI on our exact MS12 firmware for a post-CVE-2018-0284 config-injection.
Both are hardware-free and doable now; neither is guaranteed. A dumped binary makes (1) far easier but isn't strictly
required if we can obtain the firmware image another way.

## Feasibility of static-analyzing the lock (option "A") — VERDICT: blocked without hardware
Attempted to obtain the binaries that implement the lock (`serial_logincheck`, `odm`) for Ghidra analysis. Where they
could come from, and why each fails:
- **meraki-builder** (`halmartin/meraki-builder`): does NOT ship stock binaries. It builds a *replacement* buildroot
  rootfs and its `nand/extract.sh` operates on **a NAND dump you supply** (`mtd12-original.dat`, ~21 MB UBI volume).
  Its root trick is literally `sed 's;/usr/bin/serial_logincheck;/bin/ash;'` on an extracted `passwd`/`inittab` — it
  patches *around* the lock, so it never needs to understand it. Confirms the binaries live only in a real dump.
- **GPL source** (`halmartin/switch-11-22-ms220`, 146 MB): kernel-3.18 + openwrt + extern only. No rootfs, no
  `serial_logincheck`/`odm` (proprietary, stripped from GPL). Confirmed by tree + code search.
- **Public flash dumps**: none found. GitHub code/repo search (`mtd12-original.dat`, `meraki nanddump part1`,
  `meraki ms220`) and web search turned up no downloadable MS2xx NAND/NOR dump. Everyone who has one made it
  themselves via `nanddump` **after** rooting through the hardware flasher.
- **Cloud/network**: firmware is delivered TLS-encrypted from Meraki cloud to a cloud-managed device; not obtainable.

**Conclusion**: the binaries that enforce the `<Meraki>` lock exist only inside a real flash dump, and no public dump
exists. Getting one requires the **SPI/JTAG hardware route we were trying to avoid** → so "reverse the lock in Ghidra
first, cheaply, to decide whether to buy a programmer" is a chicken-and-egg dead end. Ghidra analysis (A) is fully
viable, but only *after* the first hardware dump — at which point it's worth doing (a console bug found there could
make future units hardware-free). The only genuinely hardware-free probe left is **(B) the live Local Status Page**
web UI, though our `switch-12`/2020 firmware is almost certainly past the CVE-2018-0284 fix.

## Sources
- meraki-builder (MS320 buildroot): https://github.com/halmartin/meraki-builder
- Rooting the MS220-8P (bootsh patch, CRC32/SHA1, flashrom): https://leo.leung.xyz/wiki/Rooting_the_Meraki_MS220-8P
- MS220-8P notes (layout, nandwrite, hardware flasher): https://leo.leung.xyz/wiki/Meraki_MS220-8P
- WatchMySys — modifying MS220-8P firmware: https://watchmysys.com/blog/2020/04/modifying-the-cisco-meraki-ms220-8p-firmware/
- WatchMySys — breaking secure boot on Z3/GX20 (newer devices, context): https://watchmysys.com/blog/2024/04/breaking-secure-boot-on-the-meraki-z3-and-meraki-go-gx20/
- meraki-uboot GPL source (riptidewave93): https://github.com/riptidewave93/meraki-uboot
- OpenWrt mscc target scope: https://toh.openwrt.org/
- CVE-2018-0284 Local Status Page priv-esc (fixed MS 9.37 / 10.20): https://sec.cloudapps.cisco.com/security/center/content/CiscoSecurityAdvisory/cisco-sa-20181107-meraki
- Salesforce "Meraki RCE" series (MX-focused, patched): https://medium.com/salesforce-engineering/meraki-rce-when-red-team-and-vulnerability-research-fell-in-love-3a119ce2cf56

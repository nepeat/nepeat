# small NixOS probing image

Status: the **full NixOS SD image built successfully** and passed filesystem,
boot-header, size and target-architecture checks. See the measured
[image report](nixos-image-report.json). **The clean kernel now boots into a RAM shell on hardware** with all eight
threads, 16 GiB RAM, SD, USB hub and fan/temperature readings. Management
Ethernet now passes DHCP, ping and a hash-verified TFTP transfer. PCI BARs/IRQs
and all eight NOR partitions enumerate. See [bring-up evidence](linux-bringup.md). Full NixOS
userspace has not yet booted on hardware. No SD/NOR installation or saved environment changes were made.

The standalone [flake](../../nix/flake.nix) pins current `nixos-unstable` to
`d4f25a3e562008648d45bcf2d70bcc7422cad84e` (2026-11-11), selected on 2026-11-14.
It supplies Linux **7.3.4**, GCC 16.2 and systemd 262. Builds run on x86_64;
the target is **big-endian e6500, ELFv2**, not the incompatible ppc64le/POWER8
Alpine image. GCC is configured explicitly for e6500. Rust's BE ELFv2 target
uses the generic `ppc64` CPU, rather than requiring POWER8/VSX.

This mirrors useful settings from `nepeat/nix/nixos/base.nix`: networkd,
key-only SSH with the fleet's existing public root key, Los Angeles timezone,
serial root autologin for bring-up, tmux/vim and CLI diagnostics. It does not
import the personal desktop/Home Manager/agent package graph. The personal
flake and its existing changes were not modified.

## storage budget

Measured card: **7,948,206,080 bytes** (7,580 MiB).

| item | budget/layout |
| --- | --- |
| complete sparse raw image | 7 GiB (7,168 MiB), MBR |
| FAT32 boot partition | 128 MiB, starts at 1 MiB |
| ext4 root | 7,039 MiB, starts at 129 MiB, 1% reserved |
| system runtime closure | **hard limit 3 GiB**, charged by uncompressed files |
| persistent journal | capped at 64 MiB; keep 1 GiB free |
| temporary files | tmpfs, at most 25% of RAM |
| SD swap | none |

The image builder includes a closure-budget check and reports actual ext4
free blocks. This reserves several GiB for captures, additional utilities and
update staging. Measured runtime closure upper estimate: **950,710,272 bytes (906.67 MiB)**.
The completed root has **6,337,105,920 bytes free (5.90 GiB)**, or
**6,263,300,096 bytes (5.83 GiB)** excluding its 1% reserve. The boot partition also provides a hard capacity check
when files are copied into it.

No on-switch Nix evaluator/daemon/compiler, documentation bundle, full
firmware bundle or live generation switching is included. This is an
**image-updated NixOS appliance**. Add utilities to `environment.systemPackages`
and cross-build on the workstation. Standard `nix shell`/`nixos-rebuild` on
the switch is not available in this profile. Re-enabling Nix would require
re-measuring the closure and designing store registration/update retention.

Included tools: smartctl, ethtool, lsusb, lspci, ip/ss, ping, tcpdump, i2c-tools,
sensors, lsblk/blkid/fdisk, e2fsprogs, mtd-utils, dtc/fdtget, curl, jq, file,
strace, lsof, procps, htop, tmux, vim, Python 3, libgpiod, mmc-utils and the
board's `as7760-fan` tool.

## kernel and board support

[Kernel configuration](../../nix/packages/linux-qoriq-t2080/default.nix) starts at `allnoconfig`, with common
distribution defaults and automatic module selection disabled. It retains
Book E / e6500, eight hardware threads, 64-bit physical memory, 32-bit compat,
8250 console, QorIQ clocks, FMan/DPAA management Ethernet, PCIe/MSI/PAMU,
SDHCI/eSDHC, USB EHCI/storage/UAS, FSL SATA, ext4/FAT/JFFS2/NOR, I2C/mux/EEPROM,
GPIO/SPI, sensors, autofs, loop devices, memory-pressure reporting, required
service-manager interfaces and nftables. No
DWARF/BTF kernel debug information, LTO, kernel Rust, GPU/audio/wifi stacks.

A small machine-match patch adds `accton,as7760_32x` to mainline's existing
CoreNet platform. The captured vendor DT is adapted with modern DPAA pools/clocks/ports/PCS,
legacy U-Boot LIODN fixup compatibility, a PCI CPU aperture above RAM, and
thermal mux nodes recovered from firmware. RAM boots verify the probing
interfaces listed in [bring-up notes](linux-bringup.md). ASIC DMA, SPI wiring
and unconfirmed factory DT nodes still need investigation. RX hashing stays
on (the old rxhash-off workaround was a misdiagnosis, removed 2026-11-14); PCI ASPM uses the performance policy. The factory SDK is not included, and
front-panel switching is not enabled by these kernel options.

See [thermal evidence and control](thermal.md). First boot defaults to full
fan duty; no quiet curve has been validated.

## build

From `ai_re/`, always use its devshell. Keep logs/results outside the flake
source directory so generated data does not change the source snapshot.

```sh
nix develop -c nix build path:./nix#as7760-sd-image \
  --out-link devices/edge-core-7760-32x/nixos-build/result-image-final \
  --max-jobs 2 --cores 8 --builders '' -L
```

Since 2026-11-14 the flake is `ai_re/nix` and system outputs carry an
`as7760-` prefix (`#kernel` → `#as7760-kernel`, …); see ai_re/nix/README.md.

Other outputs: `#kernel`, `#kernel-image`, `#fan-driver`, `#kernel-config`,
`#system`, `#boot-files`,
`#size-check`, `#ram-probe`. The first build must bootstrap the big-endian e6500 toolchain;
subsequent builds reuse it. Build logs are in ignored `nixos-build/`.

`#boot-files` converts ELF vmlinux to a flat binary and wraps gzip data in a
legacy PowerPC/Linux `uImage` at load/entry zero, matching the captured factory
image convention. It separately wraps the NixOS gzip initramfs as `uInitrd`.
Header and payload CRCs, gzip contents, DT magic and non-overlapping staging
windows are checked independently. `boot-commands.txt` contains explicit
`fatload`/`bootm` commands and the exact NixOS init store path, with no saveenv.

`#sd-image` copies the runtime closure, creates FAT/ext4 with root ownership,
checks both filesystems, writes an MBR sparse image and emits size reports and
SHA256SUMS. It never accesses a real block device or writes firmware.

The boot files currently expect the candidate root filesystem on the SD
card. They are **not** a self-contained RAM-root image. Build, size/CRC/filesystem checks and RAM kernel/DT probing have passed.
Full NixOS userspace still needs its own boot validation before treating the
SD image as a verified installed system. The existing
verified backups remain the recovery baseline.

## build integration fixes

- Directly copying the external fan module omitted nixpkgs's required
  per-module certificate. kbuild `modules_install` now produces it; module
  aggregation passed afterward.
- The minimal kernel initially omitted `AUTOFS_FS`, which NixOS requires in
  its systemd initramfs. Added built-in autofs and loop devices, and PSI for
  the enabled oom service. The PS/2 keyboard module request was disabled.
  Removed the experimental fixed-UART early debug console after diagnosing
  unsafe early mapping; the ordinary serial console works.
  Compared the kernel configuration with
  [systemd 262's requirements](https://github.com/systemd/systemd/blob/v262/README).
- Pinned unstable requested user units for disabled machined/nspawn/vmspawn
  components. `systemd-minimal.nix` gives those four units matching conditions;
  user-unit assembly and the full image then passed.
- The initramfs builder warns about resolving `ld64.so.2` from libc/libm.
  Independent archive inspection confirms the required ELFv2 loader is present
  at the interpreter path; this is not treated as a missing loader.

## completed artifact checks

- Final candidate: `nixos-build/result-image-final/as7760.img`, exactly
  **7,516,192,768 bytes (7 GiB)**, smaller than the measured card. SHA256:
  `2198b9765ef0befec4c740ee1627b41e8b88af4314ce9da4d6b049ada8632816`.
- Linux 7.3.4 e6500 build passed. The clean kernel is **10,876,380 bytes**
  raw; legacy uImage **4,922,999 bytes (4.69 MiB)**, load/entry 0. SHA256:
  `257a0f6949e17f82332164b5d2b64aa837284f673325c82bb335aa20b9df9571`.
- Full NixOS uInitrd: **25,139,325 bytes (23.97 MiB)** compressed, 80,352,256
  bytes unpacked. Boot files: `nixos-build/result-boot-final/`.
- Both FAT32/ext4 checks passed. Read-only loop inspection confirmed uid/gid 0
  on `/`, `/nix/store`, `/etc`, `/tmp` in the prior clean candidate using the
  same builder; `/tmp` has mode 1777. Loop detached.
- Prior clean candidate runtime audit: 428 store paths. All 1,916 executable/shared-object ELF
  files found are ELF64 big-endian PowerPC; no host executable leakage found.
- QEMU e6500 executes freshly built glibc/gawk, static musl/BusyBox, systemd,
  ethtool, gpiodetect, sensors, smartctl, lspci and lsusb. CPU/ABI smoke checks
  only, not whole-board emulation or hardware-access tests.
- Fan module compiled/modposted/linked against the actual kernel, with its
  nixpkgs module certificate. No unresolved-symbol override used.
- Header checks cover CRCs, gzip data, newc format, DT magic and staging windows.
  Budget checker accepts a small fixture and rejects a sparse 4-GiB file.

## netboot: full NixOS in RAM (boot-tested)

`#netboot` (`netboot-root.nix` + `netboot-files.nix`) evaluates the same
configuration with `/` on tmpfs and `/nix/store` = overlay(tmpfs, zstd
squashfs loop-mounted from the initramfs). Volatile journal. Bundle: `uImage`
(same kernel), `uInitrd` (`-C none` legacy header around gzip stage 1 + raw
squashfs cpio, 4-byte aligned), `as7760.dtb`, `bootargs.txt`, `manifest.json`.
Booted to multi-user in 23 s with no failed units on 2026-11-14; see
PROGRESS.md and `nixos-netboot-boot-20261114.txt`. The SD variant now gets
its filesystems from `sd-root.nix`.

## RAM probe and hardware boot

`#ram-probe` builds a self-contained static **musl/BusyBox** root plus the fan
module, kernel/DT and native ethtool/lspci/lsusb/sensors with their Nix library
closure. It is not a stock Alpine distribution or an apk package
repository; it supplies a small RAM environment while the NixOS build remains
the requested full system. No storage is mounted by its init script. The
final diagnostic initramfs is about 40.9 MiB compressed. It configures management
DHCP in RAM and allows 90 seconds for link/upstream readiness. The older minimal
probe was about 881 KiB.

Final bundle: `nixos-build/result-ram-probe-final/` (`uImage`, `uInitrd`,
`as7760.dtb`). At an idle U-Boot prompt:

```sh
nix develop -c python3 devices/edge-core-7760-32x/load_ram_probe.py \
  devices/edge-core-7760-32x/nixos-build/result-ram-probe-final/uImage --boot
```

Watch with `nix develop -c tmux attach -t serial-7760`.

Use the persistent `serial-7760` tmux console. Current trial log:
[ram-probe-boot-20261114.txt](ram-probe-boot-20261114.txt). TFTP serves only
public build artifacts from `/srv/tftpboot/as7760-probe/`; original firmware
and private environment data are not served.

Initial blank boots were traced with temporary UART markers, then a temporary
mapping patch exposed a panic in early `ioremap` allocation. The `earlycon`
argument reached that path before normal Book E memory initialization. The
fixed-UART early-debug hook had the same ordering problem. Removing both
boots the uninstrumented kernel successfully; retain only the normal serial
console. Diagnostic binary patches are excluded from release artifacts.

The first clean boot detected eight threads, 16 GiB RAM, SD and USB. The fan
module loaded and returned twelve RPMs and four board temperatures. A full
PWM write/readback passed. Modern DT adaptations and AMD CFI command-set support now expose the NOR
partitions, PCI BARs/IRQs and management Ethernet. Full-memory DHCP and ping
passed, along with a kernel-sized TFTP download whose SHA256 matched. See PROGRESS.md for the latest measured hardware status.

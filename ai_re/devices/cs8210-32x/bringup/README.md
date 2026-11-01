# experimental serial-only Linux 7 bring-up

**Built and tested to an interactive shell on QEMU virt; not booted on CS8210.**
QEMU supplies its own DT, RAM map, firmware and PL011 UART. It does not validate
our CS8210 DTB, 8250 UART, installed U-Boot, hardware clocks, RAM reservations,
interrupt wiring, cooling, PCI or networking.

**Review status:** the [Fable review triage](../reviews/2026-10-17-triage.md)
found an earlycon divisor bug and missing vendor memory reservations. Those
corrections remain outstanding; the artifact hashes below identify the reviewed,
unchanged candidate. Resolve these before a hardware trial, and check the timer
frequency and bootargs handoff described in the review.

## artifacts

Outputs live in ignored `../analysis/research/serial-build/`:

| Artifact | Size | Purpose |
| --- | ---: | --- |
| `arch/arm64/boot/Image` | 5,820,424 bytes | Uncompressed upstream 7.2.11 ARM64 kernel with embedded initramfs |
| `Image.gz` | 3,088,579 bytes | gzip payload |
| `uImage` | 3,088,643 bytes | ARM64/Linux/gzip legacy wrapper, load/entry `0x04000000` |
| `cs8210-serial.dtb` | 7,113 bytes | Compiled board candidate |
| `.config`, `manifest.json`, `arm64-header.json` | — | Actual config, hashes, image-header/load evidence |
| `qemu-smoke.txt`, `qemu-command.json`, `qemu-result.json` | — | Complete emulator transcript, invocation and outcome |

The Image's reserved in-memory size from its header is 6,094,848 bytes, larger
than the on-disk payload because it includes runtime zero-filled storage.
The uImage SHA-256 after the load-address correction is
`9fb9a5cae551db4b89bfa7c6369fab9e068da9b65d5f2d8300a81131265f39ba`.

The build uses no vendor kernel patches. `serial.config` selects Alpine ARM64,
16-CPU capacity, PSCI, GICv3, 8250 serial, initramfs and basic userspace. PL011 is
also enabled solely for the emulator test. PCI, storage, networking, SPI, I2C,
MTD and watchdog drivers are disabled. Thus this build cannot establish SSD
or Ethernet support; those are later increments.

`init` mounts devtmpfs/proc/sysfs/tmpfs, reports kernel/memory information, and
starts an interactive shell. Its BusyBox and glibc 2.25 runtime come from the
captured ONIE image. No vendor installer, daemon, board module, SDK or firmware
update is included. Shell job control is unavailable on `/dev/console` in this
minimal setup, but commands work. This is a diagnostic userspace, not a proposed
production distribution.

## rebuild and validate

Run from the repository root. The already downloaded/extracted inputs are
`analysis/kernel-review/linux-7.2.11/` and the captured SSD's ONIE updater.
The source archive checksum is retained under `analysis/kernel-review/`.

```sh
nix develop ./ai_re --command python3 \
  ai_re/devices/cs8210-32x/scripts/unpack_onie.py \
  ai_re/devices/cs8210-32x/analysis/extracted/root/image/onie/onie-updater-arm64-celestica_cs8210-r0 \
  ai_re/devices/cs8210-32x/analysis/research/onie

nix develop ./ai_re --command python3 \
  ai_re/devices/cs8210-32x/scripts/build_serial.py --jobs 8

nix develop ./ai_re --command python3 \
  ai_re/devices/cs8210-32x/scripts/smoke_serial.py
```

Validation completed:

- Cross-compiled Image with GCC 15.3.0 / binutils 2.46.
- DT preprocessing and dtc compilation, with no dtc diagnostics. This is not
  a full dt-schema validation.
- Required config options checked after Kconfig dependency resolution.
- QEMU Cortex-A72, GICv3, PSCI and four CPUs boot to `/init`; shell executes
  `uname`, reads uptime and lists CPU0..CPU3 in sysfs. Virtualization-enabled
  QEMU enters the kernel at EL2.
- First emulator attempt found a missing `/lib64 -> lib` symlink required by
  ONIE's loader. Added it, rebuilt, and the next test passed.
- Legacy uImage header and payload CRCs validated independently.

## candidate DT and future RAM layout

`cs8210-serial.dts` includes upstream `amazon/alpine-v3.dtsi`, adds UART0 at
`0xfd883000` with a 500-MHz clock and ttyS0/115200 console, and keeps the upstream
1-MiB secure-monitor reservation. Its 1-GiB memory entry is a conservative
placeholder copied from the packaged vendor DT, not a measurement. Firmware
may rewrite it. Capture the live firmware DT and RAM reservations first.

PCI and MSI are disabled in the candidate DT. MSI carries the vendor range
160/800 plus the missing interrupt-controller declaration for later work;
those settings have not been exercised. No storage-capable DT is supplied yet.

Proposed non-overlapping placement, conditional on live RAM/firmware checks:

| Item | Address |
| --- | --- |
| Uncompressed kernel destination / entry | `0x04000000` |
| DTB staging | `0x10000000` |
| Compressed uImage staging | `0x30000000` |
| Initramfs | Embedded in kernel; no separately loaded archive |

The Image header has `text_offset=0`; loading it at the stock `0x04080000`
would violate the documented 2-MiB-base alignment. The build script derives
load/entry from the header. These placements fit the placeholder 1-GiB range
and avoid mutual overlap, but may still conflict with live firmware use until
verified. Do not copy DMC12's DT staging address or the old kernel's load address
without checking the actual images.

## first hardware session

1. Confirm UART electrical levels/pinout; start a logged tmux+tio session.
2. Capture a full stock boot, U-Boot version/environment/memory report, runtime
   DT, stock PCI config, board identity, fan RPM/temperatures and watchdog state.
3. Back up installed flash and EEPROMs before persistent modification. The
   updater is not an installed-flash backup.
4. Establish observed cooling/watchdog behavior and reliable power recovery.
5. Check bootm capability, RAM placement and firmware DT fixups. Stage the
   separately named candidate in RAM and try it without saving environment or
   replacing factory boot files.
6. Once console/SMP/timers work on hardware, incrementally add board monitoring,
   PCI coherency setup, AHCI/USB, management Ethernet, then SDK integration.

The exact live boot commands should be constructed from that captured
environment. No unverified persistent boot-script change is included.

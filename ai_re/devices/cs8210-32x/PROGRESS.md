# CS8210-32X

## identity

- device model: Celestica CS8210-32X (user identification; Celestica CS8210
  confirmed by the SSD's device tree and diagnostic package). Codename: Briggs.
- chassis S/N: `C8210F2B02211113T0029` (provided label photo).
- SSD: Western Digital Ultrastar DC SA210, 120 GB M.2 SATA.
- SSD model: `HBS3A1912A4M4B1`; S/N: `A069BCC5`; P/N: `0TS1653`.
- SSD firmware: `M6102100`; exact capacity: 120,034,123,776 bytes.
- acquisition attachment: USB adapter, `/dev/sdb`; serial confirmed against photo.
- platform software: ARM64, Annapurna Labs Alpine; Innovium/Teralynx SDK.
- serial console: image configures ttyS0 at 115200, 8-bit; actual console baud,
  framing, pinout and logic levels have not been measured. See analysis caveat.
- board components and power: not physically inspected yet.

## current status

Full SSD acquisition and initial offline analysis completed. The archive passed
full decompression, frame checksum validation, exact length checking, and SHA-256
comparison against the original acquisition stream. The original SSD was never
mounted or written; it remains kernel-read-only until unplug/reconnect.

The SSD holds an Ubuntu 14.04.6 ARM64 **Briggs DIAGOS 0.1.1** environment, with
Linux 4.9.124, ONIE/U-Boot update images, CPLD/FPGA firmware, board diagnostics,
and Briggs SDK 0.1.0 and 0.1.2. Six other filesystems contain only `lost+found`.
No separate production NOS installation was found in the allocated files.

See [ssd-analysis.md](ssd-analysis.md) for partition layout, boot architecture,
useful RE entry points, and limits of the findings.

Kernel review: see [kernel-review.md](kernel-review.md). Recovered the complete
4.9.124+ config and historical boot logs. Upstream 7.2.11 supports the Alpine v3
CPU/interrupt/PCI/AHCI foundation; DMC12's mainline-plus-board-DTS approach is a
useful template. Serial/initramfs boot is plausible; v3 Ethernet, board control
and Innovium switching need separate work. Follow-up [offline research](offline-research.md)
found public Alpine v3 HAL source and decoded watchdog/fan controls. An unmodified
mainline 7.2.11 [serial candidate](bringup/README.md) now builds and boots to an
interactive shell on QEMU virt. **No CS8210 hardware boot has been attempted.**
The [Fable review triage](reviews/2026-10-17-triage.md) identifies outstanding
earlycon and memory-reservation corrections before that candidate is tried.

## artifacts

- `images/ssd-2026-10-17.img.zst`: full-device image, 1,677,810,900 bytes
  compressed from 120,034,123,776 bytes (98.6% smaller).
- `evidence/`: acquisition metadata, partition table, SMART output, verification.
- `photos/`: original HEIF label photos and converted PNGs.
- `analysis/extracted/`: 856 selected firmware, SDK, diagnostic and identity
  files, 268,351,514 bytes logical size; hashed in `evidence/extracted-manifest.json`.
- `analysis/celestica_cs8210-r0.dts`: decompiled board device tree.
- Temporary raw analysis image was removed after inspection. All mounts and loop
  devices were released; only the zstd full-disk image is retained.
- `scripts/acquire.py`, `scripts/verify.py`: acquisition and verification methods.

Binary images, photos, and raw evidence are local-only and ignored by git. They
can contain device configuration or secrets. Summarized findings belong here.

## log

### 2026-10-17 — Claude Code / Fable review

- Completed the requested broad review with `claude-fable-5-1`, plus a focused
  earlycon source check using the same model. The broad review's internal advisor
  timed out; Fable itself returned a completed assessment.
- Confirmed incorrect explicit-baud earlycon divisor and omitted vendor no-map
  reservations. Identified packaged 50-MHz versus logged 75-MHz timer discrepancy
  and the need for explicit temporary bootargs handoff.
- Independently checked the findings: rejected the missing DMC12 address claim
  and alleged variant-specific Marvell switch difference; did not accept the
  blanket inference that stock devices all used legacy interrupts.
- Saved [original review](reviews/2026-10-17-fable.md),
  [focused UART review](reviews/2026-10-17-fable-uart.md) and
  [triage with evidence and follow-ups](reviews/2026-10-17-triage.md).
- Documentation-only review. DTS, kernel config, init, scripts and artifacts
  remain unchanged; corrections are pending. No SSD or switch access this turn.

### 2026-10-17 — deeper offline research and first mainline build

- Found public MikroTik GPL patch with Alpine v3 HAL 2.6; recovered the exact
  revision-4-to-2 standard-Ethernet compatibility path and PCI coherency setup.
- Safely unpacked ONIE updater/FIT/rootfs as data, validated payload SHA-1 and
  FIT CRCs, and recovered platform startup/firmware installation details.
- Mapped both I2C trees and the board-variant discriminator. Recovered and
  emulated system/fan watchdog and inverted max-fan setter behavior with stubbed
  I2C, producing reproducible register traces. See [board-control.md](board-control.md).
- Recovered 16 Innovium IPD ABI/support type layouts and 26 ioctl dispatch targets
  from module DWARF/disassembly; complete command semantics remain unfinished.
- Added a compact serial-only DTS, kernel config, initramfs, build script and QEMU
  smoke test. Linux 7.2.11 reaches an interactive shell with four emulated A72
  CPUs at EL2. This does not validate the actual board or board DTB.
- Fixed the initramfs library-path symlink after the first emulator boot failed.
  Corrected uImage load/entry to 0x04000000 based on the modern ARM64 Image header;
  stock 0x04080000 assumes a different text offset. CRCs/alignment verified.
- Added U-Boot tools and QEMU to the devshell for packaging/offline validation.
- Renamed `scripts/inspect.py` to `inspect_ssd.py`: the old filename shadowed
  Python's standard-library inspect module when argparse imported dataclasses.
  The accidental import attempted read-only setup but failed because the old
  temporary raw image was already deleted; no loop or mount was created.
- No source SSD read, switch connection, flashing or external posting this turn.
  Updated findings, bounded source-search results and live checks in
  [offline-research.md](offline-research.md).

### 2026-10-17 — kernel and upstream portability review

- Validated vendor uImage CRCs, decompressed its ARM64 Image and recovered IKCONFIG.
- Compared both DTBs, module metadata, and vendor config against Linux 7.2.11.
- Inspected user's DMC12 repository at `5d02d1b3de04082aea3960cd74db0c24f5b3ed32`.
  Recorded reusable MSI, clock, PCIe and U-Boot packaging details and v2/v3 gaps.
- Recovered historical dmesg from the compressed image: 16 Cortex-A72 r0p3 CPUs,
  PSCI 0.2, roughly 7.75 GiB described RAM, NIC revision 4 handled via revision-2
  compatibility. These are stored logs, not new live hardware measurements.
- Recorded firmware handoff and load-address checks needed before a 7.x trial.
- All work stayed offline from the switch; temporary partition image removed.

### 2026-10-17 — initial SSD acquisition

- Created this device project at the user's request.
- Confirmed `/dev/sdb` model, serial, exact size, and that no partitions were mounted.
- Set the kernel block device read-only before acquiring; no writes to SSD media.
- Acquired the entire disk, including free/unpartitioned space, through SHA-256
  and `zstd -T4 -6 --check`, 20:41:01–20:45:43 UTC. No read errors and no zero
  substitution. Raw SHA-256:
  `4681fc3f7ab837ea51dea780834aa0336563f70adb361791d226b0b719fb21c2`.
- SMART autodetection failed through the USB adapter; explicit `-d sat` worked.
  SMART health reports PASSED, 74 power-on hours, 161 power cycles, zero reported
  reallocated sectors, uncorrectable errors, or interface CRC errors. No recorded
  SMART errors or self-tests. Vendor-specific attributes are not decoded reliably
  because this SSD is absent from the tool's database.
- The disk uses a DOS/MBR table with seven Linux filesystems and an extended
  partition. All seven were inventoried from the verified copy using read-only
  loop devices and `ro,noload,nosuid,nodev,noexec` mounts. No image programs ran.
- Full archive verification completed at 20:47:12 UTC. Compressed SHA-256:
  `0f046f54676b35a82e3bdd5dc184fb60b7da5a017fa4df26137505c2a7c66cd5`.
  Both raw and compressed checksums are also saved under `images/`.
- Saved original label photos and converted their HEIF content to viewable PNGs.
- Added zstd, smartmontools and e2fsprogs to the existing ai_re devshell for this
  acquisition and filesystem inspection. All project tooling ran in that shell
  after reading the repository instructions.

## next useful work

- Capture the real switch's serial boot log and read-only U-Boot environment.
- Back up onboard SPI flash separately; the SSD only contains update packages.
- Map the SDK's port configurations and Python API wrappers to the hardware.
- Confirm physical board revision and console electrical levels before probing.

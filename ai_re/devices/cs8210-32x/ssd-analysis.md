# CS8210-32X SSD analysis — 2026-10-17

## result and provenance

This is a factory/development diagnostic disk for Celestica CS8210 (Briggs).
The only populated filesystem is partition 3. It contains Ubuntu 14.04.6 LTS
ARM64, Briggs DIAGOS 0.1.1, a board-specific Linux kernel, diagnostic programs,
hardware firmware packages, and switch SDKs. No separate production NOS was
found in the allocated filesystem inventory.

Findings below come from the acquired SSD, not web specifications. Paths beginning
with `/root` or `/etc` refer to paths **inside the switch image**, not this host.
Selected files are available under `analysis/extracted/` with the same hierarchy.

Acquisition: `/dev/sdb`, SSD `HBS3A1912A4M4B1`, serial `A069BCC5`, all
120,034,123,776 bytes. Original device unmounted and kernel-read-only. SHA-256
was calculated while reading, then compared against the complete decoded zstd
stream. Both length and hash matched, and zstd's frame checksum passed. No read
errors. The archive is 1,677,810,900 bytes (1.56 GiB), a 98.6% reduction.

- image: `images/ssd-2026-10-17.img.zst`
- metadata: `evidence/acquisition.json`, `evidence/verification.json`
- checksum files: `images/SHA256SUMS`, `images/RAW-SHA256SUMS`
- inventories: `evidence/pN-files.txt`, `evidence/pN-superblock.txt`
- extracted-file hashes: `evidence/extracted-manifest.json`

## partition layout

DOS/MBR, 512-byte sectors. MBR signature is `55aa`; its first 446 bytes are zero,
so there is no conventional MBR boot code. Partition numbers below are preserved
from the original disk. p2 contains logical partitions p5–p9.

| partition | first sector | sector count | size (GiB) | filesystem / observed contents |
| --- | ---: | ---: | ---: | --- |
| p1 | 63 | 18,185,517 | 8.672 | ext3; only `lost+found` |
| p2 | 18,185,580 | 63,697,725 | 30.373 | extended container |
| p3 | 81,883,305 | 3,919,860 | 1.869 | ext3; diagnostic root filesystem |
| p5 | 18,185,643 | 18,185,517 | 8.672 | ext4; only `lost+found` |
| p6 | 36,371,223 | 18,185,517 | 8.672 | ext4; only `lost+found` |
| p7 | 54,556,803 | 3,919,797 | 1.869 | ext4; only `lost+found` |
| p8 | 58,476,663 | 32,067 | 0.015 | ext4; only `lost+found` |
| p9 | 58,508,793 | 23,374,512 | 11.146 | ext4; only `lost+found` |

The final partition ends at byte 43,931,220,480. The trailing 76,102,903,296
bytes are unpartitioned and included in the image. Empty directory inventories
do not prove that free blocks contain no deleted data; no undelete/carving pass
was performed. All seven superblocks report clean state. That is not a full
filesystem consistency check. Recorded timestamps include January 2000 and May
2021; these may reflect factory clock settings or a cloned template.

## OS and hardware clues

| evidence | observation |
| --- | --- |
| `/etc/os-release` | Ubuntu 14.04.6 LTS, Trusty Tahr |
| `/etc/diagos_version`, `/etc/releaseNotes` | Briggs DIAGOS 0.1.1; release notes dated 2021-04-01 |
| `/bin/bash` ELF header | AArch64 executable |
| `/root/uImage` header | Linux/ARM64 4.9.124; image date 2021-04-02; load/entry `0x04080000` |
| `/root/celestica_cs8210-r0.dtb` | model `celestica,cs8210`, compatible `annapurna-labs,alpine`, DT version 3.47.312 |
| `/root/diag/configs/cpu.yaml` | expected 16 cores at 2000 MHz; actual hardware not measured |
| `/root/diag/configs/pcis.yaml` | Innovium controller expected at PCI ID `1d98:1b58`, 8 GT/s x4 |
| SDK `libifcs.so` symbols | Teralynx-specific functions; exact ASIC generation not independently established |
| `/root/diag/fru.log` | stored FRU data mentions Annapurna Labs K2X-N; not a live FRU read |

The CPU YAML's descriptive `armv8 cortex-a15` string is internally questionable;
do not use it to identify a precise CPU microarchitecture. The DTB and ELF headers
are stronger evidence for the ARM64/Alpine platform. DT memory defaults likewise
do not establish installed RAM capacity.

## boot and console

`/etc/fstab` identifies `/dev/sda3` as the ext3 root filesystem. `/boot` is empty;
the diagnostic kernel and board DTBs are under `/root`. Vendor READMEs describe
booting from U-Boot using `diagos_bootcmd` and booting ONIE from onboard flash.
One README spells the diagnostic command `diag_bootcmd`; neither spelling has
been tested on this unit. The actual saved boot environment is outside this SSD.

`/etc/init/ttyS0.conf` runs getty with `-8 115200 -L ttyS0 vt100`. The device tree
uses `console=ttyS0` and an early UART at `0xfd883000`. However, the DIAGOS release
notes also describe changing a SysCPLD console logger default to 9600. Thus 115200
is the diagnostic OS's configured baud, not a confirmed physical-console setting.
Pinout, voltage levels, parity and stop bits have not been measured.

The DTB describes a separate 32 MiB SPI boot-flash layout:

| label | offset | size |
| --- | ---: | ---: |
| uboot | `0x000000` | `0x380000` |
| uboot-env | `0x380000` | `0x040000` |
| unused | `0x3c0000` | `0x040000` |
| board_eeprom | `0x400000` | `0x010000` |
| onie | `0x410000` | `0x1bf0000` |

It also describes a separate SPI auxiliary flash for `i2cfpga` (32 MiB) and an
Infineon SLB9670 TPM. These are device-tree descriptions, not physical reads.
`/etc/fw_env.config` refers to `/dev/mtd1`. The SSD image does **not** back up these
flash devices, current U-Boot environment, or live EEPROM contents.

The supplied U-Boot README has a questionable OS flashing example targeting
`/dev/mtd4`, while the DTB assigns the ONIE region to partition 4. Treat its
flashing examples as unverified; no update or flash command was executed.

## useful RE artifacts

- `/root/image/onie/`: ONIE updater (30,568,311 bytes) and FIT image
  `celestica_cs8210-r0.itb` (26,833,084 bytes), plus update notes.
- `/root/image/uboot/`: `celestica_cs8210-boot.img` (2,583,025 bytes) and notes.
- `/root/image/os/`: Linux uImage, ordinary board DTB, and a `_12v_21` DTB whose
  README identifies it as the 21-inch variant. This does not establish which
  physical variant the user's chassis is.
- `/root/fw/`: system, fan and LED CPLD images; I2C and MDIO FPGA images; ASC
  controller configuration. Includes `briggs_sysfpga_v13_0330.vme` and
  `i2c_fpga_v01.02.0E_20200604.bin`.
- `/root/driver/`: board-specific kernel modules and installation script, covering
  SFPs, CPLDs, I2C/MDIO FPGA, fans, power supplies and regulators.
- `/root/diag/`: compiled `cel-*` tests and YAML maps for buses, devices,
  EEPROMs, sensors, fans, rails, LEDs and PHYs. Useful hardware-map starting point.
- `/root/sdk/`: Briggs SDK packages 0.1.0 and 0.1.2. The 0.1.2 tree contains
  `innovium.user`, `libifcs.so`, `ipd.ko`, Python CLI/API wrappers, port YAML,
  Credo PHY code/firmware, forwarding control and traffic-test scripts.
- SDK 0.1.2 `auto_load_user.sh` defaults to `PAM4_400G_32`; other listed modes
  include 128x100G, 32x100G, 32x40G, 128x25G and 128x10G. `dynamic_config.sh`
  supports mixed port/breakout configurations. These are software options,
  not link modes verified on the hardware during this session.
- `/root/sdk/onie_sdk_init.sh` selects SDK 0.1.2. Root-level forwarding-control
  symlinks also select that version.

The image contains root shell history, SSH host keys, and existing SDK logs.
SSH configuration enables password authentication and root login, but disallows
empty passwords. No account passwords were tested or changed. History and host
private keys were left inside the compressed image, not copied into tracked notes.
Raw evidence and extracted vendor files remain private local artifacts ignored
by git. No binaries or scripts from the switch were executed.

## storage and repeat inspection

The temporary raw image was deleted after analysis; all mounts and loop devices
were released. Selected files occupy about 111 MiB on this host's compressed ZFS
dataset (268 MB logical size). Original label photos are retained in `photos/`.

From the repository root, regenerate and inventory a temporary verified copy:

```sh
nix develop ./ai_re --command python3 ai_re/devices/cs8210-32x/scripts/verify.py
nix develop ./ai_re --command python3 ai_re/devices/cs8210-32x/scripts/inspect_ssd.py
```

`verify.py` refuses to overwrite an existing working image. `inspect.py` mounts
read-only with journal replay disabled and cleans up its loop device and mounts.
Remove the regenerated `analysis/ssd.img` after finishing if space is preferred.

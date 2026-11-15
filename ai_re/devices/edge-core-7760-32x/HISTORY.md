# Edge-Core 7760-32X — archived journal

Older [PROGRESS.md](PROGRESS.md) entries, moved verbatim on 2026-11-14 (paths
like `nixos/…` now live under `ai_re/nix/`). Newest first.

## 2026-11-14 (night) — NixOS on the SD card (RAM root, near-zero SD writes); SD identity; U-Boot feasibility

- **SD card replaced with the RAM-root NixOS boot image** after re-verifying the
  original backup (`dumps/mmcblk0.img` SHA256 `71d5a5e6…`) and the card's
  CID. 1-GiB image (`#sd-image`, FAT32 `NIXOS_BOOT` only), readback SHA256
  match `203139d2…`. Older ext4-root layout kept as `#sd-image-ext4`.
- Boot: `powercycle_to_uboot.py` then [boot_sd.py](boot_sd.py), which
  `fatload`s `boot.scr` and `source`s it. Volatile, **no saveenv**: SD
  is not the default boot yet. Kernel+241 MiB initrd load in ~11 s; 15.8 s to
  multi-user; 0 failed units.
- **SD wear**: `/boot` vfat mounted `ro`; root tmpfs; journal volatile with
  RuntimeMaxUse=1G; no swap/fstrim/nix. First boot wrote 10 sectors (one-time
  SSH host key into `/boot/persist`); **second cold boot: 0 writes**, still 0
  after 4 min. Host key and machine-id (`4868c596…`) stable across boots.
- [SD identity](sd-card.md): genuine **SanDisk SDHC** (CID MID 0x03/OID "SD",
  PNM SA08G, 04/2018), SD-only SCR/SSR registers present, Class 10/U1/V10,
  card-detect line asserted. Socket vs soldered: needs a physical look.
- [U-Boot feasibility](u-boot-port.md): **secure boot not fused** (ITS=0, SRKH=0,
  SB_EN=0, non-E T2080). Upstream 2026.10 T2080RDB NOR layout identical to this
  board's. ONIE's AS7710-32X GPL port documents the deltas. Recovery path is
  the open question; nothing written to NOR. Stock upstream T2080RDB U-Boot
  2026.10 **builds via nixpkgs** (`#uboot-t2080rdb`): u-boot.bin is exactly
  768 KiB (= mtd7). Build-only; not flashed or run.

## 2026-11-14 (late evening) — "KeyGen/rxhash RX failure" was a misdiagnosis; workaround removed

- Register probe [fman_kg_probe.py](fman_kg_probe.py) (`/dev/mem`, read-only
  apart from KeyGen AR *read* commands), output
  [fman-kg-probe-20261114.txt](fman-kg-probe-20261114.txt):
  - BMI RX port 0x08 `fmbm_rfpne` = `0x00480000` (NIA_ENG_HWK) in **every**
    phase, rxhash on or off. `ethtool -K eth0 rxhash` only flips
    `NETIF_F_RXHASH` (whether the driver copies the hash into the skb); it never
    bypassed KeyGen. KeyGen hashed every frame all along.
  - Scheme 0: enabled, fqb 128, hc 127 (128 PCD FQs 128–255), port 8 partition
    bit set, no KG error/scheme-error events. KeyGen packet counter == driver
    rx packets + rx dropped exactly (442776 = 53277 + 389499).
  - With rxhash **on**: pings 5/5, RX spread over all 8 per-CPU portals.
- **Cold boot with the networkd pre-start workaround removed**, hashing on:
  DHCP unattended, LAN 20/20, internet 5/5, 0 failed units. 256 MiB to the
  switch over ssh ~52 MiB/s, SHA256 match; 256 MiB from it ~82 MiB/s; **0 RX
  errors**. Workaround removed from `configuration.nix` (RAM probe init still
  carries it; harmless). Original RAM-probe RX failure most likely the short
  18-s DHCP window/upstream readiness, not hashing; not re-tested on that root.
- **"Stack RX drops" explained:** ~293 frames/s of 802.1Q-tagged multicast
  (mDNS/SSDP reflected by the gateway on VLANs 1/3/4/20/621…) arrive because
  the upstream port is a trunk; kernel has no 8021Q, so `netif_receive_skb`
  returns NET_RX_DROP (the driver's only rx_dropped path). ~38 frames/s
  untagged. Harmless; fix on the switch (untagged/access port) if wanted.
  [rx_traffic_mix.sh](rx_traffic_mix.sh) reproduces the classification.
- Open: PCIe AER *Unsupported Request* logged by ASIC 01:00.1 at uptime 1512 s
  (TLP MRd, requester 0000, bus addr 0xdf5594c0 — outside both functions' BARs,
  which sit at bus 0xd8000000–0xd9ffffff). Time coincides with the probe run,
  which only maps FMan CCSR; cause unknown. Another AER pair at 3.5 s boot.
- Root on sea420-desktop (`root@sea420-desktop` key) and the usual GitHub keys
  are now authorized; ssh with `-o UserKnownHostsFile=/dev/null` because host
  keys regenerate on every RAM boot.

## 2026-11-14 (evening) — full NixOS userspace boots from RAM (netboot)

- **Full NixOS 26.11 userspace boots on hardware**, non-destructively: new
  `#netboot` flake output carries the whole system closure as a zstd squashfs
  (218 MiB) inside the initramfs, tmpfs/overlay on top. TFTP + volatile
  `bootm`; **SD not mounted, no NOR/SD/saveenv writes.**
  [Boot capture](nixos-netboot-boot-20261114.txt).
- Startup 23.2 s (2.7 kernel + 8.0 initrd + 12.5 userspace). **Zero failed
  units.** systemd stage-1 initrd, switch-root, udev, journald, networkd,
  resolved, timesyncd (synced), nftables, logind, serial root autologin all OK.
- networkd pre-start `ethtool -K eth0 rxhash off` works on hardware
  (`receive-hashing: off`); eth0 DHCP 10.60.2.253 unattended.
- `sshd.service` active; workstation reached key exchange, then was refused
  because only the `erin-laptop` key is authorized (expected, not a fault).
- `as7760-thermal` service active: PWM 255, all 12 rotors 17.9–21.7k RPM, no
  faults, four LM75s 28.0–30.0 C. Full-speed policy unchanged.
- First attempt failed: `Initramfs unpacking failed: invalid magic` → squashfs
  missing → emergency mode (root locked). Cause: the uncompressed squashfs cpio
  followed the gzip stage-1 member at a non-4-byte-aligned offset, which the
  kernel's initramfs parser rejects. Fixed by zero-padding stage 1 to 512 bytes;
  bundle build now asserts the alignment. Also enabled
  `boot.initrd.systemd.emergencyAccess` for bring-up.
- Kernel gained SQUASHFS(+ZSTD, per-CPU decomp) and OVERLAY_FS. This **changes
  the kernel for every output**: booted netboot uImage SHA256 `9c49869d…`;
  the older `result-ram-probe-final`/SD image links still hold the previous
  kernel. SD layout moved to `sd-root.nix` (no content change intended);
  SD image not rebuilt in this session.
- `load_ram_probe.py` now takes `bootargs.txt` from the bundle when present.

## 2026-11-14 — Linux 7.3.4 RAM boot and management networking work

- **Clean kernel boots into a serial RAM shell**, all eight e6500 threads,
  16 GiB RAM, original SD card and USB hub detected. No SD/NOR installation
  or saved environment writes. [First successful boot](linux-7.3.4-first-boot-20261114.txt).
- Removed `earlycon` and fixed-UART early-debug hooks: these attempted early
  I/O mappings before the Book E memory allocator/mapping setup was ready.
  Ordinary `console=ttyS0,115200n8` works; release kernel has no trace patches.
- **Thermal hardware verified in Linux:** four LM75 temperatures 24.5–26.5 C,
  twelve fan rotors around 18–22k RPM. Writing full PWM 255 read back correctly;
  no reduced-speed test. [Thermal evidence](thermal.md).
- NOR now enumerates all eight partitions with AMDSTD. Read hashes of RCW,
  hw-info, FMan firmware, U-Boot and the SD partition sector match backups.
- Modern DPAA clocks/pools/port/PCS bindings bring up eth0 at 1 Gbps. TX and
  completions work. **Disabling RX hashing restores reception**; preserved legacy
  compatible tags also let U-Boot inject LIODNs. With all 16 GiB available, DHCP,
  workstation/gateway pings (zero loss), and a 4,922,999-byte TFTP transfer with
  matching SHA256 pass. Underlying KeyGen issue unresolved; startup disables
  hashing. Final probe cold-boot DHCP succeeds unattended with a 90-second
  retry window; shell remains available via `serial-7760`.
  [Final boot](linux-7.3.4-final-boot-20261114.txt).
  Stack RX drops are observed, but DMA/frame/driver RX errors stay zero.
  [Bring-up details](linux-bringup.md), [network evidence](linux-7.3.4-network-boot-20261114.txt).
- PCI aperture moved above RAM to 0xc60000000; both ASIC BAR sets assigned.
  ASPM performance policy fixes intermittent configuration reads and permits
  IRQ assignment on cold boot. VPD read fails; ASIC DMA/forwarding untested.
- USB hub 0424:2514 and four GPIO character devices enumerate. No SPI controller
  is instantiated by the captured DT; its old GPIO-backed board hookup remains
  to recover. RTC enumerates with oscillator disabled; not set during probing.
- Added target ethtool/lspci/lsusb/sensors to RAM probe (~40.9 MiB compressed);
  these run on hardware. [Detailed tools trial](linux-7.3.4-tools-boot-20261114.txt).
  Workstation devshell adds poppler-utils for reference-manual register searches.
- Full **7-GiB NixOS image built**, runtime closure 906.67 MiB, root about
  5.90 GiB free. Filesystem and ABI checks passed; full NixOS userspace has
  **not yet booted on hardware**. [Measured image report](nixos-image-report.json),
  [build notes](nixos.md), [ongoing serial capture](ram-probe-boot-20261114.txt).

## 2026-11-14 — first Linux 7 RAM boot attempt

- Built static musl/BusyBox RAM root (about 881 KiB compressed), e6500 kernel
  (4.69 MiB legacy uImage), board DT and fan module. Both userspace execution
  under QEMU e6500 and independent image CRC checks passed.
- Loaded all three artifacts over TFTP, using DHCP 10.60.2.253 and workstation
  10.60.2.208. U-Boot verified both image checksums, decompressed the kernel,
  relocated initramfs/DT, then produced **no Linux serial output**. This is
  not a successful boot. Added the fixed-UART early debug console for diagnosis.
- [Boot attempt capture](ram-probe-boot-20261114.txt). Power-cycled back to
  U-Boot and intercepted autoboot through tmux. No SD/NOR or saved environment
  writes. Use paced [serial commands](serial_command.py): bursts can lose
  characters while this bootloader echoes or computes checksums.
- NixOS system build continues; an additional built-in autofs dependency and
  the external-module certificate packaging have been corrected. Final image
  and on-hardware runtime verification remain outstanding.

## 2026-11-14 — minimal NixOS build and thermal reverse engineering

- [NixOS configuration/build notes](nixos.md): latest nixos-unstable, e6500
  big endian ELFv2, Linux 7.3.4, minimal drivers and probing tools. Derivations
  evaluate; first cross-build in progress. Candidate 7-GiB image with a 3-GiB
  runtime-closure gate; actual size and boot not yet verified.
- [Thermal implementation/evidence](thermal.md): recovered fan and temperature
  routing from factory diagnostics; confirmed live in U-Boot through tmux.
  All six modules present, duty 15/15, twelve rotors running; four board
  temperatures about 24.5–27 C. Only temporary mux writes, restored afterward.
- Added hwmon fan driver, LM75 DT nodes, sensors and full-speed monitoring to
  the initial NixOS build. Linux-7.3.4 x86 API compile and control fixtures pass;
  PowerPC module build now passes too; runtime validation pending. No
  SD/NOR/firmware environment writes.

## 2026-11-14 — newer SDK / Linux 7 investigation

See [SDK research and compile evidence](sdk-research/README.md).

- Bundled SDK identified as **6.4.8**; OpenBCM **6.5.27** explicitly dispatches
  this BCM56930 device through the Tomahawk driver.
- Newer public GPL kernel modules found in SONiC **sdk-6.5.35-xgs**.
- With documented API fixes, kernel BDE, user BDE and KNET modules **compile,
  modpost, link and generate BTF against Linux 7.3.4 on x86_64**. Not loaded;
  not yet cross-built or tested on this PowerPC switch.
- Important mismatch: 6.5.27 KNET protocol 13 versus 6.5.35 protocol 24.
  Preserve matched userspace/module contracts; do not mix releases blindly.
- SDK core has Broadcom-specific licensing; kernel modules have a GPLv2 option.
  No complete, fully open Linux-7-ready package for this board verified.
- Hardware and acquired images untouched during this source/compile investigation.

## 2026-11-14 — initial boot and verified SD / NOR acquisition

- Chassis/EEPROM serial **776032X16521259**; EEPROM product **7760-32X-A-AC-F**, part `FP3AO7632004A`, revision `R01I`, device version 11. Manufacture date reported as 2018-11-19 16:34:47. TLV CRC valid. Base MAC `8C:EA:1B:89:C9:3C` (4 addresses).
- **Clean cold boot captured** in local `initial-boot-20261114.txt`. Outlet was initially off; logging connected before power-on. No autoboot interruption. Ctrl-C after ONIE entered installer discovery exposed the shell; `onie-discovery-stop` then stopped discovery. No installer success observed.
- CPU: Freescale T2080 rev 1.1, e6500 rev 2.0, four cores at 1.8 GHz. U-Boot detects **16 GiB DDR3 ECC**, two `U10U7211P8FU-BDE` DIMMs. ONIE kernel restricts RAM to 1920 MiB.
- U-Boot **2014.07-g8946503**, built 2018-02-06 17:37:35, vendor **1.7.0.10**. Autoboot interruption **confirmed** as `123` + Enter, yielding `LOADER=>`. ONIE **2015.02.00.30**, Linux **3.12.19-rt30-onie+**, ppc64. `nos_bootcmd=true` as read.
- Broadcom PCI functions `14e4:b930` at 01:00.0 and 01:00.1. ONIE initializes SDK and 128 KNET interfaces, each attempting DHCP; this delays startup by minutes.
- **No SATA SSD enumerated.** Whole disk exposed is an **SDHC SA08G** at `/dev/mmcblk0`, **7,948,206,080 bytes**, 15,523,840 sectors. CID `035344534130384780b12435c0012400`. One FAT32 partition. Disk and flash were unmounted during acquisition.
- **128 MiB parallel NOR**: eight contiguous MTD regions, map below. Kernel identifies manufacturer `0x89`, chip `0x227e`. U-Boot banner says “boot from NAND” but also reports NAND 0 MiB; Linux identifies these regions as NOR. Do not infer a NAND device from that banner.
- Declared EEPROM: 24c256 at I2C `0-0050`, 32768 bytes. **Read failed with Connection timed out**, leaving a zero-byte `.partial`; not a successful image and physical presence unconfirmed. Identity TLV begins at offset zero of the verified `mtd1` hw-info image. Other EEPROM nodes at buses 12–43 map to transceiver slots in the vendor DTB. SPI `spi0.0` is a GPIO-backed `spidev`, not an enumerated flash chip; its hardware role is unconfirmed.
- Acquisition helper: [acquire.py](acquire.py). Read-only device streams via `nc`, controlled entirely through tmux/tio. Exact byte counts, independent source SHA-256, transfer hash and saved-file reread hash must agree before a `.partial` becomes final. Local `dumps/manifest.jsonl` records verified files.
- Raw captures and images are git-ignored: U-Boot environment contains SSH host private keys. Device directory mode 0700. No firmware/environment writes requested. ONIE itself may perform automatic boot-time actions; this is a post-initial-boot snapshot, not a claim of forensic write blocking.


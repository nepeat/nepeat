# agent handoff — 2026-11-14

## start here

Device: **Edge-Core AS7760-32X**, serial **776032X16521259**. Freescale/NXP
T2080, eight e6500 hardware threads, 16 GiB RAM, Broadcom BCM56930 ASIC
(two PCI functions, `14e4:b930`). User wants a small modern probing OS, then
further switch bring-up. Latest direction is a stripped-down NixOS unstable
system, with diagnostic utilities, sensors and fan control. Alpine was an
earlier option; the working RAM root is musl/BusyBox plus Nix-built utilities,
not a stock Alpine installation.

**Current state: see the Status block in [PROGRESS.md](PROGRESS.md)** (upstream
U-Boot in NOR → extlinux → NixOS on SD; Nix code in `ai_re/nix`). The sections
below are recipes and background from the bring-up; where they mention the
factory `LOADER=>` prompt, `nos_bootcmd` chains or `path:…/nixos#name`, read:
prompt `=>`, `bootflow scan -lb`, `path:./nix#as7760-<name>`. RAM/TFTP boots
(`as7760-netboot`, `load_ram_probe.py`) still work for rescue.

Read [PROGRESS.md](PROGRESS.md), [Linux bring-up](linux-bringup.md),
[NixOS build notes](nixos.md), [image report](nixos-image-report.json),
[thermal evidence](thermal.md), then [SDK research](sdk-research/README.md).

## operating rules and live access

- All tools run inside the project devshell: `nix develop -c ...` from
  `/home/erin/nepeat/ai_re`. Follow the supplied/project AGENTS.md instructions.
- Keep user-watchable interaction in the **existing tmux/tio session**; do not
  open the UART independently. Confirmed framing: **115200, 8N1, no flow control**.
- Adapter at handoff:
  `/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_B0016Y5Q-if00-port0`.
- Record captures as `.txt`. Tio's session log is
  `serial-20261114-020529.txt`; an active tmux pipe also appends to
  `ram-probe-boot-20261114.txt`. Power/boot helpers depend on that capture.
- Preserve original storage/backups. Public TFTP staging must contain only
  public build artifacts. **U-Boot environment contains private SSH key data**:
  do not print the full environment or publish private inventories/dumps.
- The workspace has many pre-existing changes. No commit/staging was requested.
  This device directory is currently untracked; large binaries/builds are
  ignored. Do not alter/stage other devices or the user's personal Nix changes.
- Workstation devshell gained `poppler-utils` for reference PDF searches.

```sh
# watch / inspect
nix develop -c tmux attach -t serial-7760
nix develop -c tmux capture-pane -t serial-7760 -p -S -80

# send a command at an idle Linux root prompt, then inspect the pane
nix develop -c python3 devices/edge-core-7760-32x/serial_command.py \
  'ip addr show eth0; free -m; sensors'
```

`serial_command.py` sends paced hexadecimal tmux keystrokes. Use it for shell
metacharacters and U-Boot commands; unpaced bursts previously lost characters.
It returning does not mean a long device command has completed. Wait for the
prompt. The RAM probe has no SSH daemon; full NixOS enables key-only SSH.

## verified state and remaining boundaries

- Clean uninstrumented Linux 7.3.4 boots with eight threads and 15,976 MiB
  reported RAM. QEMU e6500 also ran target utilities as an ABI smoke check.
- Management BCM54616S PHY links at 1 Gbps/full. **`ethtool -K eth0 rxhash off`
  is required**. Final init applies it, raises eth0 and runs DHCP with a
  90-second retry allowance. Initial 18-second attempt timed out; final
  cold boot obtained its lease without intervention. DHCP script sets address,
  route and RAM DNS configuration. Full NixOS has a networkd pre-start hook
  for the same workaround, but that service has not run on hardware yet.
- Full-memory workstation/gateway pings passed without loss; a 4,922,999-byte
  TFTP download matched kernel SHA256. RX DMA/frame/driver error counters were
  zero, but stack RX-drop counters grew. This was not a throughput or stability
  qualification; the underlying FMan KeyGen/hash issue remains unresolved.
- Original SD enumerates as SA08G, **7,948,206,080 bytes (7.40 GiB)**.
  No separate SATA SSD detected; installed storage type was initially unknown.
- NOR enumerates all eight partitions. Read hashes of MTD0/1/5/7 and SD sector
  zero match acquisition. Other partitions were not re-hashed in Linux 7.
- Both EHCI hosts and hub `0424:2514` enumerate; no USB disk was attached.
  Four GPIO character devices enumerate. SPI controller is absent from the
  captured DT; factory GPIO-backed SPI wiring still needs recovery.
- Both ASIC PCI functions have BARs and IRQs (43, 17 in the checked boot).
  ASIC driver/DMA/SDK/packet forwarding have **not** been exercised.
- Fan module loads, twelve rotors read roughly 18–22k RPM, four LM75 sensors
  roughly 24.5–27 C. Full PWM 255 write/readback passed. No reduced-duty test.
  NixOS monitoring enforces full speed; that systemd service is untested on
  hardware. `sensors` misprints PWM 255 as 128%; raw sysfs and board control
  tool are authoritative. Keep full speed until cooling policy is validated.
- RTC oscillator is disabled (not set). Factory LM90@4c and mux@77 probes fail;
  physical population is unconfirmed. PAMU late-probe warning and PCI VPD/ROM
  quirks remain; do not infer working ASIC DMA from enumeration alone.

Evidence: [final unattended boot](linux-7.3.4-final-boot-20261114.txt),
[network transfer/PCI checks](linux-7.3.4-network-boot-20261114.txt).

## current artifacts and reproducible builds

Standalone source flake: `nixos/`, pinned nixos-unstable revision
`d4f25a3e562008648d45bcf2d70bcc7422cad84e` (2026-11-11).
Build host x86_64 Linux; target **big-endian e6500 ELFv2**
(`powerpc64-unknown-linux-gnuabielfv2`). Do not substitute ppc64le/POWER8 packages.
Linux 7.3.4, GCC 16.2, systemd 262. Kernel starts from `allnoconfig`, with
explicit peripheral/service requirements and no trace/debug patches.

| local link under `nixos-build/` | purpose / resolved store output |
| --- | --- |
| `result-ram-probe-final/` | **boot-tested** RAM bundle; `/nix/store/dkpli52angyg0y0a0yd3ivgfdy6i24nj-as7760-ram-probe` |
| `result-boot-final/` | full NixOS boot files; `/nix/store/789dl3p84gm23wqs9y09lpr6k561fiwi-as7760-legacy-boot-files` |
| `result-image-final/as7760.img` | full SD image; `/nix/store/cm3312cj0brwgghbycd0ych0mrrywgk2-as7760-nixos-sd-image/as7760.img` |

The **RAM and NixOS initrds are different**. RAM uInitrd is 42,906,561 bytes;
full NixOS uInitrd is 25,139,325 bytes and expects the prepared root filesystem.
Both bundles contain `uImage`, `uInitrd`, `as7760.dtb`, validation manifest and
boot commands. Build manifests conservatively say hardware unverified;
[nixos-image-report.json](nixos-image-report.json) records actual external tests.

SD image: 7,516,192,768 bytes (7 GiB), MBR, 128-MiB FAT boot at 1 MiB,
7,039-MiB ext4 root at 129 MiB. Runtime closure 950,710,272 bytes (~906.67 MiB),
3-GiB gate; root free 6,337,105,920 bytes, 6,263,300,096 excluding reserve.
SHA256: `2198b9765ef0befec4c740ee1627b41e8b88af4314ce9da4d6b049ada8632816`.
Kernel uImage: 4,922,999 bytes, SHA256
`257a0f6949e17f82332164b5d2b64aa837284f673325c82bb335aa20b9df9571`.
Other hashes and exact checks are in the report. Filesystem checks passed.
Detailed ELF/ownership audits were on the prior clean candidate with the same
toolchain/packages/image builder; the report explicitly records this scope.

```sh
nix develop -c nix build path:./nix#as7760-ram-probe \
  --out-link devices/edge-core-7760-32x/nixos-build/result-ram-probe-final \
  --max-jobs 2 --cores 8 --builders '' -L
nix develop -c nix build path:./nix#as7760-sd-image \
  --out-link devices/edge-core-7760-32x/nixos-build/result-image-final \
  --max-jobs 2 --cores 8 --builders '' -L
nix develop -c nix flake check path:./devices/edge-core-7760-32x/nixos --no-build
```

Keep logs/results outside `nixos/`: `path:` flakes can include ignored build
files and Python caches. Old clean/debug/DT/LIODN output links are experiments,
not the final candidate. Builds are complete at handoff; don't restart large
builds or cancel shared Nix processes without a reason.

## full NixOS netboot recipe (boot-tested 2026-11-14)

```sh
nix develop -c nix build path:./nix#as7760-netboot \
  --out-link devices/edge-core-7760-32x/nixos-build/result-netboot \
  --max-jobs 2 --cores 8 --builders '' -L
nix develop -c python3 devices/edge-core-7760-32x/powercycle_to_uboot.py
B=devices/edge-core-7760-32x/nixos-build/result-netboot
nix develop -c python3 devices/edge-core-7760-32x/load_ram_probe.py $B/uImage --bundle $B --boot
```

uInitrd ≈241 MiB (gzip stage-1 + 512-byte pad + raw cpio with the squashfs);
TFTP ≈30 s at ~9 MiB/s. U-Boot relocates it to 0x20f08000–0x2ffff999. The
loader reads `bootargs.txt` (has `init=<toplevel>/init`). Keep the bundle
under 448 MiB (asserted) so relocation stays clear of kernel staging.
Module: `ai_re/nix/modules/profiles/netboot-root.nix`; bundle: `ai_re/nix/packages/as7760-images/netboot-files.nix`.

## RAM reboot recipe

TFTP server at handoff: atftpd on UDP 69, `/srv/tftpboot`, workstation
`10.60.2.208`; staged public files are in `as7760-probe/`. Check those before
reusing the helper. Power entity is `switch.server_room_lab_plug` via `lab-power`.
The following power-cycles the switch and discards its RAM shell:

```sh
nix develop -c python3 devices/edge-core-7760-32x/powercycle_to_uboot.py
# wait for helper success / idle LOADER=> prompt
nix develop -c python3 devices/edge-core-7760-32x/load_ram_probe.py \
  devices/edge-core-7760-32x/nixos-build/result-ram-probe-final/uImage --boot
```

The loader defaults to the final RAM bundle, sets volatile DHCP/server/boot
variables, TFTP-transfers and verifies byte counts, then issues a single:

```text
setenv bootargs console=ttyS0,115200n8 rdinit=/init panic=0 pcie_aspm.policy=performance
bootm 0x10000000 0x20000000 0x08000000
```

Staging addresses: kernel `0x10000000`, initrd `0x20000000`, DT `0x08000000`.
Kernel legacy PowerPC uImage load/entry is zero. Do not use staged `bootm loados`:
loading at zero overwrote exception vectors while U-Boot was still running.
No helper saves the environment. DHCP in U-Boot may take repeated attempts;
loader allows about 120 seconds per command.

## fixes to preserve

1. **No earlycon or fixed-UART early-debug options.** Those performed early
   mappings before Book E memory setup and caused the original blank boot.
   Normal serial console works; no binary instrumentation ships.
2. DT DPAA reserved pools, modern QorIQ clock references, v3 port IDs,
   `fsl,fman-ports`, channel range and management SGMII PCS are necessary.
   Keep old RX compatible strings secondary so factory U-Boot injects LIODNs
   (live RX LIODN 0x425). See the DTS and bring-up notes before editing.
3. PCIe4 CPU aperture `0xc60000000`, 128 MiB, replaces `0xd8000000` which overlaps
   RAM; PCI bus address remains `0xd8000000`. Nested bridge range also corrected.
4. `pcie_aspm.policy=performance` keeps the FSL link check in L0, avoiding
   intermittent config read failures/missing IRQs. `pcie_aspm=off` is not a
   demonstrated equivalent; it may leave firmware-programmed link state.
5. NOR CFI command set is **0002** despite manufacturer ID 0x89. Keep
   `MTD_CFI_AMDSTD`; do not blindly change byte order based on that ID.
6. ~~Disable RX hashing~~ — **superseded 2026-11-14**: `ethtool -K rxhash`
   never bypasses KeyGen (`fmbm_rfpne` stays HWK); full NixOS cold-boots and
   passes bulk RX with hashing on. Workaround removed from NixOS; RAM probe
   init still has it. Stack rx_dropped = VLAN-tagged multicast from the trunk.
7. Fan module must use kbuild `modules_install` for nixpkgs module certificates.
   Keep autofs, loop and PSI support needed by NixOS/systemd. See build notes
   for minimal systemd unit packaging and loader checks.

## originals and recovery evidence

Protected/ignored originals under `dumps/`: `mmcblk0.img`, `mtd0.bin` through
`mtd7.bin`, concatenated `nor-full-128MiB.bin`, `SHA256SUMS`, `manifest.jsonl`,
captured `onie.dts`/DTB and extracted firmware. Preserve these in place.
Private U-Boot/inventory files are siblings; never paste full contents.

NOR order: RCW 128 KiB, hw-info 128 KiB, ONIE 32 MiB, diagnostics 32 MiB,
open JFFS2 0x3ec0000 bytes, FMan firmware 128 KiB, env 128 KiB, U-Boot 768 KiB.
Board EEPROM capture is named `.partial`; do not describe it as complete.
Initial boot capture is [initial-boot-20261114.txt](initial-boot-20261114.txt).
No new full acquisition is needed just to resume probing.

## recommended next work

1. ~~Boot full NixOS userspace non-destructively~~ **done 2026-11-14 via
   `#netboot`** (systemd, udev, networkd, sshd, thermal all validated; see
   PROGRESS). Still open: SSH key for workstation-side automation (user's
   call), rebuild + verify the SD image with the new kernel, and only then,
   if the user asks, decide about installing to SD. A request to resume probing
   is not by itself a request to overwrite the original SD or save new U-Boot
   defaults.
2. ~~FMan KeyGen / stack drops~~ **resolved 2026-11-14** (misdiagnosis + VLAN
   trunk drops; see PROGRESS). Open from it: unexplained PCIe AER UR from ASIC
   01:00.1. Management Ethernet working says nothing about front-panel ASIC
   support.
3. Continue the [SDK research](sdk-research/README.md) from its actual evidence.
   Factory SDK is 6.4.8; OpenBCM 6.5.27 identifies this ASIC, but previous Linux 7
   module/API compilation work does not prove PowerPC ABI, firmware integration
   or working hardware. Read its README before proposing an SDK path.
4. Recover the board's SPI/GPIO hookup, identify unconfirmed I2C nodes, and
   investigate RTC/PAMU/PCI quirks as needed. Keep full-speed cooling until
   temperatures and fan behavior support a deliberate reduced-duty policy.

Update PROGRESS.md with dated results and keep failures/uncertainty explicit.

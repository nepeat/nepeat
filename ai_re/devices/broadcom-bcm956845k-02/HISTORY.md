# BCM956845K-02 — earlier investigation journal

Archived from PROGRESS.md after native network integration.
For current device state, see [PROGRESS.md](PROGRESS.md).

## 2026-11-09 — peer review (Claude); no device interaction

- Reviewed the whole subdir, `tools/` and the flake/`.envrc` changes while the
  TAP bridge-domain kernel patch was in progress. Nothing was sent to the
  serial console or SSH session.
- Verified against the factory binary: `bcm_l2_addr_t` is 64 bytes, so the
  adapter's L2 buffer is exact; KNET stub bytes match `patch_sdk.py`.
  `tools/tests` 8/8 pass. The stageable set is 102 files / 1.5 MiB with no
  secrets or binaries.
- Changed: Home Assistant token moved from tracked `.envrc` to untracked
  `.envrc.local` (`direnv allow` needed); `bridge.c` status-report buffer
  clamp (compiles under `-Werror`, not yet rebuilt into an image); both
  hardware test scripts now always write their JSON record even if cleanup fails.
- Open items for whoever is driving: record the CINT offset measurements as
  evidence, notes on the 950 patch, apply-path port bounce, evidence-file
  overwrite. Details: [review-20261109.md](review-20261109.md).

## 2026-11-09 — hardware IPv4 routing proven; Linux synchronization is feasible

- Corrected the earlier scope: CPU routing describes the adapter's current
  integration, not a BCM56845 limitation. Live SDK reports 4096 RIFs,
  8192 next hops/hosts and 16384 route entries (reported capacities, not scale tests).
- Created isolated VLANs/RIFs 4001/4002, a next hop and an IPv4 /24 LPM route.
  **20/20 exact frames routed through the ASIC with Linux ip_forward=0**:
  next-hop/source MAC rewrites, TTL 64→63, valid IPv4 checksum and LPM HIT.
- Deleting the hardware route stopped egress reception (negative control).
  Egress MAC-loopback capture used a scoped field CopyToCpu+Drop rule;
  the first capture attempt failed and is retained separately in the evidence.
- Test RIF/route/next-hop/field/MAC/VLAN objects removed, original forwarding
  setting restored, and all ports restored to 40G. No flash/image changes.
- Automatic Linux bridge/VLAN/FDB/neighbor/route synchronization remains to
  implement. SDK orchestration can provide hardware forwarding; TAP alone
  does not provide native switchdev reporting or duplicate-flood protection.
- [Findings, mappings, sources and hardware proof](openwrt/offload/README.md).

## 2026-11-09 — real Linux port interfaces and native LuCI integration

- Current RAM image boots ASIC + **16 real Linux `swp1`–`swp16` devices**
  automatically. Normal netifd IP configuration and native LuCI network pages
  can use them. Physical jack numbering is still unverified.
- Factory SDK KNET APIs are compiled-out stubs returning BCM_E_UNAVAIL.
  Implemented an experimental **SDK packet API/TAP adapter**, using a
  hash-checked binary extension. This is explicitly **not Broadcom KNET**.
- Fresh-boot tests passed: Linux admin up/down changes ASIC enable state;
  20/20 exact frame round trips on each of swp1 and swp16; native static IP
  and Linux ARP response; authenticated native LuCI view without JS errors.
- Temporary test address and MAC loopbacks removed; all 16 ports restored
  to 40G. CFE, saved boot settings and factory flash remain untouched.
- Hardware VLANs remain on the Broadcom ASIC page/UCI. Linux bridge/VLAN
  changes are not synchronized/offloaded; Linux IP routing is CPU-based.
  External 40G link/forwarding and throughput remain untested.
- [Design, usage, limitations and boot/test evidence](openwrt/sdk-netdev/README.md).

## 2026-11-08 — ASIC working under OpenWrt; native 40G and LuCI

- BCM56845_A3 initialized under OpenWrt 25.12.1 / Linux 6.12.74 using a
  new bounded BDE compatibility module and the recovered SDK 5.9.1.
  CPU DMA loopback passed 73,440 TX/RX packets (Run 1, Pass 1, Fail 0).
- This is a **40G-capable switch**. Native **16×40G** configuration tested:
  SDK readback reports 40GFD / XLAUI. Default changed from 64×10G breakout
  to 16×40G; both lane profiles are available. Physical jack mapping and
  external 40G link/forwarding remain untested.
- OpenWrt UCI port enable/speed/PVID and tagged/untagged VLAN application
  verified against actual ASIC readback. Invalid speed rejected without
  changing hardware; test VLAN removed and defaults restored.
- LuCI HTTPS at `https://10.60.2.203/`, Network → Broadcom ASIC. Real browser
  login and custom port/VLAN view passed without JS exceptions. Generated
  root password is in ignored `openwrt/work/luci-root-password.txt`.
- RAM-only: CFE, factory firmware and saved boot configuration preserved.
  SDK polling mode used; no claim of production stability or routed offload.
- Complete image cold-netboot passed: automatic driver/ASIC initialization,
  default 16×40G apply, LuCI Save & Apply and repeat 73,440-packet CPU DMA test.
  [Hash-linked result](openwrt/asic-boot-result.json).
- [Architecture, commands, evidence and limitations](openwrt/asic-integration.md).

## 2026-11-08 — OpenWrt RAM netboot passed

- **Current device state: OpenWrt 25.12.1 / Linux 6.12.74 root shell**, in
  `serial-bcm956845` at 9600 8N1. Runtime identity includes `(emma’s version)`.
- Management eth0: 1 Gb full duplex, DHCP `10.60.2.203/24`, ping to host
  5/5 replies with zero loss. UART and Ethernet interrupts confirmed active.
- Fixed two porting mistakes exposed by boot tests: DTB must be in the
  initial 64 MiB mapping (now 48 MiB); modern MPIC IRQs differ from CFE's
  old internal numbering (UART 42; Ethernet 29/30/34). Early UART output
  now preserves CFE's divisor.
- Nothing flashed; RAM-backed rootfs, no MTD/PCI interfaces in prototype.
  Stock CFE/VxWorks boot settings preserved. ASIC support is proven
  separately under factory Linux, not integrated into OpenWrt yet.
- [Boot attempts and successful verification](openwrt/boot-testing.md),
  [hash-linked hardware result](openwrt/boot-result.json),
  [successful boot transcript](openwrt/boot-test-4-20261108.txt).

## 2026-11-08 — Linux ASIC initialization and DMA loopback confirmed

- Factory Linux BDE modules and SDK 5.9.1 successfully attached BCM56845_A3,
  initialized MMU/BCM/ports, and read BCM84740 PHY SPI-ROM firmware versions.
- CPU Loopback (`tr 17 c=5`) transmitted and received 73,440 packets;
  `tl 17` reports Run 1, Pass 1, Fail 0. External-port forwarding and modern
  OpenWrt driver compatibility remain untested.
- Flash was mounted read-only for configuration copies; SDK ran with its
  working configuration in RAM. [Procedure and results](asic-bringup.md),
  [transcript](asic-linux-test-20261108.txt).
- During initial netboot setup, CFE networking failed after both 3-second
  and 15-second outlet power cycles despite a 1 Gb link. Factory Linux can
  reach the build host on the same management interface/VLAN. Investigating
  bootloader network initialization before the TFTP boot. Subsequently,
  static-address TFTP succeeded even after a cold cycle; see the newer entry.

## 2026-11-08 — first OpenWrt netboot build completed

- User requested an OpenWrt build, explicitly **netboot for testing**, with
  build label **`(emma’s version)`**. Board port/build scripts are under
  [openwrt/](openwrt/README.md).
- Pinned OpenWrt 25.12.1 and checksum-verified published PowerPC 8548/musl
  toolchain. Added an FHS OpenWrt build shell to the Nix devshell; prerequisite
  checks pass. The complete kernel/rootfs/image build and artifact validation
  finished with exit status 0.
- Static helper compiled by that toolchain ran on the switch and recovered
  CFE's actual 8546-byte DTB through a read-only RAM read. A fresh test tree
  uses the observed clocks/UART/MPIC/Ethernet layout; old broken reservation
  and initrd fields were not copied into it.
- Target is a gzip legacy uImage with embedded RAM rootfs plus external DTB;
  flash, PCI/ASIC and `/dev/mem` interfaces disabled in the prototype. No
  persistent install image or flash-writing boot command is produced.
- Artifacts: `openwrt/artifacts/`: 7,288,607-byte `emma-openwrt-netboot.uImage`,
  external DTB, hash manifest, configs and temporary CFE command file. uImage
  CRCs, gzip, image architecture and disabled MTD/PCI/DEVMEM checks passed.
  Requested UTF-8 branding verified in generated identity/banner/JSON and
  the actual image payload. Hardware boot was subsequently tested above.
- Build tmux session: `openwrt-bcm98548`; log:
  `openwrt/work/build-compile.txt`. At this checkpoint the Linux factory
  root shell remained running; the build script itself does not reboot.
- Factory Linux management eth0 was brought up at its lab address
  `10.60.2.203/24` for the probe download. Temporary HTTP server was stopped.

## 2026-11-08 — storage interface probe

- Boot/firmware storage confirmed as **memory-mapped parallel CFI NOR flash**,
  x16 / 16-bit bank, exposed via MTD. Two 128 MiB mappings, each split into
  1 MiB CFE and 127 MiB OS; erase size 128 KiB.
- No USB/SATA/MMC storage enumerates in the running kernel; PCI shows only
  Freescale bridges and the switch ASIC. Missing drivers could conceal unused
  hardware, so this is not proof of physical connector absence.
- Soldered versus socketed remains unknown. "PLCC flash" is a BSP label,
  not sufficient evidence of actual chip packaging.
- Read-only results and remaining physical-inspection questions:
  [storage.md](storage.md). Linux root shell stays running; flash unmounted.

## 2026-11-08 — NOS research

- Exact-chip Linux precedents found: historical ONL lists BCM56845 systems
  (Dell S4810-ON and Agema AG-7448CU); IP Infusion documents BCM56845 support
  in ZebOS-XP. Neither establishes a ready image for this board.
- Offline inspection confirms our Linux `bcm.user` contains SDK 5.9.1 and
  `BCM56845_A3` strings. Live Linux ASIC/SDK operation is still untested.
- A small SDK-based NOS is a credible target; modern standard SONiC remains
  a poor fit for 512 MiB / 32-bit PPC without substantial platform work.
- Sources, scope and proposed milestone: [nos-options.md](nos-options.md).
  Current device state is unchanged: Linux root shell, flash unmounted.

## 2026-11-08 — Linux root shell obtained

- **Current state: Linux BusyBox `#` root shell**, PID 1, in tmux session
  `serial-bcm956845`. `id` confirms `uid=0(root) gid=0(root)`.
- Booted the existing `uImage.gto-2_6` from flash into RAM, with temporary
  `rdinit=/bin/sh` to skip Broadcom's automatic application startup. No saved
  boot settings changed; normal power-on still follows the VxWorks startup.
- Linux confirms **e500v2 rev 2.2, 997.5 MHz, PVR `0x80210022`,
  SVR `0x80310021`, 512 MiB RAM**, platform BCM98548 XMC.
- Mounted only `/proc` and `/sys` in RAM. Flash filesystems remain unmounted;
  no SDK kernel modules loaded and `/broadcom/bcm.user` not launched.
- Runtime image includes `/broadcom/bcm.user` (18,743,092 bytes),
  `linux-kernel-bde.ko` and `linux-user-bde.ko`; ASIC operation under Linux
  remains untested.
- Exact boot procedure and findings: [linux-shell.md](linux-shell.md).
  [Linux boot capture](linux-boot-20261108-190650.txt),
  [shell verification transcript](linux-session-20261108.txt).

## 2026-11-08 — full boot capture and firmware acquisition

- **Complete cold-boot log:** [boot-20261108-184531.txt](boot-20261108-184531.txt),
  through `BCM.0>`. User-authorized outlet cycle, then a second cycle to interrupt
  startup with Ctrl-C. Device was left powered on at **`CFE>`** in
  `serial-bcm956845` at this stage; see latest entry for current state.
- **Confirmed:** CFE-NTSW 5.1.2, BCM98548XMC board ID `0x51`, CPU core
  ~997–998 MHz, CCB ~399 MHz, RAM 512 MiB. Switch ASIC **BCM56845_A3**, PCI
  ID `14e4:b845` / revision `0x04` (vendor inferred from Broadcom PCI identity).
- **Acquired both 128 MiB mapped flash regions**, including boot regions.
  Local raw images: `dumps/flash2-full.bin` and `dumps/flash3-full.bin`.
  Hashes: [dumps/SHA256SUMS](dumps/SHA256SUMS); chunk inventory:
  [dumps/acquisition.json](dumps/acquisition.json).
- Extracted all six active filesystem files into `firmware/`; checked VxWorks
  gzip CRC and Linux uImage header/data CRCs. Linux bundle contains kernel
  **2.6.21.7-hrt1-WR2.0bl_standard**, plus a second compressed component.
  No alternate image boot attempted.
- CFE's TFTP upload begins at **DATA block 0** after block-size negotiation;
  normal atftpd silently lost the first 1428 bytes. A scoped receiver fixed
  acquisition; exact lengths, image CRCs and five independent sample re-reads
  now pass. Temporary receiver/routing rules and host staging were cleaned up.
- Network originally DHCP `10.60.20.203` on camera VLAN; user moved the port,
  then DHCP supplied `10.60.2.203` on lab VLAN. CFE network setup was temporary;
  no persistent environment or flash writes performed.
- Procedure, failure record and bank details: [firmware-dump.md](firmware-dump.md).
  CFE transcript: [cfe-session-20261108.txt](cfe-session-20261108.txt).

## 2026-11-08 — control-plane reconnaissance

- **Control board:** `Broadcom SBC8548`; SDK platform `GTO_MPC8548`.
  Strong evidence for an MPC8548-family PowerPC/e500v2 control plane.
- **Memory:** BSP reports physical and VxWorks memory top `0x20000000`
  (consistent with 512 MiB at base zero). Heap snapshot: 373,070,352 bytes
  free, 109,524,208 allocated. No destructive memory test performed.
- **Software:** VxWorks 6.5, WIND kernel 2.10, BSP `2.0/2`;
  Broadcom `sdk-xgs-robo-5.9.1`, built 2010-12-16.
- **Storage:** mounted `flash:` FAT16/VFAT filesystem;
  `flashFsSize = 0x07f00000` (127 MiB), 115,415,040 bytes free.
  Physical flash chip identity and total capacity remain unknown.
- **Potential alternate OS:** `flash:/uImage.gto-2_6` exists (11,401,844 bytes).
  Filename suggests Linux, but its header/content and bootability are unverified.
- Native VxWorks shell is accessible using SDK `shell`, without a login.
  At this stage the session was in native `->` shell; see latest entry above
  for the current prompt.
- Details, exact queries, uncertainties and next steps: [control-plane.md](control-plane.md).
- Raw evidence continues in [serial-20261108-183606.txt](serial-20261108-183606.txt).
  Chassis photo: [chassis-label.png](chassis-label.png).
- Actions so far: help, version, symbol lookup, memory/volume/task summaries,
  directory listing and read-only BSP getters. No reboot, flash changes,
  filesystem initialization, clock changes, or switch configuration writes.

## 2026-11-08 — initial console connection

- User identifies this as a Broadcom reference-design switch; chassis label
  in supplied photo: **BCM956845K-02**, **S/N 1376316**.
- Already powered on and booted before connection. Do not power-cycle to
  obtain an initial console prompt.
- Visible interfaces: RJ45 marked **RS-232**, Ethernet positions marked 1
  and 2 (position 2 appears unpopulated), PWR indicator and RST control.
- Electrical assumption: the labeled external RS-232 console uses RS-232
  signaling, not bare 3.3V/5V TTL. Voltage and RJ45 pinout have not been
  measured/confirmed; do not connect a bare TTL UART to this port.
- Host has one enumerated serial adapter:
  `/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_B0016Y5Q-if00-port0`
  (`/dev/ttyUSB0`). Adapter enumeration alone does not establish its electrical
  signaling or cable pinout. Using the user's existing console connection.
- Confirmed working settings: **9600 baud, 8 data bits, no parity, 1 stop bit,
  no flow control**. A single carriage return produced a clean `BCM.0>` prompt,
  consistent with a Broadcom diagnostic shell. No device commands sent yet.
- Persistent tmux session: `serial-bcm956845`. Human attach:
  `tmux attach -t serial-bcm956845`; detach with Ctrl-b then d.
- Capture: [serial-20261108-183606.txt](serial-20261108-183606.txt).
  Logging enabled via tio with append; this is a live session capture, not a
  boot log (device was already booted).
- Initial unprivileged open failed with `Permission denied`: current user
  does not have serial-device read/write access. Existing passwordless sudo
  was available; restarted only the waiting tio process using `sudo -n`
  inside the user's tmux session. Connected successfully. No device permission
  changes, power cycling, break signals, or configuration writes performed.

## Identification leads

The chassis number suggests BCM56845 silicon; this remains an inference
until console or board inspection confirms it. Broadcom's BCM56840 family
[product brief](https://docs.broadcom.com/doc/12358267) describes 10/40GbE
switching, up to 64x10GbE or 16x40GbE at the family level. This does not
establish this board's fitted ports or active configuration.

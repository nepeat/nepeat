# OpenWrt netboot prototype (emma’s version)

This is a board port for the **BCM98548XMC control plane** in BCM956845K-02,
using pinned OpenWrt **25.12.1** source commit
`d6115e42127481645ae7210c9d8ca1bd14313473` and its published MPC85xx cross-toolchain.

This is **TFTP → RAM initramfs only**, with no flash driver or persistent
installation image. It now includes working experimental **BCM56845_A3
ASIC initialization**, native Linux/netifd port/VLAN/IPv4 route synchronization and an authenticated
**LuCI web interface**. The default lane profile is **16×40G**, with
**64×10G** breakout available. Internal CPU DMA loopback has passed;
external 40G link/forwarding has not been tested.

See [ASIC integration, usage and limitations](asic-integration.md) and
[real Linux port devices through the SDK/TAP adapter](sdk-netdev/README.md).
The factory SDK has KNET disabled; this adapter uses its working packet APIs.
[Native configuration and hardware forwarding](native-network/README.md) now
cover bridges, VLANs, FDB/neighbor resolution and supported IPv4 routes.
The hardware page retains lane profiles, PHY settings and diagnostics.
Build manifests identify compiled artifacts; hardware test records separately
identify which exact image was booted.

## Build

From the project root, enter the nix devshell:

```sh
nix develop
bash devices/broadcom-bcm956845k-02/openwrt/build.sh
```

The build script enters `openwrt-build-shell`, the FHS build environment
defined in the project flake. The FHS compiler wrappers apply `/usr` header
and library paths only to native host compilers, since OpenWrt deliberately
clears `C_INCLUDE_PATH`. Do not apply those host paths to the PowerPC compiler.

`prepare.py` checks the source revision and cross-toolchain SHA-256, installs
the board overlay and writes a seed configuration. Feed revisions come from
the pinned OpenWrt release's `feeds.conf.default`. Source/downloads/build
outputs live under ignored `work/`; final netboot files go in ignored
`artifacts/`.

The SDK integration needs the ignored device input `firmware/linux-rootfs.bin`.
Recover it from the owned factory dump using [firmware-dump.md](../firmware-dump.md)
and `extract_fat16.py`; proprietary binaries are not distributed in git.

Default build parallelism is 16 jobs; override with `OPENWRT_JOBS`.
The initial build completed successfully on 2026-11-08 in named tmux session
`openwrt-bcm98548`. The session and log remain available:

```sh
tmux attach -t openwrt-bcm98548
tail -F devices/broadcom-bcm956845k-02/openwrt/work/build-compile.txt
```

`package_netboot.py` validates uImage header/data CRCs, gzip integrity,
PowerPC/Linux image type, zero load/entry address, requested branding, and
disabled flash/general physical-memory interfaces and presence of the
restricted ASIC compatibility module/SDK provenance before collecting artifacts.
It emits `manifest.json` with hashes and a **compiled, not implicitly booted**
status. No upload or reboot occurs in `build.sh`.

The original control-plane-only image and fixes are documented in
[boot testing](boot-testing.md). Current ASIC integration evidence is in
[ASIC integration](asic-integration.md); generated artifacts are under
`artifacts/`.

## Branding

The exact UTF-8 string **`(emma’s version)`** is in `CONFIG_VERSION_CODE`,
the banner/release identification, OS `PRETTY_NAME`, and `/etc/emma-build.json`.
Distribution/version numbering and filenames retain the upstream format so
package repository URLs and CFE commands do not acquire spaces/parentheses.
The artifact manifest carries the same label.

## Temporary CFE netboot

After artifacts have passed packaging checks, publish these read-only files
under the existing host TFTP root in a new `emma-openwrt/` directory:

- `emma-openwrt-netboot.uImage`: gzip kernel with embedded initramfs.
- `bcm98548xmc-netboot.dtb`: external board device tree.
- `manifest.json`: build identity and SHA-256 hashes.

`artifacts/cfe-netboot.txt` contains commands for the selected TFTP server
(default `10.60.2.208`). At a fresh CFE prompt:

```text
ifconfig eth0 -addr=10.60.2.203 -mask=255.255.255.0 -gw=10.60.2.1
load -raw -addr=0x03000000 -max=0x10000 -tftp 10.60.2.208:emma-openwrt/bcm98548xmc-netboot.dtb
setenv LINUX_CMDLINE "console=ttyS0,9600 earlycon=uart8250,mmio,0xe0004500"
boot -uboot -extdtb -dtbaddr=0x03000000 -tftp 10.60.2.208:emma-openwrt/emma-openwrt-netboot.uImage
```

The DTB is placed at **48 MiB**, above the kernel/initramfs decompression
area and within the kernel's initial 64 MiB mapping. A test at 128 MiB
faulted in `fdt_check_header`; see `boot-test-2-20261108.txt`.
Do not use CFE `flash`, `ptable save`, `setenv -p`, or change `STARTUP` for
these tests. Power cycling preserves the original VxWorks boot path.

The management interface requests DHCP; no DHCP server or forwarding through
the switch ASIC is configured. Serial remains **9600 8N1**.

## Hardware evidence and assumptions

`read-cfe-dtb.c` was cross-compiled statically with the published OpenWrt
PowerPC/musl toolchain and executed successfully on the factory Linux image.
It opened `/dev/mem` **read-only** and recovered the 8546-byte CFE-generated
device tree at physical `0x00ffc000`, with framed hex over serial:

- `cfe-generated.dtb`: recovered binary.
- `cfe-generated.dts`: decoded evidence; retain unchanged.
- `overlay/.../bcm98548xmc-netboot.dts`: explicit modern test tree, based on
  recovered memory/clock/UART/MPIC/MDIO/Ethernet values.

The recovered DTB uses old bindings and contains a malformed reservation
entry. The prototype uses a fresh external tree, omitting the old initrd
addresses, broken reservation, flash, second UART and optional buses. PCIe
and a reserved DMA pool were added for the ASIC compatibility module.
Management PHY address 1 and MAC are from this exact board. The RGMII delay
mode has passed management-network traffic tests.

For the read-only DTB probe, factory Linux eth0 was temporarily configured
with its existing lab address `10.60.2.203/24`. A temporary HTTP server on
host `10.60.2.208:8080` served the executable and was stopped after capture.
Only `/tmp` in RAM was written on the switch. BusyBox's old `wget -O` refuses
existing files; the helper was downloaded to a new RAM file then copied into
an existing executable inode. A PHY link message disrupted one serial prompt
match; the command completed and the query helper was adjusted accordingly.

## Initial build issues resolved

- Nix host filesystem lacks the normal Linux binary-loader/header layout:
  added a declarative FHS build environment, not ad-hoc system symlinks.
- Required ncurses library output as well as headers.
- OpenWrt prefers native `cc`/`c++` and clears include-path environment
  variables: wrappers cover all four native compiler names.
- `CONFIG_IMAGEOPT=y` is needed for custom version options to survive defconfig.
- Parentheses in the distribution name can leak into artifact paths:
  use the release code/metadata for branding and disable code-based filenames.
- Host static archives require the native GCC LTO plugin; wrappers for
  `ar`, `nm` and `ranlib` load the matching plugin from the Nix environment.
- The FHS user namespace cannot chown to arbitrary package IDs. The build
  sets `FAKEROOTDONTTRYCHOWN=1` so fakeroot records the intended ownership
  without issuing real chown calls to unmapped IDs.

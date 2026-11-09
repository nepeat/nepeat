# NOS feasibility and exact-chip precedents — 2026-11-08

Assessment: a compact Linux + ASIC SDK control plane is credible on this
board; a current standard SONiC image is not an established path.
Follow-up hardware tests now confirm OpenWrt RAM netboot with a root shell
and management DHCP/ping, and separately factory Linux SDK initialization
plus CPU DMA loopback. See [boot testing](openwrt/boot-testing.md) and
[ASIC bring-up](asic-bringup.md). No persistent NOS was installed.

## Preferred small-distro candidate: OpenWrt

Follow-up research found a concrete packaging path: OpenWrt's
[mpc85xx target](https://github.com/openwrt/openwrt/blob/main/target/linux/mpc85xx/Makefile)
uses `ARCH=powerpc`, `CPU_TYPE=8548`, with P1010/P1020/P2020 board families.
Its [powerpc_8548 package repository](https://downloads.openwrt.org/snapshots/packages/powerpc_8548/packages/)
contains actual FRR binaries, including zebra, bgpd and ospfd. This makes it
a promising base; it does not establish a bootable image for BCM98548XMC
or working Broadcom ASIC integration.

Board kernel/device-tree bring-up and CFE RAM image packaging now work.
Remaining work: SDK/BDE compatibility with a newer kernel and the forwarding agent.
Current packages cannot be assumed to run on factory Linux 2.6.21.
512 MiB RAM and a 127 MiB OS region are plausible for a selectively packaged
OpenWrt image; final size and runtime footprint still need measurement.

## Evidence on our device

- Actual ASIC: BCM56845_A3, original Trident (not Trident II/III).
- Actual CPU: single e500v2 rev 2.2, 997.5 MHz, big-endian 32-bit PowerPC/SPE.
- RAM: 512 MiB; early Linux shell snapshot had ~469 MiB free.
- Two 128 MiB flash mappings; active OS filesystem is 127 MiB. These are not
  interchangeable with a large persistent disk for container images.
- The original Linux 2.6.21.7 initramfs boots to UID 0 and contains its BSP,
  `linux-kernel-bde.ko`, `linux-user-bde.ko` and `/broadcom/bcm.user`.
- Offline inspection of that 18,743,092-byte executable found the exact
  strings `sdk-xgs-robo-5.9.1`, `BCM56840_A0`, and `BCM56845_A3`, plus other
  BCM56845 revisions. Subsequent live factory Linux tests confirmed chip
  attachment, MMU/BCM/PHY initialization and CPU loopback: 73,440 TX/RX
  packets, 1 pass and 0 failures. External-port forwarding remains untested.

## External precedents

| Stack | Evidence | Limit |
| --- | --- | --- |
| Open Network Linux (ONL) | Historical HCL lists Dell S4810-ON and Agema AG-7448CU with BCM56845 and P2020 CPU as supported/tested | ONL is a platform OS foundation with example forwarding code, not a complete routing/switching control plane. Those board ports are references, not images for this reference design. |
| IP Infusion ZebOS-XP | Vendor datasheet explicitly lists BCM56845 (Trident), SDK 6.3.7 and Wind River Linux 3.0 / Linux 2.6.27 | Historical commercial SDK/NOS integration; no ready image, license entitlement or current support for our board established. |
| SONiC | Current platform list includes later Trident generations; build machinery has AMD64/ARM paths | No original-Trident/PPC platform image or validated BCM56845 SAI implementation found in this research. |

Sources:

- [Historical ONL supported hardware](https://github.com/opennetworklinux/ONL/blob/master/docs/SupportedHardware.md).
- [ONL FAQ: platform foundation and forwarding responsibilities](https://opennetlinux.org/faq.html).
- [IP Infusion ZebIC / ZebOS-XP datasheet](https://builders.intel.com/docs/ipi_zebic_data_sheet.pdf).
- [SONiC supported platforms](https://sonic-net.github.io/SONiC/Supported-Devices-and-Platforms.html).
- [SONiC architecture: containers, Redis and ASIC integration](https://github.com/sonic-net/SONiC/wiki/Architecture).
- [Current SONiC build architecture handling](https://github.com/sonic-net/sonic-buildimage/blob/master/slave.mk).

## Practical interpretation

512 MiB RAM and ~1 GHz are plausible for modest VLAN/L2/L3 control and a
small routing daemon, because packet forwarding would be handled by the ASIC.
CPU speed does not itself cap the hardware forwarding rate. Actual route
scale, convergence, SDK memory footprint, packet-punt load and PHY/port
initialization still need measurement.

A full current SONiC port would require a PowerPC/SPE userspace/toolchain path,
a much newer kernel than this Linux 2.6 image, storage for its normal Debian/
container layout, and SAI/platform integration. Its container/Redis design
also makes 512 MiB a poor engineering target. This is an assessment, not an
assertion of a formally measured minimum SONiC RAM requirement or proof that
a heavily reduced derivative cannot be made to run.

A smaller stack could retain/rebuild the board Linux BSP and use the SDK for
ports, VLANs, L2 and L3 programming, with a purpose-built management agent and
optional routing daemon. FRR/Quagga alone does not supply ASIC integration:
routes, interfaces, traps and CPU packet I/O need an adapter to the SDK or an
appropriate kernel interface. Modern userspace versions may also require a
kernel/toolchain update rather than running unchanged on this old image.

Next decisive milestone: prove the included Linux BDE modules and SDK can
enumerate the actual BCM56845_A3, then demonstrate controlled forwarding
between two ports. Linux shell access alone does not establish that milestone.

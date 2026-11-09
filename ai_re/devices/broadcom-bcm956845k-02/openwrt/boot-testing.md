# RAM netboot testing — 2026-11-08

No flash writes or saved boot-environment changes were used. CFE's stock
VxWorks startup remains the power-on default. Serial: 9600 8N1 in
`serial-bcm956845`, with the existing continuous `.txt` capture retained.

## Boot attempts

1. Initial image (`3676c1cb…`), external DTB at 128 MiB: CFE successfully
   downloaded 7,288,607 bytes, decompressed 28,162,364 bytes and entered
   address zero. No kernel output appeared. [Capture](boot-test-1-20261108.txt).
2. Added PowerPC early 16550 debugging. Removed the explicit baud argument
   from `earlycon=uart8250,mmio,0xe0004500`: preserving CFE's UART divisor
   avoids reprogramming it with the generic default UART clock instead of
   this board's 399 MHz input. The debug kernel exposed a data-access fault
   at virtual `0xc8000000` in `fdt_check_header`, reading the DTB at physical
   `0x08000000`. [Capture](boot-test-2-20261108.txt).
3. Moved DTB to `0x03000000` (48 MiB), inside the kernel's initial 64 MiB
   mapping and above the roughly 27 MiB unpacked image. Kernel, `/init`,
   procd and ubus started. Management Ethernet negotiated 1 Gb, but TX
   watchdogs and MPIC mask timeouts occurred; pressing Enter produced no
   interactive console. [Capture](boot-test-3-20261108.txt).
4. **Passed:** corrected the interrupt binding. CFE's old kernel
   remapped internal MPIC interrupts ahead of external interrupts. Modern
   bindings use hardware numbering, adding 16 to the CFE UART/Ethernet IRQs.
   UART becomes 42; Ethernet TX/RX/error become 29/30/34. The updated DTB
   uses four interrupt cells and single-CPU affinity. Rebuilt the image,
   TFTP-booted it, pressed Enter and reached `root@OpenWrt:~#`.

## Successful boot verification

- OpenWrt 25.12.1, Linux 6.12.74, BusyBox 1.37.0, system `e500v2`.
- Runtime banner, `/etc/emma-build.json` and ubus board identity contain
  `(emma’s version)`; ubus reports rootfs `initramfs`.
- Management eth0 negotiated 1 Gb full duplex, obtained `10.60.2.203/24`
  through DHCP, and reports interface `up: true` via ubus.
- Device to build host ping: 5 sent, 5 received, 0% loss; RTT
  min/avg/max 0.276/0.337/0.438 ms. Host to device also received all replies.
- `/proc/interrupts` shows active Ethernet TX/RX on hardware IRQs 29/30 and
  UART RX on IRQ 42. No spurious or machine-check interrupts at verification.
  No MPIC mask timeouts or TX watchdogs appeared in this successful boot.
- Root and working filesystems are RAM-backed. No `/dev/mtd*` or
  `/sys/bus/pci` exists in this prototype.
- Successful boot image: 7,289,369 bytes, SHA-256
  `0acff120fd29ba29496fcd70667a203e360241e7f013bb1c6eb0d1d98a949ae2`.
- DTB: 1,993 bytes, SHA-256
  `bf32af23c4a3c9e38f4df34fdfd73945022c2561b9b6b9aec0a884c833d37622`.
- [Full successful boot and verification](boot-test-4-20261108.txt).

Device left running this OpenWrt image in RAM. Nothing was flashed.

The old remapping is visible in Linux 2.6.21's
[MPC85xx platform initialization](https://github.com/torvalds/linux/blob/v2.6.21/arch/powerpc/platforms/85xx/mpc85xx_cds.c).
Modern values are in the kernel's `pq3-duart-0.dtsi`, `pq3-etsec1-0.dtsi`
and `pq3-mpic.dtsi`, included by
[mpc8548si-post.dtsi](https://github.com/torvalds/linux/blob/v6.12/arch/powerpc/boot/dts/fsl/mpc8548si-post.dtsi).

## CFE networking

DHCP repeatedly failed despite a 1 Gb link; a first static-address ping also
failed. Factory Linux could reach the host on the same interface. Tried
3-second and 15-second outlet power cycles, Linux warm reboot, interface
deactivation, a speed request (CFE still reported 1 Gb), and stock VxWorks
startup followed by its `reboot` command.

Successful TFTP was then established using a static address. Crucially,
**a later 15-second cold cycle also succeeded with static configuration and
direct TFTP**, without booting an OS first. Stock VxWorks initialization is
therefore not established as a requirement. DHCP/ping failure alone did not
predict the subsequent TFTP outcome.

```text
ifconfig eth0 -addr=10.60.2.203 -mask=255.255.255.0 -gw=10.60.2.1
load -raw -addr=0x03000000 -max=0x10000 -tftp 10.60.2.208:emma-openwrt/bcm98548xmc-netboot.dtb
setenv LINUX_CMDLINE "console=ttyS0,9600 earlycon=uart8250,mmio,0xe0004500"
boot -uboot -extdtb -dtbaddr=0x03000000 -tftp 10.60.2.208:emma-openwrt/emma-openwrt-netboot.uImage
```

The address above is this device's existing lab address; check availability
before reusing it in another environment. Files are served read-only from
`/srv/tftpboot/emma-openwrt/` by the host's existing TFTP server.

## ASIC test is separate

The factory Linux SDK initialized BCM56845_A3 and passed CPU DMA loopback
(73,440 TX/RX packets; 1 pass, 0 failures). See
[ASIC bring-up](../asic-bringup.md). The OpenWrt prototype still disables
PCI and does not contain a working modern-kernel BDE integration.

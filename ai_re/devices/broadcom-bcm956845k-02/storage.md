# Storage identification — 2026-11-08

**Boot/firmware storage is memory-mapped parallel CFI NOR flash.** The mapped
firmware storage is not a USB/SATA disk or eMMC device. Its physical packaging
(soldered TSOP/BGA versus socketed device) remains unconfirmed.

## Positive evidence

The Linux boot log identifies both flash mappings through the CFI
AMD/Fujitsu command-set driver, with one x16 device in a 16-bit bank and
128 MiB mapped size. This is parallel NOR, not a block disk interface.
CFI command-set identity alone does not establish an exact manufacturer or
part number. See [Linux's command-set driver](https://github.com/torvalds/linux/blob/master/drivers/mtd/chips/cfi_cmdset_0002.c).

| Mapping | Base | Linux partitions | Capacity |
| --- | --- | --- | --- |
| Boot flash | `0xf8000000` | `mtd0`: CFE, `mtd1`: OS | 1 MiB + 127 MiB |
| PLCC boot flash | `0xf0000000` | `mtd2`: CFE, `mtd3`: OS | 1 MiB + 127 MiB |

All four partitions report erase size `0x00020000` (128 KiB). The primary OS
region holds the VxWorks FAT16 volume. Current Linux `/` is an initramfs in
RAM; flash remains unmounted. Full raw mapping backups already exist under
`dumps/`; see [firmware-dump.md](firmware-dump.md).

The second OS region reads erased while both boot regions are identical.
Software mappings do not by themselves prove physical chip count or whether
some addresses are aliased by board logic.

## Additional read-only enumeration

Queries in the live Linux root shell:

```sh
cat /proc/partitions
cat /proc/mtd
ls /sys/block
ls /sys/class/mtd
ls /sys/bus
cat /proc/iomem
for d in /sys/bus/pci/devices/*; do echo "$d"; cat "$d/class" "$d/vendor" "$d/device"; done
ls /sys/bus/platform/devices /sys/bus/of_platform/devices
```

- `/proc/partitions`: only `mtdblock0` through `mtdblock3`.
- `/sys/block`: MTD blocks plus RAM disks; no disk or MMC block devices.
- Registered buses: i2c, mdio_bus, of_platform, pci, pci_express, platform.
  No USB or MMC bus registered in this kernel.
- PCI devices: Freescale host bridges `1057:0013`, `1957:0013`, and Broadcom
  Ethernet-class device `14e4:b845` (the switch ASIC). No enumerated SATA or
  USB controller.
- Platform devices: four Gianfar Ethernet devices, MDIO, two I2C controllers,
  serial devices and RTC `m41t00`.
- MTD sysfs class entries in this old kernel expose only `dev`, `subsystem`
  and `uevent`; no chip-ID attribute was available there.

Absence from this old kernel's enumeration does not prove the board lacks an
unused connector/controller or a device whose driver is not compiled in.

## Physical inspection still needed

The BSP label **"PLCC boot flash"** suggests historical PLCC/socket support,
but is not proof that the fitted 128 MiB mapping corresponds to a socketed
PLCC package. Do not infer replaceability from the label alone.
Board photos/markings are needed to identify the flash parts, sockets, and
any optional USB/SATA/expansion connectors. No electrical probing or memory
command writes were performed during this software inspection.

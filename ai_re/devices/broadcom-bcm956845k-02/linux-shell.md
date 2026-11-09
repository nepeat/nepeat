# Linux root shell — 2026-11-08

This documents the factory Linux shell procedure. The device was later
successfully RAM-booted into OpenWrt; see [current status](PROGRESS.md) and
[boot testing](openwrt/boot-testing.md).

The stored Linux image boots successfully into a **root BusyBox shell**.
Current serial session: `serial-bcm956845`, 9600 8N1, no flow control.

```sh
tmux attach -t serial-bcm956845
```

Detach with Ctrl-b then d. Shell prompt is `#`; `id` returned
`uid=0(root) gid=0(root)`. `echo $$` returned `1`: this shell is PID 1.
Keep it running; exiting PID 1 would terminate the init process and may panic
the kernel. Power cycling returns to the saved VxWorks startup sequence.

## What was booted

Original, unmodified `flash0.os:uImage.gto-2_6`, previously backed up and
validated. CFE loads/decompresses the kernel at address zero, copies the
compressed initramfs to `0x01000000`, and generates a device tree at
`0x00ffc000`. No image was uploaded or written to flash for this boot.

CFE `printenv` already contained these vendor-provided shortcuts:

```text
cmdline26ram = setenv LINUX_CMDLINE "console=ttyS0,9600 root=/dev/ram ramdisk_size=65536 rd_start=0x1000000 rd_size=14495817"
bootlinux26ram = boot -uboot -fatfs flash0.os:uImage.gto-2_6
bootlinux = cmdline26ram&&bootlinux26ram
```

`help setenv` explicitly states the default is RAM-only; `-p`, `-P`, or `-ro`
would persist values. None of those switches was used.

At a fresh `CFE>` prompt, these are the exact commands used:

```text
setenv LINUX_CMDLINE "console=ttyS0,9600 root=/dev/ram ramdisk_size=65536 rd_start=0x1000000 rd_size=14495817 rdinit=/bin/sh"
boot -uboot -fatfs flash0.os:uImage.gto-2_6
```

CFE replaced the inherited, stale `rd_size=14495817` with the actual compressed
initramfs size `9720364`; Linux `/proc/cmdline` confirmed:

```text
console=ttyS0,9600 root=/dev/ram ramdisk_size=65536 rd_start=0x1000000 rd_size=9720364 rdinit=/bin/sh
```

## Why use an early shell

Offline inspection established the second multi-file payload is a gzip
compressed `newc` cpio initramfs containing 641 archive entries. `/init` links
to BusyBox, `/etc/inittab` invokes `/etc/init.d/rcS`, then offers an `askfirst`
console `/bin/sh`. No password is involved in that configured console shell.

However, `rcS` first calls `/broadcom/auto_launch_user.sh`, which mounts the
flash filesystem read/write by default and starts `./bcm.user`. We passed
`rdinit=/bin/sh` to obtain the shell before any of that startup sequence.
The original initramfs and its configuration were not modified.

## Verified runtime inventory

| Property | Observed |
| --- | --- |
| OS | Linux `2.6.21.7-hrt1-WR2.0bl_standard` |
| Kernel build | `#3 PREEMPT Wed Jun 17 09:08:06 PDT 2009` |
| Compiler | GCC 4.1.2, Wind River Linux Sourcery G++ 4.1-84 |
| Shell | BusyBox v1.4.1, built 2008-07-22 15:02:38 PDT, `msh` |
| Identity | UID 0, GID 0, PID 1 |
| CPU | e500v2 revision 2.2, 997.500000 MHz |
| PVR / SVR | `0x80210022` / `0x80310021` |
| Platform | BCM98548 XMC, Freescale Semiconductor |
| RAM | 512 MB in `/proc/cpuinfo`; `MemTotal: 516228 kB` |
| Free memory snapshot | `MemFree: 480132 kB` |
| Loaded modules | `/proc/modules` empty |
| Root | RAM initramfs (`rootfs / rootfs rw`) |

`uname` was not found in this minimal image; `/proc/version` supplied the
kernel identification. `/proc` and `/sys` were manually mounted for inspection:

```sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
id
cat /proc/version
cat /proc/cpuinfo
cat /proc/meminfo
cat /proc/mtd
cat /proc/mounts
cat /proc/cmdline
cat /proc/modules
```

`/proc/mounts` shows **only rootfs, proc, and sysfs**. Flash remains unmounted.
Management interfaces were detected by the Gianfar driver but Linux networking
has not yet been configured/tested. No daemon startup script was run.

## Linux flash device mapping

| Device | Size | Erase size | Name / CFE mapping |
| --- | --- | --- | --- |
| `mtd0` | `0x00100000` | `0x00020000` | Boot flash cfe / `flash0.boot` |
| `mtd1` | `0x07f00000` | `0x00020000` | Boot flash os / `flash0.os` |
| `mtd2` | `0x00100000` | `0x00020000` | PLCC flash cfe / `flash1.boot` |
| `mtd3` | `0x07f00000` | `0x00020000` | PLCC flash os / `flash1.os` |

Both CFI probes report one x16 device in a 16-bit bank, AMD/Fujitsu command
set, top boot, and 128 MiB mapped size. The kernel found no valid flash
partition table and applied its defaults, consistent with CFE observations.

The initramfs has `/broadcom/bcm.user` (18,743,092 bytes),
`linux-kernel-bde.ko` (40,712 bytes) and `linux-user-bde.ko` (24,104 bytes).
At the initial shell checkpoint these were untested. Subsequent
[factory Linux ASIC tests](asic-bringup.md) loaded both modules, initialized
BCM56845_A3 and passed CPU loopback (73,440 packets, 1 pass, 0 failures).

## Evidence and next steps

- [Linux boot capture](linux-boot-20261108-190650.txt).
- [Boot plus shell verification transcript](linux-session-20261108.txt).
- [Full firmware backups and CRC verification](firmware-dump.md).

Potential follow-up: configure management networking in RAM; inspect SDK build
and supported-chip strings offline; evaluate BDE module compatibility and
ASIC enumeration. Persistent NOS installation is a separate, untested step.

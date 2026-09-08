# NixOS on the Accton AS7772-32X

NixOS port for the **Accton AS7772-32X** 32-port QSFP28 network switch (Freescale T2080
PowerPC + Broadcom BCM56960 Tomahawk ASIC). Boots over the network; no permanent flash
modification required.

---

## Build

Requires Nix with flakes enabled on an x86_64-linux host.

```bash
nix build .#fitImage
# output: result/as7772-nixos-netboot.itb
```

Individual targets:

```bash
nix build .#kernel      # kernel + DTB only
nix build .#initramfs   # initramfs only
```

---

## Booting from TFTP

Copy the FIT image to your TFTP root, then at the U-Boot prompt:

```
dhcp
tftp 0x1000000 as7772-nixos-netboot.itb
bootm 0x1000000
```

To make it persist across reboots:

```
setenv nos_bootcmd 'dhcp; tftp $loadaddr as7772-nixos-netboot.itb; bootm $loadaddr'
saveenv
boot
```

---

## Booting from HTTP

U-Boot's `wget` command fetches over plain HTTP (no HTTPS). Any HTTP server on the
management network works — nginx, Apache, or a quick `python3 -m http.server 80`.

### Option A — FIT image (one download, recommended)

```
dhcp
wget 0x1000000 http://192.168.1.99/as7772-nixos-netboot.itb
bootm 0x1000000
```

### Option B — Separate kernel + DTB + initramfs

Load addresses are chosen to avoid overlap (kernel ≤ ~10 MB, DTB ~36 KB):

```
dhcp

# Kernel uImage
wget 0x1000000 http://192.168.1.99/as7772/uImage

# DTB
wget 0x3000000 http://192.168.1.99/as7772/accton_as7772_32x-r0.dtb

# Initramfs (raw cpio.gz — capture $filesize before the next download overwrites it)
wget 0x4000000 http://192.168.1.99/as7772/initramfs.cpio.gz
setenv initrdsize ${filesize}

# Boot: bootm <kernel> <initrd>:<size> <fdt>
bootm 0x1000000 0x4000000:${initrdsize} 0x3000000
```

For Option B, serve `result/boot/uImage`, `result/boot/accton_as7772_32x-r0.dtb`, and
`result/initramfs.cpio.gz` from your HTTP server.

---

## Lab Network

| Parameter | Value |
| --- | --- |
| Switch management IP | `192.168.1.10` |
| TFTP/HTTP/NFS server | `192.168.1.99` |
| Switch MAC | `1c:ea:0b:38:2b:d5` |
| Console | `ttyS0`, 115200 8N1 (front USB port) |

---

## Hardware

| Field | Value |
| --- | --- |
| CPU | Freescale T2080, 4× e6500 cores (8 HW threads), 64-bit big-endian |
| RAM | 1920 MB |
| Switch ASIC | Broadcom BCM56960 (Tomahawk), 32× QSFP28 100G |
| Management port | FMan DTSEC1, SGMII 1G, PHY BCM5400 |
| Flash | 128 MB NOR (ONIE / diag / open partitions) |

# Factory Linux ASIC bring-up — 2026-11-08

**Confirmed:** the saved factory Linux SDK initializes BCM56845_A3 and passes
its CPU DMA loopback test. This is not yet ASIC support in OpenWrt, nor an
external-port forwarding test.

Evidence: [serial transcript](asic-linux-test-20261108.txt).

## Procedure

Booted the original Linux image through CFE with temporary `rdinit=/bin/sh`,
as documented in [linux-shell.md](linux-shell.md). Mounted proc/sysfs and
brought management eth0 up at `10.60.2.203/24`; it reached `10.60.2.208`.

Copied the board's actual configuration and startup script from a read-only
flash mount into RAM. Did not execute `auto_launch_user.sh`, which would
mount flash read/write.

```sh
mount -t vfat -o ro /dev/mtdblock1 /mnt
mkdir /tmp/asic-test
cp /mnt/config.bcm /mnt/rc.soc /tmp/asic-test/
insmod /broadcom/linux-kernel-bde.ko
insmod /broadcom/linux-user-bde.ko
cd /tmp/asic-test
export BCM_CONFIG_FILE=/tmp/asic-test/config.bcm
/broadcom/bcm.user
```

Both BDE modules loaded; character devices major 127/126 were present.
The proprietary modules taint the factory kernel, as expected.
`/proc/mounts` confirmed the flash mount was read-only. SDK configuration
and working files were in RAM.

## Results

- SDK `sdk-xgs-robo-5.9.1`, built 2010-12-16; platform `GTO_MPC8548`.
- PCI unit 0: device `0xb845`, revision `0x04`, chip `BCM56845_A3`,
  driver `BCM56840_A0`.
- SOC attached, MMU initialized, BCM driver initialized, L2 shadowing and
  port modes initialized; interactive `BCM.0>` prompt reached.
- External BCM84740 PHY initialization reported SPI-ROM firmware `0xd00c`
  across ports 1–64. This identifies a local PHY firmware source; it does
  not prove that every physical connector is working.
- `show unit` independently identified `BCM56845_A3`.
- `ps` exposed xe0–xe63, all link-down during this test.
- `tr 17 c=5` transferred **73,440 packets**, with transmitted and received
  counts equal. SDK `tl 17` reported CPU Loopback: Run 1, Pass 1, Fail 0.

No external traffic generator or cable loop was used. No external-port
forwarding, 10/40 Gb link, routing offload or OpenWrt BDE compatibility is
claimed. The modern OpenWrt kernel still needs a compatible BDE interface.

## Network/reset observation

On the first 3-second outlet power cycle, CFE negotiated 1 Gb but received
no DHCP responses and could not ping the build host, even using the previous
static address. Factory Linux subsequently reached the same host on eth0.
The user confirmed the lab VLAN connection and suggested a fuller power
cycle. A subsequent test uses 15 seconds off before retrying CFE netboot.

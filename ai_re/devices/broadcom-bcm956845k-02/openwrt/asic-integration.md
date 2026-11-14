# BCM56845 under OpenWrt

## Hardware results, 2026-11-08

OpenWrt 25.12.1 / Linux 6.12.74 on the MPC8548 has initialized the
BCM56845_A3 and its BCM84740 PHYs using the recovered SDK 5.9.1 application.
This switch supports 40 GbE. Native **16×40G** is now the default; **64×10G**
is a separate lane/breakout profile, not the only capability of the ASIC.
SDK readback in native mode reports `Forced(40GFD,pause)` and `IF(XLAUI)`.
These are logical SDK port counts; front-panel jack numbering is not mapped.

- [40G initialization/readback](asic-40g-test-20261108.txt).
- [OpenWrt SDK initialization and CPU DMA loopback](asic-openwrt-success-20261108.txt):
  test 17 transmitted and received 73,440 packets; Run 1, Pass 1, Fail 0.
- [Configuration test](configuration-test-20261108.json): disable xe0, enable
  at 40G, program VLAN 4079 (xe0 untagged, xe1 tagged), change xe0 PVID,
  inspect hardware readback, reject an unsupported speed, restore defaults.

External 40G link, optical/copper compatibility, external forwarding,
throughput and long-term stability are not validated. Hardware routing
was validated subsequently; see [native integration](native-network/README.md).
The CPU loopback test is internal DMA evidence, not external wire-rate proof.

The complete image was then cold-netbooted and passed automatic ASIC startup,
configuration and LuCI tests: [hash-linked result](asic-boot-result.json),
[boot capture](boot-asic-20261108.txt). CPU loopback also passed again in
native 40G mode: [result](asic-40g-loopback-20261108.json).

## Architecture

The original proprietary, static PowerPC `bcm.user` is extracted from this
owned device's Linux firmware during preparation. Its original SHA-256 is
`ea4fdc1f4e172cb641e64abd6cd239baaaaa762f2eab1a67ce34992de75d96a8`.
The ASIC-only image replaced NUL-terminated `/dev/mem` paths, at equal length,
by `/dev/bcm`. The current image also appends our own experimental SDK/TAP
adapter; [Linux interface integration](sdk-netdev/README.md) documents its
exact-binary bootstrap and limitations. Generated provenance records hashes. The SDK binary is not
committed, and no right to redistribute it is implied.

The handwritten GPL `bcm-bde-compat` Linux module implements the legacy
92-byte ioctl ABI, PCI configuration access, SDK semaphores and bounded
BAR/DMA mapping. It recognizes only PCI `14e4:b845`, maps its 256 KiB BAR
at `0x60000000` and uses 16 MiB of a reserved, uncached DMA region starting
at `0x1c000000`. Root-only device nodes also require CAP_SYS_RAWIO.
General `/dev/mem` and flash drivers remain disabled. SDK interrupt polling
is enabled; hardware interrupt operation is not part of the validated path.

`bcmd` owns the single SDK process through a PTY and serializes commands
through a root-only UNIX socket. `bcmctl` exposes this socket, including JSON
output and SDK error codes. Do not start a second SDK process alongside it.
The procd `bcm-switch` service initializes the selected lane profile with
ports disabled and creates SDK/TAP Linux ports. `bcm-network` then programs
the ASIC from authoritative native Linux/netifd state. Inspect
`/tmp/bcm-sdk/native-status.json`, `native-plan.json` and `console.txt`.
The native controller validates supported configurations and leaves front
ports disabled on failure; management Ethernet is independent.

## Configure

Use standard LuCI **Network → Interfaces** at `https://10.60.2.203/` for
interfaces, bridges, VLANs, IPv4 addresses and routes. **Network → Switch
hardware** retains lane profiles, PHY settings and hardware status.
[Native integration](native-network/README.md) describes supported settings,
reconciliation, hardware routing, tests and limits. The older port/VLAN editor
and its separate `bcm_switch` forwarding configuration have been removed.

The certificate is self-signed. Login is `root`; the generated password is
in ignored host file `work/luci-root-password.txt` (mode 0600). Build setup
stages the same password hash, authorized task key and stable SSH host key
into later RAM images. Never commit these secrets or publish the rootfs.

Read-only CLI examples:

```sh
bcmctl --json 'ps'
bcmctl --json 'port xe0'
ubus call bcm_switch status '{}'
ip -d link show swp1
bridge vlan show
ip -4 route show
```

Hardware settings remain in `/etc/config/bcm_switch`: `core.profile` is
`40g` (16 logical ports) or `10g` (64 logical ports); each `xeN` has speed and
autonegotiation. Speed `0` means profile maximum; explicit settings are
1000/10000 and, in 40G mode, 40000. Changing profile restarts the ASIC and
changes logical mapping; adapt native network membership to active ports.
Linux controls admin state. Physical front-panel mapping is unverified.

The authenticated rpcd plugin exposes hardware status/reconcile under its
own ACL. Native network configuration uses the standard LuCI network ACLs
and reload triggers. Earlier editor validation is historical evidence in
[luci-test-20261108.json](luci-test-20261108.json); current validation is
recorded with the [native controller](native-network/README.md).

All configuration remains in RAM and is lost on power cycle. CFE and stock
flash images are untouched. The image is an experimental hardware bring-up,
not a production NOS. Power cycling normally returns to stock VxWorks unless
CFE is interrupted and the RAM image is loaded again.

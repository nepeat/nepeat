# ai_re devices

Index only: each device's `PROGRESS.md` is the source of truth. When a
device's Status changes, update its row and column here in the same turn.

## Devices

| dir | what it is | state (2026-11-15) |
| --- | --- | --- |
| [amzn-pinnacles](devices/amzn-pinnacles/PROGRESS.md) | Amazon-internal Fire HD 10 (2021) work tablet, MT8183 | rooted per boot (CVE-2022-38181); unlock scheme mapped, no bootloader unlock |
| [broadcom-bcm956845k-02](devices/broadcom-bcm956845k-02/PROGRESS.md) | Broadcom reference switch, BCM56845 Trident, e500v2 CPU card | OpenWrt RAM-netboot; native Linux config programs L2/VLAN/IPv4 in the ASIC; flash untouched |
| [cs8210-32x](devices/cs8210-32x/PROGRESS.md) | Celestica CS8210-32X, 32x400G, Teralynx 7 | NixOS on SSD (fand, node-exporter); vendor SDK by hand; 400G + static MACsec on a DAC loop (2026-11-15); boot ROM UART CLI reachable without writes (Route R); off the house network since the 2026-11-11 incident |
| [dmc-12-48r-as0](devices/dmc-12-48r-as0/PROGRESS.md) | DMC-12-48R-AS0 48-port console server, Annapurna Alpine V2 | lab console server: NixOS (mainline 7.3, minimal kernel) on SSD, `consoled` serves 48 ports over ssh (P1 cs8210, P48 as7760), fand, TPM 2.0, metrics; our U-Boot 2026.10 from SPI (IPXE slot, factory U-Boot kept as the `u` escape); new units: `provision.py` (20 coming) |
| [delta-tn48m](devices/delta-tn48m/PROGRESS.md) | Delta TN48M, 48x1G + 4x10G, Prestera AC3X | NixOS boots unattended (chainloaded mainline U-Boot); front ports admin-down; SoC secure-boot fused |
| [edge-core-7760-32x](devices/edge-core-7760-32x/PROGRESS.md) | Edge-Core AS7760-32X, 32x100G, Tomahawk, T2080 | powered off (plug 1) since 2026-11-15; NixOS on SD, upstream U-Boot in NOR; SONiC (NixOS services) on sai-openbcm at boot: L2/LAG/L3/BGP/BFD/ACL on the chip over a lab loop; console not live (DMC-12 port 48) |
| [eqam-400b](devices/eqam-400b/) | Blonder Tongue EQAM-400B edge QAM | no PROGRESS; newer-firmware search incomplete (`search-state.json`) |
| [hikvision-DS-2CD2T22WD-I5](devices/hikvision-DS-2CD2T22WD-I5/PROGRESS.md) | Hikvision 2 MP bullet camera | recovered, on V5.5.82 with root shell ([WRITEUP](devices/hikvision-DS-2CD2T22WD-I5/WRITEUP.md)) |
| [lenovo-flex5-14alc05](devices/lenovo-flex5-14alc05/PROGRESS.md) | Lenovo Flex 5 laptop, bricked BIOS | recovered 2026-07-20, closed |
| [meraki-ms320](devices/meraki-ms320/PROGRESS.md) | Meraki MS320-48 switch, VCore-III MIPS | stock locked firmware; in-circuit SPI read failed, needs U10 desolder |
| [meraki-mx250](devices/meraki-mx250/PROGRESS.md) | Meraki MX250 router, Xeon-D | signed boot + TAm; two SPI chips dumped, U97 + USB storage left |
| [netvue-ni-3421](devices/netvue-ni-3421/PROGRESS.md) | Netvue pan/tilt Wi-Fi camera | full flash dump verified and analysed |
| [perle-scr1618](devices/perle-scr1618/PROGRESS.md) | Perle IOLAN SCR1618 console server | firmware question answered (4.5.G4); rootfs analysed, offline |
| [raritan-px3-5475v](devices/raritan-px3-5475v/PROGRESS.md) | Raritan PX3 rack PDU | firmware analysed offline; no CLI escape found |
| [sonic-nixos](devices/sonic-nixos/PROGRESS.md) | subproject: SONiC switch stack as NixOS services | full control plane + FRR on the AS7760 at boot (sai-openbcm); QoS, warm reboot, line-rate tests open |
| [steam-deck-lcd](devices/steam-deck-lcd/PROGRESS.md) | Steam Deck LCD, bricked BIOS | recovery planned, not attempted |
| [za-816s-4-w](devices/za-816s-4-w/PROGRESS.md) | dual Ingenic T31 PTZ Wi-Fi camera | both flashes dumped; console access TODO |

## Switch features

Cells: **yes** = verified on hardware (date or note); **partial** = some of it
works (say what); **chip-capable** = the silicon does it, our stack doesn't
yet; **no**; **n/a**; **unknown** = the device's notes don't say. Notes below
link the evidence.

| feature | [AS7760-32X](devices/edge-core-7760-32x/PROGRESS.md) | [CS8210-32X](devices/cs8210-32x/PROGRESS.md) | [TN48M](devices/delta-tn48m/PROGRESS.md) | [BCM956845K-02](devices/broadcom-bcm956845k-02/PROGRESS.md) | [MS320-48](devices/meraki-ms320/PROGRESS.md) |
| --- | --- | --- | --- | --- | --- |
| CPU/SoC | NXP T2080 (e6500, ppc64 BE) | Annapurna Alpine v3, 16x A72 | Marvell Armada 7040, 4x A72 | PowerPC e500v2 (BCM98548XMC card) | VCore-III, MIPS 24KEc |
| switch ASIC | BCM56930_B1 Tomahawk (SDK BCM56960_A0) | Teralynx 7 (IVM77700) | Prestera 98DX3265 AC3X | BCM56845_A3 Trident | in the SoC (VCore-III family) |
| OS now | NixOS 26.11 on SD | NixOS 26.11 on SSD (Linux 7.3.4) | NixOS unstable on SSD | OpenWrt 25.12.1, RAM only | stock Meraki |
| boot path | upstream U-Boot 2026.10 in NOR | vendor U-Boot 2018.05 | vendor U-Boot (signed, fused) → mainline U-Boot 2026.10 chainload | vendor CFE netboot | vendor LinuxLoader |
| NOS / dataplane driver | OpenBCM 6.5.27 + sai-openbcm | vendor SDK (chroot) + open ipd | mainline `prestera` + fw 4.1 | Broadcom SDK + native-network controller | stock |
| ports up (real links) | yes: 10G + 25G (2026-11-14), 25G under SONiC (2026-11-15); 100G untested [^a1] | yes: 10G breakouts (2026-11-10/11); 400G CR8 DAC loop 1<->16 at ~386 Gb/s per direction, also with static MACsec on the Credo PHYs (2026-11-15) [^c1] | no: 52 netdevs, admin-down | no: MAC loopback only [^b1] | unknown |
| L2 VLAN | yes: SDK + SAI (2026-11-15) [^a2] | chip-capable [^c2] | unknown [^t1] | yes (2026-11-09) [^b1] | unknown |
| LAG (hardware) | yes: LACP PortChannel over the lab loop, LAG members in the ASIC (2026-11-15) [^a2] | chip-capable [^c2] | unknown [^t1] | unknown | unknown |
| LACP (control plane) | yes: teamd/teammgrd PortChannels over the lab loop, teamd respawn (2026-11-15) [^a2] | partial: Linux bond over CPU netdevs, failover verified [^c3] | unknown [^t1] | unknown | unknown |
| L3 routing | yes: VRFs, routed in the ASIC, ECMP, BGP/static via FRR over the lab loop (2026-11-15) [^a2] | chip-capable: Linux routes on CPU only [^c2] | unknown [^t1] | partial: IPv4 hardware routing in MAC loopback [^b1] | unknown |
| ACL | yes: ingress ACL drop + counters, CoPP policers on the field processor (2026-11-15) [^a2] | unknown | unknown [^t1] | chip-capable: not offloaded [^b1] | unknown |
| QoS | chip-capable | unknown | unknown [^t1] | unknown | unknown |
| BFD | yes: software (FRR bfdd), < 1 s BGP failover over the lab loop (2026-11-15) [^a2] | unknown | unknown | unknown | unknown |
| KNET / CPU netdevs | yes: KNET TX/RX (2026-11-14) | yes: `fpp*` via ipd, ~1.6 Gb/s CPU path [^c3] | partial: netdevs exist, no traffic | partial: SDK/TAP, KNET compiled out [^b1] | unknown |
| SAI | partial: sai-openbcm: ports, L2/VLAN/FDB, LAG, L3/VRF/ECMP, ingress ACL, policers, temperatures; no QoS, buffers, mirror, tunnels [^a2] | partial: our sai-teralynx (shared frontend + IFCS backend on the vendor libifcs.so): init, 32 ports, 400G loop up (2026-11-16); no LAG/ACL/traps yet [^c6] | no | no | no |
| SONiC stack | partial: at boot as NixOS services: swss, FRR (BGP, static, BFD), pmon, LLDP, crash recovery on the chip (2026-11-15); no QoS, warm reboot [^a2] | built, not run: `hardware.cs8210.sonic` (off) [^c6] | no | no | no |
| fan control | yes: fand, board + ASIC zones [^a3] | yes: fand, board + ASIC die zones (2026-11-15) [^c4] | yes: fancontrol, PWM floor 96 | unknown | unknown |
| ASIC temperature | yes: 8 PVTMON, also over SAI under syncd (2026-11-15) [^a3] | yes: MCP3425 diode (2026-11-10) [^c4] | unknown | unknown | unknown |
| board sensors | yes: 4x LM75 [^a3] | yes: LM75s + I2C hwmon [^c4] | yes: tmp1075/ADT7473, 35-40 °C | unknown | unknown |
| optics EEPROM | yes: `as7760-qsfp` (2026-11-14) [^a4] | yes: `cs8210-qsfp`, SFF-8636/8472 (2026-11-10) [^c5] | unknown | unknown | unknown |
| QSFP reset/presence | yes: CPLD#1 (2026-11-15) [^a4] | yes: I2C FPGA (2026-11-10) [^c5] | unknown (SFP+ cages) | unknown | unknown |
| LPMODE | no pin; SFF-8636 soft path untested [^a4] | partial: FPGA bit mapped, not exercised [^c5] | n/a (SFP+) | unknown | unknown |
| LEDs | partial: public port LED program loaded by syncd at SDK init, not checked by eye; CPLD LEDs no | unknown | unknown | unknown | unknown |
| metrics / exporter | yes: node-exporter + `switch_*`, `fand_*`, `switch_asic_*` [^a5] | yes: node-exporter + `switch_*`, `fand_*` (2026-11-15) | unknown | unknown | unknown |

[^a1]: Port 1 breakout to port 32 via QSA, set up with `bcm.user`: [sonic-nixos/PROGRESS.md](devices/sonic-nixos/PROGRESS.md).
[^a2]: Through SONiC on sai-openbcm over the lab loop (cage 1 breakout leg into cage 32, both ends on the switch), 2026-11-15: evidence files listed in the Status of [sonic-nixos/PROGRESS.md](devices/sonic-nixos/PROGRESS.md); SAI coverage in the [sai-openbcm README](nix/packages/sai-openbcm/README.md).
[^a3]: [edge-core-7760-32x/thermal.md](devices/edge-core-7760-32x/thermal.md). `openbcm-asic-temp` holds the SDK lock; stop it before `bcm.user`/syncd.
[^a4]: [edge-core-7760-32x/qsfp-control.md](devices/edge-core-7760-32x/qsfp-control.md).
[^a5]: Scraped by the lab VictoriaMetrics, see [tools/README.md](tools/README.md).
[^c1]: Entries (p), (w), (x) of [cs8210-32x/PROGRESS.md](devices/cs8210-32x/PROGRESS.md). The data plane is started by hand and not enabled at boot.
[^c2]: The SDK's knet setup puts each port in its own L2 domain with the CPU. Bridges, VLANs and LAG are designed offline only ([switchd/DESIGN.md](devices/cs8210-32x/switchd/DESIGN.md)); hardware FIB limits in [fib-capacity.md](devices/cs8210-32x/fib-capacity.md).
[^c3]: 802.3ad bond with the lab aggregation switch, entries (t)-(x). A hung ASIC once took down the house network (entry (z)); guard any test that can loop.
[^c4]: `cs8210-fancontrol`, entry 2026-11-10 (e).
[^c5]: Entries 2026-11-10 (f), (l), (o), (u).
[^c6]: [cs8210-32x/sonic.md](devices/cs8210-32x/sonic.md); no arm64 Teralynx SAI exists, so the SAI is ours.
[^t1]: Mainline `prestera` (switchdev) loads with firmware; nothing on the front ports has been exercised ([delta-tn48m/PROGRESS.md](devices/delta-tn48m/PROGRESS.md)).
[^b1]: Real ASIC packet tests in MAC loopback only; no external link or 40G throughput test. Scope and limits: [openwrt/native-network/README.md](devices/broadcom-bcm956845k-02/openwrt/native-network/README.md).

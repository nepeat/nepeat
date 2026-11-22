# AS7772-32X external PHYs (BCM82391 "Evora")

Hub for PHY matters. State 2026-11-22: all 32 PHYs answer and take the vendor init (100G MACsec bypass, firmware
D00B, factory TX taps); both DACs (cages 1-2, 31-32) pass frames at line rate; MACsec proven live.

| topic | where |
| --- | --- |
| PHY init at boot, SONiC on top, operating it | [sonic.md](sonic.md) |
| re-hosted vendor library (traces, boot script, tools) | [evora-rehost.md](evora-rehost.md), [evora-rehost/README.md](evora-rehost/README.md) |
| vendor code and firmware (static RE) | [evora-vendor-re.md](evora-vendor-re.md) |
| public sources (none for Evora), chip facts | [evora-public-sources.md](evora-public-sources.md) |
| MDIO timing, source-level | [mdio-review.md](mdio-review.md) |
| load test, PHY counters, TX taps | [load-test.md](load-test.md) |
| MACsec | [macsec.md](macsec.md) |
| link state that follows the cable (line-side link, line power) | [below](#line-side-link-and-power), [sonic.md](sonic.md) |
| factory boot chain, why the vendor diag fails here | [boot-chain-diff.md](boot-chain-diff.md) |

## The rule: `init misc` before any external MDIO access
- On Tomahawk the SDK programs the external MDIO rate and hold delay only in **misc init** (`init misc` /
  `init all`), from `rate_ext_mdio_divisor` and `mdio_output_delay` (default 10). After a bare **`init soc`** the
  chip keeps its reset values: divisor 24 (about 17.7 MHz MDC) and `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY` = **3**.
- Delay sweep ([evidence/mdio-delay-sweep-20261122.txt](evidence/mdio-delay-sweep-20261122.txt), sha256
  d083beb19a4b): 0-2 = every read bit-shifted (0x2391 reads as 0x4723), **3 = the edge (20 of 32 ids answer)**,
  4-15 = correct. The divisor made no difference. **Do not sweep delays 0-3 on PHYs that are wanted afterwards.**
- **A PHY that takes a corrupted frame hangs** (one constant on every register: 0xffff, 0x0000, 0x1900, 0xff9c)
  **until a cold power cycle**; the in-band reset does not help.
- Every "silent", "flaky" and "hung" PHY of 2026-11-21 was this: our probes and the first MDIO bridge ran
  `init soc` only. There are no dead PHYs, and the CPLDs are not involved.
- Proof: cold power cycle, first access at delay 8: **32 of 32** ids return 0x2391 / 0x80a0
  (`evidence/mdio-id-delay8-20261122-coldboot.txt`, sha256 9712d8d7f394); then `extphy init all` rc 0 on all
  eight groups.
- Fixed in: `nix/modules/boards/as7772/sdk-config.nix` (`rate_ext_mdio_divisor=53`, `mdio_output_delay=10` = the
  factory's values), `evora-rehost/mdio.py` (bridge runs `init soc`, `init misc`); `openbcm-mdio-script` reads
  both values back and refuses to run if they differ.
- **No clause 22** to these PHYs (never needed; suspected, not proven, to hang them).
- **Clause 45 device 1 only, and no register sweeps** (both found the hard way, 2026-11-22,
  [evidence/extlink-explore-20261122.txt](evidence/extlink-explore-20261122.txt)): reads of devices 2-5, 7, 29-31
  on a running PHY left it answering one constant (0x2391) on every register, the hung state above; a read sweep
  of 1.8000-1.90ff left the PHY passing traffic but its firmware message interface dead (1.8221 never
  acknowledges) until the next boot script run. Which register does that was not bisected.
- The first night's scan scripts are in [evidence/do-not-run/](evidence/do-not-run/README.md): they run at delay 3
  and one sends clause 22.
- Unverified: whether `init all`'s default external-PHY probe (bus 0-3, addresses 2-9) sends frames to the
  Evoras. syncd runs `init all` on every boot with the PHYs already up, with no harm seen.

## Addressing and port map
- Clause 45 through the switch CMIC's external MDIO. PHY id = SDK MDIO address (bit 8 and bits 6:5 = bus 0-7,
  bits 4:0 = address); bus *b* holds addresses 4*b* .. 4*b*+3. **Chip ID: dev 1 reg 0x8b00 = 0x2391,
  0x8b01 = 0x80a0** (0x82391). The IEEE ID registers 1.2/1.3 read 0x0000 on a live PHY.
- One id per cage, 4 lanes each. Diag name `ce<N-1>` = cage N (same cage order as the AS7760).

| cages | PHY ids | bus, addresses | diag ports | SDK logical ports | TSCF cores |
| --- | --- | --- | --- | --- | --- |
| 1-4 | 0x00-0x03 | 0, 0-3 | ce0-ce3 | 50, 54, 58, 62 | 12-15 |
| 5-8 | 0x24-0x27 | 1, 4-7 | ce4-ce7 | 68, 72, 76, 80 | 16-19 |
| 9-12 | 0x48-0x4b | 2, 8-11 | ce8-ce11 | 34, 38, 42, 46 | 8-11 |
| 13-16 | 0x6c-0x6f | 3, 12-15 | ce12-ce15 | 84, 88, 92, 96 | 20-23 |
| 17-20 | 0x110-0x113 | 4, 16-19 | ce16-ce19 | 102, 106, 110, 114 | 24-27 |
| 21-24 | 0x134-0x137 | 5, 20-23 | ce20-ce23 | 17, 21, 25, 29 | 4-7 |
| 25-28 | 0x158-0x15b | 6, 24-27 | ce24-ce27 | 118, 122, 126, 130 | 28-31 |
| 29-32 | 0x17c-0x17f | 7, 28-31 | ce28-ce31 | 1, 5, 9, 13 | 0-3 |

- Cage to PHY id is the factory `port_map-100G.csv`; cables have confirmed it for cages 1, 2, 31, 32 only.
- The PHYs are driven beside the SDK (no `port_phy_addr`). ASIC ports: `an=off if=CAUI`, no FEC, average IPG 16.
  The PHY does RS-FEC on the cage side (`if_line=CAUI4_C2C fec=1`), plain CAUI to the ASIC.
- Lane maps: [sdk-config.md](sdk-config.md).

## What has been shown on hardware
| what | result | detail |
| --- | --- | --- |
| vendor init, all 32 | `extphy init all` (331 s through the ssh bridge) + `cfg <first id of each bus> nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1` (6 s each) rc 0; `status`: FEC enabled, low-latency bypass egress + ingress | traces `dumps/x/evora-trace/all-init.*`, `all-cfg.*` |
| links, SDK shell ([asic-link-2dacs.soc](asic-link-2dacs.soc)) | ce0, ce1, ce30, ce31 up at 100G; CPU `tx` frames, counters equal on both ends: ce0 -> ce1 2000 / 508,000 B, ce1 -> ce0 4000 / 4,088,000 B, ce30 -> ce31 6000 / 3,060,000 B, ce31 -> ce30 8000 / 12,128,000 B; no RFCS / RERPKT | `evidence/asic-link-2dacs-20261122-run1.txt` (9cc557414b1d); first link 2026-11-21: `evidence/asic-link-ce0-ce1-20261121-run1.txt` |
| init replayed on the switch at boot, SONiC | 32/32 verified, 4 ports up, pings 0 lost after cold power cycles | [sonic.md](sonic.md) |
| load | 26 min at 100 % of 100G both ways on both DACs, 0 frames lost, RS-FEC corrected/uncorrected 0 (plain SDK rings, SONiC stopped) | [load-test.md](load-test.md) |
| factory TX taps | line-side per-lane taps (`preem-25G.soc`) in the boot script, all 32 PHYs | [load-test.md](load-test.md) |
| PHY counters with SONiC up | `evora-rehost/phystat.py` through syncd's diag socket | [load-test.md](load-test.md) |
| MACsec | vendor mode 2 and our own keys (GCM-AES-128, XPN-256) on cages 1-2; on from the booted bypass state without a firmware download on cages 31-32. No frame captured, not load-tested, SONiC stopped, `AVERAGE_IPG` 48 | [macsec.md](macsec.md) |

Not done: frames above 8996 B, pause through the PHY, 25G break-out, a cable on the other 28 cages, soak beyond
26 min, MACsec at boot or under SONiC.

## Line-side link and power
Measured 2026-11-22 ([evidence/extlink-explore-20261122.txt](evidence/extlink-explore-20261122.txt), sha256 1ecd6bbd3d3f); used by
`nix/modules/boards/as7772/ext-phy.nix` (how SONiC uses it: [sonic.md](sonic.md)).
- **The PHY does not pass link state through.** ASIC port disabled: only that PHY's system side drops; its line
  side keeps transmitting and the far end stays up. Cable side down: the system side, and so the ASIC link, stays up.
- **Line-side link = PCS live status 0xc161, bits 1:0 = 3** (0 without link; what the vendor `status` prints as
  link=UP/DOWN; which bit is which is not known). 0xc160 is the latched companion (clear on read). No plain
  device-1 register carries it (1.826c, 1.8a39, 1.8c01 looked like it between two PHYs and do not follow the line).
- **Firmware message** (how the vendor library reads and writes the PCS / SerDes registers; one message is 8-10
  MDIO operations): bytes to 1.a000.. as little-endian words, 1.8222 = 0x1111 (read) or 0x1101 (write) starts it,
  1.8221 reads 0x11e3 once when done (clear on read), read data at 1.a101 with 1.a100 = 2.
  Read: `06 00 | side 01 | reg lo hi | lane 00`; write: `09 00 | side 00 | 01 reg-lo | reg-hi lane | 00 data-lo | data-hi xx`;
  side 1 = line, 0 = system. The side byte alone selects the side (1.8b90 is only for the LMI path).
- **Line power** (vendor `extphy power <phy> 0|1`, absent from its usage text): per lane 0xd0e1 bit 7 (receiver
  off; 0xa008 on, 0xa088 off) and 0xd139 bit 0 (transmitter off; 0x0350 / 0x0351); power-on also writes 0xc010
  (index 6) back as read (0x0003). Same values on every PHY and lane looked at. Off takes the link down at both
  ends of the cable; on brings it back by itself (far end up 0.8 s later under SONiC). 1.8251 stays 0x000d.

## Modes (vendor `extphy init ... mode=`)
The Evora is a MAC-PHY inline on every port; the vendor init brings its MACsec engine up even for bypass.
| mode | meaning | switch MAC `AVERAGE_IPG` |
| --- | --- | --- |
| 0 | MACsec bypass (**what boots**) | 16 |
| 1 | bypass with break-out (4 x 25G) | 16 |
| 2 | "MACsec enabled for power consumption testing" (factory burn-in; fixed test key) | 48 (the 32 bytes MACsec adds) |

## What the vendor diag does (`dumps/x/diag/root/usr/local/`)
- `rc.soc-100G`: `init all`, `port ce an=off if=CAUI`, `extphy init all`, `extphy cfg ...` per bus,
  `preem-25G.soc` (`extphy pre` taps), `mac_ipg_16.soc`. Command reference: [evora-vendor-re.md](evora-vendor-re.md).
- `extphy` usage text as printed: `serial-20261121-2125-factory-diag.txt` (git-ignored).

## Dead end: running the vendor diag on this unit (2026-11-21)
Booted the NOR diag image from our U-Boot with `init=/bin/sh` (no NOR or SSD write) and started the vendor
`bcm.user` (SDK 6.5.9): it attaches, but every SCHAN operation times out, several CMIC registers read 0xdeadbeef
and MDIO reads return 0xbeef, warm and cold. Likely cause (**not tested**): the diag kernel expects PCIe4 memory
at CPU 0xd8000000, our U-Boot opens that window elsewhere, so the BAR lands on DDR. Analysis and a dtb-patch
recipe: [boot-chain-diff.md](boot-chain-diff.md).

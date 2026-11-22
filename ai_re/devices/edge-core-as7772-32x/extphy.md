# AS7772-32X external PHYs (BCM82391 "Evora")

State 2026-11-22: **all 32 PHYs answer, all 32 take the vendor init (100G MACsec bypass, firmware D00B), and both
DACs (cages 1-2, 31-32) pass frames.** The PHYs are brought up by the vendor's own library re-hosted in an
emulator on the workstation ([evora-rehost.md](evora-rehost.md)); **at boot the switch replays the recorded
init itself and runs SONiC on top: [sonic.md](sonic.md)** (a warm reboot clears the PHYs, an SDK restart does not). Vendor code and
firmware: [evora-vendor-re.md](evora-vendor-re.md). Public sources (none for Evora):
[evora-public-sources.md](evora-public-sources.md). MDIO review: [mdio-review.md](mdio-review.md).

## The rule: `init misc` before any external MDIO access
- On Tomahawk the SDK programs the external MDIO rate and hold delay only in **misc init** (`init misc` /
  `init all`), from `rate_ext_mdio_divisor` and `mdio_output_delay` (default 10). After a bare **`init soc`** the
  chip keeps its reset values: divisor 24 (about 17.7 MHz MDC) and `CMIC_MIIM_CONFIG.MDIO_OUT_DELAY` = **3**.
- Delay sweep ([mdio-delay-sweep-20261122.txt](mdio-delay-sweep-20261122.txt), sha256 d083beb19a4b): 0-2 = every read
  bit-shifted (0x2391 reads as 0x4723), **3 = the edge (20 of 32 ids answer)**, 4-15 = correct. The divisor made
  no difference in the sweep.
- Frames at the edge are mis-framed for some PHYs, and **a PHY that takes a corrupted frame hangs** (one constant
  on every register: 0xffff, 0x0000, 0x1900, 0xff9c) **until a cold power cycle**; the in-band reset does not help.
  Every "silent", "flaky" and "hung" PHY of 2026-11-21 was this: our probes and the first version of the MDIO
  bridge ran `init soc` only. There are no dead PHYs, and the CPLDs are not involved.
- Proof: cold power cycle, first access at delay 8: **32 of 32** ids return 0x2391 / 0x80a0
  (`mdio-id-delay8-20261122-coldboot.txt`, sha256 9712d8d7f394); then `extphy init all` rc 0 on all eight groups,
  id 0x02 included.
- Fixed in: `nix/modules/boards/as7772/sdk-config.nix` (`rate_ext_mdio_divisor=53`, `mdio_output_delay=10` = the
  factory's values, effective after `init misc`), `evora-rehost/mdio.py` (bridge runs `init soc`, `init misc`).
  The old `mdio-*.soc` scan scripts are kept as evidence and **must not be reused**: they run at delay 3, and
  `mdio-silent.soc` also sends clause 22. Unverified: whether `init all`'s default external-PHY probe
  (bus 0-3, addresses 2-9) sends frames to the Evoras; two runs after `init all` showed no harm.

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

  Cage to PHY id is the factory `port_map-100G.csv`; cables have confirmed it for cages 1, 2, 31, 32 only.
- The PHYs are driven beside the SDK (no `port_phy_addr`); the ASIC ports run `an=off if=CAUI`, no FEC on the
  ASIC side. The PHY does FEC on the cage side (`if_line=CAUI4_C2C fec=1`), plain CAUI to the ASIC.

## Bring-up and traffic (2026-11-22)
- PHYs: `evora-rehost/rehost.py --backend ssh ... 'init all'` (331 s for 32 PHYs through the ssh bridge), then
  `cfg <first id of each bus> nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1` (6 s each). Traces
  `dumps/x/evora-trace/all-init.*`, `all-cfg.*`. `status`: FEC enabled, low-latency bypass egress + ingress.
- ASIC: [asic-link-2dacs.soc](asic-link-2dacs.soc) with our `bcm.user` and the generated config: `init all`,
  other ports disabled, one VLAN per test port, `an=off if=CAUI sp=100000`, `CLMAC_TX_CTRL AVERAGE_IPG=16`.
  ce0, ce1, ce30, ce31 **up at 100G**; CPU `tx` frames, counters equal on both ends of each cable:
  ce0 -> ce1 2000 / 508,000 B, ce1 -> ce0 4000 / 4,088,000 B, ce30 -> ce31 6000 / 3,060,000 B,
  ce31 -> ce30 8000 / 12,128,000 B; no RFCS / RERPKT. `asic-link-2dacs-20261122-run1.txt` (sha256 9cc557414b1d).
  First link (cages 1-2 only, 2026-11-21): `asic-link-ce0-ce1-20261121-run1.txt`.
- **Load, 2026-11-22 ([load-test.md](load-test.md)):** both DACs 26 min at 100 % of 100G line rate in both directions
  (96.4 Gb/s of frames per port per direction at IPG 16, 385.7 Gb/s transmitted in total, 64-8996 B mix), **0
  frames lost, RS-FEC corrected/uncorrected 0**, ASIC 49 C max. Run with the plain SDK (SONiC stopped) as two
  counter-rotating L2 rings; SONiC's own forwarding path is ping-tested only.
- **TX taps:** the factory per-lane line-side taps (`preem-25G.soc`) are in the boot script for all 32 PHYs.
- PHY counters with SONiC running: `evora-rehost/phystat.py` (FEC, PCS, PHY MAC MIBs) through syncd's diag socket.
- Not done: frames above 8996 B, pause through the PHY, 25G break-out, a cable on the other 28 cages, soak
  beyond 26 min.

## MACsec
- The Evora is a MAC-PHY inline on every port; the vendor init brings its MACsec engine up
  (`cfye_device_init`, `secy_device_init`, `secy_config_set`) and then sets bypass. We run **bypass** (vendor
  `init` mode 0). Mode 1 = bypass with break-out, mode 2 = "MACsec enabled for power consumption testing"
  (factory burn-in, with `AVERAGE_IPG` 48 instead of 16 on the switch MAC for the 32 bytes MACsec adds).
- **Nothing encrypted has been tried.** The `extphy` shell has no key commands; the library has the
  `bcm_plp_secy_*` calls (public header `epdm_sec.h`, see evora-public-sources.md), reachable through the re-host.

## What the vendor diag does (`dumps/x/diag/root/usr/local/`)
- `rc.soc-100G`: `init all`, `port ce an=off if=CAUI`, `extphy init all`, `extphy cfg ...` per bus,
  `preem-25G.soc` (`extphy pre` taps, **not applied by us yet**), `mac_ipg_16.soc`.
- `extphy` usage text: `serial-20261121-2125-factory-diag.txt` (git-ignored, sha256 7a94bd73c27d).

## Dead end: running the vendor diag on this unit (2026-11-21)
Booted the NOR diag image from our U-Boot with `init=/bin/sh` (no NOR or SSD write) and started the vendor
`bcm.user` (SDK 6.5.9): it attaches, but every SCHAN operation times out, several CMIC registers read 0xdeadbeef
and MDIO reads return 0xbeef, warm and cold. Likely cause (from the factory LAWs and the diag device tree, **not
tested**): the diag kernel expects PCIe4 memory at CPU 0xd8000000 as the factory U-Boot maps it, ours opens that
window elsewhere, so the BAR lands on DDR (which reads 0xdeadbeef): [boot-chain-diff.md](boot-chain-diff.md),
with a dtb-patch recipe should the vendor shell ever be wanted. That document also shows the factory boot chain
does no PHY/CPLD/GPIO step that ours skips.

## Next
1. Done 2026-11-22: PHY init at boot + SONiC ([sonic.md](sonic.md)). Load at rate and soak are not.
2. Done 2026-11-22: factory taps, PHY-side counters, 26 min load ([load-test.md](load-test.md)). Longer soak is not.
3. MACsec on: mode 2 first, then keys through `bcm_plp_secy_*`.

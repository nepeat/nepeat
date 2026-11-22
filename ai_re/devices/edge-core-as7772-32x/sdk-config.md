# AS7772-32X Broadcom SDK config

State 2026-11-21: generator written and checked against the factory files **offline only**; nothing here has run
on the switch, and no port can pass traffic until the PHYs are initialised ([extphy.md](extphy.md)).

- Generator: `nix/modules/boards/as7772/sdk-config.nix` (board data: one tx/rx lane map per cage) on
  `openbcm.configs.tomahawk`; `tomahawkBase` in `nix/packages/openbcm/configs/default.nix` makes the base from
  board values instead of the public AS7712 file's. Wired as `services.sonic.sai.sdkConfig.generator`
  (`mkForce` over the AS7760's); SONiC stays off.
- Output, 32x100G (sha256 b39dfc489607…): `/nix/store/81ifmhl23x8ywhhgrj8las8yybhw593m-as7772-32x-config.bcm`
  (`nixos-build/result-sdk-config-32x100G`). The as7772 system with SONiC forced on generates the same bytes at
  `/nix/store/p0g8vb2s11i3k5239ih23g68q6n3n642-as7772-32x-config.bcm`. 128x25G: `nixos-build/result-sdk-config-128x25G`.
- Compare tool: [sdk-config-compare.py](sdk-config-compare.py) `[--full] A B` / `--dump A`: compares what the SDK
  reads (by physical core and lane, defaults applied), so files with other logical numbers compare key by key.
- Tests: `nix/modules/boards/as7772/sdk-config-test.nix` (command in its header), log
  `nixos-build/sdk-config-tests-20261121.txt`. Neutrality: `nixos-build/sdk-config-drvpaths-20261121.txt`.

Factory files (`dumps/x/diag/root/usr/local/bin/`, sha256 prefix): config.bcm-100G 7a3dda3be23e, config.bcm-25G
f2399892171e, config.bcm-MFG ce3cc7429ad2, preem-25G.soc 48d9149e02d0, preem-10G.soc 3b9f9405ccad,
ES7632BTx.hex a8636dec35bf, port_map-100G.csv 10c61ba25bca. AS7760 factory: `dumps/x/diag7760/root/usr/bin/`
config.bcm-100G 1aee6b18791f, config.bcm-25G 689ff3fd08c2.

## AS7772 factory vs AS7760 factory

Same in both the 100G and the 25G pair (tool output, "same"): every portmap (physical port, speed, logical
number), management ports 66=129:10 and 100=131:10, both pbmp keys, the whole dport map, and the four no-op
portmaps on fixed logical ports (33, 67, 101, 135). config.bcm-MFG equals config.bcm-100G except comments
(MACsec is switched on by `extphy init all mode=2` in rc.soc-MFG, not in the config).
The AS7772 100G and 25G files give the same lane maps.

| what | AS7760 | AS7772 |
| --- | --- | --- |
| lane maps | cage-side layout | **27 of 32 cores differ** (36 of 64 keys), table below |
| polarity flips | 13 keys on 10 cores | **none** in any of the three files |
| ASIC TX taps | none in config.bcm; `100GTXpreemphasis.soc` sets pre/main/post on each ASIC lane (`phy ceN CL93N72_...`) | none in config.bcm, **none by script**: `preem-25G.soc` / `preem-10G.soc` are 128 x `extphy pre <PHY id> <lane> ...` only, i.e. taps of the PHY |
| MDIO keys | none | `rate_ext_mdio_divisor=53`, `rate_ext_mdio_dividend=1` ("MDC clock rate = 8 MHz" in the file; SDK 6.5.27 `tomahawk.c` defaults to about 2 MHz) |
| other PHY keys | none | none: no `port_phy_addr`, `phy_ext_*`, `phy_chain_*`, `phy_xaui_*`, `phy_*_lane_map`. The PHYs are not SDK PHYs to the factory software; its own `extphy` command (PHY library in `bcm.user`) drives them |
| other globals | `sim_clear_all_mem=1`, `ctr_evict_enable=0` (100G file only) | both commented out |
| port map CSV | `phy_addr` 0 on every row | `phy_addr` = the cage's PHY id (table below), 0xfff on the two management ports |
| rc.soc-100G | pre-emphasis script, `portled` | adds `port ce an=off if=CAUI`, `extphy init all`, `extphy cfg <id> nphy=4 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1`, then `preem-25G`, `portled-default` |
| rc.soc-25G | pre-emphasis script, `portled` | adds `port xe0-xe127 an=off if=CR`, `extphy init all mode=1`, per lane `extphy cfg ... speed=25000 if_line=CR if_sys=CR lane=N fec=1 breakout=1` |
| MAC | - | `mac_ipg_16.soc` / `mac_ipg_48.soc`: `CLMAC_TX_CTRL AVERAGE_IPG` = 16 (bypass) or 48 (MACsec on), run by the loopback test, not by rc.soc |
| LED | `portled.soc`: 32 `CMIC_LEDUP0/1_PORT_ORDER_REMAP` writes + a 240-byte program inline, the same on both processors | `portled-default.soc`: `led 0/1 load ES7632BTx.hex` (124 bytes, another program), **no port-order remap anywhere in the diag tree**; `LED_{100G,40G,25G,10G,off}.hex` (48 bytes each) are the forced-colour test programs |

Lane maps (SDK notation, 0x3210 = straight; an absent key is 0x3210). PHY id = `phy_addr` of port_map-100G.csv:

| cage | lanes | PHY id | AS7772 tx / rx | AS7760 tx / rx | AS7760 polarity tx / rx |
| --- | --- | --- | --- | --- | --- |
| 1 | 49-52 | 0x0 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 2 | 53-56 | 0x1 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 3 | 57-60 | 0x2 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 4 | 61-64 | 0x3 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 5 | 65-68 | 0x24 | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 6 | 69-72 | 0x25 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 7 | 73-76 | 0x26 | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 8 | 77-80 | 0x27 | 0x0123 / 0x0123 | same | - |
| 9 | 33-36 | 0x48 | 0x0123 / 0x0123 | 0x3210 / 0x3210 | - |
| 10 | 37-40 | 0x49 | 0x3210 / 0x3210 | same | - |
| 11 | 41-44 | 0x4a | 0x0123 / 0x0123 | 0x3210 / 0x3210 | - |
| 12 | 45-48 | 0x4b | 0x3210 / 0x3210 | same | - |
| 13 | 81-84 | 0x6c | 0x3210 / 0x2301 | 0x1032 / 0x1032 | 0x6 / 0xf |
| 14 | 85-88 | 0x6d | 0x0123 / 0x1203 | 0x0123 / 0x0123 | 0x0 / 0xf |
| 15 | 89-92 | 0x6e | 0x3210 / 0x2301 | 0x1032 / 0x1032 | 0xe / 0xf |
| 16 | 93-96 | 0x6f | 0x0123 / 0x0123 | 0x3210 / 0x3210 | 0x1 / 0x0 |
| 17 | 97-100 | 0x110 | 0x3210 / 0x2301 | 0x1032 / 0x1032 | 0x1 / 0x0 |
| 18 | 101-104 | 0x111 | 0x0123 / 0x1032 | 0x0123 / 0x0123 | 0x0 / 0xf |
| 19 | 105-108 | 0x112 | 0x3210 / 0x2301 | 0x1032 / 0x1032 | 0xe / 0x0 |
| 20 | 109-112 | 0x113 | 0x0123 / 0x1203 | 0x0123 / 0x0123 | 0x0 / 0x2 |
| 21 | 17-20 | 0x134 | 0x0123 / 0x0123 | 0x3210 / 0x3210 | 0xf / 0x0 |
| 22 | 21-24 | 0x135 | 0x3210 / 0x3210 | same | - |
| 23 | 25-28 | 0x136 | 0x0123 / 0x0123 | 0x3210 / 0x3210 | - |
| 24 | 29-32 | 0x137 | 0x3210 / 0x3210 | same | - |
| 25 | 113-116 | 0x158 | 0x3210 / 0x2103 | 0x3210 / 0x3210 | 0xf / 0xa |
| 26 | 117-120 | 0x159 | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 27 | 121-124 | 0x15a | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 28 | 125-128 | 0x15b | 0x3210 / 0x0123 | 0x3210 / 0x3210 | - |
| 29 | 1-4 | 0x17c | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 30 | 5-8 | 0x17d | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 31 | 9-12 | 0x17e | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |
| 32 | 13-16 | 0x17f | 0x0123 / 0x3210 | 0x0123 / 0x0123 | - |

The `extphy pre` arguments after the lane: 25G file 2, 40-60, 15-31, 4, 0, 8; 10G file 0, 42-72, 9-26, 0, 0, 8
(decimal ranges over the 128 lines). Their meaning (pre / main / post / ...) is **a guess**, not decoded.

Not done: the two LED programs are not disassembled, so how the AS7772 program orders ports (it has no remap
registers to lean on) is unknown.

## Generated 32x100G vs factory config.bcm-100G

`sdk-config-compare.py --full dumps/x/diag/root/usr/local/bin/config.bcm-100G nixos-build/result-sdk-config-32x100G`:
ports, management ports, lane maps (all 64), polarity, per-lane TX keys, both pbmp keys (as physical ports) and
the dport map are the same. Remaining differences, each the AS7760 generator's existing decision unless noted:

| key | factory | generated | decision |
| --- | --- | --- | --- |
| logical port numbers | compact (1-8, 34-41, 68-75, 102-109) | physical + pipe offset (28 of 34 differ) | keep ours: valid Tomahawk numbers, a breakout never renumbers. The factory 25G file uses exactly ours. **Consequence:** the factory port_map CSV / `extphy` scripts name ports by the factory's 100G numbers, so they do not carry over as they are |
| `portmap_33/67/101/135` | 132:10, 133:10, 134:10, 135:10 | absent | drop: fixed loopback ports, SDK 6.5.27 ignores the lines |
| `dport_map_direct` | 1 | 0 | keep ours: diag-shell naming only (what the AS7760 runs) |
| `rate_ext_mdio_divisor` / `_dividend` | 53 / 1 | 53 / 1 | **AS7772-specific, kept as the factory has it.** Untested by us beyond the 2026-11-21 MDIO scan (which ran with the factory file and got no ID). If MDIO reads stay unreliable, removing both (`sdkConfig.settings.<key> = null`) gives the SDK's 2 MHz |
| `ccm_dma_enable`, `ccmdma_intr_enable`, `mem_cache_enable` | 0 | 0 | same (set by the board file, as on the AS7760) |
| `arl_clean_timeout_usec`, `asf_mem_profile`, `bcm_num_cos`, `bcm_stat_flags`, `bcm_stat_jumbo`, `cdma_timeout_usec`, `dma_desc_timeout_usec`, `ipv6_lpm_128b_enable`, `lpm_scaling_enable`, `max_vp_lags`, `oversubscribe_mode` | absent | public AS7712 values | keep: the SONiC globals the AS7760 runs with. Never run on this board |
| `miim_intr_enable` | absent | 0 | keep (public AS7712, runs on the AS7760). **PHY-adjacent:** MDIO completion is polled instead of interrupt-driven; the AS7760 has no external MDIO use to prove it there |
| `l3_alpm_enable` | absent | absent | same (the public file's 2 is removed, see the AS7760 file) |
| `serdes_preemphasis_*`, `serdes_driver_current_*`, `phy_xaui_*_polarity_flip_*` | absent | absent | same. Unlike the AS7760, the public AS7712 taps are **not** carried: they are tuned for ASIC-to-cage traces |

128x25G (`result-sdk-config-128x25G` vs config.bcm-25G): ports, numbering, lane maps, dport map the same; the
globals differ as above; the factory puts the management ports and the unused bits 33/67/101 into
`pbmp_oversubscribe` / `pbmp_xport_xe`, ours has the 128 ports (+ management in `pbmp_xport_xe`), as on the AS7760.

## Not covered by a config.bcm (open, for whoever brings up the ports)

- **PHY init, mode and line-side taps**: everything `extphy` does. No code of ours.
- **ASIC port settings the factory rc applies**: autoneg off, interface CAUI (100G) / CR (25G). The factory enables
  FEC on the PHY (`fec=1`) and sets none on the ASIC port; whether the ASIC side must run without FEC is
  **not established**. sai-openbcm's defaults were chosen for the AS7760's direct-attach cages.
- **IPG 16** on the switch MAC (the vendor's workaround against frame corruption with this PHY).
- **LED**: with SONiC on, `../as7760/sonic-platform.nix` would load the public AS7712 LED program and port-order
  remap (`hardware.as7760.ledProgram`), which is the AS7760's, not this board's. Set it to `null` or to a
  converted `ES7632BTx.hex` before enabling SONiC here. With PHYs in the path the ASIC's link state is the
  ASIC-to-PHY link, so link LEDs may not follow the cable either way (inference).
- The cage order (`services.sonic.cages`, the AS7712 map) rests on the factory dport map and CSV `slot_id`
  being equal to the AS7760's; it has not been checked with a cable on this board.
